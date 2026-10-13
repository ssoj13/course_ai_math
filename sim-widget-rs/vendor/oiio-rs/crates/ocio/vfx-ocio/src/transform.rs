//! Transform definitions for color space conversions.
//!
//! Transforms define operations applied to pixel values:
//! - Matrix transforms (primaries conversion)
//! - Transfer function (OETF/EOTF)
//! - LUT application (1D, 3D)
//! - CDL (Color Decision List)
//! - And more...
//!
//! Transforms can be chained via `GroupTransform`.

use std::path::PathBuf;

/// Transform application direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransformDirection {
    /// Forward transform.
    #[default]
    Forward,
    /// Inverse transform.
    Inverse,
}

impl TransformDirection {
    /// Returns the opposite direction.
    #[inline]
    pub fn inverse(self) -> Self {
        match self {
            Self::Forward => Self::Inverse,
            Self::Inverse => Self::Forward,
        }
    }
}

/// Interpolation method for LUTs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interpolation {
    /// OCIO `INTERP_DEFAULT`: the file format's own default, evaluated as linear (trilinear for
    /// a 3D LUT). The default of a FileTransform, which OCIO does not write out.
    #[default]
    Default,
    /// Nearest neighbor.
    Nearest,
    /// Linear interpolation.
    Linear,
    /// Tetrahedral interpolation (default for 3D).
    Tetrahedral,
    /// Best available (context-dependent).
    Best,
}

impl Interpolation {
    /// The method a 3D LUT is actually evaluated with, per OCIO v2
    /// `Lut3DOpData::GetConcreteInterpolation`: `Best` and `Tetrahedral` are tetrahedral;
    /// everything else, `Nearest` included ("in OCIO v2, INTERP_NEAREST is implemented as
    /// trilinear"), is trilinear.
    #[must_use]
    pub fn lut3d_method(self) -> vfx_lut::Interpolation {
        match self {
            Self::Tetrahedral | Self::Best => vfx_lut::Interpolation::Tetrahedral,
            Self::Default | Self::Nearest | Self::Linear => vfx_lut::Interpolation::Linear,
        }
    }
}

/// Optional hue adjustment for 1D LUTs (OCIO parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lut1DHueAdjust {
    /// No adjustment.
    #[default]
    None,
    /// ACES DW3 hue adjustment.
    Dw3,
    /// Weighted Yellow Power Norm. Defined but not implemented by OCIO, which refuses it; so does
    /// the processor (`InvalidTransform`).
    Wypn,
}

/// Color transform definition.
///
/// This enum represents all transform types supported by OCIO.
#[derive(Debug, Clone)]
pub enum Transform {
    /// 4x4 matrix transform.
    Matrix(MatrixTransform),

    /// Transfer function (builtin).
    BuiltinTransfer(BuiltinTransferTransform),

    /// Exponent/gamma.
    Exponent(ExponentTransform),

    /// Exponent with linear segment (sRGB, Rec.709 style).
    ExponentWithLinear(ExponentWithLinearTransform),

    /// Log transform.
    Log(LogTransform),

    /// Log affine transform (OCIO v2).
    LogAffine(LogAffineTransform),

    /// Log camera transform (ACEScct, LogC, S-Log3).
    LogCamera(LogCameraTransform),

    /// 1D LUT from file.
    FileTransform(FileTransform),

    /// CDL (slope/offset/power/sat).
    Cdl(CdlTransform),

    /// Range remapping.
    Range(RangeTransform),

    /// Fixed function (ACES specific).
    FixedFunction(FixedFunctionTransform),

    /// Exposure/contrast adjustment.
    ExposureContrast(ExposureContrastTransform),

    /// Reference to named color space.
    ColorSpace(ColorSpaceTransform),

    /// Reference to named look.
    Look(LookTransform),

    /// Reference to display/view.
    DisplayView(DisplayViewTransform),

    /// Group of chained transforms.
    Group(GroupTransform),

    /// Builtin transform by name (OCIO v2).
    Builtin(BuiltinTransform),

    /// Allocation transform (for GPU optimization).
    Allocation(AllocationTransform),

    /// Grading primary transform (lift/gamma/gain).
    GradingPrimary(GradingPrimaryTransform),

    /// Grading RGB curves.
    GradingRgbCurve(GradingRgbCurveTransform),

    /// Grading tone transform (shadows/midtones/highlights).
    GradingTone(GradingToneTransform),

    /// Grading hue curve transform (8 hue-based curves).
    GradingHueCurve(GradingHueCurveTransform),

    /// Inline 1D LUT transform.
    Lut1D(Lut1DTransform),

    /// Inline 3D LUT transform.
    Lut3D(Lut3DTransform),
}

impl Transform {
    /// Creates a matrix transform from a 4x4 array.
    pub fn matrix(m: [f64; 16]) -> Self {
        Self::Matrix(MatrixTransform {
            name: String::new(),
            matrix: m,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        })
    }

    /// Creates a group transform.
    pub fn group(transforms: Vec<Transform>) -> Self {
        Self::Group(GroupTransform {
            name: String::new(),
            transforms,
            direction: TransformDirection::Forward,
        })
    }

    /// Creates a file transform (LUT reference).
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::FileTransform(FileTransform {
            src: path.into(),
            ccc_id: None,
            cdl_style: CdlStyle::NoClamp,
            interpolation: Interpolation::default(),
            direction: TransformDirection::Forward,
        })
    }

    /// Returns the inverse of this transform.
    pub fn inverse(self) -> Self {
        match self {
            Self::Matrix(mut t) => {
                t.direction = t.direction.inverse();
                Self::Matrix(t)
            }
            Self::BuiltinTransfer(mut t) => {
                t.direction = t.direction.inverse();
                Self::BuiltinTransfer(t)
            }
            Self::Exponent(mut t) => {
                t.direction = t.direction.inverse();
                Self::Exponent(t)
            }
            Self::ExponentWithLinear(mut t) => {
                t.direction = t.direction.inverse();
                Self::ExponentWithLinear(t)
            }
            Self::Log(mut t) => {
                t.direction = t.direction.inverse();
                Self::Log(t)
            }
            Self::LogAffine(mut t) => {
                t.direction = t.direction.inverse();
                Self::LogAffine(t)
            }
            Self::LogCamera(mut t) => {
                t.direction = t.direction.inverse();
                Self::LogCamera(t)
            }
            Self::FileTransform(mut t) => {
                t.direction = t.direction.inverse();
                Self::FileTransform(t)
            }
            Self::Cdl(mut t) => {
                t.direction = t.direction.inverse();
                Self::Cdl(t)
            }
            Self::Range(mut t) => {
                t.direction = t.direction.inverse();
                Self::Range(t)
            }
            Self::FixedFunction(mut t) => {
                t.direction = t.direction.inverse();
                Self::FixedFunction(t)
            }
            Self::ExposureContrast(mut t) => {
                t.direction = t.direction.inverse();
                Self::ExposureContrast(t)
            }
            Self::ColorSpace(mut t) => {
                t.direction = t.direction.inverse();
                Self::ColorSpace(t)
            }
            Self::Look(mut t) => {
                t.direction = t.direction.inverse();
                Self::Look(t)
            }
            Self::DisplayView(mut t) => {
                t.direction = t.direction.inverse();
                Self::DisplayView(t)
            }
            Self::Group(mut t) => {
                // Flip the direction ONLY — do NOT reorder `t.transforms`.
                //
                // Inverting a group means applying its children in reverse order, but that
                // reordering belongs to compilation, not to the stored transform:
                // `Processor::compile_transform` already walks a group with combined direction
                // `Inverse` back-to-front (`processor/compile.rs:244-247`), exactly as the
                // reference's `BuildGroupOps` does (`OpenColorIO/src/OpenColorIO/transforms/
                // GroupTransform.cpp:196-202`, `i` from N-1 down to 0). The C++ never permutes
                // the stored list.
                //
                // Reversing here too made the list flip TWICE — once eagerly, once at compile —
                // and the two cancelled, so an inverted group ran its children in FORWARD order
                // with each child individually inverted. Every op inverts correctly in
                // isolation, so unit tests on a single-child group pass; it only breaks where
                // the children do not commute, e.g. a channel-mixing matrix followed by a
                // non-linear curve. That was P8: `ACEScg -> sRGB -> ACEScg` came back as
                // 0.27273 instead of 0.3, with a different error per channel.
                t.direction = t.direction.inverse();
                Self::Group(t)
            }
            Self::Builtin(mut t) => {
                t.direction = t.direction.inverse();
                Self::Builtin(t)
            }
            Self::Allocation(mut t) => {
                t.direction = t.direction.inverse();
                Self::Allocation(t)
            }
            Self::GradingPrimary(mut t) => {
                t.direction = t.direction.inverse();
                Self::GradingPrimary(t)
            }
            Self::GradingRgbCurve(mut t) => {
                t.direction = t.direction.inverse();
                Self::GradingRgbCurve(t)
            }
            Self::GradingTone(mut t) => {
                t.direction = t.direction.inverse();
                Self::GradingTone(t)
            }
            Self::GradingHueCurve(mut t) => {
                t.direction = t.direction.inverse();
                Self::GradingHueCurve(t)
            }
            Self::Lut1D(mut t) => {
                t.direction = t.direction.inverse();
                Self::Lut1D(t)
            }
            Self::Lut3D(mut t) => {
                t.direction = t.direction.inverse();
                Self::Lut3D(t)
            }
        }
    }
}

