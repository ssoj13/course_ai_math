//! GradingTone for every shading language: OCIO `GradingToneOpGPU.cpp` over [`Dialect`]
//! (ROADMAP G6).
//!
//! Every function below is the one of the same name in the reference (`Add_MidsFwd_Shader`,
//! `Add_HighlightShadowRev_Shader`, ...), statement for statement; only the spellings come from the
//! dialect (declarations, `?:` as WGSL `select`, the RGB write-back). OCIO's `const` locals are
//! plain bindings here: GLSL ES 1.00 rejects a `const` whose initializer reads a uniform, and the
//! values are the same.

use super::dialect::{Dialect, Ty, nf};
use super::grading::{lin_to_log, log_to_lin};
use super::types::{GradingToneUniforms, OpUniforms};
use crate::grading_tone::GradingTonePreRender;
use crate::transform::GradingToneStyle;

/// The four channel passes of a control: red, green, blue, then master on all three.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ch {
    R,
    G,
    B,
    M,
}

impl Ch {
    /// The `(red, green, blue, master)` slot of a control.
    fn slot(self) -> usize {
        match self {
            Ch::R => 0,
            Ch::G => 1,
            Ch::B => 2,
            Ch::M => 3,
        }
    }

    /// The pixel expression the pass reads and writes (`color.r`, ..., `color.rgb`).
    fn pix(self) -> &'static str {
        match self {
            Ch::R => "color.r",
            Ch::G => "color.g",
            Ch::B => "color.b",
            Ch::M => "color.rgb",
        }
    }
}

/// One control's six values as shader expressions: `(r, g, b, m, start, width)`.
type Control = [String; 6];

/// OCIO `GTProperties`: the shader expression of every value, a uniform or (without uniforms, as
/// OCIO's non-dynamic path) a literal.
struct Props {
    blacks: Control,
    shadows: Control,
    midtones: Control,
    highlights: Control,
    whites: Control,
    scontrast: String,
}

/// A statement writer with OCIO's `GpuShaderText` indentation.
struct W<'a> {
    d: &'a Dialect,
    out: Vec<String>,
    depth: usize,
}

impl W<'_> {
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
    /// `} else {` / `} else if (c) {`.
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
    fn fu(&mut self, name: &str) {
        let s = self.d.var_uninit(Ty::F, name);
        self.l(s);
    }
    fn v(&mut self, name: &str, expr: &str) {
        let s = self.d.let_(Ty::V3, name, expr);
        self.l(s);
    }
    fn vv(&mut self, name: &str, expr: &str) {
        let s = self.d.var_(Ty::V3, name, expr);
        self.l(s);
    }
    fn vu(&mut self, name: &str) {
        let s = self.d.var_uninit(Ty::V3, name);
        self.l(s);
    }
    /// `dst = cond ? t : f;`.
    fn sel(&mut self, dst: &str, cond: &str, t: &str, f: &str) {
        let s = format!("{dst} = {};", self.d.sel(cond, t, f));
        self.l(s);
    }
    /// The same selection on each of `.r`, `.g`, `.b`: `c`, `t` and `f` hold `{}` for the component.
    fn sel3(&mut self, dst: &str, cond: &str, t: &str, f: &str) {
        for c in ["r", "g", "b"] {
            let sub = |s: &str| s.replace("{}", c);
            self.sel(&format!("{dst}.{c}"), &sub(cond), &sub(t), &sub(f));
        }
    }
    /// `pix = res;` for the pass's channel(s).
    fn write(&mut self, ch: Ch, res: &str) {
        if ch == Ch::M {
            let s = self.d.set(res).trim_start().to_string();
            self.l(s);
        } else {
            self.l(format!("{} = {res};", ch.pix()));
        }
    }
    /// `t` for the pass: a scalar, or the RGB for master; mutable when `var`.
    fn t(&mut self, ch: Ch, var: bool) {
        match (ch == Ch::M, var) {
            (true, true) => self.vv("t", "color.rgb"),
            (true, false) => self.v("t", "color.rgb"),
            (false, true) => self.fv("t", ch.pix()),
            (false, false) => self.f("t", ch.pix()),
        }
    }
    fn lines(&mut self, lines: Vec<String>) {
        for s in lines {
            self.l(s);
        }
    }
}

