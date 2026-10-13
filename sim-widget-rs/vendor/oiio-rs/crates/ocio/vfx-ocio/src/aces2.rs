//! ACES 2.0 Output Transform implementation.
//!
//! Ported from OpenColorIO's ACES2/Transform.cpp.
//! Implements CAM16-based JMh perceptual color space, tonescale compression,
//! chroma compression, and gamut compression.
//!
//! # Where OCIO and the ACES CTL disagree
//!
//! This module follows OCIO in all four cases below. The CTL files are the ACES reference
//! implementation (`Lib.Academy.{OutputTransform,Tonescale}.ctl`).
//!
//! 1. **`gamma_top` interpolation weight.** The CTL's `interpolation_weight(h, h_lo, h_hi)`
//!    returns `h - h_lo` and never uses `h_hi`, so the weight is not normalized by the
//!    interval width. OCIO normalizes it to `(h - h_lo) / (h_hi - h_lo)` (`Transform.cpp:76-79`).
//!    The hue intervals are not uniform (corners are adjusted), so the CTL weight scales the
//!    gamma lerp wrongly. This reads as a CTL defect that OCIO fixed.
//! 2. **Tonescale of negative luminance.** The CTL clamps, `pow(max(0, x) / (x + s_2), g)`
//!    (`Tonescale.ctl:83`), so negative scene values map to 0. OCIO applies the tonescale to
//!    `|J|` and restores the sign (`Transform.cpp:385-389`), which keeps the transform
//!    invertible. Output differs from the CTL only for negative input.
//! 3. **The M == 0 guard.** The CTL tests `M < 0 || J > limit`, so M == 0 still runs through
//!    `compress_gamut`, which returns `{J, 0, h}` anyway. OCIO returns early on `M <= 0`
//!    (`Transform.cpp:1103`). The output is the same.
//! 4. **Table layout.** The CTL keeps a cusp table searched by uniform hue and a separate gamma
//!    table. Its `lookup_hue_interval` also seeds the search with `totalTableSize` where the
//!    nominal size is expected. OCIO merged cusp and gamma into one corner-aligned `Table3D`
//!    with a narrower search range. That is a deliberate OCIO redesign. As a result this module
//!    matches OCIO numerically, and both differ very slightly from raw CTL output near the hues
//!    of the gamut corners, below the visible threshold.

use crate::error::OcioError;
use std::f32::consts::PI;
use vfx_math::lerp as lerpf;

// --- Types ---

pub type F3 = [f32; 3];
pub type M33 = [f32; 9];

/// Chromaticity coordinate pair.
#[derive(Debug, Clone, Copy)]
pub struct Chromaticity {
    pub x: f32,
    pub y: f32,
}

/// Set of RGB primaries + white point.
#[derive(Debug, Clone, Copy)]
pub struct Primaries {
    pub red: Chromaticity,
    pub green: Chromaticity,
    pub blue: Chromaticity,
    pub white: Chromaticity,
}

// --- Well-known primaries ---

pub const ACES_AP0: Primaries = Primaries {
    red: Chromaticity {
        x: 0.7347,
        y: 0.2653,
    },
    green: Chromaticity { x: 0.0, y: 1.0 },
    blue: Chromaticity {
        x: 0.0001,
        y: -0.077,
    },
    white: Chromaticity {
        x: 0.32168,
        y: 0.33767,
    },
};

pub const ACES_AP1: Primaries = Primaries {
    red: Chromaticity { x: 0.713, y: 0.293 },
    green: Chromaticity { x: 0.165, y: 0.83 },
    blue: Chromaticity { x: 0.128, y: 0.044 },
    white: Chromaticity {
        x: 0.32168,
        y: 0.33767,
    },
};

const CAM16_PRIMARIES: Primaries = Primaries {
    red: Chromaticity {
        x: 0.8336,
        y: 0.1735,
    },
    green: Chromaticity {
        x: 2.3854,
        y: -1.4659,
    },
    blue: Chromaticity {
        x: 0.087,
        y: -0.125,
    },
    white: Chromaticity { x: 0.333, y: 0.333 },
};

// --- Constants ---

const HUE_LIMIT: f32 = 360.0;
const REFERENCE_LUMINANCE: f32 = 100.0;
const L_A: f32 = 100.0;
const Y_B: f32 = 20.0;
const SURROUND: F3 = [0.9, 0.59, 0.9]; // Dim surround

pub(crate) const J_SCALE: f32 = 100.0;
const CAM_NL_Y_REF: f32 = 100.0;
pub(crate) const CAM_NL_OFFSET: f32 = 0.2713 * CAM_NL_Y_REF;
const CAM_NL_SCALE: f32 = 4.0 * CAM_NL_Y_REF;

/// CAM16 post-adaptation nonlinearity exponent.
///
/// Single source for `post_adapt_compress_fwd_inner`/`_inv_inner` (below) and the GPU shader
/// emitter (`gpu/aces2.rs`), so the CPU oracle and the emitted shader can never drift on this
/// value. Extracting the former bare `0.42` literals to this named const is bit-identical.
pub(crate) const POST_ADAPT_POWER: f32 = 0.42;

/// Upper limit on the pre-inverse adapted response magnitude, matching the reference GPU shader's
/// `min(abs(rgb_a), 0.99)` (`FixedFunctionOpGPU.cpp:481`). Shared with `gpu/aces2.rs`.
pub(crate) const POST_ADAPT_RA_LIMIT: f32 = 0.99;

// Chroma compression
const CHROMA_COMPRESS: f32 = 2.4;
const CHROMA_COMPRESS_FACT: f32 = 3.3;
const CHROMA_EXPAND: f32 = 1.3;
const CHROMA_EXPAND_FACT: f32 = 0.69;
const CHROMA_EXPAND_THR: f32 = 0.5;

// Gamut compression
//
// `SMOOTH_CUSPS`, `CUSP_MID_BLEND`, `FOCUS_GAIN_BLEND` and `COMPRESSION_THRESHOLD` are `pub(crate)`
// because the GPU gamut-compress emitter (`gpu/aces2.rs`, ACES2-03 stage 3) transcribes the same
// runtime maths and must source these from here rather than retyping them — the literal-vs-computed
// drift class this crate bans. The rest stay private (table-generation only).
pub(crate) const SMOOTH_CUSPS: f32 = 0.12;
const SMOOTH_M: f32 = 0.27;
pub(crate) const CUSP_MID_BLEND: f32 = 1.3;
pub(crate) const FOCUS_GAIN_BLEND: f32 = 0.3;
#[allow(dead_code)]
const FOCUS_ADJUST_GAIN_INV: f32 = 1.0 / 0.55;
const FOCUS_DISTANCE: f32 = 1.35;
const FOCUS_DISTANCE_SCALING: f32 = 1.75;
pub(crate) const COMPRESSION_THRESHOLD: f32 = 0.75;

// Table generation
const GAMMA_MINIMUM: f32 = 0.0;
const GAMMA_MAXIMUM: f32 = 5.0;
const GAMMA_SEARCH_STEP: f32 = 0.4;
const GAMMA_ACCURACY: f32 = 1e-5;

const CUSP_CORNER_COUNT: usize = 6;
const TOTAL_CORNER_COUNT: usize = CUSP_CORNER_COUNT + 2;
const MAX_SORTED_CORNERS: usize = 2 * CUSP_CORNER_COUNT;
const REACH_CUSP_TOLERANCE: f32 = 1e-3;
const DISPLAY_CUSP_TOLERANCE: f32 = 1e-7;

const GAMMA_TEST_COUNT: usize = 5;

// --- Table types ---

const TABLE_NOMINAL_SIZE: usize = 360;
/// Guard-entry layout follows OCIO C++ `Common.h`:
/// 1 lower wrap slot at index 0, 360 nominal slots, then **two** upper
/// wrap slots so the binary-search lookup can `lerp(table[i_hi-1],
/// table[i_hi])` even when `i_hi == TABLE_UPPER_WRAP + 1` near hue
/// 360°.
const TABLE_TOTAL_SIZE: usize = TABLE_NOMINAL_SIZE + 3;
const TABLE_BASE_INDEX: usize = 1;
/// Lower guard-slot index. Shared with the GPU gamut emitter (`gpu/aces2.rs`), which clamps the
/// hue binary-search `i_lo` here exactly as `lookup_hue_interval`.
pub(crate) const TABLE_LOWER_WRAP: usize = 0;
/// First upper guard-slot index (== `TABLE_FIRST_NOM + 360`). Shared with the GPU gamut emitter,
/// which clamps the hue binary-search `i_hi` here exactly as `lookup_hue_interval`.
pub(crate) const TABLE_UPPER_WRAP: usize = TABLE_BASE_INDEX + TABLE_NOMINAL_SIZE;
/// Index of the first nominal (non-wrap) hue slot. Shared with the GPU emitter (`gpu/aces2.rs`),
/// whose reach-table sampler reproduces `reach_m_from_table`'s `i_lo = floor(h) + TABLE_FIRST_NOM`.
pub(crate) const TABLE_FIRST_NOM: usize = TABLE_BASE_INDEX;
const TABLE_LAST_NOM: usize = TABLE_UPPER_WRAP - 1;

#[derive(Debug, Clone)]
pub struct Table1D {
    pub data: [f32; TABLE_TOTAL_SIZE],
}

impl Default for Table1D {
    fn default() -> Self {
        Self {
            data: [0.0; TABLE_TOTAL_SIZE],
        }
    }
}

impl std::ops::Index<usize> for Table1D {
    type Output = f32;
    fn index(&self, i: usize) -> &f32 {
        &self.data[i]
    }
}
impl std::ops::IndexMut<usize> for Table1D {
    fn index_mut(&mut self, i: usize) -> &mut f32 {
        &mut self.data[i]
    }
}

impl Table1D {
    fn hue_pos_in_uniform(&self, hue: f32) -> usize {
        hue as usize
    }
    fn nominal_hue_pos(&self, hue: f32) -> usize {
        TABLE_FIRST_NOM + self.hue_pos_in_uniform(hue)
    }
}

#[derive(Debug, Clone)]
pub struct Table3D {
    pub data: [[f32; 3]; TABLE_TOTAL_SIZE],
}

impl Default for Table3D {
    fn default() -> Self {
        Self {
            data: [[0.0; 3]; TABLE_TOTAL_SIZE],
        }
    }
}

impl std::ops::Index<usize> for Table3D {
    type Output = [f32; 3];
    fn index(&self, i: usize) -> &[f32; 3] {
        &self.data[i]
    }
}
impl std::ops::IndexMut<usize> for Table3D {
    fn index_mut(&mut self, i: usize) -> &mut [f32; 3] {
        &mut self.data[i]
    }
}

// --- Parameter structs ---

#[derive(Debug, Clone)]
pub struct JMhParams {
    pub mat_rgb_to_cam16_c: M33,
    pub mat_cam16_c_to_rgb: M33,
    pub mat_cone_to_aab: M33,
    pub mat_aab_to_cone: M33,
    pub f_l_n: f32,
    pub cz: f32,
    pub inv_cz: f32,
    pub a_w_j: f32,
    pub inv_a_w_j: f32,
}

#[derive(Debug, Clone)]
pub struct ToneScaleParams {
    pub n: f32,
    pub n_r: f32,
    pub g: f32,
    pub t_1: f32,
    pub c_t: f32,
    pub s_2: f32,
    pub u_2: f32,
    pub m_2: f32,
    pub forward_limit: f32,
    pub inverse_limit: f32,
    pub log_peak: f32,
}