/// 4x4 matrix + offset transform.
#[derive(Debug, Clone)]
pub struct MatrixTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// 4x4 matrix in row-major order.
    pub matrix: [f64; 16],
    /// RGBA offset.
    pub offset: [f64; 4],
    /// Direction.
    pub direction: TransformDirection,
}

impl MatrixTransform {
    /// Identity matrix.
    pub const IDENTITY: [f64; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
}

/// Builtin transfer function transform.
#[derive(Debug, Clone)]
pub struct BuiltinTransferTransform {
    /// Transfer function name (sRGB, Rec709, PQ, HLG, etc.).
    pub style: String,
    /// Direction.
    pub direction: TransformDirection,
}

/// Exponent/gamma transform.
#[derive(Debug, Clone)]
pub struct ExponentTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Per-channel exponents [R, G, B, A].
    pub value: [f64; 4],
    /// Negative handling style.
    pub negative_style: NegativeStyle,
    /// Direction.
    pub direction: TransformDirection,
}

/// Negative value handling for exponent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NegativeStyle {
    /// Clamp negatives to zero (default for ExponentTransform).
    #[default]
    Clamp,
    /// Mirror: sign * pow(abs(x), exp).
    Mirror,
    /// Pass through unchanged.
    PassThru,
    /// Linearly extrapolate for negatives (default for ExponentWithLinear).
    Linear,
}

/// Log transform (lin-to-log or log-to-lin).
#[derive(Debug, Clone)]
pub struct LogTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Base of logarithm (10 or 2).
    pub base: f64,
    /// Direction.
    pub direction: TransformDirection,
}

/// Log affine transform (OCIO v2).
///
/// Applies a logarithmic curve with affine parameters per channel.
/// Formula: out = logSideSlope * log(linSideSlope * in + linSideOffset, base) + logSideOffset
#[derive(Debug, Clone)]
pub struct LogAffineTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Logarithm base (typically 2 or 10).
    pub base: f64,
    /// Log side slope per channel [R, G, B].
    pub log_side_slope: [f64; 3],
    /// Log side offset per channel [R, G, B].
    pub log_side_offset: [f64; 3],
    /// Linear side slope per channel [R, G, B].
    pub lin_side_slope: [f64; 3],
    /// Linear side offset per channel [R, G, B].
    pub lin_side_offset: [f64; 3],
    /// Direction.
    pub direction: TransformDirection,
}

impl Default for LogAffineTransform {
    fn default() -> Self {
        Self {
            name: String::new(),
            base: 2.0,
            log_side_slope: [1.0, 1.0, 1.0],
            log_side_offset: [0.0, 0.0, 0.0],
            lin_side_slope: [1.0, 1.0, 1.0],
            lin_side_offset: [0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        }
    }
}

impl LogAffineTransform {
    /// Creates a new LogAffineTransform with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the base.
    pub fn with_base(mut self, base: f64) -> Self {
        self.base = base;
        self
    }

    /// Sets log side parameters.
    pub fn with_log_side(mut self, slope: [f64; 3], offset: [f64; 3]) -> Self {
        self.log_side_slope = slope;
        self.log_side_offset = offset;
        self
    }

    /// Sets linear side parameters.
    pub fn with_lin_side(mut self, slope: [f64; 3], offset: [f64; 3]) -> Self {
        self.lin_side_slope = slope;
        self.lin_side_offset = offset;
        self
    }

    /// Applies the transform to RGB values.
    pub fn apply(&self, rgb: [f64; 3]) -> [f64; 3] {
        let mut out = [0.0; 3];
        let ln_base = self.base.ln();

        for i in 0..3 {
            let lin = self.lin_side_slope[i] * rgb[i] + self.lin_side_offset[i];
            if lin > 0.0 {
                out[i] = self.log_side_slope[i] * lin.ln() / ln_base + self.log_side_offset[i];
            } else {
                out[i] = self.log_side_offset[i];
            }
        }
        out
    }

    /// Applies the inverse transform.
    pub fn apply_inverse(&self, rgb: [f64; 3]) -> [f64; 3] {
        let mut out = [0.0; 3];
        let ln_base = self.base.ln();

        for i in 0..3 {
            let log_val = (rgb[i] - self.log_side_offset[i]) / self.log_side_slope[i];
            let lin = (log_val * ln_base).exp();
            out[i] = (lin - self.lin_side_offset[i]) / self.lin_side_slope[i];
        }
        out
    }
}

/// Log camera transform (OCIO v2).
///
/// Camera-specific log encoding with linear segment below break point.
/// Used for ACEScct, S-Log3, LogC, etc.
#[derive(Debug, Clone)]
pub struct LogCameraTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Logarithm base (typically 2 or 10).
    pub base: f64,
    /// Log side slope per channel [R, G, B].
    pub log_side_slope: [f64; 3],
    /// Log side offset per channel [R, G, B].
    pub log_side_offset: [f64; 3],
    /// Linear side slope per channel [R, G, B].
    pub lin_side_slope: [f64; 3],
    /// Linear side offset per channel [R, G, B].
    pub lin_side_offset: [f64; 3],
    /// Linear side break point per channel.
    /// Below this value, a linear segment is used.
    pub lin_side_break: [f64; 3],
    /// Linear slope below break point (calculated from continuity).
    pub linear_slope: Option<[f64; 3]>,
    /// Direction.
    pub direction: TransformDirection,
}

impl Default for LogCameraTransform {
    fn default() -> Self {
        Self {
            name: String::new(),
            base: 2.0,
            log_side_slope: [1.0, 1.0, 1.0],
            log_side_offset: [0.0, 0.0, 0.0],
            lin_side_slope: [1.0, 1.0, 1.0],
            lin_side_offset: [0.0, 0.0, 0.0],
            lin_side_break: [0.0, 0.0, 0.0],
            linear_slope: None,
            direction: TransformDirection::Forward,
        }
    }
}

impl LogCameraTransform {
    /// Creates a new LogCameraTransform with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the base.
    pub fn with_base(mut self, base: f64) -> Self {
        self.base = base;
        self
    }

    /// Sets the linear side break point.
    pub fn with_lin_side_break(mut self, brk: [f64; 3]) -> Self {
        self.lin_side_break = brk;
        self
    }

    /// Sets the linear slope for the segment below break.
    pub fn with_linear_slope(mut self, slope: [f64; 3]) -> Self {
        self.linear_slope = Some(slope);
        self
    }

    /// Creates ACEScct-like transform.
    pub fn acescct() -> Self {
        Self {
            name: String::new(),
            base: 2.0,
            log_side_slope: [17.52 / 9.72; 3],
            log_side_offset: [0.0729; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [9.72; 3],
            lin_side_break: [0.0078125; 3], // 2^-7
            linear_slope: Some([10.5; 3]),
            direction: TransformDirection::Forward,
        }
    }

    /// Creates ARRI LogC3-like transform.
    pub fn logc3() -> Self {
        Self {
            name: String::new(),
            base: 10.0,
            log_side_slope: [0.247190; 3],
            log_side_offset: [0.385537; 3],
            lin_side_slope: [5.555556; 3],
            lin_side_offset: [0.052272; 3],
            lin_side_break: [0.010591; 3],
            linear_slope: Some([5.367655; 3]),
            direction: TransformDirection::Forward,
        }
    }

    /// Creates Sony S-Log3-like transform.
    pub fn slog3() -> Self {
        Self {
            name: String::new(),
            base: 10.0,
            log_side_slope: [0.255620723; 3],
            log_side_offset: [0.41055718; 3],
            lin_side_slope: [5.26315789; 3],
            lin_side_offset: [0.052272; 3],
            lin_side_break: [0.01125; 3],
            linear_slope: None,
            direction: TransformDirection::Forward,
        }
    }

    /// Applies the transform to RGB values.
    pub fn apply(&self, rgb: [f64; 3]) -> [f64; 3] {
        let mut out = [0.0; 3];
        let ln_base = self.base.ln();

        for i in 0..3 {
            if rgb[i] <= self.lin_side_break[i] {
                // Linear segment
                let linear_slope = self.linear_slope.map(|s| s[i]).unwrap_or(
                    self.log_side_slope[i] * self.lin_side_slope[i]
                        / ((self.lin_side_slope[i] * self.lin_side_break[i]
                            + self.lin_side_offset[i])
                            * ln_base),
                );
                out[i] = linear_slope * rgb[i];
            } else {
                // Log segment
                let lin = self.lin_side_slope[i] * rgb[i] + self.lin_side_offset[i];
                out[i] = self.log_side_slope[i] * lin.ln() / ln_base + self.log_side_offset[i];
            }
        }
        out
    }
}

/// Exponent with linear segment transform (OCIO v2).
///
/// Applies gamma with a linear segment in the shadows.
/// Commonly used for sRGB and Rec.709 transfer functions.
#[derive(Debug, Clone)]
pub struct ExponentWithLinearTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Exponent (gamma) per channel [R, G, B, A].
    pub gamma: [f64; 4],
    /// Offset per channel (for linear segment calculation).
    pub offset: [f64; 4],
    /// Negative value handling.
    pub negative_style: NegativeStyle,
    /// Direction.
    pub direction: TransformDirection,
}

