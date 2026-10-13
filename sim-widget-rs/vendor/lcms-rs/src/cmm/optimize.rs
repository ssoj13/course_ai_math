//! Pipeline optimization (`cmsopt.c`) — a pure-Rust, bit-exact port.
//!
//! lcms2 turns a general [`Pipeline`] into a fast, evaluation-cheap form by collapsing
//! adjacent no-ops and installing a precomputed whole-pipeline evaluator
//! (`_cmsPipelineSetOptimizationParameters`) **while keeping the discrete stage chain**. This
//! port does the same via [`Pipeline::set_optimization_parameters`]: each optimizer rebuilds the
//! pipeline as the equivalent serializable stages (a resampled CLUT stage; matrix + curve stages
//! for matrix-shaper; a joined curve-set for curve-joining; prelinearization curves + CLUT for
//! linearization) and attaches the fast evaluator (`Prelin16`/`Prelin8`/`MatShaper`/`CurvesFast`)
//! as the pipeline's fast path. Evaluation runs the fast evaluator (bit-for-bit as before), while
//! `pipeline.stages()` still exposes real curve/matrix/CLUT stages an `AToB`/`BToA` writer or
//! [`crate::virt::find_combination`] can serialize.
//!
//! # Fidelity (do not "clean up")
//! * The fixed-point evaluators (`PrelinEval8`, `MatShaperEval16`) rely on signed-integer
//!   wraparound (C marks them `CMS_NO_SANITIZE`); we reproduce that with `wrapping_*`.
//! * All 16-bit round-trips reproduce `_cmsQuickSaturateWord(x * 65535)` and `x / 65535.0f`.
//! * White-point fixup (`FixWhiteMisalignment` / `PatchLUT`) is applied to the CLUT table
//!   held inside the fast evaluator, exactly as the C code patches `Grid->Tab.T` that the fast
//!   evaluator references.
//!
//! # Result convention
//! [`optimize_pipeline`] mirrors `_cmsOptimizePipeline`: on return `*lut` is always a usable
//! pipeline (either the optimized form or the pre-optimized chain). Individual optimizers
//! return `Ok(true)` when they replaced the pipeline, `Ok(false)` when not applicable
//! (pipeline untouched), and `Err` only on genuine allocation/internal failure.

use std::sync::Arc;

use crate::cms_err;
use crate::color::gamma::ToneCurve;
use crate::context::Context;
use crate::color::pcs::{end_points_by_space, icc_colorspace, reasonable_gridpoints_by_colorspace};
use crate::error::{ErrorCode, Result};
use crate::math::interp::{InterpParams, MAX_INPUT_DIMENSIONS, MAX_STAGE_CHANNELS};
use crate::math::mat3::{Mat3, Vec3};
use crate::pipeline::stage::{
    check_and_retrieve, ClutTable, Eval16, Stage, StageData, STAGE_CURVE_SET, STAGE_FLOATPCS2LAB,
    STAGE_FLOATPCS2XYZ, STAGE_IDENTITY, STAGE_LAB2FLOATPCS, STAGE_LAB2XYZ, STAGE_LABV2TOV4,
    STAGE_LABV4TOV2, STAGE_MATRIX, STAGE_NAMED_COLOR, STAGE_XYZ2FLOATPCS, STAGE_XYZ2LAB,
};
use crate::pipeline::{Pipeline, StageLoc};
use crate::types::format::PixelFormat;
use crate::types::num::{from_16_to_8, from_8_to_16, quick_saturate_word, to_fixed_domain};
use crate::types::sig::{colorspace, Signature};
use crate::MAX_CHANNELS;

// ---------------------------------------------------------------------------------------
// Constants (lcms2.h) used by the optimizer.
// ---------------------------------------------------------------------------------------

/// `cmsFLAGS_CLUT_POST_LINEARIZATION` — keep postlinearization tables if possible.
pub const FLAGS_CLUT_POST_LINEARIZATION: u32 = 0x0001;
/// `cmsFLAGS_FORCE_CLUT` — force the resampling (CLUT) optimization.
pub const FLAGS_FORCE_CLUT: u32 = 0x0002;
/// `cmsFLAGS_NOWHITEONWHITEFIXUP` — don't fix the scum dot at white.
pub const FLAGS_NOWHITEONWHITEFIXUP: u32 = 0x0004;
/// `cmsFLAGS_CLUT_PRE_LINEARIZATION` — keep prelinearization tables if possible.
pub const FLAGS_CLUT_PRE_LINEARIZATION: u32 = 0x0010;
/// `cmsFLAGS_NOCACHE` — inhibit the 1-pixel cache (set by the fast evaluators).
pub const FLAGS_NOCACHE: u32 = 0x0040;
/// `cmsFLAGS_NOOPTIMIZE` — inhibit optimizations (PreOptimize still runs).
pub const FLAGS_NOOPTIMIZE: u32 = 0x0100;

/// `INTENT_ABSOLUTE_COLORIMETRIC`.
const INTENT_ABSOLUTE_COLORIMETRIC: u32 = 3;
/// `PT_RGB` — the "our-notation" colour-space code for RGB (used with `T_COLORSPACE`).
const PT_RGB: u32 = 4;
/// `OPTIMIZED_SH(1)` — the format bit marking an optimized 8-bit matrix-shaper output.
const OPTIMIZED_SH_1: u32 = 1 << 21;
/// `PRELINEARIZATION_POINTS` — sample count for the prelinearization / joined curves.
const PRELINEARIZATION_POINTS: usize = 4096;

// ---------------------------------------------------------------------------------------
// Constant matrices for the matrix-based special stages (mirrored from `pipeline::stage`,
// where they are private). Needed by [`as_mat3`] to treat those stages as matrices.
// ---------------------------------------------------------------------------------------

const V2_TO_V4: [f64; 9] = [
    65535.0 / 65280.0, 0.0, 0.0,
    0.0, 65535.0 / 65280.0, 0.0,
    0.0, 0.0, 65535.0 / 65280.0,
];
const V4_TO_V2: [f64; 9] = [
    65280.0 / 65535.0, 0.0, 0.0,
    0.0, 65280.0 / 65535.0, 0.0,
    0.0, 0.0, 65280.0 / 65535.0,
];
const NORM_FROM_LAB_M: [f64; 9] = [
    1.0 / 100.0, 0.0, 0.0,
    0.0, 1.0 / 255.0, 0.0,
    0.0, 0.0, 1.0 / 255.0,
];
const NORM_FROM_LAB_O: [f64; 3] = [0.0, 128.0 / 255.0, 128.0 / 255.0];
const NORM_TO_LAB_M: [f64; 9] = [
    100.0, 0.0, 0.0,
    0.0, 255.0, 0.0,
    0.0, 0.0, 255.0,
];
const NORM_TO_LAB_O: [f64; 3] = [0.0, -128.0, -128.0];
const NORM_FROM_XYZ: [f64; 9] = [
    32768.0 / 65535.0, 0.0, 0.0,
    0.0, 32768.0 / 65535.0, 0.0,
    0.0, 0.0, 32768.0 / 65535.0,
];
const NORM_TO_XYZ: [f64; 9] = [
    65535.0 / 32768.0, 0.0, 0.0,
    0.0, 65535.0 / 32768.0, 0.0,
    0.0, 0.0, 65535.0 / 32768.0,
];

// ---------------------------------------------------------------------------------------
// Small numeric helpers (macros / inline routines from cmsopt.c).
// ---------------------------------------------------------------------------------------

/// `DOUBLE_TO_1FIXED14(x)` — `(cmsS1Fixed14Number) floor(x * 16384.0 + 0.5)`.
#[inline]
fn double_to_1fixed14(x: f64) -> i32 {
    (x * 16384.0 + 0.5).floor() as i32
}

/// Build a [`Mat3`] from a row-major 9-element slice.
#[inline]
fn mat3_from9(m: &[f64]) -> Mat3 {
    Mat3([
        Vec3::new(m[0], m[1], m[2]),
        Vec3::new(m[3], m[4], m[5]),
        Vec3::new(m[6], m[7], m[8]),
    ])
}

/// Flatten a [`Mat3`] back into a row-major 9-element array.
#[inline]
fn mat3_to9(m: &Mat3) -> [f64; 9] {
    [
        m.0[0].0[0], m.0[0].0[1], m.0[0].0[2],
        m.0[1].0[0], m.0[1].0[1], m.0[1].0[2],
        m.0[2].0[0], m.0[2].0[1], m.0[2].0[2],
    ]
}

/// `CloseEnoughFloat`'s tolerance (`cmsopt.c:169`: `fabs(b - a) < 0.00001f`).
///
/// The C literal is a `float`, so the comparison is against `(double)(float)0.00001` ≈
/// 9.99999974737875e-6, not the `f64` 1e-5 a bare `0.00001` would give here. The window
/// differs by ~2.5e-13 - transcribed exactly rather than rounded to something tidier.
const CLOSE_ENOUGH_FLOAT: f64 = 0.00001f32 as f64;

/// `isFloatMatrixIdentity` — near-identity with the loose [`CLOSE_ENOUGH_FLOAT`] tolerance used
/// by `_MultiplyMatrix` (distinct from `_cmsMAT3isIdentity`).
fn is_float_matrix_identity(a: &Mat3) -> bool {
    let id = Mat3::identity();
    for i in 0..3 {
        for j in 0..3 {
            if (a.0[i].0[j] - id.0[i].0[j]).abs() >= CLOSE_ENOUGH_FLOAT {
                return false;
            }
        }
    }
    true
}

