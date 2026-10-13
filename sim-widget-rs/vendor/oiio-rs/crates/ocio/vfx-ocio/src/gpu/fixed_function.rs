//! FixedFunction for every shading language: OCIO `FixedFunctionOpGPU.cpp` over [`Dialect`]
//! (ROADMAP G6).
//!
//! Each style is the reference's `Add_<style>_Shader`, statement for statement. The PQ and log
//! curves, the ACES 1.3 gamut compression and the HSY conversions have their own modules
//! (`ff_curves`, `gamut_comp`, `hsy`); the ACES 2.0 styles compile to `GpuOp::Aces2*` ops. The
//! paired styles (RGB/HSV, XYZ/xyY, ...) arrive with their direction resolved into the style
//! (`processor::resolve_fixed_function_direction`), so `forward` matters only for the others.
//!
//! OCIO's `mix(a, b, float(cond))` selections are spelled as selections here; with a weight of
//! exactly 0 or 1 they are the same value.

use super::dialect::{Dialect, Ty, nf};
use crate::transform::FixedFunctionStyle;
use vfx_core::consts::aces1::{glow_03, glow_10, red_mod_03, red_mod_10};

/// Statement lines of one op body.
struct B<'a> {
    d: &'a Dialect,
    out: Vec<String>,
    depth: usize,
}

impl B<'_> {
    fn l(&mut self, s: impl AsRef<str>) {
        let pad = "    ".repeat(self.depth);
        self.out.push(format!("{pad}{}", s.as_ref()));
    }
    fn open(&mut self, head: &str) {
        self.l(format!("{head}{{"));
        self.depth += 1;
    }
    fn close(&mut self) {
        self.depth -= 1;
        self.l("}");
    }
    fn reopen(&mut self, head: &str) {
        self.depth -= 1;
        self.l(format!("}} {head}{{"));
        self.depth += 1;
    }
    fn f(&mut self, name: &str, expr: &str) {
        let s = self.d.let_(Ty::F, name, expr);
        self.l(s);
    }
    fn fv(&mut self, name: &str, expr: &str) {
        let s = self.d.var_(Ty::F, name, expr);
        self.l(s);
    }
    fn typed(&mut self, ty: Ty, var: bool, name: &str, expr: &str) {
        let s = if var {
            self.d.var_(ty, name, expr)
        } else {
            self.d.let_(ty, name, expr)
        };
        self.l(s);
    }
    fn sel(&mut self, dst: &str, cond: &str, t: &str, f: &str) {
        let s = format!("{dst} = {};", self.d.sel(cond, t, f));
        self.l(s);
    }
    fn set_rgb(&mut self, expr: &str) {
        let s = self.d.set(expr).trim_start().to_string();
        self.l(s);
    }
    fn v3(&self, a: &str, b: &str, c: &str) -> String {
        format!("{}({a}, {b}, {c})", self.d.v3)
    }
    fn v4(&self, v: [&str; 4]) -> String {
        format!(
            "{}({}, {}, {}, {})",
            self.d.ty(Ty::V4),
            v[0],
            v[1],
            v[2],
            v[3]
        )
    }
}

/// OCIO `Add_hue_weight_shader`: `f_H`, the cubic B-spline hue weight centred on red.
///
/// The knot index stays a float (`floor` of the clamped coordinate is the reference's
/// `int(min(knot_coord, 3.))` for the non-negative coordinate): GLSL 1.20 and ES 1.00 have no
/// implicit float-to-int conversion.
fn hue_weight(b: &mut B, inv_width: f32) {
    b.f("a", "2.0 * color.r - (color.g + color.b)");
    b.f("b", "1.7320508075688772 * (color.g - color.b)");
    let hue = b.d.atan2("b", "a");
    b.f("hue", &hue);
    b.f(
        "knot_coord",
        &format!("clamp(2.0 + hue * {}, 0.0, 4.0)", nf(inv_width)),
    );
    b.f("j", "floor(min(knot_coord, 3.0))");
    b.f("t", "knot_coord - j");
    let monomials = b.v4(["t * t * t", "t * t", "t", "1.0"]);
    b.typed(Ty::V4, false, "monomials", &monomials);
    let m = [
        ["0.25", "0.0", "0.0", "0.0"],
        ["-0.75", "0.75", "0.75", "0.25"],
        ["0.75", "-1.5", "0.0", "1.0"],
        ["-0.25", "0.75", "-0.75", "0.25"],
    ];
    for (i, row) in m.iter().enumerate() {
        let v = b.v4(*row);
        b.typed(Ty::V4, false, &format!("m{i}"), &v);
    }
    let c = b.d.sel("j == 1.0", "m1", "m0");
    b.typed(Ty::V4, true, "coefs", &c);
    b.sel("coefs", "j == 2.0", "m2", "coefs");
    b.sel("coefs", "j == 3.0", "m3", "coefs");
    b.f("f_H", "dot(coefs, monomials)");
}