impl Default for ExponentWithLinearTransform {
    fn default() -> Self {
        Self {
            name: String::new(),
            gamma: [1.0, 1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0, 0.0],
            negative_style: NegativeStyle::Linear,
            direction: TransformDirection::Forward,
        }
    }
}

impl ExponentWithLinearTransform {
    /// Creates a new ExponentWithLinearTransform (negative style LINEAR, OCIO's default).
    pub fn new(gamma: [f64; 4], offset: [f64; 4]) -> Self {
        Self {
            name: String::new(),
            gamma,
            offset,
            negative_style: NegativeStyle::Linear,
            direction: TransformDirection::Forward,
        }
    }

    /// Creates sRGB transfer function.
    pub fn srgb() -> Self {
        Self::new([2.4, 2.4, 2.4, 1.0], [0.055, 0.055, 0.055, 0.0])
    }

    /// Creates Rec.709 transfer function.
    pub fn rec709() -> Self {
        Self::new(
            [1.0 / 0.45, 1.0 / 0.45, 1.0 / 0.45, 1.0],
            [0.099, 0.099, 0.099, 0.0],
        )
    }

    /// Sets the negative style: LINEAR or MIRROR only, as OCIO's
    /// `ExponentWithLinearTransform::setNegativeStyle` (`GammaOpData::ConvertStyleMonCurve`).
    /// CLAMP and PASS_THRU are errors and leave the transform unchanged.
    pub fn set_negative_style(&mut self, style: NegativeStyle) -> crate::OcioResult<()> {
        moncurve_negative_style(style)?;
        self.negative_style = style;
        Ok(())
    }

    /// Checks the transform as OCIO does before building ops: the negative style must be LINEAR
    /// or MIRROR. The fields are public, so the processor calls this at compile time too.
    pub fn validate(&self) -> crate::OcioResult<()> {
        moncurve_negative_style(self.negative_style)
    }
}

/// OCIO `GammaOpData::ConvertStyleMonCurve`'s check, with its error text.
fn moncurve_negative_style(style: NegativeStyle) -> crate::OcioResult<()> {
    match style {
        NegativeStyle::Linear | NegativeStyle::Mirror => Ok(()),
        NegativeStyle::PassThru => Err(crate::OcioError::Validation(
            "Pass thru negative extrapolation is not valid for MonCurve exponent style.".into(),
        )),
        NegativeStyle::Clamp => Err(crate::OcioError::Validation(
            "Clamp negative extrapolation is not valid for MonCurve exponent style.".into(),
        )),
    }
}

/// File-based transform (LUT, etc.).
#[derive(Debug, Clone)]
pub struct FileTransform {
    /// Source file path.
    pub src: PathBuf,
    /// CDL correction ID (for .ccc/.cdl files).
    pub ccc_id: Option<String>,
    /// CDL style (for .cc/.ccc/.cdl).
    pub cdl_style: CdlStyle,
    /// Interpolation method.
    pub interpolation: Interpolation,
    /// Direction.
    pub direction: TransformDirection,
}

/// CDL (ASC Color Decision List) transform.
#[derive(Debug, Clone)]
pub struct CdlTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Per-channel slope [R, G, B].
    pub slope: [f64; 3],
    /// Per-channel offset [R, G, B].
    pub offset: [f64; 3],
    /// Per-channel power [R, G, B].
    pub power: [f64; 3],
    /// Saturation (1.0 = no change).
    pub saturation: f64,
    /// CDL style.
    pub style: CdlStyle,
    /// Direction.
    pub direction: TransformDirection,
}

/// CDL style (order of operations).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CdlStyle {
    /// ASC CDL v1.2 (clamps to [0, 1]).
    AscCdl,
    /// No clamping: OCIO's `CDL_TRANSFORM_DEFAULT` (a CDLTransform or FileTransform without a
    /// style).
    #[default]
    NoClamp,
}

impl Default for CdlTransform {
    fn default() -> Self {
        Self {
            name: String::new(),
            slope: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            style: CdlStyle::default(),
            direction: TransformDirection::Forward,
        }
    }
}

/// Range remapping transform.
#[derive(Debug, Clone)]
pub struct RangeTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Input min (None = no clamping).
    pub min_in: Option<f64>,
    /// Input max.
    pub max_in: Option<f64>,
    /// Output min.
    pub min_out: Option<f64>,
    /// Output max.
    pub max_out: Option<f64>,
    /// Style.
    pub style: RangeStyle,
    /// Direction.
    pub direction: TransformDirection,
}

/// Range transform style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RangeStyle {
    /// Clamp values to range.
    #[default]
    Clamp,
    /// No clamping, just scale.
    NoClamp,
}

/// Fixed function transform (ACES-specific operations).
#[derive(Debug, Clone)]
pub struct FixedFunctionTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Function style.
    pub style: FixedFunctionStyle,
    /// Optional parameters.
    pub params: Vec<f64>,
    /// Direction.
    pub direction: TransformDirection,
}

/// Grading primary styles (OCIO parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradingPrimaryStyle {
    /// Log working space - brightness/contrast/gamma on log-encoded values.
    /// Uses: brightness (additive), contrast (around pivot), gamma (power curve).
    #[default]
    Log,
    /// Linear working space - exposure/contrast on linear values.
    /// Uses: offset (additive), exposure (2^exp multiplier), contrast (around pivot).
    Linear,
    /// Video working space - lift/gamma/gain model for video grading.
    /// Uses: offset (additive), slope (multiplier), gamma (power curve).
    Video,
}

/// Fixed function styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedFunctionStyle {
    /// ACES red modifier (v0.3/0.7).
    AcesRedMod03,
    /// ACES red modifier (v1.0).
    AcesRedMod10,
    /// ACES glow (v0.3/0.7).
    AcesGlow03,
    /// ACES glow (v1.0).
    AcesGlow10,
    /// ACES dark-to-dim surround correction (v1.0). Param: gamma.
    AcesDarkToDim10,
    /// ACES gamut compress (v1.3).
    AcesGamutComp13,
    /// Rec.2100 surround correction. Param: gamma.
    Rec2100Surround,
    /// RGB to HSV.
    RgbToHsv,
    /// HSV to RGB.
    HsvToRgb,
    /// XYZ to xyY.
    XyzToXyy,
    /// xyY to XYZ.
    XyyToXyz,
    /// XYZ to uvY.
    XyzToUvy,
    /// uvY to XYZ.
    UvyToXyz,
    /// XYZ to Luv.
    XyzToLuv,
    /// Luv to XYZ.
    LuvToXyz,
    /// Linear to PQ (ST-2084). 1.0 = 100 nits.
    LinToPq,
    /// PQ to Linear.
    PqToLin,
    /// Linear to parameterized gamma+log curve. Params: [mirrorPt, breakPt, gamma, logSlope, linSlope, logOffset, linOffset].
    LinToGammaLog,
    /// Gamma+log to Linear (inverse).
    GammaLogToLin,
    /// Linear to double-log curve. Params: [base, breakPt, logSlope, logOffset, linSlope, linOffset, linearSlope, log2Slope, log2Offset].
    LinToDoubleLog,
    /// Double-log to Linear (inverse).
    DoubleLogToLin,
    /// ACES 2.0 Output Transform (experimental). Params: peak_lum.
    AcesOutputTransform20,
    /// ACES 2.0 RGB to JMh (experimental).
    AcesRgbToJmh20,
    /// ACES 2.0 JMh to RGB (experimental).
    AcesJmhToRgb20,
    /// ACES 2.0 Tonescale + chroma compress (experimental).
    AcesTonescaleCompress20,
    /// ACES 2.0 Gamut compress (experimental).
    AcesGamutCompress20,
    /// RGB to HSY (linear variant).
    RgbToHsyLin,
    /// HSY (linear) to RGB.
    HsyLinToRgb,
    /// RGB to HSY (log variant).
    RgbToHsyLog,
    /// HSY (log) to RGB.
    HsyLogToRgb,
    /// RGB to HSY (video variant).
    RgbToHsyVid,
    /// HSY (video) to RGB.
    HsyVidToRgb,
}

