//! `ProcessorOp` — internal operation type for the OCIO processor pipeline.
//!
//! Extracted from `processor.rs` during the STRUCT.2 split (TODO6.md).
//! The `ProcessorOp` enum + its `impl` (is_identity, inverse) are the largest single coherent unit in the file at ~735 LOC,
//! covering every possible compiled op variant: Matrix / Lut1D / Lut3D /
//! Cdl / Exponent / Log / LogAffine / LogCamera / ExponentWithLinear /
//! Range / Transfer / ExposureContrast / FixedFunction / Allocation /
//! GradingPrimary / GradingTone / GradingRgbCurve / GradingHueCurve / NoOp.
//!
//! Re-exported at the parent module level via `pub use op::*;` so external
//! consumers (`crate::processor::ProcessorOp`) keep the same path.

use super::TransferStyle;
use crate::Interpolation;
use crate::transform::{
    AllocationType, CdlStyle, EcDynamic, ExposureContrastStyle, FixedFunctionStyle,
    GradingPrimaryStyle, NegativeStyle,
};

/// OCIO's guard against division by a vanishing CDL parameter.
///
/// `CDLOpCPU.cpp:20-23` — `1 / max(x, 1e-2)`. Without the floor a slope, power or
/// saturation of 0 (all legal in a `.cdl` file) yields an infinite reverse
/// parameter and NaN pixels. The three hand-rolled CDL inversions this module
/// replaced each used a bare `1.0 / x`.
const CDL_RCP_MIN: f32 = 1e-2;

/// OCIO `Reciprocal` (`CDLOpCPU.cpp:20`).
fn cdl_rcp(x: f32) -> f32 {
    1.0 / x.max(CDL_RCP_MIN)
}

/// Inverts a Range op's `(scale, offset, clamp_min, clamp_max)` — the ONE
/// place this derivation lives, used by both `ProcessorOp::inverse()` below
/// and `processor/compile.rs`'s `Transform::Range` arm (which builds the
/// forward-oriented params from the transform's own bounds, then calls this
/// when the direction is `Inverse`, rather than independently swapping
/// `(min_in,max_in)`/`(min_out,max_out)` and rebuilding from scratch — the
/// second derivation that let this drift out of sync in the first place).
///
/// `clamp_min`/`clamp_max` are OUTPUT bounds of the forward affine map
/// `out = clamp(in*scale + offset, clamp_min, clamp_max)` — the inverse's
/// own output bounds are the ORIGINAL mapping's INPUT bounds, recovered by
/// running each old clamp bound through the newly-inverted scale/offset
/// (`x = (y-offset)/scale` evaluated at a forward-clamp boundary gives back
/// the exact input bound that produced it — the affine part is continuous,
/// so a clamp boundary is reached, not merely approached).
pub(crate) fn invert_range_params(
    scale: f32,
    offset: f32,
    clamp_min: Option<f32>,
    clamp_max: Option<f32>,
) -> (f32, f32, Option<f32>, Option<f32>) {
    let inv_scale = 1.0 / scale;
    let inv_offset = -offset / scale;
    let remap = |c: f32| c * inv_scale + inv_offset;
    (
        inv_scale,
        inv_offset,
        clamp_min.map(remap),
        clamp_max.map(remap),
    )
}

/// Inverse of a row-major 4x4 matrix + offset (`y = M x + o`  ->  `x = M^-1 y - M^-1 o`), in
/// `f64` — the ONE matrix inversion of the processor, used by `compile.rs`'s Matrix arm, by
/// `ProcessorOp::inverse()` and by the builtin matrix builders (`builtin_transforms::color_matrix`).
///
/// OCIO inverts in double (`MatrixArray::inverse`, `ops/matrix/MatrixOpData.cpp:167-281`; the
/// offset as `-inv * offset` in `MatrixOpData::getAsForward`, :810-830) and refuses only a matrix
/// with an exactly-zero pivot. The two copies this replaced inverted in
/// `f32` and refused any determinant below an absolute `1e-10`, which rejects perfectly regular
/// scale matrices: `diag(1/4000)` (the ACES 1.x 4000-nit tone scale) has determinant 1.6e-11,
/// so its builtin could not be inverted.
pub(crate) fn invert_matrix_offset(
    m: &[f64; 16],
    offset: &[f64; 4],
) -> Option<([f64; 16], [f64; 4])> {
    let g = glam::DMat4::from_cols_array(m).transpose(); // row-major -> glam's column-major
    let det = g.determinant();
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let inv = g.inverse().transpose().to_cols_array();
    if inv.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let inv_offset: [f64; 4] =
        std::array::from_fn(|r| -(0..4).map(|c| inv[r * 4 + c] * offset[c]).sum::<f64>());
    Some((inv, inv_offset))
}

/// Parameters of OCIO's `ACES_GAMUT_COMP_13` FixedFunction, decoded once for the CPU apply and
/// the shared GPU emitter (`gpu/gamut_comp.rs`).
///
/// Param order (`FixedFunctionOpCPU.cpp:984-1002`): `[lim cyan, lim magenta, lim yellow,
/// thr cyan, thr magenta, thr yellow, power]`; the per-channel scale is the "y = 1 intersect"
/// the reference precomputes from each limit/threshold pair.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GamutComp13Params {
    pub thr: [f32; 3],
    pub scale: [f32; 3],
    pub power: f32,
}

impl GamutComp13Params {
    pub(crate) fn from_params(params: &[f32]) -> Self {
        // OCIO's defaults (the ACES 1.3 reference values), for a hand-built op without params.
        const DEFAULTS: [f32; 7] = [1.147, 1.264, 1.312, 0.815, 0.803, 0.880, 1.2];
        let p = |i: usize| params.get(i).copied().unwrap_or(DEFAULTS[i]);
        let power = p(6);
        let scale = |lim: f32, thr: f32| {
            (lim - thr) / (((1.0 - thr) / (lim - thr)).powf(-power) - 1.0).powf(1.0 / power)
        };
        let thr = [p(3), p(4), p(5)];
        Self {
            thr,
            scale: [
                scale(p(0), thr[0]),
                scale(p(1), thr[1]),
                scale(p(2), thr[2]),
            ],
            power,
        }
    }

    /// `gamut_comp` (`FixedFunctionOpCPU.cpp:924-982`) for channel `c`.
    pub(crate) fn apply(&self, val: f32, ach: f32, c: usize, forward: bool) -> f32 {
        if ach == 0.0 {
            return 0.0;
        }
        let (thr, scale, power) = (self.thr[c], self.scale[c], self.power);
        // Distance from the achromatic axis, aka inverse RGB ratios.
        let dist = (ach - val) / ach.abs();
        if dist < thr {
            return val;
        }
        let compr_dist = if forward {
            let nd = (dist - thr) / scale;
            thr + scale * nd / (1.0 + nd.powf(power)).powf(1.0 / power)
        } else if dist >= thr + scale {
            dist // avoid the singularity
        } else {
            let nd = (dist - thr) / scale;
            let p = nd.powf(power);
            thr + scale * (-(p / (p - 1.0))).powf(1.0 / power)
        };
        ach - compr_dist * ach.abs()
    }
}

/// Parameters of OCIO's `LIN_TO_GAMMA_LOG` / `GAMMA_LOG_TO_LIN` FixedFunction, decoded once
/// for the CPU apply and the shared GPU emitter (`gpu/ff_curves.rs`).
///
/// Param order (`FixedFunctionOpCPU.cpp:2233-2245`): `[mirror, break, gamma power, post-power
/// scale, pre-power offset, log base, log-side slope, log-side offset, lin-side slope, lin-side
/// offset]`; the log base is folded into `log_slope`, as the reference does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GammaLogParams {
    pub mirror: f32,
    pub brk: f32,
    pub power: f32,
    pub gamma_slope: f32,
    pub gamma_off: f32,
    pub log_slope: f32,
    pub log_off: f32,
    pub lin_slope: f32,
    pub lin_off: f32,
    /// The break and mirror points in the encoded domain (`Renderer_GAMMA_LOG_TO_LIN`'s
    /// constructor, `FixedFunctionOpCPU.cpp:2284-2285`).
    pub prime_brk: f32,
    pub prime_mirror: f32,
}

impl GammaLogParams {
    pub(crate) fn from_params(params: &[f32]) -> Self {
        let p = |i: usize, d: f32| params.get(i).copied().unwrap_or(d);
        let (mirror, brk, power, gamma_slope, gamma_off) =
            (p(0, 0.0), p(1, 0.0), p(2, 1.0), p(3, 1.0), p(4, 0.0));
        let base = p(5, 2.0);
        Self {
            mirror,
            brk,
            power,
            gamma_slope,
            gamma_off,
            log_slope: p(6, 1.0) / base.ln(),
            log_off: p(7, 0.0),
            lin_slope: p(8, 1.0),
            lin_off: p(9, 0.0),
            prime_brk: gamma_slope * (brk + gamma_off).powf(power),
            prime_mirror: gamma_slope * (mirror + gamma_off).powf(power),
        }
    }

    /// `Renderer_LIN_TO_GAMMA_LOG::apply` for one channel.
    pub(crate) fn lin_to_gamma_log(&self, x: f32) -> f32 {
        let mirrored = x - self.mirror;
        let e = mirrored.abs() + self.mirror;
        let ep = if e < self.brk {
            self.gamma_slope * (e + self.gamma_off).powf(self.power)
        } else {
            self.log_slope * (self.lin_slope * e + self.lin_off).ln() + self.log_off
        };
        // copysign(1, mirrorin): +1 at the mirror point itself.
        ep * 1.0f32.copysign(mirrored)
    }

    /// `Renderer_GAMMA_LOG_TO_LIN::apply` for one channel.
    pub(crate) fn gamma_log_to_lin(&self, x: f32) -> f32 {
        let mirrored = x - self.prime_mirror;
        let ep = mirrored.abs() + self.prime_mirror;
        let e = if ep < self.prime_brk {
            (ep / self.gamma_slope).powf(1.0 / self.power) - self.gamma_off
        } else {
            (((ep - self.log_off) / self.log_slope).exp() - self.lin_off) / self.lin_slope
        };
        e * 1.0f32.copysign(mirrored)
    }
}

/// One log segment of OCIO's double-log FixedFunction:
/// `log_slope * ln(lin_slope * x + lin_off) + log_off` (base already folded into `log_slope`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LogSegment {
    pub log_slope: f32,
    pub log_off: f32,
    pub lin_slope: f32,
    pub lin_off: f32,
}

impl LogSegment {
    pub(crate) fn fwd(&self, x: f32) -> f32 {
        self.log_slope * (self.lin_slope * x + self.lin_off).ln() + self.log_off
    }
    pub(crate) fn inv(&self, y: f32) -> f32 {
        (((y - self.log_off) / self.log_slope).exp() - self.lin_off) / self.lin_slope
    }
}

/// Parameters of OCIO's `LIN_TO_DOUBLE_LOG` / `DOUBLE_LOG_TO_LIN` FixedFunction, decoded once
/// for the CPU apply and the shared GPU emitter (`gpu/ff_curves.rs`).
///
/// Param order (`FixedFunctionOpCPU.cpp:2325-2343`): `[base, break1, break2, log1 slope, log1
/// offset, lin1 slope, lin1 offset, log2 slope, log2 offset, lin2 slope, lin2 offset, linear
/// slope, linear offset]`. The break points belong to the log segments, not to the linear
/// segment, which may be absent (`break1 == break2`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DoubleLogParams {
    pub brk1: f32,
    pub brk2: f32,
    pub seg1: LogSegment,
    pub seg2: LogSegment,
    pub lin_slope: f32,
    pub lin_off: f32,
    /// The breaks in the log domain (`Renderer_DOUBLE_LOG_TO_LIN`'s constructor, :2383-2384).
    pub brk1_log: f32,
    pub brk2_log: f32,
}

impl DoubleLogParams {
    pub(crate) fn from_params(params: &[f32]) -> Self {
        let p = |i: usize, d: f32| params.get(i).copied().unwrap_or(d);
        let ln_base = p(0, 2.0).ln();
        let seg1 = LogSegment {
            log_slope: p(3, 1.0) / ln_base,
            log_off: p(4, 0.0),
            lin_slope: p(5, 1.0),
            lin_off: p(6, 0.0),
        };
        let seg2 = LogSegment {
            log_slope: p(7, 1.0) / ln_base,
            log_off: p(8, 0.0),
            lin_slope: p(9, 1.0),
            lin_off: p(10, 0.0),
        };
        let (brk1, brk2) = (p(1, 0.0), p(2, 1.0));
        Self {
            brk1,
            brk2,
            seg1,
            seg2,
            lin_slope: p(11, 1.0),
            lin_off: p(12, 0.0),
            brk1_log: seg1.fwd(brk1),
            brk2_log: seg2.fwd(brk2),
        }
    }

    pub(crate) fn lin_to_double_log(&self, x: f32) -> f32 {
        if x <= self.brk1 {
            self.seg1.fwd(x)
        } else if x < self.brk2 {
            self.lin_slope * x + self.lin_off
        } else {
            self.seg2.fwd(x)
        }
    }

    pub(crate) fn double_log_to_lin(&self, y: f32) -> f32 {
        if y <= self.brk1_log {
            self.seg1.inv(y)
        } else if y < self.brk2_log {
            (y - self.lin_off) / self.lin_slope
        } else {
            self.seg2.inv(y)
        }
    }
}

