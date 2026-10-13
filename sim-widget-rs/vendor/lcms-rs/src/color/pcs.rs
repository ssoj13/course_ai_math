//! PCS (Profile Connection Space) conversions and colour-difference metrics.
//!
//! A pure-Rust, bit-exact port of `src/cmspcs.c` from Little-CMS (lcms2). Covers the
//! interchange-space math (XYZ ↔ xyY, XYZ ↔ L\*a\*b\*, Lab ↔ LCh), the ICC fixed-point
//! encodings for Lab and XYZ, the family of ΔE colour-difference formulas
//! (ΔE76 / CIE94 / BFD / CMC / CIEDE2000), and the colour-space enumeration utilities.
//!
//! # ICC V2 vs V4 Lab encoding
//!
//! The 16-bit Lab encoding changed between ICC profile versions and the two paths are
//! kept **strictly separate** here, exactly as in the C source:
//!
//! | | V2 (`cmsSigLabData` in v2/v3 profiles) | V4 |
//! |---|---|---|
//! | L\* scale | `/ 652.8` (0..100 → 0x0000..0xFF00) | `/ 655.35` (0..100 → 0x0000..0xFFFF) |
//! | a\*/b\* scale | `/ 256` (offset 128) | `/ 257` (offset 128) |
//! | neutral a\*/b\* | `0x8000` | `0x8080` |
//!
//! Do not attempt to unify them: the different scale factors and neutral points are
//! observable in encoded profile data.

use crate::color::d50_xyz;
use crate::types::color::{CIELCh, CIELab, CIEXYZ, CIExyY};
use crate::types::num::{quick_saturate_word, s15f16_to_f64};
use crate::types::sig::{Signature, colorspace};

/// ICC v4 max encodeable XYZ (`MAX_ENCODEABLE_XYZ`, `lcms2_internal.h`): `1 + 32767/32768`.
const MAX_ENCODEABLE_XYZ: f64 = 1.0 + 32767.0 / 32768.0;
/// V2 a\*/b\* lower clamp (`MIN_ENCODEABLE_ab2`).
const MIN_ENCODEABLE_AB2: f64 = -128.0;
/// V2 a\*/b\* upper clamp (`MAX_ENCODEABLE_ab2`): `(65535/256) - 128`.
const MAX_ENCODEABLE_AB2: f64 = (65535.0 / 256.0) - 128.0;
/// V4 a\*/b\* lower clamp (`MIN_ENCODEABLE_ab4`).
const MIN_ENCODEABLE_AB4: f64 = -128.0;
/// V4 a\*/b\* upper clamp (`MAX_ENCODEABLE_ab4`).
const MAX_ENCODEABLE_AB4: f64 = 127.0;

/// lcms2 pixel-type colour-space codes (`PT_*`, `lcms2.h`).
///
/// These are the internal "our notation" numbers used by `_cmsICCcolorSpace` /
/// `_cmsLCMScolorSpace`, distinct from the ICC 4CC [`Signature`] values. Verified against
/// `lcms2.h` lines 742..771.
pub mod pt {
    pub const ANY: i32 = 0; // Don't check colorspace
    pub const GRAY: i32 = 3;
    pub const RGB: i32 = 4;
    pub const CMY: i32 = 5;
    pub const CMYK: i32 = 6;
    pub const YCBCR: i32 = 7;
    pub const YUV: i32 = 8; // Lu'v'
    pub const XYZ: i32 = 9;
    pub const LAB: i32 = 10;
    pub const YUVK: i32 = 11; // Lu'v'K
    pub const HSV: i32 = 12;
    pub const HLS: i32 = 13;
    pub const YXY: i32 = 14;
    pub const MCH1: i32 = 15;
    pub const MCH2: i32 = 16;
    pub const MCH3: i32 = 17;
    pub const MCH4: i32 = 18;
    pub const MCH5: i32 = 19;
    pub const MCH6: i32 = 20;
    pub const MCH7: i32 = 21;
    pub const MCH8: i32 = 22;
    pub const MCH9: i32 = 23;
    pub const MCH10: i32 = 24;
    pub const MCH11: i32 = 25;
    pub const MCH12: i32 = 26;
    pub const MCH13: i32 = 27;
    pub const MCH14: i32 = 28;
    pub const MCH15: i32 = 29;
    pub const LAB_V2: i32 = 30; // Identical to PT_Lab, but using the V2 old encoding
}

/// ICC colour-space [`Signature`]s not present in [`crate::types::sig::colorspace`].
///
/// Built authoritatively from their 4CC bytes (matching `cmsColorSpaceSignature` in
/// `lcms2.h`). `MCH10..=MCH15` use `'MCHA'..'MCHF'` for the first-after-`MCH` byte.
mod cs {
    use crate::types::sig::Signature;

    pub const LUVK: Signature = Signature::new(b"LuvK");
    pub const MCH1: Signature = Signature::new(b"MCH1");
    pub const MCH2: Signature = Signature::new(b"MCH2");
    pub const MCH3: Signature = Signature::new(b"MCH3");
    pub const MCH4: Signature = Signature::new(b"MCH4");
    pub const MCH5: Signature = Signature::new(b"MCH5");
    pub const MCH6: Signature = Signature::new(b"MCH6");
    pub const MCH7: Signature = Signature::new(b"MCH7");
    pub const MCH8: Signature = Signature::new(b"MCH8");
    pub const MCH9: Signature = Signature::new(b"MCH9");
    pub const MCH10: Signature = Signature::new(b"MCHA");
    pub const MCH11: Signature = Signature::new(b"MCHB");
    pub const MCH12: Signature = Signature::new(b"MCHC");
    pub const MCH13: Signature = Signature::new(b"MCHD");
    pub const MCH14: Signature = Signature::new(b"MCHE");
    pub const MCH15: Signature = Signature::new(b"MCHF");
}

