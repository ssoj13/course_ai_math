//! Multi-Process-Element (MPE) stages — a bit-exact port of the stage half of `cmslut.c`.
//!
//! A [`Stage`] is one node of a [`crate::pipeline::Pipeline`] (`cmsStage`). lcms2 models a
//! stage as a tagged blob (`Type`/`Implements` + a `void* Data` and three function pointers
//! for eval/dup/free). This port replaces the type tag + `void*` with the [`StageData`] enum
//! and dispatches evaluation on the variant. Duplication and freeing vanish into `Clone` and
//! `Drop`; every stage evaluates **always in `f32`**, exactly as the C `EvalPtr` contract.
//!
//! # Fidelity (do not "clean up")
//! * The matrix stage accumulates in `f64` then narrows to `f32` per output — the temporary
//!   `cmsFloat64Number Tmp` in `EvaluateMatrix` — so the operation order and rounding match C.
//! * The 16-bit CLUT stage converts `f32 -> u16 -> eval16 -> u16 -> f32`
//!   (`EvaluateCLUTfloatIn16`); the float CLUT stage stays in `f32` (`EvaluateCLUTfloat`).
//! * `Lab2XYZ`/`XYZ2Lab` reproduce the exact `* 100 / * 255 - 128` V4 scaling and divide by
//!   [`MAX_ENCODEABLE_XYZ`].
//! * `LabV2ToV4`/`LabV4ToV2` and the four `Normalize*` stages are, in C, plain matrix stages
//!   (`Type == cmsSigMatrixElemType`) with a distinguishing `Implements`. We keep them as
//!   dedicated data variants but evaluate them through the **same** matrix code path
//!   ([`eval_matrix`]) with the identical constant matrices, and report `matf` from
//!   [`Stage::type_signature`]. See the module report for the M3 optimizer note.

use std::sync::Arc;

use crate::cms_err;
use crate::color::gamma::ToneCurve;
use crate::color::pcs::{cms_lab_to_xyz, cms_xyz_to_lab};
use crate::error::{ErrorCode, Result};
use crate::named::namedcolor::NamedColorList;
use crate::math::interp::{
    CMS_LERP_FLAGS_FLOAT, InterpParams, MAX_INPUT_DIMENSIONS, MAX_STAGE_CHANNELS,
};
use crate::types::color::{CIELab, CIEXYZ};
use crate::types::num::quick_saturate_word;
use crate::types::sig::Signature;
use crate::MAX_CHANNELS;

// ---------------------------------------------------------------------------------------
// Stage signatures (cmsStageSignature, lcms2.h L550..576) — built from their 4CC bytes.
// ---------------------------------------------------------------------------------------

/// `cmsSigIdentityElemType` — `'idn '`.
pub const STAGE_IDENTITY: Signature = Signature::new(b"idn ");
/// `cmsSigCurveSetElemType` — `'cvst'`.
pub const STAGE_CURVE_SET: Signature = Signature::new(b"cvst");
/// `cmsSigMatrixElemType` — `'matf'`.
pub const STAGE_MATRIX: Signature = Signature::new(b"matf");
/// `cmsSigCLutElemType` — `'clut'`.
pub const STAGE_CLUT: Signature = Signature::new(b"clut");
/// `cmsSigLab2XYZElemType` — `'x2l '` (Lab → XYZ).
pub const STAGE_LAB2XYZ: Signature = Signature::new(b"x2l ");
/// `cmsSigXYZ2LabElemType` — `'l2x '` (XYZ → Lab).
pub const STAGE_XYZ2LAB: Signature = Signature::new(b"l2x ");
/// `cmsSigLabV2toV4` — `'2 4 '`.
pub const STAGE_LABV2TOV4: Signature = Signature::new(b"2 4 ");
/// `cmsSigLabV4toV2` — `'4 2 '`.
pub const STAGE_LABV4TOV2: Signature = Signature::new(b"4 2 ");
/// `cmsSigLab2FloatPCS` — `'d2l '`.
pub const STAGE_LAB2FLOATPCS: Signature = Signature::new(b"d2l ");
/// `cmsSigFloatPCS2Lab` — `'l2d '`.
pub const STAGE_FLOATPCS2LAB: Signature = Signature::new(b"l2d ");
/// `cmsSigXYZ2FloatPCS` — `'d2x '`.
pub const STAGE_XYZ2FLOATPCS: Signature = Signature::new(b"d2x ");
/// `cmsSigFloatPCS2XYZ` — `'x2d '`.
pub const STAGE_FLOATPCS2XYZ: Signature = Signature::new(b"x2d ");
/// `cmsSigClipNegativesElemType` — `'clp '`.
pub const STAGE_CLIP_NEGATIVES: Signature = Signature::new(b"clp ");
/// `cmsSigNamedColorElemType` — `'ncl '`.
pub const STAGE_NAMED_COLOR: Signature = Signature::new(b"ncl ");

/// `SAMPLER_INSPECT` (`lcms2.h`): do not write sampler results back into the table.
pub const SAMPLER_INSPECT: u32 = 0x0100_0000;

/// ICC v4 max encodeable XYZ (`MAX_ENCODEABLE_XYZ`): `1 + 32767/32768` (kept private to
/// mirror `lcms2_internal.h`; `pcs.rs` has its own identical copy).
const MAX_ENCODEABLE_XYZ: f64 = 1.0 + 32767.0 / 32768.0;

// ---------------------------------------------------------------------------------------
// Constant matrices for the matrix-based special stages (verbatim from cmslut.c).
// ---------------------------------------------------------------------------------------

/// `V2ToV4` (`_cmsStageAllocLabV2ToV4`): `diag(65535/65280)`.
const V2_TO_V4: [f64; 9] = [
    65535.0 / 65280.0, 0.0, 0.0,
    0.0, 65535.0 / 65280.0, 0.0,
    0.0, 0.0, 65535.0 / 65280.0,
];
/// `V4ToV2` (`_cmsStageAllocLabV4ToV2`): `diag(65280/65535)`.
const V4_TO_V2: [f64; 9] = [
    65280.0 / 65535.0, 0.0, 0.0,
    0.0, 65280.0 / 65535.0, 0.0,
    0.0, 0.0, 65280.0 / 65535.0,
];
/// `a1` of `_cmsStageNormalizeFromLabFloat`: `diag(1/100, 1/255, 1/255)`.
const NORM_FROM_LAB_M: [f64; 9] = [
    1.0 / 100.0, 0.0, 0.0,
    0.0, 1.0 / 255.0, 0.0,
    0.0, 0.0, 1.0 / 255.0,
];
/// `o1` of `_cmsStageNormalizeFromLabFloat`.
const NORM_FROM_LAB_O: [f64; 3] = [0.0, 128.0 / 255.0, 128.0 / 255.0];
/// `a1` of `_cmsStageNormalizeToLabFloat`: `diag(100, 255, 255)`.
const NORM_TO_LAB_M: [f64; 9] = [
    100.0, 0.0, 0.0,
    0.0, 255.0, 0.0,
    0.0, 0.0, 255.0,
];
/// `o1` of `_cmsStageNormalizeToLabFloat`.
const NORM_TO_LAB_O: [f64; 3] = [0.0, -128.0, -128.0];
/// `a1` of `_cmsStageNormalizeFromXyzFloat`: `diag(32768/65535)`.
const NORM_FROM_XYZ: [f64; 9] = [
    32768.0 / 65535.0, 0.0, 0.0,
    0.0, 32768.0 / 65535.0, 0.0,
    0.0, 0.0, 32768.0 / 65535.0,
];
/// `a1` of `_cmsStageNormalizeToXyzFloat`: `diag(65535/32768)`.
const NORM_TO_XYZ: [f64; 9] = [
    65535.0 / 32768.0, 0.0, 0.0,
    0.0, 65535.0 / 32768.0, 0.0,
    0.0, 0.0, 65535.0 / 32768.0,
];

