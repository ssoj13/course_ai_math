//! Dynamic properties for runtime parameter adjustment.
//!
//! OCIO's mechanism is [`EcDynamic`](crate::EcDynamic) flags on an
//! `ExposureContrastTransform`, moved with `Processor::set_dynamic` (CPU) or the uniform named by
//! `GpuProcessor::dynamic_uniforms` (GPU); `LegacyViewingPipeline` builds the usual viewer
//! chain with them. [`DynamicProcessor`] below is an older, non-OCIO wrapper that bolts its
//! own exposure/contrast/gamma/saturation stage onto a finished processor; it is deprecated.
//!
//! # Example (the deprecated wrapper)
//!
//! ```no_run
//! # #![allow(deprecated)]
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_ocio::{Config, DynamicProcessor};
//!
//! let config = Config::from_file("config.ocio")?;
//! let processor = config.processor("ACEScg", "sRGB")?;
//!
//! // Create dynamic wrapper
//! let mut dynamic = DynamicProcessor::new(processor);
//!
//! // Adjust exposure in real-time
//! dynamic.set_exposure(1.5);
//! dynamic.set_contrast(1.1);
//! dynamic.set_gamma(1.0);
//!
//! // Apply with dynamic adjustments
//! let mut pixels = [[0.18_f32, 0.18, 0.18]; 100];
//! dynamic.apply_rgb(&mut pixels);
//! # Ok(())
//! # }
//! ```

// `DynamicProcessor` is deprecated but still tested and used here.
#![allow(deprecated)]

use vfx_core::pixel::{REC709_LUMA_B, REC709_LUMA_G, REC709_LUMA_R};

use crate::grading_hue_curve::GradingHueCurves;
use crate::grading_tone::GradingTone;
use crate::processor::Processor;
use crate::transform::GradingRgbCurveTransform;

/// Dynamic property types that can be adjusted at runtime.
/// OCIO ref: OpenColorTypes.h DynamicPropertyType enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DynamicPropertyType {
    /// Image exposure value (double floating point value).
    /// OCIO: DYNAMIC_PROPERTY_EXPOSURE
    Exposure = 0,
    /// Image contrast value (double floating point value).
    /// OCIO: DYNAMIC_PROPERTY_CONTRAST
    Contrast = 1,
    /// Image gamma value (double floating point value).
    /// OCIO: DYNAMIC_PROPERTY_GAMMA
    Gamma = 2,
    /// Used by GradingPrimaryTransform.
    /// OCIO: DYNAMIC_PROPERTY_GRADING_PRIMARY
    GradingPrimary = 3,
    /// Used by GradingRGBCurveTransform.
    /// OCIO: DYNAMIC_PROPERTY_GRADING_RGBCURVE
    GradingRgbCurve = 4,
    /// Used by GradingToneTransform.
    /// OCIO: DYNAMIC_PROPERTY_GRADING_TONE
    GradingTone = 5,
    /// Used by GradingHueCurveTransform.
    /// OCIO: DYNAMIC_PROPERTY_GRADING_HUECURVE
    GradingHueCurve = 6,
}

/// Grading primary values for runtime adjustment.
///
/// OCIO ref: `GradingPrimary` struct, `OpenColorTransforms.h:471-477` — all
/// seven of brightness/contrast/gamma/offset/exposure/lift/gain are
/// `GradingRGBM` (per-channel + master), which this struct now matches. Not
/// yet wired to pixel application: `DynamicPropertyValue::GradingPrimary`'s
/// only consumers are this module's own accessors — the LIVE grading-primary
/// pixel path is `GradingPrimaryTransform` (`transform.rs`) ->
/// `ProcessorOp::GradingPrimary` (`processor/op.rs`), which has its own,
/// narrower shape (no `master`, `slope` in place of `lift`+`gain`). See
/// `docs/ROADMAP.md`'s GradingPrimary item for the unification plan.
#[derive(Debug, Clone, PartialEq)]
pub struct GradingPrimary {
    /// Brightness adjustment per channel.
    pub brightness: GradingRgbm,
    /// Contrast adjustment per channel.
    pub contrast: GradingRgbm,
    /// Gamma adjustment per channel.
    pub gamma: GradingRgbm,
    /// Offset adjustment per channel.
    pub offset: GradingRgbm,
    /// Exposure adjustment in stops, per channel.
    pub exposure: GradingRgbm,
    /// Lift adjustment (shadows), per channel.
    pub lift: GradingRgbm,
    /// Gain adjustment (highlights), per channel.
    pub gain: GradingRgbm,
    /// Saturation adjustment.
    pub saturation: f64,
    /// Pivot for contrast/exposure operations.
    pub pivot: f64,
    /// Whether pivot is in black space (log) or scene-linear.
    pub pivot_black: f64,
    /// White point for log operations.
    pub pivot_white: f64,
    /// Clamp to black level.
    pub clamp_black: f64,
    /// Clamp to white level.
    pub clamp_white: f64,
}

