//! The colour-model FixedFunction styles on the CPU: OCIO `FixedFunctionOpCPU.cpp`
//! `Renderer_RGB_TO_HSV`, `_HSV_TO_RGB`, `_XYZ_TO_xyY`, `_xyY_TO_XYZ`, `_XYZ_TO_uvY`,
//! `_uvY_TO_XYZ`, `_XYZ_TO_LUV` and `_LUV_TO_XYZ`, line for line.
//!
//! These replaced textbook formulas that were not OCIO's: HSV without the extended-range
//! (negative) handling and with a different sector split, and CIE L*u*v* with `L*` in `0..100`,
//! `Yn`-style thresholds and rounded white-point constants, where OCIO's `L*` is in `0..1`.
//! Direction is resolved into the style before these run
//! (`op::resolve_fixed_function_direction`).

/// `Renderer_RGB_TO_HSV`.
pub(super) fn rgb_to_hsv(p: &mut [f32; 3]) {
    let [red, grn, blu] = *p;
    let rgb_min = red.min(grn).min(blu);
    let rgb_max = red.max(grn).max(blu);
    let mut val = rgb_max;
    let mut sat = 0.0f32;
    let mut hue = 0.0f32;
    if rgb_min != rgb_max {
        let delta = rgb_max - rgb_min;
        if rgb_max != 0.0 {
            sat = delta / rgb_max;
        }
        hue = if red == rgb_max {
            (grn - blu) / delta
        } else if grn == rgb_max {
            2.0 + (blu - red) / delta
        } else {
            4.0 + (red - grn) / delta
        };
        if hue < 0.0 {
            hue += 6.0;
        }
        hue *= 0.166_666_67;
    }
    // Extended-range inputs.
    if rgb_min < 0.0 {
        val += rgb_min;
    }
    if -rgb_min > rgb_max {
        sat = (rgb_max - rgb_min) / -rgb_min;
    }
    *p = [hue, sat, val];
}

/// `Renderer_HSV_TO_RGB`.
pub(super) fn hsv_to_rgb(p: &mut [f32; 3]) {
    const MAX_SAT: f32 = 1.999;
    let hue = (p[0] - p[0].floor()) * 6.0;
    let sat = p[1].clamp(0.0, MAX_SAT);
    let val = p[2];
    let red = ((hue - 3.0).abs() - 1.0).clamp(0.0, 1.0);
    let grn = (2.0 - (hue - 2.0).abs()).clamp(0.0, 1.0);
    let blu = (2.0 - (hue - 4.0).abs()).clamp(0.0, 1.0);
    let mut rgb_max = val;
    let mut rgb_min = val * (1.0 - sat);
    // Extended-range inputs.
    if sat > 1.0 {
        rgb_min = val * (1.0 - sat) / (2.0 - sat);
        rgb_max = val - rgb_min;
    }
    if val < 0.0 {
        rgb_min = val / (2.0 - sat);
        rgb_max = val - rgb_min;
    }
    let delta = rgb_max - rgb_min;
    *p = [
        red * delta + rgb_min,
        grn * delta + rgb_min,
        blu * delta + rgb_min,
    ];
}

/// `1 / d`, or 0 for `d == 0` (OCIO's guard).
fn recip0(d: f32) -> f32 {
    if d == 0.0 { 0.0 } else { 1.0 / d }
}

/// `Renderer_XYZ_TO_xyY`.
pub(super) fn xyz_to_xyy(p: &mut [f32; 3]) {
    let [x, y, z] = *p;
    let d = recip0(x + y + z);
    *p = [x * d, y * d, y];
}

/// `Renderer_xyY_TO_XYZ`.
pub(super) fn xyy_to_xyz(p: &mut [f32; 3]) {
    let [x, y, big_y] = *p;
    let d = recip0(y);
    *p = [big_y * x * d, big_y, big_y * (1.0 - x - y) * d];
}

/// `Renderer_XYZ_TO_uvY`.
pub(super) fn xyz_to_uvy(p: &mut [f32; 3]) {
    let [x, y, z] = *p;
    let d = recip0(x + 15.0 * y + 3.0 * z);
    *p = [4.0 * x * d, 9.0 * y * d, y];
}

/// `Renderer_uvY_TO_XYZ`.
pub(super) fn uvy_to_xyz(p: &mut [f32; 3]) {
    let [u, v, y] = *p;
    let d = recip0(v);
    *p = [
        (9.0 / 4.0) * y * u * d,
        y,
        (3.0 / 4.0) * y * (4.0 - u - 6.666_666_5 * v) * d,
    ];
}

/// `Renderer_XYZ_TO_LUV` (D65 white; `L*` in `0..1`).
pub(super) fn xyz_to_luv(p: &mut [f32; 3]) {
    let [x, y, z] = *p;
    let d = recip0(x + 15.0 * y + 3.0 * z);
    let u = 4.0 * x * d;
    let v = 9.0 * y * d;
    let lstar = if y <= 0.008_856_452 {
        9.032_963 * y
    } else {
        1.16 * y.powf(0.333_333_33) - 0.16
    };
    *p = [
        lstar,
        13.0 * lstar * (u - 0.197_83),
        13.0 * lstar * (v - 0.468_319_99),
    ];
}

/// `Renderer_LUV_TO_XYZ`.
pub(super) fn luv_to_xyz(p: &mut [f32; 3]) {
    let [lstar, ustar, vstar] = *p;
    let d = if lstar == 0.0 {
        0.0
    } else {
        0.076_923_08 / lstar
    };
    let u = ustar * d + 0.197_83;
    let v = vstar * d + 0.468_319_99;
    let tmp = (lstar + 0.16) * 0.862_068_96;
    let y = if lstar <= 0.08 {
        0.110_705_65 * lstar
    } else {
        tmp * tmp * tmp
    };
    let dd = if v == 0.0 { 0.0 } else { 0.25 / v };
    *p = [9.0 * y * u * dd, y, y * (12.0 - 3.0 * u - 20.0 * v) * dd];
}