// ---------------------------------------------------------------------------------------
// Custom-stage plugin seam
// ---------------------------------------------------------------------------------------

/// `cmsPipelineCheckAndRetreiveStages` (`cmslut.c:110`) — succeed iff `stages` is exactly
/// `types.len()` long and each stage's *type* signature matches `types` in order, returning
/// references to them.
///
/// C takes a varargs list of `n` type signatures followed by `n` `cmsStage**` out-parameters;
/// a slice in and an `Option<Vec<&Stage>>` out carry the same contract without the unchecked
/// argument count. Reached through [`crate::pipeline::Pipeline::check_and_retrieve_stages`].
pub fn check_and_retrieve<'a>(stages: &'a [Stage], types: &[Signature]) -> Option<Vec<&'a Stage>> {
    if stages.len() != types.len() {
        return None;
    }
    for (s, t) in stages.iter().zip(types.iter()) {
        if s.type_signature() != *t {
            return None;
        }
    }
    Some(stages.iter().collect())
}

/// Evaluation behaviour for a plugin / Multi-Process-Element stage (`_cmsStageEvalFn`
/// parity). Held behind an [`Arc`] so [`Stage`] stays cheaply `Clone` (the C `DupElemPtr`).
pub trait StageEval: std::fmt::Debug {
    /// Evaluate the stage in floating point: `output[..output_channels] = f(input)`.
    fn eval(&self, input: &[f32], output: &mut [f32]);
    /// Number of input channels consumed.
    fn input_channels(&self) -> u32;
    /// Number of output channels produced.
    fn output_channels(&self) -> u32;
}

/// A fast whole-pipeline evaluator installed by an optimizer (`_cmsOPTeval16Fn`).
///
/// Deliberately **16-bit only**, mirroring `cmsopt.c`: every optimizer there supplies an
/// `Eval16Fn` and `_cmsPipelineSetOptimizationParameters` (`cmslut.c:1674`) installs it into
/// `Eval16Fn` alone — `EvalFloatFn` keeps walking the discrete stage chain in full `f32`
/// precision (`_LUTevalFloat`, `cmslut.c:1354`). Keeping this trait separate from
/// [`StageEval`] is what stops the float lane from silently inheriting 16-bit quantization;
/// see the module docs on [`crate::pipeline::Pipeline`].
///
/// Channel counts are deliberately absent: C's `Eval16Fn(In, Out, Data)` carries none either,
/// and the owning [`crate::pipeline::Pipeline`] already knows them.
pub trait Eval16: std::fmt::Debug {
    /// Evaluate the whole pipeline at 16-bit precision: `output = f(input)`.
    fn eval16(&self, input: &[u16], output: &mut [u16]);
}

// ---------------------------------------------------------------------------------------
// Stage data
// ---------------------------------------------------------------------------------------

/// The CLUT sample table (`_cmsStageCLutData.Tab`): either 16-bit or float, never both.
#[derive(Clone, Debug)]
pub enum ClutTable {
    /// 16-bit samples (`Tab.T`), evaluated via `f32 -> u16 -> eval16 -> u16 -> f32`.
    U16(Vec<u16>),
    /// Float samples (`Tab.TFloat`), evaluated directly in `f32`.
    F32(Vec<f32>),
}

/// A CLUT stage payload (`_cmsStageCLutData`): the sample table plus its interpolation
/// geometry (`Params`). `nEntries`/`HasFloatValues` are implicit in [`ClutTable`].
#[derive(Clone, Debug)]
pub struct ClutData {
    /// The multidimensional sample table.
    pub table: ClutTable,
    /// Interpolation geometry over [`Self::table`] (`_cmsComputeInterpParamsEx`).
    pub params: InterpParams,
}

/// The payload of a [`Stage`], one variant per lcms2 MPE type.
///
/// `Dup`/`Free` from the C code become `Clone`/`Drop`. The matrix-based special stages
/// (`LabV2ToV4`, `LabV4ToV2`, `Normalize*`) carry no data — their fixed matrices live in
/// module constants and are evaluated through [`eval_matrix`].
#[derive(Clone, Debug)]
pub enum StageData {
    /// `cmsSigIdentityElemType` — copy input to output (`EvaluateIdentity`).
    Identity,
    /// `cmsSigCurveSetElemType` — one tone curve per channel (`EvaluateCurves`).
    ToneCurves(Vec<ToneCurve>),
    /// `cmsSigMatrixElemType` — `out = M·in + offset`, row-major `M` of `rows × cols`.
    Matrix {
        /// Row-major matrix coefficients, length `rows * cols` (`Double`).
        m: Vec<f64>,
        /// Optional per-output offset, length `rows` (`Offset`).
        offset: Option<Vec<f64>>,
        /// Output channel count.
        rows: usize,
        /// Input channel count.
        cols: usize,
    },
    /// `cmsSigCLutElemType` — multidimensional interpolation (`EvaluateCLUT*`).
    Clut(ClutData),
    /// `cmsSigLab2XYZElemType` — V4 Lab → normalised XYZ (`EvaluateLab2XYZ`).
    Lab2Xyz,
    /// `cmsSigXYZ2LabElemType` — normalised XYZ → V4 Lab (`EvaluateXYZ2Lab`).
    Xyz2Lab,
    /// `_cmsStageAllocLabV2ToV4` — matrix `diag(65535/65280)`.
    LabV2ToV4,
    /// `_cmsStageAllocLabV4ToV2` — matrix `diag(65280/65535)`.
    LabV4ToV2,
    /// `_cmsStageNormalizeFromLabFloat` — Lab → 0..1 PCS.
    NormalizeFromLab,
    /// `_cmsStageNormalizeToLabFloat` — 0..1 PCS → Lab.
    NormalizeToLab,
    /// `_cmsStageNormalizeFromXyzFloat` — XYZ → 0..1 PCS.
    NormalizeFromXyz,
    /// `_cmsStageNormalizeToXyzFloat` — 0..1 PCS → XYZ.
    NormalizeToXyz,
    /// `cmsSigClipNegativesElemType` — clamp each channel to `>= 0` (`Clipper`).
    ClipNegatives,
    /// `cmsSigNamedColorElemType` — spot-colour index → PCS/device lookup
    /// (`EvalNamedColorPCS` / `EvalNamedColor`). `use_pcs` selects the output: the entry's
    /// Lab PCS (3 channels) when `true`, else its device colorant ([`NamedColorList::colorant_count`]
    /// channels). The list is owned (the C stage keeps a `cmsDupNamedColorList` copy).
    NamedColor {
        /// The spot-colour palette (`mpe->Data`, a `cmsNAMEDCOLORLIST`).
        list: NamedColorList,
        /// `UsePCS`: emit the 3-channel Lab PCS instead of the device colorant.
        use_pcs: bool,
    },
    /// Plugin / MPE stage evaluated through a [`StageEval`] object.
    Custom(Arc<dyn StageEval + Send + Sync>),
}