impl Default for GradingPrimary {
    fn default() -> Self {
        Self {
            brightness: GradingRgbm::default(),
            contrast: GradingRgbm::one(),
            gamma: GradingRgbm::one(),
            offset: GradingRgbm::default(),
            exposure: GradingRgbm::default(),
            lift: GradingRgbm::default(),
            gain: GradingRgbm::one(),
            saturation: 1.0,
            pivot: 0.18, // OCIO default for LIN/VIDEO (`GradingPrimary(GradingStyle)`,
            // `OpenColorTransforms.h:464-469`); LOG's default is -0.2, see `log_default()`.
            pivot_black: 0.0,
            pivot_white: 1.0,
            // `GradingPrimary::NoClampBlack()`/`NoClampWhite()` (`GradingPrimary.cpp:47-55`)
            // are `-DBL_MAX`/`DBL_MAX` — "no clamp" is symmetric. `clamp_black: 0.0` here
            // used to mean every default-constructed GradingPrimary silently clamped
            // negative values to zero.
            clamp_black: -f64::MAX,
            clamp_white: f64::MAX,
        }
    }
}

impl GradingPrimary {
    /// Creates default grading primary for video style (OCIO:
    /// `GradingPrimary(GRADING_VIDEO)`, pivot 0.18 — same as `default()`).
    pub fn video_default() -> Self {
        Self::default()
    }

    /// Creates default grading primary for linear style (OCIO:
    /// `GradingPrimary(GRADING_LIN)`, pivot 0.18 — same as `default()`).
    pub fn linear_default() -> Self {
        Self::default()
    }

    /// Creates default grading primary for log style (OCIO:
    /// `GradingPrimary(GRADING_LOG)`, pivot -0.2 —
    /// `OpenColorTransforms.h:464-469`).
    pub fn log_default() -> Self {
        Self {
            pivot: -0.2,
            ..Default::default()
        }
    }

    /// Checks if all values are at identity (no effect).
    ///
    /// Mirrors OCIO's `GradingPrimaryPreRender::m_localBypass`
    /// (`GradingPrimary.cpp:104-108`): identity also requires the clamp to
    /// be at the "no clamp" sentinel — a value with real clamp bounds is
    /// NOT identity even if every other field is, since it still restricts
    /// the output range.
    pub fn is_identity(&self) -> bool {
        self.brightness.is_zero()
            && self.contrast.is_one()
            && self.gamma.is_one()
            && self.offset.is_zero()
            && self.exposure.is_zero()
            && self.lift.is_zero()
            && self.gain.is_one()
            && (self.saturation - 1.0).abs() < 1e-9
            && self.clamp_black <= -f64::MAX
            && self.clamp_white >= f64::MAX
    }

    /// OCIO `GradingPrimary::validate(GradingStyle)` (`GradingPrimary.cpp:60-102`), same
    /// bounds and messages: gamma (not LIN) and contrast (LIN) at least 0.01 on every channel
    /// including master, `pivot_white - pivot_black` at least 0.01, `clamp_black <= clamp_white`.
    ///
    /// # Errors
    ///
    /// [`crate::OcioError::Validation`] with OCIO's message.
    pub fn validate(&self, style: crate::transform::GradingPrimaryStyle) -> crate::OcioResult<()> {
        use crate::transform::GradingPrimaryStyle;
        const LOWER_BOUND: f64 = 0.01;
        const MIN: f64 = LOWER_BOUND - 0.000001;
        let below =
            |v: &GradingRgbm| v.red < MIN || v.green < MIN || v.blue < MIN || v.master < MIN;
        let fmt =
            |v: &GradingRgbm| format!("<r={}, g={}, b={}, m={}>", v.red, v.green, v.blue, v.master);
        if style != GradingPrimaryStyle::Linear && below(&self.gamma) {
            return Err(crate::OcioError::Validation(format!(
                "GradingPrimary gamma '{}' are below lower bound ({LOWER_BOUND}).",
                fmt(&self.gamma)
            )));
        }
        if style == GradingPrimaryStyle::Linear && below(&self.contrast) {
            return Err(crate::OcioError::Validation(format!(
                "GradingPrimary contrast '{}' are below lower bound ({LOWER_BOUND}).",
                fmt(&self.contrast)
            )));
        }
        if self.pivot_white - self.pivot_black < MIN {
            return Err(crate::OcioError::Validation(
                "GradingPrimary black pivot should be smaller than white pivot.".to_string(),
            ));
        }
        if self.clamp_black > self.clamp_white {
            return Err(crate::OcioError::Validation(
                "GradingPrimary black clamp should be smaller than white clamp.".to_string(),
            ));
        }
        Ok(())
    }
}