#[derive(Debug, Clone)]
pub struct SharedCompressionParams {
    pub limit_j_max: f32,
    pub model_gamma_inv: f32,
    pub reach_m_table: Table1D,
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedSharedParams {
    pub limit_j_max: f32,
    pub model_gamma_inv: f32,
    pub reach_max_m: f32,
}

#[derive(Debug, Clone)]
pub struct ChromaCompressParams {
    pub sat: f32,
    pub sat_thr: f32,
    pub compr: f32,
    pub chroma_compress_scale: f32,
}

#[derive(Debug, Clone)]
pub struct HueDependantGamutParams {
    pub gamma_bottom_inv: f32,
    pub jm_cusp: [f32; 2],
    pub gamma_top_inv: f32,
    pub focus_j: f32,
    pub analytical_threshold: f32,
}

#[derive(Debug, Clone)]
pub struct GamutCompressParams {
    pub mid_j: f32,
    pub focus_dist: f32,
    pub lower_hull_gamma_inv: f32,
    pub hue_linearity_search_range: [i32; 2],
    pub hue_table: Table1D,
    pub gamut_cusp_table: Table3D,
}

/// Full pre-computed ACES 2.0 Output Transform state.
#[derive(Debug, Clone)]
pub struct Aces2State {
    pub p_in: JMhParams,
    pub p_out: JMhParams,
    pub ts: ToneScaleParams,
    pub shared: SharedCompressionParams,
    pub chroma: ChromaCompressParams,
    pub gamut: GamutCompressParams,
}

// --- Matrix math ---

fn mult_f3_m33(v: &F3, m: &M33) -> F3 {
    [
        v[0] * m[0] + v[1] * m[1] + v[2] * m[2],
        v[0] * m[3] + v[1] * m[4] + v[2] * m[5],
        v[0] * m[6] + v[1] * m[7] + v[2] * m[8],
    ]
}

fn mult_m33_m33(a: &M33, b: &M33) -> M33 {
    [
        a[0] * b[0] + a[1] * b[3] + a[2] * b[6],
        a[0] * b[1] + a[1] * b[4] + a[2] * b[7],
        a[0] * b[2] + a[1] * b[5] + a[2] * b[8],
        a[3] * b[0] + a[4] * b[3] + a[5] * b[6],
        a[3] * b[1] + a[4] * b[4] + a[5] * b[7],
        a[3] * b[2] + a[4] * b[5] + a[5] * b[8],
        a[6] * b[0] + a[7] * b[3] + a[8] * b[6],
        a[6] * b[1] + a[7] * b[4] + a[8] * b[7],
        a[6] * b[2] + a[7] * b[5] + a[8] * b[8],
    ]
}

/// Scale rows of a 3x3 matrix by diagonal vector: diag(s) * m
fn scale_m33(m: &M33, s: &F3) -> M33 {
    [
        m[0] * s[0],
        m[1] * s[0],
        m[2] * s[0],
        m[3] * s[1],
        m[4] * s[1],
        m[5] * s[1],
        m[6] * s[2],
        m[7] * s[2],
        m[8] * s[2],
    ]
}

/// Narrow an f64 3x3 to f32, matching OCIO's `m33_from_ocio_matrix_array` (`MatrixLib.h:81-89`),
/// which casts the double-backed `MatrixArray` values to `float` on the way out.
fn narrow_m33(m: &[f64; 9]) -> M33 {
    std::array::from_fn(|i| m[i] as f32)
}

/// Invert a 3x3 matrix in f64, or report that it is singular.
///
/// OCIO's color pipeline inverts through `MatrixOpData::inverse()`, whose backing store is
/// `double` (`MatrixOpData.h:90` — `MatrixArray : ArrayDouble`), so the inverse is computed in
/// f64 and only narrowed to f32 at the `(float)` casts in `m33_from_ocio_matrix_array`. Doing
/// the whole 3x3 in f32 diverges from that by ~1e-7 per element. Those inverse matrices feed
/// `jmh_to_rgb`, which builds the reach-M and upper-hull-gamma tables; at a hue where the
/// gamut-compression gain is high (e.g. the near-cusp row at ~192.2°) the f32 rounding is
/// amplified past the 1e-5 golden tolerance. So the inversion itself must run in f64.
fn invert_f64(m: &[f64; 9]) -> Result<[f64; 9], OcioError> {
    let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6]);
    // Relative singularity test: a matrix that is singular in exact arithmetic (e.g. collinear
    // primaries, two identical rows) leaves a rounding residue around 1e-16 * scale^3, which an
    // absolute `< 1e-30` never caught, so garbage inverses went through. Real primary matrices
    // have |det| ~ scale^3. NaN (the all-zero placeholder primaries) compares false and still
    // yields a NaN inverse on purpose (see `rgb_to_xyz_f64`).
    let scale = m.iter().fold(0.0f64, |a, &v| a.max(v.abs()));
    if det.abs() < 1e-12 * scale * scale * scale {
        return Err(OcioError::InvalidTransform {
            reason: format!("ACES2: singular 3x3 matrix cannot be inverted (det = {det:e})"),
        });
    }
    let inv_det = 1.0 / det;
    Ok([
        (m[4] * m[8] - m[5] * m[7]) * inv_det,
        (m[2] * m[7] - m[1] * m[8]) * inv_det,
        (m[1] * m[5] - m[2] * m[4]) * inv_det,
        (m[5] * m[6] - m[3] * m[8]) * inv_det,
        (m[0] * m[8] - m[2] * m[6]) * inv_det,
        (m[2] * m[3] - m[0] * m[5]) * inv_det,
        (m[3] * m[7] - m[4] * m[6]) * inv_det,
        (m[1] * m[6] - m[0] * m[7]) * inv_det,
        (m[0] * m[4] - m[1] * m[3]) * inv_det,
    ])
}

/// Invert an f32 3x3 by widening to f64, inverting, and narrowing back — the exact shape of
/// OCIO's `invert_f33` (`MatrixLib.h:91-110`): float in, double inverse, float out. Used for the
/// CAM16-cone matrices that OCIO builds in f32 before inverting.
///
/// This used to `return *m` on a near-zero determinant, handing back an *un-inverted* matrix
/// that would then be used as if it were the inverse. That is unreachable for real display
/// primaries (which are always well conditioned), so it never produced a wrong pixel — but
/// it is the silent-fallback shape this codebase bans, and a latent one is still one.
fn invert_m33(m: &M33) -> Result<M33, OcioError> {
    let d: [f64; 9] = std::array::from_fn(|i| m[i] as f64);
    Ok(narrow_m33(&invert_f64(&d)?))
}

const IDENTITY_M33: M33 = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

// --- Color matrix helpers ---

/// Build the RGB-to-XYZ matrix for a set of primaries (CIE XYZ Illuminant E), in f64.
///
/// OCIO's `RGBtoXYZ_f33` (`ColorLib.h:18-23`) delegates to `build_conversion_matrix`, which
/// runs the whole normalized-primary-matrix construction — including its internal inverse — in
/// double and narrows to f32 only in `m33_from_ocio_matrix_array`. Building this in f32 (as we
/// did) narrows too early; the residual propagates through the table generation. So the
/// construction runs in f64 here and is narrowed by the callers at OCIO's narrowing points.
fn rgb_to_xyz_f64(p: &Primaries) -> Result<[f64; 9], OcioError> {
    // Normalized-primary-matrix construction in f64, narrowed by callers at OCIO's narrowing
    // points. This is the same NPM OCIO's `rgb2xyz_from_xy` (`ColorMatrixHelpers.cpp:184-227`)
    // builds — verified to narrow to a bit-identical f32 — and it deliberately keeps the
    // per-primary `x/y` normalization: for a degenerate (e.g. all-zero) primary set that yields
    // NaN entries rather than a clean singular matrix, so `invert_f64`'s `det < 1e-30` guard does
    // not fire (`NaN < 1e-30` is false) and the derivation returns NaN instead of an error —
    // preserving the long-standing behaviour that building an ACES2 op with empty/placeholder
    // primaries succeeds (the JMh params are simply unused for those ops) rather than panicking.
    let rx = p.red.x as f64;
    let ry = p.red.y as f64;
    let gx = p.green.x as f64;
    let gy = p.green.y as f64;
    let bx = p.blue.x as f64;
    let by = p.blue.y as f64;
    let wx = p.white.x as f64;
    let wy = p.white.y as f64;

    // XYZ of primaries (z = 1 - x - y), each column normalized so Y = 1.
    let rz = 1.0 - rx - ry;
    let gz = 1.0 - gx - gy;
    let bz = 1.0 - bx - by;
    let wz = 1.0 - wx - wy;
    let w_xyz = [wx / wy, 1.0, wz / wy];

    let m = [
        rx / ry,
        gx / gy,
        bx / by,
        1.0,
        1.0,
        1.0,
        rz / ry,
        gz / gy,
        bz / by,
    ];
    let m_inv = invert_f64(&m)?;
    // s = w_xyz * m_inv (row vector times matrix), all in f64.
    let s = [
        w_xyz[0] * m_inv[0] + w_xyz[1] * m_inv[1] + w_xyz[2] * m_inv[2],
        w_xyz[0] * m_inv[3] + w_xyz[1] * m_inv[4] + w_xyz[2] * m_inv[5],
        w_xyz[0] * m_inv[6] + w_xyz[1] * m_inv[7] + w_xyz[2] * m_inv[8],
    ];

    Ok([
        s[0] * m[0],
        s[1] * m[1],
        s[2] * m[2],
        s[0] * m[3],
        s[1] * m[4],
        s[2] * m[5],
        s[0] * m[6],
        s[1] * m[7],
        s[2] * m[8],
    ])
}

/// f32 RGB-to-XYZ, narrowed from the f64 build (OCIO narrows in `m33_from_ocio_matrix_array`).
fn rgb_to_xyz_m33(p: &Primaries) -> Result<M33, OcioError> {
    Ok(narrow_m33(&rgb_to_xyz_f64(p)?))
}

/// f32 XYZ-to-RGB. OCIO's `XYZtoRGB_f33` builds in double and inverts in double
/// (`ColorLib.h:25-30` — `...->inverse()`) before narrowing, so the inverse must be taken in f64
/// on the un-narrowed matrix, not on the already-narrowed f32 `rgb_to_xyz_m33`.
fn xyz_to_rgb_m33(p: &Primaries) -> Result<M33, OcioError> {
    Ok(narrow_m33(&invert_f64(&rgb_to_xyz_f64(p)?)?))
}

fn rgb_to_rgb_m33(src: &Primaries, dst: &Primaries) -> Result<M33, OcioError> {
    Ok(mult_m33_m33(&xyz_to_rgb_m33(dst)?, &rgb_to_xyz_m33(src)?))
}

// --- Hue utilities ---

fn wrap_to_hue_limit(hue: f32) -> f32 {
    let y = hue % HUE_LIMIT;
    if y < 0.0 { y + HUE_LIMIT } else { y }
}

fn to_radians(v: f32) -> f32 {
    PI * v / 180.0
}
fn from_radians_wrapped(v: f32) -> f32 {
    let y = 180.0 * v / PI;
    if y < 0.0 { y + HUE_LIMIT } else { y }
}

fn lerp_f3(a: &F3, b: &F3, t: f32) -> F3 {
    [
        lerpf(a[0], b[0], t),
        lerpf(a[1], b[1], t),
        lerpf(a[2], b[2], t),
    ]
}

fn midpoint_f(a: f32, b: f32) -> f32 {
    (a + b) * 0.5
}

// --- Table lookups ---

fn lookup_hue_interval(h: f32, hues: &Table1D, search_range: &[i32; 2]) -> usize {
    let i_start = hues.nominal_hue_pos(h);
    let mut i_lo = (i_start as i32 + search_range[0]).max(TABLE_LOWER_WRAP as i32) as usize;
    let mut i_hi = (i_start as i32 + search_range[1]).min(TABLE_UPPER_WRAP as i32) as usize;
    let mut i = i_start;

    while i_lo + 1 < i_hi {
        if h > hues[i] {
            i_lo = i;
        } else {
            i_hi = i;
        }
        i = (i_lo + i_hi) / 2;
    }
    i_hi.max(1)
}

fn interp_weight(h: f32, h_lo: f32, h_hi: f32) -> f32 {
    (h - h_lo) / (h_hi - h_lo)
}

fn cusp_from_table(i_hi: usize, t: f32, gt: &Table3D) -> F3 {
    lerp_f3(&gt[i_hi - 1], &gt[i_hi], t)
}

fn reach_m_from_table(h: f32, rt: &Table1D) -> f32 {
    let base = rt.hue_pos_in_uniform(h);
    let t = h - base as f32;
    let i_lo = base + TABLE_FIRST_NOM;
    let i_hi = i_lo + 1;
    lerpf(rt[i_lo], rt[i_hi], t)
}

// --- CAM16 core ---

fn post_adapt_compress_fwd_inner(rc: f32) -> f32 {
    let f_l_y = rc.powf(POST_ADAPT_POWER);
    f_l_y / (CAM_NL_OFFSET + f_l_y)
}

fn post_adapt_compress_inv_inner(ra: f32) -> f32 {
    let ra_lim = ra.min(POST_ADAPT_RA_LIMIT);
    let f_l_y = CAM_NL_OFFSET * ra_lim / (1.0 - ra_lim);
    f_l_y.powf(1.0 / POST_ADAPT_POWER)
}

fn post_adapt_compress_fwd(v: f32) -> f32 {
    let ra = post_adapt_compress_fwd_inner(v.abs());
    ra.copysign(v)
}

fn post_adapt_compress_inv(v: f32) -> f32 {
    let rc = post_adapt_compress_inv_inner(v.abs());
    rc.copysign(v)
}

fn achromatic_n_to_j(a: f32, cz: f32) -> f32 {
    J_SCALE * a.powf(cz)
}
fn j_to_achromatic_n(j: f32, inv_cz: f32) -> f32 {
    (j / J_SCALE).powf(inv_cz)
}

fn a_to_y(a: f32, p: &JMhParams) -> f32 {
    let ra = p.a_w_j * a;
    post_adapt_compress_inv_inner(ra) / p.f_l_n
}

