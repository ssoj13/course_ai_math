//! Color science: white point / adaptation, PCS conversions, tone curves, CIECAM02.
//!
//! This is Phase 1 (M1) of the port. It builds on `math` (matrices, interpolation) and
//! `types` (colorimetric structs). Shared colorimetric constants — the D50 reference white
//! and the ICC perceptual black — live here so every submodule references one definition.

use crate::types::color::CIEXYZ;

/// CIECAM02 appearance model (`cmscam02`). Gated on the `cam02` feature.
#[cfg(feature = "cam02")]
pub mod cam02;
pub mod gamma;
pub mod pcs;
pub mod wtpnt;

/// D50 reference white, X (`cmsD50X`).
pub const D50X: f64 = 0.9642;
/// D50 reference white, Y (`cmsD50Y`).
pub const D50Y: f64 = 1.0;
/// D50 reference white, Z (`cmsD50Z`).
pub const D50Z: f64 = 0.8249;

/// ICC V4 perceptual black point, X (`cmsPERCEPTUAL_BLACK_X`).
pub const PERCEPTUAL_BLACK_X: f64 = 0.00336;
/// ICC V4 perceptual black point, Y (`cmsPERCEPTUAL_BLACK_Y`).
pub const PERCEPTUAL_BLACK_Y: f64 = 0.0034731;
/// ICC V4 perceptual black point, Z (`cmsPERCEPTUAL_BLACK_Z`).
pub const PERCEPTUAL_BLACK_Z: f64 = 0.00287;

/// The D50 reference white as an [`CIEXYZ`] (`cmsD50_XYZ`).
#[inline]
pub const fn d50_xyz() -> CIEXYZ {
    CIEXYZ { x: D50X, y: D50Y, z: D50Z }
}
