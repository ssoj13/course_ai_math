//! Decoded tag payloads (`TagValue`) and the small tag-specific structs.
//!
//! In lcms2 a decoded tag is an opaque `void*` whose concrete C type is implied by the
//! tag-type handler (`cmsTagTypeHandler`). This port replaces that `void*` with the
//! [`TagValue`] enum: one variant per *decoded object*, not per *type signature*.
//!
//! # Why the variant set is smaller than the 36 type handlers
//! Several ICC type signatures decode to the **same** object and are chosen at write time
//! by a `DecideType` callback (see [`crate::tags::descriptor`]):
//! * `cmsSigCurveType` / `cmsSigParametricCurveType` → one [`ToneCurve`] ([`TagValue::Curve`]).
//! * `cmsSigTextType` / `cmsSigTextDescriptionType` / `cmsSigMultiLocalizedUnicodeType` →
//!   one [`Mlu`] ([`TagValue::Mlu`]).
//! * `cmsSigLut8Type` / `cmsSigLut16Type` / `cmsSigLutAtoBType` / `cmsSigLutBtoAType` /
//!   `cmsSigMultiProcessElementType` → one [`Pipeline`] ([`TagValue::Pipeline`]).
//! * `cmsSigProfileSequenceDescType` / `cmsSigProfileSequenceIdType` → one [`Seq`].
//!
//! This mirrors lcms exactly: a `cmsMLU*`, `cmsToneCurve*`, `cmsPipeline*` or `cmsSEQ*` can
//! be serialized by any of its supported type handlers; the in-memory object is shared.

use crate::named::dict::{Dict, Seq};
use crate::named::mlu::Mlu;
use crate::named::namedcolor::NamedColorList;
use crate::pipeline::Pipeline;
use crate::color::gamma::ToneCurve;
use crate::types::color::{CIEXYZ, CIExyYTriple, DateTime, IccData};
use crate::types::sig::Signature;

/// Measurement conditions (`cmsICCMeasurementConditions`), the payload of `measurementType`.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    /// Standard observer (`Observer`).
    pub observer: u32,
    /// Backing XYZ (`Backing`).
    pub backing: CIEXYZ,
    /// Measurement geometry (`Geometry`).
    pub geometry: u32,
    /// Measurement flare, as an `s15Fixed16` decoded to `f64` (`Flare`).
    pub flare: f64,
    /// Illuminant type (`IlluminantType`).
    pub illuminant_type: u32,
}

/// Viewing-conditions parameters (`cmsICCViewingConditions`), payload of `viewingConditionsType`.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewingConditions {
    /// Absolute illuminant white point XYZ (`IlluminantXYZ`).
    pub illuminant_xyz: CIEXYZ,
    /// Absolute surround XYZ (`SurroundXYZ`).
    pub surround_xyz: CIEXYZ,
    /// Illuminant type (`IlluminantType`).
    pub illuminant_type: u32,
}

/// One screening channel (`cmsScreeningChannel`).
#[derive(Debug, Clone, PartialEq)]
pub struct ScreeningChannel {
    /// Screen frequency, `s15Fixed16` → `f64` (`Frequency`).
    pub frequency: f64,
    /// Screen angle, `s15Fixed16` → `f64` (`ScreenAngle`).
    pub screen_angle: f64,
    /// Spot shape (`SpotShape`).
    pub spot_shape: u32,
}

/// Screening parameters (`cmsScreening`), payload of `screeningType`.
#[derive(Debug, Clone, PartialEq)]
pub struct Screening {
    /// Screening flag bitfield (`Flag`).
    pub flag: u32,
    /// Declared channel count (`nChannels`); clamped to `cmsMAXCHANNELS - 1` on read.
    pub n_channels: u32,
    /// The per-channel parameters (length == [`Self::n_channels`]).
    pub channels: Vec<ScreeningChannel>,
}

/// Under-colour-removal / black-generation curves plus description (`cmsUcrBg`), payload of
/// `ucrBgType`.
#[derive(Debug, Clone)]
pub struct UcrBg {
    /// Under-colour-removal curve (`Ucr`).
    pub ucr: ToneCurve,
    /// Black-generation curve (`Bg`).
    pub bg: ToneCurve,
    /// General description (`Desc`).
    pub desc: Mlu,
}

/// CICP video-signal-type parameters (`cmsVideoSignalType`), payload of `cicpType`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoSignal {
    /// Colour primaries code (`ColourPrimaries`).
    pub colour_primaries: u8,
    /// Transfer characteristics code (`TransferCharacteristics`).
    pub transfer_characteristics: u8,
    /// Matrix coefficients code (`MatrixCoefficients`).
    pub matrix_coefficients: u8,
    /// Video full-range flag (`VideoFullRangeFlag`).
    pub video_full_range_flag: u8,
}

