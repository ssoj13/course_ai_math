//! Error types for OCIO configuration parsing and processing.
//!
//! This module provides error handling for:
//! - Config file parsing (YAML/XML)
//! - Color space lookup and validation
//! - Transform chain building
//! - Display/View configuration

use std::path::PathBuf;
use thiserror::Error;

/// Result type for OCIO operations.
pub type OcioResult<T> = Result<T, OcioError>;

/// Errors that can occur during OCIO operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OcioError {
    /// I/O error reading config files.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// YAML parsing error.
    #[error("YAML parse error: {0}")]
    Yaml(String),

    /// Config file not found.
    #[error("config file not found: {path}")]
    ConfigNotFound {
        /// Path that was searched.
        path: PathBuf,
    },

    /// An `ocio://` URI or built-in config name OCIO's registry does not have
    /// (`BuiltinConfigRegistryImpl::getBuiltinConfigByName`'s message).
    #[error("Could not find '{name}' in the built-in configurations.")]
    BuiltinConfigNotFound {
        /// The name, without `ocio://`.
        name: String,
    },

    /// Invalid config version.
    #[error("unsupported config version: {version} (supported: 1.x, 2.x)")]
    UnsupportedVersion {
        /// Version string from config.
        version: String,
    },

    /// Color space not found in config.
    #[error("color space not found: {name}")]
    ColorSpaceNotFound {
        /// Name of the missing color space.
        name: String,
    },

    /// Role not defined in config.
    #[error("role not defined: {role}")]
    RoleNotDefined {
        /// Name of the undefined role.
        role: String,
    },

    /// Display not found in config.
    #[error("display not found: {name}")]
    DisplayNotFound {
        /// Name of the missing display.
        name: String,
    },

    /// View not found for display.
    #[error("view '{view}' not found for display '{display}'")]
    ViewNotFound {
        /// Display name.
        display: String,
        /// View name.
        view: String,
    },

    /// Look not found in config. The text is OCIO's `RunLookTokens` one, with the config's
    /// looks listed as OCIO lists them.
    #[error("RunLookTokens error. The specified look, '{name}', cannot be found. {}", look_list(.available))]
    LookNotFound {
        /// Name of the missing look.
        name: String,
        /// The looks the config does define, in config order.
        available: Vec<String>,
    },

    /// A look whose process space is not a colour space (`RunLookTokens`).
    #[error(
        "RunLookTokens error. The specified look, '{look}', requires processing in the ColorSpace, '{space}' which is not defined."
    )]
    LookProcessSpace {
        /// The look.
        look: String,
        /// Its process space as written.
        space: String,
    },

    /// Every `|` option of a look string needs a file that cannot be located (OCIO
    /// `ExceptionMissingFile`, thrown by `BuildLookOps` after trying each option).
    #[error("{0}")]
    MissingFile(String),

    /// A viewing pipeline's correction needs a role the config does not define
    /// (`LegacyViewingPipeline.cpp:273`, text as OCIO).
    #[error("DisplayViewTransform error: {what} requires '{role}' role to be defined.")]
    RoleRequired {
        /// The role, e.g. `scene_linear`.
        role: String,
        /// The correction that needs it, e.g. `LinearCC`.
        what: &'static str,
    },

    /// The viewing pipeline's input colour space is not in the config
    /// (`LegacyViewingPipeline.cpp:203`, text as OCIO).
    #[error("LegacyViewingPipeline error: Cannot find inputColorSpace, named '{name}'.")]
    InputColorSpaceNotFound {
        /// The name as given.
        name: String,
    },

    /// A viewing pipeline that cannot build a processor (`validate()`, text as OCIO).
    #[error("{0}")]
    Pipeline(String),

    /// No op of the processor has the requested dynamic property
    /// (`CPUProcessor.cpp:308`, text as OCIO).
    #[error("Cannot find dynamic property; not used by CPU processor.")]
    DynamicPropertyNotFound,

    /// Invalid transform definition.
    #[error("invalid transform: {reason}")]
    InvalidTransform {
        /// Description of what's wrong.
        reason: String,
    },

    /// File reference in transform not found.
    #[error("transform file not found: {path}")]
    TransformFileNotFound {
        /// Path to the missing file.
        path: PathBuf,
    },

    /// Circular reference detected in transforms.
    #[error("circular reference detected: {chain}")]
    CircularReference {
        /// Description of the circular chain.
        chain: String,
    },

    /// Context variable not set.
    #[error("context variable not set: {name}")]
    ContextVariableNotSet {
        /// Name of the missing variable.
        name: String,
    },

    /// Invalid environment variable reference.
    #[error("invalid environment reference: {expr}")]
    InvalidEnvReference {
        /// The invalid expression.
        expr: String,
    },

    /// Transform processing error.
    #[error("transform error: {0}")]
    Transform(String),

    /// GPU runtime error (device adoption, shader compile, buffer map, or readback) from the
    /// `gpu`-feature colour-processing runtime (`gpu::runtime::apply_gpu`).
    #[error("GPU error: {0}")]
    Gpu(String),

    /// LUT loading error.
    #[error("LUT error: {0}")]
    Lut(#[from] vfx_lut::LutError),

    /// An ICC profile could not be read (vfx-icc).
    #[error("ICC profile: {0}")]
    Icc(#[from] vfx_icc::IccError),

    /// General validation error.
    #[error("validation error: {0}")]
    Validation(String),
}

/// OCIO's list tail of a `RunLookTokens` error: ` (looks: a, b).`, or the empty-config note.
fn look_list(available: &[String]) -> String {
    if available.is_empty() {
        " (No looks defined in config).".to_string()
    } else {
        format!(" (looks: {}).", available.join(", "))
    }
}