/// Resolves a FixedFunction's direction the way OCIO does: a paired style run in
/// reverse IS its partner style run forward (`FixedFunctionOpData::invert` swaps
/// `LIN_TO_PQ` <-> `PQ_TO_LIN`, `XYZ_TO_xyY` <-> `xyY_TO_XYZ`, ...,
/// `FixedFunctionOpData.cpp`).
///
/// The CPU apply and all four GPU emitters implement these styles one-directionally and
/// ignore the `forward` flag, so an inverse `FixedFunctionTransform { LIN_TO_PQ }` (or the
/// inverse of any builtin built on one) used to ENCODE again instead of decoding. Called at
/// the two points a direction enters an op: `compile.rs`'s FixedFunction arm and
/// `ProcessorOp::inverse()`. Styles that are not pairs (ACES glow/red-mod/gamut-comp,
/// surround, ACES 2.0) keep their flag, which their implementations honour.
pub(crate) fn resolve_fixed_function_direction(
    style: FixedFunctionStyle,
    forward: bool,
) -> (FixedFunctionStyle, bool) {
    use FixedFunctionStyle as F;
    if forward {
        return (style, true);
    }
    let partner = match style {
        F::RgbToHsv => F::HsvToRgb,
        F::HsvToRgb => F::RgbToHsv,
        F::XyzToXyy => F::XyyToXyz,
        F::XyyToXyz => F::XyzToXyy,
        F::XyzToUvy => F::UvyToXyz,
        F::UvyToXyz => F::XyzToUvy,
        F::XyzToLuv => F::LuvToXyz,
        F::LuvToXyz => F::XyzToLuv,
        F::LinToPq => F::PqToLin,
        F::PqToLin => F::LinToPq,
        F::LinToGammaLog => F::GammaLogToLin,
        F::GammaLogToLin => F::LinToGammaLog,
        F::LinToDoubleLog => F::DoubleLogToLin,
        F::DoubleLogToLin => F::LinToDoubleLog,
        F::RgbToHsyLin => F::HsyLinToRgb,
        F::HsyLinToRgb => F::RgbToHsyLin,
        F::RgbToHsyLog => F::HsyLogToRgb,
        F::HsyLogToRgb => F::RgbToHsyLog,
        F::RgbToHsyVid => F::HsyVidToRgb,
        F::HsyVidToRgb => F::RgbToHsyVid,
        _ => return (style, false),
    };
    (partner, true)
}

/// The moncurve (OCIO `GammaOpUtils.cpp` port) lives in vfx-transfer, the one moncurve of the
/// stack (vfx-transfer, vfx-ocio CPU and GPU, and heif-rs's verbatim copy).
pub(crate) use vfx_transfer::gamma::{MoncurveParams, moncurve_eval, moncurve_params};

/// A LUT's forward table: data, size, channel count, domain min and domain max.
pub type LutTable = (Vec<f32>, usize, usize, [f32; 3], [f32; 3]);

/// A LUT op's identity for OCIO's `RemoveInverseOps` (`OpOptimizers.cpp`): the forward table
/// (as compiled, with its domain), whether this op runs it forward or inverted, and the clamp a
/// round trip imposes in each order. Two adjacent LUT ops with equal `forward` and opposite
/// `inverse` are replaced by that clamp.
#[derive(Debug, Clone, PartialEq)]
pub struct LutPair {
    /// The forward table, its size, channel count and input domain.
    pub forward: std::sync::Arc<LutTable>,
    /// This op inverts `forward`.
    pub inverse: bool,
    /// `[lo, hi]` a forward-then-inverse round trip clamps to.
    pub fwd_then_inv: [f32; 2],
    /// `[lo, hi]` an inverse-then-forward round trip clamps to.
    pub inv_then_fwd: [f32; 2],
    /// A half-domain table: OCIO replaces the pair by an identity matrix, i.e. removes it
    /// (`Lut1DOpData::getPairIdentityReplacement`), and the clamps are unused.
    pub half_domain: bool,
}

impl LutPair {
    /// The pair data of a forward 1D LUT (OCIO `Lut1DOpData::getPairIdentityReplacement`, red
    /// channel only as OCIO): forward-then-inverse clamps to the input domain narrowed by flat
    /// runs at either end (`ComponentProperties::startDomain/endDomain`); inverse-then-forward to
    /// the forward table's first and last values.
    pub(crate) fn lut1d(
        data: &[f32],
        size: usize,
        channels: usize,
        domain_min: [f32; 3],
        domain_max: [f32; 3],
    ) -> Self {
        let red = |i: usize| data[i * channels];
        let mut end = size - 1;
        while end > 0 && red(end - 1) == red(size - 1) {
            end -= 1;
        }
        let mut start = 0;
        while start < end && red(start + 1) == red(0) {
            start += 1;
        }
        let at = |i: usize| {
            domain_min[0] + (domain_max[0] - domain_min[0]) * i as f32 / (size - 1) as f32
        };
        let (first, last) = (red(0), red(size - 1));
        Self {
            forward: std::sync::Arc::new((data.to_vec(), size, channels, domain_min, domain_max)),
            inverse: false,
            fwd_then_inv: [at(start), at(end)],
            inv_then_fwd: [first.min(last), first.max(last)],
            half_domain: false,
        }
    }

    /// The pair data of a forward 3D LUT (OCIO `Lut3DOpData::getIdentityReplacement`: a clamp to
    /// the domain either way). `None` when the domain differs per channel, which one scalar
    /// Range cannot express.
    pub(crate) fn lut3d(
        data: &[f32],
        size: usize,
        domain_min: [f32; 3],
        domain_max: [f32; 3],
    ) -> Option<Self> {
        let uniform = domain_min.iter().all(|v| *v == domain_min[0])
            && domain_max.iter().all(|v| *v == domain_max[0]);
        uniform.then(|| Self {
            forward: std::sync::Arc::new((data.to_vec(), size, 3, domain_min, domain_max)),
            inverse: false,
            fwd_then_inv: [domain_min[0], domain_max[0]],
            inv_then_fwd: [domain_min[0], domain_max[0]],
            half_domain: false,
        })
    }

    /// The pair data of a forward half-domain 1D LUT: a round trip is removed outright (OCIO
    /// `Lut1DOpData::getPairIdentityReplacement` returns an identity matrix for it).
    pub(crate) fn lut1d_half(data: &[f32], channels: usize) -> Self {
        Self {
            forward: std::sync::Arc::new((data.to_vec(), 65536, channels, [0.0; 3], [1.0; 3])),
            inverse: false,
            fwd_then_inv: [f32::NEG_INFINITY, f32::INFINITY],
            inv_then_fwd: [f32::NEG_INFINITY, f32::INFINITY],
            half_domain: true,
        }
    }

    /// The clamp that replaces `self` followed by `next`, if they are a LUT and its inverse.
    pub(crate) fn pair_clamp(&self, next: &LutPair) -> Option<[f32; 2]> {
        (self.inverse != next.inverse && self.forward == next.forward).then_some(if self.inverse {
            self.inv_then_fwd
        } else {
            self.fwd_then_inv
        })
    }
}

impl ProcessorOp {
    /// The forward LUT an exact inverse LUT is baked into where it cannot run per pixel (the
    /// GPU), or `None` for any other op.
    ///
    /// 3D: OCIO `MakeFastLut3DFromInverse`, the exact inverse sampled on a 48^3 grid over
    /// `[0, 1]^3`, which is what OCIO's GPU path always does. 1D: OCIO
    /// `MakeFastLut1DFromInverse` composes the inverse onto a lookup domain: a half-domain one
    /// (the exact inverse at each of the 65536 half codes, NaN codes at 0) when the forward LUT
    /// has values outside `[0, 1]` or its file depth is float (every float LUT file format),
    /// else one entry per code of that integer depth. This port does not carry the file depth,
    /// so it always takes the half-domain bake (OCIO's float-file case; for an in-memory LUT
    /// within `[0, 1]` OCIO would bake 4096 uniform entries instead).
    pub(crate) fn gpu_baked_inverse(&self) -> Option<ProcessorOp> {
        match self {
            ProcessorOp::Lut1d {
                hue_adjust,
                inverse: Some(inv),
                channels,
                ..
            } => {
                let ch = *channels;
                let mut data = Vec::with_capacity(65536 * ch);
                for code in 0..=u16::MAX {
                    let h = half::f16::from_bits(code);
                    let x = if h.is_nan() { 0.0 } else { h.to_f32() };
                    for c in 0..ch {
                        data.push(inv.eval(c, x));
                    }
                }
                Some(ProcessorOp::Lut1d {
                    lut: data,
                    size: 65536,
                    channels: ch,
                    domain_min: [0.0; 3],
                    domain_max: [1.0; 3],
                    half_domain: true,
                    hue_adjust: *hue_adjust,
                    pair: None,
                    inverse: None,
                })
            }
            ProcessorOp::Lut3d {
                inverse: Some(inv), ..
            } => {
                const N: usize = 48;
                let blue_fastest = inv.bake_grid(N);
                let red_fastest = vfx_lut::OrderedLut3dData::blue_fastest(blue_fastest, N)
                    .into_order(vfx_lut::Lut3dOrder::RedFastest);
                Some(ProcessorOp::Lut3d {
                    lut: red_fastest.iter().flat_map(|v| v.iter().copied()).collect(),
                    size: N,
                    // `MakeFastLut3DFromInverse` composes onto a fresh 48^3 domain, whose
                    // interpolation is the default (trilinear); it does not keep the forward's.
                    interp: Interpolation::Linear,
                    domain_min: [0.0; 3],
                    domain_max: [1.0; 3],
                    pair: None,
                    inverse: None,
                })
            }
            _ => None,
        }
    }
}

/// A clamp-only Range (`lo`/`hi`: `None` leaves that side open).
/// The control points of the eight hue curves, for `GradingHueCurve` equality.
fn hue_curve_points(
    c: &crate::grading_hue_curve::GradingHueCurves,
) -> [&[crate::grading_hue_curve::HueControlPoint]; 8] {
    [
        &c.hue_hue.points,
        &c.hue_sat.points,
        &c.hue_lum.points,
        &c.lum_sat.points,
        &c.sat_sat.points,
        &c.lum_lum.points,
        &c.sat_lum.points,
        &c.hue_fx.points,
    ]
}

fn clamp_range(lo: Option<f32>, hi: Option<f32>) -> ProcessorOp {
    ProcessorOp::Range {
        scale: 1.0,
        offset: 0.0,
        clamp_min: lo,
        clamp_max: hi,
    }
}