/// Exposure/contrast adjustment.
/// OCIO parity: ExposureContrastTransform in OpenColorTransforms.h.
#[derive(Debug, Clone)]
pub struct ExposureContrastTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Exposure in stops.
    pub exposure: f64,
    /// Contrast (1.0 = no change).
    pub contrast: f64,
    /// Gamma (1.0 = no change).
    pub gamma: f64,
    /// Pivot point for contrast.
    pub pivot: f64,
    /// Log exposure step (for Logarithmic style).
    /// The increment needed to move one stop. Examples:
    /// - ACEScct: 0.057
    /// - LogC: ~0.074
    /// - Cineon: ~90/1023 = 0.088 (default)
    ///
    /// OCIO ref: getLogExposureStep()/setLogExposureStep().
    pub log_exposure_step: f64,
    /// Log mid gray (for Logarithmic style).
    /// The position of 18% gray in log space.
    /// Default: 0.435
    /// OCIO ref: getLogMidGray()/setLogMidGray().
    pub log_mid_gray: f64,
    /// Style (linear, video, or logarithmic).
    pub style: ExposureContrastStyle,
    /// Which parameters are dynamic (OCIO `makeExposureDynamic()` and friends): the compiled
    /// processor keeps the op alive at identity values and lets
    /// [`Processor::set_dynamic`](crate::Processor::set_dynamic) change them without a rebuild.
    pub dynamic: EcDynamic,
    /// Direction.
    pub direction: TransformDirection,
}

impl Default for ExposureContrastTransform {
    /// OCIO's defaults (`ExposureContrastOpData`): linear style, exposure 0, contrast 1, gamma 1,
    /// pivot 0.18, log exposure step 0.088, log mid gray 0.435, nothing dynamic, forward.
    fn default() -> Self {
        Self {
            name: String::new(),
            exposure: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            pivot: 0.18,
            log_exposure_step: 0.088,
            log_mid_gray: 0.435,
            style: ExposureContrastStyle::Linear,
            dynamic: EcDynamic::NONE,
            direction: TransformDirection::Forward,
        }
    }
}

/// The dynamic flags of an [`ExposureContrastTransform`] (OCIO `isExposureDynamic()`,
/// `isContrastDynamic()`, `isGammaDynamic()`). Deliberately not `#[non_exhaustive]`: OCIO has
/// exactly these three, and hosts build it with the constants or a struct literal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EcDynamic {
    /// Exposure can be changed on the compiled processor.
    pub exposure: bool,
    /// Contrast can be changed on the compiled processor.
    pub contrast: bool,
    /// Gamma can be changed on the compiled processor.
    pub gamma: bool,
}

impl EcDynamic {
    /// All three parameters static (OCIO's default).
    pub const NONE: Self = Self {
        exposure: false,
        contrast: false,
        gamma: false,
    };

    /// Only the exposure is dynamic: the viewer's exposure dial.
    pub const EXPOSURE: Self = Self {
        exposure: true,
        ..Self::NONE
    };

    /// Only the contrast is dynamic.
    pub const CONTRAST: Self = Self {
        contrast: true,
        ..Self::NONE
    };

    /// Only the gamma is dynamic: the viewer's gamma dial.
    pub const GAMMA: Self = Self {
        gamma: true,
        ..Self::NONE
    };

    /// True when any parameter is dynamic.
    pub const fn any(self) -> bool {
        self.exposure || self.contrast || self.gamma
    }
}

/// Exposure/contrast style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExposureContrastStyle {
    /// Linear domain.
    #[default]
    Linear,
    /// Video/log domain.
    Video,
    /// Logarithmic.
    Logarithmic,
}

/// Reference to a named color space.
#[derive(Debug, Clone)]
pub struct ColorSpaceTransform {
    /// Source color space name.
    pub src: String,
    /// Destination color space name.
    pub dst: String,
    /// Data color space bypass (default: true in OCIO).
    pub data_bypass: bool,
    /// Direction.
    pub direction: TransformDirection,
}

/// Reference to a named look.
#[derive(Debug, Clone)]
pub struct LookTransform {
    /// Source color space.
    pub src: String,
    /// Destination color space.
    pub dst: String,
    /// Look names (comma-separated for multiple).
    pub looks: String,
    /// Skip the color space conversion steps (OCIO parity).
    pub skip_color_space_conversion: bool,
    /// Direction.
    pub direction: TransformDirection,
}

/// Display/view transform reference.
#[derive(Debug, Clone)]
pub struct DisplayViewTransform {
    /// Source color space.
    pub src: String,
    /// Display name.
    pub display: String,
    /// View name.
    pub view: String,
    /// Bypass looks (default: false).
    pub looks_bypass: bool,
    /// Data color space bypass (default: true in OCIO).
    pub data_bypass: bool,
    /// Look override applied on top of the view's embedded looks when
    /// [`Self::looks_override_enabled`] is `true`. Mirrors OCIO C++
    /// `DisplayViewTransform::m_looksOverride`.
    pub looks_override: String,
    /// Enables the [`Self::looks_override`] field. Mirrors OCIO C++
    /// `DisplayViewTransform::m_looksOverrideEnabled`.
    pub looks_override_enabled: bool,
    /// Direction.
    pub direction: TransformDirection,
}

impl Default for DisplayViewTransform {
    fn default() -> Self {
        // OCIO C++ defaults: looks_bypass=false, data_bypass=true,
        // looks_override empty + disabled.
        Self {
            src: String::new(),
            display: String::new(),
            view: String::new(),
            looks_bypass: false,
            data_bypass: true,
            looks_override: String::new(),
            looks_override_enabled: false,
            direction: TransformDirection::Forward,
        }
    }
}

impl DisplayViewTransform {
    /// Sets the look override and enables it. Empty string disables
    /// the override (matches OCIO C++ where setting the override to
    /// `""` is the canonical "off" form).
    pub fn with_looks_override(mut self, looks: impl Into<String>) -> Self {
        self.looks_override = looks.into();
        self.looks_override_enabled = !self.looks_override.is_empty();
        self
    }
}

/// Group of chained transforms.
#[derive(Debug, Clone)]
pub struct GroupTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Ordered list of transforms.
    pub transforms: Vec<Transform>,
    /// Direction (affects iteration order).
    pub direction: TransformDirection,
}

/// Builtin transform by name (OCIO v2).
#[derive(Debug, Clone)]
pub struct BuiltinTransform {
    /// Builtin name (e.g., "ACEScct_to_ACES2065-1").
    pub style: String,
    /// Direction.
    pub direction: TransformDirection,
}

/// Allocation transform — explicit linear/log2 fit-to-[0,1]
/// packing used to keep HDR data inside the sampler range of
/// fixed-point textures and uniform buffers.
///
/// This is the **user-facing** transform that applies real math on
/// both CPU (`ProcessorOp::Allocation` in `processor/apply.rs`)
/// and GPU (`GpuOp::Allocation` in every `gpu/{wgsl,glsl,metal,
/// hlsl}.rs` backend). It is the direct port of OCIO C++
/// `AllocationTransform` (`OpenColorIO/src/.../AllocationTransform.h`).
///
/// OCIO C++ has a second, *distinct* concept named
/// `CreateGpuAllocationNoOp` (`OpenColorIO/src/.../NoOps.cpp`) that
/// the C++ `BuildColorSpace{To,From}ReferenceOps` slips into the op
/// vector as a CPU no-op marker — it exists solely so the legacy
/// `LegacyShaderGen` GPU path can wrap downstream ops with
/// pack/unpack code for fixed-point texture samplers. We do **not**
/// port that marker: every supported vfx-ocio shader target
/// (WGSL via wgpu, Metal, GLSL 3.30+, HLSL SM 5.0+, Vulkan GLSL,
/// GLSL ES 3.00) uses float16/float32 textures and writes the
/// math out directly without a pack/unpack frame. Re-adding it
/// would only be required if a legacy OpenGL fixed-function
/// shader generator were ever introduced.
#[derive(Debug, Clone)]
pub struct AllocationTransform {
    /// Allocation type.
    pub allocation: AllocationType,
    /// Allocation variables (min, max, etc.).
    pub vars: Vec<f64>,
    /// Direction.
    pub direction: TransformDirection,
}

