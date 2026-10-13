//! Tone curves (gamma) — a bit-exact port of `cmsgamma.c`.
//!
//! A [`ToneCurve`] is a curve stored as a list of [`CurveSegment`]s, where each segment is
//! either *sampled* (`seg_type == 0`, evaluated by 1-D float interpolation) or *parametric*
//! (evaluated by [`default_eval_parametric_fn`]). Alongside the segments a limited-precision
//! 16-bit table (`table16`) is kept for fast `u16` evaluation, exactly as lcms2 does.
//!
//! # Fidelity (do not "clean up")
//! Every parametric formula, the `MATRIX_DET_TOLERANCE` domain guards, the ±1e22 sentinels
//! and the float operation order are transcribed verbatim from `cmsgamma.c`. Signed
//! parametric types denote the analytic inverse: the *signed* key is kept in the segment and
//! `abs(Type)` is used to look the function up (see [`get_parametric_curve_by_type`]).
//!
//! # Thread-safety note
//! C's `EvalSegmentedFn` mutates `SegInterp[i]->Table` in place before each sampled lerp
//! (`cmsgamma.c` ~L742). This port does **not** replicate that mutation: [`InterpParams`]
//! carries only geometry and the sampled points are passed to `eval_float` locally, so
//! [`ToneCurve::eval_segmented`] stays a pure `&self` method.

use crate::cms_err;
use crate::context::Context;
use crate::error::{ErrorCode, Result};
use crate::math::interp::{CMS_LERP_FLAGS_FLOAT, InterpParams};
use crate::types::num::quick_saturate_word;

/// Maximum number of nodes handled by smoothing/estimation (`MAX_NODES_IN_CURVE`).
pub const MAX_NODES_IN_CURVE: usize = 4097;

/// `MINUS_INF` domain sentinel as a `f32` (matches C's `-1E22F`).
const MINUS_INF_F32: f32 = -1e22;
/// `PLUS_INF` domain sentinel as a `f32` (matches C's `+1E22F`).
const PLUS_INF_F32: f32 = 1e22;

/// `PLUS_INF` as promoted to `f64` (C promotes the `float` `1E22F`).
const PLUS_INF: f64 = 1e22_f32 as f64;
/// `MINUS_INF` as promoted to `f64`.
const MINUS_INF: f64 = -1e22_f32 as f64;

/// `MATRIX_DET_TOLERANCE` (`lcms2_internal.h`): the domain/near-zero guard for parametrics.
const MATRIX_DET_TOLERANCE: f64 = 0.0001;

/// Signature of a parametric curve evaluator (`cmsParametricCurveEvaluator`).
///
/// The built-in curves use [`default_eval_parametric_fn`]; extra curve types come from the
/// process-global [`crate::plugin`] registry (`_cmsRegisterParametricCurvesPlugin`), consulted
/// ahead of the built-ins by [`get_parametric_curve_by_type`] / [`resolve_parametric_eval`].
pub type ParametricEvaluator = fn(i32, &[f64; 10], f64) -> f64;

/// The 10 built-in parametric curve identifiers (`DefaultCurves.FunctionTypes`).
const DEFAULT_TYPES: [i32; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 108, 109];
/// Parameter counts per built-in type (`DefaultCurves.ParameterCount`).
const DEFAULT_PARAM_COUNTS: [usize; 10] = [1, 3, 4, 5, 7, 4, 5, 5, 1, 1];

/// Look up a parametric curve by (signed) type, returning its parameter count.
///
/// Ports `GetParametricCurveByType`: the plugin registry
/// (`_cmsRegisterParametricCurvesPlugin`) is consulted first, then the built-in
/// `DefaultCurves`. The lookup keys on `abs(Type)` so an inverse (negative) type resolves to
/// the same collection. Returns `None` for the sampled marker `0` and any unknown type.
pub fn get_parametric_curve_by_type(type_: i32) -> Option<usize> {
    get_parametric_curve_by_type_for(None, type_)
}

/// [`get_parametric_curve_by_type`] against the curve plugins visible to `ctx`.
///
/// C's `GetParametricCurveByType` takes the `ContextID` as its first argument because the
/// curve list it walks is a per-context chunk (`cmsgamma.c:129`).
pub fn get_parametric_curve_by_type_thr(ctx: &Context, type_: i32) -> Option<usize> {
    get_parametric_curve_by_type_for(Some(ctx), type_)
}

/// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
fn get_parametric_curve_by_type_for(ctx: Option<&Context>, type_: i32) -> Option<usize> {
    if let Some((count, _)) = crate::plugin::find_parametric_curve(ctx, type_) {
        return Some(count);
    }
    DEFAULT_TYPES
        .iter()
        .position(|&t| type_.abs() == t)
        .map(|i| DEFAULT_PARAM_COUNTS[i])
}

/// Resolve the evaluator for a (signed) parametric type: a registered plugin's evaluator wins
/// over the built-in [`default_eval_parametric_fn`]. `None` means the type is unknown (a
/// sampled/unsupported segment).
fn resolve_parametric_eval(
    ctx: Option<&Context>,
    type_: i32,
) -> Option<ParametricEvaluator> {
    if let Some((_, ev)) = crate::plugin::find_parametric_curve(ctx, type_) {
        return Some(ev);
    }
    DEFAULT_TYPES
        .iter()
        .any(|&t| type_.abs() == t)
        .then_some(default_eval_parametric_fn as ParametricEvaluator)
}

/// One curve segment (`cmsCurveSegment`).
///
/// For `x0 < x <= x1`; `seg_type == 0` marks a sampled segment (see `sampled_points`),
/// otherwise it is a parametric segment described by `params`.
#[derive(Debug, Clone)]
pub struct CurveSegment {
    /// Domain lower bound (exclusive).
    pub x0: f32,
    /// Domain upper bound (inclusive).
    pub x1: f32,
    /// Parametric type; `0` means a sampled segment, negatives are analytic inverses.
    pub seg_type: i32,
    /// Up to 10 parameters (used when `seg_type != 0`).
    pub params: [f64; 10],
    /// Number of grid points (used when `seg_type == 0`).
    pub n_grid_points: u32,
    /// Sampled points (used when `seg_type == 0`).
    pub sampled_points: Vec<f32>,
}