impl ProcessorOp {
    /// OCIO `RemoveInverseOps` (`OpOptimizers.cpp`) for `self` followed by `next`: `None` if they
    /// are not a pair of inverses; `Some(None)` if the pair is an exact identity and both go;
    /// `Some(Some(op))` if the round trip still clamps and `op` replaces the pair. The rules are
    /// each op data's `isInverse` + `getIdentityReplacement`:
    /// - Lut1D / Lut3D: [`LutPair`].
    /// - Log / LogAffine / LogCamera (`LogOpData`, all channels equal): log then antilog clamps
    ///   below at 0 (Log) or at `-linSideOffset / linSideSlope` (LogAffine); the other order and
    ///   the camera styles are exact.
    /// - Moncurve (`GammaOpData` `MONCURVE_*`): exact in both orders.
    pub(crate) fn pair_identity_with(&self, next: &ProcessorOp) -> Option<Option<ProcessorOp>> {
        use ProcessorOp as P;
        let all_eq = |v: &[f32; 3]| v[1] == v[0] && v[2] == v[0];
        match (self, next) {
            (P::Lut1d { pair: Some(a), .. }, P::Lut1d { pair: Some(b), .. })
            | (P::Lut3d { pair: Some(a), .. }, P::Lut3d { pair: Some(b), .. }) => {
                let [lo, hi] = a.pair_clamp(b)?;
                Some((!a.half_domain).then(|| clamp_range(Some(lo), Some(hi))))
            }
            (
                P::Log {
                    base: b1,
                    forward: f1,
                },
                P::Log {
                    base: b2,
                    forward: f2,
                },
            ) if b1 == b2 && f1 != f2 => Some(f1.then(|| clamp_range(Some(0.0), None))),
            (
                P::LogAffine {
                    base,
                    log_side_slope,
                    log_side_offset,
                    lin_side_slope,
                    lin_side_offset,
                    forward,
                },
                P::LogAffine {
                    base: base2,
                    log_side_slope: lss2,
                    log_side_offset: lso2,
                    lin_side_slope: ls2,
                    lin_side_offset: lo2,
                    forward: forward2,
                },
            ) if forward != forward2
                && base == base2
                && (
                    log_side_slope,
                    log_side_offset,
                    lin_side_slope,
                    lin_side_offset,
                ) == (lss2, lso2, ls2, lo2)
                && [
                    log_side_slope,
                    log_side_offset,
                    lin_side_slope,
                    lin_side_offset,
                ]
                .into_iter()
                .all(all_eq) =>
            {
                Some(
                    forward
                        .then(|| clamp_range(Some(-lin_side_offset[0] / lin_side_slope[0]), None)),
                )
            }
            (
                P::LogCamera { forward, .. },
                P::LogCamera {
                    forward: forward2, ..
                },
            ) if forward != forward2 => {
                // Equal parameters: compare everything but the direction.
                let same = |a: &ProcessorOp| match a {
                    P::LogCamera {
                        base,
                        log_side_slope,
                        log_side_offset,
                        lin_side_slope,
                        lin_side_offset,
                        lin_side_break,
                        linear_slope,
                        ..
                    } => Some((
                        *base,
                        *log_side_slope,
                        *log_side_offset,
                        *lin_side_slope,
                        *lin_side_offset,
                        *lin_side_break,
                        *linear_slope,
                    )),
                    _ => None,
                };
                let a = same(self)?;
                let equal_channels = [a.1, a.2, a.3, a.4, a.5, a.6].iter().all(all_eq);
                (a == same(next)? && equal_channels).then_some(None)
            }
            (
                P::ExponentWithLinear {
                    gamma,
                    offset,
                    mirror,
                    forward,
                },
                P::ExponentWithLinear {
                    gamma: g2,
                    offset: o2,
                    mirror: m2,
                    forward: f2,
                },
            ) if forward != f2 && (gamma, offset, mirror) == (g2, o2, m2) => Some(None),
            // CDLOpData::isInverse (same forward parameters and style, opposite direction);
            // getIdentityReplacement: the clamping styles still clamp to [0, 1].
            (
                P::Cdl {
                    slope,
                    offset,
                    power,
                    saturation,
                    style,
                    inverse,
                },
                P::Cdl {
                    slope: s2,
                    offset: o2,
                    power: p2,
                    saturation: sat2,
                    style: st2,
                    inverse: i2,
                },
            ) if inverse.is_some() != i2.is_some()
                && (slope, offset, power, saturation, style) == (s2, o2, p2, sat2, st2) =>
            {
                Some((*style == CdlStyle::AscCdl).then(|| clamp_range(Some(0.0), Some(1.0))))
            }
            // ExposureContrastOpData::isInverse: equal data, opposite direction, nothing dynamic.
            // Exact identity replacement.
            (
                P::ExposureContrast {
                    exposure,
                    contrast,
                    gamma,
                    pivot,
                    log_exposure_step,
                    log_mid_gray,
                    style,
                    dynamic,
                    forward,
                },
                P::ExposureContrast {
                    exposure: e2,
                    contrast: c2,
                    gamma: g2,
                    pivot: pv2,
                    log_exposure_step: ls2,
                    log_mid_gray: lm2,
                    style: st2,
                    dynamic: dyn2,
                    forward: f2,
                },
            ) if !dynamic.any()
                && !dyn2.any()
                && forward != f2
                && (
                    exposure,
                    contrast,
                    gamma,
                    pivot,
                    log_exposure_step,
                    log_mid_gray,
                    style,
                ) == (e2, c2, g2, pv2, ls2, lm2, st2) =>
            {
                Some(None)
            } // FixedFunctionOpData::isInverse: the inverse of (style, forward) with equal
            // parameters (a paired style's inverse is its partner run forward); REC2100_SURROUND
            // is also inverted by the reciprocal parameter in the same direction.
            (
                P::FixedFunction {
                    style,
                    params,
                    forward,
                },
                P::FixedFunction {
                    style: st2,
                    params: p2,
                    forward: f2,
                },
            ) => {
                // The inverse of `self`: a paired style (always held forward) is its partner.
                let inv = if *forward {
                    resolve_fixed_function_direction(*style, false)
                } else {
                    (*style, true)
                };
                // OCIO compares the double parameters exactly (`p == 1 / q`); this op holds them
                // as f32, so the reciprocal is compared within f32 rounding.
                let surround = *style == FixedFunctionStyle::Rec2100Surround
                    && style == st2
                    && forward == f2
                    && params.len() == 1
                    && p2.len() == 1
                    && (params[0] as f64 * p2[0] as f64 - 1.0).abs() < 1e-6;
                (surround || ((*st2, *f2) == inv && params == p2)).then_some(None)
            }
            // The ACES 2.0 output transform: its state is shared per parameter set
            // (`init_output_transform_cached`), so the same Arc means the same parameters.
            (
                P::Aces2OutputTransform { state, forward },
                P::Aces2OutputTransform {
                    state: s2,
                    forward: f2,
                },
            ) if forward != f2 && std::sync::Arc::ptr_eq(state, s2) => Some(None),
            // Grading*OpData::isInverse: same style and value, opposite direction (not dynamic;
            // this port's grading ops are not). GradingPrimary's replacement keeps its clamps.
            (
                P::GradingPrimary {
                    style,
                    raw,
                    forward,
                    ..
                },
                P::GradingPrimary {
                    style: st2,
                    raw: r2,
                    forward: f2,
                    ..
                },
            ) if forward != f2 && style == st2 && raw == r2 => {
                let lo = (raw.clamp_black != -f64::MAX).then_some(raw.clamp_black as f32);
                let hi = (raw.clamp_white != f64::MAX).then_some(raw.clamp_white as f32);
                Some((lo.is_some() || hi.is_some()).then(|| clamp_range(lo, hi)))
            }
            (
                P::GradingRgbCurve {
                    style,
                    raw,
                    bypass_lin_to_log,
                    forward,
                    ..
                },
                P::GradingRgbCurve {
                    style: st2,
                    raw: r2,
                    bypass_lin_to_log: b2,
                    forward: f2,
                    ..
                },
            ) if forward != f2
                && style == st2
                && (*style != crate::transform::GradingToneStyle::Linear
                    || bypass_lin_to_log == b2)
                && (&raw.red, &raw.green, &raw.blue, &raw.master)
                    == (&r2.red, &r2.green, &r2.blue, &r2.master)
                && (
                    &raw.red_slopes,
                    &raw.green_slopes,
                    &raw.blue_slopes,
                    &raw.master_slopes,
                ) == (
                    &r2.red_slopes,
                    &r2.green_slopes,
                    &r2.blue_slopes,
                    &r2.master_slopes,
                ) =>
            {
                Some(None)
            }
            (
                P::GradingTone {
                    style,
                    value,
                    forward,
                    ..
                },
                P::GradingTone {
                    style: st2,
                    value: v2,
                    forward: f2,
                    ..
                },
            ) if forward != f2 && style == st2 && value == v2 => Some(None),
            (
                P::GradingHueCurve {
                    curves,
                    slopes,
                    rgb_to_hsy,
                    draw_curve_only,
                    forward,
                    ..
                },
                P::GradingHueCurve {
                    curves: c2,
                    slopes: s2,
                    rgb_to_hsy: h2,
                    draw_curve_only: d2,
                    forward: f2,
                    ..
                },
            ) if forward != f2
                && curves.style == c2.style
                && (curves.style != crate::grading_hue_curve::GradingStyle::Linear
                    || rgb_to_hsy == h2)
                && draw_curve_only == d2
                && slopes == s2
                && hue_curve_points(curves) == hue_curve_points(c2) =>
            {
                Some(None)
            }
            _ => None,
        }
    }

    /// OCIO `ReplaceIdentityOps` for the ops whose identity still clamps: an identity ASC CDL
    /// is a clamp to [0, 1] (`CDLOpData::getIdentityReplacement`), an identity BASIC gamma
    /// (Exponent with `Clamp`) a clamp at 0 (`GammaOpData::getIdentityReplacement`). OCIO's
    /// identity tests are exact.
    pub(crate) fn identity_replacement(&self) -> Option<ProcessorOp> {
        match self {
            ProcessorOp::Cdl {
                slope,
                offset,
                power,
                saturation,
                style: CdlStyle::AscCdl,
                ..
            } if *slope == [1.0; 3]
                && *offset == [0.0; 3]
                && *power == [1.0; 3]
                && *saturation == 1.0 =>
            {
                Some(clamp_range(Some(0.0), Some(1.0)))
            }
            ProcessorOp::Exponent {
                value,
                negative_style: NegativeStyle::Clamp,
            } if *value == [1.0; 4] => Some(clamp_range(Some(0.0), None)),
            _ => None,
        }
    }

    /// OCIO `ReplaceOps` (`SIMPLIFY_OPS`, part of LOSSLESS): `CDLOpData::getSimplerReplacement`
    /// turns a CDL without power (and not an identity) into its slope/offset matrix, the
    /// saturation matrix and the [0, 1] clamps, so they can combine with neighbours. The
    /// inverse is the reversed list with each matrix inverted.
    pub(crate) fn simpler_replacement(&self) -> Option<Vec<ProcessorOp>> {
        let ProcessorOp::Cdl {
            slope,
            offset,
            power,
            saturation,
            style,
            inverse,
        } = self
        else {
            return None;
        };
        let identity = *slope == [1.0; 3] && *offset == [0.0; 3] && *saturation == 1.0;
        if *power != [1.0; 3] || identity {
            return None;
        }
        let clamps = *style == CdlStyle::AscCdl;
        let fwd = inverse.is_none();
        // A singular inverse (a zero slope) is left to the CDL op, which floors it.
        let matrix_op = |m: [f64; 16], o: [f64; 4]| -> Option<ProcessorOp> {
            let (m, o) = if fwd {
                (m, o)
            } else {
                invert_matrix_offset(&m, &o)?
            };
            Some(ProcessorOp::Matrix {
                matrix: m.map(|v| v as f32),
                offset: o.map(|v| v as f32),
            })
        };
        let mut so = [0.0f64; 16];
        for c in 0..3 {
            so[c * 5] = slope[c] as f64;
        }
        so[15] = 1.0;
        let mut ops = vec![matrix_op(
            so,
            [offset[0] as f64, offset[1] as f64, offset[2] as f64, 0.0],
        )?];
        if *saturation != 1.0 {
            if clamps {
                ops.push(clamp_range(Some(0.0), Some(1.0)));
            }
            // MatrixTransform::Sat with the Rec.709 luma OCIO's CDL uses.
            let s = *saturation as f64;
            let luma = [0.2126, 0.7152, 0.0722];
            let mut m = [0.0f64; 16];
            for i in 0..3 {
                for j in 0..3 {
                    m[i * 4 + j] = (1.0 - s) * luma[j] + if i == j { s } else { 0.0 };
                }
            }
            m[15] = 1.0;
            ops.push(matrix_op(m, [0.0; 4])?);
        }
        if clamps {
            ops.push(clamp_range(Some(0.0), Some(1.0)));
        }
        if !fwd {
            ops.reverse();
            // Better than the reference: OCIO's reversed list ends on the inverted slope/offset
            // matrix, so its power-free `CDL_V1_2_REV` returns values outside [0, 1] (PyOpenColorIO
            // 2.5.1: -0.0167 and 1.122 for [-0.2, 1.4]) while its renderer path and the ASC v1.2
            // spec clamp the result. `vfx_lut::cdl::apply_cdl_inverse_clamped` clamps; so does this.
            if clamps {
                ops.push(clamp_range(Some(0.0), Some(1.0)));
            }
        }
        Some(ops)
    }

    /// OCIO `CombineOps` for the types LOSSLESS composes: Matrix (COMP_MATRIX), Range
    /// (COMP_RANGE) and basic gamma (COMP_GAMMA, `GammaOpData::mayCompose` / `compose`).
    /// `None` when `self` and `next` do not combine.
    pub(crate) fn combine_with(&self, next: &ProcessorOp) -> Option<ProcessorOp> {
        use ProcessorOp as P;
        match (self, next) {
            (
                P::Matrix {
                    matrix: m1,
                    offset: o1,
                },
                P::Matrix {
                    matrix: m2,
                    offset: o2,
                },
            ) => {
                // next(self(x)) = M2 (M1 x + o1) + o2, composed in f64 as OCIO.
                let (m1, m2) = (m1.map(|v| v as f64), m2.map(|v| v as f64));
                let mut m = [0.0f64; 16];
                let mut o = [0.0f64; 4];
                for i in 0..4 {
                    for j in 0..4 {
                        m[i * 4 + j] = (0..4).map(|k| m2[i * 4 + k] * m1[k * 4 + j]).sum();
                    }
                    o[i] = (0..4).map(|k| m2[i * 4 + k] * o1[k] as f64).sum::<f64>() + o2[i] as f64;
                }
                Some(P::Matrix {
                    matrix: m.map(|v| v as f32),
                    offset: o.map(|v| v as f32),
                })
            }
            (P::Range { .. }, P::Range { .. }) => self.combine_range_with(next),
            (
                P::Exponent {
                    value: v1,
                    negative_style: s1,
                },
                P::Exponent {
                    value: v2,
                    negative_style: s2,
                },
            ) => {
                use NegativeStyle::{Clamp, Mirror, PassThru};
                // mayCompose: a clamping (BASIC) gamma composes with any basic style; MIRROR
                // with BASIC or MIRROR; PASS_THRU with BASIC or PASS_THRU. The result clamps if
                // either did.
                let style = match (s1, s2) {
                    (Clamp, Clamp | Mirror | PassThru) | (Mirror | PassThru, Clamp) => Clamp,
                    (Mirror, Mirror) => Mirror,
                    (PassThru, PassThru) => PassThru,
                    _ => return None,
                };
                // The exponents multiply; RoundAround1 makes e.g. 1/0.45 * 0.45 exactly 1.
                let value = std::array::from_fn(|c| {
                    let v = v1[c] as f64 * v2[c] as f64;
                    if (v - 1.0).abs() < 1e-6 {
                        1.0
                    } else {
                        v as f32
                    }
                });
                Some(P::Exponent {
                    value,
                    negative_style: style,
                })
            }
            _ => None,
        }
    }