/// The tone-curve set of a `cmsSigCurveSetElemType` stage (`_cmsStageGetPtrToCurveSet`).
fn stage_curves(s: &Stage) -> Option<&[ToneCurve]> {
    match &s.data {
        StageData::ToneCurves(v) => Some(v),
        _ => None,
    }
}

/// The 3×3 matrix (+ optional offset) backing any stage whose *type* is `cmsSigMatrixElemType`
/// — true matrix stages and the matrix-based special stages alike. Returns `None` for a
/// non-3×3 matrix or a non-matrix stage.
fn as_mat3(s: &Stage) -> Option<(Mat3, Option<[f64; 3]>)> {
    match &s.data {
        StageData::Matrix { m, offset, rows, cols } if *rows == 3 && *cols == 3 => {
            let off = offset.as_ref().map(|o| [o[0], o[1], o[2]]);
            Some((mat3_from9(m), off))
        }
        StageData::LabV2ToV4 => Some((mat3_from9(&V2_TO_V4), None)),
        StageData::LabV4ToV2 => Some((mat3_from9(&V4_TO_V2), None)),
        StageData::NormalizeFromLab => Some((mat3_from9(&NORM_FROM_LAB_M), Some(NORM_FROM_LAB_O))),
        StageData::NormalizeToLab => Some((mat3_from9(&NORM_TO_LAB_M), Some(NORM_TO_LAB_O))),
        StageData::NormalizeFromXyz => Some((mat3_from9(&NORM_FROM_XYZ), None)),
        StageData::NormalizeToXyz => Some((mat3_from9(&NORM_TO_XYZ), None)),
        _ => None,
    }
}

/// `AllCurvesAreLinear` — every curve of a curve-set stage is linear.
fn all_curves_are_linear(curves: &[ToneCurve]) -> bool {
    curves.iter().all(|c| c.is_linear())
}

/// `IsDegenerated` — a table with wide flat (zero/pole) areas is not optimizeable.
fn is_degenerated(table: &[u16]) -> bool {
    let n_entries = table.len();
    let mut zeros = 0usize;
    let mut poles = 0usize;
    for &v in table {
        if v == 0x0000 {
            zeros += 1;
        }
        if v == 0xffff {
            poles += 1;
        }
    }
    if zeros == 1 && poles == 1 {
        return false; // linear tables
    }
    if zeros > n_entries / 20 {
        return true;
    }
    if poles > n_entries / 20 {
        return true;
    }
    false
}

/// `SlopeLimiting` — normalize a 16-bit table's endpoints by slope-limiting the 2% tails.
fn slope_limiting(table: &mut [u16]) {
    let n_entries = table.len();
    let at_begin = (n_entries as f64 * 0.02 + 0.5).floor() as i32; // Cutoff at 2%
    let at_end = n_entries as i32 - at_begin - 1; // And 98%
    if at_begin <= 0 || at_end < 0 {
        return;
    }

    let (begin_val, end_val) = if table[0] > table[n_entries - 1] {
        (0xffff, 0) // descending
    } else {
        (0, 0xffff)
    };

    // Begin of curve.
    let val = table[at_begin as usize] as f64;
    let slope = (val - begin_val as f64) / at_begin as f64;
    let beta = val - slope * at_begin as f64;
    for i in 0..at_begin {
        table[i as usize] = quick_saturate_word(i as f64 * slope + beta);
    }

    // End of curve (AtBegin holds the X interval, same in both cases).
    let val = table[at_end as usize] as f64;
    let slope = (end_val as f64 - val) / at_begin as f64;
    let beta = val - slope * at_end as f64;
    for i in at_end..n_entries as i32 {
        table[i as usize] = quick_saturate_word(i as f64 * slope + beta);
    }
}

// ---------------------------------------------------------------------------------------
// Fast evaluators (installed as the pipeline's fast path via install_optimized).
// ---------------------------------------------------------------------------------------

/// `FastEvaluateCurves8`/`FastEvaluateCurves16` — a per-channel direct table lookup
/// (`Curves16Data`).
#[derive(Debug)]
struct CurvesFast {
    /// One lookup table per channel (`Curves`): 256 entries when `is8`, else 65536.
    curves: Vec<Vec<u16>>,
    /// Whether the input is 8-bit (index by `In >> 8`) or 16-bit (index by `In`).
    is8: bool,
}

impl Eval16 for CurvesFast {
    fn eval16(&self, input: &[u16], output: &mut [u16]) {
        for (i, table) in self.curves.iter().enumerate() {
            let x = if self.is8 { (input[i] >> 8) as usize } else { input[i] as usize };
            output[i] = table[x];
        }
    }
}

/// `CurvesAlloc` — precompute one lookup table per curve.
fn curves_alloc(curves: &[ToneCurve], n_elements: usize) -> Vec<Vec<u16>> {
    curves
        .iter()
        .map(|g| {
            (0..n_elements)
                .map(|j| {
                    if n_elements == 256 {
                        g.eval_u16(from_8_to_16(j as u8))
                    } else {
                        g.eval_u16(j as u16)
                    }
                })
                .collect()
        })
        .collect()
}

/// `PrelinEval16` — optional input curves → CLUT → optional output curves, all in 16-bit
/// (`Prelin16Data`). Curves stored `None` when absent (the `Eval16nop1D` case).
#[derive(Debug)]
struct Prelin16 {
    curve_in: Option<Vec<ToneCurve>>,
    curve_out: Option<Vec<ToneCurve>>,
    clut_table: Vec<u16>,
    clut_params: InterpParams,
}

impl Eval16 for Prelin16 {
    fn eval16(&self, input: &[u16], output: &mut [u16]) {
        let ni = self.clut_params.n_inputs;
        let no = self.clut_params.n_outputs;
        prelin16_eval(
            self.curve_in.as_deref(),
            &self.clut_table,
            &self.clut_params,
            self.curve_out.as_deref(),
            &input[..ni],
            &mut output[..no],
        );
    }
}

/// `PrelinEval16` body operating on raw data (shared by [`Prelin16`] and the white fixup).
fn prelin16_eval(
    curve_in: Option<&[ToneCurve]>,
    clut_table: &[u16],
    clut_params: &InterpParams,
    curve_out: Option<&[ToneCurve]>,
    input: &[u16],
    output: &mut [u16],
) {
    let ni = clut_params.n_inputs;
    let no = clut_params.n_outputs;
    let mut abc = [0u16; MAX_INPUT_DIMENSIONS];
    for i in 0..ni {
        abc[i] = match curve_in {
            Some(c) => c[i].eval_u16(input[i]),
            None => input[i],
        };
    }
    let mut def = [0u16; MAX_STAGE_CHANNELS];
    clut_params.eval16(clut_table, &abc[..ni], &mut def[..no]);
    for i in 0..no {
        output[i] = match curve_out {
            Some(c) => c[i].eval_u16(def[i]),
            None => def[i],
        };
    }
}

/// Precomputed 8-bit input nodes/offsets (`Prelin8Data`, sans the CLUT itself).
#[derive(Debug, Clone)]
struct Prelin8Tables {
    x0: [u32; 256],
    y0: [u32; 256],
    z0: [u32; 256],
    rx: [u16; 256],
    ry: [u16; 256],
    rz: [u16; 256],
}

/// `PrelinEval8` fast evaluator (`Prelin8Data`): tetrahedral 8-bit input interpolation.
#[derive(Debug)]
struct Prelin8 {
    t: Prelin8Tables,
    clut_table: Vec<u16>,
    clut_params: InterpParams,
}

impl Eval16 for Prelin8 {
    fn eval16(&self, input: &[u16], output: &mut [u16]) {
        let no = self.clut_params.n_outputs;
        prelin8_eval(&self.t, &self.clut_table, &self.clut_params, &input[..3], &mut output[..no]);
    }
}

/// `PrelinOpt8alloc` — precompute the 8-bit node/offset tables from the input curves.
fn prelin8_alloc(p: &InterpParams, curves: &[ToneCurve]) -> Prelin8Tables {
    let mut t = Prelin8Tables {
        x0: [0; 256],
        y0: [0; 256],
        z0: [0; 256],
        rx: [0; 256],
        ry: [0; 256],
        rz: [0; 256],
    };
    for i in 0..256 {
        // 8-bit input arrives as x*257, so the msb byte is x; feed FROM_8_TO_16(i).
        let input = [
            curves[0].eval_u16(from_8_to_16(i as u8)),
            curves[1].eval_u16(from_8_to_16(i as u8)),
            curves[2].eval_u16(from_8_to_16(i as u8)),
        ];
        let v1 = to_fixed_domain(input[0] as i32 * p.domain[0] as i32);
        let v2 = to_fixed_domain(input[1] as i32 * p.domain[1] as i32);
        let v3 = to_fixed_domain(input[2] as i32 * p.domain[2] as i32);

        t.x0[i] = p.opta[2].wrapping_mul((v1 >> 16) as u32);
        t.y0[i] = p.opta[1].wrapping_mul((v2 >> 16) as u32);
        t.z0[i] = p.opta[0].wrapping_mul((v3 >> 16) as u32);

        t.rx[i] = (v1 & 0xFFFF) as u16;
        t.ry[i] = (v2 & 0xFFFF) as u16;
        t.rz[i] = (v3 & 0xFFFF) as u16;
    }
    t
}