// ===========================================================================================
// Space conversions: XYZ <-> xyY, XYZ <-> Lab, Lab <-> LCh
// ===========================================================================================

/// XYZ → xyY chromaticity (`cmsXYZ2xyY`).
pub fn cms_xyz_to_xyy(source: &CIEXYZ) -> CIExyY {
    let isum = 1.0 / (source.x + source.y + source.z);
    CIExyY { x: source.x * isum, y: source.y * isum, yy: source.y }
}

/// xyY → XYZ (`cmsxyY2XYZ`).
pub fn cms_xyy_to_xyz(source: &CIExyY) -> CIEXYZ {
    CIEXYZ {
        x: (source.x / source.y) * source.yy,
        y: source.yy,
        z: ((1.0 - source.x - source.y) / source.y) * source.yy,
    }
}

/// CIELab forward companding function `f` (`f` in cmspcs.c).
///
/// Asymmetric cubic/linear with the break at `Limit = (24/116)^3`. The `Limit` product is
/// written out (not `powi`) to match the exact C constant folding.
fn f(t: f64) -> f64 {
    let limit = (24.0 / 116.0) * (24.0 / 116.0) * (24.0 / 116.0);
    if t <= limit {
        (841.0 / 108.0) * t + (16.0 / 116.0)
    } else {
        t.powf(1.0 / 3.0)
    }
}

/// Inverse companding function `f_1` (`f_1` in cmspcs.c).
///
/// Note the break point here is `Limit = 24/116` (the *un-cubed* value), matching C.
fn f_1(t: f64) -> f64 {
    let limit = 24.0 / 116.0;
    if t <= limit {
        (108.0 / 841.0) * (t - (16.0 / 116.0))
    } else {
        t * t * t
    }
}

/// XYZ → CIELab (`cmsXYZ2Lab`). Handles some negative XYZ inputs, as in C.
///
/// `white_point == None` selects D50 (`cmsD50_XYZ()`).
pub fn cms_xyz_to_lab(white_point: Option<&CIEXYZ>, xyz: &CIEXYZ) -> CIELab {
    let wp = white_point.copied().unwrap_or_else(d50_xyz);

    let fx = f(xyz.x / wp.x);
    let fy = f(xyz.y / wp.y);
    let fz = f(xyz.z / wp.z);

    CIELab { l: 116.0 * fy - 16.0, a: 500.0 * (fx - fy), b: 200.0 * (fy - fz) }
}

/// CIELab → XYZ (`cmsLab2XYZ`). Can return negative XYZ in some cases, as in C.
///
/// `white_point == None` selects D50 (`cmsD50_XYZ()`).
pub fn cms_lab_to_xyz(white_point: Option<&CIEXYZ>, lab: &CIELab) -> CIEXYZ {
    let wp = white_point.copied().unwrap_or_else(d50_xyz);

    let y = (lab.l + 16.0) / 116.0;
    let x = y + 0.002 * lab.a;
    let z = y - 0.005 * lab.b;

    CIEXYZ { x: f_1(x) * wp.x, y: f_1(y) * wp.y, z: f_1(z) * wp.z }
}

/// Square helper (`Sqr`).
#[inline]
fn sqr(v: f64) -> f64 {
    v * v
}

/// `atan2` in degrees, returning 0 when `a == b == 0` (`atan2deg`).
///
/// Result normalised to `[0, 360)` via the same `while` loops as C.
fn atan2deg(a: f64, b: f64) -> f64 {
    let mut h = if a == 0.0 && b == 0.0 { 0.0 } else { a.atan2(b) };

    h *= 180.0 / std::f64::consts::PI;

    while h > 360.0 {
        h -= 360.0;
    }
    while h < 0.0 {
        h += 360.0;
    }

    h
}

/// Degrees → radians (`RADIANS`).
#[inline]
fn radians(deg: f64) -> f64 {
    (deg * std::f64::consts::PI) / 180.0
}

/// CIELab → CIELCh cylindrical coordinates (`cmsLab2LCh`). No range checks (C parity).
pub fn cms_lab_to_lch(lab: &CIELab) -> CIELCh {
    CIELCh {
        l: lab.l,
        c: (sqr(lab.a) + sqr(lab.b)).powf(0.5),
        h: atan2deg(lab.b, lab.a),
    }
}

/// CIELCh → CIELab (`cmsLCh2Lab`). No range checks (C parity).
pub fn cms_lch_to_lab(lch: &CIELCh) -> CIELab {
    let h = (lch.h * std::f64::consts::PI) / 180.0;
    CIELab { l: lch.l, a: lch.c * h.cos(), b: lch.c * h.sin() }
}

// ===========================================================================================
// Lab encoding / decoding — V2 and V4 kept SEPARATE
// ===========================================================================================

/// V2 L\* decode (`L2float2`): `v / 652.8`.
#[inline]
fn l2float2(v: u16) -> f64 {
    v as f64 / 652.800
}

/// V2 a\*/b\* decode (`ab2float2`): `v / 256 - 128`.
#[inline]
fn ab2float2(v: u16) -> f64 {
    (v as f64 / 256.0) - 128.0
}

/// V2 L\* encode (`L2Fix2`): `sat(L * 652.8)`.
#[inline]
fn l2fix2(l: f64) -> u16 {
    quick_saturate_word(l * 652.8)
}

/// V2 a\*/b\* encode (`ab2Fix2`): `sat((ab + 128) * 256)`.
#[inline]
fn ab2fix2(ab: f64) -> u16 {
    quick_saturate_word((ab + 128.0) * 256.0)
}