impl CurveSegment {
    /// A zeroed segment, mirroring `memset(&Seg, 0, ...)` before field assignment.
    fn zeroed() -> Self {
        Self {
            x0: 0.0,
            x1: 0.0,
            seg_type: 0,
            params: [0.0; 10],
            n_grid_points: 0,
            sampled_points: Vec::new(),
        }
    }
}

/// A tone curve (`cmsToneCurve`).
///
/// Owns the parametric/sampled `segments`, the pre-located per-segment evaluators
/// (`evals`) and sampled-segment interpolation geometry (`seg_interp`), and the
/// limited-precision `table16` with its 1-D interpolation geometry (`interp_params`).
#[derive(Debug, Clone)]
pub struct ToneCurve {
    /// Number of segments (`nSegments`).
    n_segments: usize,
    /// The curve segments (`Segments`). Empty when the curve is table-only.
    segments: Vec<CurveSegment>,
    /// Per-segment evaluator handle (`Evals`); `None` for sampled/unknown types.
    evals: Vec<Option<ParametricEvaluator>>,
    /// Per-segment 1-D interpolation geometry for sampled segments (`SegInterp`).
    seg_interp: Vec<Option<InterpParams>>,
    /// Number of entries in `table16` (`nEntries`).
    n_entries: usize,
    /// Limited-precision 16-bit sampling of the whole curve (`Table16`).
    table16: Vec<u16>,
    /// 1-D `u16` interpolation geometry over `table16` (`InterpParams`).
    interp_params: Option<InterpParams>,
}

// ---------------------------------------------------------------------------------------
// Sigmoidals (used by parametric types 109 / -109)
// ---------------------------------------------------------------------------------------

/// `sigmoid_base` — steepness-`k` sigmoid centred on 0.
#[inline]
fn sigmoid_base(k: f64, t: f64) -> f64 {
    (1.0 / (1.0 + (-k * t).exp())) - 0.5
}

/// `inverted_sigmoid_base` — inverse of [`sigmoid_base`].
#[inline]
fn inverted_sigmoid_base(k: f64, t: f64) -> f64 {
    -((1.0 / (t + 0.5)) - 1.0).ln() / k
}

/// `sigmoid_factory` — sigmoid remapped to pass through (0,0) and (1,1).
#[inline]
fn sigmoid_factory(k: f64, t: f64) -> f64 {
    let correction = 0.5 / sigmoid_base(k, 1.0);
    correction * sigmoid_base(k, 2.0 * t - 1.0) + 0.5
}

/// `inverse_sigmoid_factory` — inverse of [`sigmoid_factory`].
#[inline]
fn inverse_sigmoid_factory(k: f64, t: f64) -> f64 {
    let correction = 0.5 / sigmoid_base(k, 1.0);
    (inverted_sigmoid_base(k, (t - 0.5) / correction) + 1.0) / 2.0
}