fn j_to_y(j: f32, p: &JMhParams) -> f32 {
    a_to_y(j_to_achromatic_n(j, p.inv_cz), p)
}

fn y_to_j_inner(y: f32, p: &JMhParams) -> f32 {
    let ra = post_adapt_compress_fwd_inner(y * p.f_l_n);
    achromatic_n_to_j(ra * p.inv_a_w_j, p.cz)
}

pub fn y_to_j(y: f32, p: &JMhParams) -> f32 {
    let j = y_to_j_inner(y.abs(), p);
    j.copysign(y)
}

pub fn rgb_to_aab(rgb: &F3, p: &JMhParams) -> F3 {
    let rgb_m = mult_f3_m33(rgb, &p.mat_rgb_to_cam16_c);
    let rgb_a = [
        post_adapt_compress_fwd(rgb_m[0]),
        post_adapt_compress_fwd(rgb_m[1]),
        post_adapt_compress_fwd(rgb_m[2]),
    ];
    mult_f3_m33(&rgb_a, &p.mat_cone_to_aab)
}

pub fn aab_to_jmh(aab: &F3, p: &JMhParams) -> F3 {
    if aab[0] <= 0.0 {
        return [0.0, 0.0, 0.0];
    }
    let j = achromatic_n_to_j(aab[0], p.cz);
    let m = (aab[1] * aab[1] + aab[2] * aab[2]).sqrt();
    let h = from_radians_wrapped(aab[2].atan2(aab[1]));
    [j, m, h]
}

pub fn rgb_to_jmh(rgb: &F3, p: &JMhParams) -> F3 {
    aab_to_jmh(&rgb_to_aab(rgb, p), p)
}

pub fn jmh_to_aab_trig(jmh: &F3, cos_hr: f32, sin_hr: f32, p: &JMhParams) -> F3 {
    let a_n = j_to_achromatic_n(jmh[0], p.inv_cz);
    [a_n, jmh[1] * cos_hr, jmh[1] * sin_hr]
}

pub fn jmh_to_aab(jmh: &F3, p: &JMhParams) -> F3 {
    let h_rad = to_radians(jmh[2]);
    jmh_to_aab_trig(jmh, h_rad.cos(), h_rad.sin(), p)
}

pub fn aab_to_rgb(aab: &F3, p: &JMhParams) -> F3 {
    let rgb_a = mult_f3_m33(aab, &p.mat_aab_to_cone);
    let rgb_m = [
        post_adapt_compress_inv(rgb_a[0]),
        post_adapt_compress_inv(rgb_a[1]),
        post_adapt_compress_inv(rgb_a[2]),
    ];
    mult_f3_m33(&rgb_m, &p.mat_cam16_c_to_rgb)
}

pub fn jmh_to_rgb(jmh: &F3, p: &JMhParams) -> F3 {
    aab_to_rgb(&jmh_to_aab(jmh, p), p)
}

// --- Tonescale ---

fn aces_tonescale_fwd(y_in: f32, pt: &ToneScaleParams) -> f32 {
    let f = pt.m_2 * (y_in / (y_in + pt.s_2)).powf(pt.g);
    (f * f / (f + pt.t_1)).max(0.0) * pt.n_r
}

fn aces_tonescale_inv(y_in: f32, pt: &ToneScaleParams) -> f32 {
    let y_ts_norm = y_in / REFERENCE_LUMINANCE;
    let z = y_ts_norm.max(0.0).min(pt.inverse_limit);
    let f = (z + (z * (4.0 * pt.t_1 + z)).sqrt()) * 0.5;
    pt.s_2 / ((pt.m_2 / f).powf(1.0 / pt.g) - 1.0)
}

fn tonescale_fwd_j(j: f32, p: &JMhParams, pt: &ToneScaleParams) -> f32 {
    let j_abs = j.abs();
    let y_in = j_to_y(j_abs, p);
    let y_out = aces_tonescale_fwd(y_in, pt);
    let j_out = y_to_j_inner(y_out, p);
    j_out.copysign(j)
}

fn tonescale_inv_j(j: f32, p: &JMhParams, pt: &ToneScaleParams) -> f32 {
    let j_abs = j.abs();
    let y_in = j_to_y(j_abs, p);
    let y_out = aces_tonescale_inv(y_in, pt);
    let j_out = y_to_j_inner(y_out, p);
    j_out.copysign(j)
}

fn tonescale_a_to_j_fwd(a: f32, p: &JMhParams, pt: &ToneScaleParams) -> f32 {
    let y_in = a_to_y(a, p);
    let y_out = aces_tonescale_fwd(y_in, pt);
    let j_out = y_to_j_inner(y_out, p);
    j_out.copysign(a)
}

// --- Chroma compress ---

/// Fourier-series weights of the CAM16 chroma-normalisation curve `Mnorm(h)`.
///
/// Layout `[cos1, cos2, cos3, sin1, sin2, sin3, bias]`, matching the term order of
/// [`chroma_compress_norm`] and OCIO's `_Add_ChromaCompressionNorm_Shader`
/// (`FixedFunctionOpGPU.cpp:711-718`). Named here so the CPU oracle and the GPU emitter
/// (`gpu/aces2.rs`) read one source rather than each retyping the seven magic numbers — the
/// literal-vs-computed drift class this crate bans. Extracting the former inline literals is
/// bit-identical.
pub(crate) const CHROMA_NORM_WEIGHTS: [f32; 7] = [
    11.34072, 16.46899, 7.88380, 14.66441, -6.37224, 9.19364, 77.12896,
];

pub fn chroma_compress_norm(cos_hr: f32, sin_hr: f32, scale: f32) -> f32 {
    let cos2 = 2.0 * cos_hr * cos_hr - 1.0;
    let sin2 = 2.0 * cos_hr * sin_hr;
    let cos3 = 4.0 * cos_hr * cos_hr * cos_hr - 3.0 * cos_hr;
    let sin3 = 3.0 * sin_hr - 4.0 * sin_hr * sin_hr * sin_hr;

    let w = &CHROMA_NORM_WEIGHTS;
    let m = w[0] * cos_hr
        + w[1] * cos2
        + w[2] * cos3
        + w[3] * sin_hr
        + w[4] * sin2
        + w[5] * sin3
        + w[6];
    m * scale
}

fn toe_fwd(x: f32, limit: f32, k1_in: f32, k2_in: f32) -> f32 {
    if x > limit {
        return x;
    }
    let k2 = k2_in.max(0.001);
    let k1 = (k1_in * k1_in + k2 * k2).sqrt();
    let k3 = (limit + k1) / (limit + k2);
    let mb = k3 * x - k1;
    let mac = k2 * k3 * x;
    0.5 * (mb + (mb * mb + 4.0 * mac).sqrt())
}

fn toe_inv(x: f32, limit: f32, k1_in: f32, k2_in: f32) -> f32 {
    if x > limit {
        return x;
    }
    let k2 = k2_in.max(0.001);
    let k1 = (k1_in * k1_in + k2 * k2).sqrt();
    let k3 = (limit + k1) / (limit + k2);
    (x * x + k1 * x) / (k3 * (x + k2))
}

pub fn chroma_compress_fwd(
    jmh: &F3,
    j_ts: f32,
    mnorm: f32,
    pr: &ResolvedSharedParams,
    pc: &ChromaCompressParams,
) -> F3 {
    let (j, m, h) = (jmh[0], jmh[1], jmh[2]);
    let mut m_cp = m;

    if m != 0.0 {
        let nj = j_ts / pr.limit_j_max;
        let snj = (1.0 - nj).max(0.0);
        let limit = nj.powf(pr.model_gamma_inv) * pr.reach_max_m / mnorm;

        m_cp = m * (j_ts / j).powf(pr.model_gamma_inv);
        m_cp /= mnorm;
        m_cp = limit
            - toe_fwd(
                limit - m_cp,
                limit - 0.001,
                snj * pc.sat,
                (nj * nj + pc.sat_thr).sqrt(),
            );
        m_cp = toe_fwd(m_cp, limit, nj * pc.compr, snj);
        m_cp *= mnorm;
    }
    [j_ts, m_cp, h]
}

pub fn chroma_compress_inv(
    jmh: &F3,
    j: f32,
    mnorm: f32,
    pr: &ResolvedSharedParams,
    pc: &ChromaCompressParams,
) -> F3 {
    let (j_ts, m_cp, h) = (jmh[0], jmh[1], jmh[2]);
    let mut m = m_cp;

    if m_cp != 0.0 {
        let nj = j_ts / pr.limit_j_max;
        let snj = (1.0 - nj).max(0.0);
        let limit = nj.powf(pr.model_gamma_inv) * pr.reach_max_m / mnorm;

        m /= mnorm;
        m = toe_inv(m, limit, nj * pc.compr, snj);
        m = limit
            - toe_inv(
                limit - m,
                limit - 0.001,
                snj * pc.sat,
                (nj * nj + pc.sat_thr).sqrt(),
            );
        m *= mnorm;
        m *= (j_ts / j).powf(-pr.model_gamma_inv);
    }
    [j, m, h]
}

// --- Gamut compress ---

fn get_focus_gain(j: f32, threshold: f32, limit_j_max: f32, focus_dist: f32) -> f32 {
    let mut gain = limit_j_max * focus_dist;
    if j > threshold {
        let adj = ((limit_j_max - threshold) / (limit_j_max - j).max(0.0001)).log10();
        let adj = adj * adj + 1.0;
        gain *= adj;
    }
    gain
}

fn solve_j_intersect(j: f32, m: f32, focus_j: f32, max_j: f32, slope_gain: f32) -> f32 {
    let m_scaled = m / slope_gain;
    let a = m_scaled / focus_j;

    if j < focus_j {
        let b = 1.0 - m_scaled;
        let c = -j;
        let det = b * b - 4.0 * a * c;
        -2.0 * c / (b + det.sqrt())
    } else {
        let b = -(1.0 + m_scaled + max_j * a);
        let c = max_j * m_scaled + j;
        let det = b * b - 4.0 * a * c;
        -2.0 * c / (b - det.sqrt())
    }
}

fn smin_scaled(a: f32, b: f32, scale_ref: f32) -> f32 {
    let s_scaled = SMOOTH_CUSPS * scale_ref;
    let h = (s_scaled - (a - b).abs()).max(0.0) / s_scaled;
    a.min(b) - h * h * h * s_scaled * (1.0 / 6.0)
}

fn compute_compression_slope(
    intersect_j: f32,
    focus_j: f32,
    limit_j_max: f32,
    slope_gain: f32,
) -> f32 {
    let dir = if intersect_j < focus_j {
        intersect_j
    } else {
        limit_j_max - intersect_j
    };
    dir * (intersect_j - focus_j) / (focus_j * slope_gain)
}

fn estimate_line_boundary_m(
    j_intersect: f32,
    slope: f32,
    inv_gamma: f32,
    j_max: f32,
    m_max: f32,
    j_ref: f32,
) -> f32 {
    let nj = j_intersect / j_ref;
    let shifted = j_ref * nj.powf(inv_gamma);
    shifted * m_max / (j_max - slope * m_max)
}

fn find_gamut_boundary_m(
    jm_cusp: &[f32; 2],
    j_max: f32,
    gamma_top_inv: f32,
    gamma_bottom_inv: f32,
    j_intersect_src: f32,
    slope: f32,
    j_intersect_cusp: f32,
) -> f32 {
    let m_lower = estimate_line_boundary_m(
        j_intersect_src,
        slope,
        gamma_bottom_inv,
        jm_cusp[0],
        jm_cusp[1],
        j_intersect_cusp,
    );
    let f_j_cusp = j_max - j_intersect_cusp;
    let f_j_src = j_max - j_intersect_src;
    let f_jm_j = j_max - jm_cusp[0];
    let m_upper =
        estimate_line_boundary_m(f_j_src, -slope, gamma_top_inv, f_jm_j, jm_cusp[1], f_j_cusp);
    smin_scaled(m_lower, m_upper, jm_cusp[1])
}

fn reinhard_fwd(scale: f32, nd: f32) -> f32 {
    scale * nd / (1.0 + nd)
}
fn reinhard_inv(scale: f32, nd: f32) -> f32 {
    if nd >= 1.0 {
        scale
    } else {
        scale * -(nd / (nd - 1.0))
    }
}

fn remap_m_fwd(m: f32, gamut_m: f32, reach_m: f32) -> f32 {
    let ratio = gamut_m / reach_m;
    let proportion = ratio.max(COMPRESSION_THRESHOLD);
    let threshold = proportion * gamut_m;
    if m <= threshold || proportion >= 1.0 {
        return m;
    }
    let m_off = m - threshold;
    let gamut_off = gamut_m - threshold;
    let reach_off = reach_m - threshold;
    let scale = reach_off / (reach_off / gamut_off - 1.0);
    threshold + reinhard_fwd(scale, m_off / scale)
}

