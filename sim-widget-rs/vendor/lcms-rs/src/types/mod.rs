//! Foundational types — the crate's public contract.
//!
//! These mirror the base typedefs and structs of `lcms2.h`: fixed-point conversions
//! ([`num`]), colorimetric structs ([`color`]), ICC 4CC signatures ([`sig`]) and the
//! packed pixel-format bitfield ([`format`]). Everything else in the port is built on
//! top of these, so they are defined first (Phase 0) and change rarely.

pub mod color;
pub mod format;
pub mod num;
pub mod sig;

pub use color::{CIELCh, CIELab, CIExyY, CIExyYTriple, CIEXYZ, CIEXYZTriple, DateTime, IccData};
pub use format::PixelFormat;
pub use sig::Signature;