    /// OCIO COMP_LUT1D (`Lut1DOp::combineWith`, `Lut1DOpData::Compose` with
    /// `COMPOSE_RESAMPLE_BIG`): two forward 1D LUTs without hue adjust become one. A half-domain
    /// or 65536-entry first LUT keeps its own domain and its entries are run through the second;
    /// a smaller one is resampled on 65536 entries over its domain through both. The result has
    /// three channels, as OCIO's.
    pub(crate) fn compose_lut1d_with(&self, next: &ProcessorOp) -> Option<ProcessorOp> {
        use crate::transform::Lut1DHueAdjust::None as NoHue;
        let (
            ProcessorOp::Lut1d {
                lut,
                size,
                channels,
                domain_min,
                domain_max,
                half_domain,
                hue_adjust: NoHue,
                inverse: None,
                ..
            },
            ProcessorOp::Lut1d {
                hue_adjust: NoHue,
                inverse: None,
                ..
            },
        ) = (self, next)
        else {
            return None;
        };
        const BIG: usize = 65536;
        let entry = |i: usize| -> [f32; 3] {
            std::array::from_fn(|c| lut[i * channels + c.min(channels - 1)])
        };
        let keep_domain = *half_domain || *size >= BIG;
        let n = if keep_domain { *size } else { BIG };
        let mut data = Vec::with_capacity(n * 3);
        for i in 0..n {
            let mut px = if keep_domain {
                entry(i)
            } else {
                let mut x: [f32; 3] = std::array::from_fn(|c| {
                    domain_min[c] + (domain_max[c] - domain_min[c]) * i as f32 / (BIG - 1) as f32
                });
                super::Processor::apply_op_to_rgb(self, &mut x);
                x
            };
            super::Processor::apply_op_to_rgb(next, &mut px);
            data.extend_from_slice(&px);
        }
        let pair = Some(if *half_domain {
            LutPair::lut1d_half(&data, 3)
        } else {
            LutPair::lut1d(&data, n, 3, *domain_min, *domain_max)
        });
        Some(ProcessorOp::Lut1d {
            lut: data,
            size: n,
            channels: 3,
            domain_min: *domain_min,
            domain_max: *domain_max,
            half_domain: *half_domain,
            hue_adjust: NoHue,
            pair,
            inverse: None,
        })
    }

    /// OCIO COMP_LUT3D (`Lut3DOp::combineWith`, `Lut3DOpData::Compose`): two forward 3D LUTs
    /// become one on the larger grid. When the first grid is at least as large, its node values
    /// are run through the second; otherwise an identity grid of the second's size (over the
    /// first's domain) is run through both. The first LUT's interpolation is kept.
    pub(crate) fn compose_lut3d_with(&self, next: &ProcessorOp) -> Option<ProcessorOp> {
        let (
            ProcessorOp::Lut3d {
                lut,
                size,
                interp,
                domain_min,
                domain_max,
                inverse: None,
                ..
            },
            ProcessorOp::Lut3d {
                size: size2,
                inverse: None,
                ..
            },
        ) = (self, next)
        else {
            return None;
        };
        let n = (*size).max(*size2);
        let mut data = Vec::with_capacity(n * n * n * 3);
        // Red-fastest, as `ProcessorOp::Lut3d.lut`.
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    let mut px = if n == *size {
                        let i = (b * n * n + g * n + r) * 3;
                        [lut[i], lut[i + 1], lut[i + 2]]
                    } else {
                        let mut x: [f32; 3] = std::array::from_fn(|c| {
                            let k = [r, g, b][c];
                            domain_min[c]
                                + (domain_max[c] - domain_min[c]) * k as f32 / (n - 1) as f32
                        });
                        super::Processor::apply_op_to_rgb(self, &mut x);
                        x
                    };
                    super::Processor::apply_op_to_rgb(next, &mut px);
                    data.extend_from_slice(&px);
                }
            }
        }
        let pair = LutPair::lut3d(&data, n, *domain_min, *domain_max);
        Some(ProcessorOp::Lut3d {
            lut: data,
            size: n,
            interp: *interp,
            domain_min: *domain_min,
            domain_max: *domain_max,
            pair,
            inverse: None,
        })
    }

    /// OCIO `RangeOp::combineWith` (COMP_RANGE, part of LOSSLESS): `self` then `next` as one
    /// Range when `next` does not flip the sign (a flip would swap the clamp bounds).
    pub(crate) fn combine_range_with(&self, next: &ProcessorOp) -> Option<ProcessorOp> {
        let (
            ProcessorOp::Range {
                scale: s1,
                offset: o1,
                clamp_min: lo1,
                clamp_max: hi1,
            },
            ProcessorOp::Range {
                scale: s2,
                offset: o2,
                clamp_min: lo2,
                clamp_max: hi2,
            },
        ) = (self, next)
        else {
            return None;
        };
        if *s2 <= 0.0 {
            return None;
        }
        let through = |v: Option<f32>| v.map(|v| v * s2 + o2);
        let tighter = |a: Option<f32>, b: Option<f32>, max: bool| match (a, b) {
            (Some(a), Some(b)) => Some(if max { a.max(b) } else { a.min(b) }),
            (a, b) => a.or(b),
        };
        let lo = tighter(through(*lo1), *lo2, true);
        let mut hi = tighter(through(*hi1), *hi2, false);
        // Disjoint clamps give a constant: keep the upper bound at the lower one.
        if let (Some(l), Some(h)) = (lo, hi)
            && h < l
        {
            hi = Some(l);
        }
        Some(ProcessorOp::Range {
            scale: s1 * s2,
            offset: o1 * s2 + o2,
            clamp_min: lo,
            clamp_max: hi,
        })
    }
}

/// The precomputed, direction-resolved parameters `ProcessorOp::GradingPrimary`
/// carries. Field meaning is style-dependent (see [`grading_primary_params`]'s
/// doc comment), exactly mirroring the `ProcessorOp::GradingPrimary` struct
/// variant's own fields.
pub(crate) struct GradingPrimaryParams {
    pub brightness: [f32; 3],
    pub contrast: [f32; 3],
    pub gamma: [f32; 3],
    pub exposure: [f32; 3],
    pub offset: [f32; 3],
    pub slope: [f32; 3],
    pub pivot: f32,
}

/// OCIO's `GradingPrimaryPreRender::m_localBypass` (`GradingPrimary.cpp:108-110` plus the
/// per-style tail of `update`): the op is skipped only when saturation is 1, BOTH clamps are
/// OCIO's "no clamp" (`NoClampBlack() = -DBL_MAX`, `NoClampWhite() = DBL_MAX`) and every
/// direction-resolved parameter is exactly its identity. The fields a style does not use hold
/// their identity in [`GradingPrimaryParams`], so one test covers the three styles. The GPU
/// emitters skip the whole body on it (EMIT-12).
pub(crate) fn grading_primary_local_bypass(
    p: &GradingPrimaryParams,
    v: &crate::dynamic::GradingPrimary,
) -> bool {
    v.saturation == 1.0
        && v.clamp_black == -f64::MAX
        && v.clamp_white == f64::MAX
        && p.brightness == [0.0; 3]
        && p.contrast == [1.0; 3]
        && p.gamma == [1.0; 3]
        && p.exposure == [1.0; 3]
        && p.offset == [0.0; 3]
        && p.slope == [1.0; 3]
}

/// Ports OCIO's `GradingPrimaryPreRender::update` (`ops/gradingprimary/
/// GradingPrimary.cpp:100-220`) — the ONE place the raw RGBM
/// `crate::dynamic::GradingPrimary` value is combined (master folded into
/// each channel) and direction-resolved (inverted/reciprocated for
/// `!forward`) into the flat per-channel numbers `ProcessorOp::GradingPrimary`
/// and every GPU emitter consume directly. Called once at compile time
/// (`processor/compile.rs`'s `Transform::GradingPrimary` arm) — CPU apply and
/// all four GPU emitters read the SAME already-resolved fields, so fixing
/// this one function fixes all five places at once (unlike the LUT3D order
/// bug, where CPU and GPU genuinely diverged).
///
/// Style semantics (which fields matter, matching OCIO's own per-style
/// wiring — the other fields are left at their identity default by the
/// caller building the raw `GradingPrimary` value, not by this function):
/// - **Log**: `brightness` (additive), `contrast`/`gamma` (multiplicative,
///   around a remapped `pivot = 0.5 + raw_pivot*0.5`).
/// - **Linear**: `offset` (additive), `exposure` (`2^stops` multiplier),
///   `contrast` (power-law around a remapped `pivot = 0.18 * 2^raw_pivot`).
/// - **Video**: `offset` (additive, `offset+lift` combined), `slope`
///   (algebraically derived from `lift`/`gain`/`pivot_black`/`pivot_white`,
///   NOT a directly-settable field), `gamma` (multiplicative).
pub(crate) fn grading_primary_params(
    style: GradingPrimaryStyle,
    forward: bool,
    v: &crate::dynamic::GradingPrimary,
) -> GradingPrimaryParams {
    let rgbm3 = |g: &crate::dynamic::GradingRgbm| -> [f64; 3] {
        [g.red + g.master, g.green + g.master, g.blue + g.master]
    };
    let rgbm3_mul = |g: &crate::dynamic::GradingRgbm| -> [f64; 3] {
        [g.master * g.red, g.master * g.green, g.master * g.blue]
    };

    match style {
        GradingPrimaryStyle::Log => {
            let b = rgbm3(&v.brightness);
            let c = rgbm3_mul(&v.contrast);
            let g = rgbm3_mul(&v.gamma);
            let sign = if forward { 1.0 } else { -1.0 };
            let brightness = [
                (sign * b[0] * 6.25 / 1023.0) as f32,
                (sign * b[1] * 6.25 / 1023.0) as f32,
                (sign * b[2] * 6.25 / 1023.0) as f32,
            ];
            let (contrast, gamma) = if forward {
                (
                    [c[0] as f32, c[1] as f32, c[2] as f32],
                    [
                        (1.0 / g[0]) as f32,
                        (1.0 / g[1]) as f32,
                        (1.0 / g[2]) as f32,
                    ],
                )
            } else {
                let inv = |x: f64| 1.0 / if x == 0.0 { 1.0 } else { x };
                (
                    [inv(c[0]) as f32, inv(c[1]) as f32, inv(c[2]) as f32],
                    [g[0] as f32, g[1] as f32, g[2] as f32],
                )
            };
            GradingPrimaryParams {
                brightness,
                contrast,
                gamma,
                exposure: [1.0; 3],
                offset: [0.0; 3],
                slope: [1.0; 3],
                pivot: (0.5 + v.pivot * 0.5) as f32,
            }
        }
        GradingPrimaryStyle::Linear => {
            let o = rgbm3(&v.offset);
            let e = rgbm3(&v.exposure);
            let c = rgbm3_mul(&v.contrast);
            let sign = if forward { 1.0 } else { -1.0 };
            let offset = [
                (sign * o[0]) as f32,
                (sign * o[1]) as f32,
                (sign * o[2]) as f32,
            ];
            let (exposure, contrast) = if forward {
                (
                    [
                        2.0_f32.powf(e[0] as f32),
                        2.0_f32.powf(e[1] as f32),
                        2.0_f32.powf(e[2] as f32),
                    ],
                    [c[0] as f32, c[1] as f32, c[2] as f32],
                )
            } else {
                let inv = |x: f64| 1.0 / if x == 0.0 { 1.0 } else { x };
                (
                    [
                        (1.0 / 2.0_f32.powf(e[0] as f32)),
                        (1.0 / 2.0_f32.powf(e[1] as f32)),
                        (1.0 / 2.0_f32.powf(e[2] as f32)),
                    ],
                    [inv(c[0]) as f32, inv(c[1]) as f32, inv(c[2]) as f32],
                )
            };
            GradingPrimaryParams {
                brightness: [0.0; 3],
                contrast,
                gamma: [1.0; 3],
                exposure,
                offset,
                slope: [1.0; 3],
                pivot: (0.18 * 2.0_f64.powf(v.pivot)) as f32,
            }
        }
        GradingPrimaryStyle::Video => {
            let o = rgbm3(&v.offset);
            let l = rgbm3(&v.lift);
            let g = rgbm3_mul(&v.gamma);
            let gain_raw = rgbm3_mul(&v.gain);
            let gain = [
                if gain_raw[0] == 0.0 { 1.0 } else { gain_raw[0] },
                if gain_raw[1] == 0.0 { 1.0 } else { gain_raw[1] },
                if gain_raw[2] == 0.0 { 1.0 } else { gain_raw[2] },
            ];
            let pb = v.pivot_black;
            let pw = v.pivot_white;
            let (offset, slope, gamma) = if forward {
                let offset = [
                    (o[0] + l[0]) as f32,
                    (o[1] + l[1]) as f32,
                    (o[2] + l[2]) as f32,
                ];
                let slope_den = [
                    pw / gain[0] + l[0] - pb,
                    pw / gain[1] + l[1] - pb,
                    pw / gain[2] + l[2] - pb,
                ];
                let slope = [
                    ((pw - pb)
                        / if slope_den[0] == 0.0 {
                            1.0
                        } else {
                            slope_den[0]
                        }) as f32,
                    ((pw - pb)
                        / if slope_den[1] == 0.0 {
                            1.0
                        } else {
                            slope_den[1]
                        }) as f32,
                    ((pw - pb)
                        / if slope_den[2] == 0.0 {
                            1.0
                        } else {
                            slope_den[2]
                        }) as f32,
                ];
                let gamma = [
                    (1.0 / g[0]) as f32,
                    (1.0 / g[1]) as f32,
                    (1.0 / g[2]) as f32,
                ];
                (offset, slope, gamma)
            } else {
                let offset = [
                    (-(o[0] + l[0])) as f32,
                    (-(o[1] + l[1])) as f32,
                    (-(o[2] + l[2])) as f32,
                ];
                let slope = [
                    ((pw / gain[0] + (l[0] - pb)) / (pw - pb)) as f32,
                    ((pw / gain[1] + (l[1] - pb)) / (pw - pb)) as f32,
                    ((pw / gain[2] + (l[2] - pb)) / (pw - pb)) as f32,
                ];
                let gamma = [g[0] as f32, g[1] as f32, g[2] as f32];
                (offset, slope, gamma)
            };
            GradingPrimaryParams {
                brightness: [0.0; 3],
                contrast: [1.0; 3],
                gamma,
                exposure: [1.0; 3],
                offset,
                slope,
                pivot: 0.0,
            }
        }
    }
}