/// OCIO `Add_MidsPre_Shader`: opens the local scope and the `if (mid_adj != 1.)` block.
fn mids_pre(w: &mut W, ch: Ch, p: &Props, style: GradingToneStyle) {
    let (top, _top_sc, bottom, _pivot) = GradingTonePreRender::from_style(style);
    w.open("");
    w.f("halo", "0.4");
    w.fv(
        "mid_adj",
        &format!("clamp({}, 0.01, 1.99)", p.midtones[ch.slot()]),
    );
    w.open("if (mid_adj != 1.0) ");
    w.f("x0", &nf(bottom));
    w.f("x5", &nf(top));
    w.f("max_width", "(x5 - x0) * 0.95");
    w.f(
        "width",
        &format!("clamp({}, 0.01, max_width)", p.midtones[5]),
    );
    w.f("min_cent", "x0 + width * 0.51");
    w.f("max_cent", "x5 - width * 0.51");
    w.f(
        "center",
        &format!("clamp({}, min_cent, max_cent)", p.midtones[4]),
    );
    w.f("x1", "center - width * 0.5");
    w.f("x4", "x1 + width");
    w.f("x2", "x1 + (x4 - x1) * 0.25");
    w.f("x3", "x1 + (x4 - x1) * 0.75");
    w.f("y0", "x0");
    w.f("m0", "1.0");
    w.f("m5", "1.0");
    w.f("min_slope", "0.1");
    w.l("mid_adj = mid_adj - 1.0;");
    w.l("mid_adj = mid_adj * (1.0 - min_slope);");
    w.f("m2", "1.0 + mid_adj");
    w.f("m3", "1.0 - mid_adj");
    w.fv("m1", "1.0 + mid_adj * halo");
    w.fv("m4", "1.0 - mid_adj * halo");
    w.open("if (center <= (x5 + x0) * 0.5) ");
    w.f("area", "(x1 - x0) * (m1 - m0) * 0.5 + (x2 - x1) * ((m1 - m0) + (m2 - m1) * 0.5) + (center - x2) * (m2 - m0) * 0.5");
    w.l("m4 = (-0.5 * (x5 - x4) * m5 + (x4 - x3) * (0.5 * m3 - m5) + (x3 - center) * (m3 - m5) * 0.5 + area) / (-0.5 * (x5 - x3));");
    w.reopen("else ");
    w.f("area", "(x5 - x4) * (m4 - m5) * 0.5 + (x4 - x3) * ((m4 - m5) + (m3 - m4) * 0.5) + (x3 - center) * (m3 - m5) * 0.5");
    w.l("m1 = (-0.5 * (x1 - x0) * m0 + (x2 - x1) * (0.5 * m2 - m0) + (center - x2) * (m2 - m0) * 0.5 + area) / (-0.5 * (x2 - x0));");
    w.close();
    w.f("y1", "y0 + (m0 + m1) * (x1 - x0) * 0.5");
    w.f("y2", "y1 + (m1 + m2) * (x2 - x1) * 0.5");
    w.f("y3", "y2 + (m2 + m3) * (x3 - x2) * 0.5");
    w.f("y4", "y3 + (m3 + m4) * (x4 - x3) * 0.5");
    w.f("y5", "y4 + (m4 + m5) * (x5 - x4) * 0.5");
}

/// OCIO `Add_MidsFwd_Shader`.
fn mids_fwd(w: &mut W, ch: Ch, p: &Props, style: GradingToneStyle) {
    mids_pre(w, ch, p, style);
    let segs = [
        ("tL", "x0", "x1", "m0", "m1", "y0"),
        ("tM", "x1", "x2", "m1", "m2", "y1"),
        ("tR", "x2", "x3", "m2", "m3", "y2"),
        ("tR2", "x3", "x4", "m3", "m4", "y3"),
        ("tR3", "x4", "x5", "m4", "m5", "y4"),
    ];
    let (decl, m) = if ch == Ch::M {
        (Ty::V3, true)
    } else {
        (Ty::F, false)
    };
    w.t(ch, false);
    for (tn, a, b, ..) in segs {
        let s = w.d.let_(decl, tn, &format!("(t - {a}) / ({b} - {a})"));
        w.l(s);
    }
    for (tn, a, b, ma, mb, y) in segs {
        let f = tn.replacen('t', "f", 1);
        let s = w.d.let_(
            decl,
            &f,
            &format!("{tn} * ({b} - {a}) * ({tn} * 0.5 * ({mb} - {ma}) + {ma}) + {y}"),
        );
        w.l(s);
    }
    if !m {
        w.fv("res", &w.d.sel("t < x1", "fL", "fM"));
        w.sel("res", "t > x2", "fR", "res");
        w.sel("res", "t > x3", "fR2", "res");
        w.sel("res", "t > x4", "fR3", "res");
        w.sel("res", "t < x0", "y0 + (t - x0) * m0", "res");
        w.sel("res", "t > x5", "y5 + (t - x5) * m5", "res");
    } else {
        w.vu("res");
        w.sel3("res", "t.{} < x1", "fL.{}", "fM.{}");
        w.sel3("res", "t.{} > x2", "fR.{}", "res.{}");
        w.sel3("res", "t.{} > x3", "fR2.{}", "res.{}");
        w.sel3("res", "t.{} > x4", "fR3.{}", "res.{}");
        w.sel3("res", "t.{} < x0", "y0 + (t.{} - x0) * m0", "res.{}");
        w.sel3("res", "t.{} > x5", "y5 + (t.{} - x5) * m5", "res.{}");
    }
    w.write(ch, "res");
    w.close(); // if (mid_adj != 1.)
    w.close(); // local scope
}