/// RGB + Master values for grading.
/// OCIO ref: GradingRGBM.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradingRgbm {
    /// Red channel value.
    pub red: f64,
    /// Green channel value.
    pub green: f64,
    /// Blue channel value.
    pub blue: f64,
    /// Master (all channels) value.
    pub master: f64,
}

impl Default for GradingRgbm {
    fn default() -> Self {
        Self {
            red: 0.0,
            green: 0.0,
            blue: 0.0,
            master: 0.0,
        }
    }
}

impl GradingRgbm {
    /// Creates RGBM with all values set to 1.0.
    pub fn one() -> Self {
        Self {
            red: 1.0,
            green: 1.0,
            blue: 1.0,
            master: 1.0,
        }
    }

    /// Creates RGBM from individual values.
    pub fn new(red: f64, green: f64, blue: f64, master: f64) -> Self {
        Self {
            red,
            green,
            blue,
            master,
        }
    }

    /// Checks if all values are zero.
    pub fn is_zero(&self) -> bool {
        self.red.abs() < 1e-9
            && self.green.abs() < 1e-9
            && self.blue.abs() < 1e-9
            && self.master.abs() < 1e-9
    }

    /// Checks if all values are one.
    pub fn is_one(&self) -> bool {
        (self.red - 1.0).abs() < 1e-9
            && (self.green - 1.0).abs() < 1e-9
            && (self.blue - 1.0).abs() < 1e-9
            && (self.master - 1.0).abs() < 1e-9
    }
}

/// Dynamic property value that can be accessed and modified at runtime.
/// OCIO ref: DynamicProperty base class and derived interfaces.
#[derive(Debug, Clone)]
pub enum DynamicPropertyValue {
    /// Double precision floating point value (exposure, contrast, gamma).
    Double(f64),
    /// Double precision floating point value with an explicit dynamic property type.
    TypedDouble {
        /// Dynamic property type associated with the value.
        prop_type: DynamicPropertyType,
        /// Stored double value.
        value: f64,
    },
    /// Grading primary parameters.
    GradingPrimary(GradingPrimary),
    /// Grading RGB curve parameters.
    GradingRgbCurve(GradingRgbCurveTransform),
    /// Grading tone parameters.
    GradingTone(GradingTone),
    /// Grading hue curve parameters.
    GradingHueCurve(GradingHueCurves),
}

impl DynamicPropertyValue {
    /// Returns the value as a double, if applicable.
    pub fn as_double(&self) -> Option<f64> {
        match self {
            DynamicPropertyValue::Double(v) => Some(*v),
            DynamicPropertyValue::TypedDouble { value, .. } => Some(*value),
            _ => None,
        }
    }

    /// Returns the value as a typed double, if applicable.
    pub fn as_typed_double(&self) -> Option<(DynamicPropertyType, f64)> {
        match self {
            DynamicPropertyValue::TypedDouble { prop_type, value } => Some((*prop_type, *value)),
            _ => None,
        }
    }