/// Saturation as a row-major 4x4 matrix: `out = luma + k * (in - luma)`.
///
/// The reverse CDL renderer applies the *same* saturation formula with `k =
/// Reciprocal(sat)` rather than inverting the forward saturation matrix, so this
/// is built from `k` directly to stay bit-comparable with the reference.
fn saturation_matrix(k: f32) -> [f32; 16] {
    use vfx_core::consts::luma::{REC709_LUMA_B, REC709_LUMA_G, REC709_LUMA_R};
    let w = [REC709_LUMA_R, REC709_LUMA_G, REC709_LUMA_B];
    let e = |i: usize, j: usize| if i == j { k } else { 0.0 } + (1.0 - k) * w[j];
    [
        e(0, 0),
        e(0, 1),
        e(0, 2),
        0.0, //
        e(1, 0),
        e(1, 1),
        e(1, 2),
        0.0, //
        e(2, 0),
        e(2, 1),
        e(2, 2),
        0.0, //
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

/// The inverse of a forward CDL, as an op **sequence**.
///
/// A forward CDL is `slope -> offset -> [clamp] -> power -> saturation -> [clamp]`.
/// Its inverse therefore runs the reverse order — saturation first, slope last —
/// which a single forward-CDL op with reciprocal parameters cannot express. All
/// three inversion sites in this crate did exactly that: they inverted the
/// parameters but left the order forward, so the result was not an inverse at all
/// (only the degenerate sat=1, power=1 case came out right).
///
/// Mirrors `CDLRendererRev::apply` (`CDLOpCPU.cpp:443-469`): clamp, saturation,
/// power, offset, slope, clamp — with the clamps present only for the clamping
/// style, and `ApplyPower<CLAMP>`'s own pre-power clamp made explicit.
///
/// Composed from ops that already exist and are verified on every backend, rather
/// than from a new `*_REV` op variant: a new variant would need a matching branch
/// in each of the four shader emitters, and until it had one the GPU path would
/// silently run forward math on reverse parameters.
pub(crate) fn cdl_inverse_ops(
    slope: [f32; 3],
    offset: [f32; 3],
    power: [f32; 3],
    saturation: f32,
    style: CdlStyle,
) -> Vec<ProcessorOp> {
    let clamps = matches!(style, CdlStyle::AscCdl);
    let clamp_01 = ProcessorOp::Range {
        scale: 1.0,
        offset: 0.0,
        clamp_min: Some(0.0),
        clamp_max: Some(1.0),
    };

    let slope_rev = slope.map(cdl_rcp);
    let mut ops = Vec::with_capacity(6);

    if clamps {
        ops.push(clamp_01.clone());
    }

    // Saturation, reversed.
    ops.push(ProcessorOp::Matrix {
        matrix: saturation_matrix(cdl_rcp(saturation)),
        offset: [0.0; 4],
    });

    // Power, reversed. `ApplyPower<true>` clamps to [0,1] first, after which every
    // value is non-negative; `ApplyPower<false>` passes negatives through untouched.
    // Both are `PassThru` on RGB. It must be `PassThru` for alpha too: the exponent op
    // evaluates alpha with exponent 1, which `PassThru` leaves alone while `Clamp` would
    // zero a negative alpha, and a CDL never touches alpha (B28).
    if clamps {
        ops.push(clamp_01.clone());
    }
    ops.push(ProcessorOp::Exponent {
        value: [cdl_rcp(power[0]), cdl_rcp(power[1]), cdl_rcp(power[2]), 1.0],
        negative_style: NegativeStyle::PassThru,
    });

    // Offset then slope, i.e. `(v - offset) * slope_rev`, expressed in the affine
    // `v * slope + offset` form the CDL op evaluates. `NoClamp` because the
    // trailing clamp below is the reference's single `ApplyClamp` at this point.
    ops.push(ProcessorOp::Cdl {
        slope: slope_rev,
        offset: [
            -offset[0] * slope_rev[0],
            -offset[1] * slope_rev[1],
            -offset[2] * slope_rev[2],
        ],
        power: [1.0; 3],
        saturation: 1.0,
        style: CdlStyle::NoClamp,
        inverse: None,
    });

    if clamps {
        ops.push(clamp_01);
    }

    ops
}

/// Internal operation type.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum ProcessorOp {
    /// 4x4 matrix + offset.
    Matrix { matrix: [f32; 16], offset: [f32; 4] },
    /// 1D LUT.
    Lut1d {
        lut: Vec<f32>,
        size: usize,
        channels: usize,
        /// Per-channel domain min [R, G, B]
        domain_min: [f32; 3],
        /// Per-channel domain max [R, G, B]
        domain_max: [f32; 3],
        /// OCIO `LUT_INPUT_HALF_CODE`: `lut` holds one entry per 16-bit half code (65536) and
        /// is indexed by the input's half representation, not by `domain_min..domain_max`
        /// (which is then unused). See `math::half_domain_edges`.
        half_domain: bool,
        /// Optional hue adjustment.
        hue_adjust: crate::transform::Lut1DHueAdjust,
        /// The forward table this op evaluates or inverts, for the optimizer's inverse-pair
        /// removal (`None`: no pair can be recognised).
        pair: Option<LutPair>,
        /// `Some`: this op is the exact inverse of the forward table in `lut` (and `domain_*` is
        /// that table's input domain), evaluated per pixel as OCIO's `InvLut1DRenderer`.
        inverse: Option<std::sync::Arc<super::inv_lut::InvLut1d>>,
    },
    /// 3D LUT.
    Lut3d {
        lut: Vec<f32>,
        size: usize,
        interp: Interpolation,
        domain_min: [f32; 3],
        domain_max: [f32; 3],
        /// As `Lut1d::pair`.
        pair: Option<LutPair>,
        /// `Some`: this op is the exact inverse of the forward table in `lut` (OCIO
        /// `InvLut3DRenderer`).
        inverse: Option<std::sync::Arc<super::inv_lut::InvLut3d>>,
    },
    /// Exponent.
    Exponent {
        value: [f32; 4],
        negative_style: NegativeStyle,
    },
    /// Simple log base conversion (OCIO Log10/Log2).
    /// Forward: out = log(base, max(MIN_POSITIVE, in))
    /// Inverse: out = pow(base, in)
    Log { base: f32, forward: bool },
    /// CDL. The parameters are always the FORWARD CDL's (OCIO `CDLOpData`); `inverse: Some`
    /// makes this op the inverse (`CDLRendererRev`), evaluated as the op sequence
    /// [`cdl_inverse_ops`] built once at construction ([`ProcessorOp::cdl`]). Keeping one op
    /// with a direction is what lets the optimizer pair a CDL with its inverse as OCIO does.
    Cdl {
        slope: [f32; 3],
        offset: [f32; 3],
        power: [f32; 3],
        saturation: f32,
        style: CdlStyle,
        inverse: Option<std::sync::Arc<Vec<ProcessorOp>>>,
    },
    /// Range clamp/scale.
    Range {
        scale: f32,
        offset: f32,
        clamp_min: Option<f32>,
        clamp_max: Option<f32>,
    },
    /// Built-in transfer function.
    Transfer { style: TransferStyle, forward: bool },
    /// Exposure/contrast adjustment (OCIO parity).
    /// OCIO ref: ExposureContrastOpCPU.cpp.
    ExposureContrast {
        exposure: f32,
        contrast: f32,
        gamma: f32,
        pivot: f32,
        /// Log exposure step (for Logarithmic style).
        /// Default: 0.088 (Cineon).
        log_exposure_step: f32,
        /// Log mid gray (for Logarithmic style).
        /// Default: 0.435.
        log_mid_gray: f32,
        style: ExposureContrastStyle,
        /// Parameters a host may change after the build ([`crate::Processor::set_dynamic`]).
        /// A dynamic op is never an identity and never pairs away, whatever its values now.
        dynamic: EcDynamic,
        /// Direction. `exposure`/`contrast`/`gamma` are ALWAYS the forward parameters (what a
        /// dynamic property holds); the CPU renderer and the shader pick OCIO's reverse form
        /// (`ECLinearRevRenderer` and friends) when this is false.
        forward: bool,
    },
    /// Fixed function (ACES-specific).
    FixedFunction {
        style: FixedFunctionStyle,
        params: Vec<f32>,
        forward: bool,
    },
    /// ACES 2.0 Output Transform (pre-computed state).
    ///
    /// `Arc`, not `Box` (PLAN9.md O10): `state` comes from
    /// [`crate::aces2::init_output_transform_cached`], which shares one instance across every
    /// processor built for the same `(peak_lum, primaries)` pair. `Box` would defeat that by
    /// deep-copying the ~7.3 KB table (`hue_table` + `gamut_cusp_table`) every time this op is
    /// cloned - which `ProcessorCache::get_or_create*` does on EVERY cache hit, and `invert()`
    /// does on every inversion.
    Aces2OutputTransform {
        state: std::sync::Arc<crate::aces2::Aces2State>,
        forward: bool,
    },
    /// ACES 2.0 RGB <-> JMh conversion (pre-computed params).
    Aces2RgbJmh {
        params: Box<crate::aces2::JMhParams>,
        forward: bool,
    },
    /// ACES 2.0 Tonescale + Chroma compress (pre-computed).
    Aces2TonescaleCompress {
        jmh_params: Box<crate::aces2::JMhParams>,
        ts_params: Box<crate::aces2::ToneScaleParams>,
        shared: Box<crate::aces2::SharedCompressionParams>,
        chroma: Box<crate::aces2::ChromaCompressParams>,
        forward: bool,
    },
    /// ACES 2.0 Gamut compress (pre-computed).
    Aces2GamutCompress {
        shared: Box<crate::aces2::SharedCompressionParams>,
        gamut: Box<crate::aces2::GamutCompressParams>,
        forward: bool,
    },
    /// Allocation (log/linear).
    Allocation {
        allocation: AllocationType,
        vars: Vec<f32>,
        forward: bool,
    },
    /// Grading primary (OCIO parity with Log/Linear/Video styles).
    GradingPrimary {
        /// Style: Log, Linear, or Video.
        style: GradingPrimaryStyle,
        /// Per-channel brightness (Log) / offset (Linear/Video).
        brightness: [f32; 3],
        /// Per-channel contrast.
        contrast: [f32; 3],
        /// Per-channel gamma (Log/Video).
        gamma: [f32; 3],
        /// Per-channel exposure in stops (Linear).
        exposure: [f32; 3],
        /// Per-channel offset (Linear/Video).
        offset: [f32; 3],
        /// Per-channel slope (Video).
        slope: [f32; 3],
        /// Saturation.
        saturation: f32,
        /// Pivot for contrast.
        pivot: f32,
        /// Pivot black for gamma.
        pivot_black: f32,
        /// Pivot white for gamma.
        pivot_white: f32,
        /// Clamp black.
        clamp_black: f32,
        /// Clamp white.
        clamp_white: f32,
        /// Forward direction.
        forward: bool,
        /// The raw, un-direction-resolved value this op was compiled from
        /// (`style` + `forward` above are the direction it was resolved
        /// FOR; this is the master/per-channel RGBM split
        /// `op::grading_primary_params` consumed to produce every other
        /// field). Kept so `ProcessorOp::inverse()` can re-run
        /// `grading_primary_params` for the OPPOSITE direction instead of
        /// hand-negating/reciprocating the already-resolved fields — one
        /// derivation path, not two that can drift (the same class of fix
        /// already applied to ExposureContrast/Range;
        /// `op::invert_exposure_contrast_params`/`invert_range_params`).
        /// Also lets `accessors.rs`'s introspection-only `Transform`
        /// reconstruction be EXACT instead of a best-effort
        /// identity-master guess.
        raw: crate::dynamic::GradingPrimary,
    },
    /// Grading RGB curves (OCIO parity).
    /// OCIO ref: GradingRGBCurveOpCPU.cpp.
    GradingRgbCurve {
        /// Grading style (Log/Linear/Video).
        style: crate::transform::GradingToneStyle,
        /// The transform's own control points and slopes, kept so `accessors.rs` can rebuild
        /// the exact `Transform` (the fitted knots/coefs below are not invertible back to them).
        raw: Box<crate::transform::GradingRgbCurveTransform>,
        /// The fitted quadratic B-spline knots/coefs (OCIO `KnotsCoefs`) for Red, Green, Blue
        /// and Master. The CPU apply evaluates them directly (`KnotsCoefs::evalCurve` /
        /// `evalCurveRev`, `GradingRGBCurveOpCPU.cpp:30-53`) and the GPU uploads the same data
        /// as uniforms, so both paths evaluate one fit over an unbounded domain.
        curve_data: crate::grading_bspline::CurveUniformData<
            { crate::grading_bspline::RGB_CURVE_COUNT * 2 },
        >,
        /// Bypass Lin-to-Log conversion (only relevant for Linear style).
        bypass_lin_to_log: bool,
        /// Forward direction (for GPU/curve eval parity).
        forward: bool,
    },
    /// Grading tone (OCIO parity with 5 zones + S-contrast).
    ///
    /// Carries the resolved `GradingTone` value plus its precomputed curve tables
    /// (`GradingTonePreRender`), so the per-pixel apply is a direct call into the
    /// faithful OCIO evaluator (no per-pixel re-derivation). OCIO ref:
    /// GradingToneOpCPU.cpp.
    GradingTone {
        /// Processing style (Log/Linear/Video).
        style: crate::transform::GradingToneStyle,
        /// Resolved GradingTone parameter value (RGBM + start/width per zone).
        value: crate::grading_tone::GradingTone,
        /// Precomputed curve knots/slopes (built once at compile time).
        pre_render: Box<crate::grading_tone::GradingTonePreRender>,
        /// Forward direction.
        forward: bool,
    },
    /// Grading hue curve (8 hue-based curves).
    GradingHueCurve {
        /// Control-point metadata (accessors / dynamic props). Evaluation uses `curve_data`.
        /// Boxed: eight control-point curves would otherwise make this the largest variant by far.
        curves: Box<crate::grading_hue_curve::GradingHueCurves>,
        /// Per-curve user slopes in `HueCurveType::ALL` order. Round-tripped by accessors so
        /// processor→transform→processor does not silently drop explicit slopes (CPU and GPU
        /// both honour them via `curve_data`).
        slopes: [Vec<f32>; crate::grading_bspline::HUE_CURVE_COUNT],
        /// RGB->HSY conversion style.
        rgb_to_hsy: crate::transform::HsyTransformStyle,
        /// Fitted B-spline uniforms — the ONE evaluation source for CPU and GPU.
        curve_data: crate::grading_bspline::CurveUniformData<
            { crate::grading_bspline::HUE_CURVE_COUNT * 2 },
        >,
        /// Draw-curve-only mode (UI graphing).
        draw_curve_only: bool,
        forward: bool,
    },
    /// LogAffine transform (OCIO v2 parity).
    /// Forward: out = log_slope * log(base, max(MIN, lin_slope * x + lin_offset)) + log_offset
    /// Inverse: out = (pow(base, (x - log_offset) / log_slope) - lin_offset) / lin_slope
    LogAffine {
        base: f32,
        log_side_slope: [f32; 3],
        log_side_offset: [f32; 3],
        lin_side_slope: [f32; 3],
        lin_side_offset: [f32; 3],
        forward: bool,
    },
    /// LogCamera (OCIO parity) - piecewise log for camera curves (ARRI LogC, Sony S-Log3).
    /// Forward: if x >= lin_break: log formula, else: linear_slope * x + linear_offset
    /// Inverse: if in >= log_break: inverse log, else: (in - linear_offset) / linear_slope
    LogCamera {
        base: f32,
        log_base: f32, // Precomputed: base.ln()
        log_side_slope: [f32; 3],
        log_side_offset: [f32; 3],
        lin_side_slope: [f32; 3],
        lin_side_offset: [f32; 3],
        lin_side_break: [f32; 3],
        linear_slope: [f32; 3],
        // Precomputed values (OCIO: computed in updateData)
        log_side_break: [f32; 3],
        linear_offset: [f32; 3],
        forward: bool,
    },
    /// Exponent with linear segment (sRGB, Rec.709 style): OCIO's moncurve. Its negatives are
    /// LINEAR (`mirror == false`) or MIRROR; CLAMP and PASS_THRU do not exist for it (B24).
    ExponentWithLinear {
        gamma: [f32; 4],
        offset: [f32; 4],
        mirror: bool,
        forward: bool,
    },
}

/// Parse ACES 2.0 primaries from params (8 floats: rx,ry,gx,gy,bx,by,wx,wy starting at offset).
pub(crate) fn parse_aces2_primaries(params: &[f32], offset: usize) -> crate::aces2::Primaries {
    let g = |i: usize| params.get(offset + i).copied().unwrap_or(0.0);
    crate::aces2::Primaries {
        red: crate::aces2::Chromaticity { x: g(0), y: g(1) },
        green: crate::aces2::Chromaticity { x: g(2), y: g(3) },
        blue: crate::aces2::Chromaticity { x: g(4), y: g(5) },
        white: crate::aces2::Chromaticity { x: g(6), y: g(7) },
    }
}

/// Converts ACES 2.0 FixedFunction styles to precomputed ACES2 ProcessorOps.
/// Returns `Ok(None)` for non-ACES2 fixed-function styles, and an error when the config's
/// primaries are degenerate (e.g. collinear), instead of panicking in the matrix derivation.
/// All-zero placeholder primaries still build (they give NaN on purpose; see `aces2.rs`).
pub(crate) fn fixed_function_to_aces2_op(
    style: FixedFunctionStyle,
    params: &[f32],
    forward: bool,
) -> crate::OcioResult<Option<ProcessorOp>> {
    Ok(match style {
        FixedFunctionStyle::AcesOutputTransform20 => {
            let peak = params.first().copied().unwrap_or(1000.0);
            let lp = parse_aces2_primaries(params, 1);
            let state = crate::aces2::init_output_transform_cached(peak, &lp)?;
            Some(ProcessorOp::Aces2OutputTransform { state, forward })
        }
        FixedFunctionStyle::AcesRgbToJmh20 | FixedFunctionStyle::AcesJmhToRgb20 => {
            let p = parse_aces2_primaries(params, 0);
            let jmh_params = crate::aces2::try_init_jmh_params(&p)?;
            let is_rgb_to = style == FixedFunctionStyle::AcesRgbToJmh20;
            Some(ProcessorOp::Aces2RgbJmh {
                params: Box::new(jmh_params),
                forward: if is_rgb_to { forward } else { !forward },
            })
        }
        FixedFunctionStyle::AcesTonescaleCompress20 => {
            let peak = params.first().copied().unwrap_or(1000.0);
            let jmh_p = crate::aces2::init_jmh_params(&crate::aces2::ACES_AP0);
            let ts_p = crate::aces2::init_tonescale_params(peak);
            let reach = crate::aces2::init_jmh_params(&crate::aces2::ACES_AP1);
            let shared = crate::aces2::init_shared_compression_params(peak, &jmh_p, &reach);
            let chroma = crate::aces2::init_chroma_compress_params(peak, &ts_p);
            Some(ProcessorOp::Aces2TonescaleCompress {
                jmh_params: Box::new(jmh_p),
                ts_params: Box::new(ts_p),
                shared: Box::new(shared),
                chroma: Box::new(chroma),
                forward,
            })
        }
        FixedFunctionStyle::AcesGamutCompress20 => {
            let peak = params.first().copied().unwrap_or(1000.0);
            let lp = parse_aces2_primaries(params, 1);
            let p_in = crate::aces2::init_jmh_params(&crate::aces2::ACES_AP0);
            let p_lim = crate::aces2::try_init_jmh_params(&lp)?;
            let ts = crate::aces2::init_tonescale_params(peak);
            let reach = crate::aces2::init_jmh_params(&crate::aces2::ACES_AP1);
            let shared = crate::aces2::init_shared_compression_params(peak, &p_in, &reach);
            let gamut =
                crate::aces2::init_gamut_compress_params(peak, &p_in, &p_lim, &ts, &shared, &reach);
            Some(ProcessorOp::Aces2GamutCompress {
                shared: Box::new(shared),
                gamut: Box::new(gamut),
                forward,
            })
        }
        _ => None,
    })
}

impl ProcessorOp {
    /// The variant's name, for diagnostics — e.g. naming an op the GPU path cannot convert.
    ///
    /// Kept as an explicit match rather than parsed out of `{:?}` so a new variant is a compile
    /// error here, not a silently-blank name in an error message.
    pub fn variant_name(&self) -> &'static str {
        match self {
            ProcessorOp::Matrix { .. } => "Matrix",
            ProcessorOp::Cdl { .. } => "Cdl",
            ProcessorOp::Exponent { .. } => "Exponent",
            ProcessorOp::Log { .. } => "Log",
            ProcessorOp::Range { .. } => "Range",
            ProcessorOp::Lut1d { .. } => "Lut1d",
            ProcessorOp::Lut3d { .. } => "Lut3d",
            ProcessorOp::Transfer { .. } => "Transfer",
            ProcessorOp::ExposureContrast { .. } => "ExposureContrast",
            ProcessorOp::FixedFunction { .. } => "FixedFunction",
            ProcessorOp::Allocation { .. } => "Allocation",
            ProcessorOp::GradingPrimary { .. } => "GradingPrimary",
            ProcessorOp::GradingRgbCurve { .. } => "GradingRgbCurve",
            ProcessorOp::GradingHueCurve { .. } => "GradingHueCurve",
            ProcessorOp::GradingTone { .. } => "GradingTone",
            ProcessorOp::Aces2OutputTransform { .. } => "Aces2OutputTransform",
            ProcessorOp::Aces2RgbJmh { .. } => "Aces2RgbJmh",
            ProcessorOp::Aces2TonescaleCompress { .. } => "Aces2TonescaleCompress",
            ProcessorOp::Aces2GamutCompress { .. } => "Aces2GamutCompress",
            ProcessorOp::LogAffine { .. } => "LogAffine",
            ProcessorOp::LogCamera { .. } => "LogCamera",
            ProcessorOp::ExponentWithLinear { .. } => "ExponentWithLinear",
        }
    }

    /// Returns true if this operation is an identity (no-op).
    pub fn is_identity(&self) -> bool {
        match self {
            ProcessorOp::Matrix { matrix, offset } => {
                // Identity matrix check
                let identity = [
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
                ];
                let is_identity_matrix = matrix
                    .iter()
                    .zip(identity.iter())
                    .all(|(a, b)| (a - b).abs() < 1e-6);
                let is_zero_offset = offset.iter().all(|v| v.abs() < 1e-6);
                is_identity_matrix && is_zero_offset
            }
            // A unit exponent is still not a no-op under `Clamp`, which maps negatives to
            // zero for any exponent. Only the styles that leave negatives alone can be
            // dropped.
            ProcessorOp::Exponent {
                value,
                negative_style,
            } => {
                value.iter().all(|v| (*v - 1.0).abs() < 1e-6)
                    && matches!(
                        negative_style,
                        NegativeStyle::PassThru | NegativeStyle::Mirror
                    )
            }
            ProcessorOp::Cdl {
                slope,
                offset,
                power,
                saturation,
                style,
                ..
            } => {
                // Unit SOP parameters do NOT make an ASC CDL an identity: the v1.2 style
                // clamps to [0,1] before the power and again after saturation, so dropping
                // it here let out-of-range values through that the CDL was there to bound.
                matches!(style, CdlStyle::NoClamp)
                    && slope.iter().all(|v| (*v - 1.0).abs() < 1e-6)
                    && offset.iter().all(|v| v.abs() < 1e-6)
                    && power.iter().all(|v| (*v - 1.0).abs() < 1e-6)
                    && (*saturation - 1.0).abs() < 1e-6
            }
            ProcessorOp::Range {
                scale,
                offset,
                clamp_min,
                clamp_max,
            } => {
                (*scale - 1.0).abs() < 1e-6
                    && offset.abs() < 1e-6
                    && clamp_min.is_none()
                    && clamp_max.is_none()
            }
            // OCIO `ExposureContrastOpData::isIdentity`: a dynamic parameter may leave identity
            // at any time, so the op must survive the optimizer.
            ProcessorOp::ExposureContrast {
                exposure,
                contrast,
                gamma,
                dynamic,
                ..
            } => {
                !dynamic.any()
                    && exposure.abs() < 1e-6
                    && (*contrast - 1.0).abs() < 1e-6
                    && (*gamma - 1.0).abs() < 1e-6
            }
            ProcessorOp::GradingPrimary {
                brightness,
                contrast,
                gamma,
                exposure,
                offset,
                slope,
                saturation,
                ..
            } => {
                brightness.iter().all(|v| v.abs() < 1e-6)
                    && contrast.iter().all(|v| (*v - 1.0).abs() < 1e-6)
                    && gamma.iter().all(|v| (*v - 1.0).abs() < 1e-6)
                    && exposure.iter().all(|v| v.abs() < 1e-6)
                    && offset.iter().all(|v| v.abs() < 1e-6)
                    && slope.iter().all(|v| (*v - 1.0).abs() < 1e-6)
                    && (*saturation - 1.0).abs() < 1e-6
            }
            // LUTs, Transfer, Log, etc. are generally not identity
            _ => false,
        }
    }

    /// Creates the inverse of this operation, as a **sequence**.
    ///
    /// Most ops invert to exactly one op, but not all can: a CDL's inverse runs its
    /// stages in the opposite order and needs several (see [`cdl_inverse_ops`]).
    /// Returning `Vec` rather than `Self` is what lets those be correct instead of
    /// approximated by a same-shaped op with reciprocal parameters.
    ///
    /// Returns an error if the operation cannot be inverted.
    pub fn inverse(&self) -> crate::OcioResult<Vec<Self>> {
        match self {
            ProcessorOp::Cdl {
                slope,
                offset,
                power,
                saturation,
                style,
                inverse,
            } => Ok(vec![ProcessorOp::cdl(
                *slope,
                *offset,
                *power,
                *saturation,
                *style,
                inverse.is_some(),
            )]),
            other => Ok(vec![other.inverse_one()?]),
        }
    }

    /// A CDL op (forward parameters) in `forward` or inverse direction.
    pub(crate) fn cdl(
        slope: [f32; 3],
        offset: [f32; 3],
        power: [f32; 3],
        saturation: f32,
        style: CdlStyle,
        forward: bool,
    ) -> Self {
        let inverse = (!forward)
            .then(|| std::sync::Arc::new(cdl_inverse_ops(slope, offset, power, saturation, style)));
        ProcessorOp::Cdl {
            slope,
            offset,
            power,
            saturation,
            style,
            inverse,
        }
    }

    /// Inverse of the ops whose inverse is a single op.
    fn inverse_one(&self) -> crate::OcioResult<Self> {
        use crate::OcioError;

        match self {
            ProcessorOp::Matrix { matrix, offset } => {
                let (inv, inv_off) =
                    invert_matrix_offset(&matrix.map(|v| v as f64), &offset.map(|v| v as f64))
                        .ok_or_else(|| OcioError::InvalidTransform {
                            reason: "Matrix is singular and cannot be inverted".into(),
                        })?;
                let (inv, inv_off) = (inv.map(|v| v as f32), inv_off.map(|v| v as f32));
                Ok(ProcessorOp::Matrix {
                    matrix: inv,
                    offset: inv_off,
                })
            }
            ProcessorOp::Exponent {
                value,
                negative_style,
            } => Ok(ProcessorOp::Exponent {
                value: [
                    1.0 / value[0],
                    1.0 / value[1],
                    1.0 / value[2],
                    1.0 / value[3],
                ],
                negative_style: *negative_style,
            }),
            ProcessorOp::Transfer { style, forward } => Ok(ProcessorOp::Transfer {
                style: *style,
                forward: !forward,
            }),
            ProcessorOp::Log { base, forward } => Ok(ProcessorOp::Log {
                base: *base,
                forward: !forward,
            }),
            ProcessorOp::LogAffine {
                base,
                log_side_slope,
                log_side_offset,
                lin_side_slope,
                lin_side_offset,
                forward,
            } => Ok(ProcessorOp::LogAffine {
                base: *base,
                log_side_slope: *log_side_slope,
                log_side_offset: *log_side_offset,
                lin_side_slope: *lin_side_slope,
                lin_side_offset: *lin_side_offset,
                forward: !forward,
            }),
            ProcessorOp::LogCamera {
                base,
                log_base,
                log_side_slope,
                log_side_offset,
                lin_side_slope,
                lin_side_offset,
                lin_side_break,
                linear_slope,
                log_side_break,
                linear_offset,
                forward,
            } => Ok(ProcessorOp::LogCamera {
                base: *base,
                log_base: *log_base,
                log_side_slope: *log_side_slope,
                log_side_offset: *log_side_offset,
                lin_side_slope: *lin_side_slope,
                lin_side_offset: *lin_side_offset,
                lin_side_break: *lin_side_break,
                linear_slope: *linear_slope,
                log_side_break: *log_side_break,
                linear_offset: *linear_offset,
                forward: !forward,
            }),
            // Handled by `inverse`, which can return the required op sequence.
            ProcessorOp::Cdl { .. } => Err(OcioError::InvalidTransform {
                reason: "CDL inverse is a sequence; call ProcessorOp::inverse".into(),
            }),
            ProcessorOp::Range {
                scale,
                offset,
                clamp_min,
                clamp_max,
            } => {
                // Shares `invert_range_params` with `processor/compile.rs`'s
                // `Transform::Range` arm instead of re-deriving the same
                // remap independently (see that function's doc comment for
                // the derivation) — the same duplication shape that let
                // this arm carry `clamp_min`/`clamp_max` over UNCHANGED
                // before it was first fixed.
                let (scale, offset, clamp_min, clamp_max) =
                    invert_range_params(*scale, *offset, *clamp_min, *clamp_max);
                Ok(ProcessorOp::Range {
                    scale,
                    offset,
                    clamp_min,
                    clamp_max,
                })
            }
            // Both LUT arms previously returned the LUT unchanged, so `inverse()` on any
            // processor containing one produced a chain that silently applied the
            // FORWARD LUT where its inverse belonged — plausible-looking wrong pixels
            // with no diagnostic. Inversion at this level would need the LUT's domain
            // and a monotonicity check (see `math::invert_lut1d`), which the compile
            // path has and this one does not; refusing is the only honest answer.
            ProcessorOp::Lut1d { .. } => Err(OcioError::InvalidTransform {
                reason: "1D LUT cannot be inverted in place; build the inverse at compile \
                         time, where the LUT domain is known"
                    .into(),
            }),
            ProcessorOp::Lut3d { .. } => Err(OcioError::InvalidTransform {
                reason: "3D LUT has no analytic inverse; build the inverse at compile time".into(),
            }),
            ProcessorOp::FixedFunction {
                style,
                params,
                forward,
            } => {
                let (style, forward) = resolve_fixed_function_direction(*style, !forward);
                Ok(ProcessorOp::FixedFunction {
                    style,
                    params: params.clone(),
                    forward,
                })
            }
            ProcessorOp::ExposureContrast {
                exposure,
                contrast,
                gamma,
                pivot,
                log_exposure_step,
                log_mid_gray,
                style,
                dynamic,
                forward,
            } => Ok(ProcessorOp::ExposureContrast {
                exposure: *exposure,
                contrast: *contrast,
                gamma: *gamma,
                pivot: *pivot,
                log_exposure_step: *log_exposure_step,
                log_mid_gray: *log_mid_gray,
                style: *style,
                dynamic: *dynamic,
                forward: !forward,
            }),
            ProcessorOp::GradingPrimary {
                style,
                forward,
                raw,
                ..
            } => {
                // Found while auditing this arm for the GradingPrimary live-path fix
                // (`docs/ROADMAP.md` BUG11 item 1): the old comment here ("parameters
                // carried through UNCHANGED... apply path already implements the
                // reverse direction from the forward parameters") described a real
                // fact about the OLD `ProcessorOp::GradingPrimary`, where these fields
                // were RAW/unresolved and `apply.rs` derived both directions from the
                // SAME numbers on the fly. Since `op::grading_primary_params` started
                // precomputing DIRECTION-RESOLVED values at compile time (master
                // folded in, negated/reciprocated for the direction the op was built
                // for), that fact stopped being true.
                //
                // The FIRST fix here (hand-negating additive fields and reciprocating
                // multiplicative ones) was itself the same duplication mistake this
                // sweep already fixed twice for ExposureContrast and Range: a SECOND,
                // independent derivation of "what the opposite direction's resolved
                // values look like", parallel to `grading_primary_params` but not
                // calling it — free to silently drift from the reference the moment
                // either derivation changes and the other doesn't. It also already
                // had one small, real divergence: `grading_primary_params`'s LOG/LINEAR
                // inverse branch guards `contrast` against a zero master*channel
                // product (`inv(x) = 1.0 / if x == 0.0 { 1.0 } else { x }`, avoiding an
                // infinite/NaN result), but a bare `recip(forward_contrast)` has no such
                // guard — degenerate `contrast = 0` configs inverted differently
                // depending on which path built the op. (Checked VIDEO's `slope`
                // algebraically before assuming it had the same kind of gap: it
                // doesn't — `slope_inverse = slope_den/(pw-pb)` IS exactly
                // `1/slope_forward` since `slope_forward = (pw-pb)/slope_den`, so the
                // hand-`recip()` was numerically fine there, just still duplicated.)
                // Fixed for real by keeping the RAW `GradingPrimary` value (`raw`,
                // never direction-resolved) on the op and re-running
                // `grading_primary_params` for the FLIPPED direction — one derivation
                // path, used identically at compile time and here.
                let new_forward = !forward;
                let p = grading_primary_params(*style, new_forward, raw);
                Ok(ProcessorOp::GradingPrimary {
                    style: *style,
                    brightness: p.brightness,
                    contrast: p.contrast,
                    gamma: p.gamma,
                    exposure: p.exposure,
                    offset: p.offset,
                    slope: p.slope,
                    saturation: raw.saturation as f32,
                    pivot: p.pivot,
                    pivot_black: raw.pivot_black as f32,
                    pivot_white: raw.pivot_white as f32,
                    clamp_black: raw.clamp_black as f32,
                    clamp_white: raw.clamp_white as f32,
                    forward: new_forward,
                    raw: raw.clone(),
                })
            }
            ProcessorOp::GradingTone { .. } => {
                // GradingTone inverse is complex - for now just flip forward flag
                let mut inv = self.clone();
                if let ProcessorOp::GradingTone { forward, .. } = &mut inv {
                    *forward = !*forward;
                }
                Ok(inv)
            }
            ProcessorOp::GradingRgbCurve { .. } => {
                // The fitted curves are evaluated in closed form in both directions
                // (`grading_bspline::eval_curve_fwd` / `eval_curve_rev`), so the inverse is the
                // same fit with the direction flipped — exactly what OCIO's
                // `GradingRGBCurveRevOpCPU` does with the same `KnotsCoefs`.
                let mut inv = self.clone();
                if let ProcessorOp::GradingRgbCurve { forward, .. } = &mut inv {
                    *forward = !*forward;
                }
                Ok(inv)
            }
            ProcessorOp::GradingHueCurve { .. } => {
                // Curve inversions require numerical methods - not supported
                Err(OcioError::InvalidTransform {
                    reason: "Curve operations cannot be analytically inverted".into(),
                })
            }
            ProcessorOp::Allocation {
                allocation,
                vars,
                forward,
            } => Ok(ProcessorOp::Allocation {
                allocation: *allocation,
                vars: vars.clone(),
                forward: !forward,
            }),
            ProcessorOp::ExponentWithLinear {
                gamma,
                offset,
                mirror,
                forward,
            } => Ok(ProcessorOp::ExponentWithLinear {
                gamma: *gamma,
                offset: *offset,
                mirror: *mirror,
                forward: !forward,
            }),
            // ACES2 ops - just flip forward flag
            ProcessorOp::Aces2OutputTransform { state, forward } => {
                Ok(ProcessorOp::Aces2OutputTransform {
                    state: state.clone(),
                    forward: !forward,
                })
            }
            ProcessorOp::Aces2RgbJmh { params, forward } => Ok(ProcessorOp::Aces2RgbJmh {
                params: params.clone(),
                forward: !forward,
            }),
            ProcessorOp::Aces2TonescaleCompress {
                jmh_params,
                ts_params,
                shared,
                chroma,
                forward,
            } => Ok(ProcessorOp::Aces2TonescaleCompress {
                jmh_params: jmh_params.clone(),
                ts_params: ts_params.clone(),
                shared: shared.clone(),
                chroma: chroma.clone(),
                forward: !forward,
            }),
            ProcessorOp::Aces2GamutCompress {
                shared,
                gamut,
                forward,
            } => Ok(ProcessorOp::Aces2GamutCompress {
                shared: shared.clone(),
                gamut: gamut.clone(),
                forward: !forward,
            }),
        }
    }
}