/// The built-in parametric evaluator (`DefaultEvalParametricFn`).
///
/// Evaluates one of the 10 ICC parametric curve types (or its negative analytic inverse) at
/// `r`. Transcribed formula-for-formula from `cmsgamma.c`, including every
/// `MATRIX_DET_TOLERANCE` guard and the `pow`/`log`/`log10`/`exp` operation order. Returns 0
/// for an unsupported type, as the C `default:` branch does.
pub fn default_eval_parametric_fn(type_: i32, params: &[f64; 10], r: f64) -> f64 {
    let val: f64;
    match type_ {
        // X = Y ^ Gamma
        1 => {
            if r < 0.0 {
                if (params[0] - 1.0).abs() < MATRIX_DET_TOLERANCE {
                    val = r;
                } else {
                    val = 0.0;
                }
            } else {
                val = r.powf(params[0]);
            }
        }

        // Type 1 reversed: X = Y ^ 1/gamma
        -1 => {
            if r < 0.0 {
                if (params[0] - 1.0).abs() < MATRIX_DET_TOLERANCE {
                    val = r;
                } else {
                    val = 0.0;
                }
            } else if params[0].abs() < MATRIX_DET_TOLERANCE {
                val = PLUS_INF;
            } else {
                val = r.powf(1.0 / params[0]);
            }
        }

        // CIE 122-1966: Y = (aX + b)^Gamma | X >= -b/a ; else 0
        2 => {
            if params[1].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                let disc = -params[2] / params[1];
                if r >= disc {
                    let e = params[1] * r + params[2];
                    if e > 0.0 {
                        val = e.powf(params[0]);
                    } else {
                        val = 0.0;
                    }
                } else {
                    val = 0.0;
                }
            }
        }

        // Type 2 reversed: X = (Y^1/g - b) / a
        -2 => {
            if params[0].abs() < MATRIX_DET_TOLERANCE || params[1].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                let mut v = if r < 0.0 {
                    0.0
                } else {
                    (r.powf(1.0 / params[0]) - params[2]) / params[1]
                };
                if v < 0.0 {
                    v = 0.0;
                }
                val = v;
            }
        }

        // IEC 61966-3: Y = (aX + b)^Gamma + c | X >= disc ; else c
        3 => {
            if params[1].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                let mut disc = -params[2] / params[1];
                if disc < 0.0 {
                    disc = 0.0;
                }
                if r >= disc {
                    let e = params[1] * r + params[2];
                    if e > 0.0 {
                        val = e.powf(params[0]) + params[3];
                    } else {
                        val = 0.0;
                    }
                } else {
                    val = params[3];
                }
            }
        }

        // Type 3 reversed
        -3 => {
            if params[0].abs() < MATRIX_DET_TOLERANCE || params[1].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else if r >= params[3] {
                let e = r - params[3];
                if e > 0.0 {
                    val = (e.powf(1.0 / params[0]) - params[2]) / params[1];
                } else {
                    val = 0.0;
                }
            } else {
                val = -params[2] / params[1];
            }
        }

        // IEC 61966-2.1 (sRGB): Y = (aX + b)^Gamma | X >= d ; else cX
        4 => {
            if r >= params[4] {
                let e = params[1] * r + params[2];
                if e > 0.0 {
                    val = e.powf(params[0]);
                } else {
                    val = 0.0;
                }
            } else {
                val = r * params[3];
            }
        }

        // Type 4 reversed
        -4 => {
            let e = params[1] * params[4] + params[2];
            let disc = if e < 0.0 { 0.0 } else { e.powf(params[0]) };
            if r >= disc {
                if params[0].abs() < MATRIX_DET_TOLERANCE || params[1].abs() < MATRIX_DET_TOLERANCE
                {
                    val = 0.0;
                } else {
                    val = (r.powf(1.0 / params[0]) - params[2]) / params[1];
                }
            } else if params[3].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                val = r / params[3];
            }
        }

        // Y = (aX + b)^Gamma + e | X >= d ; else cX + f
        5 => {
            if r >= params[4] {
                let e = params[1] * r + params[2];
                if e > 0.0 {
                    val = e.powf(params[0]) + params[5];
                } else {
                    val = params[5];
                }
            } else {
                val = r * params[3] + params[6];
            }
        }

        // Type 5 reversed
        -5 => {
            let disc = params[3] * params[4] + params[6];
            if r >= disc {
                let e = r - params[5];
                // C nests `e < 0` and the tolerance guards as separate `Val = 0` branches;
                // merged here (all yield 0 with no side effects) to keep it clippy-clean.
                if e < 0.0
                    || params[0].abs() < MATRIX_DET_TOLERANCE
                    || params[1].abs() < MATRIX_DET_TOLERANCE
                {
                    val = 0.0;
                } else {
                    val = (e.powf(1.0 / params[0]) - params[2]) / params[1];
                }
            } else if params[3].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                val = (r - params[6]) / params[3];
            }
        }

        // Y = (a*X + b)^Gamma + c  (type 6; identical to 5 without d). Gamma 1.0 => no clamp.
        6 => {
            let e = params[1] * r + params[2];
            if params[0] == 1.0 {
                val = e + params[3];
            } else if e < 0.0 {
                val = params[3];
            } else {
                val = e.powf(params[0]) + params[3];
            }
        }

        // Type 6 reversed: ((Y - c)^1/Gamma - b) / a
        -6 => {
            if params[0].abs() < MATRIX_DET_TOLERANCE || params[1].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                let e = r - params[3];
                if e < 0.0 {
                    val = 0.0;
                } else {
                    val = (e.powf(1.0 / params[0]) - params[2]) / params[1];
                }
            }
        }

        // Y = a * log10(b * X^Gamma + c) + d
        7 => {
            let e = params[2] * r.powf(params[0]) + params[3];
            if e <= 0.0 {
                val = params[4];
            } else {
                val = params[1] * e.log10() + params[4];
            }
        }

        // Type 7 reversed: X = ((10^((Y-d)/a) - c) / b)^(1/g)
        -7 => {
            if params[0].abs() < MATRIX_DET_TOLERANCE
                || params[1].abs() < MATRIX_DET_TOLERANCE
                || params[2].abs() < MATRIX_DET_TOLERANCE
            {
                val = 0.0;
            } else {
                val = ((10.0_f64.powf((r - params[4]) / params[1]) - params[3]) / params[2])
                    .powf(1.0 / params[0]);
            }
        }

        // Y = a * b^(c*X + d) + e
        8 => {
            val = params[0] * params[1].powf(params[2] * r + params[3]) + params[4];
        }

        // Type 8 reversed: Y = (log((y-e)/a)/log(b) - d) / c
        -8 => {
            let disc = r - params[4];
            // C nests `disc < 0` and the tolerance guards as separate `Val = 0` branches;
            // merged here (all yield 0 with no side effects) to keep it clippy-clean.
            if disc < 0.0
                || params[0].abs() < MATRIX_DET_TOLERANCE
                || params[2].abs() < MATRIX_DET_TOLERANCE
            {
                val = 0.0;
            } else {
                val = ((disc / params[0]).ln() / params[1].ln() - params[3]) / params[2];
            }
        }

        // S-shaped: (1 - (1-x)^1/g)^1/g
        108 => {
            if params[0].abs() < MATRIX_DET_TOLERANCE {
                val = 0.0;
            } else {
                val = (1.0 - (1.0 - r).powf(1.0 / params[0])).powf(1.0 / params[0]);
            }
        }

        // Type 108 reversed: 1 - (1 - y^g)^g
        -108 => {
            val = 1.0 - (1.0 - r.powf(params[0])).powf(params[0]);
        }

        // Sigmoidals
        109 => {
            val = sigmoid_factory(params[0], r);
        }
        -109 => {
            val = inverse_sigmoid_factory(params[0], r);
        }

        // Unsupported parametric curve. Should never reach here.
        _ => return 0.0,
    }
    val
}

/// `EntriesByGamma` — sample count heuristic for a pure gamma segment.
fn entries_by_gamma(gamma: f64) -> u32 {
    if (gamma - 1.0).abs() < 0.001 {
        return 2;
    }
    4096
}

impl ToneCurve {
    // -----------------------------------------------------------------------------------
    // Allocation
    // -----------------------------------------------------------------------------------