/// `maxval` / `minval` of the pixel.
fn max_min(b: &mut B) {
    b.f("maxval", "max(color.r, max(color.g, color.b))");
    b.f("minval", "min(color.r, min(color.g, color.b))");
}

/// The red-channel solve of the inverse red modifiers (`ka`/`kb`/`kc`).
fn red_mod_inverse_solve(b: &mut B, pivot: f32, one_minus_scale: f32) {
    let (p, s) = (nf(pivot), nf(one_minus_scale));
    b.f("ka", &format!("f_H * {s} - 1.0"));
    b.f("kb", &format!("color.r - f_H * ({p} + minval) * {s}"));
    b.f("kc", &format!("f_H * {p} * minval * {s}"));
    b.l("color.r = (-kb - sqrt(kb * kb - 4.0 * ka * kc)) / (2.0 * ka);");
}

/// The hue restoration of the 0.3 red modifier: the other channels keep their place between
/// `minval` and the new maximum.
fn red_mod_03_restore(b: &mut B) {
    b.f("maxval2", "max(color.r, max(color.g, color.b))");
    b.f("newChroma", "maxval2 - minval");
    b.set_rgb("minval + delta * newChroma / oldChroma");
}

/// OCIO `Add_RedMod_03_Fwd_Shader` / `_Inv_` and `Add_RedMod_10_Fwd_Shader` / `_Inv_`.
fn red_mod(b: &mut B, v03: bool, forward: bool) {
    let (s, p, w) = if v03 {
        (
            red_mod_03::ONE_MINUS_SCALE,
            red_mod_03::PIVOT,
            red_mod_03::INV_WIDTH,
        )
    } else {
        (
            red_mod_10::ONE_MINUS_SCALE,
            red_mod_10::PIVOT,
            red_mod_10::INV_WIDTH,
        )
    };
    hue_weight(b, w);
    if forward {
        max_min(b);
        if v03 {
            b.f("oldChroma", "max(1e-10, maxval - minval)");
            b.typed(Ty::V3, false, "delta", "color.rgb - minval");
        }
        b.f(
            "f_S",
            "(max(1e-10, maxval) - max(1e-10, minval)) / max(1e-2, maxval)",
        );
        b.l(format!(
            "color.r = color.r + f_H * f_S * ({} - color.r) * {};",
            nf(p),
            nf(s)
        ));
        if v03 {
            red_mod_03_restore(b);
        }
    } else {
        // If f_H == 0 the solve still moves a negative red, hence the test.
        b.open("if (f_H > 0.0) ");
        if v03 {
            max_min(b);
            b.f("oldChroma", "max(1e-10, maxval - minval)");
            b.typed(Ty::V3, false, "delta", "color.rgb - minval");
        } else {
            b.f("minval", "min(color.g, color.b)");
        }
        red_mod_inverse_solve(b, p, s);
        if v03 {
            red_mod_03_restore(b);
        }
        b.close();
    }
}

