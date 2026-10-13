//! Color space definitions and compile-time color space safety.
//!
//! This module provides the [`ColorSpace`] trait and marker types for various
//! color spaces used in VFX production.
//!
//! # Architecture
//!
//! The color system has two complementary representations:
//!
//! 1. **Compile-time markers** ([`ColorSpace`] trait) - Zero-sized types like
//!    [`Srgb`], [`AcesCg`] that enable type-safe image operations.
//!    An `Image<Srgb>` cannot be accidentally mixed with `Image<AcesCg>`.
//!
//! 2. **Runtime identifiers** ([`ColorSpaceId`] enum) - For serialization,
//!    dynamic dispatch, OCIO integration, and metadata storage.
//!
//! The bridge between them:
//! ```text
//! Compile-time          Runtime              Math
//! ────────────          ───────              ────
//! trait ColorSpace  ──► ColorSpaceId  ──► vfx_primaries::Primaries
//!   const ID            .name()            ::from_id()
//!   const NAME          .is_linear()       conversion_matrix()
//!   const PRIMARIES     .primaries()
//! ```
//!
//! # Design Rationale
//!
//! - **Why both?** Compile-time safety catches errors at build time, but
//!   runtime IDs are needed for I/O, configs, and user-facing code.
//! - **Why `const ID`?** Links marker types to runtime enum without overhead.
//! - **Why not just enum?** Would lose compile-time type safety.
//!
//! # Supported Color Spaces
//!
//! ## Scene-Referred (Linear)
//! - [`AcesCg`] - ACEScg (AP1 primaries, linear) - Primary working space
//! - [`Aces2065`] - ACES2065-1 (AP0 primaries, linear) - Archival format
//! - [`LinearSrgb`] - Linear sRGB (Rec.709 primaries, linear)
//! - [`Rec2020`] - ITU-R BT.2020 (wide gamut, linear)
//!
//! ## Display-Referred (Non-Linear)
//! - [`Srgb`] - sRGB with standard transfer function
//! - [`Rec709`] - ITU-R BT.709 with gamma 2.4
//! - [`DciP3`] - DCI-P3 (theater)
//! - [`DisplayP3`] - Display P3 (Apple devices)
//! - [`Rec2100Pq`] - ITU-R BT.2100 with SMPTE ST 2084 (PQ) transfer - HDR10
//! - [`Rec2100Hlg`] - ITU-R BT.2100 with ARIB STD-B67 (HLG) transfer - HDR broadcast
//!
//! ## Log-Encoded
//! - [`AcesCct`] - ACEScct (AP1 primaries, log-like curve)
//! - [`AcesCc`] - ACEScc (AP1 primaries, pure log)
//!
//! # Usage
//!
//! ```
//! use vfx_core::prelude::*;
//!
//! // Color spaces are used as type parameters
//! fn process_image<C: ColorSpace>(img: &Image<C, f32, 3>) {
//!     println!("Processing image in {} color space", C::NAME);
//! }
//! ```
//!
//! # Dependencies
//!
//! This module has no external dependencies. It defines the core abstractions
//! used by:
//! - `vfx-primaries` - RGB to XYZ matrices based on primaries
//! - `vfx-transfer` - Transfer functions based on `IS_LINEAR` flag
//! - `vfx-aces` - ACES-specific color space handling

use std::borrow::Cow;
use std::fmt;

// ============================================================================
// ColorSpaceId - Runtime color space identifier
// ============================================================================

/// Runtime identifier for color spaces.
///
/// Bridges compile-time marker types with runtime operations.
/// Use this for:
/// - Serialization/deserialization
/// - Dynamic dispatch
/// - OCIO integration
/// - Metadata storage
///
/// # Example
///
/// ```rust
/// use vfx_core::{ColorSpace, ColorSpaceId, Srgb, AcesCg};
///
/// // Get ID from compile-time type
/// let id = Srgb::ID;
/// assert_eq!(id.name().as_ref(), "sRGB");
///
/// // Parse from string
/// let id = ColorSpaceId::from_name("ACEScg").unwrap();
/// assert_eq!(id, ColorSpaceId::AcesCg);
/// ```
///
/// # PLAN9.md O4 - the `GammaHundredths` payload
///
/// This enum was `#[repr(u8)]` (a fixed-discriminant C-like enum) until PLAN9.md O4 added
/// [`ColorSpaceId::GammaHundredths`], a variant that carries a `u16` payload (the writer-emitted
/// gamma value in hundredths, e.g. `220` for `Gamma2.2`). A payload-carrying variant cannot have a
/// fixed `repr(u8)` discriminant, so the repr and all the explicit `= N` discriminants were
/// dropped; nothing in this crate or its dependents matched on the numeric discriminant value
/// (verified via workspace grep), so this is a source-compatible change.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColorSpaceId {
    /// sRGB with standard transfer function.
    Srgb,
    /// Linear sRGB (Rec.709 primaries, linear).
    LinearSrgb,
    /// ACEScg - standard ACES working space.
    AcesCg,
    /// ACES2065-1 - archival ACES format.
    Aces2065,
    /// ACEScct - log-encoded ACES for grading.
    AcesCct,
    /// ACEScc - pure log ACES.
    AcesCc,
    /// Rec.709 broadcast standard.
    Rec709,
    /// Rec.2020 wide gamut (linear).
    Rec2020,
    /// DCI-P3 theatrical.
    DciP3,
    /// Display P3 (Apple).
    DisplayP3,
    /// Rec.2100 with SMPTE ST 2084 (PQ) transfer - HDR10. Rec.2020 primaries, non-linear.
    Rec2100Pq,
    /// Rec.2100 with ARIB STD-B67 (HLG) transfer - HDR broadcast. Rec.2020 primaries, non-linear.
    Rec2100Hlg,
    /// CIE XYZ - not an RGB primaries space (OIIO's `lin_ciexyzd65_scene` parses to it).
    Xyz,
    /// Generic log-encoded space (this crate's "KodakLog" vocabulary; PLAN9.md O4: `dpx.rs`
    /// emits `"log"` for DPX transfer characteristic code 3).
    Log,
    /// CMYK - not an RGB space (PLAN9.md O4: `tiff.rs` emits `"CMYK"` for raw CMYK TIFFs).
    Cmyk,
    /// Parametrised gamma family. Payload is the display gamma in hundredths
    /// (`round(gamma * 100)`, e.g. `220` for a 2.2 gamma) - PLAN9.md O4: `png.rs`/`dpx.rs` emit
    /// `format!("Gamma{}", rounded)` for an unlisted gamma transfer curve.
    GammaHundredths(u16),
    /// Unknown or unspecified.
    #[default]
    Unknown,
}