/// V4 L\* decode (`L2float4`): `v / 655.35`.
#[inline]
fn l2float4(v: u16) -> f64 {
    v as f64 / 655.35
}

/// V4 a\*/b\* decode (`ab2float4`): `v / 257 - 128`.
#[inline]
fn ab2float4(v: u16) -> f64 {
    (v as f64 / 257.0) - 128.0
}

/// V4 L\* encode (`L2Fix4`): `sat(L * 655.35)`.
#[inline]
fn l2fix4(l: f64) -> u16 {
    quick_saturate_word(l * 655.35)
}

/// V4 a\*/b\* encode (`ab2Fix4`): `sat((ab + 128) * 257)`.
#[inline]
fn ab2fix4(ab: f64) -> u16 {
    quick_saturate_word((ab + 128.0) * 257.0)
}

/// Decode a V2-encoded 16-bit Lab triple (`cmsLabEncoded2FloatV2`).
pub fn cms_lab_encoded_to_float_v2(w_lab: &[u16; 3]) -> CIELab {
    CIELab { l: l2float2(w_lab[0]), a: ab2float2(w_lab[1]), b: ab2float2(w_lab[2]) }
}

/// Decode a V4-encoded 16-bit Lab triple (`cmsLabEncoded2Float`).
pub fn cms_lab_encoded_to_float(w_lab: &[u16; 3]) -> CIELab {
    CIELab { l: l2float4(w_lab[0]), a: ab2float4(w_lab[1]), b: ab2float4(w_lab[2]) }
}

/// Clamp L\* into the V2 encodeable range (`Clamp_L_doubleV2`): `[0, 0xFFFF*100/0xFF00]`.
fn clamp_l_double_v2(l: f64) -> f64 {
    let l_max = (0xFFFF as f64 * 100.0) / 0xFF00 as f64;
    l.clamp(0.0, l_max)
}

/// Clamp a\*/b\* into the V2 encodeable range (`Clamp_ab_doubleV2`).
fn clamp_ab_double_v2(ab: f64) -> f64 {
    ab.clamp(MIN_ENCODEABLE_AB2, MAX_ENCODEABLE_AB2)
}

/// Encode a float Lab as a V2 16-bit triple (`cmsFloat2LabEncodedV2`).
pub fn cms_float_to_lab_encoded_v2(f_lab: &CIELab) -> [u16; 3] {
    let l = clamp_l_double_v2(f_lab.l);
    let a = clamp_ab_double_v2(f_lab.a);
    let b = clamp_ab_double_v2(f_lab.b);
    [l2fix2(l), ab2fix2(a), ab2fix2(b)]
}

/// Clamp L\* into the V4 encodeable range (`Clamp_L_doubleV4`): `[0, 100]`.
fn clamp_l_double_v4(l: f64) -> f64 {
    l.clamp(0.0, 100.0)
}

/// Clamp a\*/b\* into the V4 encodeable range (`Clamp_ab_doubleV4`).
fn clamp_ab_double_v4(ab: f64) -> f64 {
    ab.clamp(MIN_ENCODEABLE_AB4, MAX_ENCODEABLE_AB4)
}

/// Encode a float Lab as a V4 16-bit triple (`cmsFloat2LabEncoded`).
pub fn cms_float_to_lab_encoded(f_lab: &CIELab) -> [u16; 3] {
    let l = clamp_l_double_v4(f_lab.l);
    let a = clamp_ab_double_v4(f_lab.a);
    let b = clamp_ab_double_v4(f_lab.b);
    [l2fix4(l), ab2fix4(a), ab2fix4(b)]
}

// ===========================================================================================
// XYZ encoding / decoding — 1.15 fixed point
// ===========================================================================================

/// XYZ component encode (`XYZ2Fix`): all three use 1.15 fixed point, `sat(d * 32768)`.
#[inline]
fn xyz2fix(d: f64) -> u16 {
    quick_saturate_word(d * 32768.0)
}

/// XYZ component decode (`XYZ2float`).
///
/// C reinterprets the 1.15 word as 15.16 by shifting left one bit, then converts via
/// `_cms15Fixed16toDouble`.
#[inline]
fn xyz2float(v: u16) -> f64 {
    // From 1.15 to 15.16
    let fix32 = (v as i32) << 1;
    s15f16_to_f64(fix32)
}

/// Encode a float XYZ as a 1.15 fixed-point 16-bit triple (`cmsFloat2XYZEncoded`).
///
/// Clamps to `[0, MAX_ENCODEABLE_XYZ]` per component; a non-positive Y zeroes all three
/// components first (exactly as the C source orders the checks).
// The per-component upper/lower checks are transcribed verbatim from C in their original
// order (not `f64::clamp`): the ordering after the Y<=0 zeroing and the NaN fall-through
// behaviour are part of the bit-exact contract.
#[allow(clippy::manual_clamp)]
pub fn cms_float_to_xyz_encoded(f_xyz: &CIEXYZ) -> [u16; 3] {
    let mut xyz = *f_xyz;

    // Clamp to encodeable values.
    if xyz.y <= 0.0 {
        xyz.x = 0.0;
        xyz.y = 0.0;
        xyz.z = 0.0;
    }

    if xyz.x > MAX_ENCODEABLE_XYZ {
        xyz.x = MAX_ENCODEABLE_XYZ;
    }
    if xyz.x < 0.0 {
        xyz.x = 0.0;
    }

    if xyz.y > MAX_ENCODEABLE_XYZ {
        xyz.y = MAX_ENCODEABLE_XYZ;
    }
    if xyz.y < 0.0 {
        xyz.y = 0.0;
    }

    if xyz.z > MAX_ENCODEABLE_XYZ {
        xyz.z = MAX_ENCODEABLE_XYZ;
    }
    if xyz.z < 0.0 {
        xyz.z = 0.0;
    }

    [xyz2fix(xyz.x), xyz2fix(xyz.y), xyz2fix(xyz.z)]
}