fn remap_m_inv(m: f32, gamut_m: f32, reach_m: f32) -> f32 {
    let ratio = gamut_m / reach_m;
    let proportion = ratio.max(COMPRESSION_THRESHOLD);
    let threshold = proportion * gamut_m;
    if m <= threshold || proportion >= 1.0 {
        return m;
    }
    let m_off = m - threshold;
    let gamut_off = gamut_m - threshold;
    let reach_off = reach_m - threshold;
    let scale = reach_off / (reach_off / gamut_off - 1.0);
    threshold + reinhard_inv(scale, m_off / scale)
}

fn compress_gamut_core<const INV: bool>(
    jmh: &F3,
    jx: f32,
    sr: &ResolvedSharedParams,
    p: &GamutCompressParams,
    hdp: &HueDependantGamutParams,
) -> F3 {
    let (j, m, h) = (jmh[0], jmh[1], jmh[2]);

    let slope_gain = get_focus_gain(jx, hdp.analytical_threshold, sr.limit_j_max, p.focus_dist);
    let j_intersect_src = solve_j_intersect(j, m, hdp.focus_j, sr.limit_j_max, slope_gain);
    let gamut_slope =
        compute_compression_slope(j_intersect_src, hdp.focus_j, sr.limit_j_max, slope_gain);
    let j_intersect_cusp = solve_j_intersect(
        hdp.jm_cusp[0],
        hdp.jm_cusp[1],
        hdp.focus_j,
        sr.limit_j_max,
        slope_gain,
    );
    let gamut_m = find_gamut_boundary_m(
        &hdp.jm_cusp,
        sr.limit_j_max,
        hdp.gamma_top_inv,
        hdp.gamma_bottom_inv,
        j_intersect_src,
        gamut_slope,
        j_intersect_cusp,
    );

    if gamut_m <= 0.0 {
        return [j, 0.0, h];
    }

    let reach_m = estimate_line_boundary_m(
        j_intersect_src,
        gamut_slope,
        sr.model_gamma_inv,
        sr.limit_j_max,
        sr.reach_max_m,
        sr.limit_j_max,
    );
    let remapped = if INV {
        remap_m_inv(m, gamut_m, reach_m)
    } else {
        remap_m_fwd(m, gamut_m, reach_m)
    };

    [j_intersect_src + remapped * gamut_slope, remapped, h]
}

fn compute_focus_j(cusp_j: f32, mid_j: f32, limit_j_max: f32) -> f32 {
    lerpf(
        cusp_j,
        mid_j,
        (CUSP_MID_BLEND - cusp_j / limit_j_max).min(1.0),
    )
}

fn init_hue_gamut_params(
    hue: f32,
    sr: &ResolvedSharedParams,
    p: &GamutCompressParams,
) -> HueDependantGamutParams {
    let i_hi = lookup_hue_interval(hue, &p.hue_table, &p.hue_linearity_search_range);
    let t = interp_weight(hue, p.hue_table[i_hi - 1], p.hue_table[i_hi]);
    let cusp = cusp_from_table(i_hi, t, &p.gamut_cusp_table);

    let jm_cusp = [cusp[0], cusp[1]];
    let gamma_top_inv = cusp[2];
    let focus_j = compute_focus_j(jm_cusp[0], p.mid_j, sr.limit_j_max);
    let threshold = lerpf(jm_cusp[0], sr.limit_j_max, FOCUS_GAIN_BLEND);

    HueDependantGamutParams {
        gamma_bottom_inv: p.lower_hull_gamma_inv,
        jm_cusp,
        gamma_top_inv,
        focus_j,
        analytical_threshold: threshold,
    }
}

pub fn gamut_compress_fwd(jmh: &F3, sr: &ResolvedSharedParams, p: &GamutCompressParams) -> F3 {
    let (j, m, h) = (jmh[0], jmh[1], jmh[2]);
    if j <= 0.0 {
        return [0.0, 0.0, h];
    }
    if m <= 0.0 || j > sr.limit_j_max {
        return [j, 0.0, h];
    }
    let hdp = init_hue_gamut_params(h, sr, p);
    compress_gamut_core::<false>(jmh, j, sr, p, &hdp)
}

pub fn gamut_compress_inv(jmh: &F3, sr: &ResolvedSharedParams, p: &GamutCompressParams) -> F3 {
    let (j, m, h) = (jmh[0], jmh[1], jmh[2]);
    if j <= 0.0 {
        return [0.0, 0.0, h];
    }
    if m <= 0.0 || j > sr.limit_j_max {
        return [j, 0.0, h];
    }
    let hdp = init_hue_gamut_params(h, sr, p);
    let mut jx = j;
    if jx > hdp.analytical_threshold {
        jx = compress_gamut_core::<true>(jmh, jx, sr, p, &hdp)[0];
    }
    compress_gamut_core::<true>(jmh, jx, sr, p, &hdp)
}

// --- Initialization ---

fn model_gamma() -> f32 {
    SURROUND[1] * (1.48 + (Y_B / REFERENCE_LUMINANCE).sqrt())
}

/// Derive the CAM16 JMh parameters for a set of primaries.
///
/// # Panics
///
/// Panics if `prims` is degenerate enough that one of the derived matrices is singular.
/// That cannot happen for any real set of display primaries — three distinct chromaticities
/// and a white point always give a well-conditioned matrix — so reaching it means the
/// primaries are corrupt at the source, which is a programming error rather than bad user
/// data. Use [`try_init_jmh_params`] where the primaries come from a config file or any
/// other untrusted input and an error is the appropriate response.
///
/// This wrapper exists so the fallible derivation is available without forcing a signature
/// change on the six call sites in `processor/op.rs`, which belong to another work lane.
pub fn init_jmh_params(prims: &Primaries) -> JMhParams {
    match try_init_jmh_params(prims) {
        Ok(p) => p,
        Err(e) => panic!("ACES2: degenerate primaries {prims:?} — {e}"),
    }
}

/// Fallible form of [`init_jmh_params`]. See that function for when to prefer which.
pub fn try_init_jmh_params(prims: &Primaries) -> Result<JMhParams, OcioError> {
    let base_cone_to_aab: M33 = [
        2.0,
        1.0,
        1.0 / 20.0,
        1.0,
        -12.0 / 11.0,
        1.0 / 11.0,
        1.0 / 9.0,
        1.0 / 9.0,
        -2.0 / 9.0,
    ];

    let mat16 = xyz_to_rgb_m33(&CAM16_PRIMARIES)?;
    let rgb_to_xyz = rgb_to_xyz_m33(prims)?;
    let xyz_w = mult_f3_m33(&[REFERENCE_LUMINANCE; 3], &rgb_to_xyz);
    let y_w = xyz_w[1];
    let rgb_w = mult_f3_m33(&xyz_w, &mat16);

    let k = 1.0 / (5.0 * L_A + 1.0);
    let k4 = k * k * k * k;
    let f_l = 0.2 * k4 * (5.0 * L_A) + 0.1 * (1.0 - k4).powi(2) * (5.0 * L_A).powf(1.0 / 3.0);
    let f_l_n = f_l / REFERENCE_LUMINANCE;
    let cz = model_gamma();
    let inv_cz = 1.0 / cz;

    let d_rgb = [
        f_l_n * y_w / rgb_w[0],
        f_l_n * y_w / rgb_w[1],
        f_l_n * y_w / rgb_w[2],
    ];

    let rgb_wc = [
        d_rgb[0] * rgb_w[0],
        d_rgb[1] * rgb_w[1],
        d_rgb[2] * rgb_w[2],
    ];
    let rgb_aw = [
        post_adapt_compress_fwd(rgb_wc[0]),
        post_adapt_compress_fwd(rgb_wc[1]),
        post_adapt_compress_fwd(rgb_wc[2]),
    ];

    let cam_nl_s = [CAM_NL_SCALE; 3]; // actually this is the scaling used in OCIO
    // The OCIO code: scale_f33(Identity_M33, f3_from_f(cam_nl_scale))
    // which scales rows by cam_nl_scale
    let cone_to_aab = mult_m33_m33(&scale_m33(&IDENTITY_M33, &cam_nl_s), &base_cone_to_aab);

    let a_w = cone_to_aab[0] * rgb_aw[0] + cone_to_aab[1] * rgb_aw[1] + cone_to_aab[2] * rgb_aw[2];
    let a_w_j = post_adapt_compress_fwd_inner(f_l);
    let inv_a_w_j = 1.0 / a_w_j;

    let mat_rgb_to_cam16 = mult_m33_m33(
        &rgb_to_rgb_m33(prims, &CAM16_PRIMARIES)?,
        &scale_m33(&IDENTITY_M33, &[REFERENCE_LUMINANCE; 3]),
    );
    let mat_rgb_to_cam16_c = mult_m33_m33(&scale_m33(&IDENTITY_M33, &d_rgb), &mat_rgb_to_cam16);
    let mat_cam16_c_to_rgb = invert_m33(&mat_rgb_to_cam16_c)?;

    let mat_cone_to_aab = [
        cone_to_aab[0] / a_w,
        cone_to_aab[1] / a_w,
        cone_to_aab[2] / a_w,
        cone_to_aab[3] * 43.0 * SURROUND[2],
        cone_to_aab[4] * 43.0 * SURROUND[2],
        cone_to_aab[5] * 43.0 * SURROUND[2],
        cone_to_aab[6] * 43.0 * SURROUND[2],
        cone_to_aab[7] * 43.0 * SURROUND[2],
        cone_to_aab[8] * 43.0 * SURROUND[2],
    ];
    let mat_aab_to_cone = invert_m33(&mat_cone_to_aab)?;

    Ok(JMhParams {
        mat_rgb_to_cam16_c,
        mat_cam16_c_to_rgb,
        mat_cone_to_aab,
        mat_aab_to_cone,
        f_l_n,
        cz,
        inv_cz,
        a_w_j,
        inv_a_w_j,
    })
}

pub fn init_tonescale_params(peak_lum: f32) -> ToneScaleParams {
    let n = peak_lum;
    let n_r = 100.0;
    let g = 1.15;
    let c = 0.18;
    let c_d = 10.013;
    let w_g = 0.14;
    let t_1 = 0.04;
    let r_hit_min = 128.0;
    let r_hit_max = 896.0;

    let r_hit = r_hit_min + (r_hit_max - r_hit_min) * ((n / n_r).ln() / (10000.0_f32 / 100.0).ln());
    let m_0 = n / n_r;
    let m_1 = 0.5 * (m_0 + (m_0 * (m_0 + 4.0 * t_1)).sqrt());
    let u = ((r_hit / m_1) / (r_hit / m_1 + 1.0)).powf(g);
    let _m = m_1 / u;
    let w_i = (n / 100.0).log2();
    let c_t = c_d / n_r * (1.0 + w_i * w_g);
    let g_ip = 0.5 * (c_t + (c_t * (c_t + 4.0 * t_1)).sqrt());
    let g_ipp2 = -(m_1 * (g_ip / _m).powf(1.0 / g)) / ((g_ip / _m).powf(1.0 / g) - 1.0);
    let w_2 = c / g_ipp2;
    let s_2 = w_2 * m_1 * REFERENCE_LUMINANCE;
    let u_2 = ((r_hit / m_1) / (r_hit / m_1 + w_2)).powf(g);
    let m_2 = m_1 / u_2;
    let inverse_limit = n / (u_2 * n_r);
    let forward_limit = 8.0 * r_hit;
    let log_peak = (n / n_r).log10();

    ToneScaleParams {
        n,
        n_r,
        g,
        t_1,
        c_t,
        s_2,
        u_2,
        m_2,
        forward_limit,
        inverse_limit,
        log_peak,
    }
}

pub fn init_shared_compression_params(
    peak_lum: f32,
    input_params: &JMhParams,
    reach_params: &JMhParams,
) -> SharedCompressionParams {
    let limit_j_max = y_to_j(peak_lum, input_params);
    let model_gamma_inv = 1.0 / model_gamma();
    SharedCompressionParams {
        limit_j_max,
        model_gamma_inv,
        reach_m_table: make_reach_m_table(reach_params, limit_j_max),
    }
}

pub fn resolve_compression_params(hue: f32, p: &SharedCompressionParams) -> ResolvedSharedParams {
    ResolvedSharedParams {
        limit_j_max: p.limit_j_max,
        model_gamma_inv: p.model_gamma_inv,
        reach_max_m: reach_m_from_table(hue, &p.reach_m_table),
    }
}