/// The uniform names of one quadratic segment: `(y, xa, xb, ma, mb)`, its start value, its x
/// range and the slopes at both ends.
type QuadSegment = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

/// One quadratic segment's inverse, OCIO's `c`/`b`/`a`/`discrim`/`tmp` block; `c` (and so the
/// result) is a vector for master.
fn quad_rev(w: &mut W, vec: bool, dst: &str, seg: QuadSegment) {
    let (y, xa, xb, ma, mb) = seg;
    w.open("");
    let cty = if vec { Ty::V3 } else { Ty::F };
    let s = w.d.let_(cty, "c", &format!("{y} - t"));
    w.l(s);
    w.f("b", &format!("{ma} * ({xb} - {xa})"));
    w.f("a", &format!("0.5 * ({mb} - {ma}) * ({xb} - {xa})"));
    let s = w.d.let_(cty, "discrim", "sqrt(b * b - 4.0 * a * c)");
    w.l(s);
    let s = w.d.let_(cty, "tmp", "(-2.0 * c) / (discrim + b)");
    w.l(s);
    w.l(format!("{dst} = tmp * ({xb} - {xa}) + {xa};"));
    w.close();
}

/// OCIO `Add_MidsRev_Shader`.
fn mids_rev(w: &mut W, ch: Ch, p: &Props, style: GradingToneStyle) {
    mids_pre(w, ch, p, style);
    let segs: [QuadSegment; 5] = [
        ("y4", "x4", "x5", "m4", "m5"),
        ("y3", "x3", "x4", "m3", "m4"),
        ("y2", "x2", "x3", "m2", "m3"),
        ("y1", "x1", "x2", "m1", "m2"),
        ("y0", "x0", "x1", "m0", "m1"),
    ];
    if ch != Ch::M {
        w.t(ch, false);
        w.fu("res");
        w.open("if (t >= y5) ");
        w.l("res = x5 + (t - y5) / m5;");
        for seg in segs {
            w.reopen(&format!("else if (t >= {}) ", seg.0));
            quad_rev(w, false, "res", seg);
        }
        w.reopen("else ");
        w.l("res = x0 + (t - y0) / m0;");
        w.close();
    } else {
        w.t(ch, false);
        let outs = ["outR3", "outR2", "outR", "outM", "outL"];
        for o in outs {
            w.vu(o);
        }
        // OCIO's own TODO: the vector form evaluates every segment.
        for (o, seg) in outs.iter().zip(segs) {
            quad_rev(w, true, o, seg);
        }
        w.vu("res");
        w.sel3("res", "t.{} < y1", "outL.{}", "outM.{}");
        w.sel3("res", "t.{} > y2", "outR.{}", "res.{}");
        w.sel3("res", "t.{} > y3", "outR2.{}", "res.{}");
        w.sel3("res", "t.{} > y4", "outR3.{}", "res.{}");
        w.sel3("res", "t.{} < y0", "x0 + (t.{} - y0) * m0", "res.{}");
        w.sel3("res", "t.{} > y5", "x5 + (t.{} - y5) * m5", "res.{}");
    }
    w.write(ch, "res");
    w.close(); // if (mid_adj != 1.)
    w.close(); // local scope
}

/// OCIO `Add_HighlightShadowPre_Shader`: opens the local scope.
fn highlight_shadow_pre(w: &mut W, ch: Ch, p: &Props, shadow: bool) {
    let c = if shadow { &p.shadows } else { &p.highlights };
    let (start, pivot) = (&c[4], &c[5]);
    w.open("");
    if shadow {
        w.f("x0", pivot);
        w.f("x2", start);
        w.f("m2", "1.0");
    } else {
        w.f("x0", start);
        w.f("x2", pivot);
        w.f("m0", "1.0");
    }
    w.f("y0", "x0");
    w.f("y2", "x2");
    w.f("x1", "x0 + (x2 - x0) * 0.5");
    if shadow {
        w.f("val", &c[ch.slot()]);
    } else {
        w.f("val", &format!("2.0 - {}", c[ch.slot()]));
    }
}