    /// Low-level allocator (`AllocateToneCurveStruct`).
    ///
    /// `n_entries` may be 0 (no optimization table) and `segments` may be empty (inverse
    /// case, table only), but not both. Locates the per-segment evaluator and sampled-
    /// segment interpolation geometry up front, exactly as the C code does.
    fn allocate(
        ctx: Option<&Context>,
        n_entries: usize,
        segments: &[CurveSegment],
        values: Option<&[u16]>,
    ) -> Result<Self> {
        // We allow huge tables, which are then restricted for smoothing operations.
        if n_entries > 65530 {
            return Err(cms_err!(
                ErrorCode::Range,
                "Couldn't create tone curve of more than 65530 entries"
            ));
        }
        if n_entries == 0 && segments.is_empty() {
            return Err(cms_err!(
                ErrorCode::Range,
                "Couldn't create tone curve with zero segments and no table"
            ));
        }

        // 16-bit limited-precision table.
        let mut table16 = vec![0u16; n_entries];
        if let Some(v) = values {
            for (dst, &src) in table16.iter_mut().zip(v.iter()).take(n_entries) {
                *dst = src;
            }
        }

        // Per-segment evaluator + sampled interpolation geometry.
        let n_segments = segments.len();
        let mut out_segments = Vec::with_capacity(n_segments);
        let mut evals: Vec<Option<ParametricEvaluator>> = Vec::with_capacity(n_segments);
        let mut seg_interp: Vec<Option<InterpParams>> = Vec::with_capacity(n_segments);

        for seg in segments {
            // Type 0 is a special marker for table-based curves.
            let interp = if seg.seg_type == 0 {
                Some(InterpParams::new_for(
                    ctx,
                    seg.n_grid_points,
                    1,
                    1,
                    CMS_LERP_FLAGS_FLOAT,
                )?)
            } else {
                None
            };

            let mut s = seg.clone();
            // Only sampled segments keep their points; everything else drops them.
            let keep_points = seg.seg_type == 0 && !seg.sampled_points.is_empty();
            if !keep_points {
                s.sampled_points = Vec::new();
            }

            // Pre-locate the evaluator: a registered plugin curve wins over the built-ins;
            // None => sampled/unknown.
            let eval: Option<ParametricEvaluator> = if seg.seg_type == 0 {
                None
            } else {
                resolve_parametric_eval(ctx, seg.seg_type)
            };

            out_segments.push(s);
            evals.push(eval);
            seg_interp.push(interp);
        }

        // 1-D interpolation over the whole 16-bit table (CMS_LERP_FLAGS_16BITS == 0).
        let interp_params = if n_entries > 0 {
            Some(InterpParams::new_for(ctx, n_entries as u32, 1, 1, 0)?)
        } else {
            None
        };

        Ok(Self {
            n_segments,
            segments: out_segments,
            evals,
            seg_interp,
            n_entries,
            table16,
            interp_params,
        })
    }

    // -----------------------------------------------------------------------------------
    // Builders
    // -----------------------------------------------------------------------------------

    /// `cmsBuildTabulatedToneCurve16` — a table-only curve (no float description).
    pub fn build_tabulated_16(n_entries: usize, values: &[u16]) -> Result<Self> {
        Self::allocate(None, n_entries, &[], Some(values))
    }

    /// `cmsBuildTabulatedToneCurve16THR` — [`Self::build_tabulated_16`] against `ctx`.
    pub fn build_tabulated_16_thr(
        ctx: &Context,
        n_entries: usize,
        values: &[u16],
    ) -> Result<Self> {
        Self::allocate(Some(ctx), n_entries, &[], Some(values))
    }

    /// `cmsBuildSegmentedToneCurve` — build a segmented curve and fill its 16-bit table.
    pub fn build_segmented(segments: &[CurveSegment]) -> Result<Self> {
        Self::build_segmented_for(None, segments)
    }

    /// `cmsBuildSegmentedToneCurveTHR` — [`Self::build_segmented`] against `ctx`.
    pub fn build_segmented_thr(ctx: &Context, segments: &[CurveSegment]) -> Result<Self> {
        Self::build_segmented_for(Some(ctx), segments)
    }

    /// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
    fn build_segmented_for(ctx: Option<&Context>, segments: &[CurveSegment]) -> Result<Self> {
        // STRICTER THAN C, deliberately. `cmsBuildSegmentedToneCurve` does not reject
        // `nSegments == 0`: `AllocateToneCurveStruct` only bails when entries AND segments are
        // both zero (`cmsgamma.c:226`), so a zero-segment call builds a 4096-entry table filled
        // by `EvalSegmentedFn`, which with no segments returns MINUS_INF for every input - i.e.
        // a silently all-zero curve. Refusing is better than handing back that, and this
        // function already returns `Result`, so it costs nothing. It was an `assert!` before,
        // which panicked across a safe `pub` API.
        if segments.is_empty() {
            return Err(cms_err!(ErrorCode::Range, "cmsBuildSegmentedToneCurve: no segments"));
        }

        // Optimization for identity/gamma curves.
        let mut n_grid: u32 = 4096;
        if segments.len() == 1 && segments[0].seg_type == 1 {
            n_grid = entries_by_gamma(segments[0].params[0]);
        }

        let mut g = Self::allocate(ctx, n_grid as usize, segments, None)?;

        // Approximate a 16-bit table (used mainly on 8/16-bit transforms).
        for i in 0..n_grid as usize {
            let r = i as f64 / (n_grid - 1) as f64;
            let val = g.eval_segmented(r);
            g.table16[i] = quick_saturate_word(val * 65535.0);
        }

        Ok(g)
    }

    /// `ReadSegmentedCurve`'s implicit-point fix-up (`cmstypes.c:4326-4334`).
    ///
    /// In the ICC `curf` encoding the first sample of a sampled (`samf`) segment is implicit and
    /// never serialized, so the tag reader parks a `0.0` placeholder at `sampled_points[0]`.
    /// That slot is not just a serialization placeholder — it is a live interpolation knot read
    /// by [`Self::eval_segmented`] — so C repairs every type-0 segment after building the curve,
    /// by evaluating the curve itself at that segment's `x0`.
    ///
    /// **Placement matters.** C runs this in the *tag reader*, not inside
    /// `cmsBuildSegmentedToneCurve`, and this port follows it: for a curve built by
    /// [`Self::build_tabulated_float`] the same slot holds the caller's genuine first sample,
    /// which must not be overwritten. Only a curve parsed from a tag needs the repair.
    pub(crate) fn fix_implicit_sampled_points(&mut self) {
        // Evaluate first, assign after: `eval_segmented` borrows the segments immutably.
        let fixed: Vec<(usize, f32)> = self
            .segments
            .iter()
            .enumerate()
            .filter(|(_, s)| s.seg_type == 0 && !s.sampled_points.is_empty())
            .map(|(i, s)| (i, s.x0 as f64))
            .map(|(i, x0)| (i, self.eval_segmented(x0) as f32))
            .collect();
        for (i, v) in fixed {
            self.segments[i].sampled_points[0] = v;
        }
    }

    /// `cmsBuildTabulatedToneCurveFloat` — wrap float samples in the standard 3 segments
    /// (constant type-6 head / sampled type-0 body / constant type-6 tail).
    pub fn build_tabulated_float(values: &[f32]) -> Result<Self> {
        Self::build_tabulated_float_for(None, values)
    }

    /// `cmsBuildTabulatedToneCurveFloatTHR` — [`Self::build_tabulated_float`] against `ctx`.
    pub fn build_tabulated_float_thr(ctx: &Context, values: &[f32]) -> Result<Self> {
        Self::build_tabulated_float_for(Some(ctx), values)
    }