/// OCIO `Add_Glow_03_Fwd_Shader` / `Add_Glow_03_Inv_Shader` (the 1.0 glow runs the same with its
/// own gain and mid point).
fn glow(b: &mut B, gain: f32, mid: f32, forward: bool) {
    b.f("chroma", "sqrt(color.b * (color.b - color.g) + color.g * (color.g - color.r) + color.r * (color.r - color.b))");
    b.f("YC", "(color.b + color.g + color.r + 1.75 * chroma) / 3.0");
    max_min(b);
    b.f(
        "sat",
        "(max(1e-10, maxval) - max(1e-10, minval)) / max(1e-2, maxval)",
    );
    b.f("x", "(sat - 0.4) * 5.0");
    b.f("t", "max(0.0, 1.0 - 0.5 * abs(x))");
    b.f("s", "0.5 * (1.0 + sign(x) * (1.0 - t * t))");
    b.f("GlowGain", &format!("{} * s", nf(gain)));
    b.f("GlowMid", &nf(mid));
    let first = if forward {
        b.d.sel(
            "YC > GlowMid * 2.0 / 3.0",
            "GlowGain * (GlowMid / YC - 0.5)",
            "GlowGain",
        )
    } else {
        b.d.sel(
            "YC > (1.0 + GlowGain) * GlowMid * 2.0 / 3.0",
            "GlowGain * (GlowMid / YC - 0.5) / (GlowGain * 0.5 - 1.0)",
            "-GlowGain / (1.0 + GlowGain)",
        )
    };
    b.fv("glowGainOut", &first);
    b.sel("glowGainOut", "YC > GlowMid * 2.0", "0.0", "glowGainOut");
    b.set_rgb("color.rgb * glowGainOut + color.rgb");
}

/// OCIO `Add_Surround_10_Fwd_Shader` (the inverse runs it with the inverse gamma).
fn dark_to_dim(b: &mut B, gamma: f32) {
    b.f("Y", "max(1e-10, 0.27222871678091454 * color.r + 0.67408176581114831 * color.g + 0.053689517407937051 * color.b)");
    b.f("Ypow_over_Y", &format!("pow(Y, {})", nf(gamma - 1.0)));
    b.set_rgb("color.rgb * Ypow_over_Y");
}

/// OCIO `Add_Rec2100_Surround_Shader`.
fn rec2100_surround(b: &mut B, gamma: f32, forward: bool) {
    let (gamma, min_lum) = if forward {
        (gamma, 1e-4_f32)
    } else {
        (1.0 / gamma, 1e-4_f32.powf(gamma))
    };
    b.fv(
        "Y",
        "0.2627 * color.r + 0.6780 * color.g + 0.0593 * color.b",
    );
    b.l(format!("Y = max({}, abs(Y));", nf(min_lum)));
    b.f("Ypow_over_Y", &format!("pow(Y, {})", nf(gamma - 1.0)));
    b.set_rgb("color.rgb * Ypow_over_Y");
}

/// OCIO `Add_RGB_TO_HSV`.
fn rgb_to_hsv(b: &mut B) {
    b.f("minRGB", "min(color.r, min(color.g, color.b))");
    b.f("maxRGB", "max(color.r, max(color.g, color.b))");
    b.fv("val", "maxRGB");
    b.fv("sat", "0.0");
    b.fv("hue", "0.0");
    b.open("if (minRGB != maxRGB) ");
    b.open("if (val != 0.0) ");
    b.l("sat = (maxRGB - minRGB) / val;");
    b.close();
    b.f("OneOverMaxMinusMin", "1.0 / (maxRGB - minRGB)");
    b.open("if (maxRGB == color.r) ");
    b.l("hue = (color.g - color.b) * OneOverMaxMinusMin;");
    b.reopen("else if (maxRGB == color.g) ");
    b.l("hue = 2.0 + (color.b - color.r) * OneOverMaxMinusMin;");
    b.reopen("else ");
    b.l("hue = 4.0 + (color.r - color.g) * OneOverMaxMinusMin;");
    b.close();
    b.open("if (hue < 0.0) ");
    b.l("hue = hue + 6.0;");
    b.close();
    b.close();
    b.open("if (minRGB < 0.0) ");
    b.l("val = val + minRGB;");
    b.close();
    b.open("if (-minRGB > maxRGB) ");
    b.l("sat = (maxRGB - minRGB) / -minRGB;");
    b.close();
    let v = b.v3("hue * (1.0 / 6.0)", "sat", "val");
    b.set_rgb(&v);
}