pub fn init_chroma_compress_params(peak_lum: f32, ts: &ToneScaleParams) -> ChromaCompressParams {
    let compr = CHROMA_COMPRESS + CHROMA_COMPRESS * CHROMA_COMPRESS_FACT * ts.log_peak;
    let sat = (CHROMA_EXPAND - CHROMA_EXPAND * CHROMA_EXPAND_FACT * ts.log_peak).max(0.2);
    let sat_thr = CHROMA_EXPAND_THR / ts.n;
    let ccs = (0.03379 * peak_lum).powf(0.30596) - 0.45135;
    ChromaCompressParams {
        sat,
        sat_thr,
        compr,
        chroma_compress_scale: ccs,
    }
}

// --- Reference-precision (f64) conversions for table generation ---
//
// OCIO's golden gamut-compress values are the double-precision ACES reference (the CTL/CTL-derived
// output the FixedFunctionOpCPU tests anchor to). The cusp- and reach-table searches are the point
// where an f32 evaluation of `rgb_to_jmh`/`jmh_to_rgb` measurably diverges from that reference:
// e.g. at hue ~192.2° the f32 cusp M lands ~5e-4 off the double value, which — because that hue's
// J sits next to `focus_j`, an ill-conditioned point of `solve_j_intersect` — is amplified ~10x in
// the forward gamut boundary and pushes both the inverse anchor and the near-black output-transform
// channel past the 1e-5 golden tolerance. Generating the tables in f64 (narrowing only into the f32
// table storage) removes that loss so the tables match the reference. Runtime evaluation stays f32.
//
// These widen the already-f64-derived f32 param matrices back to f64; the matrices carry ~1e-7
// rounding, far below the table tolerances, so this recovers reference-accurate table entries.

type D3 = [f64; 3];

fn mult_d3_m33(v: &D3, m: &M33) -> D3 {
    [
        v[0] * m[0] as f64 + v[1] * m[1] as f64 + v[2] * m[2] as f64,
        v[0] * m[3] as f64 + v[1] * m[4] as f64 + v[2] * m[5] as f64,
        v[0] * m[6] as f64 + v[1] * m[7] as f64 + v[2] * m[8] as f64,
    ]
}

fn post_adapt_fwd_inner_d(rc: f64) -> f64 {
    let y = rc.powf(POST_ADAPT_POWER as f64);
    y / (CAM_NL_OFFSET as f64 + y)
}
fn post_adapt_inv_inner_d(ra: f64) -> f64 {
    let r = ra.min(POST_ADAPT_RA_LIMIT as f64);
    let y = CAM_NL_OFFSET as f64 * r / (1.0 - r);
    y.powf(1.0 / POST_ADAPT_POWER as f64)
}
fn post_adapt_fwd_d(v: f64) -> f64 {
    post_adapt_fwd_inner_d(v.abs()).copysign(v)
}
fn post_adapt_inv_d(v: f64) -> f64 {
    post_adapt_inv_inner_d(v.abs()).copysign(v)
}
fn achromatic_n_to_j_d(a: f64, cz: f64) -> f64 {
    J_SCALE as f64 * a.powf(cz)
}
fn j_to_achromatic_n_d(j: f64, inv_cz: f64) -> f64 {
    (j / J_SCALE as f64).powf(inv_cz)
}

/// f64 `rgb_to_aab` (widened matrices).
fn rgb_to_aab_d(rgb: &D3, p: &JMhParams) -> D3 {
    let rgb_m = mult_d3_m33(rgb, &p.mat_rgb_to_cam16_c);
    let rgb_a = [
        post_adapt_fwd_d(rgb_m[0]),
        post_adapt_fwd_d(rgb_m[1]),
        post_adapt_fwd_d(rgb_m[2]),
    ];
    mult_d3_m33(&rgb_a, &p.mat_cone_to_aab)
}

/// f64 `rgb_to_jmh` (hue in degrees).
fn rgb_to_jmh_d(rgb: &D3, p: &JMhParams) -> D3 {
    let aab = rgb_to_aab_d(rgb, p);
    if aab[0] <= 0.0 {
        return [0.0, 0.0, 0.0];
    }
    let j = achromatic_n_to_j_d(aab[0], p.cz as f64);
    let m = (aab[1] * aab[1] + aab[2] * aab[2]).sqrt();
    let mut h = 180.0 * aab[2].atan2(aab[1]) / std::f64::consts::PI;
    if h < 0.0 {
        h += HUE_LIMIT as f64;
    }
    [j, m, h]
}

/// f64 `jmh_to_rgb` (hue in degrees).
fn jmh_to_rgb_d(jmh: &D3, p: &JMhParams) -> D3 {
    let h_rad = std::f64::consts::PI * jmh[2] / 180.0;
    let a_n = j_to_achromatic_n_d(jmh[0], p.inv_cz as f64);
    let aab = [a_n, jmh[1] * h_rad.cos(), jmh[1] * h_rad.sin()];
    let rgb_a = mult_d3_m33(&aab, &p.mat_aab_to_cone);
    let rgb_m = [
        post_adapt_inv_d(rgb_a[0]),
        post_adapt_inv_d(rgb_a[1]),
        post_adapt_inv_d(rgb_a[2]),
    ];
    mult_d3_m33(&rgb_m, &p.mat_cam16_c_to_rgb)
}

// --- Table generation ---

fn gen_unit_cube_corner(corner: usize) -> F3 {
    [
        if ((corner + 1) % CUSP_CORNER_COUNT) < 3 {
            1.0
        } else {
            0.0
        },
        if ((corner + 5) % CUSP_CORNER_COUNT) < 3 {
            1.0
        } else {
            0.0
        },
        if ((corner + 3) % CUSP_CORNER_COUNT) < 3 {
            1.0
        } else {
            0.0
        },
    ]
}

fn build_limiting_cusp_corners(
    params: &JMhParams,
    peak_lum: f32,
) -> ([D3; TOTAL_CORNER_COUNT], [D3; TOTAL_CORNER_COUNT]) {
    let mut tmp_rgb = [[0.0f64; 3]; CUSP_CORNER_COUNT];
    let mut tmp_jmh = [[0.0f64; 3]; CUSP_CORNER_COUNT];
    let mut min_idx = 0usize;
    let scale = peak_lum as f64 / REFERENCE_LUMINANCE as f64;

    for i in 0..CUSP_CORNER_COUNT {
        let c = gen_unit_cube_corner(i);
        tmp_rgb[i] = [
            scale * c[0] as f64,
            scale * c[1] as f64,
            scale * c[2] as f64,
        ];
        tmp_jmh[i] = rgb_to_jmh_d(&tmp_rgb[i], params);
        if tmp_jmh[i][2] < tmp_jmh[min_idx][2] {
            min_idx = i;
        }
    }

    let mut rgb_corners = [[0.0f64; 3]; TOTAL_CORNER_COUNT];
    let mut jmh_corners = [[0.0f64; 3]; TOTAL_CORNER_COUNT];
    for i in 0..CUSP_CORNER_COUNT {
        rgb_corners[i + 1] = tmp_rgb[(i + min_idx) % CUSP_CORNER_COUNT];
        jmh_corners[i + 1] = tmp_jmh[(i + min_idx) % CUSP_CORNER_COUNT];
    }
    rgb_corners[0] = rgb_corners[CUSP_CORNER_COUNT];
    rgb_corners[CUSP_CORNER_COUNT + 1] = rgb_corners[1];
    jmh_corners[0] = jmh_corners[CUSP_CORNER_COUNT];
    jmh_corners[CUSP_CORNER_COUNT + 1] = jmh_corners[1];
    jmh_corners[0][2] -= HUE_LIMIT as f64;
    jmh_corners[CUSP_CORNER_COUNT + 1][2] += HUE_LIMIT as f64;

    (rgb_corners, jmh_corners)
}

fn find_reach_corners(params: &JMhParams, limit_j: f32, max_src: f32) -> [D3; TOTAL_CORNER_COUNT] {
    let limit_a = j_to_achromatic_n_d(limit_j as f64, params.inv_cz as f64);
    let mut tmp = [[0.0f64; 3]; CUSP_CORNER_COUNT];
    let mut min_idx = 0usize;

    for i in 0..CUSP_CORNER_COUNT {
        let v = gen_unit_cube_corner(i);
        let (v0, v1, v2) = (v[0] as f64, v[1] as f64, v[2] as f64);
        let mut lower = 0.0f64;
        let mut upper = max_src as f64;
        while upper - lower > REACH_CUSP_TOLERANCE as f64 {
            let test = (lower + upper) * 0.5;
            let tc = [test * v0, test * v1, test * v2];
            let a = rgb_to_aab_d(&tc, params)[0];
            if a < limit_a {
                lower = test;
            } else {
                upper = test;
            }
            if a == limit_a {
                break;
            }
        }
        let uc = [upper * v0, upper * v1, upper * v2];
        tmp[i] = rgb_to_jmh_d(&uc, params);
        if tmp[i][2] < tmp[min_idx][2] {
            min_idx = i;
        }
    }

    let mut corners = [[0.0f64; 3]; TOTAL_CORNER_COUNT];
    for i in 0..CUSP_CORNER_COUNT {
        corners[i + 1] = tmp[(i + min_idx) % CUSP_CORNER_COUNT];
    }
    corners[0] = corners[CUSP_CORNER_COUNT];
    corners[CUSP_CORNER_COUNT + 1] = corners[1];
    corners[0][2] -= HUE_LIMIT as f64;
    corners[CUSP_CORNER_COUNT + 1][2] += HUE_LIMIT as f64;
    corners
}

fn extract_sorted_cube_hues(
    reach: &[D3; TOTAL_CORNER_COUNT],
    display: &[D3; TOTAL_CORNER_COUNT],
) -> (Vec<f64>, usize) {
    let mut sorted = vec![0.0f64; MAX_SORTED_CORNERS];
    let mut idx = 0;
    let mut ri = 1;
    let mut di = 1;

    while ri < CUSP_CORNER_COUNT + 1 || di < CUSP_CORNER_COUNT + 1 {
        let rh = if ri < CUSP_CORNER_COUNT + 1 {
            reach[ri][2]
        } else {
            f64::MAX
        };
        let dh = if di < CUSP_CORNER_COUNT + 1 {
            display[di][2]
        } else {
            f64::MAX
        };

        if (rh - dh).abs() < 1e-10 {
            sorted[idx] = rh;
            ri += 1;
            di += 1;
        } else if rh < dh {
            sorted[idx] = rh;
            ri += 1;
        } else {
            sorted[idx] = dh;
            di += 1;
        }
        idx += 1;
    }
    (sorted, idx)
}

fn build_hue_table(hue_table: &mut Table1D, sorted_hues: &[f64], unique_hues: usize) {
    let ideal_spacing = TABLE_NOMINAL_SIZE as f64 / HUE_LIMIT as f64;
    let mut samples_count = [0u32; 2 * CUSP_CORNER_COUNT + 2];
    let mut last_idx = u32::MAX;
    let mut min_index: u32 = if sorted_hues[0] == 0.0 { 0 } else { 1 };

    for hue_idx in 0..unique_hues {
        let mut nominal = ((sorted_hues[hue_idx] * ideal_spacing).round() as u32)
            .max(min_index)
            .min(TABLE_NOMINAL_SIZE as u32 - 1);

        if last_idx == nominal {
            if hue_idx > 1 && samples_count[hue_idx - 2] != samples_count[hue_idx - 1] - 1 {
                samples_count[hue_idx - 1] -= 1;
            } else {
                nominal += 1;
            }
        }
        samples_count[hue_idx] = nominal.min(TABLE_NOMINAL_SIZE as u32 - 1);
        last_idx = nominal;
        min_index = nominal;
    }

    let mut total = 0u32;
    // First interval
    let cnt0 = samples_count[0];
    for s in 0..cnt0 {
        hue_table[total as usize + s as usize + 1] =
            (sorted_hues[0] * s as f64 / cnt0 as f64) as f32;
    }
    total += cnt0;

    for i in 1..unique_hues {
        let samples = samples_count[i] - samples_count[i - 1];
        let lower = sorted_hues[i - 1];
        let upper = sorted_hues[i];
        let delta = (upper - lower) / samples as f64;
        for s in 0..samples {
            hue_table[total as usize + s as usize + 1] = (lower + s as f64 * delta) as f32;
        }
        total += samples;
    }

    // Last interval
    let remaining = TABLE_NOMINAL_SIZE as u32 - total;
    let lower = sorted_hues[unique_hues - 1];
    let delta = (HUE_LIMIT as f64 - lower) / remaining as f64;
    for s in 0..remaining {
        hue_table[total as usize + s as usize + 1] = (lower + s as f64 * delta) as f32;
    }

    hue_table[TABLE_LOWER_WRAP] = hue_table[TABLE_LAST_NOM] - HUE_LIMIT;
    hue_table[TABLE_UPPER_WRAP] = hue_table[TABLE_FIRST_NOM] + HUE_LIMIT;
    // Second upper wrap slot — required for the `lerp(table[i_hi-1],
    // table[i_hi])` lookup when `i_hi` reaches `TABLE_UPPER_WRAP + 1`.
    // Mirrors OCIO C++ `Transform.cpp:733-735`.
    hue_table[TABLE_UPPER_WRAP + 1] = hue_table[TABLE_FIRST_NOM + 1] + HUE_LIMIT;
}