impl ColorSpaceId {
    /// All known color space IDs (excluding Unknown and the unbounded [`Self::GammaHundredths`]
    /// family - PLAN9.md O4 deliberately does not attempt to list every gamma value here).
    pub const ALL: &'static [ColorSpaceId] = &[
        Self::Srgb,
        Self::LinearSrgb,
        Self::AcesCg,
        Self::Aces2065,
        Self::AcesCct,
        Self::AcesCc,
        Self::Rec709,
        Self::Rec2020,
        Self::DciP3,
        Self::DisplayP3,
        Self::Rec2100Pq,
        Self::Rec2100Hlg,
        Self::Xyz,
        Self::Log,
        Self::Cmyk,
    ];

    /// Human-readable name.
    ///
    /// Returns `Cow::Borrowed` for every fixed variant and `Cow::Owned` only for
    /// [`Self::GammaHundredths`], whose name is built from its payload (PLAN9.md O4). The format
    /// matches what `png.rs`/`dpx.rs` write (`format!("Gamma{}", rounded_gamma)`) exactly, so
    /// `from_name(id.name().as_ref())` round-trips.
    #[inline]
    pub fn name(&self) -> Cow<'static, str> {
        match self {
            Self::Srgb => Cow::Borrowed("sRGB"),
            Self::LinearSrgb => Cow::Borrowed("Linear sRGB"),
            Self::AcesCg => Cow::Borrowed("ACEScg"),
            Self::Aces2065 => Cow::Borrowed("ACES2065-1"),
            Self::AcesCct => Cow::Borrowed("ACEScct"),
            Self::AcesCc => Cow::Borrowed("ACEScc"),
            Self::Rec709 => Cow::Borrowed("Rec.709"),
            Self::Rec2020 => Cow::Borrowed("Rec.2020"),
            Self::DciP3 => Cow::Borrowed("DCI-P3"),
            Self::DisplayP3 => Cow::Borrowed("Display P3"),
            Self::Rec2100Pq => Cow::Borrowed("Rec.2100-PQ"),
            Self::Rec2100Hlg => Cow::Borrowed("Rec.2100-HLG"),
            Self::Xyz => Cow::Borrowed("xyz"),
            Self::Log => Cow::Borrowed("log"),
            Self::Cmyk => Cow::Borrowed("CMYK"),
            Self::GammaHundredths(h) => {
                let g = *h as f64 / 100.0;
                Cow::Owned(format!("Gamma{g}"))
            }
            Self::Unknown => Cow::Borrowed("Unknown"),
        }
    }

    /// Whether this color space uses linear encoding.
    #[inline]
    pub const fn is_linear(&self) -> bool {
        matches!(
            self,
            Self::LinearSrgb | Self::AcesCg | Self::Aces2065 | Self::Rec2020 | Self::Xyz
        )
    }

    /// CIE xy white point chromaticity.
    #[inline]
    pub const fn white_point(&self) -> (f32, f32) {
        match self {
            // D65 white point
            Self::Srgb
            | Self::LinearSrgb
            | Self::Rec709
            | Self::Rec2020
            | Self::DisplayP3
            | Self::Rec2100Pq
            | Self::Rec2100Hlg
            // Not RGB primaries spaces, but all D65-referenced (PLAN9.md O4): see `primaries()`
            // for why these three reuse sRGB's gamut rather than something more meaningful.
            | Self::Xyz
            | Self::Log
            | Self::Cmyk
            // Gamma is a transfer-only parametrisation; it carries no primaries/white point of
            // its own, so it defaults to D65 same as `Unknown`.
            | Self::GammaHundredths(_) => (0.3127, 0.3290),
            // ACES white (~D60)
            Self::AcesCg | Self::Aces2065 | Self::AcesCct | Self::AcesCc => (0.32168, 0.33767),
            // DCI white
            Self::DciP3 => (0.314, 0.351),
            Self::Unknown => (0.3127, 0.3290),
        }
    }

    /// CIE xy primaries [Red, Green, Blue].
    ///
    /// [`Self::Xyz`], [`Self::Log`], [`Self::Cmyk`] and [`Self::GammaHundredths`] are not RGB
    /// primaries spaces at all (PLAN9.md O4): XYZ has no RGB gamut, log/gamma are transfer-only
    /// parametrisations, and CMYK is a subtractive 4-channel model. They fall back to sRGB's
    /// primaries as a sentinel purely so `Primaries::from_id` (which expects an RGB triplet) has
    /// *something* to return; do not feed these through RGB-primaries matrix math and expect a
    /// meaningful result.
    #[inline]
    pub const fn primaries(&self) -> [(f32, f32); 3] {
        match self {
            // sRGB / Rec.709 primaries
            Self::Srgb
            | Self::LinearSrgb
            | Self::Rec709
            | Self::Unknown
            | Self::Xyz
            | Self::Log
            | Self::Cmyk
            | Self::GammaHundredths(_) => [(0.640, 0.330), (0.300, 0.600), (0.150, 0.060)],
            // ACES AP1 primaries
            Self::AcesCg | Self::AcesCct | Self::AcesCc => {
                [(0.713, 0.293), (0.165, 0.830), (0.128, 0.044)]
            }
            // ACES AP0 primaries
            Self::Aces2065 => [(0.7347, 0.2653), (0.0000, 1.0000), (0.0001, -0.0770)],
            // Rec.2020 primaries (Rec.2100 reuses them - only the transfer function differs)
            Self::Rec2020 | Self::Rec2100Pq | Self::Rec2100Hlg => {
                [(0.708, 0.292), (0.170, 0.797), (0.131, 0.046)]
            }
            // P3 primaries
            Self::DciP3 | Self::DisplayP3 => [(0.680, 0.320), (0.265, 0.690), (0.150, 0.060)],
        }
    }

    /// Color space family (ACES, sRGB, Rec, P3).
    #[inline]
    pub const fn family(&self) -> Option<&'static str> {
        match self {
            Self::AcesCg | Self::Aces2065 | Self::AcesCct | Self::AcesCc => Some("ACES"),
            Self::Srgb | Self::LinearSrgb => Some("sRGB"),
            Self::Rec709 | Self::Rec2020 | Self::Rec2100Pq | Self::Rec2100Hlg => Some("Rec"),
            Self::DciP3 | Self::DisplayP3 => Some("P3"),
            Self::Xyz => Some("XYZ"),
            Self::Cmyk => Some("CMYK"),
            // Log and Gamma are transfer-only parametrisations, not a primaries family.
            Self::Log | Self::GammaHundredths(_) => None,
            Self::Unknown => None,
        }
    }

    /// Parse from name string (case-insensitive).
    ///
    /// Accepts the OpenImageIO 3.1 names every vfx-io reader sets (`vfx_io::colorspace`):
    /// `srgb_rec709_scene`, `lin_rec709_scene`, `lin_ap1_scene`, `lin_ap0_scene`,
    /// `lin_rec2020_scene`, `lin_ciexyzd65_scene`, `KodakLog`, `Rec709`, `CMYK` and
    /// `g<NN>_rec709_scene` (a pure gamma of NN tenths), plus the older spellings (`"linear"`,
    /// `"sRGB"`, `"Gamma2.2"`, ...).
    pub fn from_name(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        match lower.as_str() {
            "srgb" | "s-rgb" | "srgb_rec709_scene" => Some(Self::Srgb),
            "linear srgb" | "linearsrgb" | "linear_srgb" | "srgb-linear" | "linear" | "lin"
            | "scene_linear" | "lin_rec709_scene" | "lin_rec709" | "lin_srgb" => {
                Some(Self::LinearSrgb)
            }
            "acescg" | "aces_cg" | "aces-cg" | "lin_ap1_scene" | "lin_ap1" => Some(Self::AcesCg),
            "aces2065-1" | "aces2065" | "ap0" | "lin_ap0_scene" | "lin_ap0" => Some(Self::Aces2065),
            "acescct" | "aces_cct" | "aces-cct" => Some(Self::AcesCct),
            "acescc" | "aces_cc" | "aces-cc" => Some(Self::AcesCc),
            "rec709" | "rec.709" | "bt709" | "bt.709" => Some(Self::Rec709),
            "rec2020" | "rec.2020" | "bt2020" | "bt.2020" | "lin_rec2020_scene" => {
                Some(Self::Rec2020)
            }
            "dci-p3" | "dcip3" | "dci_p3" => Some(Self::DciP3),
            "display p3" | "displayp3" | "display_p3" | "p3-display" | "display p3 hdr"
            | "displayp3_hdr" | "displayp3-hdr" => Some(Self::DisplayP3),
            "rec.2100-pq" | "rec2100-pq" | "rec2100pq" | "bt.2100-pq" | "bt2100pq" => {
                Some(Self::Rec2100Pq)
            }
            "rec.2100-hlg" | "rec2100-hlg" | "rec2100hlg" | "bt.2100-hlg" | "bt2100hlg" => {
                Some(Self::Rec2100Hlg)
            }
            "xyz" | "cie-xyz" | "ciexyz" | "lin_ciexyzd65_scene" => Some(Self::Xyz),
            "log" | "kodaklog" | "kodak_log" => Some(Self::Log),
            "cmyk" => Some(Self::Cmyk),
            "unknown" | "" => Some(Self::Unknown),
            _ => {
                // PLAN9.md O4: `"Gamma{n}"` (any case) parses its numeric suffix as the display
                // gamma and stores it as hundredths, matching the exact format png.rs/dpx.rs
                // write (`format!("Gamma{}", rounded)`) so `id.name()` round-trips back here.
                // OIIO's `g<NN>_rec709_scene` (`set_colorspace_rec709_gamma`): NN tenths.
                if let Some(tenths) = lower
                    .strip_prefix('g')
                    .and_then(|r| r.strip_suffix("_rec709_scene"))
                    .and_then(|d| d.parse::<u16>().ok())
                    .filter(|t| *t > 0 && *t <= u16::MAX / 10)
                {
                    return Some(Self::GammaHundredths(tenths * 10));
                }
                lower.strip_prefix("gamma").and_then(|suffix| {
                    let gamma: f64 = suffix.parse().ok()?;
                    if !gamma.is_finite() || gamma <= 0.0 {
                        return None;
                    }
                    let hundredths = (gamma * 100.0).round();
                    if !hundredths.is_finite() || hundredths <= 0.0 || hundredths > u16::MAX as f64
                    {
                        return None;
                    }
                    Some(Self::GammaHundredths(hundredths as u16))
                })
            }
        }
    }
}