#[cfg(test)]
mod cdl_inverse_ops_tests {
    //! `cdl_inverse_ops` had no numeric test exercising non-identity power
    //! AND saturation TOGETHER — every existing test used one or the other
    //! alone, which can't catch an inversion that gets the STAGE ORDER
    //! wrong (see this function's own doc comment: naive reciprocal
    //! parameters fed through the forward pipeline only happens to agree
    //! with a correct reverse when power==1 and saturation==1, exactly the
    //! cases that were covered). Round-trips a forward CDL op through
    //! `cdl_inverse_ops` and separately cross-checks the same inverse
    //! against `vfx_lut::cdl::apply_cdl_inverse_{clamped,unclamped}`
    //! (verified against `CDLOpCPU.cpp`'s `CDLRendererRev`), for both
    //! `CDL_V1_2` (clamping) and `NO_CLAMP` styles.
    use super::*;
    use crate::processor::Processor;

    fn forward(
        pixel: [f32; 3],
        slope: [f32; 3],
        offset: [f32; 3],
        power: [f32; 3],
        saturation: f32,
        style: CdlStyle,
    ) -> [f32; 3] {
        let proc = Processor::from_ops(vec![ProcessorOp::Cdl {
            slope,
            offset,
            power,
            saturation,
            style,
            inverse: None,
        }]);
        let mut pixels = [pixel];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    fn inverse_via_ops(
        pixel: [f32; 3],
        slope: [f32; 3],
        offset: [f32; 3],
        power: [f32; 3],
        saturation: f32,
        style: CdlStyle,
    ) -> [f32; 3] {
        let proc = Processor::from_ops(cdl_inverse_ops(slope, offset, power, saturation, style));
        let mut pixels = [pixel];
        proc.apply_rgb(&mut pixels);
        pixels[0]
    }

    /// Slope/offset chosen to keep the forward output well inside `(0, 1)`
    /// for the clamped case (so the round-trip isn't trivially exercising a
    /// clamp boundary), and power/saturation both far from identity so an
    /// order-swapped or naive-reciprocal inversion would visibly diverge.
    const SLOPE: [f32; 3] = [0.9, 1.1, 0.8];
    const OFFSET: [f32; 3] = [0.05, -0.03, 0.02];
    const POWER: [f32; 3] = [1.6, 0.7, 1.3];
    const SATURATION: f32 = 0.6;
    const PIXELS: [[f32; 3]; 3] = [[0.5, 0.4, 0.6], [0.2, 0.8, 0.35], [0.65, 0.1, 0.5]];

    #[test]
    fn round_trips_forward_then_inverse_ascc_dl_with_power_and_saturation() {
        for original in PIXELS {
            let fwd = forward(original, SLOPE, OFFSET, POWER, SATURATION, CdlStyle::AscCdl);
            let back = inverse_via_ops(fwd, SLOPE, OFFSET, POWER, SATURATION, CdlStyle::AscCdl);
            for i in 0..3 {
                assert!(
                    (back[i] - original[i]).abs() < 1e-4,
                    "AscCdl round-trip diverged at channel {i}: {:?} -> {:?} -> {:?}",
                    original,
                    fwd,
                    back
                );
            }
        }
    }

    #[test]
    fn round_trips_forward_then_inverse_no_clamp_with_power_and_saturation() {
        // NoClamp allows values outside [0,1] to pass through the whole
        // pipeline, so pick a pixel that stays positive post-slope/offset
        // (ApplyPower<false> only agrees with powf for non-negative input;
        // that "negative passes through unchanged" branch is exercised by
        // COLOR-13's own tests, not this round-trip's job).
        let original = [0.5_f32, 1.3, 0.9];
        let fwd = forward(
            original,
            SLOPE,
            OFFSET,
            POWER,
            SATURATION,
            CdlStyle::NoClamp,
        );
        let back = inverse_via_ops(fwd, SLOPE, OFFSET, POWER, SATURATION, CdlStyle::NoClamp);
        for i in 0..3 {
            assert!(
                (back[i] - original[i]).abs() < 1e-4,
                "NoClamp round-trip diverged at channel {i}: {:?} -> {:?} -> {:?}",
                original,
                fwd,
                back
            );
        }
    }

    /// `cdl_inverse_ops`'s `ProcessorOp::Exponent` uses the exact `f32::powf`;
    /// `vfx_lut::cdl`'s inverse helpers use `fast_pow` (a deliberate
    /// exp2/log2 approximation, matching OCIO's own SSE fast-path). The two
    /// are expected to diverge by a small but non-ULP amount after a power
    /// stage — this tolerance is set from the largest observed divergence
    /// at these non-identity power/saturation parameters (~1.4e-5), with
    /// margin, not tightened to `1e-5`/`1e-6` as if these were the same
    /// pow implementation.
    const CROSS_CHECK_TOLERANCE: f32 = 2e-4;

    #[test]
    fn cross_checks_against_vfx_lut_cdl_inverse_ascc_dl() {
        for original in PIXELS {
            let fwd = forward(original, SLOPE, OFFSET, POWER, SATURATION, CdlStyle::AscCdl);

            let via_ops = inverse_via_ops(fwd, SLOPE, OFFSET, POWER, SATURATION, CdlStyle::AscCdl);

            let mut via_vfx_lut = fwd;
            vfx_lut::cdl::apply_cdl_inverse_clamped(
                &mut via_vfx_lut,
                SLOPE,
                OFFSET,
                POWER,
                SATURATION,
            );

            for i in 0..3 {
                assert!(
                    (via_ops[i] - via_vfx_lut[i]).abs() < CROSS_CHECK_TOLERANCE,
                    "cdl_inverse_ops vs vfx_lut::apply_cdl_inverse_clamped diverged at channel \
                     {i}: {:?} vs {:?} (forward output {:?})",
                    via_ops,
                    via_vfx_lut,
                    fwd
                );
            }
        }
    }

    #[test]
    fn cross_checks_against_vfx_lut_cdl_inverse_no_clamp() {
        let original = [0.5_f32, 1.3, 0.9];
        let fwd = forward(
            original,
            SLOPE,
            OFFSET,
            POWER,
            SATURATION,
            CdlStyle::NoClamp,
        );

        let via_ops = inverse_via_ops(fwd, SLOPE, OFFSET, POWER, SATURATION, CdlStyle::NoClamp);

        let mut via_vfx_lut = fwd;
        vfx_lut::cdl::apply_cdl_inverse_unclamped(
            &mut via_vfx_lut,
            SLOPE,
            OFFSET,
            POWER,
            SATURATION,
        );

        for i in 0..3 {
            assert!(
                (via_ops[i] - via_vfx_lut[i]).abs() < CROSS_CHECK_TOLERANCE,
                "cdl_inverse_ops vs vfx_lut::apply_cdl_inverse_unclamped diverged at channel \
                 {i}: {:?} vs {:?} (forward output {:?})",
                via_ops,
                via_vfx_lut,
                fwd
            );
        }
    }
}

#[cfg(test)]
mod exposure_contrast_inverse_tests {
    //! P0-c / OCIO-02 residual: `ProcessorOp::inverse()`'s ExposureContrast
    //! arm (reached via `Processor::inverse()`, `processor.rs:567`) still
    //! just negated `exposure`, correct only when `contrast * gamma == 1`.
    //! `processor/compile.rs`'s `Transform::ExposureContrast` arm — a
    //! SEPARATE code path building the same op at config-compile time — had
    //! already been fixed to pre-divide exposure by the effective contrast
    //! before negating; this copy had not. Non-identity contrast AND gamma
    //! together are required to catch it: either alone leaves
    //! `effective_contrast == other factor`, which can still coincidentally
    //! round-trip through the wrong formula in degenerate cases.
    use super::*;
    use crate::processor::Processor;

