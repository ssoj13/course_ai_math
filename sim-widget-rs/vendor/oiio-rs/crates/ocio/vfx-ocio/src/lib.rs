//! OpenColorIO-compatible color management for VFX.
//!
//! This crate provides a Rust implementation of OCIO functionality:
//! - Load and parse `.ocio` configuration files
//! - Define and manage color spaces
//! - Build transform chains for color conversion
//! - Built-in ACES configurations
//!
//! # Quick Start
//!
//! ```
//! use vfx_ocio::{Config, builtin};
//!
//! // Use built-in ACES config
//! let config = builtin::default_config();
//!
//! // Look up color spaces
//! let acescg = config.colorspace("ACEScg").unwrap();
//! println!("Working space: {}", acescg.name());
//!
//! // Create a processor
//! let processor = config.processor("ACEScg", "sRGB - Display").unwrap();
//!
//! // Apply to pixels
//! let mut pixels = [[0.18_f32, 0.18, 0.18]];
//! processor.apply_rgb(&mut pixels);
//! ```
//!
//! # Loading External Configs
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_ocio::Config;
//!
//! let config = Config::from_file("path/to/config.ocio")?;
//!
//! // List available color spaces
//! for cs in config.colorspaces() {
//!     println!("{}: {:?}", cs.name(), cs.encoding());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Roles
//!
//! Roles provide semantic access to color spaces:
//!
//! ```
//! use vfx_ocio::builtin;
//!
//! let config = builtin::default_config();
//!
//! // Access by role name
//! let linear = config.colorspace("scene_linear").unwrap();
//! assert_eq!(linear.name(), "ACEScg");
//! ```
//!
//! # Display Pipeline
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_ocio::Config;
//!
//! let config = Config::from_file("config.ocio")?;
//!
//! // Create display processor
//! let proc = config.display_processor("ACEScg", "sRGB - Display", "ACES 2.0 - SDR 100 nits (Rec.709)")?;
//! let mut pixels = [[0.18_f32, 0.18, 0.18]; 100];
//! proc.apply_rgb(&mut pixels);
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

#[allow(missing_docs)]
pub mod aces2;
mod archive;
mod baker;
mod builtin_transforms;
mod cache;
mod colorspace;
mod config;
mod config_builder;
mod context;
mod display;
mod dynamic;
mod error;
mod gpu;
mod gpu_shader_utils;
pub mod grading_bspline;
mod grading_hue_curve;
pub mod grading_tone;
pub mod hsy;
mod io_proxy;
mod logging;
mod look;
mod parity_tests;
mod parse_utils;
mod path_utils;
mod processor;
mod role;
mod system_monitors;
mod transform;
mod viewing;

pub mod builtin;
pub mod validate;

/// RGB primaries and the matrices between them, from the same builders the builtin transforms
/// use (a port of OCIO's `ColorMatrixHelpers`, checked against PyOpenColorIO 2.5.1 to 1e-12).
///
/// OCIO keeps these internal; a host that works in display-linear light (e.g. an HDR viewer going
/// from OCIO's display reference, CIE XYZ D65, to linear Rec.709) needs them, and copying the
/// numbers would be a second source of truth. The builders here check their primaries and return
/// an error where OCIO would throw on a singular matrix (or pass inf/NaN along).
pub mod color_matrix {
    pub use crate::builtin_transforms::color_matrix::{
        ACES_AP0, ACES_AP1, Adaptation, CIE_XYZ_ILLUM_E, M44, P3_D60, P3_D65, P3_DCI, Primaries,
        REC709, REC709_D60, REC2020, REC2020_D60, conversion_matrix,
        conversion_matrix_from_xyz_d65, conversion_matrix_to_xyz_d65, whitepoint,
    };
}