// Default derived via #[derive(Default)] on ColorSpaceId enum

impl fmt::Display for ColorSpaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name().as_ref())
    }
}

// ============================================================================
// ColorSpace Trait
// ============================================================================

/// Trait for color space marker types.
///
/// This trait provides compile-time information about color spaces,
/// enabling type-safe color operations.
///
/// # Implementing Custom Color Spaces
///
/// ```
/// use vfx_core::{ColorSpace, ColorSpaceId};
///
/// #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
/// pub struct MyCustomSpace;
///
/// impl ColorSpace for MyCustomSpace {
///     const ID: ColorSpaceId = ColorSpaceId::Unknown;
///     const NAME: &'static str = "MyCustom";
///     const IS_LINEAR: bool = true;
///     const WHITE_POINT: (f32, f32) = (0.3127, 0.3290); // D65
///     const PRIMARIES: [(f32, f32); 3] = [
///         (0.640, 0.330), // Red
///         (0.300, 0.600), // Green
///         (0.150, 0.060), // Blue
///     ];
/// }
/// ```
pub trait ColorSpace: Copy + Clone + Default + Send + Sync + fmt::Debug + 'static {
    /// Runtime identifier for this color space.
    ///
    /// Enables bridging between compile-time and runtime representations.
    const ID: ColorSpaceId;

    /// Human-readable name of the color space.
    ///
    /// Used for display, logging, and metadata.
    const NAME: &'static str;

    /// Whether this color space uses linear light encoding.
    ///
    /// - `true` for scene-referred linear spaces (ACEScg, Linear sRGB)
    /// - `false` for display-referred or log-encoded spaces (sRGB, ACEScct)
    const IS_LINEAR: bool;

    /// CIE xy chromaticity coordinates of the white point.
    ///
    /// Common values:
    /// - D65: (0.3127, 0.3290) - sRGB, Rec.709, Rec.2020
    /// - D60: (0.32168, 0.33767) - ACES
    /// - DCI: (0.314, 0.351) - DCI-P3
    const WHITE_POINT: (f32, f32);

    /// CIE xy chromaticity coordinates of RGB primaries.
    ///
    /// Order: `[Red, Green, Blue]`
    ///
    /// These define the color gamut of the color space.
    const PRIMARIES: [(f32, f32); 3];

    /// Optional family/category for grouping related color spaces.
    ///
    /// Examples: "ACES", "Rec", "Display"
    const FAMILY: Option<&'static str> = None;

    /// Whether this is a scene-referred color space.
    ///
    /// Scene-referred spaces can represent values outside [0, 1].
    #[inline]
    fn is_scene_referred() -> bool {
        Self::IS_LINEAR
    }

    /// Whether this is a display-referred color space.
    ///
    /// Display-referred spaces are typically clamped to [0, 1].
    #[inline]
    fn is_display_referred() -> bool {
        !Self::IS_LINEAR
    }
}