/// OCIO `Add_FauxCubicFwdEval_Shader`.
fn faux_cubic_fwd(w: &mut W, ch: Ch) {
    w.f("y1", "(0.5 / (x2 - x0)) * ((2.0 * y0 + m0 * (x1 - x0)) * (x2 - x1) + (2.0 * y2 - m2 * (x2 - x1)) * (x1 - x0))");
    w.t(ch, false);
    let ty = if ch == Ch::M { Ty::V3 } else { Ty::F };
    for (name, expr) in [
        ("tL", "(t - x0) / (x1 - x0)"),
        ("tR", "(t - x1) / (x2 - x1)"),
        (
            "fL",
            "y0 * (1.0 - tL * tL) + y1 * tL * tL + m0 * (1.0 - tL) * tL * (x1 - x0)",
        ),
        (
            "fR",
            "y1 * (1.0 - tR) * (1.0 - tR) + y2 * (2.0 - tR) * tR + m2 * (tR - 1.0) * tR * (x2 - x1)",
        ),
    ] {
        let s = w.d.let_(ty, name, expr);
        w.l(s);
    }
    if ch != Ch::M {
        w.fv("res", &w.d.sel("t < x1", "fL", "fR"));
        w.sel("res", "t < x0", "y0 + (t - x0) * m0", "res");
        w.sel("res", "t > x2", "y2 + (t - x2) * m2", "res");
    } else {
        w.vu("res");
        w.sel3("res", "t.{} < x1", "fL.{}", "fR.{}");
        w.sel3("res", "t.{} < x0", "y0 + (t.{} - x0) * m0", "res.{}");
        w.sel3("res", "t.{} > x2", "y2 + (t.{} - x2) * m2", "res.{}");
    }
    w.write(ch, "res");
}

/// OCIO `Add_FauxCubicRevEval_Shader`.
fn faux_cubic_rev(w: &mut W, ch: Ch) {
    w.f("y1", "(0.5 / (x2 - x0)) * ((2.0 * y0 + m0 * (x1 - x0)) * (x2 - x1) + (2.0 * y2 - m2 * (x2 - x1)) * (x1 - x0))");
    w.t(ch, false);
    let ty = if ch == Ch::M { Ty::V3 } else { Ty::F };
    let d = w.d;
    let vec = |w: &mut W, name: &str, expr: &str| {
        let s = d.let_(ty, name, expr);
        w.l(s);
    };
    vec(w, "cL", "y0 - t");
    w.f("bL", "m0 * (x1 - x0)");
    w.f("aL", "y1 - y0 - m0 * (x1 - x0)");
    vec(w, "discrimL", "sqrt(bL * bL - 4.0 * aL * cL)");
    vec(w, "outL", "(-2.0 * cL) / (discrimL + bL) * (x1 - x0) + x0");
    vec(w, "cR", "y1 - t");
    w.f("bR", "2.0 * y2 - 2.0 * y1 - m2 * (x2 - x1)");
    w.f("aR", "y1 - y2 + m2 * (x2 - x1)");
    vec(w, "discrimR", "sqrt(bR * bR - 4.0 * aR * cR)");
    vec(w, "outR", "(-2.0 * cR) / (discrimR + bR) * (x2 - x1) + x1");
    if ch != Ch::M {
        w.fv("res", &w.d.sel("t < y1", "outL", "outR"));
        w.sel("res", "t < y0", "x0 + (t - y0) / m0", "res");
        w.sel("res", "t > y2", "x2 + (t - y2) / m2", "res");
    } else {
        w.vu("res");
        w.sel3("res", "t.{} < y1", "outL.{}", "outR.{}");
        w.sel3("res", "t.{} < y0", "x0 + (t.{} - y0) / m0", "res.{}");
        w.sel3("res", "t.{} > y2", "x2 + (t.{} - y2) / m2", "res.{}");
    }
    w.write(ch, "res");
}

/// OCIO `Add_HighlightShadowFwd_Shader` (`fwd`) / `Add_HighlightShadowRev_Shader`: the two slope
/// cases run the faux cubic one way or the other.
fn highlight_shadow(w: &mut W, ch: Ch, p: &Props, shadow: bool, fwd: bool) {
    highlight_shadow_pre(w, ch, p, shadow);
    let m = if shadow { "m0" } else { "m2" };
    w.open("if (val < 1.0) ");
    w.f(m, "max(0.01, val)");
    if fwd {
        faux_cubic_fwd(w, ch)
    } else {
        faux_cubic_rev(w, ch)
    }
    w.reopen("else if (val > 1.0) ");
    w.f(m, "max(0.01, 2.0 - val)");
    if fwd {
        faux_cubic_rev(w, ch)
    } else {
        faux_cubic_fwd(w, ch)
    }
    w.close();
    w.close(); // local scope
}