/// `PrelinEval8` — the tetrahedral 8-bit interpolation (`CMS_NO_SANITIZE`: wrapping i32).
fn prelin8_eval(
    t: &Prelin8Tables,
    lut: &[u16],
    p: &InterpParams,
    input: &[u16],
    output: &mut [u16],
) {
    let r = (input[0] >> 8) as usize;
    let g = (input[1] >> 8) as usize;
    let b = (input[2] >> 8) as usize;

    let x0 = t.x0[r] as i32;
    let y0 = t.y0[g] as i32;
    let z0 = t.z0[b] as i32;

    let rx = t.rx[r] as i32;
    let ry = t.ry[g] as i32;
    let rz = t.rz[b] as i32;

    let x1 = x0 + if rx == 0 { 0 } else { p.opta[2] as i32 };
    let y1 = y0 + if ry == 0 { 0 } else { p.opta[1] as i32 };
    let z1 = z0 + if rz == 0 { 0 } else { p.opta[0] as i32 };

    let total_out = p.n_outputs;
    let dens = |i: i32, j: i32, k: i32, oc: i32| lut[(i + j + k + oc) as usize] as i32;

    for oc in 0..total_out as i32 {
        let c0 = dens(x0, y0, z0, oc);
        let (c1, c2, c3);
        if rx >= ry && ry >= rz {
            c1 = dens(x1, y0, z0, oc) - c0;
            c2 = dens(x1, y1, z0, oc) - dens(x1, y0, z0, oc);
            c3 = dens(x1, y1, z1, oc) - dens(x1, y1, z0, oc);
        } else if rx >= rz && rz >= ry {
            c1 = dens(x1, y0, z0, oc) - c0;
            c2 = dens(x1, y1, z1, oc) - dens(x1, y0, z1, oc);
            c3 = dens(x1, y0, z1, oc) - dens(x1, y0, z0, oc);
        } else if rz >= rx && rx >= ry {
            c1 = dens(x1, y0, z1, oc) - dens(x0, y0, z1, oc);
            c2 = dens(x1, y1, z1, oc) - dens(x1, y0, z1, oc);
            c3 = dens(x0, y0, z1, oc) - c0;
        } else if ry >= rx && rx >= rz {
            c1 = dens(x1, y1, z0, oc) - dens(x0, y1, z0, oc);
            c2 = dens(x0, y1, z0, oc) - c0;
            c3 = dens(x1, y1, z1, oc) - dens(x1, y1, z0, oc);
        } else if ry >= rz && rz >= rx {
            c1 = dens(x1, y1, z1, oc) - dens(x0, y1, z1, oc);
            c2 = dens(x0, y1, z0, oc) - c0;
            c3 = dens(x0, y1, z1, oc) - dens(x0, y1, z0, oc);
        } else if rz >= ry && ry >= rx {
            c1 = dens(x1, y1, z1, oc) - dens(x0, y1, z1, oc);
            c2 = dens(x0, y1, z1, oc) - dens(x0, y0, z1, oc);
            c3 = dens(x0, y0, z1, oc) - c0;
        } else {
            c1 = 0;
            c2 = 0;
            c3 = 0;
        }

        let rest = c1
            .wrapping_mul(rx)
            .wrapping_add(c2.wrapping_mul(ry))
            .wrapping_add(c3.wrapping_mul(rz))
            .wrapping_add(0x8001);
        output[oc as usize] =
            c0.wrapping_add((rest.wrapping_add(rest >> 16)) >> 16) as u16;
    }
}

/// `MatShaperEval16` fast evaluator (`MatShaper8Data`): first shaper → 1.14 matrix → second
/// shaper. RGB only.
#[derive(Debug)]
struct MatShaper {
    /// 8-bit → 1.14 fixed, per channel (`Shaper1*`).
    shaper1: [[i32; 256]; 3],
    /// n.14 → n.14 matrix (`Mat`).
    mat: [[i32; 3]; 3],
    /// n.14 offsets (`Off`).
    off: [i32; 3],
    /// 1.14 → output, per channel (`Shaper2*`, 16385 entries).
    shaper2: [Vec<u16>; 3],
}

impl Eval16 for MatShaper {
    fn eval16(&self, input: &[u16], output: &mut [u16]) {
        // In[] is assured to come from an 8-bit number (a << 8 | a), so & 0xFF recovers it.
        let ri = (input[0] & 0xFF) as usize;
        let gi = (input[1] & 0xFF) as usize;
        let bi = (input[2] & 0xFF) as usize;

        let r = self.shaper1[0][ri];
        let g = self.shaper1[1][gi];
        let b = self.shaper1[2][bi];

        let eval_row = |row: usize| -> i32 {
            self.mat[row][0]
                .wrapping_mul(r)
                .wrapping_add(self.mat[row][1].wrapping_mul(g))
                .wrapping_add(self.mat[row][2].wrapping_mul(b))
                .wrapping_add(self.off[row])
                .wrapping_add(0x2000)
                >> 14
        };
        let l1 = eval_row(0);
        let l2 = eval_row(1);
        let l3 = eval_row(2);

        let clip = |l: i32| -> usize {
            if l < 0 {
                0
            } else if l > 16384 {
                16384
            } else {
                l as usize
            }
        };

        output[0] = self.shaper2[0][clip(l1)];
        output[1] = self.shaper2[1][clip(l2)];
        output[2] = self.shaper2[2][clip(l3)];
    }
}


/// `FillFirstShaper` — 8-bit → 1.14 after the curve.
fn fill_first_shaper(table: &mut [i32; 256], curve: &ToneCurve) {
    for (i, slot) in table.iter_mut().enumerate() {
        let r = (i as f64 / 255.0) as f32;
        let y = curve.eval_f32(r);
        *slot = if (y as f64) < 131072.0 {
            double_to_1fixed14(y as f64)
        } else {
            0x7fff_ffff
        };
    }
}

/// `FillSecondShaper` — 1.14 → output after the curve.
fn fill_second_shaper(table: &mut [u16], curve: &ToneCurve, is_8bits: bool) {
    for (i, slot) in table.iter_mut().enumerate() {
        let r = (i as f64 / 16384.0) as f32;
        // Val comes 0..1.0; clamp exactly as the C `if (Val<0) Val=0; if (Val>1) Val=1;`.
        let val = curve.eval_f32(r).clamp(0.0, 1.0);
        if is_8bits {
            // Quantize to 8 bits then re-expand (low byte == msb, so a later & 0xff works).
            let w = quick_saturate_word(val as f64 * 65535.0);
            let b = from_16_to_8(w);
            *slot = from_8_to_16(b);
        } else {
            *slot = quick_saturate_word(val as f64 * 65535.0);
        }
    }
}

/// `SetMatShaper` — build the matrix-shaper fast evaluator; marks 8-bit output as optimized.
fn set_mat_shaper(
    curve1: &[ToneCurve],
    mat: &Mat3,
    off: Option<[f64; 3]>,
    curve2: &[ToneCurve],
    output_format: &mut PixelFormat,
) -> MatShaper {
    let is_8bits = output_format.bytes() == 1;

    let mut shaper1 = [[0i32; 256], [0i32; 256], [0i32; 256]];
    fill_first_shaper(&mut shaper1[0], &curve1[0]);
    fill_first_shaper(&mut shaper1[1], &curve1[1]);
    fill_first_shaper(&mut shaper1[2], &curve1[2]);

    let mut shaper2 = [vec![0u16; 16385], vec![0u16; 16385], vec![0u16; 16385]];
    fill_second_shaper(&mut shaper2[0], &curve2[0], is_8bits);
    fill_second_shaper(&mut shaper2[1], &curve2[1], is_8bits);
    fill_second_shaper(&mut shaper2[2], &curve2[2], is_8bits);

    let mut m = [[0i32; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = double_to_1fixed14(mat.0[i].0[j]);
        }
    }

    let mut o = [0i32; 3];
    for (i, oi) in o.iter_mut().enumerate() {
        *oi = match off {
            Some(v) => double_to_1fixed14(v[i]),
            None => 0,
        };
    }

    if is_8bits {
        *output_format = PixelFormat::from_raw(output_format.raw() | OPTIMIZED_SH_1);
    }

    MatShaper { shaper1, mat: m, off: o, shaper2 }
}

// ---------------------------------------------------------------------------------------
// White-point fixup (FixWhiteMisalignment / PatchLUT / WhitesAreEqual).
// ---------------------------------------------------------------------------------------

/// `WhitesAreEqual` — equal, or so extremely different the fixup should be skipped.
fn whites_are_equal(n: usize, white1: &[u16], white2: &[u16]) -> bool {
    for i in 0..n {
        if (white1[i] as i32 - white2[i] as i32).abs() > 0xf000 {
            return true; // so different the fixup should be avoided
        }
        if white1[i] != white2[i] {
            return false;
        }
    }
    true
}

