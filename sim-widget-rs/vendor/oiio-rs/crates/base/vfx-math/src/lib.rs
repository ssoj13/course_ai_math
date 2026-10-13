//! # vfx-math
//!
//! Math utilities for VFX color and image processing.
//!
//! This crate provides mathematical primitives essential for color management:
//!
//! - [`Mat3`] - 3x3 matrices for color space transformations
//! - [`Vec3`] - 3D vectors for XYZ/RGB triplets
//! - [`layout`] - layout-tagged matrices: row-major vs column-major as a *type*, not a
//!   comment. Deliberately not glob-re-exported, so `layout::Mat3<RowMajor>` reads as the
//!   convention it is and cannot be confused with [`Mat3`].
//! - Chromatic adaptation transforms (Bradford, CAT02, etc.)
//! - Interpolation utilities (lerp, smoothstep, spline)
//!
//! # Design
//!
//! This crate wraps [`glam`] types with VFX-specific functionality.
//! All matrix operations assume **row-major** storage and **column vectors**:
//!
//! ```text
//! result = matrix * vector
//! ```
//!
//! # Usage
//!
//! ```rust
//! use vfx_math::{Mat3, Vec3};
//!
//! // Create RGB to XYZ matrix
//! let rgb_to_xyz = Mat3::from_rows([
//!     [0.4124564, 0.3575761, 0.1804375],
//!     [0.2126729, 0.7151522, 0.0721750],
//!     [0.0193339, 0.1191920, 0.9503041],
//! ]);
//!
//! // Transform a color
//! let rgb = Vec3::new(1.0, 0.5, 0.25);
//! let xyz = rgb_to_xyz * rgb;
//! ```
//!
//! # Dependencies
//!
//! - [`glam`] - Fast SIMD-accelerated math
//! - [`vfx-core`] - Core types
//!
//! # Used By
//!
//! - `vfx-primaries` - RGB/XYZ matrix generation
//! - `vfx-color` - Color space conversions
//! - `vfx-adapt` - Chromatic adaptation

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

mod adapt;
mod bitdepth;
mod interp;
pub mod layout;
mod mat3;
mod rational;
pub mod simd;
mod vec3;

pub use adapt::*;
pub use bitdepth::*;
pub use interp::*;
pub use mat3::*;
pub use rational::*;
pub use vec3::*;

/// Re-export glam types for direct use
pub mod glam {
    pub use ::glam::{Mat3 as GlamMat3, Vec3 as GlamVec3, Vec3A};
}