/// Decode a 1.15 fixed-point 16-bit XYZ triple (`cmsXYZEncoded2Float`).
pub fn cms_xyz_encoded_to_float(xyz: &[u16; 3]) -> CIEXYZ {
    CIEXYZ { x: xyz2float(xyz[0]), y: xyz2float(xyz[1]), z: xyz2float(xyz[2]) }
}

// ===========================================================================================
// Colour-difference metrics (ΔE)
// ===========================================================================================

/// ΔE76 — Euclidean Lab distance (`cmsDeltaE`).
pub fn cms_delta_e(lab1: &CIELab, lab2: &CIELab) -> f64 {
    let dl = (lab1.l - lab2.l).abs();
    let da = (lab1.a - lab2.a).abs();
    let db = (lab1.b - lab2.b).abs();
    (sqr(dl) + sqr(da) + sqr(db)).powf(0.5)
}

/// CIE94 ΔE (`cmsCIE94DeltaE`).
pub fn cms_cie94_delta_e(lab1: &CIELab, lab2: &CIELab) -> f64 {
    let dl = (lab1.l - lab2.l).abs();

    let lch1 = cms_lab_to_lch(lab1);
    let lch2 = cms_lab_to_lch(lab2);

    let dc = (lch1.c - lch2.c).abs();
    let de = cms_delta_e(lab1, lab2);

    let dhsq = sqr(de) - sqr(dl) - sqr(dc);
    let dh = if dhsq < 0.0 { 0.0 } else { dhsq.powf(0.5) };

    let c12 = (lch1.c * lch2.c).sqrt();

    let sc = 1.0 + (0.048 * c12);
    let sh = 1.0 + (0.014 * c12);

    (sqr(dl) + sqr(dc) / sqr(sc) + sqr(dh) / sqr(sh)).sqrt()
}

/// BFD lightness auxiliary (`ComputeLBFD`).
fn compute_lbfd(lab: &CIELab) -> f64 {
    let yt = if lab.l > 7.996969 {
        (sqr((lab.l + 16.0) / 116.0) * ((lab.l + 16.0) / 116.0)) * 100.0
    } else {
        100.0 * (lab.l / 903.3)
    };

    54.6 * (std::f64::consts::LOG10_E * ((yt + 1.5).ln())) - 9.6
}

/// BFD(1:1) ΔE between two Lab values (`cmsBFDdeltaE`).
pub fn cms_bfd_delta_e(lab1: &CIELab, lab2: &CIELab) -> f64 {
    let lbfd1 = compute_lbfd(lab1);
    let lbfd2 = compute_lbfd(lab2);
    let delta_l = lbfd2 - lbfd1;

    let lch1 = cms_lab_to_lch(lab1);
    let lch2 = cms_lab_to_lch(lab2);

    let delta_c = lch2.c - lch1.c;
    let ave_c = (lch1.c + lch2.c) / 2.0;
    let ave_h = (lch1.h + lch2.h) / 2.0;

    let de = cms_delta_e(lab1, lab2);

    let deltah = if sqr(de) > (sqr(lab2.l - lab1.l) + sqr(delta_c)) {
        (sqr(de) - sqr(lab2.l - lab1.l) - sqr(delta_c)).sqrt()
    } else {
        0.0
    };

    let dc = 0.035 * ave_c / (1.0 + 0.00365 * ave_c) + 0.521;
    let g = (sqr(sqr(ave_c)) / (sqr(sqr(ave_c)) + 14000.0)).sqrt();
    let t = 0.627
        + (0.055 * ((ave_h - 254.0) / (180.0 / std::f64::consts::PI)).cos()
            - 0.040 * ((2.0 * ave_h - 136.0) / (180.0 / std::f64::consts::PI)).cos()
            + 0.070 * ((3.0 * ave_h - 31.0) / (180.0 / std::f64::consts::PI)).cos()
            + 0.049 * ((4.0 * ave_h + 114.0) / (180.0 / std::f64::consts::PI)).cos()
            - 0.015 * ((5.0 * ave_h - 103.0) / (180.0 / std::f64::consts::PI)).cos());

    let dh = dc * (g * t + 1.0 - g);
    let rh = -0.260 * ((ave_h - 308.0) / (180.0 / std::f64::consts::PI)).cos()
        - 0.379 * ((2.0 * ave_h - 160.0) / (180.0 / std::f64::consts::PI)).cos()
        - 0.636 * ((3.0 * ave_h + 254.0) / (180.0 / std::f64::consts::PI)).cos()
        + 0.226 * ((4.0 * ave_h + 140.0) / (180.0 / std::f64::consts::PI)).cos()
        - 0.194 * ((5.0 * ave_h + 280.0) / (180.0 / std::f64::consts::PI)).cos();

    let rc = ((ave_c * ave_c * ave_c * ave_c * ave_c * ave_c)
        / ((ave_c * ave_c * ave_c * ave_c * ave_c * ave_c) + 70000000.0))
        .sqrt();
    let rt = rh * rc;

    (sqr(delta_l) + sqr(delta_c / dc) + sqr(deltah / dh) + (rt * (delta_c / dc) * (deltah / dh)))
        .sqrt()
}