/// Microsoft MHC2 parameters (`cmsMHC2Type`), payload of `MHC2Type`.
#[derive(Debug, Clone, PartialEq)]
pub struct Mhc2 {
    /// Number of entries in each regamma curve (`CurveEntries`).
    pub curve_entries: u32,
    /// Minimum luminance, `s15Fixed16` → `f64` (`MinLuminance`).
    pub min_luminance: f64,
    /// Peak luminance, `s15Fixed16` → `f64` (`PeakLuminance`).
    pub peak_luminance: f64,
    /// The 3×4 XYZ→XYZ matrix (`XYZ2XYZmatrix`), each element `s15Fixed16` → `f64`.
    pub matrix: [[f64; 4]; 3],
    /// Red regamma curve, each entry `s15Fixed16` → `f64` (`RedCurve`).
    pub red_curve: Vec<f64>,
    /// Green regamma curve (`GreenCurve`).
    pub green_curve: Vec<f64>,
    /// Blue regamma curve (`BlueCurve`).
    pub blue_curve: Vec<f64>,
}

/// A decoded tag payload — the Rust equivalent of the `void*` returned by a
/// `cmsTagTypeHandler`'s read callback and consumed by its write callback.
///
/// See the module docs for why one variant may back several ICC type signatures.
///
/// The variants differ widely in size (a `Signature` is 4 bytes; a `Pipeline` or `Mlu` is
/// large). Boxing the big ones is deliberately avoided: a `TagValue` is the decoded payload of
/// a single tag — constructed once per tag on profile read/write, never in a hot loop — so the
/// extra allocation and indirection would buy nothing while obscuring the 1:1 mapping to
/// lcms's decoded objects.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum TagValue {
    /// `cmsSigXYZType` — a single encoded XYZ (the handler only ever reads one).
    Xyz(CIEXYZ),
    /// `cmsSigChromaticityType` — R/G/B chromaticities (each `Y = 1`).
    Chromaticity(CIExyYTriple),
    /// `cmsSigColorantOrderType` — colorant laydown order; length `cmsMAXCHANNELS`,
    /// `0xFF` marks unused slots (kept so writes recover the exact count).
    ColorantOrder(Vec<u8>),
    /// `cmsSigS15Fixed16ArrayType` — array of `s15Fixed16` decoded to `f64`.
    S15Fixed16Array(Vec<f64>),
    /// `cmsSigU16Fixed16ArrayType` — array of `u16Fixed16` decoded to `f64`.
    U16Fixed16Array(Vec<f64>),
    /// `cmsSigUInt8ArrayType` — raw `u8` array.
    UInt8Array(Vec<u8>),
    /// `cmsSigUInt32ArrayType` — `u32` array.
    UInt32Array(Vec<u32>),
    /// `cmsSigUInt64ArrayType` — `u64` array.
    UInt64Array(Vec<u64>),
    /// `cmsSigSignatureType` — a single 4CC.
    Signature(Signature),
    /// `text` / `desc` / `mluc` — a localized string container.
    Mlu(Mlu),
    /// `curv` / `para` — a tone curve.
    Curve(ToneCurve),
    /// `cmsSigDateTimeType` — a decoded timestamp.
    DateTime(DateTime),
    /// `cmsSigDataType` — flag-tagged binary blob.
    Data(IccData),
    /// `mft1` / `mft2` / `mAB ` / `mBA ` / `mpet` — a colour-transform pipeline.
    Pipeline(Pipeline),
    /// `cmsSigNamedColor2Type` — named-colour palette (with device colorants).
    NamedColor(NamedColorList),
    /// `cmsSigColorantTableType` — colorant table (named-colour list, PCS only).
    ColorantTable(NamedColorList),
    /// `pseq` / `psid` — a profile-sequence description.
    Seq(Seq),
    /// `cmsSigMeasurementType`.
    Measurement(Measurement),
    /// `cmsSigUcrBgType`.
    UcrBg(UcrBg),
    /// `cmsSigCrdInfoType` — PostScript CRD names in an MLU (language `"PS"`).
    CrdInfo(Mlu),
    /// `cmsSigScreeningType`.
    Screening(Screening),
    /// `cmsSigViewingConditionsType`.
    ViewingConditions(ViewingConditions),
    /// `cmsSigDictType` — metadata dictionary.
    Dict(Dict),
    /// `cmsSigcicpType`.
    Cicp(VideoSignal),
    /// `cmsSigVcgtType` — video-card gamma; always exactly three tone curves.
    Vcgt(Vec<ToneCurve>),
    /// `cmsSigMHC2Type`.
    Mhc2(Mhc2),
}