/// OCIO `Add_HSV_TO_RGB`.
fn hsv_to_rgb(b: &mut B) {
    b.f("Hue", "(color.r - floor(color.r)) * 6.0");
    b.f("Sat", "clamp(color.g, 0.0, 1.999)");
    b.f("Val", "color.b");
    b.f("R", "abs(Hue - 3.0) - 1.0");
    b.f("G", "2.0 - abs(Hue - 2.0)");
    b.f("B", "2.0 - abs(Hue - 4.0)");
    let rgb = format!(
        "clamp({}, {}, {})",
        b.v3("R", "G", "B"),
        b.d.k(0.0),
        b.d.k(1.0)
    );
    b.typed(Ty::V3, false, "RGB", &rgb);
    b.fv("rgbMax", "Val");
    b.fv("rgbMin", "Val * (1.0 - Sat)");
    b.open("if (Sat > 1.0) ");
    b.l("rgbMin = Val * (1.0 - Sat) / (2.0 - Sat);");
    b.l("rgbMax = Val - rgbMin;");
    b.close();
    b.open("if (Val < 0.0) ");
    b.l("rgbMin = Val / (2.0 - Sat);");
    b.l("rgbMax = Val - rgbMin;");
    b.close();
    b.set_rgb("RGB * (rgbMax - rgbMin) + rgbMin");
}

/// OCIO `Add_XYZ_TO_xyY` (`uv`: `Add_XYZ_TO_uvY`).
fn xyz_to_xyy(b: &mut B, uv: bool) {
    let dd = if uv {
        "color.r + 15.0 * color.g + 3.0 * color.b"
    } else {
        "color.r + color.g + color.b"
    };
    b.f("d0", dd);
    let d = b.d.sel("d0 == 0.0", "0.0", "1.0 / d0");
    b.f("d", &d);
    b.l("color.b = color.g;");
    if uv {
        b.l("color.r = color.r * 4.0 * d;");
        b.l("color.g = color.g * 9.0 * d;");
    } else {
        b.l("color.r = color.r * d;");
        b.l("color.g = color.g * d;");
    }
}

/// OCIO `Add_xyY_TO_XYZ` (`uv`: `Add_uvY_TO_XYZ`).
fn xyy_to_xyz(b: &mut B, uv: bool) {
    let d = b.d.sel("color.g == 0.0", "0.0", "1.0 / color.g");
    b.f("d", &d);
    b.f("Y", "color.b");
    if uv {
        b.l("color.b = (3.0 / 4.0) * Y * (4.0 - color.r - 6.6666666666666667 * color.g) * d;");
        b.l("color.r = color.r * (9.0 / 4.0) * Y * d;");
    } else {
        b.l("color.b = Y * (1.0 - color.r - color.g) * d;");
        b.l("color.r = color.r * Y * d;");
    }
    b.l("color.g = Y;");
}

/// OCIO `Add_XYZ_TO_LUV`.
fn xyz_to_luv(b: &mut B) {
    b.f("d0", "color.r + 15.0 * color.g + 3.0 * color.b");
    let d = b.d.sel("d0 == 0.0", "0.0", "1.0 / d0");
    b.f("d", &d);
    b.f("u", "color.r * 4.0 * d");
    b.f("v", "color.g * 9.0 * d");
    b.f("Y", "color.g");
    let l = b.d.sel(
        "Y <= 0.008856451679",
        "9.0329629629629608 * Y",
        "1.16 * pow(max(0.0, Y), 1.0 / 3.0) - 0.16",
    );
    b.f("Lstar", &l);
    b.f("ustar", "13.0 * Lstar * (u - 0.19783001)");
    b.f("vstar", "13.0 * Lstar * (v - 0.46831999)");
    let v = b.v3("Lstar", "ustar", "vstar");
    b.set_rgb(&v);
}