/// One pipeline element (`cmsStage`).
///
/// `input_channels`/`output_channels` mirror `InputChannels`/`OutputChannels`; `implements`
/// is the *function* signature used by the optimizer (`Implements`), which defaults to the
/// stage type but is overridden for the identity-curve / Lab-conversion helpers.
#[derive(Clone, Debug)]
pub struct Stage {
    /// Input channel count (`InputChannels`).
    pub input_channels: u32,
    /// Output channel count (`OutputChannels`).
    pub output_channels: u32,
    /// The "function" signature for optimization (`Implements`).
    pub implements: Signature,
    /// The stage payload + behaviour.
    pub data: StageData,
}

impl Stage {
    // -----------------------------------------------------------------------------------
    // Constructors
    // -----------------------------------------------------------------------------------

    /// `cmsStageAllocIdentity` — an `n`-channel pass-through stage.
    pub fn alloc_identity(n_channels: u32) -> Stage {
        Stage {
            input_channels: n_channels,
            output_channels: n_channels,
            implements: STAGE_IDENTITY,
            data: StageData::Identity,
        }
    }

    /// `cmsStageAllocToneCurves` — one tone curve per channel.
    ///
    /// `curves == None` builds identity gamma-1.0 curves (`cmsBuildGamma(1.0)`), matching the
    /// C behaviour when `Curves` is `NULL`; otherwise each supplied curve is duplicated.
    ///
    /// A slice shorter than `n_channels` is rejected rather than indexed: C dereferences
    /// `Curves[i]` for every `i < nChannels` (`cmslut.c:274-281`) from a caller-owned array and
    /// simply reads past the end, but this is a safe `pub` API and the short-slice case is
    /// reachable (e.g. `create_linearization_device_link(CMYK, &[one_curve])`). Checked here so
    /// every caller is covered rather than at each call site.
    pub fn alloc_tone_curves(n_channels: u32, curves: Option<&[ToneCurve]>) -> Result<Stage> {
        if let Some(cs) = curves {
            if cs.len() < n_channels as usize {
                return Err(cms_err!(
                    ErrorCode::Range,
                    "too few tone curves: {} < {}",
                    cs.len(),
                    n_channels
                ));
            }
        }
        let mut the_curves = Vec::with_capacity(n_channels as usize);
        for i in 0..n_channels as usize {
            let c = match curves {
                None => ToneCurve::build_gamma(1.0)?,
                Some(cs) => cs[i].clone(),
            };
            the_curves.push(c);
        }
        Ok(Stage {
            input_channels: n_channels,
            output_channels: n_channels,
            implements: STAGE_CURVE_SET,
            data: StageData::ToneCurves(the_curves),
        })
    }

    /// `cmsStageAllocMatrix` — `out = M·in (+ offset)`.
    ///
    /// `M` is `rows × cols` row-major (`rows` outputs, `cols` inputs); `offset`, if present,
    /// has length `rows`.
    ///
    /// Sizes are rejected on exactly C's `UINT_MAX` ratio guards (`cmslut.c:386-392`). A plain
    /// 64-bit `checked_mul` is *not* equivalent: `Rows = Cols = 65535` gives `n =
    /// 4_294_836_225`, which C refuses (`n >= UINT_MAX / 65535`) but which would otherwise
    /// proceed here to a ~34 GB `vec!` — and a `Vec` allocation failure aborts the process.
    pub fn alloc_matrix(
        rows: usize,
        cols: usize,
        matrix: &[f64],
        offset: Option<&[f64]>,
    ) -> Result<Stage> {
        // C takes both dimensions as `cmsUInt32Number`, so anything wider is out of range
        // before the ratio guards below can even be expressed.
        if rows > u32::MAX as usize || cols > u32::MAX as usize {
            return Err(cms_err!(ErrorCode::Range, "matrix dimensions out of range ({rows}x{cols})"));
        }
        let n = rows.checked_mul(cols).ok_or_else(|| {
            cms_err!(ErrorCode::Range, "matrix size overflow ({rows}x{cols})")
        })?;
        if n == 0 {
            return Err(cms_err!(ErrorCode::Range, "matrix cannot be empty"));
        }
        // `n != 0` above makes both divisors non-zero, as in C. C's trailing `n < Rows ||
        // n < Cols` wraparound check is subsumed: `checked_mul` on `usize` cannot wrap here.
        const UINT_MAX: usize = u32::MAX as usize;
        if n >= UINT_MAX / cols || n >= UINT_MAX / rows {
            return Err(cms_err!(ErrorCode::Range, "matrix size overflow ({rows}x{cols})"));
        }
        if matrix.len() < n {
            return Err(cms_err!(
                ErrorCode::Range,
                "matrix data too short: {} < {}",
                matrix.len(),
                n
            ));
        }
        if let Some(off) = offset {
            if off.len() < rows {
                return Err(cms_err!(
                    ErrorCode::Range,
                    "offset too short: {} < {}",
                    off.len(),
                    rows
                ));
            }
        }
        Ok(Stage {
            input_channels: cols as u32,
            output_channels: rows as u32,
            implements: STAGE_MATRIX,
            data: StageData::Matrix {
                m: matrix[..n].to_vec(),
                offset: offset.map(|o| o[..rows].to_vec()),
                rows,
                cols,
            },
        })
    }

    /// `cmsStageAllocCLut16bitGranular` — a 16-bit CLUT with per-dimension node counts.
    pub fn alloc_clut_16bit_granular(
        clut_points: &[u32],
        input_chan: u32,
        output_chan: u32,
        table: Option<&[u16]>,
    ) -> Result<Stage> {
        if input_chan as usize > MAX_INPUT_DIMENSIONS {
            return Err(cms_err!(
                ErrorCode::Range,
                "Too many input channels ({input_chan} channels, max={MAX_INPUT_DIMENSIONS})"
            ));
        }
        let n = output_chan as usize * cube_size(clut_points, input_chan as usize) as usize;
        if n == 0 {
            return Err(cms_err!(ErrorCode::Range, "CLUT would have zero entries"));
        }
        let mut tab = vec![0u16; n];
        if let Some(src) = table {
            for (d, &s) in tab.iter_mut().zip(src.iter()).take(n) {
                *d = s;
            }
        }
        let params =
            InterpParams::new_ex(clut_points, input_chan as usize, output_chan as usize, 0)?;
        Ok(Stage {
            input_channels: input_chan,
            output_channels: output_chan,
            implements: STAGE_CLUT,
            data: StageData::Clut(ClutData { table: ClutTable::U16(tab), params }),
        })
    }

    /// `cmsStageAllocCLut16bit` — a 16-bit CLUT with a uniform node count per dimension.
    pub fn alloc_clut_16bit(
        n_grid_points: u32,
        input_chan: u32,
        output_chan: u32,
        table: Option<&[u16]>,
    ) -> Result<Stage> {
        let dims = vec![n_grid_points; input_chan as usize];
        Self::alloc_clut_16bit_granular(&dims, input_chan, output_chan, table)
    }

