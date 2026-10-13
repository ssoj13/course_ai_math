//! # vfx-core
//!
//! Core types for VFX image and color processing.
//!
//! This crate provides the foundational types used throughout the VFX-RS ecosystem:
//!
//! - [`ColorSpace`] - Trait and marker types for compile-time color space safety
//! - [`Pixel`], [`Rgb`], [`Rgba`] - Generic pixel types with color space tracking
//! - [`Image`] - Zero-copy image buffer with color space awareness
//! - [`ImageSpec`] - Image metadata and specifications
//! - [`Rect`], [`Roi`] - Region of interest types
//! - [`consts`] - single source for the numeric constants stated in more than one crate
//!   (white points and their two conventions, exposure/contrast, LogLin, ACES 1.x).
//!   Deliberately not glob-re-exported: `consts::loglin::M` should say where it came from.
//!
//! ## Design Philosophy
//!
//! The core principle is **compile-time color space safety**. An image in sRGB
//! cannot be accidentally mixed with an image in ACEScg without explicit conversion:
//!
//! ```no_run
//! use vfx_core::{AcesCg, Image, Srgb};
//!
//! let srgb: Image<Srgb, f32, 3> = Image::new(1920, 1080);
//! // Changing the colour space is explicit (the pixel conversion itself is vfx-color's):
//! let aces: Image<AcesCg, f32, 3> = srgb.clone().reinterpret();
//! // let bad: Image<AcesCg, f32, 3> = srgb; // compile error: mismatched types
//! ```
//!
//! ## Crate Structure
//!
//! This crate is the foundation of VFX-RS and has no internal dependencies.
//! All other VFX-RS crates depend on `vfx-core`:
//!
//! ```text
//! vfx-core (this crate)
//!    ^
//!    |
//!    +-- vfx-math (matrices, interpolation)
//!    +-- vfx-lut (LUT types)
//!    +-- vfx-transfer (transfer functions)
//!    +-- vfx-primaries (color primaries)
//!    +-- vfx-io (image I/O)
//!    +-- vfx-ops (image operations)
//!    +-- ... (all other crates)
//! ```
//!
//! ## Feature Flags
//!
//! - `serde` - Enable serialization for metadata types
//! - `rayon` (default) - [`runner::Rayon`] is the [`runner::default_runner`]: the data-parallel
//!   CPU work ([`resample`], and vfx-compute / vfx-stmap through [`runner`]) runs on rayon's pool.
//!   Without it the default is [`runner::Serial`] and rayon is not linked; an OpenFX plug-in builds
//!   that way and passes its host's MultiThread suite as a [`runner::Runner`].

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

pub mod alpha;
/// Channel classification by name (OIIO's alpha / Z / colour rules). The one owner.
pub mod channels;
pub mod colorspace;
pub mod consts;
pub mod error;
/// The workspace's single resampling-filter vocabulary, shared by `vfx-ops`,
/// `vfx-warp` and `vfx-compute`. See the module docs: its discriminants are a GPU
/// ABI and must not be renumbered.
pub mod filter;
pub mod format;
pub mod image;
pub mod mip;
pub mod pixel;
pub mod rect;
pub mod resample;
pub mod runner;
pub mod spec;
pub mod st;
#[doc(hidden)]
pub mod test_support;
pub mod texture_filter;
pub mod texture_lookup;

// Re-exports for convenience
pub use colorspace::*;
pub use error::*;
pub use filter::{FilterDesc, FilterKind};
pub use format::*;
pub use image::*;
pub use pixel::{
    PixelFormat, REC709_LUMA, REC709_LUMA_B, REC709_LUMA_G, REC709_LUMA_R, Rgb, Rgba,
    luminance_rec709, quantize_bits, quantize_i32, quantize_scaled, quantize_u8, quantize_u16,
    u16_to_u8,
};
pub use rect::*;
pub use spec::*;
pub use st::{StConvention, StDecode};

/// Prelude module for convenient imports.
///
/// # Usage
///
/// ```
/// use vfx_core::prelude::*;
/// ```
pub mod prelude {
    pub use crate::colorspace::{
        Aces2065, AcesCc, AcesCct, AcesCg, ColorSpace, ColorSpaceId, DciP3, DisplayP3, LinearSrgb,
        Rec709, Rec2020, Srgb,
    };
    pub use crate::error::{Error, Result};
    pub use crate::format::{Aggregate, BaseType, BitDepth, DataFormat, TypeDesc, VecSemantics};
    pub use crate::image::{Image, ImageView, ImageViewMut};
    pub use crate::pixel::{
        PixelFormat, REC709_LUMA, REC709_LUMA_B, REC709_LUMA_G, REC709_LUMA_R, Rgb, Rgba,
        luminance_rec709,
    };
    pub use crate::rect::{Rect, Roi, Roi3D};
    pub use crate::spec::ImageSpec;
}