/// OCIO `Add_WhiteBlackPre_Shader`: opens the local scope.
fn white_black_pre(w: &mut W, ch: Ch, p: &Props, black: bool) {
    let c = if black { &p.blacks } else { &p.whites };
    let (start, width) = (&c[4], &c[5]);
    w.open("");
    if !black {
        w.f("x0", start);
        w.f("x1", &format!("x0 + {width}"));
        w.f("m0", "1.0");
        w.f("y0", "x0");
        w.fv("m1", &c[ch.slot()]);
        w.f("mtest", "m1");
    } else {
        w.f("x1", start);
        w.f("x0", &format!("x1 - {width}"));
        w.f("m1", "1.0");
        w.f("y1", "x1");
        // Increasing the blacks control lightens.
        w.fv("m0", &format!("2.0 - {}", c[ch.slot()]));
        w.f("mtest", "m0");
    }
    w.t(ch, true);
}

/// OCIO `Add_WBFwd_Shader`.
fn wb_fwd(w: &mut W, ch: Ch, linear_extrap: bool) {
    if ch != Ch::M {
        w.f("tlocal", "(t - x0) / (x1 - x0)");
        w.fv(
            "res",
            "tlocal * (x1 - x0) * (tlocal * 0.5 * (m1 - m0) + m0) + y0",
        );
        w.sel("res", "t < x0", "y0 + (t - x0) * m0", "res");
        if linear_extrap {
            w.sel("res", "t > x1", "y1 + (t - x1) * m1", "res");
        }
    } else {
        w.v("tlocal", "(t - x0) / (x1 - x0)");
        w.vv(
            "res",
            "tlocal * (x1 - x0) * (tlocal * 0.5 * (m1 - m0) + m0) + y0",
        );
        w.sel3("res", "t.{} < x0", "y0 + (t.{} - x0) * m0", "res.{}");
        if linear_extrap {
            w.sel3("res", "t.{} > x1", "y1 + (t.{} - x1) * m1", "res.{}");
        }
    }
}

/// OCIO `Add_WBRev_Shader`.
fn wb_rev(w: &mut W, ch: Ch, linear_extrap: bool) {
    w.f("a", "0.5 * (m1 - m0) * (x1 - x0)");
    w.f("b", "m0 * (x1 - x0)");
    if ch != Ch::M {
        w.f("c", "y0 - t");
        w.f("discrim", "sqrt(b * b - 4.0 * a * c)");
        w.f("tmp", "(-2.0 * c) / (discrim + b)");
        w.fv("res", "tmp * (x1 - x0) + x0");
        w.sel("res", "t < y0", "x0 + (t - y0) / m0", "res");
        if linear_extrap {
            w.sel("res", "t > y1", "x1 + (t - y1) / m1", "res");
        }
    } else {
        w.v("c", "y0 - t");
        w.v("discrim", "sqrt(b * b - 4.0 * a * c)");
        w.v("tmp", "(-2.0 * c) / (discrim + b)");
        w.vv("res", "tmp * (x1 - x0) + x0");
        w.sel3("res", "t.{} < y0", "x0 + (t.{} - y0) / m0", "res.{}");
        if linear_extrap {
            w.sel3("res", "t.{} > y1", "x1 + (t.{} - y1) / m1", "res.{}");
        }
    }
}

/// OCIO `Add_WBExtrapPre_Shader`: the quadratic extrapolation above the whites (better HDR
/// control).
fn wb_extrap_pre(w: &mut W) {
    w.l("res = (res - x0) / gain + x0;");
    w.f("new_y1", "(x1 - x0) / gain + x0");
    w.f("xd", "x0 + (x1 - x0) * 0.99");
    w.f("md", "1.0 / (m0 + (xd - x0) * (m1 - m0) / (x1 - x0))");
    w.f("aa", "0.5 * (1.0 / m1 - md) / (x1 - xd)");
    w.f("bb", "1.0 / m1 - 2.0 * aa * x1");
    w.f("cc", "new_y1 - bb * x1 - aa * x1 * x1");
    w.l("t = (t - x0) / gain + x0;");
}