/// `PatchLUT` — overwrite the CLUT node located exactly at `at` with `value`. Works on 1/3/4
/// input channels; a non-exact node returns `false` (no change).
fn patch_lut(
    table: &mut [u16],
    p16: &InterpParams,
    at: &[u16],
    value: &[u16],
    n_channels_out: usize,
    n_channels_in: usize,
) -> bool {
    let index: i32;
    if n_channels_in == 4 {
        let px = (at[0] as f64 * p16.domain[0] as f64) / 65535.0;
        let py = (at[1] as f64 * p16.domain[1] as f64) / 65535.0;
        let pz = (at[2] as f64 * p16.domain[2] as f64) / 65535.0;
        let pw = (at[3] as f64 * p16.domain[3] as f64) / 65535.0;
        let x0 = px.floor() as i32;
        let y0 = py.floor() as i32;
        let z0 = pz.floor() as i32;
        let w0 = pw.floor() as i32;
        if (px - x0 as f64) != 0.0
            || (py - y0 as f64) != 0.0
            || (pz - z0 as f64) != 0.0
            || (pw - w0 as f64) != 0.0
        {
            return false; // not on an exact node
        }
        index = p16.opta[3] as i32 * x0
            + p16.opta[2] as i32 * y0
            + p16.opta[1] as i32 * z0
            + p16.opta[0] as i32 * w0;
    } else if n_channels_in == 3 {
        let px = (at[0] as f64 * p16.domain[0] as f64) / 65535.0;
        let py = (at[1] as f64 * p16.domain[1] as f64) / 65535.0;
        let pz = (at[2] as f64 * p16.domain[2] as f64) / 65535.0;
        let x0 = px.floor() as i32;
        let y0 = py.floor() as i32;
        let z0 = pz.floor() as i32;
        if (px - x0 as f64) != 0.0 || (py - y0 as f64) != 0.0 || (pz - z0 as f64) != 0.0 {
            return false;
        }
        index = p16.opta[2] as i32 * x0 + p16.opta[1] as i32 * y0 + p16.opta[0] as i32 * z0;
    } else if n_channels_in == 1 {
        let px = (at[0] as f64 * p16.domain[0] as f64) / 65535.0;
        let x0 = px.floor() as i32;
        if (px - x0 as f64) != 0.0 {
            return false;
        }
        index = p16.opta[0] as i32 * x0;
    } else {
        return false;
    }

    for i in 0..n_channels_out {
        table[index as usize + i] = value[i];
    }
    true
}

/// `FixWhiteMisalignment` — locate the white node and pin it to pure white (avoids scum dot).
///
/// Operates on the fast evaluator's CLUT table + optional pre/post curves. `obtained_out` is
/// white evaluated through the (un-patched) fast path. Returns `false` when the colour spaces
/// don't yield end-points or channel counts mismatch (`_cmsEndPointsBySpace` FALSE case).
#[allow(clippy::too_many_arguments)]
fn fix_white_misalignment(
    clut_table: &mut [u16],
    clut_params: &InterpParams,
    obtained_out: &[u16],
    curve_in: Option<&[ToneCurve]>,
    curve_out: Option<&[ToneCurve]>,
    entry_space: Signature,
    exit_space: Signature,
    n_ins: usize,
    n_outs: usize,
) -> bool {
    let ep_in = match end_points_by_space(entry_space) {
        Some(e) => e,
        None => return false,
    };
    let ep_out = match end_points_by_space(exit_space) {
        Some(e) => e,
        None => return false,
    };
    if n_ins != ep_in.n_outputs as usize || n_outs != ep_out.n_outputs as usize {
        return false;
    }

    if whites_are_equal(n_outs, &ep_out.white, obtained_out) {
        return true; // whites already match
    }

    // Interpolate the input white through the pre-curves.
    let mut white_in = [0u16; MAX_CHANNELS];
    for i in 0..n_ins {
        white_in[i] = match curve_in {
            Some(c) => c[i].eval_u16(ep_in.white[i]),
            None => ep_in.white[i],
        };
    }

    // Reverse the post-curves to find how white is represented before them.
    let mut white_out = [0u16; MAX_CHANNELS];
    for i in 0..n_outs {
        white_out[i] = match curve_out {
            Some(c) => match c[i].reverse() {
                Ok(inv) => inv.eval_u16(ep_out.white[i]),
                Err(_) => ep_out.white[i],
            },
            None => ep_out.white[i],
        };
    }

    // May fail; we don't care if it does.
    patch_lut(clut_table, clut_params, &white_in[..n_ins], &white_out[..n_outs], n_outs, n_ins);
    true
}

// ---------------------------------------------------------------------------------------
// Pre-optimization (identity removal, inverse-pair removal, matrix folding).
// ---------------------------------------------------------------------------------------

/// `_Remove1Op` — drop every stage whose `Implements` equals `unary_op`.
fn remove_1op(elems: &mut Vec<Stage>, unary_op: Signature) -> bool {
    let before = elems.len();
    elems.retain(|s| s.implements != unary_op);
    before != elems.len()
}

/// `_Remove2Op` — drop each adjacent `(op1, op2)` pair (by `Implements`), no advance on hit.
fn remove_2op(elems: &mut Vec<Stage>, op1: Signature, op2: Signature) -> bool {
    let mut any = false;
    let mut i = 0usize;
    while i + 1 < elems.len() {
        if elems[i].implements == op1 && elems[i + 1].implements == op2 {
            elems.remove(i + 1);
            elems.remove(i);
            any = true;
            // Do not advance: the element now at `i` is what followed the pair.
        } else {
            i += 1;
        }
    }
    any
}

/// `_MultiplyMatrix` — fold adjacent 3×3 offset-free matrix stages into their product.
///
/// Returns `false` (aborting the fold, as the C `return FALSE`) the moment an adjacent
/// matrix pair is found that has an offset or is not 3×3.
fn multiply_matrix(elems: &mut Vec<Stage>) -> bool {
    let mut any = false;
    let mut i = 0usize;
    while i + 1 < elems.len() {
        if elems[i].implements == STAGE_MATRIX && elems[i + 1].implements == STAGE_MATRIX {
            let (m1, off1) = match &elems[i].data {
                StageData::Matrix { m, offset, .. } => (m.clone(), offset.is_some()),
                _ => (Vec::new(), true),
            };
            let (m2, off2) = match &elems[i + 1].data {
                StageData::Matrix { m, offset, .. } => (m.clone(), offset.is_some()),
                _ => (Vec::new(), true),
            };

            if off1
                || off2
                || elems[i].input_channels != 3
                || elems[i].output_channels != 3
                || elems[i + 1].input_channels != 3
                || elems[i + 1].output_channels != 3
            {
                return false;
            }

            // res = m2 * m1.
            let res = Mat3::mul(&mat3_from9(&m2), &mat3_from9(&m1));

            elems.remove(i + 1);
            elems.remove(i);

            if !is_float_matrix_identity(&res) {
                let m9 = mat3_to9(&res);
                if let Ok(mult) = Stage::alloc_matrix(3, 3, &m9, None) {
                    elems.insert(i, mult);
                } else {
                    return false; // should never happen
                }
            }
            any = true;
            // Do not advance: re-examine at `i`.
        } else {
            i += 1;
        }
    }
    any
}

/// `PreOptimize` — repeatedly strip paired no-ops and fold matrices until nothing changes.
fn pre_optimize(lut: &mut Pipeline) -> bool {
    let in_ch = lut.input_channels();
    let out_ch = lut.output_channels();
    let mut elems: Vec<Stage> = lut.stages().to_vec();

    let mut any_opt = false;
    loop {
        let mut opt = false;
        opt |= remove_1op(&mut elems, STAGE_IDENTITY);
        opt |= remove_2op(&mut elems, STAGE_XYZ2LAB, STAGE_LAB2XYZ);
        opt |= remove_2op(&mut elems, STAGE_LAB2XYZ, STAGE_XYZ2LAB);
        opt |= remove_2op(&mut elems, STAGE_LABV4TOV2, STAGE_LABV2TOV4);
        opt |= remove_2op(&mut elems, STAGE_LABV2TOV4, STAGE_LABV4TOV2);
        opt |= remove_2op(&mut elems, STAGE_LAB2FLOATPCS, STAGE_FLOATPCS2LAB);
        opt |= remove_2op(&mut elems, STAGE_XYZ2FLOATPCS, STAGE_FLOATPCS2XYZ);
        opt |= multiply_matrix(&mut elems);
        if opt {
            any_opt = true;
        } else {
            break;
        }
    }

    rebuild_pipeline(lut, in_ch, out_ch, elems);
    any_opt
}

/// Replace `lut` with a fresh pipeline holding `elems` (channel counts recomputed by bless).
fn rebuild_pipeline(lut: &mut Pipeline, in_ch: u32, out_ch: u32, elems: Vec<Stage>) {
    let mut p = Pipeline::alloc(in_ch, out_ch).expect("valid channel counts");
    for s in elems {
        p.insert_stage(StageLoc::End, s);
    }
    *lut = p;
}

/// Replace `lut`'s chain with the serializable `stages` and install `eval` as the fast
/// whole-pipeline evaluator (`_cmsPipelineSetOptimizationParameters`).
///
/// Evaluation then runs `eval` (identical bytes to the former single fused stage), but the stage
/// chain stays intact so serialization / [`crate::virt::find_combination`] still sees discrete
/// curve/matrix/CLUT stages. `eval` MUST be functionally equivalent to `stages`.
fn install_optimized(
    lut: &mut Pipeline,
    in_ch: u32,
    out_ch: u32,
    stages: Vec<Stage>,
    eval: Arc<dyn Eval16 + Send + Sync>,
) {
    rebuild_pipeline(lut, in_ch, out_ch, stages);
    lut.set_optimization_parameters(eval);
}