/// CMC(l:c) ΔE between two Lab values (`cmsCMCdeltaE`).
pub fn cms_cmc_delta_e(lab1: &CIELab, lab2: &CIELab, l: f64, c: f64) -> f64 {
    if lab1.l == 0.0 && lab2.l == 0.0 {
        return 0.0;
    }

    let lch1 = cms_lab_to_lch(lab1);
    let lch2 = cms_lab_to_lch(lab2);

    let dl = lab2.l - lab1.l;
    let dc = lch2.c - lch1.c;

    let de = cms_delta_e(lab1, lab2);

    let dh = if sqr(de) > (sqr(dl) + sqr(dc)) {
        (sqr(de) - sqr(dl) - sqr(dc)).sqrt()
    } else {
        0.0
    };

    let t = if (lch1.h > 164.0) && (lch1.h < 345.0) {
        0.56 + (0.2 * ((lch1.h + 168.0) / (180.0 / std::f64::consts::PI)).cos()).abs()
    } else {
        0.36 + (0.4 * ((lch1.h + 35.0) / (180.0 / std::f64::consts::PI)).cos()).abs()
    };

    let sc = 0.0638 * lch1.c / (1.0 + 0.0131 * lch1.c) + 0.638;
    let mut sl = 0.040975 * lab1.l / (1.0 + 0.01765 * lab1.l);

    if lab1.l < 16.0 {
        sl = 0.511;
    }

    let ff = ((lch1.c * lch1.c * lch1.c * lch1.c)
        / ((lch1.c * lch1.c * lch1.c * lch1.c) + 1900.0))
        .sqrt();
    let sh = sc * (t * ff + 1.0 - ff);

    (sqr(dl / (l * sl)) + sqr(dc / (c * sc)) + sqr(dh / sh)).sqrt()
}

/// CIEDE2000 ΔE (`cmsCIE2000DeltaE`).
///
/// `kl`, `kc`, `kh` are the lightness/chroma/hue weightings. The G-factor, hue-rotation
/// mean/delta branch selection and T-weighting are transcribed verbatim from C — they are
/// branch-sensitive around the ±180° hue boundary.
pub fn cms_cie2000_delta_e(lab1: &CIELab, lab2: &CIELab, kl: f64, kc: f64, kh: f64) -> f64 {
    let l1 = lab1.l;
    let a1 = lab1.a;
    let b1 = lab1.b;
    let c = (sqr(a1) + sqr(b1)).sqrt();

    let ls = lab2.l;
    let as_ = lab2.a;
    let bs = lab2.b;
    let cs = (sqr(as_) + sqr(bs)).sqrt();

    let g = 0.5
        * (1.0
            - (((c + cs) / 2.0).powf(7.0)
                / (((c + cs) / 2.0).powf(7.0) + 25.0f64.powf(7.0)))
            .sqrt());

    let a_p = (1.0 + g) * a1;
    let b_p = b1;
    let c_p = (sqr(a_p) + sqr(b_p)).sqrt();
    let h_p = atan2deg(b_p, a_p);

    let a_ps = (1.0 + g) * as_;
    let b_ps = bs;
    let c_ps = (sqr(a_ps) + sqr(b_ps)).sqrt();
    let h_ps = atan2deg(b_ps, a_ps);

    let mean_c_p = (c_p + c_ps) / 2.0;

    let hps_plus_hp = h_ps + h_p;
    let hps_minus_hp = h_ps - h_p;

    let meanh_p = if hps_minus_hp.abs() <= 180.000001 {
        hps_plus_hp / 2.0
    } else if hps_plus_hp < 360.0 {
        (hps_plus_hp + 360.0) / 2.0
    } else {
        (hps_plus_hp - 360.0) / 2.0
    };

    let delta_h = if hps_minus_hp <= -180.000001 {
        hps_minus_hp + 360.0
    } else if hps_minus_hp > 180.0 {
        hps_minus_hp - 360.0
    } else {
        hps_minus_hp
    };
    let delta_l = ls - l1;
    let delta_c = c_ps - c_p;

    let delta_h_cap = 2.0 * (c_ps * c_p).sqrt() * (radians(delta_h) / 2.0).sin();

    let t = 1.0 - 0.17 * radians(meanh_p - 30.0).cos()
        + 0.24 * radians(2.0 * meanh_p).cos()
        + 0.32 * radians(3.0 * meanh_p + 6.0).cos()
        - 0.2 * radians(4.0 * meanh_p - 63.0).cos();

    let sl = 1.0
        + (0.015 * sqr((ls + l1) / 2.0 - 50.0)) / (20.0 + sqr((ls + l1) / 2.0 - 50.0)).sqrt();

    let sc = 1.0 + 0.045 * (c_p + c_ps) / 2.0;
    let sh = 1.0 + 0.015 * ((c_ps + c_p) / 2.0) * t;

    let delta_ro = 30.0 * (-sqr((meanh_p - 275.0) / 25.0)).exp();

    let rc = 2.0 * (mean_c_p.powf(7.0) / (mean_c_p.powf(7.0) + 25.0f64.powf(7.0))).sqrt();

    let rt = -(2.0 * radians(delta_ro)).sin() * rc;

    (sqr(delta_l / (sl * kl))
        + sqr(delta_c / (sc * kc))
        + sqr(delta_h_cap / (sh * kh))
        + rt * (delta_c / (sc * kc)) * (delta_h_cap / (sh * kh)))
    .sqrt()
}

// ===========================================================================================
// Colour-space utilities
// ===========================================================================================