/// OCIO `Add_WhiteBlackFwd_Shader` (`fwd`) / `Add_WhiteBlackRev_Shader`.
fn white_black(w: &mut W, ch: Ch, p: &Props, black: bool, fwd: bool) {
    white_black_pre(w, ch, p, black);

    // The slope decreases.
    w.open("if (mtest < 1.0) ");
    if !black {
        w.l("m1 = max(0.01, m1);");
        w.f("y1", "y0 + (m0 + m1) * (x1 - x0) * 0.5");
    } else {
        w.l("m0 = max(0.01, m0);");
        w.f("y0", "y1 - (m0 + m1) * (x1 - x0) * 0.5");
    }
    if fwd {
        wb_fwd(w, ch, true)
    } else {
        wb_rev(w, ch, true)
    }
    w.write(ch, "res");

    // The slope increases.
    w.reopen("else if (mtest > 1.0) ");
    if !black {
        w.l("m1 = 2.0 - m1;");
        w.l("m1 = max(0.01, m1);");
        w.f("gain", "(m0 + m1) * 0.5");
        w.l("t = (t - x0) * gain + x0;");
    } else {
        w.l("m0 = 2.0 - m0;");
        w.l("m0 = max(0.01, m0);");
        w.f("y0", "y1 - (m0 + m1) * (x1 - x0) * 0.5");
        w.f("gain", "(m0 + m1) * 0.5");
        w.l("t = (t - x1) * gain + x1;");
    }
    // The reference passes `isBlack` as the linear extrapolation here: the whites extrapolate
    // quadratically below instead.
    if fwd {
        wb_rev(w, ch, black)
    } else {
        wb_fwd(w, ch, black)
    }
    if !black {
        wb_extrap_pre(w);
        if fwd {
            if ch != Ch::M {
                w.sel("res", "t > x1", "(aa * t + bb) * t + cc", "res");
            } else {
                w.sel3("res", "t.{} > x1", "(aa * t.{} + bb) * t.{} + cc", "res.{}");
            }
        } else if ch != Ch::M {
            w.f("c2", "cc - t");
            w.f("discrim2", "sqrt(bb * bb - 4.0 * aa * c2)");
            w.f("res1", "(-2.0 * c2) / (discrim2 + bb)");
            w.f("brk", "(aa * x1 + bb) * x1 + cc");
            w.sel("res", "t < brk", "res", "res1");
        } else {
            w.v("c2", "cc - t");
            w.v("discrim2", "sqrt(bb * bb - 4.0 * aa * c2)");
            w.v("res1", "(-2.0 * c2) / (discrim2 + bb)");
            w.f("brk", "(aa * x1 + bb) * x1 + cc");
            w.sel3("res", "t.{} < brk", "res.{}", "res1.{}");
        }
    } else {
        w.l("res = (res - x1) / gain + x1;");
    }
    w.write(ch, "res");
    w.close(); // else if (mtest > 1.)
    w.close(); // local scope
}

/// OCIO `Add_SContrastTopPre_Shader`: opens the `if (contrast != 1.)` block and the top end's
/// local scope.
fn scontrast_top_pre(w: &mut W, p: &Props, style: GradingToneStyle) {
    let (_top, top_sc, _bottom, pivot) = GradingTonePreRender::from_style(style);
    w.fv("contrast", &p.scontrast);
    w.open("if (contrast != 1.0) ");
    // Limit the range of values to prevent reversals.
    w.sel(
        "contrast",
        "contrast > 1.0",
        "1.0 / (1.8125 - 0.8125 * min(contrast, 1.99))",
        "0.28125 + 0.71875 * max(contrast, 0.01)",
    );
    w.f("pivot", &nf(pivot));
    w.v("t", "color.rgb");
    w.open("");
    w.f("x3", &nf(top_sc));
    w.f("y3", &nf(top_sc));
    w.f("y0", "pivot + (y3 - pivot) * 0.25");
    w.f("m0", "contrast");
    w.f("x0", "pivot + (y0 - pivot) / m0");
    w.f("min_width", "(x3 - x0) * 0.3");
    w.fv("m3", "1.0 / m0");
    // NB: m0 != m3 inside `contrast != 1`.
    w.f("center", "(y3 - y0 - m3 * x3 + m0 * x0) / (m0 - m3)");
    w.fv("x1", "x0");
    w.fv("x2", "2.0 * center - x1");
    w.open("if (x2 > x3) ");
    w.l("x2 = x3;");
    w.l("x1 = 2.0 * center - x2;");
    w.reopen("else if ((x2 - x1) < min_width) ");
    w.l("x2 = x1 + min_width;");
    w.f("new_center", "(x2 + x1) * 0.5");
    w.l("m3 = (y3 - y0 + m0 * x0 - new_center * m0) / (x3 - new_center);");
    w.close();
    w.f("y1", "y0");
    w.f("y2", "y1 + (m0 + m3) * (x2 - x1) * 0.5");
}