    /// `cmsStageAllocCLutFloatGranular` — a float CLUT with per-dimension node counts.
    pub fn alloc_clut_float_granular(
        clut_points: &[u32],
        input_chan: u32,
        output_chan: u32,
        table: Option<&[f32]>,
    ) -> Result<Stage> {
        if input_chan as usize > MAX_INPUT_DIMENSIONS {
            return Err(cms_err!(
                ErrorCode::Range,
                "Too many input channels ({input_chan} channels, max={MAX_INPUT_DIMENSIONS})"
            ));
        }
        let n = output_chan as usize * cube_size(clut_points, input_chan as usize) as usize;
        if n == 0 {
            return Err(cms_err!(ErrorCode::Range, "CLUT would have zero entries"));
        }
        let mut tab = vec![0f32; n];
        if let Some(src) = table {
            for (d, &s) in tab.iter_mut().zip(src.iter()).take(n) {
                *d = s;
            }
        }
        let params = InterpParams::new_ex(
            clut_points,
            input_chan as usize,
            output_chan as usize,
            CMS_LERP_FLAGS_FLOAT,
        )?;
        Ok(Stage {
            input_channels: input_chan,
            output_channels: output_chan,
            implements: STAGE_CLUT,
            data: StageData::Clut(ClutData { table: ClutTable::F32(tab), params }),
        })
    }

    /// `cmsStageAllocCLutFloat` — a float CLUT with a uniform node count per dimension.
    pub fn alloc_clut_float(
        n_grid_points: u32,
        input_chan: u32,
        output_chan: u32,
        table: Option<&[f32]>,
    ) -> Result<Stage> {
        let dims = vec![n_grid_points; input_chan as usize];
        Self::alloc_clut_float_granular(&dims, input_chan, output_chan, table)
    }

    /// `_cmsStageAllocIdentityCurves` — identity gamma curves flagged as an identity function.
    pub fn alloc_identity_curves(n_channels: u32) -> Result<Stage> {
        let mut s = Self::alloc_tone_curves(n_channels, None)?;
        s.implements = STAGE_IDENTITY;
        Ok(s)
    }

    /// `_cmsStageAllocIdentityCLut` — a 2-node-per-axis CLUT sampled to the identity.
    pub fn alloc_identity_clut(n_chan: u32) -> Result<Stage> {
        let dims = vec![2u32; n_chan as usize];
        let mut mpe = Self::alloc_clut_16bit_granular(&dims, n_chan, n_chan, None)?;
        let n = n_chan as usize;
        let ok = mpe.sample_clut_16(
            |input, output| {
                output[..n].copy_from_slice(&input[..n]);
                true
            },
            0,
        );
        if !ok {
            return Err(cms_err!(ErrorCode::Internal, "identity CLUT sampling failed"));
        }
        mpe.implements = STAGE_IDENTITY;
        Ok(mpe)
    }

    /// `_cmsStageAllocLab2XYZ` — V4 Lab → normalised XYZ.
    pub fn alloc_lab2xyz() -> Stage {
        Stage {
            input_channels: 3,
            output_channels: 3,
            implements: STAGE_LAB2XYZ,
            data: StageData::Lab2Xyz,
        }
    }

    /// `_cmsStageAllocXYZ2Lab` — normalised XYZ → V4 Lab.
    pub fn alloc_xyz2lab() -> Stage {
        Stage {
            input_channels: 3,
            output_channels: 3,
            implements: STAGE_XYZ2LAB,
            data: StageData::Xyz2Lab,
        }
    }

    /// `_cmsStageAllocLabV2ToV4` — matrix `diag(65535/65280)`, `Implements = LabV2toV4`.
    pub fn alloc_lab_v2_to_v4() -> Stage {
        Stage {
            input_channels: 3,
            output_channels: 3,
            implements: STAGE_LABV2TOV4,
            data: StageData::LabV2ToV4,
        }
    }

    /// `_cmsStageAllocLabV4ToV2` — matrix `diag(65280/65535)`, `Implements = LabV4toV2`.
    pub fn alloc_lab_v4_to_v2() -> Stage {
        Stage {
            input_channels: 3,
            output_channels: 3,
            implements: STAGE_LABV4TOV2,
            data: StageData::LabV4ToV2,
        }
    }

    /// `_cmsStageAllocLabV2ToV4curves` — 258-entry tone curves mapping `*(0xffff/0xff00)`.
    pub fn alloc_lab_v2_to_v4_curves() -> Result<Stage> {
        // 258-entry table: map * (0xffff / 0xff00), i.e. * (257 / 256).
        let mut vals = [0u16; 258];
        for (i, v) in vals.iter_mut().enumerate().take(257) {
            *v = ((i as u32 * 0xffff + 0x80) >> 8) as u16;
        }
        vals[257] = 0xffff;
        let curves = [
            ToneCurve::build_tabulated_16(258, &vals)?,
            ToneCurve::build_tabulated_16(258, &vals)?,
            ToneCurve::build_tabulated_16(258, &vals)?,
        ];
        let mut s = Self::alloc_tone_curves(3, Some(&curves))?;
        s.implements = STAGE_LABV2TOV4;
        Ok(s)
    }

    /// `_cmsStageAllocLabPrelin` — V4 a/b prelinearization S-curves (type-108, param 2.4).
    pub fn alloc_lab_prelin() -> Result<Stage> {
        let curves = [
            ToneCurve::build_gamma(1.0)?,
            ToneCurve::build_parametric(108, &[2.4])?,
            ToneCurve::build_parametric(108, &[2.4])?,
        ];
        Self::alloc_tone_curves(3, Some(&curves))
    }

    /// `_cmsStageNormalizeFromLabFloat` — Lab → 0..1 PCS, `Implements = Lab2FloatPCS`.
    pub fn normalize_from_lab() -> Stage {
        Stage {
            input_channels: 3,
            output_channels: 3,
            implements: STAGE_LAB2FLOATPCS,
            data: StageData::NormalizeFromLab,
        }
    }

    /// `_cmsStageNormalizeToLabFloat` — 0..1 PCS → Lab, `Implements = FloatPCS2Lab`.
    pub fn normalize_to_lab() -> Stage {
        Stage {
            input_channels: 3,
            output_channels: 3,
            implements: STAGE_FLOATPCS2LAB,
            data: StageData::NormalizeToLab,
        }
    }

    /// `_cmsStageNormalizeFromXyzFloat` — XYZ → 0..1 PCS, `Implements = XYZ2FloatPCS`.
    pub fn normalize_from_xyz() -> Stage {
        Stage {
            input_channels: 3,
            output_channels: 3,
            implements: STAGE_XYZ2FLOATPCS,
            data: StageData::NormalizeFromXyz,
        }
    }

    /// `_cmsStageNormalizeToXyzFloat` — 0..1 PCS → XYZ, `Implements = FloatPCS2XYZ`.
    pub fn normalize_to_xyz() -> Stage {
        Stage {
            input_channels: 3,
            output_channels: 3,
            implements: STAGE_FLOATPCS2XYZ,
            data: StageData::NormalizeToXyz,
        }
    }

    /// `_cmsStageClipNegatives` — clamp each of `n_channels` channels to `>= 0`.
    pub fn clip_negatives(n_channels: u32) -> Stage {
        Stage {
            input_channels: n_channels,
            output_channels: n_channels,
            implements: STAGE_CLIP_NEGATIVES,
            data: StageData::ClipNegatives,
        }
    }