// ============================================================================
// ACES Color Spaces
// ============================================================================

/// ACES2065-1 - The archival ACES color space.
///
/// Uses AP0 primaries which encompass the entire visible spectrum.
/// This is the interchange format for ACES.
///
/// # Characteristics
/// - **Primaries**: AP0 (spectral locus)
/// - **White Point**: ACES white (~D60)
/// - **Transfer**: Linear
/// - **Usage**: Archival, interchange
///
/// # When to Use
/// - Long-term archival storage
/// - Interchange between facilities
/// - Maximum color preservation
///
/// # References
/// - [ACES Documentation](https://docs.acescentral.com)
/// - SMPTE ST 2065-1
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Aces2065;

impl ColorSpace for Aces2065 {
    const ID: ColorSpaceId = ColorSpaceId::Aces2065;
    const NAME: &'static str = "ACES2065-1";
    const IS_LINEAR: bool = true;
    const WHITE_POINT: (f32, f32) = (0.32168, 0.33767); // ACES white (~D60)
    const PRIMARIES: [(f32, f32); 3] = [
        (0.7347, 0.2653),  // Red (spectral locus)
        (0.0000, 1.0000),  // Green (spectral locus)
        (0.0001, -0.0770), // Blue (spectral locus, imaginary)
    ];
    const FAMILY: Option<&'static str> = Some("ACES");
}

/// ACEScg - The standard ACES working color space.
///
/// Uses AP1 primaries which are more practical for CG rendering
/// while still being wide-gamut.
///
/// # Characteristics
/// - **Primaries**: AP1
/// - **White Point**: ACES white (~D60)
/// - **Transfer**: Linear
/// - **Usage**: CG rendering, compositing
///
/// # When to Use
/// - 3D rendering
/// - Compositing
/// - Color grading working space
///
/// # References
/// - [ACEScg Specification](https://docs.acescentral.com)
/// - S-2014-004
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct AcesCg;

impl ColorSpace for AcesCg {
    const ID: ColorSpaceId = ColorSpaceId::AcesCg;
    const NAME: &'static str = "ACEScg";
    const IS_LINEAR: bool = true;
    const WHITE_POINT: (f32, f32) = (0.32168, 0.33767);
    const PRIMARIES: [(f32, f32); 3] = [
        (0.713, 0.293), // Red
        (0.165, 0.830), // Green
        (0.128, 0.044), // Blue
    ];
    const FAMILY: Option<&'static str> = Some("ACES");
}