/// Allocation type (OCIO `Allocation`), for an `AllocationTransform` and a colour space's
/// `allocation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AllocationType {
    /// Uniform allocation (`uniform`).
    #[default]
    Uniform,
    /// Logarithmic allocation (`lg2`).
    Log2,
    /// An unrecognised name (OCIO `ALLOCATION_UNKNOWN`): a config holding it loads, a processor
    /// from an `AllocationTransform` holding it fails (`BuildAllocationOp`).
    Unknown,
}

impl AllocationType {
    /// OCIO `AllocationFromString`: `uniform` or `lg2`, any case; anything else is `Unknown`.
    pub fn from_name(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "uniform" => Self::Uniform,
            "lg2" => Self::Log2,
            _ => Self::Unknown,
        }
    }

    /// OCIO `AllocationToString`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Uniform => "uniform",
            Self::Log2 => "lg2",
            Self::Unknown => "unknown",
        }
    }
}

/// Grading primary transform.
///
/// OCIO ref: `GradingPrimaryTransform` (`OpenColorTransforms.h:1240-1277`) has
/// no fields of its own — it wraps a `GradingPrimary` value directly
/// (`getValue()`/`setValue(const GradingPrimary&)`). This struct now mirrors
/// that exactly: `value` (this crate's [`crate::dynamic::GradingPrimary`],
/// which carries all seven of brightness/contrast/gamma/offset/exposure/
/// lift/gain as [`crate::dynamic::GradingRgbm`] — per-channel + master, like
/// real OCIO's `GradingRGBM`) is the SAME type `DynamicPropertyValue::
/// GradingPrimary` uses, so there is one canonical shape for this data
/// crate-wide rather than the two/three that used to disagree (see
/// `docs/ROADMAP.md`'s GradingPrimary item).
///
/// Applies color correction with different algorithms per style:
/// - **Log**: Brightness/contrast/gamma on log-encoded values (for log footage).
/// - **Linear**: Offset/exposure/contrast on linear values (for linear data).
/// - **Video**: Lift/gamma/gain model (classic video grading).
#[derive(Debug, Clone)]
pub struct GradingPrimaryTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Grading style (Log, Linear, Video). Lives on the transform, not on
    /// `GradingPrimary` itself, matching OCIO's own split (`GradingPrimary::
    /// validate(GradingStyle)` takes it as a parameter rather than storing
    /// it).
    pub style: GradingPrimaryStyle,
    /// The RGBM-per-field grading values. See the struct doc comment.
    pub value: crate::dynamic::GradingPrimary,
    /// Direction.
    pub direction: TransformDirection,
}

impl GradingPrimaryTransform {
    /// Creates an identity transform for the given style, with the
    /// style-appropriate default pivot (OCIO: `GradingPrimaryTransform::
    /// Create(style)` + `GradingPrimary(style)`,
    /// `OpenColorTransforms.h:464-469`).
    pub fn new(style: GradingPrimaryStyle) -> Self {
        let value = match style {
            GradingPrimaryStyle::Log => crate::dynamic::GradingPrimary::log_default(),
            GradingPrimaryStyle::Linear => crate::dynamic::GradingPrimary::linear_default(),
            GradingPrimaryStyle::Video => crate::dynamic::GradingPrimary::video_default(),
        };
        Self {
            name: String::new(),
            style,
            value,
            direction: TransformDirection::Forward,
        }
    }
}

impl Default for GradingPrimaryTransform {
    fn default() -> Self {
        Self::new(GradingPrimaryStyle::Log)
    }
}

impl GradingPrimaryTransform {
    /// Builds a `GradingPrimaryTransform` from flat per-channel [R,G,B]
    /// arrays with an identity master on every field (0.0 for the additive
    /// fields, 1.0 for the multiplicative ones) — this crate's PRE-
    /// unification shape, kept only as a migration aid for call sites that
    /// have no master value to set. New code should construct `value`
    /// directly instead, to actually use `master`.
    ///
    /// Note `slope` has no home here: VIDEO style no longer takes a
    /// directly-settable slope at all — it is algebraically DERIVED from
    /// `lift`/`gain`/`pivot_black`/`pivot_white` (`op::grading_primary_params`,
    /// porting OCIO's `GradingPrimaryPreRender::update`). A caller that used
    /// to set `slope` directly must be rewritten to set `lift`/`gain`.
    #[allow(clippy::too_many_arguments)]
    pub fn from_flat(
        style: GradingPrimaryStyle,
        brightness: [f64; 3],
        contrast: [f64; 3],
        gamma: [f64; 3],
        exposure: [f64; 3],
        offset: [f64; 3],
        saturation: f64,
        pivot: f64,
        pivot_black: f64,
        pivot_white: f64,
        clamp_black: f64,
        clamp_white: f64,
        direction: TransformDirection,
    ) -> Self {
        use crate::dynamic::{GradingPrimary, GradingRgbm};
        Self {
            name: String::new(),
            style,
            value: GradingPrimary {
                brightness: GradingRgbm::new(brightness[0], brightness[1], brightness[2], 0.0),
                contrast: GradingRgbm::new(contrast[0], contrast[1], contrast[2], 1.0),
                gamma: GradingRgbm::new(gamma[0], gamma[1], gamma[2], 1.0),
                offset: GradingRgbm::new(offset[0], offset[1], offset[2], 0.0),
                exposure: GradingRgbm::new(exposure[0], exposure[1], exposure[2], 0.0),
                lift: GradingRgbm::default(),
                gain: GradingRgbm::one(),
                saturation,
                pivot,
                pivot_black,
                pivot_white,
                clamp_black,
                clamp_white,
            },
            direction,
        }
    }
}

/// Grading RGB curve transform.
///
/// Applies per-channel curves for color correction.
/// OCIO parity: GradingRGBCurveTransform in OpenColorTransforms.h.
#[derive(Debug, Clone)]
pub struct GradingRgbCurveTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Grading style (Log/Linear/Video).
    /// OCIO ref: getStyle()/setStyle().
    pub style: GradingToneStyle,
    /// Red channel control points [(x, y), ...].
    pub red: Vec<[f64; 2]>,
    /// Green channel control points.
    pub green: Vec<[f64; 2]>,
    /// Blue channel control points.
    pub blue: Vec<[f64; 2]>,
    /// Master (luminance) control points.
    pub master: Vec<[f64; 2]>,
    /// Optional slopes for red channel control points.
    /// If empty, slopes are computed automatically from control points.
    /// OCIO ref: getSlope(RGBCurveType::RGB_RED, index)/setSlope().
    pub red_slopes: Vec<f64>,
    /// Optional slopes for green channel.
    pub green_slopes: Vec<f64>,
    /// Optional slopes for blue channel.
    pub blue_slopes: Vec<f64>,
    /// Optional slopes for master channel.
    pub master_slopes: Vec<f64>,
    /// Bypass Lin-to-Log conversion (OCIO parity).
    /// When true in Linear style, skips the lin-to-log pre-conversion.
    /// Useful for UI curve drawing or when input is already in log space.
    /// OCIO ref: getBypassLinToLog()/setBypassLinToLog().
    pub bypass_lin_to_log: bool,
    /// Direction.
    pub direction: TransformDirection,
}

impl Default for GradingRgbCurveTransform {
    fn default() -> Self {
        Self::new(GradingToneStyle::Log)
    }
}

impl GradingRgbCurveTransform {
    /// OCIO's default (identity) curve for a style (`GradingRGBCurveImpl::Default` /
    /// `DefaultLin`, float control points).
    pub fn default_curve(style: GradingToneStyle) -> Vec<[f64; 2]> {
        match style {
            GradingToneStyle::Linear => vec![[-7.0, -7.0], [0.0, 0.0], [7.0, 7.0]],
            _ => vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
        }
    }

    /// An identity transform of a style (OCIO `GradingRGBCurveTransform::Create(style)`).
    pub fn new(style: GradingToneStyle) -> Self {
        let identity = Self::default_curve(style);
        Self {
            name: String::new(),
            style,
            red: identity.clone(),
            green: identity.clone(),
            blue: identity.clone(),
            master: identity,
            red_slopes: Vec::new(),
            green_slopes: Vec::new(),
            blue_slopes: Vec::new(),
            master_slopes: Vec::new(),
            bypass_lin_to_log: false,
            direction: TransformDirection::Forward,
        }
    }
}