    /// Wrap a [`StageEval`] plugin object as a stage (`_cmsStageAllocPlaceholder` seam).
    pub fn alloc_custom(implements: Signature, eval: Arc<dyn StageEval + Send + Sync>) -> Stage {
        Stage {
            input_channels: eval.input_channels(),
            output_channels: eval.output_channels(),
            implements,
            data: StageData::Custom(eval),
        }
    }

    /// `_cmsStageAllocNamedColor` — a named-colour lookup stage over `list`.
    ///
    /// One input channel (the colour index); `use_pcs` (`UsePCS`) picks the output width and
    /// meaning: 3 Lab-PCS channels when `true`, else [`NamedColorList::colorant_count`] device
    /// channels — exactly `UsePCS ? 3 : NamedColorList->ColorantCount` in the C placeholder
    /// call. The list is moved in (the C code stores a `cmsDupNamedColorList` clone). Evaluation
    /// lives in [`eval_named_color`], matching `EvalNamedColorPCS`/`EvalNamedColor`.
    pub fn alloc_named_color(list: NamedColorList, use_pcs: bool) -> Stage {
        let output_channels = if use_pcs { 3 } else { list.colorant_count() as u32 };
        Stage {
            input_channels: 1,
            output_channels,
            implements: STAGE_NAMED_COLOR,
            data: StageData::NamedColor { list, use_pcs },
        }
    }

    /// Borrow the named-colour list of a [`StageData::NamedColor`] stage (`mpe->Data` for a
    /// `cmsSigNamedColorElemType` element), or `None` for any other stage. Used by
    /// `cmsGetNamedColorList` to pull the list back out of a transform's pipeline.
    pub fn named_color_list(&self) -> Option<&NamedColorList> {
        match &self.data {
            StageData::NamedColor { list, .. } => Some(list),
            _ => None,
        }
    }

    // -----------------------------------------------------------------------------------
    // Introspection
    // -----------------------------------------------------------------------------------

    /// The stage *type* signature (`cmsStageType`), derived from the variant.
    ///
    /// The matrix-based specials report `matf` — exactly as their C `Type` — while their
    /// role is distinguished by [`Stage::implements`]. [`StageData::Custom`] reports the
    /// signature it was allocated with (`cmsStageAllocPlaceholder`'s `Type`), so a
    /// plugin-registered MPE element (`_cmsRegisterMultiProcessElementPlugin`) carries its own
    /// on-disk type signature; an unnamed placeholder keeps `Signature(0)`.
    pub fn type_signature(&self) -> Signature {
        match &self.data {
            StageData::Identity => STAGE_IDENTITY,
            StageData::ToneCurves(_) => STAGE_CURVE_SET,
            StageData::Matrix { .. } => STAGE_MATRIX,
            StageData::Clut(_) => STAGE_CLUT,
            StageData::Lab2Xyz => STAGE_LAB2XYZ,
            StageData::Xyz2Lab => STAGE_XYZ2LAB,
            StageData::LabV2ToV4
            | StageData::LabV4ToV2
            | StageData::NormalizeFromLab
            | StageData::NormalizeToLab
            | StageData::NormalizeFromXyz
            | StageData::NormalizeToXyz => STAGE_MATRIX,
            StageData::ClipNegatives => STAGE_CLIP_NEGATIVES,
            StageData::NamedColor { .. } => STAGE_NAMED_COLOR,
            StageData::Custom(_) => self.implements,
        }
    }

    // -----------------------------------------------------------------------------------
    // Evaluation (always in f32, mirroring the C `EvalPtr` contract)
    // -----------------------------------------------------------------------------------

    /// Evaluate the stage: `output[..output_channels] = f(input[..input_channels])`.
    pub fn eval(&self, input: &[f32], output: &mut [f32]) {
        match &self.data {
            StageData::Identity => {
                let n = self.input_channels as usize;
                output[..n].copy_from_slice(&input[..n]);
            }
            StageData::ToneCurves(curves) => {
                for (i, c) in curves.iter().enumerate() {
                    output[i] = c.eval_f32(input[i]);
                }
            }
            StageData::Matrix { m, offset, rows, cols } => {
                eval_matrix(m, offset.as_deref(), *rows, *cols, input, output);
            }
            StageData::Clut(c) => eval_clut(c, input, output),
            StageData::Lab2Xyz => eval_lab2xyz(input, output),
            StageData::Xyz2Lab => eval_xyz2lab(input, output),
            // The matrix-based specials (`cmslut.c:1026-1132`) bind their constants directly
            // rather than re-dispatching through a fallible lookup, so this evaluation hot path
            // carries no `expect` that a future variant could turn into a live panic.
            StageData::LabV2ToV4 => eval_matrix(&V2_TO_V4, None, 3, 3, input, output),
            StageData::LabV4ToV2 => eval_matrix(&V4_TO_V2, None, 3, 3, input, output),
            StageData::NormalizeFromLab => {
                eval_matrix(&NORM_FROM_LAB_M, Some(&NORM_FROM_LAB_O), 3, 3, input, output)
            }
            StageData::NormalizeToLab => {
                eval_matrix(&NORM_TO_LAB_M, Some(&NORM_TO_LAB_O), 3, 3, input, output)
            }
            StageData::NormalizeFromXyz => eval_matrix(&NORM_FROM_XYZ, None, 3, 3, input, output),
            StageData::NormalizeToXyz => eval_matrix(&NORM_TO_XYZ, None, 3, 3, input, output),
            StageData::ClipNegatives => {
                for i in 0..self.input_channels as usize {
                    let n = input[i];
                    output[i] = if n < 0.0 { 0.0 } else { n };
                }
            }
            StageData::NamedColor { list, use_pcs } => {
                eval_named_color(list, *use_pcs, input, output)
            }
            StageData::Custom(e) => e.eval(input, output),
        }
    }

    // -----------------------------------------------------------------------------------
    // Sampling (cmsStageSampleCLut16bit / cmsStageSampleCLutFloat)
    // -----------------------------------------------------------------------------------

    /// `cmsStageSampleCLut16bit` — sweep the whole input space of a 16-bit CLUT stage.
    ///
    /// For each knot the current output is read from the table, passed to `sampler` (which
    /// may overwrite it), and — unless `SAMPLER_INSPECT` is set — written back. Returns
    /// `false` if this is not a 16-bit CLUT stage or `sampler` aborts (returns `false`).
    pub fn sample_clut_16<F>(&mut self, mut sampler: F, dw_flags: u32) -> bool
    where
        F: FnMut(&[u16], &mut [u16]) -> bool,
    {
        let clut = match &mut self.data {
            StageData::Clut(c) => c,
            _ => return false,
        };
        let n_samples = clut.params.n_samples;
        let n_inputs = clut.params.n_inputs;
        let n_outputs = clut.params.n_outputs;
        if n_inputs == 0 || n_outputs == 0 {
            return false;
        }
        if n_inputs > MAX_INPUT_DIMENSIONS || n_outputs >= MAX_STAGE_CHANNELS {
            return false;
        }
        let table = match &mut clut.table {
            ClutTable::U16(t) => t,
            ClutTable::F32(_) => return false,
        };
        let n_total = cube_size(&n_samples, n_inputs);
        if n_total == 0 {
            return false;
        }

        let mut in_ = [0u16; MAX_INPUT_DIMENSIONS + 1];
        let mut out = [0u16; MAX_STAGE_CHANNELS];
        let mut index = 0usize;
        for i in 0..n_total as usize {
            let mut rest = i;
            for t in (0..n_inputs).rev() {
                let colorant = rest % n_samples[t] as usize;
                rest /= n_samples[t] as usize;
                in_[t] = quantize_val(colorant as f64, n_samples[t]);
            }
            out[..n_outputs].copy_from_slice(&table[index..index + n_outputs]);

            if !sampler(&in_[..n_inputs], &mut out[..n_outputs]) {
                return false;
            }
            if dw_flags & SAMPLER_INSPECT == 0 {
                table[index..index + n_outputs].copy_from_slice(&out[..n_outputs]);
            }
            index += n_outputs;
        }
        true
    }