/// ACEScct - Log-encoded ACES for color correction.
///
/// Uses AP1 primaries with a log-like transfer function optimized
/// for color grading in DaVinci Resolve and similar tools.
///
/// # Characteristics
/// - **Primaries**: AP1
/// - **White Point**: ACES white (~D60)
/// - **Transfer**: ACEScct log curve (toe + log)
/// - **Usage**: Color grading
///
/// # Transfer Function
/// The ACEScct curve has a toe region for better shadow handling:
/// - Below 0.0078125: linear portion
/// - Above: logarithmic portion
///
/// # When to Use
/// - Color grading sessions
/// - When you need CDL-like controls
/// - DaVinci Resolve, Baselight
///
/// # Note
/// The actual transfer function is implemented in `vfx-transfer`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct AcesCct;

impl ColorSpace for AcesCct {
    const ID: ColorSpaceId = ColorSpaceId::AcesCct;
    const NAME: &'static str = "ACEScct";
    const IS_LINEAR: bool = false;
    const WHITE_POINT: (f32, f32) = (0.32168, 0.33767);
    const PRIMARIES: [(f32, f32); 3] = [(0.713, 0.293), (0.165, 0.830), (0.128, 0.044)];
    const FAMILY: Option<&'static str> = Some("ACES");
}

/// ACEScc - Pure log-encoded ACES for color correction.
///
/// Similar to ACEScct but with a pure logarithmic curve (no toe).
///
/// # Characteristics
/// - **Primaries**: AP1
/// - **White Point**: ACES white (~D60)
/// - **Transfer**: Pure log curve
/// - **Usage**: Color grading (legacy)
///
/// # Note
/// ACEScct is generally preferred over ACEScc for modern workflows.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct AcesCc;

impl ColorSpace for AcesCc {
    const ID: ColorSpaceId = ColorSpaceId::AcesCc;
    const NAME: &'static str = "ACEScc";
    const IS_LINEAR: bool = false;
    const WHITE_POINT: (f32, f32) = (0.32168, 0.33767);
    const PRIMARIES: [(f32, f32); 3] = [(0.713, 0.293), (0.165, 0.830), (0.128, 0.044)];
    const FAMILY: Option<&'static str> = Some("ACES");
}

// ============================================================================
// sRGB / Rec.709 Color Spaces
// ============================================================================

/// sRGB - Standard RGB color space for web and consumer displays.
///
/// The most common color space for images on the web and in consumer
/// applications.
///
/// # Characteristics
/// - **Primaries**: Rec.709 / sRGB
/// - **White Point**: D65
/// - **Transfer**: sRGB curve (linear near black, ~2.2 gamma)
/// - **Usage**: Web, photography, consumer displays
///
/// # Transfer Function
/// ```text
/// if L <= 0.0031308:
///     V = 12.92 * L
/// else:
///     V = 1.055 * L^(1/2.4) - 0.055
/// ```
///
/// # When to Use
/// - Final output for web
/// - JPEG/PNG export
/// - Consumer display viewing
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Srgb;

impl ColorSpace for Srgb {
    const ID: ColorSpaceId = ColorSpaceId::Srgb;
    const NAME: &'static str = "sRGB";
    const IS_LINEAR: bool = false;
    const WHITE_POINT: (f32, f32) = (0.3127, 0.3290); // D65
    const PRIMARIES: [(f32, f32); 3] = [
        (0.640, 0.330), // Red
        (0.300, 0.600), // Green
        (0.150, 0.060), // Blue
    ];
    const FAMILY: Option<&'static str> = Some("sRGB");
}

/// Linear sRGB - sRGB primaries with linear transfer.
///
/// Same primaries as sRGB but without the gamma curve.
/// Common intermediate space for rendering and compositing.
///
/// # Characteristics
/// - **Primaries**: Rec.709 / sRGB
/// - **White Point**: D65
/// - **Transfer**: Linear (1.0 gamma)
/// - **Usage**: Rendering, compositing, intermediate
///
/// # When to Use
/// - Blender, Unity, Unreal (linear workflow)
/// - Compositing in Nuke, After Effects
/// - When sRGB output is the final target
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct LinearSrgb;

impl ColorSpace for LinearSrgb {
    const ID: ColorSpaceId = ColorSpaceId::LinearSrgb;
    const NAME: &'static str = "Linear sRGB";
    const IS_LINEAR: bool = true;
    const WHITE_POINT: (f32, f32) = (0.3127, 0.3290);
    const PRIMARIES: [(f32, f32); 3] = [(0.640, 0.330), (0.300, 0.600), (0.150, 0.060)];
    const FAMILY: Option<&'static str> = Some("sRGB");
}

/// Rec.709 - ITU-R BT.709 broadcast color space.
///
/// The HD video standard. Same primaries as sRGB but with
/// BT.1886 transfer function (2.4 gamma).
///
/// # Characteristics
/// - **Primaries**: Rec.709
/// - **White Point**: D65
/// - **Transfer**: BT.1886 (2.4 gamma)
/// - **Usage**: HD broadcast, video production
///
/// # When to Use
/// - Video output for broadcast
/// - HD video editing
/// - When targeting TV displays
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rec709;

impl ColorSpace for Rec709 {
    const ID: ColorSpaceId = ColorSpaceId::Rec709;
    const NAME: &'static str = "Rec.709";
    const IS_LINEAR: bool = false;
    const WHITE_POINT: (f32, f32) = (0.3127, 0.3290);
    const PRIMARIES: [(f32, f32); 3] = [(0.640, 0.330), (0.300, 0.600), (0.150, 0.060)];
    const FAMILY: Option<&'static str> = Some("Rec");
}

// ============================================================================
// Wide Gamut Color Spaces
// ============================================================================