impl GradingRgbCurveTransform {
    /// Returns true if slopes are using default (auto-computed) values.
    /// OCIO ref: slopesAreDefault(RGBCurveType).
    pub fn slopes_are_default(&self, curve: RgbCurveType) -> bool {
        match curve {
            RgbCurveType::Red => self.red_slopes.is_empty(),
            RgbCurveType::Green => self.green_slopes.is_empty(),
            RgbCurveType::Blue => self.blue_slopes.is_empty(),
            RgbCurveType::Master => self.master_slopes.is_empty(),
        }
    }
}

/// RGB curve type for slope access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RgbCurveType {
    /// Red channel curve.
    Red,
    /// Green channel curve.
    Green,
    /// Blue channel curve.
    Blue,
    /// Master (all channels) curve.
    Master,
}

/// Grading tone style (OCIO parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradingToneStyle {
    /// Log style: top=1.0, bottom=0.0, pivot=0.4
    #[default]
    Log,
    /// Linear style: top=7.5, bottom=-5.5, pivot=0.0
    Linear,
    /// Video style: top=1.0, bottom=0.0, pivot=0.4
    Video,
}

impl GradingToneStyle {
    /// Returns (top, top_sc, bottom, pivot) for this style.
    pub fn zone_params(&self) -> (f64, f64, f64, f64) {
        match self {
            GradingToneStyle::Log => (1.0, 1.0, 0.0, 0.4),
            GradingToneStyle::Linear => (7.5, 6.5, -5.5, 0.0),
            GradingToneStyle::Video => (1.0, 1.0, 0.0, 0.4),
        }
    }
}

/// RGBM zone value (Red, Green, Blue, Master + start/width).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradingRgbmsw {
    /// Red channel value.
    pub red: f64,
    /// Green channel value.
    pub green: f64,
    /// Blue channel value.
    pub blue: f64,
    /// Master (affects all channels).
    pub master: f64,
    /// Start position for this zone.
    pub start: f64,
    /// Width of the zone.
    pub width: f64,
}

impl Default for GradingRgbmsw {
    fn default() -> Self {
        Self {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            master: 1.0,
            start: 0.0,
            width: 1.0,
        }
    }
}

impl GradingRgbmsw {
    /// Creates identity value (no change).
    pub fn identity() -> Self {
        Self::default()
    }

    /// Creates blacks identity (all zeros except master=1).
    pub fn blacks_identity() -> Self {
        Self {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            master: 1.0,
            start: 0.0,
            width: 0.0,
        }
    }

    /// Returns RGBM as array [r, g, b, m].
    pub fn rgbm(&self) -> [f64; 4] {
        [self.red, self.green, self.blue, self.master]
    }
}

/// Grading tone transform (OCIO parity).
///
/// Applies tonal adjustments using 5 zones:
/// - Blacks: lift/gain for darkest values
/// - Shadows: faux-cubic curve for dark areas
/// - Midtones: 6-point spline for mid-range
/// - Highlights: faux-cubic curve for bright areas  
/// - Whites: gain for brightest values
/// - S-Contrast: quadratic contrast curve
#[derive(Debug, Clone)]
pub struct GradingToneTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Processing style (Log/Linear/Video).
    pub style: GradingToneStyle,
    /// Blacks zone (RGBM + start/width).
    pub blacks: GradingRgbmsw,
    /// Shadows zone (RGBM + start/width).
    pub shadows: GradingRgbmsw,
    /// Midtones zone (RGBM + start/width).
    pub midtones: GradingRgbmsw,
    /// Highlights zone (RGBM + start/width).
    pub highlights: GradingRgbmsw,
    /// Whites zone (RGBM + start/width).
    pub whites: GradingRgbmsw,
    /// S-Contrast (1.0 = identity).
    pub scontrast: f64,
    /// Direction.
    pub direction: TransformDirection,
}

impl Default for GradingToneTransform {
    fn default() -> Self {
        // Must match `GradingTone::from_style(Log)` — the transform Default used to carry
        // placeholder start/width (blacks/whites width=0) that `GradingTone::validate` rejects.
        let v = crate::grading_tone::GradingTone::from_style(GradingToneStyle::Log);
        Self {
            name: String::new(),
            style: GradingToneStyle::Log,
            blacks: v.blacks,
            shadows: v.shadows,
            midtones: v.midtones,
            highlights: v.highlights,
            whites: v.whites,
            scontrast: v.s_contrast,
            direction: TransformDirection::Forward,
        }
    }
}

impl GradingToneTransform {
    /// Returns true if this is identity (no effect).
    pub fn is_identity(&self) -> bool {
        fn zone_identity(z: &GradingRgbmsw) -> bool {
            (z.red - 1.0).abs() < 1e-9
                && (z.green - 1.0).abs() < 1e-9
                && (z.blue - 1.0).abs() < 1e-9
                && (z.master - 1.0).abs() < 1e-9
        }
        zone_identity(&self.blacks)
            && zone_identity(&self.shadows)
            && zone_identity(&self.midtones)
            && zone_identity(&self.highlights)
            && zone_identity(&self.whites)
            && (self.scontrast - 1.0).abs() < 1e-9
    }
}

/// Inline 1D LUT transform.
///
/// Contains embedded 1D LUT data rather than a file reference.
/// This is useful for procedurally generated LUTs or when the
/// LUT data needs to be stored directly in the configuration.
///
/// # Structure
///
/// - Data stored per channel (R, G, B) or as single shared curve
/// - Linear interpolation between entries
/// - Configurable input/output ranges
#[derive(Debug, Clone)]
pub struct Lut1DTransform {
    /// Red channel LUT data.
    pub red: Vec<f32>,
    /// Green channel LUT data (if None, uses red).
    pub green: Option<Vec<f32>>,
    /// Blue channel LUT data (if None, uses red).
    pub blue: Option<Vec<f32>>,
    /// Input domain minimum.
    pub input_min: f32,
    /// Input domain maximum.
    pub input_max: f32,
    /// Output range minimum (for half-domain LUTs).
    pub output_min: f32,
    /// Output range maximum.
    pub output_max: f32,
    /// Whether input values use half-domain (for HDR).
    pub half_domain: bool,
    /// Whether output uses raw half encoding.
    pub raw_halfs: bool,
    /// Optional hue adjustment algorithm.
    pub hue_adjust: Lut1DHueAdjust,
    /// Interpolation method.
    pub interpolation: Interpolation,
    /// Direction.
    pub direction: TransformDirection,
}

impl Lut1DTransform {
    /// Creates a new 1D LUT transform with identity values.
    pub fn identity(size: usize) -> Self {
        let data: Vec<f32> = (0..size).map(|i| i as f32 / (size - 1) as f32).collect();
        Self {
            red: data,
            green: None,
            blue: None,
            input_min: 0.0,
            input_max: 1.0,
            output_min: 0.0,
            output_max: 1.0,
            half_domain: false,
            raw_halfs: false,
            hue_adjust: Lut1DHueAdjust::None,
            interpolation: Interpolation::Linear,
            direction: TransformDirection::Forward,
        }
    }

    /// Creates a 1D LUT from raw data.
    pub fn from_data(data: Vec<f32>) -> Self {
        Self {
            red: data,
            green: None,
            blue: None,
            input_min: 0.0,
            input_max: 1.0,
            output_min: 0.0,
            output_max: 1.0,
            half_domain: false,
            raw_halfs: false,
            hue_adjust: Lut1DHueAdjust::None,
            interpolation: Interpolation::Linear,
            direction: TransformDirection::Forward,
        }
    }

    /// Creates a 1D LUT with separate RGB channels.
    pub fn from_rgb(red: Vec<f32>, green: Vec<f32>, blue: Vec<f32>) -> Self {
        Self {
            red,
            green: Some(green),
            blue: Some(blue),
            input_min: 0.0,
            input_max: 1.0,
            output_min: 0.0,
            output_max: 1.0,
            half_domain: false,
            raw_halfs: false,
            hue_adjust: Lut1DHueAdjust::None,
            interpolation: Interpolation::Linear,
            direction: TransformDirection::Forward,
        }
    }

    /// Returns the LUT size.
    pub fn size(&self) -> usize {
        self.red.len()
    }

    /// Returns whether this is a single-channel (mono) LUT.
    pub fn is_mono(&self) -> bool {
        self.green.is_none()
    }

    /// Applies the 1D LUT to a single value.
    pub fn apply(&self, value: f32) -> f32 {
        self.interpolate_channel(&self.red, value)
    }

    /// Applies the 1D LUT to RGB values.
    pub fn apply_rgb(&self, rgb: [f32; 3]) -> [f32; 3] {
        let r = self.interpolate_channel(&self.red, rgb[0]);
        let g = self.interpolate_channel(self.green.as_ref().unwrap_or(&self.red), rgb[1]);
        let b = self.interpolate_channel(self.blue.as_ref().unwrap_or(&self.red), rgb[2]);
        [r, g, b]
    }