    /// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
    fn build_tabulated_float_for(ctx: Option<&Context>, values: &[f32]) -> Result<Self> {
        let n_entries = values.len();
        if n_entries == 0 {
            return Err(cms_err!(ErrorCode::Null, "cmsBuildTabulatedToneCurveFloat: no values"));
        }

        let mut seg = [
            CurveSegment::zeroed(),
            CurveSegment::zeroed(),
            CurveSegment::zeroed(),
        ];

        // Constant = samples[0] up to 0.
        seg[0].x0 = MINUS_INF_F32;
        seg[0].x1 = 0.0;
        seg[0].seg_type = 6;
        seg[0].params[0] = 1.0;
        seg[0].params[1] = 0.0;
        seg[0].params[2] = 0.0;
        seg[0].params[3] = values[0] as f64;
        seg[0].params[4] = 0.0;

        // Sampled from 0 to 1.
        seg[1].x0 = 0.0;
        seg[1].x1 = 1.0;
        seg[1].seg_type = 0;
        seg[1].n_grid_points = n_entries as u32;
        seg[1].sampled_points = values.to_vec();

        // Constant = last sample beyond 1.
        seg[2].x0 = 1.0;
        seg[2].x1 = PLUS_INF_F32;
        seg[2].seg_type = 6;
        seg[2].params[0] = 1.0;
        seg[2].params[1] = 0.0;
        seg[2].params[2] = 0.0;
        seg[2].params[3] = values[n_entries - 1] as f64;
        seg[2].params[4] = 0.0;

        Self::build_segmented_for(ctx, &seg)
    }

    /// `cmsBuildParametricToneCurve` — a single ±INF-domain parametric segment.
    ///
    /// Type is the ICC type + 1; a negative type analytically inverts the curve.
    pub fn build_parametric(type_: i32, params: &[f64]) -> Result<Self> {
        Self::build_parametric_for(None, type_, params)
    }

    /// `cmsBuildParametricToneCurveTHR` — [`Self::build_parametric`] against `ctx`.
    pub fn build_parametric_thr(ctx: &Context, type_: i32, params: &[f64]) -> Result<Self> {
        Self::build_parametric_for(Some(ctx), type_, params)
    }

    /// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
    fn build_parametric_for(
        ctx: Option<&Context>,
        type_: i32,
        params: &[f64],
    ) -> Result<Self> {
        let count = get_parametric_curve_by_type_for(ctx, type_).ok_or_else(|| {
            cms_err!(ErrorCode::UnknownExtension, "Invalid parametric curve type {}", type_)
        })?;

        let mut seg0 = CurveSegment::zeroed();
        seg0.x0 = MINUS_INF_F32;
        seg0.x1 = PLUS_INF_F32;
        seg0.seg_type = type_;
        seg0.params[..count].copy_from_slice(&params[..count]);

        Self::build_segmented_for(ctx, std::slice::from_ref(&seg0))
    }

    /// `cmsBuildGamma` — a gamma curve (parametric type 1).
    pub fn build_gamma(gamma: f64) -> Result<Self> {
        Self::build_parametric(1, &[gamma])
    }

    /// `cmsBuildGammaTHR` — [`Self::build_gamma`] against `ctx`.
    pub fn build_gamma_thr(ctx: &Context, gamma: f64) -> Result<Self> {
        Self::build_parametric_thr(ctx, 1, &[gamma])
    }

    /// `cmsDupToneCurve` — deep copy. Equivalent to re-allocating from segments + table.
    pub fn dup(&self) -> Self {
        self.clone()
    }

    // -----------------------------------------------------------------------------------
    // Evaluation
    // -----------------------------------------------------------------------------------

    /// `EvalSegmentedFn` — evaluate the float description at `r`.
    ///
    /// Returns `MINUS_INF` when no segment matches. Sampled segments are interpolated with
    /// the segment's own points threaded into the lerp locally (no interior mutation).
    fn eval_segmented(&self, r: f64) -> f64 {
        for i in (0..self.n_segments).rev() {
            let seg = &self.segments[i];

            // Check for domain: x0 < R <= x1.
            if r > seg.x0 as f64 && r <= seg.x1 as f64 {
                let out: f64;
                if seg.seg_type == 0 {
                    // R1 = (float)(R - x0) / (x1 - x0)  (float arithmetic, matching C).
                    let r1 = ((r - seg.x0 as f64) as f32) / (seg.x1 - seg.x0);
                    let interp = self.seg_interp[i]
                        .expect("sampled segment must have interpolation geometry");
                    let mut o = [0f32];
                    interp.eval_float(&seg.sampled_points, &[r1], &mut o);
                    out = o[0] as f64;
                } else if let Some(eval) = self.evals[i] {
                    out = eval(seg.seg_type, &seg.params, r);
                } else {
                    out = 0.0;
                }

                // C: isinf(Out) catches both signs and returns PLUS_INF; the MINUS_INF
                // branch below it is dead code. Replicate the observable behaviour.
                if out.is_infinite() {
                    return PLUS_INF;
                }
                return out;
            }
        }
        MINUS_INF
    }

    /// `cmsEvalToneCurveFloat` — accurate float evaluation.
    pub fn eval_f32(&self, v: f32) -> f32 {
        // Limited-precision (table-only) curve.
        if self.n_segments == 0 {
            let in_ = quick_saturate_word(v as f64 * 65535.0);
            let out = self.eval_u16(in_);
            return out as f32 / 65535.0;
        }
        self.eval_segmented(v as f64) as f32
    }

    /// `cmsEvalToneCurve16` — fast 16-bit evaluation via the table Lerp16.
    pub fn eval_u16(&self, v: u16) -> u16 {
        match &self.interp_params {
            Some(p) => {
                let mut out = [0u16];
                p.eval16(&self.table16, &[v], &mut out);
                out[0]
            }
            None => v,
        }
    }

    // -----------------------------------------------------------------------------------
    // Reverse / join
    // -----------------------------------------------------------------------------------