    /// The op keeps the FORWARD parameters in both directions (so a dynamic value and a shader
    /// uniform mean the same thing either way); inverting flips only the direction.
    #[test]
    fn inverse_flips_direction_and_keeps_forward_parameters() {
        let op = ProcessorOp::ExposureContrast {
            exposure: 1.5,
            contrast: 2.0,
            gamma: 1.2,
            pivot: 0.5,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            style: ExposureContrastStyle::Linear,
            dynamic: EcDynamic::NONE,
            forward: true,
        };
        let inv_ops = op.inverse().expect("EC always invertible");
        assert_eq!(inv_ops.len(), 1, "EC inverts to exactly one op");
        let ProcessorOp::ExposureContrast {
            exposure,
            contrast,
            gamma,
            forward,
            ..
        } = &inv_ops[0]
        else {
            panic!("inverse of ExposureContrast must be ExposureContrast");
        };
        assert_eq!(
            (*exposure, *contrast, *gamma, *forward),
            (1.5, 2.0, 1.2, false)
        );
    }
    #[test]
    fn round_trips_through_processor_inverse_with_nonunit_contrast_and_gamma() {
        let op = ProcessorOp::ExposureContrast {
            exposure: 1.5,
            contrast: 2.0,
            gamma: 1.2,
            pivot: 0.5,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            style: ExposureContrastStyle::Linear,
            dynamic: EcDynamic::NONE,
            forward: true,
        };
        let forward = Processor::from_ops(vec![op]);
        let inverse = forward.inverse().expect("EC always invertible");

        for original in [[0.1_f32, 0.1, 0.1], [0.3, 0.3, 0.3], [0.6, 0.6, 0.6]] {
            let mut pixels = [original];
            forward.apply_rgb(&mut pixels);
            inverse.apply_rgb(&mut pixels);
            for c in 0..3 {
                assert!(
                    (pixels[0][c] - original[c]).abs() < 1e-3,
                    "ExposureContrast round-trip diverged at {original:?} channel {c}: got {:?}",
                    pixels[0]
                );
            }
        }
    }
}

#[cfg(test)]
mod range_inverse_tests {
    //! Found sweeping for the same "duplicated/wrong analytic inverse" bug
    //! class as the ExposureContrast fix. `ProcessorOp::Range::inverse()`
    //! carried `clamp_min`/`clamp_max` over UNCHANGED, but those bounds are
    //! OUTPUT bounds of `out = clamp(in*scale+offset, clamp_min,
    //! clamp_max)` — the inverse's own output bounds must be the ORIGINAL
    //! mapping's INPUT bounds, not a copy of the old output bounds. Proven
    //! with `scale=2.0, offset=0.1, clamp=[0.5,3.0]`: correctly-inverted
    //! clamp bounds are `[0.2, 1.45]` (each old bound run through the
    //! inverted affine map); the old code kept `[0.5, 3.0]`.
    use super::*;