    fn interpolate_channel(&self, data: &[f32], value: f32) -> f32 {
        if data.is_empty() {
            return value;
        }

        let size = data.len();
        // Same indexing as the compiled op (`ProcessorOp::Lut1d`): one entry per half code.
        if self.half_domain && size == 65536 {
            let (a, b, frac) = crate::processor::half_domain_edges(value);
            return (data[a] - data[b]) * (1.0 - frac) + data[b];
        }
        let range = self.input_max - self.input_min;
        let t = if range.abs() < 1e-10 {
            0.0
        } else {
            ((value - self.input_min) / range).clamp(0.0, 1.0)
        };

        let idx_f = t * (size - 1) as f32;
        let idx0 = (idx_f.floor() as usize).min(size - 1);
        let idx1 = (idx0 + 1).min(size - 1);
        let frac = idx_f - idx0 as f32;

        match self.interpolation {
            Interpolation::Nearest => data[idx0],
            _ => data[idx0] * (1.0 - frac) + data[idx1] * frac,
        }
    }
}

/// Inline 3D LUT transform.
///
/// Contains embedded 3D LUT data rather than a file reference.
/// 3D LUTs map RGB input to RGB output through a cube of color values.
///
/// # Structure
///
/// - Data stored as cube of RGB triplets
/// - `size^3` entries, **R varies fastest, B varies slowest**:
///   `index = b*size*size + g*size + r` (see `get`/`apply_*` below, and the doc on
///   `data` itself). This was previously mis-described here as "R-major order," which
///   conventionally means the opposite (R slowest) — the comment pointed readers the
///   wrong way about the exact convention that scrambles a LUT if got wrong.
/// - **This is the reverse of real OpenColorIO's own `setData`/`getValue` buffer
///   convention**, which is R-slowest/B-fastest
///   (`i = 3*((indexR*gridSize + indexG)*gridSize + indexB)`, OCIO's
///   `PyLut3DTransform.cpp`). Any interop with an OCIO-authored buffer (a Python
///   `setData` binding, a `.cube` file reader, GPU LUT upload, ...) MUST transpose
///   between the two orders — a straight copy produces a LUT that is silently
///   plausible: right length, right type, wrong everywhere. See `oiio-rs/BUG11.md`
///   for the full trail (this is item 2) and `vfx-rs`'s
///   `crates/apps/vfx-rs-py/src/ocio_transforms.rs::grid_index` for the transpose this
///   fact requires on the Python-binding side.
/// - Supports trilinear and tetrahedral interpolation
#[derive(Debug, Clone)]
pub struct Lut3DTransform {
    /// LUT data: RGB triplets, R-fastest/B-slowest (`index = b*size*size + g*size + r`
    /// — see the struct-level doc above for why this is called out explicitly and why
    /// it is NOT the same order as OCIO's own external buffer convention).
    pub data: Vec<[f32; 3]>,
    /// Cube size (e.g., 17, 33, 65).
    pub size: usize,
    /// Input domain minimum per channel.
    pub domain_min: [f32; 3],
    /// Input domain maximum per channel.
    pub domain_max: [f32; 3],
    /// Interpolation method.
    pub interpolation: Interpolation,
    /// Direction.
    pub direction: TransformDirection,
}

impl Lut3DTransform {
    /// Creates a new 3D LUT transform with identity values.
    pub fn identity(size: usize) -> Self {
        let total = size * size * size;
        let mut data = Vec::with_capacity(total);

        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let rf = r as f32 / (size - 1) as f32;
                    let gf = g as f32 / (size - 1) as f32;
                    let bf = b as f32 / (size - 1) as f32;
                    data.push([rf, gf, bf]);
                }
            }
        }

        Self {
            data,
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            interpolation: Interpolation::Tetrahedral,
            direction: TransformDirection::Forward,
        }
    }

    /// Creates a 3D LUT from raw data.
    ///
    /// `data` must already be in THIS struct's own layout — R-fastest/B-slowest
    /// (`index = b*size*size + g*size + r`, see the struct-level doc comment above) —
    /// with exactly `size^3` entries. That is the reverse of OCIO's own external buffer
    /// order; a caller handing this a buffer straight from an OCIO source must transpose
    /// first (`oiio-rs/BUG11.md`).
    pub fn from_data(data: Vec<[f32; 3]>, size: usize) -> Option<Self> {
        let expected = size * size * size;
        if data.len() != expected {
            return None;
        }
        Some(Self {
            data,
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            interpolation: Interpolation::Tetrahedral,
            direction: TransformDirection::Forward,
        })
    }

    /// Sets the input domain.
    pub fn with_domain(mut self, min: [f32; 3], max: [f32; 3]) -> Self {
        self.domain_min = min;
        self.domain_max = max;
        self
    }

    /// Sets the interpolation method.
    pub fn with_interpolation(mut self, interp: Interpolation) -> Self {
        self.interpolation = interp;
        self
    }

    /// Returns the total number of entries.
    pub fn entry_count(&self) -> usize {
        self.size * self.size * self.size
    }

    /// Gets value at grid position (r, g, b).
    fn get(&self, r: usize, g: usize, b: usize) -> [f32; 3] {
        let idx = b * self.size * self.size + g * self.size + r;
        self.data[idx]
    }

    /// Applies the 3D LUT to RGB values, with the interpolation OCIO would use
    /// ([`Interpolation::lut3d_method`]).
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = (self.size - 1) as f32;
        let p: [f32; 3] = std::array::from_fn(|c| {
            let range = self.domain_max[c] - self.domain_min[c];
            (rgb[c] - self.domain_min[c]) / range * n
        });
        vfx_lut::interpolate_lattice(
            self.size,
            p,
            self.interpolation.lut3d_method(),
            |r, g, b| self.get(r, g, b),
        )
    }
}

/// Grading hue curve transform.
///
/// Applies 8 curve types for hue-based color adjustments:
/// - HUE_HUE: shift hue based on input hue
/// - HUE_SAT: adjust saturation based on hue
/// - HUE_LUM: adjust luminance based on hue
/// - LUM_SAT: adjust saturation based on luminance
/// - SAT_SAT: adjust saturation based on saturation
/// - LUM_LUM: adjust luminance based on luminance
/// - SAT_LUM: adjust luminance based on saturation
/// - HUE_FX: special effects hue shift
///
/// Reference: OCIO GradingHueCurveTransform
#[derive(Debug, Clone)]
pub struct GradingHueCurveTransform {
    /// The transform's name (OCIO `FormatMetadata` name, the YAML `name` key). Kept for the
    /// config round trip; it does not affect the pixels.
    pub name: String,
    /// Grading style (Log, Linear, Video).
    pub style: GradingHueCurveStyle,
    /// RGB to HSY conversion style (OCIO parity).
    /// Controls whether internal RGB->HSY conversion is applied.
    pub rgb_to_hsy: HsyTransformStyle,
    /// HUE_HUE curve control points [(hue, shift), ...].
    pub hue_hue: Vec<[f64; 2]>,
    /// HUE_SAT curve control points [(hue, gain), ...].
    pub hue_sat: Vec<[f64; 2]>,
    /// HUE_LUM curve control points [(hue, gain), ...].
    pub hue_lum: Vec<[f64; 2]>,
    /// LUM_SAT curve control points [(lum, gain), ...].
    pub lum_sat: Vec<[f64; 2]>,
    /// SAT_SAT curve control points [(sat, sat), ...].
    pub sat_sat: Vec<[f64; 2]>,
    /// LUM_LUM curve control points [(lum, lum), ...].
    pub lum_lum: Vec<[f64; 2]>,
    /// SAT_LUM curve control points [(sat, gain), ...].
    pub sat_lum: Vec<[f64; 2]>,
    /// HUE_FX curve control points [(hue, shift), ...].
    pub hue_fx: Vec<[f64; 2]>,
    /// Optional slopes for HUE_HUE curve.
    /// If empty, slopes are auto-computed from control points.
    pub hue_hue_slopes: Vec<f64>,
    /// Optional slopes for HUE_SAT curve.
    pub hue_sat_slopes: Vec<f64>,
    /// Optional slopes for HUE_LUM curve.
    pub hue_lum_slopes: Vec<f64>,
    /// Optional slopes for LUM_SAT curve.
    pub lum_sat_slopes: Vec<f64>,
    /// Optional slopes for SAT_SAT curve.
    pub sat_sat_slopes: Vec<f64>,
    /// Optional slopes for LUM_LUM curve.
    pub lum_lum_slopes: Vec<f64>,
    /// Optional slopes for SAT_LUM curve.
    pub sat_lum_slopes: Vec<f64>,
    /// Optional slopes for HUE_FX curve.
    pub hue_fx_slopes: Vec<f64>,
    /// Draw-curve-only mode (UI graphing; bypasses HSY/log math).
    pub draw_curve_only: bool,
    /// Direction.
    pub direction: TransformDirection,
}