/// Rec.2020 - ITU-R BT.2020 wide gamut color space.
///
/// The UHD/4K/HDR video standard with significantly wider gamut
/// than Rec.709.
///
/// # Characteristics
/// - **Primaries**: Rec.2020 (wide gamut)
/// - **White Point**: D65
/// - **Transfer**: Can be linear, PQ, or HLG
/// - **Usage**: HDR video, UHD broadcast
///
/// # Note
/// This type represents Linear Rec.2020. For HDR output,
/// apply PQ or HLG transfer function via `vfx-transfer`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rec2020;

impl ColorSpace for Rec2020 {
    const ID: ColorSpaceId = ColorSpaceId::Rec2020;
    const NAME: &'static str = "Rec.2020";
    const IS_LINEAR: bool = true; // Linear variant
    const WHITE_POINT: (f32, f32) = (0.3127, 0.3290);
    const PRIMARIES: [(f32, f32); 3] = [
        (0.708, 0.292), // Red
        (0.170, 0.797), // Green
        (0.131, 0.046), // Blue
    ];
    const FAMILY: Option<&'static str> = Some("Rec");
}

/// Rec.2100 with SMPTE ST 2084 (PQ) transfer - the HDR10 signalling this crate previously had no
/// tag for (PLAN9.md O3/O4): a HEIF/AVIF `colr nclx` box or any other CICP-tagged source reporting
/// `TransferCharacteristics::Pq` + `ColorPrimaries::Bt2020` maps here, not to plain [`Rec2020`]
/// (which is the LINEAR variant and cannot represent a PQ-encoded signal at all).
///
/// # Characteristics
/// - **Primaries**: Rec.2020 (same as [`Rec2020`])
/// - **White Point**: D65
/// - **Transfer**: SMPTE ST 2084 (PQ), absolute luminance up to 10 000 cd/m²
/// - **Usage**: HDR10 video, PQ-tagged AVIF/HEIF stills
///
/// Linearise via `vfx_ocio::BuiltinTransferTransform { style: "pq".into(), .. }` (or
/// `vfx_transfer::pq::eotf` directly) - this crate has no external dependencies and does not
/// carry transfer-function code itself; see the module's `# Dependencies` note.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rec2100Pq;

impl ColorSpace for Rec2100Pq {
    const ID: ColorSpaceId = ColorSpaceId::Rec2100Pq;
    const NAME: &'static str = "Rec.2100-PQ";
    const IS_LINEAR: bool = false;
    const WHITE_POINT: (f32, f32) = (0.3127, 0.3290);
    const PRIMARIES: [(f32, f32); 3] = [(0.708, 0.292), (0.170, 0.797), (0.131, 0.046)];
    const FAMILY: Option<&'static str> = Some("Rec");
}

/// Rec.2100 with ARIB STD-B67 (HLG) transfer - the HDR broadcast sibling of [`Rec2100Pq`]. Same
/// primaries and white point; only the transfer function (and its relative, not absolute,
/// luminance model) differs.
///
/// Linearise via `vfx_ocio::BuiltinTransferTransform { style: "hlg".into(), .. }` (or
/// `vfx_transfer::hlg::eotf` directly) - see [`Rec2100Pq`]'s docs for why this crate itself does
/// not carry that code.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rec2100Hlg;

impl ColorSpace for Rec2100Hlg {
    const ID: ColorSpaceId = ColorSpaceId::Rec2100Hlg;
    const NAME: &'static str = "Rec.2100-HLG";
    const IS_LINEAR: bool = false;
    const WHITE_POINT: (f32, f32) = (0.3127, 0.3290);
    const PRIMARIES: [(f32, f32); 3] = [(0.708, 0.292), (0.170, 0.797), (0.131, 0.046)];
    const FAMILY: Option<&'static str> = Some("Rec");
}

/// DCI-P3 - Digital Cinema Initiative P3 color space.
///
/// The theatrical digital cinema standard.
///
/// # Characteristics
/// - **Primaries**: DCI-P3
/// - **White Point**: DCI white (greenish, ~6300K)
/// - **Transfer**: 2.6 gamma
/// - **Usage**: Digital cinema projection
///
/// # When to Use
/// - Creating DCPs (Digital Cinema Packages)
/// - Theatrical release
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct DciP3;

impl ColorSpace for DciP3 {
    const ID: ColorSpaceId = ColorSpaceId::DciP3;
    const NAME: &'static str = "DCI-P3";
    const IS_LINEAR: bool = false;
    const WHITE_POINT: (f32, f32) = (0.314, 0.351); // DCI white
    const PRIMARIES: [(f32, f32); 3] = [
        (0.680, 0.320), // Red
        (0.265, 0.690), // Green
        (0.150, 0.060), // Blue
    ];
    const FAMILY: Option<&'static str> = Some("P3");
}

/// Display P3 - Apple's P3 color space for displays.
///
/// Uses DCI-P3 primaries but with D65 white point and sRGB transfer.
///
/// # Characteristics
/// - **Primaries**: DCI-P3
/// - **White Point**: D65
/// - **Transfer**: sRGB curve
/// - **Usage**: Apple devices, modern wide-gamut displays
///
/// # When to Use
/// - Apple device displays
/// - Wide-gamut web content
/// - Modern HDR displays
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct DisplayP3;

impl ColorSpace for DisplayP3 {
    const ID: ColorSpaceId = ColorSpaceId::DisplayP3;
    const NAME: &'static str = "Display P3";
    const IS_LINEAR: bool = false;
    const WHITE_POINT: (f32, f32) = (0.3127, 0.3290); // D65
    const PRIMARIES: [(f32, f32); 3] = [(0.680, 0.320), (0.265, 0.690), (0.150, 0.060)];
    const FAMILY: Option<&'static str> = Some("P3");
}