    /// Returns the value as GradingPrimary, if applicable.
    pub fn as_grading_primary(&self) -> Option<&GradingPrimary> {
        match self {
            DynamicPropertyValue::GradingPrimary(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the value as mutable GradingPrimary, if applicable.
    pub fn as_grading_primary_mut(&mut self) -> Option<&mut GradingPrimary> {
        match self {
            DynamicPropertyValue::GradingPrimary(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the value as GradingRgbCurve, if applicable.
    pub fn as_grading_rgb_curve(&self) -> Option<&GradingRgbCurveTransform> {
        match self {
            DynamicPropertyValue::GradingRgbCurve(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the value as mutable GradingRgbCurve, if applicable.
    pub fn as_grading_rgb_curve_mut(&mut self) -> Option<&mut GradingRgbCurveTransform> {
        match self {
            DynamicPropertyValue::GradingRgbCurve(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the value as GradingTone, if applicable.
    pub fn as_grading_tone(&self) -> Option<&GradingTone> {
        match self {
            DynamicPropertyValue::GradingTone(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the value as mutable GradingTone, if applicable.
    pub fn as_grading_tone_mut(&mut self) -> Option<&mut GradingTone> {
        match self {
            DynamicPropertyValue::GradingTone(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the value as GradingHueCurve, if applicable.
    pub fn as_grading_hue_curve(&self) -> Option<&GradingHueCurves> {
        match self {
            DynamicPropertyValue::GradingHueCurve(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the value as mutable GradingHueCurve, if applicable.
    pub fn as_grading_hue_curve_mut(&mut self) -> Option<&mut GradingHueCurves> {
        match self {
            DynamicPropertyValue::GradingHueCurve(v) => Some(v),
            _ => None,
        }
    }

    /// Returns the property type for this value.
    pub fn property_type(&self) -> DynamicPropertyType {
        match self {
            // For doubles, we need context to know which type; default to Exposure
            DynamicPropertyValue::Double(_) => DynamicPropertyType::Exposure,
            DynamicPropertyValue::TypedDouble { prop_type, .. } => *prop_type,
            DynamicPropertyValue::GradingPrimary(_) => DynamicPropertyType::GradingPrimary,
            DynamicPropertyValue::GradingRgbCurve(_) => DynamicPropertyType::GradingRgbCurve,
            DynamicPropertyValue::GradingTone(_) => DynamicPropertyType::GradingTone,
            DynamicPropertyValue::GradingHueCurve(_) => DynamicPropertyType::GradingHueCurve,
        }
    }
}

/// A processor wrapper that supports runtime parameter adjustment.
///
/// **Deprecated.** This is not OCIO's dynamic property mechanism: it applies its own
/// exposure/contrast/gamma/saturation stage (not `ExposureContrastOpData`'s maths) before or
/// after the finished processor, and cannot be compiled to a shader. Use an
/// `ExposureContrastTransform` with [`EcDynamic`](crate::EcDynamic) flags (or a
/// `LegacyViewingPipeline`), `Processor::set_dynamic` and `GpuProcessor::dynamic_uniforms`.
///
/// This wraps a base processor and applies additional exposure/contrast/gamma
/// adjustments that can be modified without rebuilding the transform chain.
#[deprecated(
    since = "0.1.0",
    note = "use EcDynamic flags on an ExposureContrastTransform with Processor::set_dynamic / GpuProcessor::dynamic_uniforms (or LegacyViewingPipeline)"
)]
#[derive(Debug)]
pub struct DynamicProcessor {
    /// Base processor (immutable).
    base: Processor,
    /// Exposure adjustment in stops (0 = no change).
    exposure: f32,
    /// Contrast multiplier (1.0 = no change).
    contrast: f32,
    /// Gamma power (1.0 = no change).
    gamma: f32,
    /// Saturation multiplier (1.0 = no change).
    saturation: f32,
    /// Pivot point for contrast (typically 0.18 for scene-linear).
    pivot: f32,
    /// Whether to apply adjustments before or after base transform.
    apply_before: bool,
}

impl DynamicProcessor {
    /// Creates a new dynamic processor wrapping the given base processor.
    pub fn new(base: Processor) -> Self {
        Self {
            base,
            exposure: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            saturation: 1.0,
            pivot: 0.18,
            apply_before: false,
        }
    }

    /// Sets exposure adjustment in stops.
    ///
    /// - 0.0 = no change
    /// - 1.0 = +1 stop (2x brighter)
    /// - -1.0 = -1 stop (2x darker)
    pub fn set_exposure(&mut self, stops: f32) {
        self.exposure = stops;
    }

    /// Returns current exposure adjustment.
    pub fn exposure(&self) -> f32 {
        self.exposure
    }

    /// Sets contrast multiplier.
    ///
    /// - 1.0 = no change
    /// - >1.0 = increased contrast
    /// - <1.0 = decreased contrast
    pub fn set_contrast(&mut self, contrast: f32) {
        self.contrast = contrast;
    }

    /// Returns current contrast multiplier.
    pub fn contrast(&self) -> f32 {
        self.contrast
    }

    /// Sets gamma (power) adjustment.
    ///
    /// - 1.0 = no change
    /// - <1.0 = brighter midtones
    /// - >1.0 = darker midtones
    pub fn set_gamma(&mut self, gamma: f32) {
        self.gamma = gamma.max(0.01); // Prevent division by zero
    }

    /// Returns current gamma value.
    pub fn gamma(&self) -> f32 {
        self.gamma
    }

    /// Sets saturation multiplier.
    ///
    /// - 1.0 = no change
    /// - 0.0 = grayscale
    /// - >1.0 = increased saturation
    pub fn set_saturation(&mut self, saturation: f32) {
        self.saturation = saturation.max(0.0);
    }

    /// Returns current saturation multiplier.
    pub fn saturation(&self) -> f32 {
        self.saturation
    }

    /// Sets the contrast pivot point.
    ///
    /// Default is 0.18 (18% gray in scene-linear).
    pub fn set_pivot(&mut self, pivot: f32) {
        self.pivot = pivot;
    }

    /// Returns current pivot point.
    pub fn pivot(&self) -> f32 {
        self.pivot
    }

    /// Sets whether to apply dynamic adjustments before or after base transform.
    ///
    /// - `true`: Apply exposure/contrast/gamma BEFORE base transform
    /// - `false`: Apply AFTER base transform (default)
    pub fn set_apply_before(&mut self, before: bool) {
        self.apply_before = before;
    }

    /// Returns whether adjustments are applied before base transform.
    pub fn apply_before(&self) -> bool {
        self.apply_before
    }

    /// Resets all dynamic properties to their default values.
    pub fn reset(&mut self) {
        self.exposure = 0.0;
        self.contrast = 1.0;
        self.gamma = 1.0;
        self.saturation = 1.0;
    }

    /// Returns true if any dynamic property is modified from default.
    pub fn has_adjustments(&self) -> bool {
        self.exposure.abs() > 1e-6
            || (self.contrast - 1.0).abs() > 1e-6
            || (self.gamma - 1.0).abs() > 1e-6
            || (self.saturation - 1.0).abs() > 1e-6
    }

    /// Applies the dynamic adjustments to a single RGB pixel.
    #[inline]
    fn apply_dynamic(&self, pixel: &mut [f32; 3]) {
        // Exposure (2^stops)
        if self.exposure.abs() > 1e-6 {
            let mult = 2.0_f32.powf(self.exposure);
            pixel[0] *= mult;
            pixel[1] *= mult;
            pixel[2] *= mult;
        }

        // Contrast (around pivot)
        if (self.contrast - 1.0).abs() > 1e-6 {
            pixel[0] = self.pivot + (pixel[0] - self.pivot) * self.contrast;
            pixel[1] = self.pivot + (pixel[1] - self.pivot) * self.contrast;
            pixel[2] = self.pivot + (pixel[2] - self.pivot) * self.contrast;
        }

        // Gamma
        if (self.gamma - 1.0).abs() > 1e-6 {
            let inv_gamma = 1.0 / self.gamma;
            pixel[0] = pixel[0].max(0.0).powf(inv_gamma);
            pixel[1] = pixel[1].max(0.0).powf(inv_gamma);
            pixel[2] = pixel[2].max(0.0).powf(inv_gamma);
        }

        // Saturation
        if (self.saturation - 1.0).abs() > 1e-6 {
            let luma =
                REC709_LUMA_R * pixel[0] + REC709_LUMA_G * pixel[1] + REC709_LUMA_B * pixel[2];
            pixel[0] = luma + (pixel[0] - luma) * self.saturation;
            pixel[1] = luma + (pixel[1] - luma) * self.saturation;
            pixel[2] = luma + (pixel[2] - luma) * self.saturation;
        }
    }

    /// Applies the processor with dynamic adjustments to RGB pixel data.
    pub fn apply_rgb(&self, pixels: &mut [[f32; 3]]) {
        if self.apply_before {
            // Dynamic first, then base
            if self.has_adjustments() {
                for pixel in pixels.iter_mut() {
                    self.apply_dynamic(pixel);
                }
            }
            self.base.apply_rgb(pixels);
        } else {
            // Base first, then dynamic
            self.base.apply_rgb(pixels);
            if self.has_adjustments() {
                for pixel in pixels.iter_mut() {
                    self.apply_dynamic(pixel);
                }
            }
        }
    }

    /// Applies the processor with dynamic adjustments to RGBA pixel data.
    pub fn apply_rgba(&self, pixels: &mut [[f32; 4]]) {
        if self.apply_before {
            // Dynamic first, then base
            if self.has_adjustments() {
                for pixel in pixels.iter_mut() {
                    let mut rgb = [pixel[0], pixel[1], pixel[2]];
                    self.apply_dynamic(&mut rgb);
                    pixel[0] = rgb[0];
                    pixel[1] = rgb[1];
                    pixel[2] = rgb[2];
                }
            }
            self.base.apply_rgba(pixels);
        } else {
            // Base first, then dynamic
            self.base.apply_rgba(pixels);
            if self.has_adjustments() {
                for pixel in pixels.iter_mut() {
                    let mut rgb = [pixel[0], pixel[1], pixel[2]];
                    self.apply_dynamic(&mut rgb);
                    pixel[0] = rgb[0];
                    pixel[1] = rgb[1];
                    pixel[2] = rgb[2];
                }
            }
        }
    }

    /// Returns a reference to the base processor.
    pub fn base(&self) -> &Processor {
        &self.base
    }

    /// Consumes this and returns the base processor.
    pub fn into_base(self) -> Processor {
        self.base
    }
}

/// Builder for configuring dynamic processor settings. Deprecated with [`DynamicProcessor`].
#[deprecated(
    since = "0.1.0",
    note = "use EcDynamic flags on an ExposureContrastTransform with Processor::set_dynamic / GpuProcessor::dynamic_uniforms (or LegacyViewingPipeline)"
)]
#[derive(Debug, Clone)]
pub struct DynamicProcessorBuilder {
    exposure: f32,
    contrast: f32,
    gamma: f32,
    saturation: f32,
    pivot: f32,
    apply_before: bool,
}

impl Default for DynamicProcessorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicProcessorBuilder {
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self {
            exposure: 0.0,
            contrast: 1.0,
            gamma: 1.0,
            saturation: 1.0,
            pivot: 0.18,
            apply_before: false,
        }
    }

    /// Sets initial exposure.
    pub fn exposure(mut self, stops: f32) -> Self {
        self.exposure = stops;
        self
    }

    /// Sets initial contrast.
    pub fn contrast(mut self, contrast: f32) -> Self {
        self.contrast = contrast;
        self
    }

    /// Sets initial gamma.
    pub fn gamma(mut self, gamma: f32) -> Self {
        self.gamma = gamma.max(0.01);
        self
    }

    /// Sets initial saturation.
    pub fn saturation(mut self, saturation: f32) -> Self {
        self.saturation = saturation.max(0.0);
        self
    }

    /// Sets pivot point.
    pub fn pivot(mut self, pivot: f32) -> Self {
        self.pivot = pivot;
        self
    }

    /// Sets whether to apply before base transform.
    pub fn apply_before(mut self, before: bool) -> Self {
        self.apply_before = before;
        self
    }

    /// Builds the dynamic processor with the given base.
    pub fn build(self, base: Processor) -> DynamicProcessor {
        DynamicProcessor {
            base,
            exposure: self.exposure,
            contrast: self.contrast,
            gamma: self.gamma,
            saturation: self.saturation,
            pivot: self.pivot,
            apply_before: self.apply_before,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transform::{MatrixTransform, Transform, TransformDirection};

    /// OCIO `GradingPrimary_tests.cpp:46-79` (`GradingPrimary, validate`), same values and
    /// messages, plus the pivot and clamp checks of `GradingPrimary.cpp:93-101`.
    #[test]
    fn grading_primary_validate_matches_ocio() {
        use crate::transform::GradingPrimaryStyle::{Linear, Log, Video};
        let msg = |r: crate::OcioResult<()>| match r {
            Err(crate::OcioError::Validation(m)) => m,
            other => panic!("expected a validation error, got {other:?}"),
        };
        let mut gp = GradingPrimary::log_default();
        assert!(gp.validate(Log).is_ok());
        gp.gamma.red = 0.0001;
        let gamma =
            "GradingPrimary gamma '<r=0.0001, g=1, b=1, m=1>' are below lower bound (0.01).";
        assert_eq!(msg(gp.validate(Log)), gamma);
        assert_eq!(msg(gp.validate(Video)), gamma);
        assert!(gp.validate(Linear).is_ok(), "LIN does not use gamma");
        gp.gamma.red = 1.0;
        gp.contrast.green = 0.0001;
        assert_eq!(
            msg(gp.validate(Linear)),
            "GradingPrimary contrast '<r=1, g=0.0001, b=1, m=1>' are below lower bound (0.01)."
        );
        assert!(gp.validate(Log).is_ok() && gp.validate(Video).is_ok());
        gp.contrast.green = 1.0;
        gp.pivot_white = gp.pivot_black;
        assert_eq!(
            msg(gp.validate(Log)),
            "GradingPrimary black pivot should be smaller than white pivot."
        );
        gp.pivot_white = 1.0;
        gp.clamp_black = 0.5;
        gp.clamp_white = 0.4;
        assert_eq!(
            msg(gp.validate(Log)),
            "GradingPrimary black clamp should be smaller than white clamp."
        );
    }

    fn identity_processor() -> Processor {
        let identity = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0, 0.0, 0.0, 0.0],
            direction: TransformDirection::Forward,
        });
        Processor::from_transform(&identity, TransformDirection::Forward).unwrap()
    }

    #[test]
    fn exposure_adjustment() {
        let proc = identity_processor();
        let mut dynamic = DynamicProcessor::new(proc);

        // +1 stop should double values
        dynamic.set_exposure(1.0);

        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        dynamic.apply_rgb(&mut pixels);

        assert!((pixels[0][0] - 1.0).abs() < 0.001);
        assert!((pixels[0][1] - 1.0).abs() < 0.001);
        assert!((pixels[0][2] - 1.0).abs() < 0.001);
    }

    #[test]
    fn contrast_adjustment() {
        let proc = identity_processor();
        let mut dynamic = DynamicProcessor::new(proc);
        dynamic.set_pivot(0.5);
        dynamic.set_contrast(2.0);

        // 0.5 at pivot should stay 0.5
        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        dynamic.apply_rgb(&mut pixels);
        assert!((pixels[0][0] - 0.5).abs() < 0.001);

        // 0.75 should be pushed further from pivot
        // (0.75 - 0.5) * 2.0 + 0.5 = 1.0
        let mut pixels = [[0.75_f32, 0.75, 0.75]];
        dynamic.apply_rgb(&mut pixels);
        assert!((pixels[0][0] - 1.0).abs() < 0.001);
    }

    #[test]
    fn gamma_adjustment() {
        let proc = identity_processor();
        let mut dynamic = DynamicProcessor::new(proc);
        dynamic.set_gamma(2.2);

        // 0.5^(1/2.2) ≈ 0.73
        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        dynamic.apply_rgb(&mut pixels);
        assert!((pixels[0][0] - 0.5_f32.powf(1.0 / 2.2)).abs() < 0.001);
    }

    #[test]
    fn saturation_adjustment() {
        let proc = identity_processor();
        let mut dynamic = DynamicProcessor::new(proc);
        dynamic.set_saturation(0.0);

        // Full desaturation should produce grayscale
        let mut pixels = [[1.0_f32, 0.0, 0.0]];
        dynamic.apply_rgb(&mut pixels);

        // Every channel is the Rec.709 luma of pure red, a literal so a wrong weight in
        // the implementation's constant fails ([PyOCIO] GP LIN sat 0 on red: 0.21259999).
        for c in pixels[0] {
            assert!((c - 0.2126).abs() < 1e-6, "{:?}", pixels[0]);
        }
    }

    #[test]
    fn no_adjustments() {
        let proc = identity_processor();
        let dynamic = DynamicProcessor::new(proc);

        assert!(!dynamic.has_adjustments());

        let mut pixels = [[0.5_f32, 0.3, 0.1]];
        let original = pixels;
        dynamic.apply_rgb(&mut pixels);

        assert!((pixels[0][0] - original[0][0]).abs() < 0.0001);
        assert!((pixels[0][1] - original[0][1]).abs() < 0.0001);
        assert!((pixels[0][2] - original[0][2]).abs() < 0.0001);
    }

    #[test]
    fn reset_adjustments() {
        let proc = identity_processor();
        let mut dynamic = DynamicProcessor::new(proc);

        dynamic.set_exposure(2.0);
        dynamic.set_contrast(1.5);
        dynamic.set_gamma(2.2);
        dynamic.set_saturation(0.5);

        assert!(dynamic.has_adjustments());

        dynamic.reset();

        assert!(!dynamic.has_adjustments());
        assert!((dynamic.exposure() - 0.0).abs() < 0.0001);
        assert!((dynamic.contrast() - 1.0).abs() < 0.0001);
        assert!((dynamic.gamma() - 1.0).abs() < 0.0001);
        assert!((dynamic.saturation() - 1.0).abs() < 0.0001);
    }

    #[test]
    fn builder_pattern() {
        let proc = identity_processor();
        let dynamic = DynamicProcessorBuilder::new()
            .exposure(1.0)
            .contrast(1.2)
            .gamma(2.2)
            .saturation(0.9)
            .pivot(0.18)
            .build(proc);

        assert!((dynamic.exposure() - 1.0).abs() < 0.0001);
        assert!((dynamic.contrast() - 1.2).abs() < 0.0001);
        assert!((dynamic.gamma() - 2.2).abs() < 0.0001);
        assert!((dynamic.saturation() - 0.9).abs() < 0.0001);
    }

    #[test]
    fn apply_before_vs_after() {
        // Base x^2.2 and exposure +1 (x2): the order is visible.
        // Before: (0.5 * 2)^2.2 = 1.0. After: 0.5^2.2 * 2 = 0.43527528.
        let base = || {
            Processor::from_ops(vec![crate::processor::ProcessorOp::Transfer {
                style: crate::processor::TransferStyle::Gamma22,
                forward: false,
            }])
        };
        let mut dynamic_before = DynamicProcessor::new(base());
        dynamic_before.set_exposure(1.0);
        dynamic_before.set_apply_before(true);

        let mut dynamic_after = DynamicProcessor::new(base());
        dynamic_after.set_exposure(1.0);
        dynamic_after.set_apply_before(false);

        let mut pixels_before = [[0.5_f32, 0.5, 0.5]];
        let mut pixels_after = [[0.5_f32, 0.5, 0.5]];
        dynamic_before.apply_rgb(&mut pixels_before);
        dynamic_after.apply_rgb(&mut pixels_after);

        assert!(
            (pixels_before[0][0] - 1.0).abs() < 1e-6,
            "{:?}",
            pixels_before
        );
        assert!(
            (pixels_after[0][0] - 0.435_275_3).abs() < 1e-6,
            "{:?}",
            pixels_after
        );
    }

    #[test]
    fn grading_primary_default() {
        let primary = GradingPrimary::default();
        assert!(primary.is_identity());
        assert!((primary.pivot - 0.18).abs() < 1e-9);
        assert!((primary.saturation - 1.0).abs() < 1e-9);
    }

    #[test]
    fn grading_primary_log_default() {
        let primary = GradingPrimary::log_default();
        assert!(primary.is_identity());
        // OCIO: `GradingPrimary(GRADING_LOG)` pivot default is -0.2
        // (`OpenColorTransforms.h:464-469`) - was wrongly 0.435 here.
        assert!((primary.pivot - (-0.2)).abs() < 1e-9);
    }

    /// `docs/ROADMAP.md` GradingPrimary item: `exposure`/`lift`/`gain` were
    /// scalar `f64` (identity 0.0/0.0/1.0) instead of per-channel
    /// `GradingRgbm`, unlike OCIO's real `GradingPrimary`
    /// (`OpenColorTransforms.h:471-477`), which carries all seven of
    /// brightness/contrast/gamma/offset/exposure/lift/gain as `GradingRGBM`.
    /// Pins the RGBM identity conventions per field (exposure/lift zero,
    /// gain one - additive vs. multiplicative, same as the other four
    /// fields already had) and that a single non-master channel is enough
    /// to break identity, exactly like the RGBM fields already did.
    #[test]
    fn grading_primary_exposure_lift_gain_are_per_channel_rgbm() {
        let mut primary = GradingPrimary::default();
        assert!(primary.exposure.is_zero());
        assert!(primary.lift.is_zero());
        assert!(primary.gain.is_one());
        assert!(primary.is_identity());

        primary.exposure = GradingRgbm::new(0.0, 0.5, 0.0, 0.0);
        assert!(
            !primary.is_identity(),
            "a single non-master exposure channel must break identity"
        );

        let primary = GradingPrimary {
            lift: GradingRgbm::new(0.1, 0.0, 0.0, 0.0),
            ..Default::default()
        };
        assert!(!primary.is_identity());

        let primary = GradingPrimary {
            gain: GradingRgbm::new(1.0, 1.2, 1.0, 1.0),
            ..Default::default()
        };
        assert!(!primary.is_identity());
    }

    #[test]
    fn grading_rgbm() {
        let zero = GradingRgbm::default();
        assert!(zero.is_zero());
        assert!(!zero.is_one());

        let one = GradingRgbm::one();
        assert!(one.is_one());
        assert!(!one.is_zero());

        let custom = GradingRgbm::new(0.5, 0.6, 0.7, 0.8);
        assert!(!custom.is_zero());
        assert!(!custom.is_one());
        assert!((custom.red - 0.5).abs() < 1e-9);
    }

    #[test]
    fn dynamic_property_type_values() {
        // Verify OCIO parity values
        assert_eq!(DynamicPropertyType::Exposure as u8, 0);
        assert_eq!(DynamicPropertyType::Contrast as u8, 1);
        assert_eq!(DynamicPropertyType::Gamma as u8, 2);
        assert_eq!(DynamicPropertyType::GradingPrimary as u8, 3);
        assert_eq!(DynamicPropertyType::GradingRgbCurve as u8, 4);
        assert_eq!(DynamicPropertyType::GradingTone as u8, 5);
        assert_eq!(DynamicPropertyType::GradingHueCurve as u8, 6);
    }
}