    #[test]
    fn inverse_remaps_clamp_bounds_not_just_scale_and_offset() {
        let op = ProcessorOp::Range {
            scale: 2.0,
            offset: 0.1,
            clamp_min: Some(0.5),
            clamp_max: Some(3.0),
        };
        let inv_ops = op.inverse().expect("Range always invertible");
        assert_eq!(inv_ops.len(), 1);
        let ProcessorOp::Range {
            scale,
            offset,
            clamp_min,
            clamp_max,
        } = &inv_ops[0]
        else {
            panic!("inverse of Range must be Range");
        };
        assert!((scale - 0.5).abs() < 1e-6);
        assert!((offset - (-0.05)).abs() < 1e-6);
        assert!(
            (clamp_min.unwrap() - 0.2).abs() < 1e-5,
            "clamp_min should remap to (0.5*0.5 - 0.05) = 0.2, got {:?}",
            clamp_min
        );
        assert!(
            (clamp_max.unwrap() - 1.45).abs() < 1e-5,
            "clamp_max should remap to (3.0*0.5 - 0.05) = 1.45, got {:?}",
            clamp_max
        );

        // Numerically: feed a value whose correct inverse falls BETWEEN the
        // correct clamp window [0.2, 1.45] and the old, wrong window
        // [0.5, 3.0] — the two clamp policies disagree only in that gap.
        // y=0.7 -> t = 0.7*0.5 - 0.05 = 0.3: within [0.2,1.45] (no clip)
        // but below the old [0.5,3.0] window's floor (would clip to 0.5).
        let proc = crate::processor::Processor::from_ops(inv_ops);
        let mut pixels = [[0.7_f32, 0.7, 0.7]];
        proc.apply_rgb(&mut pixels);
        assert!(
            (pixels[0][0] - 0.3).abs() < 1e-5,
            "expected 0.3 (unclipped by the correct [0.2,1.45] window), got {:?} \
             (0.5 would mean the old wrong [0.5,3.0] window is still in effect)",
            pixels[0]
        );
    }

    /// After the review-requested dedup (`invert_range_params` shared with
    /// `processor/compile.rs`'s `Transform::Range` arm): confirms the
    /// compile-time path (a `RangeTransform` compiled with `direction:
    /// Inverse`) agrees with the op-level path (compiling forward, then
    /// calling `ProcessorOp::inverse()`) on an asymmetric, clamped range —
    /// not just that each is internally consistent.
    #[test]
    fn compile_time_inverse_matches_op_level_inverse() {
        use crate::transform::{RangeStyle, RangeTransform, Transform, TransformDirection};

        let rt_fwd = Transform::Range(RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(2.0),
            min_out: Some(0.5),
            max_out: Some(3.0),
            style: RangeStyle::Clamp,
            direction: TransformDirection::Forward,
        });
        let rt_inv = Transform::Range(RangeTransform {
            name: String::new(),
            min_in: Some(0.0),
            max_in: Some(2.0),
            min_out: Some(0.5),
            max_out: Some(3.0),
            style: RangeStyle::Clamp,
            direction: TransformDirection::Inverse,
        });

        let via_compile_inverse =
            crate::processor::Processor::from_transform(&rt_inv, TransformDirection::Forward)
                .expect("compile-time inverse");

        let forward =
            crate::processor::Processor::from_transform(&rt_fwd, TransformDirection::Forward)
                .expect("forward");
        let via_op_inverse = forward.inverse().expect("op-level inverse");

        for input in [[0.4_f32, 0.4, 0.4], [1.0, 1.0, 1.0], [2.9, 2.9, 2.9]] {
            let mut a = [input];
            let mut b = [input];
            via_compile_inverse.apply_rgb(&mut a);
            via_op_inverse.apply_rgb(&mut b);
            for c in 0..3 {
                assert!(
                    (a[0][c] - b[0][c]).abs() < 1e-5,
                    "compile-time vs op-level Range inverse disagree at {input:?} \
                     channel {c}: {:?} vs {:?}",
                    a[0],
                    b[0]
                );
            }
        }
    }
}
