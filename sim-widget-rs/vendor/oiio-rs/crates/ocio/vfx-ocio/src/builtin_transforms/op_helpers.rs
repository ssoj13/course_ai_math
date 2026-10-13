//! The op constructors the builtins are written in — ports of OCIO's `CreateMatrixOp`,
//! `CreateScaleOp`, `CreateRangeOp`, `CreateLogOp`, `CreateGammaOp`, `CreateFixedFunctionOp`,
//! `CreateLut` / `CreateHalfLut` (`transforms/builtins/OpHelpers.cpp`) and friends.
//!
//! Each returns the crate's own [`Transform`] for that op, so a builtin compiles through exactly
//! the same code path as the equivalent transform in a config. The previous implementation
//! compiled builtins through a private `compile_builtin` that re-implemented matrix inversion,
//! the LogCamera derivation, gamma/moncurve selection and a B-spline evaluator of its own; every
//! one of those was a second copy that could (and did) drift from the transform it imitated.

use super::color_matrix::M44;
use crate::transform::{
    ExponentTransform, ExponentWithLinearTransform, FixedFunctionStyle, FixedFunctionTransform,
    GradingRgbCurveTransform, GradingToneStyle, Interpolation, LogAffineTransform,
    LogCameraTransform, LogTransform, Lut1DHueAdjust, Lut1DTransform, MatrixTransform,
    NegativeStyle, RangeStyle, RangeTransform, Transform, TransformDirection,
};

const FWD: TransformDirection = TransformDirection::Forward;
const INV: TransformDirection = TransformDirection::Inverse;

/// `CreateMatrixOp(ops, matrix, dir)`.
pub fn matrix(m: M44) -> Transform {
    matrix_dir(m, FWD)
}

/// `CreateMatrixOp(ops, matrix, dir)` with an explicit direction.
pub fn matrix_dir(m: M44, direction: TransformDirection) -> Transform {
    Transform::Matrix(MatrixTransform {
        name: String::new(),
        matrix: m,
        offset: [0.0; 4],
        direction,
    })
}

/// `CreateScaleOp(ops, {s, s, s, 1}, FORWARD)`.
pub fn scale(s: f64) -> Transform {
    scale_offset(s, 0.0)
}

/// `CreateScaleOffsetOp(ops, {s, s, s, 1}, {o, o, o, 0}, FORWARD)`.
pub fn scale_offset(s: f64, o: f64) -> Transform {
    let mut m = super::color_matrix::IDENTITY;
    m[0] = s;
    m[5] = s;
    m[10] = s;
    Transform::Matrix(MatrixTransform {
        name: String::new(),
        matrix: m,
        offset: [o, o, o, 0.0],
        direction: FWD,
    })
}

/// `CreateRangeOp(ops, minIn, maxIn, minOut, maxOut, FORWARD)`; `None` is OCIO's `EmptyValue()`.
pub fn range(
    min_in: Option<f64>,
    max_in: Option<f64>,
    min_out: Option<f64>,
    max_out: Option<f64>,
) -> Transform {
    Transform::Range(RangeTransform {
        name: String::new(),
        min_in,
        max_in,
        min_out,
        max_out,
        style: RangeStyle::Clamp,
        direction: FWD,
    })
}

/// `CreateLogOp(ops, base, dir)`: forward is `log_base(x)`, inverse `base^x`.
pub fn log(base: f64, direction: TransformDirection) -> Transform {
    Transform::Log(LogTransform {
        name: String::new(),
        base,
        direction,
    })
}

/// The five/six `LogOpData::Params` of a camera log, in OCIO's order:
/// `{logSideSlope, logSideOffset, linSideSlope, linSideOffset[, linSideBreak[, linearSlope]]}`.
pub struct LogParams {
    pub log_side_slope: f64,
    pub log_side_offset: f64,
    pub lin_side_slope: f64,
    pub lin_side_offset: f64,
    pub lin_side_break: Option<f64>,
    pub linear_slope: Option<f64>,
}

/// `CreateLogOp(ops, LogOpData(base, params, params, params, TRANSFORM_DIR_INVERSE), FORWARD)`:
/// the camera "to linear" curves are all a log op held in its inverse (decode) direction.
///
/// With a `lin_side_break` it is a camera log (linear toe below the break; OCIO derives the
/// linear slope from continuity unless one is given, `LogUtils::GetLinearSlope`); without one it
/// is a plain log-affine curve.
pub fn log_to_linear(base: f64, p: &LogParams) -> Transform {
    let v = |x: f64| [x; 3];
    match p.lin_side_break {
        Some(brk) => Transform::LogCamera(LogCameraTransform {
            name: String::new(),
            base,
            log_side_slope: v(p.log_side_slope),
            log_side_offset: v(p.log_side_offset),
            lin_side_slope: v(p.lin_side_slope),
            lin_side_offset: v(p.lin_side_offset),
            lin_side_break: v(brk),
            linear_slope: p.linear_slope.map(v),
            direction: INV,
        }),
        None => Transform::LogAffine(LogAffineTransform {
            name: String::new(),
            base,
            log_side_slope: v(p.log_side_slope),
            log_side_offset: v(p.log_side_offset),
            lin_side_slope: v(p.lin_side_slope),
            lin_side_offset: v(p.lin_side_offset),
            direction: INV,
        }),
    }
}