    /// `cmsReverseToneCurveEx` — reverse the curve into `n_result_samples` points.
    ///
    /// If the curve is a single positive parametric segment, it is inverted analytically
    /// (`-Type`). Otherwise the 16-bit table is reversed numerically via [`Self::get_interval`].
    /// # Resolves against Context 0
    /// C reaches a context here indirectly, through `InCurve->InterpParams->ContextID`
    /// (`cmsgamma.c:1083`, `:1085`, `:1091`): `_cms_curve_struct` itself stores no context
    /// (`lcms2_internal.h:880-893`), but `cmsInterpParams` does (`cmsintrp.c:134`). This port
    /// deliberately does not: [`InterpParams`] stores the *resolved* kernel pointer rather than
    /// a context (`cmsintrp.c:93`, `:98`), which is what keeps it `Copy` and lifetime-free.
    /// There is therefore no context to recover from `&self`, and reaching one would mean
    /// changing this method's public signature. So the parametric lookup below resolves to
    /// Context 0 — correct for every curve built through the non-`*THR` builders, and for any
    /// curve whose parametric type is a built-in.
    pub fn reverse_ex(&self, n_result_samples: usize) -> Result<Self> {
        // Try to reverse it analytically whenever possible.
        if self.n_segments == 1
            && self.segments[0].seg_type > 0
            && get_parametric_curve_by_type(self.segments[0].seg_type).is_some()
        {
            return Self::build_parametric(-self.segments[0].seg_type, &self.segments[0].params);
        }

        // Nope, reverse the table.
        let mut out = Self::build_tabulated_16(n_result_samples, &vec![0u16; n_result_samples])?;

        // Is this an ascending or descending table?
        let ascending = !self.is_descending();

        // `a`/`b` persist across iterations: when no interval is found the previous
        // slope/intercept is reused, exactly as in C.
        let mut a = 0.0f64;
        let mut b = 0.0f64;

        for i in 0..n_result_samples {
            let y = i as f64 * 65535.0 / (n_result_samples - 1) as f64;

            let j = self.get_interval(y);
            if j >= 0 {
                let j = j as usize;
                let x1 = self.table16[j] as f64;
                let x2 = self.table16[j + 1] as f64;
                let y1 = (j as f64 * 65535.0) / (self.n_entries - 1) as f64;
                let y2 = ((j + 1) as f64 * 65535.0) / (self.n_entries - 1) as f64;

                if x1 == x2 {
                    // Collapsed interval: use either endpoint per direction.
                    out.table16[i] = quick_saturate_word(if ascending { y2 } else { y1 });
                    continue;
                } else {
                    a = (y2 - y1) / (x2 - x1);
                    b = y2 - a * x2;
                }
            }

            out.table16[i] = quick_saturate_word(a * y + b);
        }

        Ok(out)
    }

    /// `cmsReverseToneCurve` — reverse with the default 4096 samples.
    pub fn reverse(&self) -> Result<Self> {
        self.reverse_ex(4096)
    }

    /// `GetInterval` — find the node whose `[y0, y1]` brackets `in_val`.
    ///
    /// Handles both ascending and descending tables (and locally increasing/decreasing runs
    /// within them). Returns the lower node index or `-1` if none brackets `in_val`.
    fn get_interval(&self, in_val: f64) -> i32 {
        let p = match &self.interp_params {
            Some(p) => p,
            None => return -1,
        };
        let dom0 = p.domain[0];

        // A 1 point table is not allowed.
        if dom0 < 1 {
            return -1;
        }
        let lut = &self.table16;

        // Bracket helper for one node pair.
        let brackets = |y0: i32, y1: i32| -> bool {
            if y0 <= y1 {
                in_val >= y0 as f64 && in_val <= y1 as f64
            } else {
                in_val >= y1 as f64 && in_val <= y0 as f64
            }
        };

        if lut[0] < lut[dom0 as usize] {
            // Table is overall ascending.
            for i in (0..=(dom0 as i32 - 1)).rev() {
                let iu = i as usize;
                if brackets(lut[iu] as i32, lut[iu + 1] as i32) {
                    return i;
                }
            }
        } else {
            // Table is overall descending.
            for i in 0..dom0 as i32 {
                let iu = i as usize;
                if brackets(lut[iu] as i32, lut[iu + 1] as i32) {
                    return i;
                }
            }
        }

        -1
    }

    /// `cmsJoinToneCurve` — compute `y = Y^-1(X(t))` sampled at `n_resulting_points`.
    pub fn join(x: &ToneCurve, y: &ToneCurve, n_resulting_points: usize) -> Result<Self> {
        let y_reversed = y.reverse_ex(n_resulting_points)?;

        let mut res = vec![0f32; n_resulting_points];
        for (i, r) in res.iter_mut().enumerate() {
            let t = i as f32 / (n_resulting_points - 1) as f32;
            let xv = x.eval_f32(t);
            *r = y_reversed.eval_f32(xv);
        }

        Self::build_tabulated_float(&res)
    }

    // -----------------------------------------------------------------------------------
    // Smoothing
    // -----------------------------------------------------------------------------------

    /// `cmsSmoothToneCurve` — Eilers penalized-least-squares smoothing in place.
    ///
    /// A negative `lambda` skips the monotonicity/degeneracy checks (its magnitude is used).
    /// Returns `true` on success (matching the C `cmsBool`); linear curves are left untouched
    /// and succeed trivially.
    pub fn smooth(&mut self, lambda: f64) -> bool {
        let mut lambda = lambda;
        let mut success = true;

        if self.interp_params.is_none() {
            return false;
        }

        // Only non-linear curves need smoothing.
        if self.is_linear() {
            return true;
        }

        let n_items = self.n_entries;
        if n_items >= MAX_NODES_IN_CURVE {
            // Too many items in the table.
            return false;
        }

        // Allocate one more item than needed (1-based indexing, as in C).
        let mut w = vec![0f32; n_items + 1];
        let mut y = vec![0f32; n_items + 1];
        let mut z = vec![0f32; n_items + 1];

        for i in 0..n_items {
            y[i + 1] = self.table16[i] as f32;
            w[i + 1] = 1.0;
        }

        let mut not_check = false;
        if lambda < 0.0 {
            not_check = true;
            lambda = -lambda;
        }

        if !smooth2(&w, &y, &mut z, lambda as f32, n_items) {
            // Could not smooth.
            return false;
        }

        // Do some reality-checking.
        let mut zeros = 0usize;
        let mut poles = 0usize;
        for i in (2..=n_items).rev() {
            if z[i] == 0.0 {
                zeros += 1;
            }
            if z[i] >= 65535.0 {
                poles += 1;
            }
            if z[i] < z[i - 1] {
                // Non-monotonic.
                success = not_check;
                break;
            }
        }

        if success && zeros > (n_items / 3) {
            // Degenerated, mostly zeros.
            success = not_check;
        }
        if success && poles > (n_items / 3) {
            // Degenerated, mostly poles.
            success = not_check;
        }

        if success {
            for i in 0..n_items {
                self.table16[i] = quick_saturate_word(z[i + 1] as f64);
            }
        }

        success
    }