/// OCIO `Add_LUV_TO_XYZ`.
fn luv_to_xyz(b: &mut B) {
    b.f("Lstar", "color.r");
    let d =
        b.d.sel("Lstar == 0.0", "0.0", "0.076923076923076927 / Lstar");
    b.f("d", &d);
    b.f("u", "color.g * d + 0.19783001");
    b.f("v", "color.b * d + 0.46831999");
    b.f("tmp", "(Lstar + 0.16) * 0.86206896551724144");
    let y = b.d.sel(
        "Lstar <= 0.08",
        "0.11070564598794539 * Lstar",
        "tmp * tmp * tmp",
    );
    b.f("Y", &y);
    let dd = b.d.sel("v == 0.0", "0.0", "0.25 / v");
    b.f("dd", &dd);
    b.l("color.r = 9.0 * Y * u * dd;");
    b.l("color.b = Y * (12.0 - 3.0 * u - 20.0 * v) * dd;");
    b.l("color.g = Y;");
}

/// OCIO `GetFixedFunctionGPUProcessingText`: the body of `style` in its own scope.
pub(super) fn emit(
    d: &Dialect,
    style: FixedFunctionStyle,
    params: &[f32],
    forward: bool,
) -> Vec<String> {
    use FixedFunctionStyle as S;
    let mut b = B {
        d,
        out: Vec::new(),
        depth: 0,
    };
    match style {
        S::LinToPq
        | S::PqToLin
        | S::LinToGammaLog
        | S::GammaLogToLin
        | S::LinToDoubleLog
        | S::DoubleLogToLin => {
            return super::ff_curves::emit(style, params, forward, d).expect("a curve style");
        }
        S::AcesGamutComp13 => return super::gamut_comp::emit(params, forward, d),
        S::RgbToHsyLin
        | S::RgbToHsyLog
        | S::RgbToHsyVid
        | S::HsyLinToRgb
        | S::HsyLogToRgb
        | S::HsyVidToRgb => {
            return super::hsy::emit(style, d).expect("an HSY style");
        }
        S::AcesOutputTransform20
        | S::AcesRgbToJmh20
        | S::AcesJmhToRgb20
        | S::AcesTonescaleCompress20
        | S::AcesGamutCompress20 => unreachable!("ACES 2.0 compiles to GpuOp::Aces2* (convert_op)"),
        _ => {}
    }
    b.l(format!(
        "{{ // FixedFunction {style:?}{}",
        if forward { "" } else { " inverse" }
    ));
    b.depth += 1;
    match style {
        S::AcesRedMod03 => red_mod(&mut b, true, forward),
        S::AcesRedMod10 => red_mod(&mut b, false, forward),
        S::AcesGlow03 => glow(&mut b, glow_03::GLOW_GAIN, glow_03::GLOW_MID, forward),
        S::AcesGlow10 => glow(&mut b, glow_10::GLOW_GAIN, glow_10::GLOW_MID, forward),
        S::AcesDarkToDim10 => {
            // The inverse runs the forward shader with the inverse gamma (OCIO).
            let gamma = params.first().copied().unwrap_or(0.9811);
            dark_to_dim(&mut b, if forward { gamma } else { 1.0 / gamma });
        }
        S::Rec2100Surround => {
            rec2100_surround(&mut b, params.first().copied().unwrap_or(0.78), forward)
        }
        S::RgbToHsv => rgb_to_hsv(&mut b),
        S::HsvToRgb => hsv_to_rgb(&mut b),
        S::XyzToXyy => xyz_to_xyy(&mut b, false),
        S::XyyToXyz => xyy_to_xyz(&mut b, false),
        S::XyzToUvy => xyz_to_xyy(&mut b, true),
        S::UvyToXyz => xyy_to_xyz(&mut b, true),
        S::XyzToLuv => xyz_to_luv(&mut b),
        S::LuvToXyz => luv_to_xyz(&mut b),
        _ => unreachable!("handled above"),
    }
    b.close();
    b.out
}
