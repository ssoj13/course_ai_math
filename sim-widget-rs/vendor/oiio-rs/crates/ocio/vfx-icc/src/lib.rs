//! # vfx-icc
//!
//! ICC color profile support for VFX color pipelines.
//!
//! This crate provides a high-level interface for working with ICC color profiles,
//! built on `lcms-rs` — a pure-Rust port of the industry-standard Little CMS 2 engine.
//!
//! # Features
//!
//! - Load ICC profiles from files or embedded data
//! - Create standard profiles (sRGB, Adobe RGB, Display P3, ACES, etc.)
//! - Transform colors between profiles
//! - Support for different rendering intents
//! - High-precision 32-bit float processing
//!
//! # Example
//!
//! ```rust,no_run
//! use vfx_icc::{Profile, Transform, Intent};
//! use std::path::Path;
//!
//! // Load a camera profile
//! let camera = Profile::from_file(Path::new("camera.icc")).unwrap();
//!
//! // Get the working space profile
//! let aces = Profile::aces_ap0().unwrap();
//!
//! // Create a transform
//! let transform = Transform::new(&camera, &aces, Intent::Perceptual).unwrap();
//!
//! // Transform pixels
//! let mut pixels = vec![[0.5f32, 0.3, 0.2]; 100];
//! transform.apply(&mut pixels);
//! ```
//!
//! # Supported Color Spaces
//!
//! Built-in profiles include:
//! - sRGB (IEC 61966-2-1)
//! - Adobe RGB (1998)
//! - Display P3
//! - DCI-P3
//! - ACES AP0 (linear)
//! - ACES AP1 / ACEScg (linear)
//! - Rec. 709
//! - Rec. 2020
//!
//! # Thread Safety
//!
//! Transforms can be shared between threads when created with caching disabled.
//! Use [`Transform::new_uncached`] for thread-safe transforms.
//!
//! # Color engine: pure Rust, no FFI
//!
//! This crate is built on [`lcms_rs`], a **pure-Rust port** of the Little CMS 2 engine. There is
//! no C dependency, no `*-sys` crate, and no `unsafe` anywhere in `vfx-icc` — the crate is fully
//! within the workspace's native-Rust policy.
//!
//! Historically this crate bound Little CMS 2 through C FFI (the `lcms2` crate over
//! `lcms2-sys`), documented as a reviewed exception to that policy. The stated precondition for
//! revisiting it was "a mature, independently-audited native-Rust ICC engine with matching v2/v4
//! support, numerically validated against `lcms2`". `lcms-rs` is exactly that: ICC v2 + v4, zero
//! `unsafe`, zero FFI, and validated byte-identically against real Little CMS 2.19 across 1029
//! golden vectors. The swap is therefore numerically transparent — the reference color values
//! pinned by this crate's tests (sRGB EOTF, sRGB->ACEScg Bradford matrix columns, sRGB->P3)
//! are unchanged and still pass.
//!
//! **Error-handling rule (unchanged, and still load-bearing)**: every `lcms-rs` call that returns
//! `Result`/`Option` must be checked and turned into a typed [`IccError`] variant — never
//! `.unwrap()`/`.expect()` on the result of an `lcms-rs` call reachable from a public API, and
//! never a silent substitute profile on failure. Dropping FFI removes the memory-safety hazard,
//! not the correctness hazard: a mislabeled profile is still silently wrong colorimetry.

// The crate's no-`unsafe` property is enforced by the compiler rather than by review: the FFI
// this crate used to carry is gone, and `bytemuck` covers the only pixel-buffer reinterpretation
// that ever needed raw pointers.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

mod error;
mod profile;
mod standard;
mod transform;

pub use error::{IccError, IccResult};
pub use profile::Profile;
pub use standard::StandardProfile;
pub use transform::{Transform, convert_rgb};

/// Rendering intent for color transformations.
///
/// Determines how out-of-gamut colors are handled during conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Intent {
    /// Maintains color accuracy within the destination gamut.
    ///
    /// Out-of-gamut colors are clipped to the nearest in-gamut color.
    /// Best for proofing and accurate color reproduction.
    #[default]
    Perceptual,

    /// Preserves the relationship between colors.
    ///
    /// Compresses the entire source gamut to fit within the destination.
    /// Best for photographic images.
    RelativeColorimetric,

    /// Maintains saturation at the expense of accuracy.
    ///
    /// Best for business graphics where vivid colors are more important
    /// than exact color matching.
    Saturation,

    /// Like relative colorimetric but with white point adaptation.
    ///
    /// Maps source white to destination white exactly.
    /// Best for spot color matching.
    AbsoluteColorimetric,
}

impl Intent {
    /// The ICC rendering-intent code this variant maps to.
    ///
    /// `lcms-rs` models intents as bare `u32` constants (mirroring C's `INTENT_*` macros)
    /// rather than an enum, so the conversion is a plain lookup rather than a `From` impl.
    pub(crate) fn code(self) -> u32 {
        match self {
            Intent::Perceptual => lcms_rs::prelude::INTENT_PERCEPTUAL,
            Intent::RelativeColorimetric => lcms_rs::prelude::INTENT_RELATIVE_COLORIMETRIC,
            Intent::Saturation => lcms_rs::prelude::INTENT_SATURATION,
            Intent::AbsoluteColorimetric => lcms_rs::prelude::INTENT_ABSOLUTE_COLORIMETRIC,
        }
    }
}