/// `XFormSampler16` + `cmsStageAllocCLut16bit` — resample `src` into a fresh 16-bit CLUT and
/// return its table + geometry.
fn sample_clut(
    src: &Pipeline,
    n_grid_points: u32,
    input_channels: u32,
    output_channels: u32,
) -> Result<(Vec<u16>, InterpParams)> {
    let mut stage = Stage::alloc_clut_16bit(n_grid_points, input_channels, output_channels, None)?;
    let ni = input_channels as usize;
    let no = output_channels as usize;
    let ok = stage.sample_clut_16(
        |in16: &[u16], out16: &mut [u16]| {
            let mut inf = [0f32; MAX_CHANNELS];
            let mut outf = [0f32; MAX_CHANNELS];
            for i in 0..ni {
                inf[i] = in16[i] as f32 / 65535.0;
            }
            src.eval_float(&inf[..ni], &mut outf[..no]);
            for i in 0..no {
                out16[i] = quick_saturate_word(outf[i] as f64 * 65535.0);
            }
            true
        },
        0,
    );
    if !ok {
        return Err(cms_err!(ErrorCode::Internal, "CLUT resampling failed"));
    }
    match stage.data {
        StageData::Clut(c) => match c.table {
            ClutTable::U16(t) => Ok((t, c.params)),
            ClutTable::F32(_) => Err(cms_err!(ErrorCode::Internal, "unexpected float CLUT")),
        },
        _ => Err(cms_err!(ErrorCode::Internal, "sampled stage is not a CLUT")),
    }
}

// ---------------------------------------------------------------------------------------
// Optimizer: joining curves.
// ---------------------------------------------------------------------------------------

/// `OptimizeByJoiningCurves` — collapse an all-curves pipeline into one lookup (or identity).
fn optimize_by_joining_curves(
    lut: &mut Pipeline,
    _intent: u32,
    input_format: &mut PixelFormat,
    output_format: &mut PixelFormat,
    dw_flags: &mut u32,
) -> Result<bool> {
    // Lossy: does not apply in floating-point cases.
    if input_format.float() || output_format.float() {
        return Ok(false);
    }

    // Only curves in this LUT?
    for s in lut.stages() {
        if s.type_signature() != STAGE_CURVE_SET {
            return Ok(false);
        }
    }

    let n_in = lut.input_channels() as usize;
    let n_out = lut.output_channels() as usize;

    // Compute the joined 16-bit tables by evaluating a gray ramp in floating point.
    let mut tables: Vec<Vec<u16>> = vec![vec![0u16; PRELINEARIZATION_POINTS]; n_in];
    let mut inf = [0f32; MAX_CHANNELS];
    let mut outf = [0f32; MAX_CHANNELS];
    for i in 0..PRELINEARIZATION_POINTS {
        let v = (i as f64 / (PRELINEARIZATION_POINTS - 1) as f64) as f32;
        for f in inf.iter_mut().take(n_in) {
            *f = v;
        }
        lut.eval_float(&inf[..n_in], &mut outf[..n_out]);
        for (j, tab) in tables.iter_mut().enumerate() {
            tab[i] = quick_saturate_word(outf[j] as f64 * 65535.0);
        }
    }

    let curves: Vec<ToneCurve> = tables
        .iter()
        .map(|t| ToneCurve::build_tabulated_16(PRELINEARIZATION_POINTS, t))
        .collect::<Result<_>>()?;

    if !all_curves_are_linear(&curves) {
        *dw_flags |= FLAGS_NOCACHE;
        let is8 = input_format.bytes() == 1;
        let n_elements = if is8 { 256 } else { 65536 };
        let fast = curves_alloc(&curves, n_elements);
        // Discrete, serializable form: one merged curve-set stage.
        let stages = vec![Stage::alloc_tone_curves(n_in as u32, Some(&curves))?];
        let eval = Arc::new(CurvesFast { curves: fast, is8 });
        install_optimized(lut, n_in as u32, n_in as u32, stages, eval);
    } else {
        // LUT optimizes to nothing: the identity.
        *dw_flags |= FLAGS_NOCACHE;
        let mut dest = Pipeline::alloc(n_in as u32, n_in as u32)?;
        dest.insert_stage(StageLoc::End, Stage::alloc_identity(n_in as u32));
        *lut = dest;
    }
    Ok(true)
}

// ---------------------------------------------------------------------------------------
// Optimizer: matrix-shaper (8-bit RGB).
// ---------------------------------------------------------------------------------------

/// `OptimizeMatrixShaper` — fold shaper-matrix-(matrix-)shaper into a fast 1.14 evaluator.
fn optimize_matrix_shaper(
    lut: &mut Pipeline,
    intent: u32,
    input_format: &mut PixelFormat,
    output_format: &mut PixelFormat,
    dw_flags: &mut u32,
) -> Result<bool> {
    // Only RGB to RGB, 8-bit input.
    if input_format.channels() != 3 || output_format.channels() != 3 {
        return Ok(false);
    }
    if input_format.bytes() != 1 {
        return Ok(false);
    }

    let stages = lut.stages();
    let res: Mat3;
    let offset: Option<[f64; 3]>;
    let mut identity_mat = false;
    let curve1_idx: usize;
    let curve2_idx: usize;

    if let Some(m) =
        check_and_retrieve(stages, &[STAGE_CURVE_SET, STAGE_MATRIX, STAGE_MATRIX, STAGE_CURVE_SET])
    {
        // Only RGB to RGB.
        if m[1].input_channels != 3
            || m[1].output_channels != 3
            || m[2].input_channels != 3
            || m[2].output_channels != 3
        {
            return Ok(false);
        }
        let (mat1, off1) = match as_mat3(m[1]) {
            Some(v) => v,
            None => return Ok(false),
        };
        let (mat2, off2) = match as_mat3(m[2]) {
            Some(v) => v,
            None => return Ok(false),
        };
        // Input offset should be zero.
        if off1.is_some() {
            return Ok(false);
        }
        // res = mat2 * mat1.
        res = Mat3::mul(&mat2, &mat1);
        offset = off2;
        if res.is_identity() && offset.is_none() {
            identity_mat = true;
        }
        curve1_idx = 0;
        curve2_idx = 3;
    } else if let Some(m) =
        check_and_retrieve(stages, &[STAGE_CURVE_SET, STAGE_MATRIX, STAGE_CURVE_SET])
    {
        if m[1].input_channels != 3 || m[1].output_channels != 3 {
            return Ok(false);
        }
        let (mat1, off1) = match as_mat3(m[1]) {
            Some(v) => v,
            None => return Ok(false),
        };
        res = mat1;
        offset = off1;
        if res.is_identity() && offset.is_none() {
            identity_mat = true;
        }
        curve1_idx = 0;
        curve2_idx = 2;
    } else {
        return Ok(false);
    }

    let n_in = lut.input_channels();
    let n_out = lut.output_channels();

    if identity_mat {
        // Assemble Curve1 + Curve2 and let the join-curves routine collapse them.
        let mut dest = Pipeline::alloc(n_in, n_out)?;
        dest.insert_stage(StageLoc::Begin, stages[curve1_idx].clone());
        dest.insert_stage(StageLoc::End, stages[curve2_idx].clone());
        // Return value ignored, exactly as in C.
        optimize_by_joining_curves(&mut dest, intent, input_format, output_format, dw_flags)?;
        *lut = dest;
    } else {
        // Cache does not help here; the fast evaluator handles pixels directly.
        *dw_flags |= FLAGS_NOCACHE;
        let c1 = stage_curves(&stages[curve1_idx]).expect("curve set").to_vec();
        let c2 = stage_curves(&stages[curve2_idx]).expect("curve set").to_vec();
        let matshaper = set_mat_shaper(&c1, &res, offset, &c2, output_format);
        // Discrete, serializable form: input curves -> matrix -> output curves.
        let m9 = mat3_to9(&res);
        let discrete = vec![
            Stage::alloc_tone_curves(3, Some(&c1))?,
            Stage::alloc_matrix(3, 3, &m9, offset.as_ref().map(|o| o.as_slice()))?,
            Stage::alloc_tone_curves(3, Some(&c2))?,
        ];
        install_optimized(lut, n_in, n_out, discrete, Arc::new(matshaper));
    }
    Ok(true)
}

// ---------------------------------------------------------------------------------------
// Optimizer: computing linearization (prelin + CLUT), RGB.
// ---------------------------------------------------------------------------------------