fn find_display_cusp_for_hue(
    hue: f64,
    rgb_corners: &[D3; TOTAL_CORNER_COUNT],
    jmh_corners: &[D3; TOTAL_CORNER_COUNT],
    params: &JMhParams,
    prev: &mut [f64; 2],
) -> [f64; 2] {
    let mut upper_corner = 1;
    for (i, corner) in jmh_corners.iter().enumerate().skip(1) {
        if corner[2] > hue {
            upper_corner = i;
            break;
        }
    }
    let lower_corner = upper_corner - 1;

    if jmh_corners[lower_corner][2] == hue {
        return [jmh_corners[lower_corner][0], jmh_corners[lower_corner][1]];
    }

    let cl = &rgb_corners[lower_corner];
    let cu = &rgb_corners[upper_corner];
    let lerp_edge = |t: f64| -> D3 {
        [
            cl[0] + (cu[0] - cl[0]) * t,
            cl[1] + (cu[1] - cl[1]) * t,
            cl[2] + (cu[2] - cl[2]) * t,
        ]
    };
    let mut lower_t = if upper_corner == prev[0] as usize {
        prev[1]
    } else {
        0.0
    };
    let mut upper_t = 1.0f64;

    while upper_t - lower_t > DISPLAY_CUSP_TOLERANCE as f64 {
        let sample_t = (lower_t + upper_t) * 0.5;
        let jmh = rgb_to_jmh_d(&lerp_edge(sample_t), params);
        if jmh[2] < jmh_corners[lower_corner][2] {
            upper_t = sample_t;
        } else if jmh[2] >= jmh_corners[upper_corner][2] {
            lower_t = sample_t;
        } else if jmh[2] > hue {
            upper_t = sample_t;
        } else {
            lower_t = sample_t;
        }
    }

    let sample_t = (lower_t + upper_t) * 0.5;
    let jmh = rgb_to_jmh_d(&lerp_edge(sample_t), params);
    prev[0] = upper_corner as f64;
    prev[1] = sample_t;
    [jmh[0], jmh[1]]
}

fn build_cusp_table(
    hue_table: &Table1D,
    rgb_corners: &[D3; TOTAL_CORNER_COUNT],
    jmh_corners: &[D3; TOTAL_CORNER_COUNT],
    params: &JMhParams,
) -> Table3D {
    let mut prev = [0.0f64; 2];
    let mut out = Table3D::default();
    let smooth = 1.0 + SMOOTH_M as f64 * SMOOTH_CUSPS as f64;
    for i in TABLE_FIRST_NOM..TABLE_UPPER_WRAP {
        let hue = hue_table[i];
        let jm = find_display_cusp_for_hue(hue as f64, rgb_corners, jmh_corners, params, &mut prev);
        out[i] = [jm[0] as f32, (jm[1] * smooth) as f32, hue];
    }
    out[TABLE_LOWER_WRAP] = [
        out[TABLE_LAST_NOM][0],
        out[TABLE_LAST_NOM][1],
        hue_table[TABLE_LOWER_WRAP],
    ];
    out[TABLE_UPPER_WRAP] = [
        out[TABLE_FIRST_NOM][0],
        out[TABLE_FIRST_NOM][1],
        hue_table[TABLE_UPPER_WRAP],
    ];
    // Second upper-wrap slot, mirrors OCIO `Transform.cpp:830-832`.
    // Needed for `lerp(out[i_hi-1], out[i_hi])` at the 359°→0° hue
    // boundary; otherwise interpolation reads a zero-init slot.
    out[TABLE_UPPER_WRAP + 1] = [
        out[TABLE_FIRST_NOM + 1][0],
        out[TABLE_FIRST_NOM + 1][1],
        hue_table[TABLE_UPPER_WRAP + 1],
    ];
    out
}

fn outside_hull(rgb: &F3, max_val: f32) -> bool {
    rgb[0] > max_val || rgb[1] > max_val || rgb[2] > max_val
}
fn any_below_zero_d(rgb: &D3) -> bool {
    rgb[0] < 0.0 || rgb[1] < 0.0 || rgb[2] < 0.0
}

fn make_reach_m_table(params: &JMhParams, limit_j_max: f32) -> Table1D {
    let mut table = Table1D::default();
    let ljm = limit_j_max as f64;
    for i in 0..TABLE_NOMINAL_SIZE {
        let hue = i as f64; // base_hue_for_position: nominal_size == hue_limit, so just i
        let mut low = 0.0f64;
        let mut high = 50.0f64;
        let mut found = false;
        while !found && high < 1300.0 {
            let rgb = jmh_to_rgb_d(&[ljm, high, hue], params);
            found = any_below_zero_d(&rgb);
            if !found {
                low = high;
                high += 50.0;
            }
        }
        while high - low > 1e-2 {
            let mid = (high + low) * 0.5;
            let rgb = jmh_to_rgb_d(&[ljm, mid, hue], params);
            if any_below_zero_d(&rgb) {
                high = mid;
            } else {
                low = mid;
            }
        }
        table[i + TABLE_BASE_INDEX] = high as f32;
    }
    table[TABLE_LOWER_WRAP] = table[TABLE_LAST_NOM];
    table[TABLE_UPPER_WRAP] = table[TABLE_FIRST_NOM];
    // Second upper-wrap slot — mirrors OCIO `Transform.cpp:905-907`.
    table[TABLE_UPPER_WRAP + 1] = table[TABLE_FIRST_NOM + 1];
    table
}

fn make_uniform_hue_gamut_table(
    reach_params: &JMhParams,
    limit_params: &JMhParams,
    peak_lum: f32,
    forward_limit: f32,
    sp: &SharedCompressionParams,
) -> (Table3D, Table1D) {
    let reach_corners = find_reach_corners(reach_params, sp.limit_j_max, forward_limit);
    let (rgb_corners, jmh_corners) = build_limiting_cusp_corners(limit_params, peak_lum);
    let (sorted, unique) = extract_sorted_cube_hues(&reach_corners, &jmh_corners);
    let mut hue_table = Table1D::default();
    build_hue_table(&mut hue_table, &sorted, unique);
    let cusp_table = build_cusp_table(&hue_table, &rgb_corners, &jmh_corners, limit_params);
    (cusp_table, hue_table)
}

struct TestData {
    test_jmh: F3,
    j_intersect_src: f32,
    slope: f32,
    j_intersect_cusp: f32,
}

fn gen_gamma_test_data(
    jm_cusp: &[f32; 2],
    hue: f32,
    limit_j_max: f32,
    mid_j: f32,
    focus_dist: f32,
) -> [TestData; GAMMA_TEST_COUNT] {
    let positions = [0.01f32, 0.1, 0.5, 0.8, 0.99];
    let threshold = lerpf(jm_cusp[0], limit_j_max, FOCUS_GAIN_BLEND);
    let focus_j = compute_focus_j(jm_cusp[0], mid_j, limit_j_max);

    std::array::from_fn(|i| {
        let test_j = lerpf(jm_cusp[0], limit_j_max, positions[i]);
        let sg = get_focus_gain(test_j, threshold, limit_j_max, focus_dist);
        let ji = solve_j_intersect(test_j, jm_cusp[1], focus_j, limit_j_max, sg);
        TestData {
            test_jmh: [test_j, jm_cusp[1], hue],
            j_intersect_src: ji,
            slope: compute_compression_slope(ji, focus_j, limit_j_max, sg),
            j_intersect_cusp: solve_j_intersect(jm_cusp[0], jm_cusp[1], focus_j, limit_j_max, sg),
        }
    })
}

fn evaluate_gamma_fit(
    jm_cusp: &[f32; 2],
    data: &[TestData; GAMMA_TEST_COUNT],
    top_gamma_inv: f32,
    peak_lum: f32,
    limit_j_max: f32,
    lower_gamma_inv: f32,
    limit_params: &JMhParams,
) -> bool {
    let lum_limit = peak_lum / REFERENCE_LUMINANCE;
    for td in data {
        let approx_m = find_gamut_boundary_m(
            jm_cusp,
            limit_j_max,
            top_gamma_inv,
            lower_gamma_inv,
            td.j_intersect_src,
            td.slope,
            td.j_intersect_cusp,
        );
        let approx_j = td.j_intersect_src + td.slope * approx_m;
        let rgb = jmh_to_rgb(&[approx_j, approx_m, td.test_jmh[2]], limit_params);
        if !outside_hull(&rgb, lum_limit) {
            return false;
        }
    }
    true
}

/// The fixed inputs of the upper-hull gamma search: OCIO `makeUpperHullGamma`'s scalar
/// parameters (`peakLuminance`, `limitJmax`, `midJ`, `focusDist`, `lowerHullGamma`) and the
/// limiting-primaries `JMhParams`.
struct UpperHullGammaParams<'a> {
    peak_lum: f32,
    limit_j_max: f32,
    mid_j: f32,
    focus_dist: f32,
    lower_gamma_inv: f32,
    limit_params: &'a JMhParams,
}

fn make_upper_hull_gamma(
    hue_table: &Table1D,
    cusp_table: &mut Table3D,
    params: &UpperHullGammaParams<'_>,
) {
    let &UpperHullGammaParams {
        peak_lum,
        limit_j_max,
        mid_j,
        focus_dist,
        lower_gamma_inv,
        limit_params,
    } = params;
    for i in TABLE_FIRST_NOM..TABLE_UPPER_WRAP {
        let hue = hue_table[i];
        let jm_cusp = [cusp_table[i][0], cusp_table[i][1]];
        let data = gen_gamma_test_data(&jm_cusp, hue, limit_j_max, mid_j, focus_dist);

        let mut low = GAMMA_MINIMUM;
        let mut high = low + GAMMA_SEARCH_STEP;
        let mut outside = false;
        while !outside && high < GAMMA_MAXIMUM {
            if evaluate_gamma_fit(
                &jm_cusp,
                &data,
                1.0 / high,
                peak_lum,
                limit_j_max,
                lower_gamma_inv,
                limit_params,
            ) {
                outside = true;
            } else {
                low = high;
                high += GAMMA_SEARCH_STEP;
            }
        }
        while high - low > GAMMA_ACCURACY {
            let test = midpoint_f(high, low);
            if evaluate_gamma_fit(
                &jm_cusp,
                &data,
                1.0 / test,
                peak_lum,
                limit_j_max,
                lower_gamma_inv,
                limit_params,
            ) {
                high = test;
            } else {
                low = test;
            }
        }
        cusp_table[i][2] = 1.0 / high;
    }
    cusp_table[TABLE_LOWER_WRAP][2] = cusp_table[TABLE_LAST_NOM][2];
    cusp_table[TABLE_UPPER_WRAP][2] = cusp_table[TABLE_FIRST_NOM][2];
    // Second upper-wrap gamma slot — mirrors OCIO `Transform.cpp:1260-1262`.
    cusp_table[TABLE_UPPER_WRAP + 1][2] = cusp_table[TABLE_FIRST_NOM + 1][2];
}

/// Narrow the hue binary-search window to the largest observed deviation from a linear hue
/// distribution, so `lookup_hue_interval` needs ~log2(range) probes instead of log2(table).
///
/// Reference: `Transform.cpp:1360-1382`, with `lower_padding = 0`, `upper_padding = 1`.
///
/// The `pos` here MUST be a *nominal* table index — `nominal_hue_position_in_uniform_table`
/// in the reference (`Common.h:78-81`) is `first_nominal_index + floor(hue)`. This function
/// previously used the raw `floor(hue)`, while its only consumer `lookup_hue_interval` seeds
/// from `nominal_hue_pos`, which DOES include the offset. Producer and consumer therefore
/// disagreed by one slot where OCIO's agree, leaving the window one slot tighter on the low
/// side. Where corner-inserted hues locally compress the spacing — exactly the phenomenon
/// this function exists to measure — the bracket could exclude the true interval, pushing
/// `interp_weight` outside [0,1] and *extrapolating* the cusp J/M and `gamma_top_inv`.
/// Forward and inverse consult the same lookup, so round-trip tests cannot see this.
fn determine_hue_linearity_range(cusp_table: &Table3D) -> [i32; 2] {
    let mut range = [0i32, 1i32];
    for i in TABLE_FIRST_NOM..TABLE_UPPER_WRAP {
        // Clamp the uniform position, then offset — the clamp guards the table read, and
        // applying it after the offset would instead cap a valid nominal index.
        let uniform_pos = (cusp_table[i][2] as usize).min(TABLE_NOMINAL_SIZE - 1);
        let pos = TABLE_FIRST_NOM + uniform_pos;
        let delta = i as i32 - pos as i32;
        range[0] = range[0].min(delta);
        range[1] = range[1].max(delta + 1);
    }
    range
}