// ============================================================================
// Generic / Unknown Color Space
// ============================================================================

/// Unknown or unspecified color space.
///
/// Used when the color space is not known or not relevant.
/// Operations between `Unknown` and typed color spaces should
/// be avoided.
///
/// # When to Use
/// - Reading images without color space metadata
/// - Legacy workflows
/// - Testing
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Unknown;

impl ColorSpace for Unknown {
    const ID: ColorSpaceId = ColorSpaceId::Unknown;
    const NAME: &'static str = "Unknown";
    const IS_LINEAR: bool = false;
    const WHITE_POINT: (f32, f32) = (0.3127, 0.3290); // Assume D65
    const PRIMARIES: [(f32, f32); 3] = [
        (0.640, 0.330), // Assume sRGB
        (0.300, 0.600),
        (0.150, 0.060),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colorspace_id_roundtrip() {
        // All marker types have correct ID
        assert_eq!(Srgb::ID, ColorSpaceId::Srgb);
        assert_eq!(AcesCg::ID, ColorSpaceId::AcesCg);
        assert_eq!(Aces2065::ID, ColorSpaceId::Aces2065);
        assert_eq!(Rec2020::ID, ColorSpaceId::Rec2020);
        assert_eq!(Rec2100Pq::ID, ColorSpaceId::Rec2100Pq);
        assert_eq!(Rec2100Hlg::ID, ColorSpaceId::Rec2100Hlg);
    }

    /// PLAN9.md O4: writers already emit "Rec.2100-PQ"/"Rec.2100-HLG" - these two exact strings
    /// must round-trip, plus share Rec.2020's primaries (PLAN9.md O3: Rec.2100 is Rec.2020
    /// primaries with a different transfer function, not a different gamut) and be non-linear
    /// (unlike plain `Rec2020`, which IS the linear variant).
    #[test]
    fn test_rec2100_pq_hlg() {
        assert_eq!(
            ColorSpaceId::from_name("Rec.2100-PQ"),
            Some(ColorSpaceId::Rec2100Pq)
        );
        assert_eq!(
            ColorSpaceId::from_name("Rec.2100-HLG"),
            Some(ColorSpaceId::Rec2100Hlg)
        );
        assert_eq!(ColorSpaceId::Rec2100Pq.name().as_ref(), "Rec.2100-PQ");
        assert_eq!(ColorSpaceId::Rec2100Hlg.name().as_ref(), "Rec.2100-HLG");
        assert!(!ColorSpaceId::Rec2100Pq.is_linear());
        assert!(!ColorSpaceId::Rec2100Hlg.is_linear());
        assert_eq!(
            ColorSpaceId::Rec2100Pq.primaries(),
            ColorSpaceId::Rec2020.primaries()
        );
        assert_eq!(
            ColorSpaceId::Rec2100Hlg.primaries(),
            ColorSpaceId::Rec2020.primaries()
        );
        assert_eq!(ColorSpaceId::Rec2100Pq.family(), Some("Rec"));
        assert_eq!(ColorSpaceId::Rec2100Hlg.family(), Some("Rec"));
    }

    #[test]
    fn test_colorspace_id_properties() {
        let id = ColorSpaceId::AcesCg;
        assert_eq!(id.name().as_ref(), "ACEScg");
        assert!(id.is_linear());
        assert_eq!(id.family(), Some("ACES"));
    }

    #[test]
    fn test_colorspace_id_from_name() {
        assert_eq!(ColorSpaceId::from_name("sRGB"), Some(ColorSpaceId::Srgb));
        assert_eq!(
            ColorSpaceId::from_name("ACEScg"),
            Some(ColorSpaceId::AcesCg)
        );
        assert_eq!(
            ColorSpaceId::from_name("rec.709"),
            Some(ColorSpaceId::Rec709)
        );
        assert_eq!(ColorSpaceId::from_name("unknown_space"), None);
    }

    #[test]
    fn test_colorspace_id_all() {
        // ALL should contain all known spaces (PLAN9.md O3/O4: +Rec2100Pq/Rec2100Hlg;
        // PLAN9.md O4: +Xyz/Log/Cmyk. GammaHundredths is deliberately excluded - see ALL's doc.)
        assert_eq!(ColorSpaceId::ALL.len(), 15);
        assert!(ColorSpaceId::ALL.contains(&ColorSpaceId::Srgb));
        assert!(ColorSpaceId::ALL.contains(&ColorSpaceId::AcesCg));
        assert!(ColorSpaceId::ALL.contains(&ColorSpaceId::Rec2100Pq));
        assert!(ColorSpaceId::ALL.contains(&ColorSpaceId::Rec2100Hlg));
        assert!(ColorSpaceId::ALL.contains(&ColorSpaceId::Xyz));
        assert!(ColorSpaceId::ALL.contains(&ColorSpaceId::Log));
        assert!(ColorSpaceId::ALL.contains(&ColorSpaceId::Cmyk));
        assert!(!ColorSpaceId::ALL.contains(&ColorSpaceId::Unknown));
    }

    /// Every colour-space name a vfx-io reader sets (OIIO's, `vfx_io::colorspace`) and the older
    /// spellings must parse via
    /// `from_name`, and re-serialising the parsed id (`id.name()`) must parse right back to the
    /// same id - the whole point of this ticket. Enumerates the writer vocabulary directly rather
    /// than trusting a paraphrase of it.
    #[test]
    fn test_writer_emitted_names_roundtrip() {
        let writer_emitted = [
            "srgb_rec709_scene",
            "lin_rec709_scene",
            "lin_ap1_scene",
            "lin_ap0_scene",
            "KodakLog",
            "Rec709",
            "g22_rec709_scene",
            "g25_rec709_scene",
            "sRGB",
            "linear",
            "log",
            "xyz",
            "CMYK",
            "Gamma2.2",
            "Gamma1.8",
            "Gamma2.4",
            "Rec.2100-PQ",
            "Rec.2100-HLG",
            "ACEScg",
            "Rec.709",
        ];
        for s in writer_emitted {
            let id = ColorSpaceId::from_name(s)
                .unwrap_or_else(|| panic!("from_name must accept writer-emitted {s:?}"));
            let round = ColorSpaceId::from_name(id.name().as_ref()).unwrap_or_else(|| {
                panic!(
                    "from_name(id.name()) must round-trip for {s:?} (name = {:?})",
                    id.name()
                )
            });
            assert_eq!(
                round, id,
                "round-tripping {s:?} through name()/from_name() changed the id"
            );
        }

        // "linear" is an alias into LinearSrgb whose canonical name() is "Linear sRGB", not
        // "linear" - both directions must still resolve to the same id.
        assert_eq!(
            ColorSpaceId::from_name("linear"),
            Some(ColorSpaceId::LinearSrgb)
        );
        assert_eq!(
            ColorSpaceId::from_name(ColorSpaceId::LinearSrgb.name().as_ref()),
            Some(ColorSpaceId::LinearSrgb)
        );
    }

    /// PLAN9.md O4: the `GammaHundredths` payload round-trips exactly, including the
    /// hundredths-rounding png.rs/dpx.rs perform before formatting (`"Gamma2"` for a 2.0 gamma,
    /// not `"Gamma2.0"` - Rust's `f64` Display drops the trailing `.0`).
    #[test]
    fn test_gamma_hundredths_roundtrip() {
        assert_eq!(
            ColorSpaceId::from_name("Gamma2.2"),
            Some(ColorSpaceId::GammaHundredths(220))
        );
        assert_eq!(
            ColorSpaceId::GammaHundredths(220).name().as_ref(),
            "Gamma2.2"
        );
        assert_eq!(
            ColorSpaceId::from_name("Gamma2"),
            Some(ColorSpaceId::GammaHundredths(200))
        );
        assert_eq!(ColorSpaceId::GammaHundredths(200).name().as_ref(), "Gamma2");
        // Case-insensitive, and re-parsing the formatted name recovers the same id.
        assert_eq!(
            ColorSpaceId::from_name("GAMMA1.8"),
            Some(ColorSpaceId::GammaHundredths(180))
        );
        let id = ColorSpaceId::GammaHundredths(180);
        assert_eq!(ColorSpaceId::from_name(id.name().as_ref()), Some(id));
        // OIIO's names for a pure gamma: NN tenths.
        assert_eq!(
            ColorSpaceId::from_name("g22_rec709_scene"),
            Some(ColorSpaceId::GammaHundredths(220))
        );
        assert_eq!(
            ColorSpaceId::from_name("g25_rec709_scene"),
            Some(ColorSpaceId::GammaHundredths(250))
        );
        assert_eq!(ColorSpaceId::from_name("g0_rec709_scene"), None);
        // Invalid gammas are rejected, not silently coerced.
        assert_eq!(ColorSpaceId::from_name("Gamma"), None);
        assert_eq!(ColorSpaceId::from_name("Gamma0"), None);
        assert_eq!(ColorSpaceId::from_name("Gamma-1"), None);
        assert_eq!(ColorSpaceId::from_name("Gammanope"), None);
        assert_eq!(ColorSpaceId::from_name("GammaNaN"), None);
    }

    /// PLAN9.md O4: `Xyz`/`Log`/`Cmyk` are the three new non-parametrised variants - `is_linear`,
    /// `family` and the primaries/white-point sentinel documented on `primaries()`.
    #[test]
    fn test_xyz_log_cmyk() {
        assert_eq!(ColorSpaceId::from_name("xyz"), Some(ColorSpaceId::Xyz));
        assert_eq!(ColorSpaceId::from_name("log"), Some(ColorSpaceId::Log));
        assert_eq!(ColorSpaceId::from_name("CMYK"), Some(ColorSpaceId::Cmyk));
        assert_eq!(ColorSpaceId::from_name("cmyk"), Some(ColorSpaceId::Cmyk));

        assert!(ColorSpaceId::Xyz.is_linear());
        assert!(!ColorSpaceId::Log.is_linear());
        assert!(!ColorSpaceId::Cmyk.is_linear());

        assert_eq!(ColorSpaceId::Xyz.family(), Some("XYZ"));
        assert_eq!(ColorSpaceId::Log.family(), None);
        assert_eq!(ColorSpaceId::Cmyk.family(), Some("CMYK"));
    }

    #[test]
    fn test_colorspace_names() {
        assert_eq!(Srgb::NAME, "sRGB");
        assert_eq!(AcesCg::NAME, "ACEScg");
        assert_eq!(Rec2020::NAME, "Rec.2020");
    }

    #[test]
    fn test_colorspace_linearity() {
        const { assert!(!Srgb::IS_LINEAR) };
        const { assert!(AcesCg::IS_LINEAR) };
        const { assert!(LinearSrgb::IS_LINEAR) };
        const { assert!(!AcesCct::IS_LINEAR) };
    }

    #[test]
    fn test_scene_display_referred() {
        assert!(AcesCg::is_scene_referred());
        assert!(!AcesCg::is_display_referred());
        assert!(!Srgb::is_scene_referred());
        assert!(Srgb::is_display_referred());
    }

    #[test]
    fn test_aces_family() {
        assert_eq!(AcesCg::FAMILY, Some("ACES"));
        assert_eq!(AcesCct::FAMILY, Some("ACES"));
        assert_eq!(Aces2065::FAMILY, Some("ACES"));
    }
}