/// Number of LUT gridpoints to use for a colour space (`_cmsReasonableGridpointsByColorspace`).
///
/// `dw_flags` may force a value in bits 16..23, or select high/low resolution precalc.
pub fn reasonable_gridpoints_by_colorspace(colorspace: Signature, dw_flags: u32) -> u32 {
    // Already specified?
    if dw_flags & 0x00FF_0000 != 0 {
        // Yes, grab'em
        return (dw_flags >> 16) & 0xFF;
    }

    let n_channels = cms_channels_of(colorspace);

    // HighResPrecalc is maximum resolution
    if dw_flags & 0x0400 != 0 {
        if n_channels > 4 {
            return 7; // 7 for Hifi
        }
        if n_channels == 4 {
            // 23 for CMYK
            return 23;
        }
        return 49; // 49 for RGB and others
    }

    // LowResPrecal is lower resolution
    if dw_flags & 0x0800 != 0 {
        if n_channels > 4 {
            return 6; // 6 for more than 4 channels
        }
        if n_channels == 1 {
            return 33; // For monochrome
        }
        return 17; // 17 for remaining
    }

    // Default values
    if n_channels > 4 {
        return 7; // 7 for Hifi
    }
    if n_channels == 4 {
        return 17; // 17 for CMYK
    }
    33 // 33 for RGB
}

/// White/black end-point encodings for a colour space (`_cmsEndPointsBySpace`).
///
/// The `white`/`black` arrays carry four slots (matching the C statics); only the first
/// [`EndPoints::n_outputs`] are meaningful. Returns `None` for unsupported spaces (the C
/// `FALSE` case). Lab uses the **V4** neutral encoding (`0x8080`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndPoints {
    pub white: [u16; 4],
    pub black: [u16; 4],
    pub n_outputs: u32,
}

/// See [`EndPoints`] (`_cmsEndPointsBySpace`).
pub fn end_points_by_space(space: Signature) -> Option<EndPoints> {
    if space == colorspace::GRAY {
        Some(EndPoints { white: [0xffff, 0, 0, 0], black: [0, 0, 0, 0], n_outputs: 1 })
    } else if space == colorspace::RGB {
        Some(EndPoints {
            white: [0xffff, 0xffff, 0xffff, 0],
            black: [0, 0, 0, 0],
            n_outputs: 3,
        })
    } else if space == colorspace::LAB {
        // V4 Lab encoding
        Some(EndPoints {
            white: [0xFFFF, 0x8080, 0x8080, 0],
            black: [0, 0x8080, 0x8080, 0],
            n_outputs: 3,
        })
    } else if space == colorspace::CMYK {
        Some(EndPoints {
            white: [0, 0, 0, 0],
            black: [0xffff, 0xffff, 0xffff, 0xffff], // 400% of ink
            n_outputs: 4,
        })
    } else if space == colorspace::CMY {
        Some(EndPoints {
            white: [0, 0, 0, 0],
            black: [0xffff, 0xffff, 0xffff, 0],
            n_outputs: 3,
        })
    } else {
        None
    }
}

/// Translate an lcms `PT_*` notation to its ICC colour-space [`Signature`]
/// (`_cmsICCcolorSpace`). Returns `Signature(0)` for unknown input (the C default).
pub fn icc_colorspace(our_notation: i32) -> Signature {
    match our_notation {
        1 | pt::GRAY => colorspace::GRAY,
        2 | pt::RGB => colorspace::RGB,
        pt::CMY => colorspace::CMY,
        pt::CMYK => colorspace::CMYK,
        pt::YCBCR => colorspace::YCBCR,
        pt::YUV => colorspace::LUV,
        pt::XYZ => colorspace::XYZ,
        pt::LAB_V2 | pt::LAB => colorspace::LAB,
        pt::YUVK => cs::LUVK,
        pt::HSV => colorspace::HSV,
        pt::HLS => colorspace::HLS,
        pt::YXY => colorspace::YXY,
        pt::MCH1 => cs::MCH1,
        pt::MCH2 => cs::MCH2,
        pt::MCH3 => cs::MCH3,
        pt::MCH4 => cs::MCH4,
        pt::MCH5 => cs::MCH5,
        pt::MCH6 => cs::MCH6,
        pt::MCH7 => cs::MCH7,
        pt::MCH8 => cs::MCH8,
        pt::MCH9 => cs::MCH9,
        pt::MCH10 => cs::MCH10,
        pt::MCH11 => cs::MCH11,
        pt::MCH12 => cs::MCH12,
        pt::MCH13 => cs::MCH13,
        pt::MCH14 => cs::MCH14,
        pt::MCH15 => cs::MCH15,
        _ => Signature(0),
    }
}

/// Translate an ICC colour-space [`Signature`] to its lcms `PT_*` notation
/// (`_cmsLCMScolorSpace`). Returns `0` (`PT_ANY`) for unknown input (the C default).
pub fn lcms_colorspace(profile_space: Signature) -> i32 {
    // N-colorant ('nCLR') and MCH signatures both map to the same PT_MCHn.
    match profile_space {
        colorspace::GRAY => pt::GRAY,
        colorspace::RGB => pt::RGB,
        colorspace::CMY => pt::CMY,
        colorspace::CMYK => pt::CMYK,
        colorspace::YCBCR => pt::YCBCR,
        colorspace::LUV => pt::YUV,
        colorspace::XYZ => pt::XYZ,
        colorspace::LAB => pt::LAB,
        cs::LUVK => pt::YUVK,
        colorspace::HSV => pt::HSV,
        colorspace::HLS => pt::HLS,
        colorspace::YXY => pt::YXY,

        colorspace::CLR1 | cs::MCH1 => pt::MCH1,
        colorspace::CLR2 | cs::MCH2 => pt::MCH2,
        colorspace::CLR3 | cs::MCH3 => pt::MCH3,
        colorspace::CLR4 | cs::MCH4 => pt::MCH4,
        colorspace::CLR5 | cs::MCH5 => pt::MCH5,
        colorspace::CLR6 | cs::MCH6 => pt::MCH6,
        cs::MCH7 | colorspace::CLR7 => pt::MCH7,
        cs::MCH8 | colorspace::CLR8 => pt::MCH8,
        cs::MCH9 | colorspace::CLR9 => pt::MCH9,
        cs::MCH10 | colorspace::CLR10 => pt::MCH10,
        cs::MCH11 | colorspace::CLR11 => pt::MCH11,
        cs::MCH12 | colorspace::CLR12 => pt::MCH12,
        cs::MCH13 | colorspace::CLR13 => pt::MCH13,
        cs::MCH14 | colorspace::CLR14 => pt::MCH14,
        cs::MCH15 | colorspace::CLR15 => pt::MCH15,

        _ => 0,
    }
}