pub fn init_gamut_compress_params(
    peak_lum: f32,
    input_params: &JMhParams,
    limit_params: &JMhParams,
    ts: &ToneScaleParams,
    sh: &SharedCompressionParams,
    reach_params: &JMhParams,
) -> GamutCompressParams {
    let mid_j = y_to_j(ts.c_t * REFERENCE_LUMINANCE, input_params);
    let focus_dist = FOCUS_DISTANCE + FOCUS_DISTANCE * FOCUS_DISTANCE_SCALING * ts.log_peak;
    let lower_hull_gamma_inv = 1.0 / (1.14 + 0.07 * ts.log_peak);

    let (mut cusp_table, hue_table) =
        make_uniform_hue_gamut_table(reach_params, limit_params, peak_lum, ts.forward_limit, sh);
    let search_range = determine_hue_linearity_range(&cusp_table);
    make_upper_hull_gamma(
        &hue_table,
        &mut cusp_table,
        &UpperHullGammaParams {
            peak_lum,
            limit_j_max: sh.limit_j_max,
            mid_j,
            focus_dist,
            lower_gamma_inv: lower_hull_gamma_inv,
            limit_params,
        },
    );

    GamutCompressParams {
        mid_j,
        focus_dist,
        lower_hull_gamma_inv,
        hue_linearity_search_range: search_range,
        hue_table,
        gamut_cusp_table: cusp_table,
    }
}

// --- Public API: Full Output Transform ---

/// Build full ACES 2.0 Output Transform state.
///
/// # Panics
///
/// If `limiting_primaries` are degenerate (see [`init_jmh_params`]). Use
/// [`try_init_output_transform`] for primaries from a config file.
pub fn init_output_transform(peak_lum: f32, limiting_primaries: &Primaries) -> Aces2State {
    match try_init_output_transform(peak_lum, limiting_primaries) {
        Ok(state) => state,
        Err(e) => panic!("ACES2: degenerate limiting primaries {limiting_primaries:?} — {e}"),
    }
}

/// Fallible [`init_output_transform`]: an error, not a panic, for degenerate (e.g. collinear)
/// limiting primaries, which can come from a config file. All-zero placeholder primaries still
/// build (they give NaN on purpose, see `rgb_to_xyz_f64`).
pub fn try_init_output_transform(
    peak_lum: f32,
    limiting_primaries: &Primaries,
) -> Result<Aces2State, OcioError> {
    let p_in = init_jmh_params(&ACES_AP0);
    let p_out = try_init_jmh_params(limiting_primaries)?;
    let ts = init_tonescale_params(peak_lum);
    let reach = init_jmh_params(&ACES_AP1);
    let shared = init_shared_compression_params(peak_lum, &p_in, &reach);
    let chroma = init_chroma_compress_params(peak_lum, &ts);
    let gamut = init_gamut_compress_params(peak_lum, &p_in, &p_out, &ts, &shared, &reach);

    Ok(Aces2State {
        p_in,
        p_out,
        ts,
        shared,
        chroma,
        gamut,
    })
}

/// Bit-exact key for [`init_output_transform_cached`]: `(peak_lum, primaries)`.
///
/// `f32` isn't `Eq`/`Hash`; `to_bits()` gives an exact, NaN-safe key without rounding, which is
/// correct here because every caller passes the same literal values each time (config-driven
/// peak/primaries, not the result of independent floating-point computation that could differ
/// by an ULP between calls).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct OutputTransformKey {
    peak_bits: u32,
    primaries_bits: [u32; 8],
}

impl OutputTransformKey {
    fn new(peak_lum: f32, p: &Primaries) -> Self {
        Self {
            peak_bits: peak_lum.to_bits(),
            primaries_bits: [
                p.red.x.to_bits(),
                p.red.y.to_bits(),
                p.green.x.to_bits(),
                p.green.y.to_bits(),
                p.blue.x.to_bits(),
                p.blue.y.to_bits(),
                p.white.x.to_bits(),
                p.white.y.to_bits(),
            ],
        }
    }
}

fn output_transform_cache() -> &'static std::sync::RwLock<
    std::collections::HashMap<OutputTransformKey, std::sync::Arc<Aces2State>>,
> {
    static CACHE: std::sync::OnceLock<
        std::sync::RwLock<
            std::collections::HashMap<OutputTransformKey, std::sync::Arc<Aces2State>>,
        >,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::RwLock::new(std::collections::HashMap::new()))
}

/// Memoised [`init_output_transform`] (PLAN9.md O10).
///
/// `init_output_transform` builds the cusp/reach/gamma tables from scratch (a few ms; the
/// `hue_table` + `gamut_cusp_table` alone are ~7.3 KB) — correct to do once per distinct
/// `(peak_lum, limiting_primaries)` pair, but every unmemoised call (and, before this, every
/// `ProcessorOp` clone through `ProcessorCache::get_or_create*`'s cache-hit path and
/// `ProcessorOp::invert()`) paid it again. The overwhelmingly common case is one display peak
/// and one set of limiting primaries reused across many processor builds, so this shares one
/// `Aces2State` via `Arc` instead.
///
/// Returns an error (and caches nothing) for degenerate limiting primaries, which come from a
/// config's FixedFunction params. A poisoned cache lock is recovered: the map only ever holds
/// fully built states, so a panic elsewhere cannot leave it half-written.
pub fn init_output_transform_cached(
    peak_lum: f32,
    limiting_primaries: &Primaries,
) -> Result<std::sync::Arc<Aces2State>, OcioError> {
    let key = OutputTransformKey::new(peak_lum, limiting_primaries);
    if let Some(state) = output_transform_cache()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key)
    {
        return Ok(std::sync::Arc::clone(state));
    }
    let built = try_init_output_transform(peak_lum, limiting_primaries)?;
    let mut cache = output_transform_cache()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Another thread may have populated this key meanwhile; keep the first so every caller shares
    // one instance.
    Ok(std::sync::Arc::clone(
        cache
            .entry(key)
            .or_insert_with(|| std::sync::Arc::new(built)),
    ))
}

/// Apply ACES 2.0 Output Transform forward (scene-referred AP0 → display).
pub fn output_transform_fwd(rgb: &F3, st: &Aces2State) -> F3 {
    let aab = rgb_to_aab(rgb, &st.p_in);
    let jmh = aab_to_jmh(&aab, &st.p_in);

    let rp = resolve_compression_params(jmh[2], &st.shared);
    let h_rad = to_radians(jmh[2]);
    let (cos_hr, sin_hr) = (h_rad.cos(), h_rad.sin());
    let mnorm = chroma_compress_norm(cos_hr, sin_hr, st.chroma.chroma_compress_scale);

    let j_ts = tonescale_a_to_j_fwd(aab[0], &st.p_in, &st.ts);
    let tonemapped = chroma_compress_fwd(&jmh, j_ts, mnorm, &rp, &st.chroma);
    let compressed = gamut_compress_fwd(&tonemapped, &rp, &st.gamut);

    let aab_out = jmh_to_aab_trig(&compressed, cos_hr, sin_hr, &st.p_out);
    aab_to_rgb(&aab_out, &st.p_out)
}

/// Apply ACES 2.0 Output Transform inverse (display → scene-referred AP0).
pub fn output_transform_inv(rgb: &F3, st: &Aces2State) -> F3 {
    let compressed = rgb_to_jmh(rgb, &st.p_out);

    let rp = resolve_compression_params(compressed[2], &st.shared);
    let h_rad = to_radians(compressed[2]);
    let (cos_hr, sin_hr) = (h_rad.cos(), h_rad.sin());
    let mnorm = chroma_compress_norm(cos_hr, sin_hr, st.chroma.chroma_compress_scale);

    let tonemapped = gamut_compress_inv(&compressed, &rp, &st.gamut);
    let j = tonescale_inv_j(tonemapped[0], &st.p_in, &st.ts);
    let jmh = chroma_compress_inv(&tonemapped, j, mnorm, &rp, &st.chroma);

    let aab = jmh_to_aab_trig(&jmh, cos_hr, sin_hr, &st.p_in);
    aab_to_rgb(&aab, &st.p_in)
}

// --- Individual ACES 2.0 FixedFunction operations ---

/// RGB to JMh (degrees output).
pub fn rgb_to_jmh_degrees(rgb: &F3, params: &JMhParams) -> F3 {
    let jmh = rgb_to_jmh(rgb, params);
    [jmh[0], jmh[1], jmh[2]] // hue already in degrees (0..360)
}

/// JMh (degrees input) to RGB.
pub fn jmh_degrees_to_rgb(jmh: &F3, params: &JMhParams) -> F3 {
    let h = wrap_to_hue_limit(jmh[2]);
    jmh_to_rgb(&[jmh[0], jmh[1], h], params)
}

/// Tonescale + chroma compress forward (JMh in degrees).
pub fn tonescale_compress_fwd(
    jmh_deg: &F3,
    p: &JMhParams,
    ts: &ToneScaleParams,
    shared: &SharedCompressionParams,
    chroma: &ChromaCompressParams,
) -> F3 {
    let h = wrap_to_hue_limit(jmh_deg[2]);
    let h_rad = to_radians(h);
    let (cos_hr, sin_hr) = (h_rad.cos(), h_rad.sin());
    let mnorm = chroma_compress_norm(cos_hr, sin_hr, chroma.chroma_compress_scale);
    let rp = resolve_compression_params(h, shared);
    let j_ts = tonescale_fwd_j(jmh_deg[0], p, ts);
    let out = chroma_compress_fwd(&[jmh_deg[0], jmh_deg[1], h], j_ts, mnorm, &rp, chroma);
    [out[0], out[1], out[2]] // hue stays in degrees
}

/// Tonescale + chroma compress inverse (JMh in degrees).
pub fn tonescale_compress_inv(
    jmh_deg: &F3,
    p: &JMhParams,
    ts: &ToneScaleParams,
    shared: &SharedCompressionParams,
    chroma: &ChromaCompressParams,
) -> F3 {
    let h = wrap_to_hue_limit(jmh_deg[2]);
    let h_rad = to_radians(h);
    let (cos_hr, sin_hr) = (h_rad.cos(), h_rad.sin());
    let mnorm = chroma_compress_norm(cos_hr, sin_hr, chroma.chroma_compress_scale);
    let rp = resolve_compression_params(h, shared);
    let j = tonescale_inv_j(jmh_deg[0], p, ts);
    let out = chroma_compress_inv(&[jmh_deg[0], jmh_deg[1], h], j, mnorm, &rp, chroma);
    [out[0], out[1], out[2]]
}

/// Gamut compress forward (JMh in degrees).
pub fn gamut_compress_fwd_deg(
    jmh_deg: &F3,
    shared: &SharedCompressionParams,
    gamut: &GamutCompressParams,
) -> F3 {
    let h = wrap_to_hue_limit(jmh_deg[2]);
    let rp = resolve_compression_params(h, shared);
    let out = gamut_compress_fwd(&[jmh_deg[0], jmh_deg[1], h], &rp, gamut);
    [out[0], out[1], out[2]]
}