    // -----------------------------------------------------------------------------------
    // Info
    // -----------------------------------------------------------------------------------

    /// `cmsIsToneCurveLinear` — linear within 12 bits (`diff <= 0x0f` at every node).
    pub fn is_linear(&self) -> bool {
        for i in 0..self.n_entries {
            let q = quantize_val(i as f64, self.n_entries) as i32;
            let diff = (self.table16[i] as i32 - q).abs();
            if diff > 0x0f {
                return false;
            }
        }
        true
    }

    /// `cmsIsToneCurveMonotonic` — monotonic allowing a ripple of up to 2.
    pub fn is_monotonic(&self) -> bool {
        let n = self.n_entries;
        // Degenerated curves are considered monotonic.
        if n < 2 {
            return true;
        }

        if self.is_descending() {
            let mut last = self.table16[0] as i32;
            for i in 1..n {
                if self.table16[i] as i32 - last > 2 {
                    return false;
                } else {
                    last = self.table16[i] as i32;
                }
            }
        } else {
            let mut last = self.table16[n - 1] as i32;
            for i in (0..n - 1).rev() {
                if self.table16[i] as i32 - last > 2 {
                    return false;
                } else {
                    last = self.table16[i] as i32;
                }
            }
        }
        true
    }

    /// `cmsIsToneCurveDescending` — first table entry greater than the last.
    pub fn is_descending(&self) -> bool {
        self.table16[0] > self.table16[self.n_entries - 1]
    }

    /// `cmsIsToneCurveMultisegment` — more than one segment.
    pub fn is_multisegment(&self) -> bool {
        self.n_segments > 1
    }

    /// `cmsGetToneCurveParametricType` — the type of a single-segment curve, else 0.
    pub fn parametric_type(&self) -> i32 {
        if self.n_segments != 1 {
            return 0;
        }
        self.segments[0].seg_type
    }

    /// `cmsEstimateGamma` — least-squares gamma estimate, or `-1.0` if not gamma-like.
    ///
    /// Excludes the endpoints and the lower 7% (`x <= 0.07`) to avoid linear-ramp artifacts,
    /// and rejects the fit if the standard deviation of per-sample gammas exceeds `precision`.
    pub fn estimate_gamma(&self, precision: f64) -> f64 {
        let mut sum = 0.0f64;
        let mut sum2 = 0.0f64;
        let mut n = 0.0f64;

        // Excluding endpoints.
        for i in 1..(MAX_NODES_IN_CURVE - 1) {
            let x = i as f64 / (MAX_NODES_IN_CURVE - 1) as f64;
            let yv = self.eval_f32(x as f32) as f64;

            // Avoid the lower 7% to prevent artifacts due to linear ramps.
            if yv > 0.0 && yv < 1.0 && x > 0.07 {
                let gamma = yv.ln() / x.ln();
                sum += gamma;
                sum2 += gamma * gamma;
                n += 1.0;
            }
        }

        // We need enough valid samples.
        if n <= 1.0 {
            return -1.0;
        }

        // Reject if the SD says it isn't exponential at all.
        let std = ((n * sum2 - sum * sum) / (n * (n - 1.0))).sqrt();
        if std > precision {
            return -1.0;
        }

        sum / n
    }

    /// `cmsGetToneCurveEstimatedTable` — the low-res 16-bit table.
    pub fn estimated_table(&self) -> &[u16] {
        &self.table16
    }

    /// `cmsGetToneCurveEstimatedTableEntries` — number of entries in the low-res table.
    pub fn estimated_table_entries(&self) -> u32 {
        self.n_entries as u32
    }

    /// `cmsGetToneCurveSegment` — segment `n`, or `None` if out of range.
    pub fn segment(&self, n: i32) -> Option<&CurveSegment> {
        if n < 0 || n as usize >= self.n_segments {
            return None;
        }
        Some(&self.segments[n as usize])
    }
}

/// `_cmsQuantizeVal` — the ideal linear value for sample `i` of `max_samples`.
#[inline]
fn quantize_val(i: f64, max_samples: usize) -> u16 {
    let x = (i * 65535.0) / (max_samples - 1) as f64;
    quick_saturate_word(x)
}

/// `smooth2` — Eilers smoothing/interpolation with second differences.
///
/// Solves the penalized tridiagonal system in place. Vectors are **1-based**: `w`, `y`, `z`
/// are indexed `1..=m` (hence length `m + 1`); `z` receives the smoothed result. Returns
/// `false` for `m < 4` or a too-small `lambda`.
fn smooth2(w: &[f32], y: &[f32], z: &mut [f32], lambda: f32, m: usize) -> bool {
    if m < 4 || (lambda as f64) < MATRIX_DET_TOLERANCE {
        return false;
    }

    let mut c = vec![0f32; MAX_NODES_IN_CURVE];
    let mut d = vec![0f32; MAX_NODES_IN_CURVE];
    let mut e = vec![0f32; MAX_NODES_IN_CURVE];

    d[1] = w[1] + lambda;
    c[1] = -2.0 * lambda / d[1];
    e[1] = lambda / d[1];
    z[1] = w[1] * y[1];
    d[2] = w[2] + 5.0 * lambda - d[1] * c[1] * c[1];
    c[2] = (-4.0 * lambda - d[1] * c[1] * e[1]) / d[2];
    e[2] = lambda / d[2];
    z[2] = w[2] * y[2] - c[1] * z[1];

    for i in 3..(m - 1) {
        let i1 = i - 1;
        let i2 = i - 2;
        d[i] = w[i] + 6.0 * lambda - c[i1] * c[i1] * d[i1] - e[i2] * e[i2] * d[i2];
        c[i] = (-4.0 * lambda - d[i1] * c[i1] * e[i1]) / d[i];
        e[i] = lambda / d[i];
        z[i] = w[i] * y[i] - c[i1] * z[i1] - e[i2] * z[i2];
    }

    let i1 = m - 2;
    let i2 = m - 3;
    d[m - 1] = w[m - 1] + 5.0 * lambda - c[i1] * c[i1] * d[i1] - e[i2] * e[i2] * d[i2];
    c[m - 1] = (-2.0 * lambda - d[i1] * c[i1] * e[i1]) / d[m - 1];
    z[m - 1] = w[m - 1] * y[m - 1] - c[i1] * z[i1] - e[i2] * z[i2];

    let i1 = m - 1;
    let i2 = m - 2;
    d[m] = w[m] + lambda - c[i1] * c[i1] * d[i1] - e[i2] * e[i2] * d[i2];
    z[m] = (w[m] * y[m] - c[i1] * z[i1] - e[i2] * z[i2]) / d[m];
    z[m - 1] = z[m - 1] / d[m - 1] - c[m - 1] * z[m];

    for i in (1..=m - 2).rev() {
        z[i] = z[i] / d[i] - c[i] * z[i + 1] - e[i] * z[i + 2];
    }

    true
}