/// Number of channels for a colour space (`cmsChannelsOfColorSpace`).
///
/// Returns `-1` for an unknown colour space.
pub fn cms_channels_of_colorspace(color_space: Signature) -> i32 {
    match color_space {
        cs::MCH1 | colorspace::CLR1 | colorspace::GRAY => 1,
        cs::MCH2 | colorspace::CLR2 => 2,
        colorspace::XYZ
        | colorspace::LAB
        | colorspace::LUV
        | colorspace::YCBCR
        | colorspace::YXY
        | colorspace::RGB
        | colorspace::HSV
        | colorspace::HLS
        | colorspace::CMY
        | cs::MCH3
        | colorspace::CLR3 => 3,
        cs::LUVK | colorspace::CMYK | cs::MCH4 | colorspace::CLR4 => 4,
        cs::MCH5 | colorspace::CLR5 => 5,
        cs::MCH6 | colorspace::CLR6 => 6,
        cs::MCH7 | colorspace::CLR7 => 7,
        cs::MCH8 | colorspace::CLR8 => 8,
        cs::MCH9 | colorspace::CLR9 => 9,
        cs::MCH10 | colorspace::CLR10 => 10,
        cs::MCH11 | colorspace::CLR11 => 11,
        cs::MCH12 | colorspace::CLR12 => 12,
        cs::MCH13 | colorspace::CLR13 => 13,
        cs::MCH14 | colorspace::CLR14 => 14,
        cs::MCH15 | colorspace::CLR15 => 15,
        _ => -1,
    }
}