// Re-exports
pub use archive::{
    archive_config_to_path, config_archive_name, create_temp_archive_dir, extract_ocioz_archive,
    read_config_from_archive,
};
pub use baker::{Allocation, BakedLut1D, BakedLut3D, Baker, LutFormat};
pub use cache::ProcessorCache;
pub use colorspace::{
    AllocationInfo, AllocationType, BitDepth as ColorSpaceBitDepth, ColorSpace, ColorSpaceBuilder,
    ColorSpaceDirection, Encoding, Family,
};
pub use config::{
    AddDisplayViewParams, ColorSpaceVisibility, Config, ConfigVersion, EnvironmentMode, FileRule,
    FileRuleKind, NamedTransform, NamedTransformVisibility, SearchReferenceSpaceType, ViewingRule,
    VirtualDisplay, VirtualDisplayView,
};
pub use config_builder::ConfigBuilder;
pub use context::Context;
pub use display::{Display, DisplayManager, ReferenceSpaceType, View, ViewTransform};
#[allow(deprecated)] // the deprecated wrapper stays exported until it is removed
pub use dynamic::{
    DynamicProcessor, DynamicProcessorBuilder, DynamicPropertyType, DynamicPropertyValue,
    GradingPrimary, GradingRgbm,
};
pub use error::{OcioError, OcioResult};
/// Host-facing GPU resource builder (`gpu` feature): turns a [`GpuShaderCode`] into real
/// `wgpu::Buffer`/`wgpu::Texture`/`wgpu::BindGroup` resources for OCIO's `@group(1)`/`@group(2)` —
/// on the HOST'S OWN `wgpu::Device`, not only the shared one [`apply_gpu`]/[`apply_gpu_texture`]
/// use. See [`GpuOcioResources`]'s module docs for the host-integration walkthrough.
#[cfg(feature = "gpu")]
pub use gpu::GpuOcioResources;
/// GPU colour-processing runtime entry points (`gpu` feature): execute a [`Processor`]'s emitted
/// WGSL on the shared wgpu device. `apply_gpu` is the host-buffer path (RGBA-f32 in/out);
/// `apply_gpu_texture` is the ZERO-COPY texture-native path (canonical `Rgba16Float` `GpuImage` in
/// and out, no host round-trip) for the media hub's GPU-resident pipeline. Both are one-shot
/// convenience wrappers around [`GpuBufferExecutor`] / [`GpuTextureExecutor`] (PLAN9.md O0) - a
/// caller applying the SAME processor across many frames should compile an executor ONCE and reuse
/// it, rather than calling `apply_gpu`/`apply_gpu_texture` per frame (which repeats the shader
/// compile + pipeline build + uniform upload every time).
#[cfg(feature = "gpu")]
pub use gpu::runtime::{GpuBufferExecutor, GpuTextureExecutor, apply_gpu, apply_gpu_texture};
pub use gpu::{
    BatchTextureDescriptor, GpuBindingContract, GpuBindingDesc, GpuBindingKind, GpuChannelType,
    GpuInterpolation, GpuLanguage, GpuProcessor, GpuShaderCode, GpuTexture, GpuTextureType,
    GpuUniform, GpuUniformType, GpuUniformValue, OCIO_HOST_GROUP_START, OCIO_TEXTURE_GROUP,
    OCIO_UNIFORM_GROUP, TextureDimensions, WGSL_TRANSFORM_FUNCTION,
};
pub use io_proxy::ConfigIOProxy;
pub use logging::{
    LoggingFunction, LoggingLevel, get_logging_level, log_message, reset_logging_function,
    set_logging_function, set_logging_level,
};
pub use look::{Look, LookManager, parse_looks};
pub use path_utils::{reset_compute_hash_function, set_compute_hash_function};
pub use processor::{
    ChannelOrdering, FormatMetadata, OptimizationFlags, OptimizationLevel, PackedBitDepth,
    Processor, ProcessorMetadata, ProcessorOp, optimization_flags_env_override,
};
pub use role::{Roles, names as role_names};
pub use system_monitors::SystemMonitors;
pub use transform::{
    AllocationTransform, BuiltinTransferTransform, BuiltinTransform, CdlStyle, CdlTransform,
    ColorSpaceTransform, DisplayViewTransform, EcDynamic, ExponentTransform,
    ExponentWithLinearTransform, ExposureContrastStyle, ExposureContrastTransform, FileTransform,
    FixedFunctionStyle, FixedFunctionTransform, GradingHueCurveStyle, GradingHueCurveTransform,
    GradingPrimaryStyle, GradingPrimaryTransform, GradingRgbCurveTransform, GradingRgbmsw,
    GradingToneStyle, GradingToneTransform, GroupTransform, HsyTransformStyle, HueCurveType,
    Interpolation, LogAffineTransform, LogCameraTransform, LogTransform, LookTransform,
    Lut1DHueAdjust, Lut1DTransform, Lut3DTransform, MatrixTransform, NegativeStyle, RangeStyle,
    RangeTransform, RgbCurveType, Transform, TransformDirection,
};
pub use validate::{
    Issue, IssueCategory, Severity, check as validate_config, has_errors, has_warnings,
};
pub use viewing::LegacyViewingPipeline;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::TestSchema;

    #[test]
    fn quick_start_example() {
        let config = builtin::aces_2_0_studio_all_views();

        let acescg = config
            .colorspace(&TestSchema::shared().working_space)
            .unwrap();
        assert_eq!(acescg.name(), "ACEScg");

        let processor = config
            .processor(
                &TestSchema::shared().working_space,
                &TestSchema::shared().display_encoded_srgb,
            )
            .unwrap();

        // working_space (ACEScg) -> "sRGB - Display" runs the full ACES output
        // transform (tone map + sRGB encode), NOT a bare sRGB OETF, so the
        // result is NOT the textbook 0.461 sRGB-of-0.18. The reference has no
        // tabulated golden value for this path and we must not assert a value
        // we computed by running this same code, so we assert a verifiable
        // PROPERTY: the encode is finite, in display range, neutral, and moves
        // 0.18 off the input. The old test applied the transform then discarded
        // the result, passing even on an identity / no-op processor.
        let mut pixels = [[0.18_f32, 0.18, 0.18]];
        processor.apply_rgb(&mut pixels);
        let out = pixels[0];
        for c in out {
            assert!(
                c.is_finite() && (0.0..=1.0).contains(&c),
                "out of range: {c}"
            );
        }
        assert!(
            (out[0] - out[1]).abs() < 0.05 && (out[1] - out[2]).abs() < 0.05,
            "neutral grey should stay neutral: {out:?}"
        );
        assert!(
            (out[0] - 0.18).abs() > 0.05,
            "ACEScg->sRGB display encode of 0.18 must differ from input (no-op?): {out:?}"
        );
    }

    #[test]
    fn role_access() {
        let config = builtin::aces_2_0_studio_all_views();

        // Access by role name
        let linear = config.colorspace("scene_linear").unwrap();
        assert_eq!(linear.name(), "ACEScg");
    }

    #[test]
    fn context_variables() {
        let mut ctx = Context::new();
        ctx.set("SHOT", "sh010");

        let resolved = ctx.resolve("/shows/$SHOT/luts/grade.cube");
        assert_eq!(resolved, "/shows/sh010/luts/grade.cube");
    }

    #[test]
    fn transform_chain() {
        let cdl = Transform::Cdl(CdlTransform {
            slope: [1.1, 1.0, 0.9],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            ..Default::default()
        });

        let processor = Processor::from_transform(&cdl, TransformDirection::Forward).unwrap();

        let mut pixels = [[0.5_f32, 0.5, 0.5]];
        processor.apply_rgb(&mut pixels);

        assert!((pixels[0][0] - 0.55).abs() < 0.01);
    }
}