/// `OptimizeByComputingLinearization` — fit per-channel prelinearization curves, then resample
/// the remainder into a CLUT and install the `Prelin8`/`Prelin16` fast evaluator.
fn optimize_by_computing_linearization(
    lut: &mut Pipeline,
    intent: u32,
    input_format: &mut PixelFormat,
    output_format: &mut PixelFormat,
    dw_flags: &mut u32,
) -> Result<bool> {
    // Lossy: does not apply in floating-point cases.
    if input_format.float() || output_format.float() {
        return Ok(false);
    }
    // Only on chunky RGB.
    if input_format.colorspace() != PT_RGB || input_format.planar() {
        return Ok(false);
    }
    if output_format.colorspace() != PT_RGB || output_format.planar() {
        return Ok(false);
    }
    // On 16 bits, the user has to specify the feature.
    if input_format.bytes() != 1 && (*dw_flags & FLAGS_CLUT_PRE_LINEARIZATION) == 0 {
        return Ok(false);
    }

    let color_space = icc_colorspace(input_format.colorspace() as i32);
    let output_color_space = icc_colorspace(output_format.colorspace() as i32);
    if color_space == Signature(0) || output_color_space == Signature(0) {
        return Ok(false);
    }

    let n_grid_points = reasonable_gridpoints_by_colorspace(color_space, *dw_flags);
    let n_in = lut.input_channels() as usize;
    let n_out = lut.output_channels() as usize;

    // If the last stage is degenerate curves, the transform is squeezing/clipping: bail.
    match lut.last_stage() {
        None => return Ok(false),
        Some(last) => {
            if last.type_signature() == STAGE_CURVE_SET {
                let curves = stage_curves(last).expect("curve set");
                for c in curves {
                    if is_degenerated(c.estimated_table()) {
                        return Ok(false);
                    }
                }
            }
        }
    }

    // Build the forward transfer curves from a gray ramp.
    let mut trans: Vec<Vec<u16>> = vec![vec![0u16; PRELINEARIZATION_POINTS]; n_in];
    let mut inf = [0f32; MAX_CHANNELS];
    let mut outf = [0f32; MAX_CHANNELS];
    for i in 0..PRELINEARIZATION_POINTS {
        let v = (i as f64 / (PRELINEARIZATION_POINTS - 1) as f64) as f32;
        for f in inf.iter_mut().take(n_in) {
            *f = v;
        }
        lut.eval_float(&inf[..n_in], &mut outf[..n_out]);
        for (t, tab) in trans.iter_mut().enumerate() {
            tab[i] = quick_saturate_word(outf[t] as f64 * 65535.0);
        }
    }

    // Slope-limit the obtained curves.
    for tab in trans.iter_mut() {
        slope_limiting(tab);
    }

    // Suitability: monotonic and not degenerate. (Linearity is only for debugging in C.)
    for tab in &trans {
        let c = ToneCurve::build_tabulated_16(PRELINEARIZATION_POINTS, tab)?;
        if !c.is_monotonic() || is_degenerated(tab) {
            return Ok(false);
        }
    }

    // Reverse the curves.
    let mut trans_reverse: Vec<ToneCurve> = Vec::with_capacity(n_in);
    for tab in &trans {
        let c = ToneCurve::build_tabulated_16(PRELINEARIZATION_POINTS, tab)?;
        trans_reverse.push(c.reverse_ex(PRELINEARIZATION_POINTS)?);
    }

    // Insert the reversed curves at the begin of a duplicate transform, then resample it.
    let mut lut_plus = lut.clone();
    lut_plus.insert_stage(
        StageLoc::Begin,
        Stage::alloc_tone_curves(n_in as u32, Some(&trans_reverse))?,
    );

    let (mut clut_table, clut_params) =
        sample_clut(&lut_plus, n_grid_points, n_in as u32, n_out as u32)?;

    // The forward curves become the prelinearization.
    let trans_curves: Vec<ToneCurve> = trans
        .iter()
        .map(|t| ToneCurve::build_tabulated_16(PRELINEARIZATION_POINTS, t))
        .collect::<Result<_>>()?;

    let is8 = input_format.bytes() == 1;

    // Don't fix white on absolute colorimetric.
    if intent == INTENT_ABSOLUTE_COLORIMETRIC {
        *dw_flags |= FLAGS_NOWHITEONWHITEFIXUP;
    }

    if is8 {
        let tables = prelin8_alloc(&clut_params, &trans_curves);
        if (*dw_flags & FLAGS_NOWHITEONWHITEFIXUP) == 0 {
            let mut obtained = [0u16; MAX_STAGE_CHANNELS];
            let ep = end_points_by_space(color_space);
            if let Some(ep) = ep {
                prelin8_eval(&tables, &clut_table, &clut_params, &ep.white, &mut obtained[..n_out]);
            }
            if !fix_white_misalignment(
                &mut clut_table,
                &clut_params,
                &obtained[..n_out],
                Some(&trans_curves),
                None,
                color_space,
                output_color_space,
                n_in,
                n_out,
            ) {
                return Ok(false);
            }
        }
        // Discrete, serializable form: prelinearization curves -> resampled CLUT.
        let clut_stage =
            Stage::alloc_clut_16bit(n_grid_points, n_in as u32, n_out as u32, Some(&clut_table))?;
        let discrete = vec![
            Stage::alloc_tone_curves(n_in as u32, Some(&trans_curves))?,
            clut_stage,
        ];
        let eval = Arc::new(Prelin8 { t: tables, clut_table, clut_params });
        install_optimized(lut, n_in as u32, n_out as u32, discrete, eval);
    } else {
        if (*dw_flags & FLAGS_NOWHITEONWHITEFIXUP) == 0 {
            let mut obtained = [0u16; MAX_STAGE_CHANNELS];
            let ep = end_points_by_space(color_space);
            if let Some(ep) = ep {
                prelin16_eval(
                    Some(&trans_curves),
                    &clut_table,
                    &clut_params,
                    None,
                    &ep.white[..n_in],
                    &mut obtained[..n_out],
                );
            }
            if !fix_white_misalignment(
                &mut clut_table,
                &clut_params,
                &obtained[..n_out],
                Some(&trans_curves),
                None,
                color_space,
                output_color_space,
                n_in,
                n_out,
            ) {
                return Ok(false);
            }
        }
        // Discrete, serializable form: prelinearization curves -> resampled CLUT.
        let clut_stage =
            Stage::alloc_clut_16bit(n_grid_points, n_in as u32, n_out as u32, Some(&clut_table))?;
        let discrete = vec![
            Stage::alloc_tone_curves(n_in as u32, Some(&trans_curves))?,
            clut_stage,
        ];
        let eval = Arc::new(Prelin16 {
            curve_in: Some(trans_curves),
            curve_out: None,
            clut_table,
            clut_params,
        });
        install_optimized(lut, n_in as u32, n_out as u32, discrete, eval);
    }
    Ok(true)
}

// ---------------------------------------------------------------------------------------
// Optimizer: resampling.
// ---------------------------------------------------------------------------------------

/// `OptimizeByResampling` — resample the whole transform into a CLUT, optionally keeping the
/// existing pre/post linearization curves.
fn optimize_by_resampling(
    lut: &mut Pipeline,
    intent: u32,
    input_format: &mut PixelFormat,
    output_format: &mut PixelFormat,
    dw_flags: &mut u32,
) -> Result<bool> {
    // Lossy: does not apply in floating-point cases.
    if input_format.float() || output_format.float() {
        return Ok(false);
    }

    let color_space = icc_colorspace(input_format.colorspace() as i32);
    let output_color_space = icc_colorspace(output_format.colorspace() as i32);
    if color_space == Signature(0) || output_color_space == Signature(0) {
        return Ok(false);
    }

    let n_grid_points = if lut.stage_count() == 0 {
        2 // For empty LUTs, 2 points are enough.
    } else {
        // Lab16 as input cannot be optimized by a CLUT (centering issues).
        if (*dw_flags & FLAGS_FORCE_CLUT) == 0
            && color_space == colorspace::LAB
            && input_format.bytes() == 2
        {
            return Ok(false);
        }
        reasonable_gridpoints_by_colorspace(color_space, *dw_flags)
    };

    let n_in = lut.input_channels();
    let n_out = lut.output_channels();

    // Source copy to sample; pre/post curves are peeled off it if kept.
    let mut src = lut.clone();

    // Prelinearization curves are kept unless indicated by flags.
    let curve_in: Option<Vec<ToneCurve>> = if (*dw_flags & FLAGS_CLUT_PRE_LINEARIZATION) != 0 {
        let kept = src.first_stage().and_then(|s| {
            if s.type_signature() == STAGE_CURVE_SET {
                stage_curves(s).filter(|c| !all_curves_are_linear(c)).map(|c| c.to_vec())
            } else {
                None
            }
        });
        if kept.is_some() {
            src.unlink_stage(StageLoc::Begin);
        }
        kept
    } else {
        None
    };

    // Postlinearization curves are kept unless indicated by flags.
    let curve_out: Option<Vec<ToneCurve>> = if (*dw_flags & FLAGS_CLUT_POST_LINEARIZATION) != 0 {
        let kept = src.last_stage().and_then(|s| {
            if s.type_signature() == STAGE_CURVE_SET {
                stage_curves(s).filter(|c| !all_curves_are_linear(c)).map(|c| c.to_vec())
            } else {
                None
            }
        });
        if kept.is_some() {
            src.unlink_stage(StageLoc::End);
        }
        kept
    } else {
        None
    };

    let (mut clut_table, clut_params) = sample_clut(&src, n_grid_points, n_in, n_out)?;

    // Don't fix white on absolute colorimetric.
    if intent == INTENT_ABSOLUTE_COLORIMETRIC {
        *dw_flags |= FLAGS_NOWHITEONWHITEFIXUP;
    }

    if (*dw_flags & FLAGS_NOWHITEONWHITEFIXUP) == 0 {
        let mut obtained = [0u16; MAX_STAGE_CHANNELS];
        if let Some(ep) = end_points_by_space(color_space) {
            prelin16_eval(
                curve_in.as_deref(),
                &clut_table,
                &clut_params,
                curve_out.as_deref(),
                &ep.white[..n_in as usize],
                &mut obtained[..n_out as usize],
            );
        }
        // Return value ignored here (C does not gate on it in resampling).
        fix_white_misalignment(
            &mut clut_table,
            &clut_params,
            &obtained[..n_out as usize],
            curve_in.as_deref(),
            curve_out.as_deref(),
            color_space,
            output_color_space,
            n_in as usize,
            n_out as usize,
        );
    }

    // Discrete, serializable form: optional prelin curves -> resampled CLUT -> optional postlin
    // curves (the same curve stages the fast Prelin16 evaluator carries).
    let clut_stage = Stage::alloc_clut_16bit(n_grid_points, n_in, n_out, Some(&clut_table))?;
    let mut discrete: Vec<Stage> = Vec::new();
    if let Some(ci) = &curve_in {
        discrete.push(Stage::alloc_tone_curves(n_in, Some(ci))?);
    }
    discrete.push(clut_stage);
    if let Some(co) = &curve_out {
        discrete.push(Stage::alloc_tone_curves(n_out, Some(co))?);
    }

    let eval = Arc::new(Prelin16 { curve_in, curve_out, clut_table, clut_params });
    install_optimized(lut, n_in, n_out, discrete, eval);
    Ok(true)
}