/// OCIO `Add_SContrastBottomPre_Shader`: opens the bottom end's local scope.
fn scontrast_bottom_pre(w: &mut W, style: GradingToneStyle) {
    let (_top, _top_sc, bottom, _pivot) = GradingTonePreRender::from_style(style);
    w.open("");
    w.f("x0", &nf(bottom));
    w.f("y0", &nf(bottom));
    w.f("y3", "pivot - (pivot - y0) * 0.25");
    w.f("m3", "contrast");
    w.f("x3", "pivot - (pivot - y3) / m3");
    w.f("min_width", "(x3 - x0) * 0.3");
    w.fv("m0", "1.0 / m3");
    w.f("center", "(y3 - y0 - m3 * x3 + m0 * x0) / (m0 - m3)");
    w.fv("x2", "x3");
    w.fv("x1", "2.0 * center - x2");
    w.open("if (x1 < x0) ");
    w.l("x1 = x0;");
    w.l("x2 = 2.0 * center - x1;");
    w.reopen("else if ((x2 - x1) < min_width) ");
    w.l("x1 = x2 - min_width;");
    w.f("new_center", "(x2 + x1) * 0.5");
    w.l("m0 = (y3 - y0 - m3 * x3 + new_center * m3) / (new_center - x0);");
    w.close();
    w.f("y2", "y3");
    w.f("y1", "y2 - (m0 + m3) * (x2 - x1) * 0.5");
}

/// OCIO `Add_SContrastFwd_Shader` (`fwd`) / `Add_SContrastRev_Shader`.
fn scontrast(w: &mut W, p: &Props, style: GradingToneStyle, fwd: bool) {
    w.open("");
    scontrast_top_pre(w, p, style);
    let set_rgb = |w: &mut W, expr: &str| {
        let s = w.d.set(expr).trim_start().to_string();
        w.l(s);
    };
    if fwd {
        set_rgb(w, "(t - pivot) * contrast + pivot");
        w.v("tR", "(t - x1) / (x2 - x1)");
        w.v("res", "tR * (x2 - x1) * (tR * 0.5 * (m3 - m0) + m0) + y1");
        w.sel3("color", "t.{} > x1", "res.{}", "color.{}");
        w.sel3("color", "t.{} > x2", "y2 + (t.{} - x2) * m3", "color.{}");
        w.close();
        scontrast_bottom_pre(w, style);
        w.v("tR", "(t - x1) / (x2 - x1)");
        w.v("res", "tR * (x2 - x1) * (tR * 0.5 * (m3 - m0) + m0) + y1");
        w.sel3("color", "t.{} < x2", "res.{}", "color.{}");
        w.sel3("color", "t.{} < x1", "y1 + (t.{} - x1) * m0", "color.{}");
        w.close();
    } else {
        set_rgb(w, "(t - pivot) / contrast + pivot");
        let rev = |w: &mut W| {
            w.v("c", "y1 - t");
            w.f("b", "m0 * (x2 - x1)");
            w.f("a", "(m3 - m0) * 0.5 * (x2 - x1)");
            w.v("discrim", "sqrt(b * b - 4.0 * a * c)");
            w.v("res", "(x2 - x1) * (-2.0 * c) / (discrim + b) + x1");
        };
        rev(w);
        w.sel3("color", "t.{} > y1", "res.{}", "color.{}");
        w.sel3("color", "t.{} > y2", "x2 + (t.{} - y2) / m3", "color.{}");
        w.close();
        scontrast_bottom_pre(w, style);
        rev(w);
        w.sel3("color", "t.{} > y2", "color.{}", "res.{}");
        w.sel3("color", "t.{} > y1", "color.{}", "x1 + (t.{} - y1) / m0");
        w.close();
    }
    w.close(); // if (contrast != 1.)
    w.close();
}

/// A control's six values: the uniforms' names read in dialect `d`, or the literals.
fn control(d: &Dialect, names: Option<[&str; 6]>, lit: &[f32; 6]) -> Control {
    std::array::from_fn(|i| match names {
        Some(n) => d.uniform(n[i]),
        None => nf(lit[i]),
    })
}