/// OCIO `GammaOpData` styles the builtins use, all applied forward. Every one is a `*_REV`
/// (inverse) style; the variant names drop that shared suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GammaRevStyle {
    /// `BASIC_REV`: `max(x, 0)^(1/g)`.
    Basic,
    /// `BASIC_MIRROR_REV`: `sign(x) * |x|^(1/g)`.
    BasicMirror,
    /// `MONCURVE_REV`: the inverse moncurve, linear segment for everything below the break.
    Moncurve,
    /// `MONCURVE_MIRROR_REV`.
    MoncurveMirror,
}

/// `CreateGammaOp(ops, GammaOpData(style, {g[, offset]} x3, alpha {1[, 0]}), FORWARD)`.
///
/// The `*_REV` styles are the reverse of the forward gamma, so they are the corresponding
/// transform in its inverse direction: `ExponentTransform` for BASIC (negative style CLAMP or
/// MIRROR), `ExponentWithLinearTransform` for MONCURVE (negative style LINEAR or MIRROR).
pub fn gamma(style: GammaRevStyle, g: f64, offset: f64) -> Transform {
    let value = [g, g, g, 1.0];
    match style {
        GammaRevStyle::Basic | GammaRevStyle::BasicMirror => {
            Transform::Exponent(ExponentTransform {
                name: String::new(),
                value,
                negative_style: if style == GammaRevStyle::Basic {
                    NegativeStyle::Clamp
                } else {
                    NegativeStyle::Mirror
                },
                direction: INV,
            })
        }
        GammaRevStyle::Moncurve | GammaRevStyle::MoncurveMirror => {
            Transform::ExponentWithLinear(ExponentWithLinearTransform {
                name: String::new(),
                gamma: value,
                offset: [offset, offset, offset, 0.0],
                negative_style: if style == GammaRevStyle::Moncurve {
                    NegativeStyle::Linear
                } else {
                    NegativeStyle::Mirror
                },
                direction: INV,
            })
        }
    }
}

/// `CreateFixedFunctionOp(ops, style, params)` (always forward in the builtins).
pub fn fixed_function(style: FixedFunctionStyle, params: Vec<f64>) -> Transform {
    Transform::FixedFunction(FixedFunctionTransform {
        name: String::new(),
        style,
        params,
        direction: FWD,
    })
}

/// `CreateLut(ops, dim, generator)` (`OpHelpers.cpp:55-75`): a standard Lut1D over `[0, 1]`,
/// linear interpolation, one entry per `idx / (dim - 1)`, values narrowed to f32 by the
/// generator (as OCIO's `float(double)` generator does).
pub fn lut(dim: usize, generator: impl Fn(f64) -> f32) -> Transform {
    let values: Vec<f32> = (0..dim)
        .map(|i| generator(i as f64 / (dim - 1) as f64))
        .collect();
    Transform::Lut1D(Lut1DTransform {
        interpolation: Interpolation::Linear,
        hue_adjust: Lut1DHueAdjust::None,
        ..Lut1DTransform::from_data(values)
    })
}

/// `CreateHalfLut(ops, generator)` (`OpHelpers.cpp:108-139`): a half-domain Lut1D, one entry per
/// 16-bit half code. NaN codes take `generator(0)` and the infinities `generator(+/-HALF_MAX)`.
pub fn half_lut(generator: impl Fn(f64) -> f32) -> Transform {
    let values: Vec<f32> = (0..=u16::MAX)
        .map(|bits| {
            let h = half::f16::from_bits(bits);
            let v = if h.is_nan() {
                0.0
            } else if h.is_infinite() {
                if h.is_sign_negative() {
                    -f64::from(half::f16::MAX)
                } else {
                    f64::from(half::f16::MAX)
                }
            } else {
                h.to_f64()
            };
            generator(v)
        })
        .collect();
    Transform::Lut1D(Lut1DTransform {
        half_domain: true,
        interpolation: Interpolation::Linear,
        hue_adjust: Lut1DHueAdjust::None,
        ..Lut1DTransform::from_data(values)
    })
}

/// `Interpolate1D` (`OpHelpers.cpp:16-53`): piecewise-linear lookup in `(in, out)` pairs,
/// clamped to the end values outside the table.
pub fn interpolate_1d(table: &[[f64; 2]], x: f64) -> f64 {
    let n = table.len();
    if x < table[0][0] {
        return table[0][1];
    }
    if x >= table[n - 1][0] {
        return table[n - 1][1];
    }
    let i = table
        .iter()
        .position(|p| x < p[0])
        .expect("bracketed above");
    let (lo, hi) = (table[i - 1], table[i]);
    let t = (x - lo[0]) / (hi[0] - lo[0]);
    lo[1] * (1.0 - t) + hi[1] * t
}

/// A `GradingRGBCurve` in GRADING_LOG style whose R/G/B curves are identity and whose master
/// curve is `points` with explicit `slopes` — the shape of every ACES 1.x tone-curve op
/// (`ACES.cpp` `Generate_tonecurve_ops` / `Generate_hdr_tonecurve_ops`).
pub fn master_curve(points: &[[f32; 2]], slopes: &[f32]) -> Transform {
    let identity = vec![[0.0, 0.0], [1.0, 1.0]];
    Transform::GradingRgbCurve(GradingRgbCurveTransform {
        name: String::new(),
        style: GradingToneStyle::Log,
        red: identity.clone(),
        green: identity.clone(),
        blue: identity,
        master: points.iter().map(|p| [p[0] as f64, p[1] as f64]).collect(),
        red_slopes: Vec::new(),
        green_slopes: Vec::new(),
        blue_slopes: Vec::new(),
        master_slopes: slopes.iter().map(|&s| s as f64).collect(),
        bypass_lin_to_log: false,
        direction: FWD,
    })
}