    /// `cmsStageSampleCLutFloat` — as [`Self::sample_clut_16`] for a float CLUT stage.
    pub fn sample_clut_float<F>(&mut self, mut sampler: F, dw_flags: u32) -> bool
    where
        F: FnMut(&[f32], &mut [f32]) -> bool,
    {
        let clut = match &mut self.data {
            StageData::Clut(c) => c,
            _ => return false,
        };
        let n_samples = clut.params.n_samples;
        let n_inputs = clut.params.n_inputs;
        let n_outputs = clut.params.n_outputs;
        if n_inputs == 0 || n_outputs == 0 {
            return false;
        }
        if n_inputs > MAX_INPUT_DIMENSIONS || n_outputs >= MAX_STAGE_CHANNELS {
            return false;
        }
        let table = match &mut clut.table {
            ClutTable::F32(t) => t,
            ClutTable::U16(_) => return false,
        };
        let n_total = cube_size(&n_samples, n_inputs);
        if n_total == 0 {
            return false;
        }

        let mut in_ = [0f32; MAX_INPUT_DIMENSIONS + 1];
        let mut out = [0f32; MAX_STAGE_CHANNELS];
        let mut index = 0usize;
        for i in 0..n_total as usize {
            let mut rest = i;
            for t in (0..n_inputs).rev() {
                let colorant = rest % n_samples[t] as usize;
                rest /= n_samples[t] as usize;
                in_[t] = (quantize_val(colorant as f64, n_samples[t]) as f64 / 65535.0) as f32;
            }
            out[..n_outputs].copy_from_slice(&table[index..index + n_outputs]);

            if !sampler(&in_[..n_inputs], &mut out[..n_outputs]) {
                return false;
            }
            if dw_flags & SAMPLER_INSPECT == 0 {
                table[index..index + n_outputs].copy_from_slice(&out[..n_outputs]);
            }
            index += n_outputs;
        }
        true
    }
}

// ---------------------------------------------------------------------------------------
// Free-standing space sweepers (cmsSliceSpace16 / cmsSliceSpaceFloat)
// ---------------------------------------------------------------------------------------

/// `cmsSliceSpace16` — sweep an input space and call `sampler` with each 16-bit knot.
///
/// Unlike [`Stage::sample_clut_16`] there is no table: only the input vector is produced.
pub fn slice_space_16<F>(clut_points: &[u32], mut sampler: F) -> bool
where
    F: FnMut(&[u16]) -> bool,
{
    let n_inputs = clut_points.len();
    if n_inputs >= MAX_CHANNELS {
        return false;
    }
    let n_total = cube_size(clut_points, n_inputs);
    if n_total == 0 {
        return false;
    }
    let mut in_ = [0u16; MAX_CHANNELS];
    for i in 0..n_total as usize {
        let mut rest = i;
        for t in (0..n_inputs).rev() {
            let colorant = rest % clut_points[t] as usize;
            rest /= clut_points[t] as usize;
            in_[t] = quantize_val(colorant as f64, clut_points[t]);
        }
        if !sampler(&in_[..n_inputs]) {
            return false;
        }
    }
    true
}

