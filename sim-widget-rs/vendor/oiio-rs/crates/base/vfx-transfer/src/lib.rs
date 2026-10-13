//! # vfx-transfer
//!
//! Transfer functions (OETF/EOTF) for color encoding and decoding.
//!
//! Transfer functions convert between linear light values and encoded values
//! for storage, display, or transmission.
//!
//! # Terminology
//!
//! - **OETF** (Opto-Electronic Transfer Function): Linear -> Encoded (e.g., for recording)
//! - **EOTF** (Electro-Optical Transfer Function): Encoded -> Linear (e.g., for display)
//! - **Gamma**: The exponent in a power-law transfer function
//!
//! # Supported Transfer Functions
//!
//! | Function | Use Case | Range |
//! |----------|----------|-------|
//! | [`srgb`] | Web, consumer displays | [0, 1] |
//! | [`gamma`] (2.2, 2.4, 2.6; OCIO moncurve) | Legacy CRT simulation, display gamma | [0, 1] |
//! | [`rec709`] | HDTV broadcast | [0, 1] |
//! | [`bt2020`] | UHDTV (BT.2020) | [0, 1] |
//! | [`pq`] | HDR (HDR10, Dolby Vision) | [0, 10000] cd/m2 |
//! | [`hlg`] | HDR broadcast (HLG) | [0, 1] |
//! | [`log_c`] | ARRI LogC3 cameras (ALEXA) | Scene-referred |
//! | [`log_c4`] | ARRI LogC4 cameras (ALEXA 35) | Scene-referred |
//! | [`canon_log`] | Canon Log/Log2/Log3 cameras | Scene-referred |
//! | [`apple_log`] | Apple ProRes RAW | Scene-referred |
//! | [`s_log2`] | Sony cameras (F65, F55, FS7 legacy) | Scene-referred |
//! | [`s_log3`] | Sony cameras | Scene-referred |
//! | [`v_log`] | Panasonic cameras | Scene-referred |
//! | [`red_log`] | RED cameras (REDLogFilm, REDLog3G10) | Scene-referred |
//! | [`bmd_film`] | Blackmagic cameras (BMDFilm Gen5) | Scene-referred |
//!
//! # Usage
//!
//! ```rust
//! use vfx_transfer::{srgb, pq};
//!
//! // Decode sRGB to linear
//! let linear = srgb::eotf(0.5);
//!
//! // Encode linear to sRGB
//! let encoded = srgb::oetf(linear);
//!
//! // HDR: decode PQ (returns absolute luminance in cd/m2)
//! let nits = pq::eotf(0.5);
//! ```
//!
//! # Scene vs Display Referred
//!
//! - **Display-referred** (sRGB, Rec.709): Values represent display output
//! - **Scene-referred** (ACES, Log): Values represent scene light ratios
//!
//! Camera log curves (LogC, S-Log, V-Log) are scene-referred and can
//! represent very wide dynamic ranges (14+ stops).
//!
//! # Dependencies
//!
//! - `vfx-core` - Core types
//!
//! # Used By
//!
//! - `vfx-color` - Full color space conversions
//! - `vfx-io` - Image file encoding/decoding

#![warn(missing_docs)]
#![warn(rustdoc::missing_crate_level_docs)]

pub mod acescc;
pub mod acescct;
pub mod apple_log;
pub mod bmd_film;
pub mod bt2020;
pub mod canon_log;
pub mod d_log;
pub mod davinci_intermediate;
pub mod fast;
pub mod gamma;
pub mod hlg;
pub mod log_c;
pub mod log_c4;
pub mod log_camera;
pub mod pq;
pub mod rec709;
pub mod red_log;
pub mod s_log2;
pub mod s_log3;
pub mod srgb;
pub mod v_log;
