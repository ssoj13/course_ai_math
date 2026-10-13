//! Colorimetric value types, mirroring the plain structs of `lcms2.h`.
//!
//! These are pure data carriers (the C structs are field-for-field the same). Color
//! *math* over them (XYZ↔Lab, ΔE, adaptation) lands in the `color` subsystem in Phase 1;
//! here we only define the shapes so every layer shares one representation.

/// CIE XYZ tristimulus (`cmsCIEXYZ`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CIEXYZ {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// CIE xyY chromaticity + luminance (`cmsCIExyY`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CIExyY {
    pub x: f64,
    pub y: f64,
    /// Luminance (the capital `Y`).
    pub yy: f64,
}

/// CIE L\*a\*b\* (`cmsCIELab`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CIELab {
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

/// CIE L\*C\*h° (`cmsCIELCh`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CIELCh {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

/// RGB triple of XYZ colorants (`cmsCIEXYZTRIPLE`), e.g. profile primaries.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CIEXYZTriple {
    pub red: CIEXYZ,
    pub green: CIEXYZ,
    pub blue: CIEXYZ,
}

/// RGB triple of xyY chromaticities (`cmsCIExyYTRIPLE`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CIExyYTriple {
    pub red: CIExyY,
    pub green: CIExyY,
    pub blue: CIExyY,
}

/// Decoded ICC date/time (`cmsDateTimeNumber` decoded).
///
/// Stored in natural, human-facing form: full year and 1-based month. The on-disk ICC
/// representation is six big-endian `u16`s (`year, month, day, hours, minutes, seconds`);
/// the ±1900 / ±1 adjustments C performs against `struct tm` are handled at the I/O
/// boundary (the `io::endian` codec), not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DateTime {
    /// Full year, e.g. 2026.
    pub year: u16,
    /// Month, 1..=12.
    pub month: u16,
    pub day: u16,
    pub hours: u16,
    pub minutes: u16,
    pub seconds: u16,
}

/// A length/flag-tagged binary blob (`cmsICCData`).
///
/// The C struct is a flexible-array `{ len, flag, data[] }`; in Rust the payload is an
/// owned `Vec<u8>` and `len` is simply `data.len()`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IccData {
    /// `flag` field (0 = ASCII, 1 = binary, per ICC).
    pub flag: u32,
    pub data: Vec<u8>,
}