// ---------------------------------------------------------------------------------------
// Driver (_cmsOptimizePipeline).
// ---------------------------------------------------------------------------------------

/// The built-in optimizer collection (`DefaultOptimization[]`), tried in order.
type OptimizerFn =
    fn(&mut Pipeline, u32, &mut PixelFormat, &mut PixelFormat, &mut u32) -> Result<bool>;

const DEFAULT_OPTIMIZATION: [OptimizerFn; 4] = [
    optimize_by_joining_curves,
    optimize_matrix_shaper,
    optimize_by_computing_linearization,
    optimize_by_resampling,
];

/// `_cmsOptimizePipeline` — the entry point for LUT optimization.
///
/// On return `*lut` is a usable pipeline in every case (either the optimized form or the
/// pre-optimized chain). `input_format`/`output_format`/`flags` are updated in place exactly as
/// the C code mutates its `InputFormat`/`OutputFormat`/`dwFlags` out-parameters.
///
/// Plugin optimizers (`_cmsRegisterOptimizationPlugin`, via [`crate::plugin::optimizers`]) are
/// tried before the built-ins — newest first — then [`DEFAULT_OPTIMIZATION`].
pub fn optimize_pipeline(
    lut: &mut Pipeline,
    intent: u32,
    input_format: &mut PixelFormat,
    output_format: &mut PixelFormat,
    flags: &mut u32,
) -> Result<()> {
    optimize_pipeline_for(None, lut, intent, input_format, output_format, flags)
}

/// [`optimize_pipeline`] against the optimizers visible to `ctx`.
///
/// C's `_cmsOptimizePipeline` takes the `ContextID` as its first argument precisely because the
/// optimizer list it walks is a per-context chunk (`cmsopt.c`, `_cmsRegisterOptimizationPlugin`).
/// This is that form; the plain [`optimize_pipeline`] is the `ContextID == NULL` call, which
/// resolves to Context 0.
pub fn optimize_pipeline_thr(
    ctx: &Context,
    lut: &mut Pipeline,
    intent: u32,
    input_format: &mut PixelFormat,
    output_format: &mut PixelFormat,
    flags: &mut u32,
) -> Result<()> {
    optimize_pipeline_for(Some(ctx), lut, intent, input_format, output_format, flags)
}

/// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
fn optimize_pipeline_for(
    ctx: Option<&Context>,
    lut: &mut Pipeline,
    intent: u32,
    input_format: &mut PixelFormat,
    output_format: &mut PixelFormat,
    flags: &mut u32,
) -> Result<()> {
    // A CLUT is being asked for, so force that specific optimization.
    if (*flags & FLAGS_FORCE_CLUT) != 0 {
        pre_optimize(lut);
        optimize_by_resampling(lut, intent, input_format, output_format, flags)?;
        return Ok(());
    }

    // Anything to optimize? An empty pipeline evaluates as the identity (FastIdentity16).
    if lut.stage_count() == 0 {
        return Ok(());
    }

    // Named color pipelines cannot be optimized.
    for s in lut.stages() {
        if s.type_signature() == STAGE_NAMED_COLOR {
            return Ok(());
        }
    }

    // Try to get rid of identities and trivial conversions.
    pre_optimize(lut);

    // After removal do we end with an identity?
    if lut.stage_count() == 0 {
        return Ok(());
    }

    // Do not optimize, keep all precision.
    if (*flags & FLAGS_NOOPTIMIZE) != 0 {
        return Ok(());
    }

    // Plugin optimizers (`_cmsRegisterOptimizationPlugin`) are tried first — newest first —
    // exactly as lcms walks its context optimizer list ahead of `DefaultOptimization`.
    for opt in crate::plugin::optimizers(ctx) {
        if opt(lut, intent, input_format, output_format, flags)? {
            return Ok(());
        }
    }

    // Try built-in optimizations; the first that succeeds wins.
    for opt in DEFAULT_OPTIMIZATION {
        if opt(lut, intent, input_format, output_format, flags)? {
            return Ok(());
        }
    }

    // Only simple optimizations succeeded (or none) — the pipeline is still usable.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::format::FormatBuilder;

    /// TYPE_RGB_16 = COLORSPACE_SH(PT_RGB) | CHANNELS_SH(3) | BYTES_SH(2).
    fn rgb16() -> PixelFormat {
        FormatBuilder::new().colorspace(PT_RGB).channels(3).bytes(2).build()
    }
    /// TYPE_RGB_8.
    fn rgb8() -> PixelFormat {
        FormatBuilder::new().colorspace(PT_RGB).channels(3).bytes(1).build()
    }

    /// A deterministic set of RGB probe points in 0..1.
    fn probes() -> Vec<[f32; 3]> {
        let mut v = Vec::new();
        for &r in &[0.0f32, 0.13, 0.37, 0.5, 0.62, 0.88, 1.0] {
            for &g in &[0.05f32, 0.42, 0.77, 1.0] {
                for &b in &[0.0f32, 0.31, 0.66, 0.95] {
                    v.push([r, g, b]);
                }
            }
        }
        v
    }

    #[test]
    fn multiply_matrix_yields_single_equivalent_matrix() {
        // Two non-trivial 3x3 matrices; PreOptimize (under NOOPTIMIZE) must fold them.
        let m1 = [1.2, 0.1, 0.0, 0.05, 0.9, 0.1, 0.0, 0.2, 1.1];
        let m2 = [0.8, 0.0, 0.1, 0.1, 1.05, 0.0, 0.0, 0.15, 0.95];
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        lut.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m1, None).unwrap());
        lut.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m2, None).unwrap());
        let orig = lut.clone();

        let mut inf = rgb16();
        let mut outf = rgb16();
        let mut flags = FLAGS_NOOPTIMIZE;
        optimize_pipeline(&mut lut, 0, &mut inf, &mut outf, &mut flags).unwrap();

        assert_eq!(lut.stage_count(), 1, "matrix+matrix must fold to one stage");
        assert_eq!(lut.stages()[0].type_signature(), STAGE_MATRIX);

        for p in probes() {
            let mut a = [0f32; 3];
            let mut b = [0f32; 3];
            orig.eval_float(&p, &mut a);
            lut.eval_float(&p, &mut b);
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() < 1e-4, "{p:?}: {a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn joining_curves_collapses_to_equivalent_lookup() {
        // gamma 2.2 then gamma 3.0 — non-linear joined curve.
        let g1 = ToneCurve::build_gamma(2.2).unwrap();
        let g2 = ToneCurve::build_gamma(3.0).unwrap();
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        lut.insert_stage(
            StageLoc::End,
            Stage::alloc_tone_curves(3, Some(&[g1.clone(), g1.clone(), g1])).unwrap(),
        );
        lut.insert_stage(
            StageLoc::End,
            Stage::alloc_tone_curves(3, Some(&[g2.clone(), g2.clone(), g2])).unwrap(),
        );
        let orig = lut.clone();

        let mut inf = rgb16();
        let mut outf = rgb16();
        let mut flags = 0u32;
        optimize_pipeline(&mut lut, 0, &mut inf, &mut outf, &mut flags).unwrap();

        assert_eq!(lut.stage_count(), 1, "all-curves must collapse to one fast stage");
        assert!(flags & FLAGS_NOCACHE != 0, "NOCACHE must be set");

        // The optimized fast lookup must match the original curve chain closely.
        for p in probes() {
            let mut in16 = [0u16; 3];
            crate::pipeline::stage::from_float_to_16(&p, &mut in16);
            let mut a = [0u16; 3];
            let mut b = [0u16; 3];
            orig.eval_16(&in16, &mut a);
            lut.eval_16(&in16, &mut b);
            for k in 0..3 {
                let d = (a[k] as i32 - b[k] as i32).abs();
                assert!(d <= 8, "{p:?} chan {k}: {a:?} vs {b:?} (d={d})");
            }
        }
    }

    /// H1 regression: `eval_float` must NOT run the optimizer's fast evaluator.
    ///
    /// `_cmsPipelineSetOptimizationParameters` (`cmslut.c:1674`) installs `Eval16Fn` only;
    /// `EvalFloatFn` stays `_LUTevalFloat` (`cmslut.c:1354`) and walks the discrete chain.
    ///
    /// The probe has to be chosen with care. For the 16-bit optimizers (`CurvesFast`,
    /// `Prelin16`) the discrete replacement chain is a tabulated curve / 16-bit CLUT, and both
    /// quantize in float **by design** in C too (`cmsEvalToneCurveFloat`'s table detour,
    /// `cmsgamma.c:1412`; `EvaluateCLUTfloatIn16`) — so there the two lanes legitimately agree
    /// and nothing can be asserted.
    ///
    /// `MatShaper` is where the lanes genuinely part: its discrete chain is
    /// `[curves, matrix, curves]`, whose matrix stage evaluates in full precision, while the
    /// fast evaluator indexes an **8-bit** shaper (`In & 0xFF`, `optimize.rs`) and runs a 1.14
    /// fixed-point matrix. So the float lane must track the unoptimized pipeline far more
    /// closely than 8-bit granularity (1/255 ≈ 3.9e-3).
    #[test]
    fn optimized_pipeline_float_lane_bypasses_the_8bit_fast_evaluator() {
        // shaper -> matrix -> shaper, RGB, 8-bit input: exactly OptimizeMatrixShaper's shape.
        let g = ToneCurve::build_gamma(2.2).unwrap();
        let m = [0.9, 0.05, 0.02, 0.03, 0.95, 0.04, 0.01, 0.06, 0.88];
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        lut.insert_stage(
            StageLoc::End,
            Stage::alloc_tone_curves(3, Some(&[g.clone(), g.clone(), g.clone()])).unwrap(),
        );
        lut.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m, None).unwrap());
        lut.insert_stage(
            StageLoc::End,
            Stage::alloc_tone_curves(3, Some(&[g.clone(), g.clone(), g])).unwrap(),
        );
        let orig = lut.clone();

        let mut inf = rgb8();
        let mut outf = rgb8();
        let mut flags = 0u32;
        optimize_pipeline(&mut lut, 0, &mut inf, &mut outf, &mut flags).unwrap();

        // Guard: the test only bites if a fast evaluator really got installed.
        assert!(lut.is_optimized(), "a fast evaluator must be installed for this test to bite");

        // Probe strictly between two 8-bit codes, where the fast path would snap to one of them.
        let p = [0.5f32 + 1.0 / 512.0, 0.25 + 1.0 / 512.0, 0.75 + 1.0 / 512.0];
        let mut a = [0f32; 3];
        let mut b = [0f32; 3];
        orig.eval_float(&p, &mut a);
        lut.eval_float(&p, &mut b);

        for k in 0..3 {
            let d = (a[k] - b[k]).abs();
            assert!(
                d < 1.0 / 4096.0,
                "chan {k}: float lane is {d} away from the unoptimized chain ({a:?} vs {b:?}) \
                 - eval_float is running the 8-bit fast evaluator"
            );
        }

        // The 16-bit lane must still take the fast path (that is what C optimizes). Inputs must
        // be 8-bit-derived (`a << 8 | a`) — MatShaperEval16 recovers the byte with `In & 0xFF`.
        let mut c = [0u16; 3];
        lut.eval_16(&[0x8080, 0x4040, 0xc0c0], &mut c);
        assert!(c.iter().any(|&v| v != 0), "16-bit fast path must still produce output");
    }

    #[test]
    fn joining_identity_curves_yields_identity() {
        // gamma 2.2 then gamma 1/2.2 -> linear -> identity.
        let g = ToneCurve::build_gamma(2.2).unwrap();
        let gi = ToneCurve::build_gamma(1.0 / 2.2).unwrap();
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        lut.insert_stage(
            StageLoc::End,
            Stage::alloc_tone_curves(3, Some(&[g.clone(), g.clone(), g])).unwrap(),
        );
        lut.insert_stage(
            StageLoc::End,
            Stage::alloc_tone_curves(3, Some(&[gi.clone(), gi.clone(), gi])).unwrap(),
        );

        let mut inf = rgb16();
        let mut outf = rgb16();
        let mut flags = 0u32;
        optimize_pipeline(&mut lut, 0, &mut inf, &mut outf, &mut flags).unwrap();

        assert_eq!(lut.stage_count(), 1);
        assert_eq!(lut.stages()[0].type_signature(), STAGE_IDENTITY);
    }

    #[test]
    fn resampling_matches_original_within_tolerance() {
        // curves + matrix -> not all-curves, 16-bit, no PRE_LINEARIZATION -> resampling.
        let g = ToneCurve::build_gamma(2.0).unwrap();
        let m = [0.95, 0.02, 0.0, 0.03, 0.9, 0.02, 0.0, 0.05, 0.98];
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        lut.insert_stage(
            StageLoc::End,
            Stage::alloc_tone_curves(3, Some(&[g.clone(), g.clone(), g])).unwrap(),
        );
        lut.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m, None).unwrap());
        let orig = lut.clone();

        let mut inf = rgb16();
        let mut outf = rgb16();
        // Absolute colorimetric so the white-fixup does not perturb the comparison.
        let mut flags = 0u32;
        optimize_pipeline(&mut lut, INTENT_ABSOLUTE_COLORIMETRIC, &mut inf, &mut outf, &mut flags)
            .unwrap();

        // #17: resampling (no pre/post linearization kept) yields a single *serializable* CLUT
        // stage with the fast Prelin16 evaluator installed — not a fused Custom stage.
        assert_eq!(lut.stage_count(), 1, "resampling yields a single CLUT stage");
        assert_eq!(lut.stages()[0].type_signature(), crate::pipeline::stage::STAGE_CLUT);
        assert!(lut.is_optimized(), "fast evaluator installed");

        // 33-node RGB CLUT with a smooth transform: tetrahedral error is small.
        for p in probes() {
            let mut a = [0f32; 3];
            let mut b = [0f32; 3];
            orig.eval_float(&p, &mut a);
            lut.eval_float(&p, &mut b);
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() < 0.02, "{p:?} chan {k}: {a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn matrix_shaper_8bit_matches_original() {
        // shaper-matrix-shaper on 8-bit RGB -> matrix-shaper fast evaluator.
        let g = ToneCurve::build_gamma(2.2).unwrap();
        let gi = ToneCurve::build_gamma(1.0 / 2.2).unwrap();
        let m = [0.9, 0.05, 0.05, 0.05, 0.9, 0.05, 0.05, 0.05, 0.9];
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        lut.insert_stage(
            StageLoc::End,
            Stage::alloc_tone_curves(3, Some(&[g.clone(), g.clone(), g])).unwrap(),
        );
        lut.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m, None).unwrap());
        lut.insert_stage(
            StageLoc::End,
            Stage::alloc_tone_curves(3, Some(&[gi.clone(), gi.clone(), gi])).unwrap(),
        );
        let orig = lut.clone();

        let mut inf = rgb8();
        let mut outf = rgb8();
        let mut flags = 0u32;
        optimize_pipeline(&mut lut, INTENT_ABSOLUTE_COLORIMETRIC, &mut inf, &mut outf, &mut flags)
            .unwrap();

        // #17: the optimized pipeline keeps discrete, serializable stages (curves -> matrix ->
        // curves) with the fast MatShaper evaluator installed as the pipeline's fast path, rather
        // than a single fused Custom stage.
        assert_eq!(lut.stage_count(), 3, "matrix-shaper keeps [curves, matrix, curves]");
        assert_eq!(lut.stages()[0].type_signature(), STAGE_CURVE_SET);
        assert_eq!(lut.stages()[1].type_signature(), STAGE_MATRIX);
        assert_eq!(lut.stages()[2].type_signature(), STAGE_CURVE_SET);
        assert!(lut.is_optimized(), "fast evaluator installed");
        assert!(outf.optimized(), "8-bit output must be marked OPTIMIZED");

        // 8-bit input arrives as byte*257; compare the fast path to the naive chain.
        for byte_probe in [
            [0u8, 0, 0],
            [255, 255, 255],
            [128, 64, 200],
            [10, 250, 130],
            [77, 77, 77],
            [200, 30, 90],
        ] {
            let in16: Vec<u16> = byte_probe.iter().map(|&x| from_8_to_16(x)).collect();
            let mut a = [0u16; 3];
            let mut b = [0u16; 3];
            orig.eval_16(&in16, &mut a);
            lut.eval_16(&in16, &mut b);
            for k in 0..3 {
                // Matrix-shaper works in 1.14 fixed point: allow a small tolerance.
                let d = (a[k] as i32 - b[k] as i32).abs();
                assert!(d <= 300, "{byte_probe:?} chan {k}: {a:?} vs {b:?} (d={d})");
            }
        }
    }

    #[test]
    fn nooptimize_leaves_pipeline_unchanged() {
        // A lone non-identity matrix: PreOptimize can't touch it, NOOPTIMIZE skips optimizers.
        let m = [0.5, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5];
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        lut.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m, None).unwrap());
        let orig = lut.clone();

        let mut inf = rgb16();
        let mut outf = rgb16();
        let mut flags = FLAGS_NOOPTIMIZE;
        optimize_pipeline(&mut lut, 0, &mut inf, &mut outf, &mut flags).unwrap();

        assert_eq!(lut.stage_count(), 1);
        assert_eq!(lut.stages()[0].type_signature(), STAGE_MATRIX);
        for p in probes() {
            let mut a = [0f32; 3];
            let mut b = [0f32; 3];
            orig.eval_float(&p, &mut a);
            lut.eval_float(&p, &mut b);
            assert_eq!(a, b, "{p:?}");
        }
    }

    #[test]
    fn empty_pipeline_is_identity() {
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        let mut inf = rgb16();
        let mut outf = rgb16();
        let mut flags = 0u32;
        optimize_pipeline(&mut lut, 0, &mut inf, &mut outf, &mut flags).unwrap();
        assert_eq!(lut.stage_count(), 0);
        let in16 = [0x1234u16, 0x8000, 0xffff];
        let mut out16 = [0u16; 3];
        lut.eval_16(&in16, &mut out16);
        assert_eq!(out16, in16);
    }
}