/// `cmsSliceSpaceFloat` — as [`slice_space_16`] but producing float knots.
pub fn slice_space_float<F>(clut_points: &[u32], mut sampler: F) -> bool
where
    F: FnMut(&[f32]) -> bool,
{
    let n_inputs = clut_points.len();
    if n_inputs >= MAX_CHANNELS {
        return false;
    }
    let n_total = cube_size(clut_points, n_inputs);
    if n_total == 0 {
        return false;
    }
    let mut in_ = [0f32; MAX_CHANNELS];
    for i in 0..n_total as usize {
        let mut rest = i;
        for t in (0..n_inputs).rev() {
            let colorant = rest % clut_points[t] as usize;
            rest /= clut_points[t] as usize;
            in_[t] = (quantize_val(colorant as f64, clut_points[t]) as f64 / 65535.0) as f32;
        }
        if !sampler(&in_[..n_inputs]) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------

/// `EvaluateMatrix` — `out[i] = (f64)(Σ_j in[j]·M[i·cols+j] + offset[i])`.
///
/// The `f64` accumulator + per-output narrowing to `f32` is load-bearing for bit-exactness.
fn eval_matrix(
    m: &[f64],
    offset: Option<&[f64]>,
    rows: usize,
    cols: usize,
    input: &[f32],
    output: &mut [f32],
) {
    for i in 0..rows {
        let mut tmp = 0.0f64;
        for j in 0..cols {
            tmp += input[j] as f64 * m[i * cols + j];
        }
        if let Some(off) = offset {
            tmp += off[i];
        }
        output[i] = tmp as f32;
    }
}

/// `EvaluateCLUT*` — dispatch on table type. U16 goes `f32 -> u16 -> eval16 -> u16 -> f32`.
fn eval_clut(c: &ClutData, input: &[f32], output: &mut [f32]) {
    let ni = c.params.n_inputs;
    let no = c.params.n_outputs;
    match &c.table {
        ClutTable::F32(t) => c.params.eval_float(t, input, output),
        ClutTable::U16(t) => {
            let mut in16 = [0u16; MAX_STAGE_CHANNELS];
            let mut out16 = [0u16; MAX_STAGE_CHANNELS];
            from_float_to_16(&input[..ni], &mut in16[..ni]);
            c.params.eval16(t, &in16[..ni], &mut out16[..no]);
            from_16_to_float(&out16[..no], &mut output[..no]);
        }
    }
}

/// `EvaluateLab2XYZ` — V4 Lab (in 0..1) → normalised XYZ.
fn eval_lab2xyz(input: &[f32], output: &mut [f32]) {
    let lab = CIELab {
        l: input[0] as f64 * 100.0,
        a: input[1] as f64 * 255.0 - 128.0,
        b: input[2] as f64 * 255.0 - 128.0,
    };
    let xyz = cms_lab_to_xyz(None, &lab);
    output[0] = (xyz.x / MAX_ENCODEABLE_XYZ) as f32;
    output[1] = (xyz.y / MAX_ENCODEABLE_XYZ) as f32;
    output[2] = (xyz.z / MAX_ENCODEABLE_XYZ) as f32;
}

/// `EvaluateXYZ2Lab` — normalised XYZ → V4 Lab (in 0..1).
fn eval_xyz2lab(input: &[f32], output: &mut [f32]) {
    let xyz = CIEXYZ {
        x: input[0] as f64 * MAX_ENCODEABLE_XYZ,
        y: input[1] as f64 * MAX_ENCODEABLE_XYZ,
        z: input[2] as f64 * MAX_ENCODEABLE_XYZ,
    };
    let lab = cms_xyz_to_lab(None, &xyz);
    output[0] = (lab.l / 100.0) as f32;
    output[1] = ((lab.a + 128.0) / 255.0) as f32;
    output[2] = ((lab.b + 128.0) / 255.0) as f32;
}

/// `EvalNamedColorPCS` / `EvalNamedColor` — spot-colour index lookup.
///
/// The index is `_cmsQuickSaturateWord(In[0] * 65535.0)` (so `In[0]` is a 0..1-scaled colour
/// index). When it is out of range the C code signals `cmsERROR_RANGE` and zeroes the outputs;
/// we drop the (context-less) error signal but keep the zero-fill. In range, each stored value
/// is a raw u16 scaled by `/ 65535.0` — the PCS is Lab already *encoded* as u16, so this is a
/// plain normalisation, **not** a Lab→float decode (`PCS[i] / 65535.0` verbatim).
fn eval_named_color(list: &NamedColorList, use_pcs: bool, input: &[f32], output: &mut [f32]) {
    let index = quick_saturate_word(input[0] as f64 * 65535.0) as usize;
    if use_pcs {
        match list.get(index) {
            Some(c) => {
                output[0] = (c.pcs[0] as f64 / 65535.0) as f32;
                output[1] = (c.pcs[1] as f64 / 65535.0) as f32;
                output[2] = (c.pcs[2] as f64 / 65535.0) as f32;
            }
            None => output[..3].fill(0.0),
        }
    } else {
        let cc = list.colorant_count();
        match list.get(index) {
            Some(c) => {
                for (j, o) in output[..cc].iter_mut().enumerate() {
                    *o = (c.device_colorant[j] as f64 / 65535.0) as f32;
                }
            }
            None => output[..cc].fill(0.0),
        }
    }
}

/// `FromFloatTo16` — `out[i] = _cmsQuickSaturateWord(in[i] * 65535.0)` (f64 product).
pub(crate) fn from_float_to_16(input: &[f32], output: &mut [u16]) {
    for (o, &v) in output.iter_mut().zip(input.iter()) {
        *o = quick_saturate_word(v as f64 * 65535.0);
    }
}

/// `From16ToFloat` — `out[i] = (f32)in[i] / 65535.0f` (f32 division, as in C).
pub(crate) fn from_16_to_float(input: &[u16], output: &mut [f32]) {
    for (o, &v) in output.iter_mut().zip(input.iter()) {
        *o = v as f32 / 65535.0f32;
    }
}

/// `_cmsQuantizeVal` (`cmslut.c:737`) — the 0..0xffff code for sample `i` of `max_samples`.
///
/// Public because C exports it (`lcms2.def`): out-of-tree code that builds CLUT sample grids by
/// hand needs it to place nodes bit-identically to lcms.
pub fn quantize_val(i: f64, max_samples: u32) -> u16 {
    let x = (i * 65535.0) / (max_samples as f64 - 1.0);
    quick_saturate_word(x)
}

/// `CubeSize` — total node count of a `b`-dimensional hypercube, or `0` on
/// overflow / a degenerate (`dim <= 1`) dimension. `u64` accumulator + `UINT_MAX` guards
/// transcribed from C.
fn cube_size(dims: &[u32], b: usize) -> u32 {
    let mut rv: u64 = 1;
    let mut b = b;
    while b > 0 {
        let dim = dims[b - 1] as u64;
        if dim <= 1 {
            return 0;
        }
        if rv > u32::MAX as u64 / dim {
            return 0;
        }
        rv *= dim;
        b -= 1;
    }
    if rv > u32::MAX as u64 / 15 {
        return 0;
    }
    rv as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_copies_input() {
        let s = Stage::alloc_identity(3);
        assert_eq!(s.type_signature(), STAGE_IDENTITY);
        let mut out = [0f32; 3];
        s.eval(&[0.1, 0.2, 0.3], &mut out);
        assert_eq!(out, [0.1, 0.2, 0.3]);
    }

    #[test]
    fn matrix_computes_m_times_in_plus_offset() {
        // out = diag(2,3,4)·in + (0.1, 0.2, 0.3)
        let m = [2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0];
        let off = [0.1, 0.2, 0.3];
        let s = Stage::alloc_matrix(3, 3, &m, Some(&off)).unwrap();
        assert_eq!(s.type_signature(), STAGE_MATRIX);
        let mut out = [0f32; 3];
        s.eval(&[0.5, 0.5, 0.5], &mut out);
        assert!((out[0] - 1.1).abs() < 1e-6, "{out:?}");
        assert!((out[1] - 1.7).abs() < 1e-6, "{out:?}");
        assert!((out[2] - 2.3).abs() < 1e-6, "{out:?}");
    }

    #[test]
    fn matrix_rejects_bad_sizes() {
        assert!(Stage::alloc_matrix(0, 3, &[], None).is_err());
        assert!(Stage::alloc_matrix(2, 2, &[1.0, 2.0], None).is_err()); // too short
    }

    #[test]
    fn tone_curves_apply_per_channel() {
        let g = ToneCurve::build_gamma(2.2).unwrap();
        let s = Stage::alloc_tone_curves(3, Some(&[g.clone(), g.clone(), g])).unwrap();
        let mut out = [0f32; 3];
        s.eval(&[0.5, 0.0, 1.0], &mut out);
        let expected = 0.5f32.powf(2.2);
        assert!((out[0] - expected).abs() < 1e-5, "{out:?}");
        assert!((out[1] - 0.0).abs() < 1e-6);
        assert!((out[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn identity_curves_flagged_as_identity() {
        let s = Stage::alloc_identity_curves(4).unwrap();
        assert_eq!(s.implements, STAGE_IDENTITY);
        assert_eq!(s.type_signature(), STAGE_CURVE_SET);
    }

    #[test]
    fn identity_clut_round_trips_corners() {
        let s = Stage::alloc_identity_clut(3).unwrap();
        assert_eq!(s.implements, STAGE_IDENTITY);
        for inp in [[0f32, 0.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]] {
            let mut out = [0f32; 3];
            s.eval(&inp, &mut out);
            for k in 0..3 {
                assert!((out[k] - inp[k]).abs() < 1e-4, "in {inp:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn float_clut_identity_midpoint() {
        // 2^3 float identity CLUT, gray midpoint stays gray.
        let mut t = Vec::new();
        for r in 0..2 {
            for g in 0..2 {
                for b in 0..2 {
                    t.push(r as f32);
                    t.push(g as f32);
                    t.push(b as f32);
                }
            }
        }
        let s = Stage::alloc_clut_float(2, 3, 3, Some(&t)).unwrap();
        let mut out = [0f32; 3];
        s.eval(&[0.5, 0.5, 0.5], &mut out);
        for c in out {
            assert!((c - 0.5).abs() < 1e-6, "{out:?}");
        }
    }

    #[test]
    fn lab2xyz_then_xyz2lab_is_identity() {
        // A neutral-ish V4 Lab in 0..1 encoding: L*=50 -> 0.5, a*=b*=0 -> 0.50196...
        let lab_norm = [0.5f32, (128.0 / 255.0) as f32, (128.0 / 255.0) as f32];
        let to_xyz = Stage::alloc_lab2xyz();
        let to_lab = Stage::alloc_xyz2lab();
        let mut xyz = [0f32; 3];
        to_xyz.eval(&lab_norm, &mut xyz);
        let mut back = [0f32; 3];
        to_lab.eval(&xyz, &mut back);
        for k in 0..3 {
            assert!((back[k] - lab_norm[k]).abs() < 1e-4, "{back:?} vs {lab_norm:?}");
        }
    }

    #[test]
    fn clip_negatives_clamps() {
        let s = Stage::clip_negatives(3);
        let mut out = [0f32; 3];
        s.eval(&[-0.5, 0.25, -0.0], &mut out);
        assert_eq!(out, [0.0, 0.25, 0.0]);
    }

    #[test]
    fn normalize_from_lab_matches_manual_matrix() {
        let s = Stage::normalize_from_lab();
        assert_eq!(s.implements, STAGE_LAB2FLOATPCS);
        assert_eq!(s.type_signature(), STAGE_MATRIX);
        let mut out = [0f32; 3];
        s.eval(&[50.0, 0.0, 0.0], &mut out);
        // L*/100, a*/255 + 128/255, b*/255 + 128/255
        assert!((out[0] - 0.5).abs() < 1e-6, "{out:?}");
        assert!((out[1] - (128.0f32 / 255.0)).abs() < 1e-6, "{out:?}");
        assert!((out[2] - (128.0f32 / 255.0)).abs() < 1e-6, "{out:?}");
    }

    #[test]
    fn lab_v2_v4_round_trip() {
        let v2to4 = Stage::alloc_lab_v2_to_v4();
        let v4to2 = Stage::alloc_lab_v4_to_v2();
        assert_eq!(v2to4.implements, STAGE_LABV2TOV4);
        assert_eq!(v4to2.implements, STAGE_LABV4TOV2);
        let mut mid = [0f32; 3];
        v2to4.eval(&[0.4, 0.5, 0.6], &mut mid);
        let mut back = [0f32; 3];
        v4to2.eval(&mid, &mut back);
        for k in 0..3 {
            assert!((back[k] - [0.4, 0.5, 0.6][k]).abs() < 1e-5, "{back:?}");
        }
    }

    #[test]
    fn slice_space_16_visits_all_knots() {
        let mut count = 0usize;
        let ok = slice_space_16(&[2, 2], |_| {
            count += 1;
            true
        });
        assert!(ok);
        assert_eq!(count, 4);
    }

    /// Build a small 3-colour list with known PCS + 4-channel device values.
    fn sample_named_list() -> NamedColorList {
        let mut list = NamedColorList::new(4, "pre_", "_suf").unwrap();
        let mut c0 = [0u16; MAX_CHANNELS];
        c0[..4].copy_from_slice(&[0, 21845, 43690, 65535]);
        list.append("c0", [100, 30000, 65535], c0);
        let mut c1 = [0u16; MAX_CHANNELS];
        c1[..4].copy_from_slice(&[1000, 2000, 3000, 4000]);
        list.append("c1", [12345, 6789, 42], c1);
        let mut c2 = [0u16; MAX_CHANNELS];
        c2[..4].copy_from_slice(&[65535, 0, 65535, 0]);
        list.append("c2", [65535, 32768, 0], c2);
        list
    }

    #[test]
    fn named_color_pcs_stage() {
        let list = sample_named_list();
        let s = Stage::alloc_named_color(list.clone(), true);
        assert_eq!(s.type_signature(), STAGE_NAMED_COLOR);
        assert_eq!(s.input_channels, 1);
        assert_eq!(s.output_channels, 3); // UsePCS -> 3 Lab channels
        assert!(s.named_color_list().is_some());

        for idx in 0..list.count() {
            // In[0] = index / 65535 recovers `index` through _cmsQuickSaturateWord.
            let in0 = idx as f32 / 65535.0;
            let mut out = [0f32; 3];
            s.eval(&[in0], &mut out);
            let pcs = list.get(idx).unwrap().pcs;
            for k in 0..3 {
                let want = (pcs[k] as f64 / 65535.0) as f32;
                assert!((out[k] - want).abs() < 1e-9, "idx {idx} ch {k}: {out:?}");
            }
        }
    }

    #[test]
    fn named_color_device_stage() {
        let list = sample_named_list();
        let s = Stage::alloc_named_color(list.clone(), false);
        assert_eq!(s.type_signature(), STAGE_NAMED_COLOR);
        assert_eq!(s.output_channels, 4); // colorant_count device channels

        for idx in 0..list.count() {
            let in0 = idx as f32 / 65535.0;
            let mut out = [0f32; MAX_CHANNELS];
            s.eval(&[in0], &mut out);
            let dc = list.get(idx).unwrap().device_colorant;
            for k in 0..4 {
                let want = (dc[k] as f64 / 65535.0) as f32;
                assert!((out[k] - want).abs() < 1e-9, "idx {idx} ch {k}: {out:?}");
            }
        }
    }

    #[test]
    fn named_color_out_of_range_zeroes() {
        let list = sample_named_list();
        // An index past the end saturates to a valid word but fails the range check -> zeros.
        let pcs = Stage::alloc_named_color(list.clone(), true);
        let mut out = [1f32; 3];
        pcs.eval(&[1.0], &mut out); // index = 65535 >= 3
        assert_eq!(out, [0.0, 0.0, 0.0]);

        let dev = Stage::alloc_named_color(list, false);
        let mut out = [1f32; MAX_CHANNELS];
        dev.eval(&[1.0], &mut out);
        assert_eq!(&out[..4], &[0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn cube_size_matches_c() {
        assert_eq!(cube_size(&[2, 2, 2], 3), 8);
        assert_eq!(cube_size(&[17, 17, 17], 3), 17 * 17 * 17);
        assert_eq!(cube_size(&[1, 2], 2), 0); // degenerate dim
    }

    /// C indexes `Curves[i]` for every `i < nChannels` (`cmslut.c:274-281`). This is a safe
    /// `pub` API returning `Result`, so a short slice must be an error, not an index panic —
    /// `create_linearization_device_link(CMYK, &[one_curve])` reaches exactly this case.
    #[test]
    fn alloc_tone_curves_rejects_short_slice() {
        let one = [ToneCurve::build_gamma(2.2).unwrap()];
        assert!(Stage::alloc_tone_curves(4, Some(&one)).is_err());
        assert!(Stage::alloc_tone_curves(1, Some(&one)).is_ok());
        // `None` still means "build n identity curves", independent of any slice length.
        assert!(Stage::alloc_tone_curves(4, None).is_ok());
    }

    /// C's ratio guards (`cmslut.c:386-392`) reject sizes that a 64-bit `checked_mul` would
    /// wave through: `65535 * 65535` fits in `usize` but is `>= UINT_MAX / 65535`, and would
    /// otherwise reach a ~34 GB allocation.
    #[test]
    fn alloc_matrix_matches_c_overflow_guards() {
        // The size must be rejected *as an overflow*, before the data-length check: with a
        // short slice every oversize call errors either way, so assert on which guard fired.
        // Otherwise this test passes even with the ratio guards deleted.
        let err = Stage::alloc_matrix(65535, 65535, &[], None).unwrap_err();
        assert!(
            err.message.contains("overflow"),
            "65535x65535 must trip the C ratio guard, got: {}",
            err.message
        );
        assert!(Stage::alloc_matrix(0, 3, &[], None).is_err());
        let m = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        assert!(Stage::alloc_matrix(3, 3, &m, None).is_ok());
    }
}