/// Number of channels for a colour space (`cmsChannelsOf`, **deprecated**).
///
/// Compatibility wrapper: an unknown colour space maps to `3` rather than an error.
pub fn cms_channels_of(color_space: Signature) -> u32 {
    let n = cms_channels_of_colorspace(color_space);
    if n < 0 { 3 } else { n as u32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    // colorspace::LUVK is referenced in a match arm; make sure it resolves to the same
    // signature as our local cs::LUVK (both are 'LuvK').
    #[test]
    fn luvk_signatures_agree() {
        assert_eq!(colorspace::LUV.to_bytes(), *b"Luv ");
        assert_eq!(cs::LUVK.to_bytes(), *b"LuvK");
    }

    #[test]
    fn white_maps_to_l100() {
        // XYZ of the D50 white, with D50 as the reference, must yield L*=100, a*=b*=0.
        let wp = d50_xyz();
        let lab = cms_xyz_to_lab(None, &wp);
        assert!((lab.l - 100.0).abs() < 1e-12, "L*={}", lab.l);
        assert!(lab.a.abs() < 1e-12, "a*={}", lab.a);
        assert!(lab.b.abs() < 1e-12, "b*={}", lab.b);

        // Passing D50 explicitly must give the identical result.
        let lab2 = cms_xyz_to_lab(Some(&wp), &wp);
        assert_eq!(lab, lab2);
    }

    #[test]
    fn lab_xyz_round_trip() {
        let xyz = CIEXYZ { x: 0.3, y: 0.25, z: 0.18 };
        let lab = cms_xyz_to_lab(None, &xyz);
        let back = cms_lab_to_xyz(None, &lab);
        assert!((back.x - xyz.x).abs() < 1e-9, "x {} vs {}", back.x, xyz.x);
        assert!((back.y - xyz.y).abs() < 1e-9, "y {} vs {}", back.y, xyz.y);
        assert!((back.z - xyz.z).abs() < 1e-9, "z {} vs {}", back.z, xyz.z);
    }

    #[test]
    fn xyy_round_trip() {
        let xyz = CIEXYZ { x: 0.4, y: 0.5, z: 0.6 };
        let xyy = cms_xyz_to_xyy(&xyz);
        let back = cms_xyy_to_xyz(&xyy);
        assert!((back.x - xyz.x).abs() < 1e-12);
        assert!((back.y - xyz.y).abs() < 1e-12);
        assert!((back.z - xyz.z).abs() < 1e-12);
    }

    #[test]
    fn lab_lch_round_trip() {
        let lab = CIELab { l: 50.0, a: 20.0, b: -30.0 };
        let lch = cms_lab_to_lch(&lab);
        let back = cms_lch_to_lab(&lch);
        assert!((back.l - lab.l).abs() < 1e-12);
        assert!((back.a - lab.a).abs() < 1e-9, "a {} vs {}", back.a, lab.a);
        assert!((back.b - lab.b).abs() < 1e-9, "b {} vs {}", back.b, lab.b);
    }

    #[test]
    fn v2_v4_neutral_differs() {
        // Neutral Lab (a*=b*=0, L*=100): V2 neutral a/b is 0x8000, V4 is 0x8080; and L*
        // encodes 0xFF00 (V2) vs 0xFFFF (V4).
        let lab = CIELab { l: 100.0, a: 0.0, b: 0.0 };
        let v2 = cms_float_to_lab_encoded_v2(&lab);
        let v4 = cms_float_to_lab_encoded(&lab);
        assert_eq!(v2, [0xFF00, 0x8000, 0x8000]);
        assert_eq!(v4, [0xFFFF, 0x8080, 0x8080]);
        assert_ne!(v2, v4);
    }

    #[test]
    fn v4_lab_encode_decode_round_trip() {
        let lab = CIELab { l: 42.0, a: -12.0, b: 33.0 };
        let enc = cms_float_to_lab_encoded(&lab);
        let dec = cms_lab_encoded_to_float(&enc);
        // 16-bit quantisation: within one code step.
        assert!((dec.l - lab.l).abs() < 0.01, "L {} vs {}", dec.l, lab.l);
        assert!((dec.a - lab.a).abs() < 0.01, "a {} vs {}", dec.a, lab.a);
        assert!((dec.b - lab.b).abs() < 0.01, "b {} vs {}", dec.b, lab.b);
    }

    #[test]
    fn xyz_encode_decode_round_trip() {
        let xyz = CIEXYZ { x: 0.9642, y: 1.0, z: 0.8249 };
        let enc = cms_float_to_xyz_encoded(&xyz);
        let dec = cms_xyz_encoded_to_float(&enc);
        assert!((dec.x - xyz.x).abs() < 1e-4, "x {} vs {}", dec.x, xyz.x);
        assert!((dec.y - xyz.y).abs() < 1e-4, "y {} vs {}", dec.y, xyz.y);
        assert!((dec.z - xyz.z).abs() < 1e-4, "z {} vs {}", dec.z, xyz.z);
    }

    #[test]
    fn xyz_nonpositive_y_zeroes_all() {
        let xyz = CIEXYZ { x: 0.5, y: -0.1, z: 0.5 };
        assert_eq!(cms_float_to_xyz_encoded(&xyz), [0, 0, 0]);
    }

    #[test]
    fn delta_e_identical_is_zero() {
        let lab = CIELab { l: 50.0, a: 10.0, b: -20.0 };
        assert_eq!(cms_delta_e(&lab, &lab), 0.0);
        assert_eq!(cms_cie94_delta_e(&lab, &lab), 0.0);
        assert_eq!(cms_cmc_delta_e(&lab, &lab, 1.0, 1.0), 0.0);
        assert_eq!(cms_cie2000_delta_e(&lab, &lab, 1.0, 1.0, 1.0), 0.0);
    }

    #[test]
    fn delta_e76_known_value() {
        // Simple axis distance: pure L* difference of 10 → ΔE76 = 10.
        let a = CIELab { l: 50.0, a: 0.0, b: 0.0 };
        let b = CIELab { l: 60.0, a: 0.0, b: 0.0 };
        assert!((cms_delta_e(&a, &b) - 10.0).abs() < 1e-12);
        // 3-4-5 triangle in (a,b): ΔE = 5.
        let c = CIELab { l: 50.0, a: 3.0, b: 4.0 };
        assert!((cms_delta_e(&a, &c) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn atan2deg_normalises() {
        assert_eq!(atan2deg(0.0, 0.0), 0.0);
        assert!((atan2deg(1.0, 0.0) - 90.0).abs() < 1e-12);
        assert!((atan2deg(-1.0, 0.0) - 270.0).abs() < 1e-12); // wrapped into [0,360)
        assert!((atan2deg(0.0, 1.0) - 0.0).abs() < 1e-12);
    }

    #[test]
    fn channels_and_colorspace_maps() {
        assert_eq!(cms_channels_of_colorspace(colorspace::RGB), 3);
        assert_eq!(cms_channels_of_colorspace(colorspace::CMYK), 4);
        assert_eq!(cms_channels_of_colorspace(colorspace::GRAY), 1);
        assert_eq!(cms_channels_of_colorspace(cs::MCH15), 15);
        assert_eq!(cms_channels_of_colorspace(Signature::new(b"zzzz")), -1);

        // Deprecated wrapper: unknown → 3.
        assert_eq!(cms_channels_of(Signature::new(b"zzzz")), 3);
        assert_eq!(cms_channels_of(colorspace::CMYK), 4);

        // PT round-trips.
        assert_eq!(icc_colorspace(pt::RGB), colorspace::RGB);
        assert_eq!(lcms_colorspace(colorspace::RGB), pt::RGB);
        assert_eq!(icc_colorspace(pt::LAB_V2), colorspace::LAB);
        assert_eq!(icc_colorspace(pt::MCH10), cs::MCH10);
        assert_eq!(lcms_colorspace(cs::MCH10), pt::MCH10);
        assert_eq!(lcms_colorspace(colorspace::CLR10), pt::MCH10);
        assert_eq!(icc_colorspace(9999), Signature(0));
        assert_eq!(lcms_colorspace(Signature::new(b"zzzz")), pt::ANY);
    }

    #[test]
    fn gridpoints_and_endpoints() {
        // Forced value in bits 16..23.
        assert_eq!(reasonable_gridpoints_by_colorspace(colorspace::RGB, 0x0011_0000), 0x11);
        // Defaults.
        assert_eq!(reasonable_gridpoints_by_colorspace(colorspace::RGB, 0), 33);
        assert_eq!(reasonable_gridpoints_by_colorspace(colorspace::CMYK, 0), 17);
        // High-res precalc.
        assert_eq!(reasonable_gridpoints_by_colorspace(colorspace::RGB, 0x0400), 49);

        let ep = end_points_by_space(colorspace::LAB).unwrap();
        assert_eq!(ep.white, [0xFFFF, 0x8080, 0x8080, 0]);
        assert_eq!(ep.n_outputs, 3);
        assert!(end_points_by_space(colorspace::XYZ).is_none());
    }
}