/// OCIO `GetGradingToneGPUShaderProgram`: `AddGTForwardShader` / `AddGTInverseShader` inside the
/// local-bypass test.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit(
    d: &Dialect,
    style: GradingToneStyle,
    blacks: &[f32; 6],
    shadows: &[f32; 6],
    midtones: &[f32; 6],
    highlights: &[f32; 6],
    whites: &[f32; 6],
    scontrast_value: f32,
    forward: bool,
    uniforms: Option<&OpUniforms>,
) -> Vec<String> {
    let u = match uniforms {
        Some(OpUniforms::GradingTone(tone)) => {
            let GradingToneUniforms {
                blacks_r,
                blacks_g,
                blacks_b,
                blacks_m,
                blacks_start,
                blacks_width,
                shadows_r,
                shadows_g,
                shadows_b,
                shadows_m,
                shadows_start,
                shadows_width,
                midtones_r,
                midtones_g,
                midtones_b,
                midtones_m,
                midtones_start,
                midtones_width,
                highlights_r,
                highlights_g,
                highlights_b,
                highlights_m,
                highlights_start,
                highlights_width,
                whites_r,
                whites_g,
                whites_b,
                whites_m,
                whites_start,
                whites_width,
                scontrast,
                local_bypass,
            } = &**tone;
            Some((
                [
                    blacks_r,
                    blacks_g,
                    blacks_b,
                    blacks_m,
                    blacks_start,
                    blacks_width,
                ]
                .map(String::as_str),
                [
                    shadows_r,
                    shadows_g,
                    shadows_b,
                    shadows_m,
                    shadows_start,
                    shadows_width,
                ]
                .map(String::as_str),
                [
                    midtones_r,
                    midtones_g,
                    midtones_b,
                    midtones_m,
                    midtones_start,
                    midtones_width,
                ]
                .map(String::as_str),
                [
                    highlights_r,
                    highlights_g,
                    highlights_b,
                    highlights_m,
                    highlights_start,
                    highlights_width,
                ]
                .map(String::as_str),
                [
                    whites_r,
                    whites_g,
                    whites_b,
                    whites_m,
                    whites_start,
                    whites_width,
                ]
                .map(String::as_str),
                scontrast.as_str(),
                local_bypass.as_str(),
            ))
        }
        _ => None,
    };
    let p = Props {
        blacks: control(d, u.map(|u| u.0), blacks),
        shadows: control(d, u.map(|u| u.1), shadows),
        midtones: control(d, u.map(|u| u.2), midtones),
        highlights: control(d, u.map(|u| u.3), highlights),
        whites: control(d, u.map(|u| u.4), whites),
        scontrast: u.map_or_else(|| nf(scontrast_value), |u| d.uniform(u.5)),
    };
    let style_name = match style {
        GradingToneStyle::Log => "log",
        GradingToneStyle::Linear => "linear",
        GradingToneStyle::Video => "video",
    };
    let mut w = W {
        d,
        out: Vec::new(),
        depth: 0,
    };
    w.l(format!(
        "{{ // GradingTone '{style_name}' {}",
        if forward { "forward" } else { "inverse" }
    ));
    w.depth += 1;
    if let Some(u) = u {
        w.open(&format!("if (!{}) ", d.uniform_bool(u.6)));
    }
    let lin = style == GradingToneStyle::Linear;
    if lin {
        // NB (OCIO): the round trip through log errs in the lowest bit.
        w.lines(lin_to_log(d, false));
    }
    use Ch::*;
    if forward {
        for c in [R, G, B, M] {
            mids_fwd(&mut w, c, &p, style);
        }
        for c in [R, G, B, M] {
            highlight_shadow(&mut w, c, &p, false, true);
        }
        for c in [R, G, B, M] {
            white_black(&mut w, c, &p, false, true);
        }
        for c in [R, G, B, M] {
            highlight_shadow(&mut w, c, &p, true, true);
        }
        for c in [R, G, B, M] {
            white_black(&mut w, c, &p, true, true);
        }
        scontrast(&mut w, &p, style, true);
    } else {
        scontrast(&mut w, &p, style, false);
        for c in [M, R, G, B] {
            white_black(&mut w, c, &p, true, false);
        }
        for c in [M, R, G, B] {
            highlight_shadow(&mut w, c, &p, true, false);
        }
        for c in [M, R, G, B] {
            white_black(&mut w, c, &p, false, false);
        }
        for c in [M, R, G, B] {
            highlight_shadow(&mut w, c, &p, false, false);
        }
        for c in [M, R, G, B] {
            mids_rev(&mut w, c, &p, style);
        }
    }
    if lin {
        w.lines(log_to_lin(d, false));
    }
    // OCIO's TODO: high control values can overflow half float; clamp to its maximum.
    let v4 = match d.lang {
        super::GpuLanguage::Wgsl => "vec4<f32>",
        super::GpuLanguage::Hlsl50 | super::GpuLanguage::Metal => "float4",
        _ => "vec4",
    };
    w.l(format!(
        "color = min(color, {v4}(65504.0, 65504.0, 65504.0, 65504.0));"
    ));
    if u.is_some() {
        w.close();
    }
    w.close();
    w.out
}