/// Gamut compress inverse (JMh in degrees).
pub fn gamut_compress_inv_deg(
    jmh_deg: &F3,
    shared: &SharedCompressionParams,
    gamut: &GamutCompressParams,
) -> F3 {
    let h = wrap_to_hue_limit(jmh_deg[2]);
    let rp = resolve_compression_params(h, shared);
    let out = gamut_compress_inv(&[jmh_deg[0], jmh_deg[1], h], &rp, gamut);
    [out[0], out[1], out[2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Helpers for building SDR/HDR states (Rec.709 / Rec.2020 primaries) ---

    fn sdr_state() -> Aces2State {
        // Rec.709 primaries, 100 nit SDR
        init_output_transform(
            100.0,
            &Primaries {
                red: Chromaticity { x: 0.64, y: 0.33 },
                green: Chromaticity { x: 0.30, y: 0.60 },
                blue: Chromaticity { x: 0.15, y: 0.06 },
                white: Chromaticity {
                    x: 0.3127,
                    y: 0.3290,
                },
            },
        )
    }

    fn hdr1000_state() -> Aces2State {
        // Rec.2020 primaries, 1000 nit HDR
        init_output_transform(
            1000.0,
            &Primaries {
                red: Chromaticity { x: 0.708, y: 0.292 },
                green: Chromaticity { x: 0.170, y: 0.797 },
                blue: Chromaticity { x: 0.131, y: 0.046 },
                white: Chromaticity {
                    x: 0.3127,
                    y: 0.3290,
                },
            },
        )
    }

    /// PLAN9.md O10: `init_output_transform_cached` must actually share instances, not just
    /// return equal-valued ones - a `.clone()`-per-call implementation would pass an
    /// equality-only test while still paying the full table rebuild (and every downstream
    /// `Arc`-clone) every time.
    #[test]
    fn init_output_transform_cached_shares_instance_per_key() {
        let rec709 = Primaries {
            red: Chromaticity { x: 0.64, y: 0.33 },
            green: Chromaticity { x: 0.30, y: 0.60 },
            blue: Chromaticity { x: 0.15, y: 0.06 },
            white: Chromaticity {
                x: 0.3127,
                y: 0.3290,
            },
        };
        let a = init_output_transform_cached(100.0, &rec709).expect("Rec.709");
        let b = init_output_transform_cached(100.0, &rec709).expect("Rec.709");
        assert!(
            std::sync::Arc::ptr_eq(&a, &b),
            "same (peak_lum, primaries) must return the SAME Arc instance"
        );

        let rec2020 = Primaries {
            red: Chromaticity { x: 0.708, y: 0.292 },
            green: Chromaticity { x: 0.170, y: 0.797 },
            blue: Chromaticity { x: 0.131, y: 0.046 },
            white: Chromaticity {
                x: 0.3127,
                y: 0.3290,
            },
        };
        let c = init_output_transform_cached(1000.0, &rec2020).expect("Rec.2020");
        assert!(
            !std::sync::Arc::ptr_eq(&a, &c),
            "a different (peak_lum, primaries) key must NOT collide with an unrelated one"
        );
        // And the cached value must actually match an uncached build, bit-for-bit.
        let uncached = init_output_transform(100.0, &rec709);
        assert_eq!(a.p_out.f_l_n, uncached.p_out.f_l_n);
    }

    // --- Tests ported from vfx-color/src/aces2/transform.rs ---

    #[test]
    fn test_sdr_peak_luminance() {
        // SDR state should use 100 nit peak — verify tonescale was built for it.
        // ToneScaleParams stores the peak in ts.n_r (normalised) and ts.peak_luminance.
        let st = sdr_state();
        // The tonescale peak is stored inside ts; confirm it was initialised with 100 nits
        // by running the forward transform on a very bright input and checking it clamps.
        let bright = output_transform_fwd(&[100.0, 100.0, 100.0], &st);
        assert!(
            bright[0] <= 1.0 + 1e-4 && bright[1] <= 1.0 + 1e-4 && bright[2] <= 1.0 + 1e-4,
            "SDR output should be clamped near 1.0, got {:?}",
            bright
        );
    }

    #[test]
    fn test_black_is_black() {
        let st = sdr_state();
        let result = output_transform_fwd(&[0.0, 0.0, 0.0], &st);
        assert!(result[0].abs() < 1e-4, "R not black: {}", result[0]);
        assert!(result[1].abs() < 1e-4, "G not black: {}", result[1]);
        assert!(result[2].abs() < 1e-4, "B not black: {}", result[2]);
    }

    #[test]
    fn test_output_clamped() {
        // Very high SDR input should be clamped to [0, 1]
        let st = sdr_state();
        let result = output_transform_fwd(&[10.0, 10.0, 10.0], &st);
        assert!(
            result[0] >= 0.0 && result[0] <= 1.0,
            "R={} out of range",
            result[0]
        );
        assert!(
            result[1] >= 0.0 && result[1] <= 1.0,
            "G={} out of range",
            result[1]
        );
        assert!(
            result[2] >= 0.0 && result[2] <= 1.0,
            "B={} out of range",
            result[2]
        );
    }

    #[test]
    fn test_midgray_produces_output() {
        // ACES2 SDR (100 nit, Rec.709) output transform of neutral 0.18.
        // The old test only asserted in-range + neutral, which an identity /
        // no-op transform (result == 0.18) satisfies. The ACES2 output
        // transform is computed numerically (the OpenColorIO reference has no
        // tabulated golden value for 0.18), and we must NOT assert a value
        // produced by running this same implementation — so we assert a
        // verifiable PROPERTY: finite, in display range, neutral, and moved off
        // the linear input by a clear margin (a no-op fails the move).
        let st = sdr_state();
        let result = output_transform_fwd(&[0.18, 0.18, 0.18], &st);
        for (i, c) in result.iter().enumerate() {
            assert!(
                c.is_finite() && *c >= 0.0 && *c <= 1.0,
                "Mid-gray channel {i} out of range: {c}"
            );
        }
        // Neutral in -> neutral out (achromatic path preserves grey).
        assert!(
            (result[0] - result[1]).abs() < 0.1 && (result[1] - result[2]).abs() < 0.1,
            "Gray should stay neutral: {result:?}"
        );
        // Display-encoded result must differ from the scene-linear input;
        // an identity / no-op transform would leave it at 0.18.
        assert!(
            (result[0] - 0.18).abs() > 0.02,
            "ACES2 SDR output of 0.18 must differ from input (no-op?): {result:?}"
        );
    }

    #[test]
    fn test_monotonic() {
        // Increasing neutral input should produce monotonically increasing luminance
        let st = sdr_state();
        let mut prev_lum = 0.0_f32;
        for i in 1..20 {
            let v = i as f32 * 0.1;
            let r = output_transform_fwd(&[v, v, v], &st);
            // Rec.709 luminance coefficients
            let lum = r[0] * 0.2126 + r[1] * 0.7152 + r[2] * 0.0722;
            assert!(
                lum >= prev_lum - 1e-5,
                "Not monotonic at v={}: lum={}, prev={}",
                v,
                lum,
                prev_lum
            );
            prev_lum = lum;
        }
    }

    #[test]
    fn test_hdr_vs_sdr() {
        // A bright scene-linear input (2.0) drives the SDR (100 nit) tone scale
        // into its near-clamped top end, while the HDR (1000 nit) tone scale has
        // 10x the headroom and keeps mapping brightness upward. So the HDR
        // display-encoded luminance MUST exceed the SDR one. The old test only
        // checked both were > 0 and never actually compared them — it passed
        // even if HDR and SDR produced identical output.
        let sdr = sdr_state();
        let hdr = hdr1000_state();
        let input = [2.0, 2.0, 2.0];
        let sdr_out = output_transform_fwd(&input, &sdr);
        let hdr_out = output_transform_fwd(&input, &hdr);
        let sdr_lum = sdr_out[0] + sdr_out[1] + sdr_out[2];
        let hdr_lum = hdr_out[0] + hdr_out[1] + hdr_out[2];
        assert!(sdr_lum > 0.0, "SDR output must be positive: {sdr_out:?}");
        assert!(
            hdr_lum > sdr_lum,
            "HDR (1000 nit) should encode brighter than SDR (100 nit) for input 2.0: \
             hdr_lum={hdr_lum} sdr_lum={sdr_lum}"
        );
    }

    #[test]
    fn test_sdr_vs_hdr_gamut_difference() {
        // SDR (Rec.709) and HDR (Rec.2020) states should produce different results for
        // a saturated colour — this verifies different limiting primaries are being used.
        let sdr = sdr_state();
        let hdr = hdr1000_state();
        // Saturated red: Rec.2020 can represent wider gamut so results differ
        let saturated_red = [1.0, 0.0, 0.0];
        let sdr_out = output_transform_fwd(&saturated_red, &sdr);
        let hdr_out = output_transform_fwd(&saturated_red, &hdr);
        // The two transforms must yield different results (different limiting gamuts)
        let diff = (sdr_out[0] - hdr_out[0]).abs()
            + (sdr_out[1] - hdr_out[1]).abs()
            + (sdr_out[2] - hdr_out[2]).abs();
        assert!(
            diff > 0.01,
            "SDR and HDR outputs should differ for saturated input; diff={}",
            diff
        );
    }

    #[test]
    fn test_neutral_buffer_alpha_passthrough() {
        // Simulate processing an interleaved RGBA buffer: alpha must be unchanged.
        let st = sdr_state();
        let pixels: &[[f32; 4]] = &[[0.18, 0.18, 0.18, 1.0], [0.5, 0.3, 0.2, 0.5]];
        for px in pixels {
            let rgb_in = [px[0], px[1], px[2]];
            let rgb_out = output_transform_fwd(&rgb_in, &st);
            // Verify rgb output is in range.
            for (i, &ch) in rgb_out.iter().enumerate() {
                assert!(
                    (0.0..=1.0).contains(&ch),
                    "channel {} out of range: {}",
                    i,
                    ch
                );
            }
            // The old test asserted `(px[3]-px[3]).abs() < 1e-6` — a tautology
            // that holds regardless of correctness. The RGB transform does not
            // touch alpha, so there is nothing meaningful to assert about it;
            // instead assert the transform actually did something to a
            // non-trivial (non-neutral) pixel, catching an identity / no-op.
            if (rgb_in[0] - rgb_in[1]).abs() > 1e-6 || (rgb_in[1] - rgb_in[2]).abs() > 1e-6 {
                let moved = (rgb_out[0] - rgb_in[0]).abs()
                    + (rgb_out[1] - rgb_in[1]).abs()
                    + (rgb_out[2] - rgb_in[2]).abs();
                assert!(
                    moved > 1e-3,
                    "RGB transform was a no-op on {rgb_in:?} -> {rgb_out:?}"
                );
            }
        }
    }

    #[test]
    fn test_rgb_to_jmh_roundtrip() {
        let params = init_jmh_params(&ACES_AP0);
        let rgb = [0.18, 0.18, 0.18];
        let jmh = rgb_to_jmh(&rgb, &params);
        let rgb2 = jmh_to_rgb(&jmh, &params);
        for i in 0..3 {
            assert!(
                (rgb[i] - rgb2[i]).abs() < 1e-4,
                "channel {} mismatch: {} vs {}",
                i,
                rgb[i],
                rgb2[i]
            );
        }
    }

    #[test]
    fn test_achromatic_j_roundtrip() {
        let params = init_jmh_params(&ACES_AP0);
        let y = 0.18;
        let j = y_to_j(y, &params);
        let y2 = j_to_y(j, &params);
        assert!((y - y2).abs() < 1e-5, "Y roundtrip: {} vs {}", y, y2);
    }

    #[test]
    fn test_tonescale_roundtrip() {
        let ts = init_tonescale_params(1000.0);
        let y = 0.18;
        let y_out = aces_tonescale_fwd(y, &ts);
        let y_back = aces_tonescale_inv(y_out, &ts);
        assert!(
            (y - y_back).abs() < 1e-3,
            "tonescale roundtrip: {} vs {}",
            y,
            y_back
        );
    }

    #[test]
    fn test_output_transform_smoke() {
        // Rec.709 primaries for sRGB-like display
        let rec709 = Primaries {
            red: Chromaticity { x: 0.64, y: 0.33 },
            green: Chromaticity { x: 0.30, y: 0.60 },
            blue: Chromaticity { x: 0.15, y: 0.06 },
            white: Chromaticity {
                x: 0.3127,
                y: 0.3290,
            },
        };
        let st = init_output_transform(1000.0, &rec709);
        let rgb_in = [0.18, 0.18, 0.18];
        let rgb_out = output_transform_fwd(&rgb_in, &st);
        // Should produce a valid non-zero result
        assert!(
            rgb_out[0] > 0.0 && rgb_out[1] > 0.0 && rgb_out[2] > 0.0,
            "output should be positive: {:?}",
            rgb_out
        );
        // Inverse roundtrip
        let rgb_back = output_transform_inv(&rgb_out, &st);
        for i in 0..3 {
            assert!(
                (rgb_in[i] - rgb_back[i]).abs() < 0.05,
                "channel {} roundtrip: {} vs {}",
                i,
                rgb_in[i],
                rgb_back[i]
            );
        }
    }

    #[test]
    fn test_matrix_inversion() {
        let m = rgb_to_xyz_m33(&ACES_AP0).expect("AP0 primaries are well conditioned");
        let m_inv = invert_m33(&m).expect("AP0 RGB-to-XYZ is invertible");
        let id = mult_m33_m33(&m, &m_inv);
        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (id[i * 3 + j] - expected).abs() < 1e-4,
                    "identity[{}][{}] = {} (expected {})",
                    i,
                    j,
                    id[i * 3 + j],
                    expected
                );
            }
        }
    }

    /// A singular matrix must be reported, not silently handed back un-inverted.
    ///
    /// This is the regression guard for the old `det < 1e-30 => return *m` fallback: under
    /// that code this call returned `m` itself, and the caller multiplied by a matrix that
    /// was not an inverse without ever learning it.
    #[test]
    fn test_singular_matrix_is_an_error() {
        // Rank 2: the third row is the sum of the first two, so det == 0 exactly.
        let singular: M33 = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 5.0, 7.0, 9.0];
        assert!(
            invert_m33(&singular).is_err(),
            "singular matrix must return an error rather than the input matrix"
        );
    }
}