/// Utility (`cmsFreeToneCurveTriple`) — drop a triple of optional curves.
///
/// In Rust the curves free themselves via [`Drop`] (all storage is `Vec`), so this only sets
/// the slots to `None`, mirroring the C post-condition `Curve[0]=Curve[1]=Curve[2]=NULL`.
pub fn free_tone_curve_triple(curves: &mut [Option<ToneCurve>; 3]) {
    curves[0] = None;
    curves[1] = None;
    curves[2] = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamma_2_2_endpoints_and_midpoint() {
        let g = ToneCurve::build_gamma(2.2).unwrap();
        // Segmented curve => accurate float eval.
        assert!((g.eval_f32(0.0) - 0.0).abs() < 1e-6);
        assert!((g.eval_f32(1.0) - 1.0).abs() < 1e-6);
        let expected = 0.5f64.powf(2.2) as f32;
        assert!((g.eval_f32(0.5) - expected).abs() < 1e-6, "got {}", g.eval_f32(0.5));
    }

    #[test]
    fn eval16_endpoints() {
        let g = ToneCurve::build_gamma(2.2).unwrap();
        assert_eq!(g.eval_u16(0), 0);
        assert_eq!(g.eval_u16(0xffff), 0xffff);
    }

    #[test]
    fn parametric_srgb_type4_endpoints() {
        // sRGB parameters: gamma, a, b, c, d.
        let a = 1.0 / 1.055;
        let b = 0.055 / 1.055;
        let params = [2.4, a, b, 1.0 / 12.92, 0.04045];
        let g = ToneCurve::build_parametric(4, &params).unwrap();
        assert!((g.eval_f32(0.0) - 0.0).abs() < 1e-6);
        assert!((g.eval_f32(1.0) - 1.0).abs() < 1e-6, "got {}", g.eval_f32(1.0));
        // A known sRGB point: encoded 0.5 -> ~0.214.
        let mid = g.eval_f32(0.5);
        assert!((mid - 0.214).abs() < 0.01, "got {mid}");
    }

    #[test]
    fn reverse_of_gamma_is_inverse_gamma() {
        let g = ToneCurve::build_gamma(2.2).unwrap();
        let r = g.reverse().unwrap();
        // Analytic inverse: single parametric segment of type -1.
        assert_eq!(r.parametric_type(), -1);
        let x = 0.5f32;
        let expected = x.powf(1.0 / 2.2);
        assert!((r.eval_f32(x) - expected).abs() < 1e-4, "got {}", r.eval_f32(x));
        // Round trip: reverse(forward(x)) ~= x.
        let round = r.eval_f32(g.eval_f32(x));
        assert!((round - x).abs() < 1e-4, "round trip {round}");
    }

    #[test]
    fn linear_curve_detected() {
        let g = ToneCurve::build_gamma(1.0).unwrap();
        assert!(g.is_linear());
        assert!(g.is_monotonic());
        assert!(!g.is_descending());
    }

    #[test]
    fn estimate_gamma_recovers_exponent() {
        let g = ToneCurve::build_gamma(2.2).unwrap();
        let est = g.estimate_gamma(0.01);
        assert!((est - 2.2).abs() < 0.05, "estimated {est}");
    }

    #[test]
    fn tabulated_float_endpoints() {
        // A simple ramp; endpoints must pass through.
        let vals: Vec<f32> = (0..256).map(|i| i as f32 / 255.0).collect();
        let g = ToneCurve::build_tabulated_float(&vals).unwrap();
        assert!((g.eval_f32(0.0) - 0.0).abs() < 1e-4);
        assert!((g.eval_f32(1.0) - 1.0).abs() < 1e-4);
        assert!((g.eval_f32(0.5) - 0.5).abs() < 1e-3, "got {}", g.eval_f32(0.5));
    }

    #[test]
    fn tabulated16_roundtrip() {
        let table: Vec<u16> = (0..256).map(|i| (i * 257) as u16).collect();
        let g = ToneCurve::build_tabulated_16(256, &table).unwrap();
        assert_eq!(g.eval_u16(0), 0);
        assert_eq!(g.eval_u16(0xffff), 0xffff);
        assert_eq!(g.estimated_table_entries(), 256);
    }

    #[test]
    fn parametric_type6_gamma_one_no_clamp() {
        // Type 6, gamma 1.0: Y = e + c where e = a*R + b; verify the no-clamp identity path.
        let params = [1.0, 1.0, 0.0, 0.0];
        let v = default_eval_parametric_fn(6, &{ let mut p = [0.0; 10]; p[..4].copy_from_slice(&params); p }, -0.3);
        assert!((v - (-0.3)).abs() < 1e-12);
    }

    #[test]
    fn unknown_parametric_type_rejected() {
        assert!(ToneCurve::build_parametric(42, &[1.0]).is_err());
    }

    #[test]
    fn dup_is_independent() {
        let g = ToneCurve::build_gamma(2.2).unwrap();
        let d = g.dup();
        assert_eq!(g.eval_u16(0x4000), d.eval_u16(0x4000));
    }
}