/// Grading hue curve style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradingHueCurveStyle {
    /// Log style (for log-encoded footage).
    #[default]
    Log,
    /// Linear style (for scene-linear footage).
    Linear,
    /// Video style (for display-referred footage).
    Video,
}

/// HSY Transform style (OCIO parity).
/// Controls whether RGB->HSY conversion is applied before hue curve processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HsyTransformStyle {
    /// No HSY conversion - bypass RGB->HSY (useful for applying external HSY transform).
    None,
    /// Default HSY conversion (standard RGB->HSY formula).
    #[default]
    Default,
}

impl Default for GradingHueCurveTransform {
    fn default() -> Self {
        Self::new(GradingHueCurveStyle::Log)
    }
}

impl GradingHueCurveTransform {
    /// An identity transform of a style (OCIO `GradingHueCurveTransform::Create(style)`,
    /// `GradingHueCurveImpl::DefaultCurves` / `DefaultCurvesLin`, float control points).
    pub fn new(style: GradingHueCurveStyle) -> Self {
        // Identity curves
        let sixth = |k: f64| (k / 6.0) as f32 as f64;
        let hue_6pts_identity = |v: f64| {
            vec![
                [0.0, v],
                [sixth(1.0), v],
                [sixth(2.0), v],
                [0.5, v],
                [sixth(4.0), v],
                [sixth(5.0), v],
            ]
        };
        let hue_hue_identity = (0..6).map(|k| [sixth(k as f64), sixth(k as f64)]).collect();
        let diag = vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]];
        let horiz_1 = vec![[0.0, 1.0], [0.5, 1.0], [1.0, 1.0]];
        let lin = style == GradingHueCurveStyle::Linear;

        Self {
            name: String::new(),
            style,
            rgb_to_hsy: HsyTransformStyle::Default,
            hue_hue: hue_hue_identity,
            hue_sat: hue_6pts_identity(1.0),
            hue_lum: hue_6pts_identity(1.0),
            lum_sat: if lin {
                vec![[-7.0, 1.0], [0.0, 1.0], [7.0, 1.0]]
            } else {
                horiz_1.clone()
            },
            sat_sat: diag.clone(),
            lum_lum: if lin {
                vec![[-7.0, -7.0], [0.0, 0.0], [7.0, 7.0]]
            } else {
                diag
            },
            sat_lum: horiz_1,
            hue_fx: hue_6pts_identity(0.0),
            hue_hue_slopes: Vec::new(),
            hue_sat_slopes: Vec::new(),
            hue_lum_slopes: Vec::new(),
            lum_sat_slopes: Vec::new(),
            sat_sat_slopes: Vec::new(),
            lum_lum_slopes: Vec::new(),
            sat_lum_slopes: Vec::new(),
            hue_fx_slopes: Vec::new(),
            draw_curve_only: false,
            direction: TransformDirection::Forward,
        }
    }
}

impl GradingHueCurveTransform {
    /// Returns true if slopes are using default (auto-computed) values.
    /// OCIO ref: slopesAreDefault(HueCurveType).
    pub fn slopes_are_default(&self, curve: HueCurveType) -> bool {
        match curve {
            HueCurveType::HueHue => self.hue_hue_slopes.is_empty(),
            HueCurveType::HueSat => self.hue_sat_slopes.is_empty(),
            HueCurveType::HueLum => self.hue_lum_slopes.is_empty(),
            HueCurveType::LumSat => self.lum_sat_slopes.is_empty(),
            HueCurveType::SatSat => self.sat_sat_slopes.is_empty(),
            HueCurveType::LumLum => self.lum_lum_slopes.is_empty(),
            HueCurveType::SatLum => self.sat_lum_slopes.is_empty(),
            HueCurveType::HueFx => self.hue_fx_slopes.is_empty(),
        }
    }
}

/// Hue curve type for slope access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HueCurveType {
    /// Hue-to-hue curve.
    HueHue,
    /// Hue-to-saturation curve.
    HueSat,
    /// Hue-to-luminance curve.
    HueLum,
    /// Luminance-to-saturation curve.
    LumSat,
    /// Saturation-to-saturation curve.
    SatSat,
    /// Luminance-to-luminance curve.
    LumLum,
    /// Saturation-to-luminance curve.
    SatLum,
    /// Hue FX curve.
    HueFx,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_inverse() {
        assert_eq!(
            TransformDirection::Forward.inverse(),
            TransformDirection::Inverse
        );
        assert_eq!(
            TransformDirection::Inverse.inverse(),
            TransformDirection::Forward
        );
    }

    #[test]
    fn matrix_transform() {
        let t = Transform::matrix(MatrixTransform::IDENTITY);
        if let Transform::Matrix(m) = t {
            assert_eq!(m.matrix[0], 1.0);
            assert_eq!(m.direction, TransformDirection::Forward);
        }
    }

    #[test]
    fn group_inverse() {
        let g = Transform::group(vec![
            Transform::matrix(MatrixTransform::IDENTITY),
            Transform::file("test.spi1d"),
        ]);
        let inv = g.inverse();
        if let Transform::Group(g) = inv {
            assert_eq!(g.direction, TransformDirection::Inverse);
            assert_eq!(g.transforms.len(), 2);
        }
    }

    #[test]
    fn lut1d_identity() {
        let lut = Lut1DTransform::identity(256);
        assert_eq!(lut.size(), 256);
        assert!(lut.is_mono());

        // Identity should pass through values
        assert!((lut.apply(0.0) - 0.0).abs() < 0.01);
        assert!((lut.apply(0.5) - 0.5).abs() < 0.01);
        assert!((lut.apply(1.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn lut1d_rgb() {
        let lut = Lut1DTransform::identity(256);
        let result = lut.apply_rgb([0.5, 0.3, 0.8]);
        assert!((result[0] - 0.5).abs() < 0.01);
        assert!((result[1] - 0.3).abs() < 0.01);
        assert!((result[2] - 0.8).abs() < 0.01);
    }

    #[test]
    fn lut1d_inverse() {
        let lut = Transform::Lut1D(Lut1DTransform::identity(256));
        let inv = lut.inverse();
        if let Transform::Lut1D(l) = inv {
            assert_eq!(l.direction, TransformDirection::Inverse);
        }
    }

    #[test]
    fn lut3d_identity() {
        let lut = Lut3DTransform::identity(17);
        assert_eq!(lut.size, 17);
        assert_eq!(lut.entry_count(), 17 * 17 * 17);

        // Identity should pass through values
        let result = lut.apply([0.5, 0.3, 0.8]);
        assert!((result[0] - 0.5).abs() < 0.1);
        assert!((result[1] - 0.3).abs() < 0.1);
        assert!((result[2] - 0.8).abs() < 0.1);
    }

    #[test]
    fn lut3d_corners() {
        let lut = Lut3DTransform::identity(33);

        // Black
        let black = lut.apply([0.0, 0.0, 0.0]);
        assert!(black[0].abs() < 0.01);

        // White
        let white = lut.apply([1.0, 1.0, 1.0]);
        assert!((white[0] - 1.0).abs() < 0.01);

        // Red
        let red = lut.apply([1.0, 0.0, 0.0]);
        assert!((red[0] - 1.0).abs() < 0.01);
        assert!(red[1].abs() < 0.01);
    }

    #[test]
    fn lut3d_from_data() {
        // Every entry maps to a CONSTANT that differs from the input: a real LUT
        // apply returns the stored value, a no-op/passthrough returns the input.
        // The old fixture stored [0.5,0.5,0.5] and probed [0.5,0.5,0.5], so BOTH
        // a passthrough AND a constant-return passed. Distinct per channel also
        // catches a channel mix-up.
        let data: Vec<[f32; 3]> = (0..8).map(|_| [0.2, 0.4, 0.6]).collect();
        let lut = Lut3DTransform::from_data(data, 2).unwrap();
        let result = lut.apply([0.5, 0.5, 0.5]);
        assert!((result[0] - 0.2).abs() < 1e-5, "r={}", result[0]);
        assert!((result[1] - 0.4).abs() < 1e-5, "g={}", result[1]);
        assert!((result[2] - 0.6).abs() < 1e-5, "b={}", result[2]);
    }

    #[test]
    fn lut3d_inverse() {
        let lut = Transform::Lut3D(Lut3DTransform::identity(17));
        let inv = lut.inverse();
        if let Transform::Lut3D(l) = inv {
            assert_eq!(l.direction, TransformDirection::Inverse);
        }
    }
}
