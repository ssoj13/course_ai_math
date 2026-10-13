//! Common LUT Format (CLF) parser and writer.
//!
//! CLF is an XML-based LUT format defined by the Academy of Motion Picture
//! Arts and Sciences (AMPAS). It supports multiple processing operations
//! in a single file, making it ideal for complex color pipelines.
//!
//! # Supported Node Types
//!
//! - [`LUT1D`](ProcessNode::Lut1D) - 1-dimensional lookup table
//! - [`LUT3D`](ProcessNode::Lut3D) - 3-dimensional lookup table
//! - [`Matrix`](ProcessNode::Matrix) - 3x3 or 3x4 color matrix
//! - [`Range`](ProcessNode::Range) - Input/output domain scaling
//! - [`ASC_CDL`](ProcessNode::Cdl) - ASC Color Decision List (Slope/Offset/Power)
//! - [`Log`](ProcessNode::Log) - Logarithmic transform
//! - [`Exponent`](ProcessNode::Exponent) - Power/gamma function
//!
//! # File Structure
//!
//! ```xml
//! <?xml version="1.0" encoding="UTF-8"?>
//! <ProcessList id="example" compCLFversion="3.0">
//!   <Description>Example CLF</Description>
//!   <LUT1D inBitDepth="32f" outBitDepth="32f">
//!     <Array dim="1024 3">
//!       <!-- data -->
//!     </Array>
//!   </LUT1D>
//!   <LUT3D inBitDepth="32f" outBitDepth="32f">
//!     <Array dim="33 33 33 3">
//!       <!-- data -->
//!     </Array>
//!   </LUT3D>
//! </ProcessList>
//! ```
//!
//! This module reads and writes the file; it does not evaluate it. A CLF/CTF is processed by
//! vfx-ocio (`FileTransform`), which compiles each node into the same ops as the matching OCIO
//! transform (one evaluator, as OCIO's `FileFormatCTF` builds ops).
//!
//! # Example
//!
//! ```rust,no_run
//! use vfx_lut::clf::read_clf;
//! use std::path::Path;
//!
//! let clf = read_clf(Path::new("grade.clf")).unwrap();
//! println!("{} nodes", clf.nodes.len());
//! ```
//!
//! # References
//!
//! - [CLF Specification](https://acescentral.com/clf/)
//! - [S-2014-006 Common LUT Format](https://github.com/ampas/CLF)

use crate::{Interpolation, Lut1D, Lut3D, LutError, LutResult};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

/// CLF version supported by this implementation.
pub const CLF_VERSION: &str = "3.0";

/// CTF version supported by this implementation.
pub const CTF_VERSION: &str = "2.0";

/// Re-export BitDepth from vfx-core.
pub use vfx_core::BitDepth;

/// `ASC_CDL` style (OCIO `CDLOpData::Style`). CLF names `Fwd`, `Rev`, `FwdNoClamp`,
/// `RevNoClamp`; CTF also `v1.2_Fwd`, `v1.2_Rev`, `noClampFwd`, `noClampRev` (case-insensitive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CdlStyle {
    /// ASC CDL v1.2 with its [0, 1] clamps, forward.
    #[default]
    Fwd,
    /// ASC CDL v1.2, inverse.
    Rev,
    /// No clamps, forward.
    FwdNoClamp,
    /// No clamps, inverse.
    RevNoClamp,
}

impl CdlStyle {
    /// OCIO `CDLOpData::GetStyle`.
    fn from_clf(s: &str) -> Option<Self> {
        [
            ("v1.2_fwd", Self::Fwd),
            ("fwd", Self::Fwd),
            ("v1.2_rev", Self::Rev),
            ("rev", Self::Rev),
            ("noclampfwd", Self::FwdNoClamp),
            ("fwdnoclamp", Self::FwdNoClamp),
            ("noclamprev", Self::RevNoClamp),
            ("revnoclamp", Self::RevNoClamp),
        ]
        .into_iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(s))
        .map(|(_, v)| v)
    }

    /// The CLF name (`CDLOpData::GetStyleName`).
    pub fn clf_name(self) -> &'static str {
        match self {
            Self::Fwd => "Fwd",
            Self::Rev => "Rev",
            Self::FwdNoClamp => "FwdNoClamp",
            Self::RevNoClamp => "RevNoClamp",
        }
    }
}

/// `ASC_CDL` node: SOP per channel, saturation, style.
#[derive(Debug, Clone, PartialEq)]
pub struct CdlParams {
    /// Slope per channel [R, G, B].
    pub slope: [f64; 3],
    /// Offset per channel [R, G, B].
    pub offset: [f64; 3],
    /// Power per channel [R, G, B].
    pub power: [f64; 3],
    /// Saturation.
    pub saturation: f64,
    /// Style (direction and clamping).
    pub style: CdlStyle,
}

impl Default for CdlParams {
    fn default() -> Self {
        Self {
            slope: [1.0; 3],
            offset: [0.0; 3],
            power: [1.0; 3],
            saturation: 1.0,
            style: CdlStyle::Fwd,
        }
    }
}

/// `Log` node style (OCIO `LogUtils` / CLF v3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogStyle {
    /// `log10`.
    #[default]
    Log10,
    /// `log2`.
    Log2,
    /// `antiLog10`.
    AntiLog10,
    /// `antiLog2`.
    AntiLog2,
    /// `linToLog`: `logSideSlope * log(linSideSlope * x + linSideOffset, base) + logSideOffset`.
    LinToLog,
    /// `logToLin`, the inverse.
    LogToLin,
    /// `cameraLinToLog`: `linToLog` above `linSideBreak`, a straight line below it.
    CameraLinToLog,
    /// `cameraLogToLin`, the inverse.
    CameraLogToLin,
}

impl LogStyle {
    const NAMES: [(&'static str, LogStyle); 8] = [
        ("log10", LogStyle::Log10),
        ("log2", LogStyle::Log2),
        ("antiLog10", LogStyle::AntiLog10),
        ("antiLog2", LogStyle::AntiLog2),
        ("linToLog", LogStyle::LinToLog),
        ("logToLin", LogStyle::LogToLin),
        ("cameraLinToLog", LogStyle::CameraLinToLog),
        ("cameraLogToLin", LogStyle::CameraLogToLin),
    ];

    fn from_clf(s: &str) -> Option<Self> {
        Self::NAMES
            .into_iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(s))
            .map(|(_, v)| v)
    }

    /// The CLF name.
    pub fn clf_name(self) -> &'static str {
        Self::NAMES
            .into_iter()
            .find(|(_, v)| *v == self)
            .map(|(k, _)| k)
            .unwrap_or("log10")
    }
}

/// `Log` node: style and per-channel `LogParams` (OCIO `LogOpData`; the base is shared).
#[derive(Debug, Clone, PartialEq)]
pub struct LogParams {
    /// Style.
    pub style: LogStyle,
    /// Logarithm base (`LogParams base`, default 2; `log10`/`log2` fix it).
    pub base: f64,
    /// `logSideSlope` per channel.
    pub log_side_slope: [f64; 3],
    /// `logSideOffset` per channel.
    pub log_side_offset: [f64; 3],
    /// `linSideSlope` per channel.
    pub lin_side_slope: [f64; 3],
    /// `linSideOffset` per channel.
    pub lin_side_offset: [f64; 3],
    /// `linSideBreak` per channel (camera styles).
    pub lin_side_break: Option<[f64; 3]>,
    /// `linearSlope` per channel (camera styles; derived from the break when absent).
    pub linear_slope: Option<[f64; 3]>,
}

impl Default for LogParams {
    fn default() -> Self {
        Self {
            style: LogStyle::Log10,
            base: 2.0,
            log_side_slope: [1.0; 3],
            log_side_offset: [0.0; 3],
            lin_side_slope: [1.0; 3],
            lin_side_offset: [0.0; 3],
            lin_side_break: None,
            linear_slope: None,
        }
    }
}

/// `Exponent` node style (OCIO `GammaOpData::Style`, CLF names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExponentStyle {
    /// `basicFwd`: `pow(max(0, x), g)`.
    #[default]
    BasicFwd,
    /// `basicRev`.
    BasicRev,
    /// `basicMirrorFwd`: `sign(x) * pow(|x|, g)`.
    BasicMirrorFwd,
    /// `basicMirrorRev`.
    BasicMirrorRev,
    /// `basicPassThruFwd`: negatives unchanged.
    BasicPassThruFwd,
    /// `basicPassThruRev`.
    BasicPassThruRev,
    /// `monCurveFwd`: the moncurve (power with a linear toe), negatives on the linear segment.
    MonCurveFwd,
    /// `monCurveRev`.
    MonCurveRev,
    /// `monCurveMirrorFwd`.
    MonCurveMirrorFwd,
    /// `monCurveMirrorRev`.
    MonCurveMirrorRev,
}

impl ExponentStyle {
    const NAMES: [(&'static str, ExponentStyle); 10] = [
        ("basicFwd", ExponentStyle::BasicFwd),
        ("basicRev", ExponentStyle::BasicRev),
        ("basicMirrorFwd", ExponentStyle::BasicMirrorFwd),
        ("basicMirrorRev", ExponentStyle::BasicMirrorRev),
        ("basicPassThruFwd", ExponentStyle::BasicPassThruFwd),
        ("basicPassThruRev", ExponentStyle::BasicPassThruRev),
        ("monCurveFwd", ExponentStyle::MonCurveFwd),
        ("monCurveRev", ExponentStyle::MonCurveRev),
        ("monCurveMirrorFwd", ExponentStyle::MonCurveMirrorFwd),
        ("monCurveMirrorRev", ExponentStyle::MonCurveMirrorRev),
    ];

    fn from_clf(s: &str) -> Option<Self> {
        Self::NAMES
            .into_iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(s))
            .map(|(_, v)| v)
    }

    /// The CLF name.
    pub fn clf_name(self) -> &'static str {
        Self::NAMES
            .into_iter()
            .find(|(_, v)| *v == self)
            .map(|(k, _)| k)
            .unwrap_or("basicFwd")
    }

    /// A moncurve style (these take an `offset`).
    pub fn is_moncurve(self) -> bool {
        matches!(
            self,
            Self::MonCurveFwd
                | Self::MonCurveRev
                | Self::MonCurveMirrorFwd
                | Self::MonCurveMirrorRev
        )
    }
}

/// `Exponent` node: style and per-channel `ExponentParams` (R, G, B, A; OCIO `GammaOpData`).
#[derive(Debug, Clone, PartialEq)]
pub struct ExponentParams {
    /// Style.
    pub style: ExponentStyle,
    /// `exponent` per channel (R, G, B, A); A defaults to 1.
    pub exponent: [f64; 4],
    /// `offset` per channel (moncurve styles only); A defaults to 0.
    pub offset: [f64; 4],
}

impl Default for ExponentParams {
    fn default() -> Self {
        Self {
            style: ExponentStyle::BasicFwd,
            exponent: [1.0; 4],
            offset: [0.0; 4],
        }
    }
}

/// Range (domain scaling) parameters: OCIO's `RangeOpData`.
///
/// CLF bounds are single values shared by R, G and B (`CTFReaderRangeValueElt` rejects more
/// than one); any of them may be absent. The reader stores them normalised by the node's bit
/// depths (`RangeOpData::normalize`), so they are in the 0..1 scale of a 32f node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeParams {
    /// Minimum input value.
    pub min_in: Option<f64>,
    /// Maximum input value.
    pub max_in: Option<f64>,
    /// Minimum output value.
    pub min_out: Option<f64>,
    /// Maximum output value.
    pub max_out: Option<f64>,
    /// Clamp to the output bounds (`style="Clamp"`, the CLF default); `false` is
    /// `style="noClamp"`, which OCIO turns into a scale/offset matrix.
    pub clamp: bool,
}

impl Default for RangeParams {
    fn default() -> Self {
        Self {
            min_in: Some(0.0),
            max_in: Some(1.0),
            min_out: Some(0.0),
            max_out: Some(1.0),
            clamp: true,
        }
    }
}

/// A validated range as OCIO evaluates it: `out = in * scale + offset`, then clamped to
/// `[clamp_min, clamp_max]` where present.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeAffine {
    /// Scale.
    pub scale: f64,
    /// Offset.
    pub offset: f64,
    /// Lower output bound, if clamping.
    pub clamp_min: Option<f64>,
    /// Upper output bound, if clamping.
    pub clamp_max: Option<f64>,
}

impl RangeParams {
    /// Validates the bounds and computes the scale/offset: OCIO `RangeOpData::validate` +
    /// `fillScaleOffset`, plus `convertToMatrix`'s rule for the non-clamping style. The error
    /// messages are OCIO's.
    pub fn affine(&self) -> LutResult<RangeAffine> {
        let fail = |m: &str| Err(LutError::ParseError(m.to_string()));
        let (min_in, max_in, min_out, max_out) =
            (self.min_in, self.max_in, self.min_out, self.max_out);
        if min_in.is_some() != min_out.is_some() {
            return fail("In and out minimum limits must be both set or both missing in Range.");
        }
        if max_in.is_some() != max_out.is_some() {
            return fail("In and out maximum limits must be both set or both missing in Range.");
        }
        let (scale, offset) = match (min_in.zip(min_out), max_in.zip(max_out)) {
            (None, None) => {
                return fail("At least minimum or maximum limits must be set in Range.");
            }
            (Some((lo_in, lo_out)), Some((hi_in, hi_out))) => {
                // Polarity inversion is not allowed.
                if lo_in > hi_in {
                    return fail("Range maximum input value is less than minimum input value");
                }
                if lo_out > hi_out {
                    return fail("Range maximum output value is less than minimum output value");
                }
                let denom = hi_in - lo_in;
                if denom.abs() < 1e-6 {
                    return fail("Range maxInValue is too close to minInValue");
                }
                // Out min == max is allowed: a constant.
                let scale = (hi_out - lo_out) / denom;
                (scale, lo_out - scale * lo_in)
            }
            // A one-sided range is a pure clamp: CLF v3 requires matching in/out bounds.
            (Some((lo_in, lo_out)), None) => {
                if floats_differ(lo_out, lo_in) {
                    return fail(
                        "In and out minimum limits must be equal if maximum values are missing in Range.",
                    );
                }
                (1.0, 0.0)
            }
            (None, Some((hi_in, hi_out))) => {
                if floats_differ(hi_out, hi_in) {
                    return fail(
                        "In and out maximum limits must be equal if minimum values are missing in Range.",
                    );
                }
                (1.0, 0.0)
            }
        };
        if !self.clamp {
            if min_in.is_none() || max_in.is_none() {
                return fail("Non-clamping Range min & max values have to be set.");
            }
            return Ok(RangeAffine {
                scale,
                offset,
                clamp_min: None,
                clamp_max: None,
            });
        }
        Ok(RangeAffine {
            scale,
            offset,
            clamp_min: min_out,
            clamp_max: max_out,
        })
    }
}

/// OCIO `RangeOpData::FloatsDiffer`: absolute 1e-6 near zero, relative 1e-6 otherwise.
fn floats_differ(x1: f64, x2: f64) -> bool {
    if x1.abs() < 1e-3 {
        (x1 - x2).abs() > 1e-6
    } else {
        (1.0 - x2 / x1).abs() > 1e-6
    }
}

/// A processing node in the CLF ProcessList.
///
/// Each node represents one color transformation step.
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessNode {
    /// 1D lookup table.
    Lut1D {
        /// LUT data.
        lut: Lut1D,
        /// Input bit depth.
        in_depth: BitDepth,
        /// Output bit depth.
        out_depth: BitDepth,
    },

    /// 3D lookup table.
    Lut3D {
        /// LUT data.
        lut: Lut3D,
        /// Input bit depth.
        in_depth: BitDepth,
        /// Output bit depth.
        out_depth: BitDepth,
    },

    /// Color matrix: 3x3, 3x4 (offsets), 4x4 (alpha) or 4x5 (alpha and offsets).
    Matrix {
        /// Matrix values (row-major, 9, 12, 16 or 20 elements; the offsets are the last column).
        values: Vec<f32>,
        /// Input bit depth.
        in_depth: BitDepth,
        /// Output bit depth.
        out_depth: BitDepth,
    },

    /// Range (domain scaling).
    Range(RangeParams),

    /// ASC Color Decision List.
    Cdl(CdlParams),

    /// Logarithmic transform.
    Log(LogParams),

    /// Exponent/gamma.
    Exponent(ExponentParams),
}

/// A CLF ProcessList containing multiple processing nodes.
///
/// This is the root element of a CLF file, containing metadata
/// and an ordered list of color transformations.
///
/// # Example
///
/// ```rust
/// use vfx_lut::clf::{ProcessList, ProcessNode};
/// use vfx_lut::Lut1D;
///
/// let mut clf = ProcessList::new("my_grade");
/// clf.description = Some("Primary grade".into());
///
/// // Add a gamma curve
/// clf.nodes.push(ProcessNode::Lut1D {
///     lut: Lut1D::gamma(1024, 2.2),
///     in_depth: Default::default(),
///     out_depth: Default::default(),
/// });
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessList {
    /// Unique identifier for this process list.
    pub id: String,
    /// Optional human-readable name.
    pub name: Option<String>,
    /// Optional description.
    pub description: Option<String>,
    /// Input color space descriptor.
    pub input_descriptor: Option<String>,
    /// Output color space descriptor.
    pub output_descriptor: Option<String>,
    /// CLF version.
    pub version: String,
    /// Ordered list of processing nodes.
    pub nodes: Vec<ProcessNode>,
}

impl ProcessList {
    /// Creates a new empty ProcessList.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for this process list
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: None,
            description: None,
            input_descriptor: None,
            output_descriptor: None,
            version: CLF_VERSION.into(),
            nodes: Vec::new(),
        }
    }
}

/// Reads a CLF file from disk.
///
/// Parses the XML structure and returns a [`ProcessList`] containing
/// all processing nodes.
///
/// # Arguments
///
/// * `path` - Path to the .clf file
///
/// # Errors
///
/// Returns error if file cannot be read or contains invalid CLF data.
///
/// # Example
///
/// ```rust,no_run
/// use vfx_lut::clf::read_clf;
/// use std::path::Path;
///
/// let clf = read_clf(Path::new("grade.clf")).unwrap();
/// println!("CLF has {} nodes", clf.nodes.len());
/// ```
pub fn read_clf(path: &Path) -> LutResult<ProcessList> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    parse_clf(reader)
}

/// Reads an Autodesk CTF (Color Transform Format) file.
///
/// CTF is Autodesk's extended version of CLF used in Flame/Smoke.
/// This function parses both standard CLF elements and CTF-specific
/// extensions like InvLut1D, InvLut3D, ReferenceSpace.
///
/// # Arguments
///
/// * `path` - Path to the .ctf file
///
/// # Example
///
/// ```rust,no_run
/// use vfx_lut::clf::read_ctf;
/// use std::path::Path;
///
/// let ctf = read_ctf(Path::new("grade.ctf")).unwrap();
/// ```
pub fn read_ctf(path: &Path) -> LutResult<ProcessList> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    parse_ctf(reader)
}

/// Parses CTF from a reader.
///
/// CTF uses slightly different root elements than CLF but shares
/// most processing nodes.
pub fn parse_ctf<R: BufRead>(reader: R) -> LutResult<ProcessList> {
    // CTF parsing uses same logic as CLF with extended element support
    parse_clf_internal(reader, true)
}

/// Parses CLF from a reader.
///
/// # Arguments
///
/// * `reader` - Any type implementing `BufRead`
pub fn parse_clf<R: BufRead>(reader: R) -> LutResult<ProcessList> {
    parse_clf_internal(reader, false)
}

/// Internal parser supporting both CLF and CTF formats.
fn parse_clf_internal<R: BufRead>(reader: R, ctf_mode: bool) -> LutResult<ProcessList> {
    let mut xml = Reader::from_reader(reader);
    // Text is collected piecewise by `crate::xml_text` and trimmed where it is used.
    xml.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut result: Option<ProcessList> = None;
    let mut current_text = String::new();
    let mut in_array = false;
    let mut array_data = String::new();
    let mut array_dim: Vec<usize> = Vec::new();
    let mut current_node: Option<(&'static str, BitDepth, BitDepth)> = None;
    let mut current_interp = Interpolation::Linear;
    // <Range>: minIn, maxIn, minOut, maxOut as written (bit-depth scaled), and the style.
    let mut range_bounds: [Option<f64>; 4] = [None; 4];
    let mut range_clamp = true;
    // <ASC_CDL>, <Exponent>, <Log> being read.
    let mut cdl: Option<CdlParams> = None;
    let mut exponent: Option<ExponentParams> = None;
    let mut log: Option<LogParams> = None;
    // Open elements: the root is depth 0, its process nodes depth 1.
    let mut depth = 0usize;
    let fmt: &'static str = if ctf_mode { "CTF" } else { "CLF" };

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) => {
                let name = e.name().as_ref().to_string();
                if depth == 1 {
                    check_process_node(&name, fmt)?;
                }
                match name.as_str() {
                    "ExponentParams" => read_exponent_params(e, exponent.as_mut(), fmt)?,
                    "LogParams" => read_log_params(e, log.as_mut(), fmt)?,
                    // `<Log style="log10"/>`: a node with no parameters.
                    "Log" => {
                        if let Some(pl) = &mut result {
                            pl.nodes.push(ProcessNode::Log(start_log(e, fmt)?));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Start(ref e)) => {
                let name = e.name().as_ref().to_string();
                if depth == 1 {
                    check_process_node(&name, fmt)?;
                }
                depth += 1;
                current_text.clear();

                match name.as_str() {
                    "ASC_CDL" => {
                        let style = match attr_value(e, "style") {
                            None => CdlStyle::Fwd,
                            Some(v) => CdlStyle::from_clf(&v).ok_or_else(|| {
                                LutError::ParseError(format!("{fmt}: unknown ASC_CDL style {v:?}"))
                            })?,
                        };
                        cdl = Some(CdlParams {
                            style,
                            ..Default::default()
                        });
                    }
                    "Exponent" => {
                        let v = attr_value(e, "style").ok_or_else(|| {
                            LutError::ParseError(format!("{fmt}: Exponent has no style"))
                        })?;
                        let style = ExponentStyle::from_clf(&v).ok_or_else(|| {
                            LutError::ParseError(format!("{fmt}: unknown Exponent style {v:?}"))
                        })?;
                        exponent = Some(ExponentParams {
                            style,
                            ..Default::default()
                        });
                    }
                    "Log" => log = Some(start_log(e, fmt)?),
                    "ExponentParams" => read_exponent_params(e, exponent.as_mut(), fmt)?,
                    "LogParams" => read_log_params(e, log.as_mut(), fmt)?,
                    // CTF uses different root elements
                    "ProcessList" | "CtfFile" | "ColorTransformFile" => {
                        let mut id = String::new();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == "id" {
                                id = attr.value.clone().into();
                            }
                        }
                        result = Some(ProcessList::new(id));
                    }
                    "LUT1D" | "LUT3D" => {
                        let fmt: &'static str = if ctf_mode { "CTF" } else { "CLF" };
                        let mut in_depth = BitDepth::F32;
                        let mut out_depth = BitDepth::F32;
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                "inBitDepth" => {
                                    let s = attr.value.clone().into_owned();
                                    in_depth = BitDepth::from_clf_str(&s).ok_or({
                                        LutError::InvalidBitDepth {
                                            format: fmt,
                                            value: s,
                                        }
                                    })?;
                                }
                                "outBitDepth" => {
                                    let s = attr.value.clone().into_owned();
                                    out_depth = BitDepth::from_clf_str(&s).ok_or({
                                        LutError::InvalidBitDepth {
                                            format: fmt,
                                            value: s,
                                        }
                                    })?;
                                }
                                "interpolation" => {
                                    let s = attr.value.clone();
                                    current_interp = match s.as_ref() {
                                        "nearest" => Interpolation::Nearest,
                                        "tetrahedral" => Interpolation::Tetrahedral,
                                        _ => Interpolation::Linear,
                                    };
                                }
                                // COLOR-05: halfDomain/rawHalfs/hueAdjust (LUT1D-only per
                                // CTFReaderLut1DElt::isOpParameterValid, CTFReaderHelper.cpp:4242-4247)
                                // change how the array must be interpreted (half-float bit-pattern
                                // indexing / values, dw3 hue correction). This crate's Lut1D has no
                                // half-domain indexing, so honouring them would require changes to
                                // lut1d.rs, which is out of scope here — fail loudly by name instead
                                // of silently producing a LUT that ignores the attribute.
                                "halfDomain" if name == "LUT1D" => {
                                    return Err(LutError::Unsupported {
                                        format: fmt,
                                        feature: "halfDomain",
                                        detail: "half-float domain indexing is not implemented"
                                            .into(),
                                    });
                                }
                                "rawHalfs" if name == "LUT1D" => {
                                    return Err(LutError::Unsupported {
                                        format: fmt,
                                        feature: "rawHalfs",
                                        detail:
                                            "raw half-bit-pattern array values are not implemented"
                                                .into(),
                                    });
                                }
                                "hueAdjust" if name == "LUT1D" => {
                                    return Err(LutError::Unsupported {
                                        format: fmt,
                                        feature: "hueAdjust",
                                        detail: "dw3 hue adjustment is not implemented".into(),
                                    });
                                }
                                _ => {}
                            }
                        }
                        current_node = Some((
                            if name == "LUT1D" { "LUT1D" } else { "LUT3D" },
                            in_depth,
                            out_depth,
                        ));
                    }
                    "Matrix" => {
                        let fmt: &'static str = if ctf_mode { "CTF" } else { "CLF" };
                        let (in_depth, out_depth) = node_bit_depths(e, fmt)?;
                        current_node = Some(("Matrix", in_depth, out_depth));
                    }
                    // CTFReaderRangeElt(_1_7): `style="noClamp"` (case-insensitive) turns the
                    // clamp off; anything else, or no style, clamps.
                    "Range" => {
                        let fmt: &'static str = if ctf_mode { "CTF" } else { "CLF" };
                        let (in_depth, out_depth) = node_bit_depths(e, fmt)?;
                        range_bounds = [None; 4];
                        range_clamp = !e.attributes().flatten().any(|a| {
                            a.key.as_ref() == "style"
                                && a.value.as_ref().eq_ignore_ascii_case("noClamp")
                        });
                        current_node = Some(("Range", in_depth, out_depth));
                    }
                    "Array" => {
                        in_array = true;
                        array_data.clear();
                        array_dim.clear();
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == "dim" {
                                let dims = attr.value.clone();
                                // A silently-dropped bad token here shifts every later index
                                // (e.g. array_dim[1], the component count LUT1D/LUT3D below
                                // rely on) — must fail loudly rather than mis-parse.
                                for tok in dims.split_whitespace() {
                                    let d: usize = tok.parse().map_err(|_| {
                                        LutError::ParseError(format!(
                                            "CLF: Array 'dim' attribute has non-numeric value {tok:?}"
                                        ))
                                    })?;
                                    array_dim.push(d);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let out = if in_array {
                    &mut array_data
                } else {
                    &mut current_text
                };
                crate::xml_text::push_text(out, e);
            }
            Ok(Event::GeneralRef(ref e)) => {
                let out = if in_array {
                    &mut array_data
                } else {
                    &mut current_text
                };
                crate::xml_text::push_ref(out, e)?;
            }
            Ok(Event::CData(ref e)) => {
                let out = if in_array {
                    &mut array_data
                } else {
                    &mut current_text
                };
                crate::xml_text::push_cdata(out, e);
            }
            Ok(Event::End(ref e)) => {
                let name = e.name().as_ref().to_string();
                depth = depth.saturating_sub(1);

                match name.as_str() {
                    "Slope" | "Offset" | "Power" if cdl.is_some() => {
                        let v = parse_numbers(&current_text, fmt, &name)?;
                        let v: [f64; 3] = v.try_into().map_err(|v: Vec<f64>| {
                            LutError::ParseError(format!(
                                "{fmt}: ASC_CDL {name} needs 3 values, found {}",
                                v.len()
                            ))
                        })?;
                        if let Some(c) = cdl.as_mut() {
                            match name.as_str() {
                                "Slope" => c.slope = v,
                                "Offset" => c.offset = v,
                                _ => c.power = v,
                            }
                        }
                    }
                    "Saturation" if cdl.is_some() => {
                        let v = parse_numbers(&current_text, fmt, &name)?;
                        let [sat] = v.as_slice() else {
                            return Err(LutError::ParseError(format!(
                                "{fmt}: ASC_CDL Saturation needs 1 value, found {}",
                                v.len()
                            )));
                        };
                        if let Some(c) = cdl.as_mut() {
                            c.saturation = *sat;
                        }
                    }
                    "ASC_CDL" => {
                        if let (Some(pl), Some(c)) = (&mut result, cdl.take()) {
                            pl.nodes.push(ProcessNode::Cdl(c));
                        }
                    }
                    "Exponent" => {
                        if let (Some(pl), Some(x)) = (&mut result, exponent.take()) {
                            pl.nodes.push(ProcessNode::Exponent(x));
                        }
                    }
                    "Log" => {
                        if let (Some(pl), Some(l)) = (&mut result, log.take()) {
                            pl.nodes.push(ProcessNode::Log(l));
                        }
                    }
                    // Only the ProcessList's own children (depth 1 once closed): a process
                    // node's `<Description>` describes that node, not the list. OCIO keeps
                    // every list-level Description; they are joined by newlines here.
                    "Description" if depth == 1 => {
                        if let Some(ref mut pl) = result {
                            match &mut pl.description {
                                Some(d) => {
                                    d.push('\n');
                                    d.push_str(current_text.trim());
                                }
                                None => pl.description = Some(current_text.trim().to_string()),
                            }
                        }
                    }
                    "InputDescriptor" if depth == 1 => {
                        if let Some(ref mut pl) = result {
                            pl.input_descriptor = Some(current_text.trim().to_string());
                        }
                    }
                    "OutputDescriptor" if depth == 1 => {
                        if let Some(ref mut pl) = result {
                            pl.output_descriptor = Some(current_text.trim().to_string());
                        }
                    }
                    "Array" => {
                        in_array = false;
                    }
                    "LUT1D" => {
                        if let (Some(pl), Some((_, in_depth, out_depth))) =
                            (&mut result, current_node.take())
                        {
                            let fmt: &'static str = if ctf_mode { "CTF" } else { "CLF" };
                            let values: Vec<f32> = array_data
                                .split_whitespace()
                                .map(|s| {
                                    s.parse::<f32>().map_err(|_| {
                                        LutError::ParseError(format!(
                                            "{fmt}: LUT1D array value {s:?} is not a valid number"
                                        ))
                                    })
                                })
                                .collect::<LutResult<Vec<f32>>>()?;

                            if values.is_empty() {
                                return Err(LutError::ParseError(format!(
                                    "{fmt}: LUT1D Array has no values"
                                )));
                            }

                            // dim="<length> <components>": components tells mono (1) apart
                            // from 3-channel (3) — CTFReaderLut1DElt::updateDimension requires
                            // dims[1] in {1, 3} (CTFReaderHelper.cpp:4279).
                            let components = *array_dim.get(1).unwrap_or(&1);

                            // COLOR-02: raw array values are expressed in outBitDepth's integer
                            // domain and must be normalised to [0,1]
                            // (CTFReaderLut1DElt::end, CTFReaderHelper.cpp:4254: `scale = 1/outMax`).
                            let out_scale = out_depth.scale();

                            let lut = match components {
                                1 => {
                                    let r: Vec<f32> =
                                        values.iter().map(|v| v / out_scale).collect();
                                    Lut1D::from_data(r, 0.0, 1.0)?
                                }
                                // COLOR-03: a 3-channel array must stay 3-channel, not flatten
                                // to mono.
                                3 => {
                                    if !values.len().is_multiple_of(3) {
                                        return Err(LutError::ParseError(format!(
                                            "{fmt}: LUT1D Array declares 3 components but has {} values (not a multiple of 3)",
                                            values.len()
                                        )));
                                    }
                                    let n = values.len() / 3;
                                    let mut r = Vec::with_capacity(n);
                                    let mut g = Vec::with_capacity(n);
                                    let mut b = Vec::with_capacity(n);
                                    for chunk in values.as_chunks::<3>().0 {
                                        r.push(chunk[0] / out_scale);
                                        g.push(chunk[1] / out_scale);
                                        b.push(chunk[2] / out_scale);
                                    }
                                    Lut1D::from_rgb(r, g, b, 0.0, 1.0)?
                                }
                                other => {
                                    return Err(LutError::Unsupported {
                                        format: fmt,
                                        feature: "LUT1D Array components",
                                        detail: format!("{other} (only 1 or 3 supported)"),
                                    });
                                }
                            };

                            pl.nodes.push(ProcessNode::Lut1D {
                                lut,
                                in_depth,
                                out_depth,
                            });
                        }
                    }
                    "LUT3D" => {
                        if let (Some(pl), Some((_, in_depth, out_depth))) =
                            (&mut result, current_node.take())
                        {
                            let fmt: &'static str = if ctf_mode { "CTF" } else { "CLF" };
                            let values: Vec<f32> = array_data
                                .split_whitespace()
                                .map(|s| {
                                    s.parse::<f32>().map_err(|_| {
                                        LutError::ParseError(format!(
                                            "{fmt}: LUT3D array value {s:?} is not a valid number"
                                        ))
                                    })
                                })
                                .collect::<LutResult<Vec<f32>>>()?;

                            if array_dim.is_empty() {
                                return Err(LutError::ParseError(format!(
                                    "{fmt}: LUT3D Array is missing a 'dim' attribute"
                                )));
                            }
                            let size = array_dim[0];

                            // OCIO Lut3DOpData rejects a grid outside [2, Max3DLUTLength]; the
                            // count is checked arithmetic (a hostile "dim" must not overflow).
                            let total = crate::limits::check_3d_size(fmt, size)?
                                .checked_mul(3)
                                .ok_or_else(|| LutError::SizeOutOfRange {
                                    format: fmt,
                                    dims: vec![size as u64],
                                    reason: "LUT3D value count overflows".into(),
                                })?;

                            if values.len() != total {
                                return Err(LutError::ParseError(format!(
                                    "{fmt}: LUT3D Array declares size {size} (expects {total} values), found {}",
                                    values.len()
                                )));
                            }

                            // COLOR-02: raw array values are expressed in outBitDepth's integer
                            // domain and must be normalised to [0,1]
                            // (CTFReaderLut3DElt::end, CTFReaderHelper.cpp:4534).
                            let out_scale = out_depth.scale();
                            let data: Vec<[f32; 3]> = values
                                .as_chunks::<3>().0.iter()
                                .map(|c| [c[0] / out_scale, c[1] / out_scale, c[2] / out_scale])
                                .collect();

                            let lut =
                                Lut3D::from_data(data, size)?.with_interpolation(current_interp);
                            pl.nodes.push(ProcessNode::Lut3D {
                                lut,
                                in_depth,
                                out_depth,
                            });
                        }
                        current_interp = Interpolation::Linear;
                    }
                    // CTFReaderRangeValueElt::setRawData: exactly one number per bound.
                    "minInValue" | "maxInValue" | "minOutValue" | "maxOutValue" => {
                        let fmt: &'static str = if ctf_mode { "CTF" } else { "CLF" };
                        let values: Vec<f64> = current_text
                            .split_whitespace()
                            .map(|t| {
                                t.parse::<f64>().map_err(|_| {
                                    LutError::ParseError(format!(
                                        "{fmt}: Illegal '{name}' value {t:?}"
                                    ))
                                })
                            })
                            .collect::<LutResult<_>>()?;
                        if values.len() != 1 {
                            return Err(LutError::ParseError(format!(
                                "{fmt}: Range element: non-single value."
                            )));
                        }
                        let slot = match name.as_str() {
                            "minInValue" => 0,
                            "maxInValue" => 1,
                            "minOutValue" => 2,
                            _ => 3,
                        };
                        range_bounds[slot] = Some(values[0]);
                    }
                    // CTFReaderRangeElt::end: normalise by the bit depths, then validate.
                    "Range" => {
                        if let (Some(pl), Some((_, in_depth, out_depth))) =
                            (&mut result, current_node.take())
                        {
                            let in_scale = in_depth.scale() as f64;
                            let out_scale = out_depth.scale() as f64;
                            let [min_in, max_in, min_out, max_out] = range_bounds;
                            let params = RangeParams {
                                min_in: min_in.map(|v| v / in_scale),
                                max_in: max_in.map(|v| v / in_scale),
                                min_out: min_out.map(|v| v / out_scale),
                                max_out: max_out.map(|v| v / out_scale),
                                clamp: range_clamp,
                            };
                            params.affine()?;
                            pl.nodes.push(ProcessNode::Range(params));
                        }
                    }
                    "Matrix" => {
                        if let (Some(pl), Some((_, in_depth, out_depth))) =
                            (&mut result, current_node.take())
                        {
                            let fmt: &'static str = if ctf_mode { "CTF" } else { "CLF" };
                            let mut values: Vec<f32> = array_data
                                .split_whitespace()
                                .map(|s| {
                                    s.parse::<f32>().map_err(|_| {
                                        LutError::ParseError(format!(
                                            "{fmt}: Matrix array value {s:?} is not a valid number"
                                        ))
                                    })
                                })
                                .collect::<LutResult<Vec<f32>>>()?;

                            // 3x3, 3x4 (offsets), 4x4 (alpha), 4x5 (alpha and offsets), as
                            // `CTFReaderMatrixElt_1_3::endArray`.
                            if ![9, 12, 16, 20].contains(&values.len()) {
                                return Err(LutError::ParseError(format!(
                                    "{fmt}: Matrix Array must have 9 (3x3), 12 (3x4), 16 (4x4) or 20 (4x5) values, found {}",
                                    values.len()
                                )));
                            }

                            // COLOR-02: the 3x3 block scales by inMax/outMax, the offset column
                            // (3x4 form only) scales by 1/outMax alone — they are NOT the same
                            // rule (MatrixOpData::scale, MatrixOpData.cpp:871-877; invoked from
                            // CTFReaderMatrixElt::end, CTFReaderHelper.cpp:4669).
                            let in_scale = in_depth.scale();
                            let out_scale = out_depth.scale();
                            let combined = in_scale / out_scale;
                            let is_offset = matrix_offset_index(values.len());
                            for (i, v) in values.iter_mut().enumerate() {
                                if is_offset(i) {
                                    *v /= out_scale;
                                } else {
                                    *v *= combined;
                                }
                            }

                            pl.nodes.push(ProcessNode::Matrix {
                                values,
                                in_depth,
                                out_depth,
                            });
                        }
                    }
                    _ => {}
                }
                current_text.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(LutError::ParseError(format!("XML error: {}", e))),
            _ => {}
        }
        buf.clear();
    }

    result.ok_or_else(|| {
        LutError::ParseError(if ctf_mode {
            "missing CtfFile/ProcessList element".into()
        } else {
            "missing ProcessList element".into()
        })
    })
}

/// A process node's `inBitDepth` / `outBitDepth` attributes (32f when absent).
fn node_bit_depths(e: &BytesStart<'_>, fmt: &'static str) -> LutResult<(BitDepth, BitDepth)> {
    let mut depths = (BitDepth::F32, BitDepth::F32);
    for attr in e.attributes().flatten() {
        let slot = match attr.key.as_ref() {
            "inBitDepth" => &mut depths.0,
            "outBitDepth" => &mut depths.1,
            _ => continue,
        };
        let s = attr.value.clone().into_owned();
        *slot = BitDepth::from_clf_str(&s).ok_or(LutError::InvalidBitDepth {
            format: fmt,
            value: s,
        })?;
    }
    Ok(depths)
}

/// OCIO process nodes this reader does not implement. A file that uses one fails loudly: OCIO
/// would process it, so dropping it would give a different picture. Elements OCIO does not
/// know either are skipped, as OCIO does (with a warning there).
const UNSUPPORTED_OCIO_NODES: [&str; 12] = [
    "ACES",
    "ExposureContrast",
    "FixedFunction",
    "Function",
    "Gamma",
    "GradingHueCurve",
    "GradingPrimary",
    "GradingRGBCurve",
    "GradingTone",
    "InverseLUT1D",
    "InverseLUT3D",
    "Reference",
];

/// A direct child of the ProcessList: an OCIO process node this reader cannot build is an error.
fn check_process_node(name: &str, fmt: &'static str) -> LutResult<()> {
    if UNSUPPORTED_OCIO_NODES.contains(&name) {
        return Err(LutError::Unsupported {
            format: fmt,
            feature: "process node",
            detail: format!("<{name}> is an OCIO node this reader does not implement"),
        });
    }
    Ok(())
}

/// An attribute's value.
fn attr_value(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .map(|a| a.value.clone().into_owned())
}

/// Whitespace-separated numbers.
fn parse_numbers(text: &str, fmt: &'static str, what: &str) -> LutResult<Vec<f64>> {
    text.split_whitespace()
        .map(|t| {
            t.parse::<f64>().map_err(|_| {
                LutError::ParseError(format!("{fmt}: {what} value {t:?} is not a number"))
            })
        })
        .collect()
}

/// One number attribute, if present.
fn num_attr(e: &BytesStart<'_>, key: &str, fmt: &'static str) -> LutResult<Option<f64>> {
    match attr_value(e, key) {
        None => Ok(None),
        Some(v) => match parse_numbers(&v, fmt, key)?.as_slice() {
            [x] => Ok(Some(*x)),
            _ => Err(LutError::ParseError(format!(
                "{fmt}: {key} must be one number, got {v:?}"
            ))),
        },
    }
}

/// The channels a `*Params` element applies to: its `channel` attribute (R, G, B, A), or R, G, B.
fn param_channels(e: &BytesStart<'_>, fmt: &'static str) -> LutResult<Vec<usize>> {
    match attr_value(e, "channel") {
        None => Ok(vec![0, 1, 2]),
        Some(c) => match c.as_str() {
            "R" | "r" => Ok(vec![0]),
            "G" | "g" => Ok(vec![1]),
            "B" | "b" => Ok(vec![2]),
            "A" | "a" => Ok(vec![3]),
            _ => Err(LutError::ParseError(format!(
                "{fmt}: unknown channel {c:?}"
            ))),
        },
    }
}

/// `<ExponentParams exponent= offset= channel=>` (OCIO `CTFReaderGammaParamsElt`).
fn read_exponent_params(
    e: &BytesStart<'_>,
    node: Option<&mut ExponentParams>,
    fmt: &'static str,
) -> LutResult<()> {
    let Some(node) = node else { return Ok(()) };
    let exp = num_attr(e, "exponent", fmt)?
        .ok_or_else(|| LutError::ParseError(format!("{fmt}: ExponentParams has no exponent")))?;
    let offset = num_attr(e, "offset", fmt)?;
    if node.style.is_moncurve() && offset.is_none() {
        return Err(LutError::ParseError(format!(
            "{fmt}: {} needs an offset in ExponentParams",
            node.style.clf_name()
        )));
    }
    for c in param_channels(e, fmt)? {
        node.exponent[c] = exp;
        if let Some(o) = offset {
            node.offset[c] = o;
        }
    }
    Ok(())
}

/// `<Log style=>` (OCIO `CTFReaderLogElt`).
fn start_log(e: &BytesStart<'_>, fmt: &'static str) -> LutResult<LogParams> {
    let v = attr_value(e, "style")
        .ok_or_else(|| LutError::ParseError(format!("{fmt}: Log has no style")))?;
    let style = LogStyle::from_clf(&v)
        .ok_or_else(|| LutError::ParseError(format!("{fmt}: unknown Log style {v:?}")))?;
    let base = match style {
        LogStyle::Log10 | LogStyle::AntiLog10 => 10.0,
        _ => 2.0,
    };
    Ok(LogParams {
        style,
        base,
        ..Default::default()
    })
}

/// `<LogParams base= logSideSlope= ... channel=>` (OCIO `CTFReaderLogParamsElt`).
fn read_log_params(
    e: &BytesStart<'_>,
    node: Option<&mut LogParams>,
    fmt: &'static str,
) -> LutResult<()> {
    let Some(node) = node else { return Ok(()) };
    if let Some(b) = num_attr(e, "base", fmt)? {
        node.base = b;
    }
    let fields = [
        ("logSideSlope", num_attr(e, "logSideSlope", fmt)?),
        ("logSideOffset", num_attr(e, "logSideOffset", fmt)?),
        ("linSideSlope", num_attr(e, "linSideSlope", fmt)?),
        ("linSideOffset", num_attr(e, "linSideOffset", fmt)?),
        ("linSideBreak", num_attr(e, "linSideBreak", fmt)?),
        ("linearSlope", num_attr(e, "linearSlope", fmt)?),
    ];
    for c in param_channels(e, fmt)? {
        if c == 3 {
            return Err(LutError::ParseError(format!(
                "{fmt}: LogParams has no alpha channel"
            )));
        }
        for (key, v) in fields {
            let Some(v) = v else { continue };
            match key {
                "logSideSlope" => node.log_side_slope[c] = v,
                "logSideOffset" => node.log_side_offset[c] = v,
                "linSideSlope" => node.lin_side_slope[c] = v,
                "linSideOffset" => node.lin_side_offset[c] = v,
                "linSideBreak" => node.lin_side_break.get_or_insert([0.0; 3])[c] = v,
                _ => node.linear_slope.get_or_insert([0.0; 3])[c] = v,
            }
        }
    }
    Ok(())
}

/// Writes an Autodesk CTF file.
///
/// CTF files use the same structure as CLF but with .ctf extension
/// and a different root element.
pub fn write_ctf(path: &Path, clf: &ProcessList) -> LutResult<()> {
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    write_ctf_to(writer, clf)
}

/// Writes CTF to any writer.
pub fn write_ctf_to<W: Write>(writer: W, clf: &ProcessList) -> LutResult<()> {
    write_process_list(writer, clf, SubFormat::Ctf)
}

/// Writes a CLF file to disk.
///
/// # Arguments
///
/// * `path` - Output path for the .clf file
/// * `clf` - ProcessList to write
///
/// # Errors
///
/// Returns error if file cannot be written.
///
/// # Example
///
/// ```rust,no_run
/// use vfx_lut::clf::{ProcessList, write_clf};
/// use std::path::Path;
///
/// let clf = ProcessList::new("my_grade");
/// write_clf(Path::new("output.clf"), &clf).unwrap();
/// ```
pub fn write_clf(path: &Path, clf: &ProcessList) -> LutResult<()> {
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    write_clf_to(writer, clf)
}

/// Writes CLF to any writer.
pub fn write_clf_to<W: Write>(writer: W, clf: &ProcessList) -> LutResult<()> {
    write_process_list(writer, clf, SubFormat::Clf)
}

/// The two XML dialects of one document (OCIO `CTFReaderTransform` `SubFormat`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SubFormat {
    Clf,
    Ctf,
}

/// The SMPTE ST 2136-1 namespace OCIO writes on a CLF root (`CTFTransform.cpp:41`).
const SMPTE_XMLNS_URL: &str = "http://www.smpte-ra.org/ns/2136-1/2024";

/// Writes a process list as OCIO's `TransformWriter::write` does (`CTFTransform.cpp:2615-2690`):
/// the root is `ProcessList` in both dialects; a CLF carries `compCLFversion` and the SMPTE
/// namespace, a CTF its `version`; then `id` and `name`.
fn write_process_list<W: Write>(writer: W, clf: &ProcessList, format: SubFormat) -> LutResult<()> {
    let mut xml = Writer::new_with_indent(writer, b' ', 2);

    xml_event(
        &mut xml,
        Event::Decl(quick_xml::events::BytesDecl::new(
            "1.0",
            Some("UTF-8"),
            None,
        )),
    )?;

    let mut root = BytesStart::new("ProcessList");
    match format {
        SubFormat::Clf => {
            root.push_attribute(("compCLFversion", clf.version.as_str()));
            root.push_attribute(("xmlns", SMPTE_XMLNS_URL));
        }
        SubFormat::Ctf => root.push_attribute(("version", CTF_VERSION)),
    }
    root.push_attribute(("id", clf.id.as_str()));
    if let Some(ref name) = clf.name {
        root.push_attribute(("name", name.as_str()));
    }
    xml_event(&mut xml, Event::Start(root))?;

    if let Some(ref desc) = clf.description {
        write_text_element(&mut xml, "Description", desc)?;
    }
    if let Some(ref inp) = clf.input_descriptor {
        write_text_element(&mut xml, "InputDescriptor", inp)?;
    }
    if let Some(ref out) = clf.output_descriptor {
        write_text_element(&mut xml, "OutputDescriptor", out)?;
    }
    for node in &clf.nodes {
        write_node(&mut xml, node)?;
    }
    xml_event(&mut xml, Event::End(BytesEnd::new("ProcessList")))
}

/// Helper to write a text element.
/// Writes one XML event.
fn xml_event<W: Write>(xml: &mut Writer<W>, event: Event<'_>) -> LutResult<()> {
    xml.write_event(event)
        .map_err(|e| LutError::ParseError(format!("write error: {e}")))
}

fn write_text_element<W: Write>(xml: &mut Writer<W>, name: &str, text: &str) -> LutResult<()> {
    xml.write_event(Event::Start(BytesStart::new(name)))
        .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;
    xml.write_event(Event::Text(BytesText::new(text)))
        .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;
    xml.write_event(Event::End(BytesEnd::new(name)))
        .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;
    Ok(())
}

/// Whether entry `i` of a Matrix array of `len` values is an offset (the last column of the 3x4
/// and 4x5 forms), which scales by the out depth alone.
fn matrix_offset_index(len: usize) -> impl Fn(usize) -> bool {
    move |i| match len {
        12 => i % 4 == 3,
        20 => i % 5 == 4,
        _ => false,
    }
}

/// The node that maps a LUT's input domain onto `[0, 1]`, which a CLF `LUT1D`/`LUT3D` cannot
/// state itself: a `Range` when the three channels share the domain, else a 3x4 `Matrix`
/// (scale and offset per channel; the LUT clamps its input anyway). `None` for `[0, 1]`.
fn domain_node(domain_min: [f32; 3], domain_max: [f32; 3]) -> Option<ProcessNode> {
    if domain_min == [0.0; 3] && domain_max == [1.0; 3] {
        return None;
    }
    if domain_min.iter().all(|&v| v == domain_min[0])
        && domain_max.iter().all(|&v| v == domain_max[0])
    {
        return Some(ProcessNode::Range(RangeParams {
            min_in: Some(domain_min[0] as f64),
            max_in: Some(domain_max[0] as f64),
            min_out: Some(0.0),
            max_out: Some(1.0),
            clamp: true,
        }));
    }
    let mut values = vec![0.0f32; 12];
    for c in 0..3 {
        let scale = 1.0 / (domain_max[c] - domain_min[c]);
        values[c * 4 + c] = scale;
        values[c * 4 + 3] = -domain_min[c] * scale;
    }
    Some(ProcessNode::Matrix {
        values,
        in_depth: BitDepth::F32,
        out_depth: BitDepth::F32,
    })
}

/// LUT array values in `out_depth`'s code range (the reader divides by the same scale), each at
/// the shortest precision that reads back to the same `f32`.
fn lut_values(values: impl Iterator<Item = f32>, out_depth: BitDepth, per_line: usize) -> String {
    let scale = out_depth.scale();
    let mut out = String::new();
    for (i, v) in values.enumerate() {
        if i > 0 {
            out.push(if i % per_line == 0 { '\n' } else { ' ' });
        }
        out.push_str(&(v * scale).to_string());
    }
    out
}

/// Helper to write a process node.
fn write_node<W: Write>(xml: &mut Writer<W>, node: &ProcessNode) -> LutResult<()> {
    match node {
        ProcessNode::Lut1D {
            lut,
            in_depth,
            out_depth,
        } => {
            if let Some(domain) = domain_node(lut.domain_min, lut.domain_max) {
                write_node(xml, &domain)?;
            }
            let mut start = BytesStart::new("LUT1D");
            start.push_attribute(("inBitDepth", in_depth.clf_str()));
            start.push_attribute(("outBitDepth", out_depth.clf_str()));
            xml.write_event(Event::Start(start))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;

            // Array
            let mut arr = BytesStart::new("Array");
            // All three channels (a mono LUT as one column): writing only `r` dropped G and B.
            let values = match (&lut.g, &lut.b) {
                (Some(g), Some(b)) => {
                    arr.push_attribute(("dim", format!("{} 3", lut.size()).as_str()));
                    let rgb = (0..lut.size()).flat_map(|i| [lut.r[i], g[i], b[i]]);
                    lut_values(rgb, *out_depth, 3)
                }
                _ => {
                    arr.push_attribute(("dim", format!("{} 1", lut.size()).as_str()));
                    lut_values(lut.r.iter().copied(), *out_depth, 1)
                }
            };
            xml.write_event(Event::Start(arr))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;
            xml.write_event(Event::Text(BytesText::new(&values)))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;

            xml.write_event(Event::End(BytesEnd::new("Array")))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;
            xml.write_event(Event::End(BytesEnd::new("LUT1D")))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;
        }
        ProcessNode::Lut3D {
            lut,
            in_depth,
            out_depth,
        } => {
            if let Some(domain) = domain_node(lut.domain_min, lut.domain_max) {
                write_node(xml, &domain)?;
            }
            let mut start = BytesStart::new("LUT3D");
            if lut.interpolation == Interpolation::Tetrahedral {
                start.push_attribute(("interpolation", "tetrahedral"));
            }
            start.push_attribute(("inBitDepth", in_depth.clf_str()));
            start.push_attribute(("outBitDepth", out_depth.clf_str()));
            xml.write_event(Event::Start(start))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;

            // Array
            let mut arr = BytesStart::new("Array");
            arr.push_attribute((
                "dim",
                format!("{} {} {} 3", lut.size, lut.size, lut.size).as_str(),
            ));
            xml.write_event(Event::Start(arr))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;

            let values = lut_values(lut.data.iter().flatten().copied(), *out_depth, 3);
            xml.write_event(Event::Text(BytesText::new(&values)))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;

            xml.write_event(Event::End(BytesEnd::new("Array")))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;
            xml.write_event(Event::End(BytesEnd::new("LUT3D")))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;
        }
        ProcessNode::Matrix {
            values,
            in_depth,
            out_depth,
        } => {
            let mut start = BytesStart::new("Matrix");
            start.push_attribute(("inBitDepth", in_depth.clf_str()));
            start.push_attribute(("outBitDepth", out_depth.clf_str()));
            xml.write_event(Event::Start(start))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;

            // 3x3, 3x4 (offsets), 4x4 (alpha) or 4x5 (alpha and offsets).
            let (rows, cols) = match values.len() {
                9 => (3, 3),
                12 => (3, 4),
                16 => (4, 4),
                20 => (4, 5),
                n => {
                    return Err(LutError::ParseError(format!(
                        "Matrix node must have 9, 12, 16 or 20 values, has {n}"
                    )));
                }
            };
            let mut arr = BytesStart::new("Array");
            arr.push_attribute(("dim", format!("{} {}", rows, cols).as_str()));
            xml.write_event(Event::Start(arr))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;

            // The inverse of the reader's bit-depth scaling, at full precision.
            let is_offset = matrix_offset_index(values.len());
            let (in_scale, out_scale) = (in_depth.scale(), out_depth.scale());
            let text: String = values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    if is_offset(i) {
                        v * out_scale
                    } else {
                        v * out_scale / in_scale
                    }
                    .to_string()
                })
                .collect::<Vec<_>>()
                .join(" ");
            xml.write_event(Event::Text(BytesText::new(&text)))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;

            xml.write_event(Event::End(BytesEnd::new("Array")))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;
            xml.write_event(Event::End(BytesEnd::new("Matrix")))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;
        }
        ProcessNode::Cdl(params) => {
            let mut start = BytesStart::new("ASC_CDL");
            start.push_attribute(("inBitDepth", "32f"));
            start.push_attribute(("outBitDepth", "32f"));
            start.push_attribute(("style", params.style.clf_name()));
            xml_event(xml, Event::Start(start))?;
            xml_event(xml, Event::Start(BytesStart::new("SOPNode")))?;
            let three = |v: [f64; 3]| format!("{} {} {}", v[0], v[1], v[2]);
            write_text_element(xml, "Slope", &three(params.slope))?;
            write_text_element(xml, "Offset", &three(params.offset))?;
            write_text_element(xml, "Power", &three(params.power))?;
            xml_event(xml, Event::End(BytesEnd::new("SOPNode")))?;
            xml_event(xml, Event::Start(BytesStart::new("SatNode")))?;
            write_text_element(xml, "Saturation", &format!("{}", params.saturation))?;
            xml_event(xml, Event::End(BytesEnd::new("SatNode")))?;
            xml_event(xml, Event::End(BytesEnd::new("ASC_CDL")))?;
        }
        ProcessNode::Range(params) => {
            // Bounds are stored normalised, so the node is written as 32f -> 32f.
            let mut start = BytesStart::new("Range");
            start.push_attribute(("inBitDepth", "32f"));
            start.push_attribute(("outBitDepth", "32f"));
            if !params.clamp {
                start.push_attribute(("style", "noClamp"));
            }
            xml.write_event(Event::Start(start))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;
            for (tag, value) in [
                ("minInValue", params.min_in),
                ("maxInValue", params.max_in),
                ("minOutValue", params.min_out),
                ("maxOutValue", params.max_out),
            ] {
                if let Some(v) = value {
                    write_text_element(xml, tag, &format!("{v}"))?;
                }
            }
            xml.write_event(Event::End(BytesEnd::new("Range")))
                .map_err(|e| LutError::ParseError(format!("write error: {}", e)))?;
        }
        ProcessNode::Log(params) => {
            let mut start = BytesStart::new("Log");
            start.push_attribute(("inBitDepth", "32f"));
            start.push_attribute(("outBitDepth", "32f"));
            start.push_attribute(("style", params.style.clf_name()));
            if matches!(
                params.style,
                LogStyle::Log10 | LogStyle::Log2 | LogStyle::AntiLog10 | LogStyle::AntiLog2
            ) {
                xml_event(xml, Event::Empty(start))?;
            } else {
                xml_event(xml, Event::Start(start))?;
                // One LogParams per channel (the reader takes both forms).
                for (c, ch) in ["R", "G", "B"].into_iter().enumerate() {
                    let mut p = BytesStart::new("LogParams");
                    p.push_attribute(("channel", ch));
                    p.push_attribute(("base", params.base.to_string().as_str()));
                    for (key, v) in [
                        ("logSideSlope", Some(params.log_side_slope[c])),
                        ("logSideOffset", Some(params.log_side_offset[c])),
                        ("linSideSlope", Some(params.lin_side_slope[c])),
                        ("linSideOffset", Some(params.lin_side_offset[c])),
                        ("linSideBreak", params.lin_side_break.map(|b| b[c])),
                        ("linearSlope", params.linear_slope.map(|b| b[c])),
                    ] {
                        if let Some(v) = v {
                            p.push_attribute((key, v.to_string().as_str()));
                        }
                    }
                    xml_event(xml, Event::Empty(p))?;
                }
                xml_event(xml, Event::End(BytesEnd::new("Log")))?;
            }
        }
        ProcessNode::Exponent(params) => {
            let mut start = BytesStart::new("Exponent");
            start.push_attribute(("inBitDepth", "32f"));
            start.push_attribute(("outBitDepth", "32f"));
            start.push_attribute(("style", params.style.clf_name()));
            xml_event(xml, Event::Start(start))?;
            // One ExponentParams per channel; alpha only when it is not the identity.
            for (c, ch) in ["R", "G", "B", "A"].into_iter().enumerate() {
                if c == 3 && params.exponent[3] == 1.0 && params.offset[3] == 0.0 {
                    continue;
                }
                let mut p = BytesStart::new("ExponentParams");
                p.push_attribute(("channel", ch));
                p.push_attribute(("exponent", params.exponent[c].to_string().as_str()));
                if params.style.is_moncurve() {
                    p.push_attribute(("offset", params.offset[c].to_string().as_str()));
                }
                xml_event(xml, Event::Empty(p))?;
            }
            xml_event(xml, Event::End(BytesEnd::new("Exponent")))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit_depth() {
        assert_eq!(BitDepth::U8.scale(), 255.0);
        assert_eq!(BitDepth::U10.scale(), 1023.0);
        assert_eq!(BitDepth::F32.scale(), 1.0);

        assert_eq!(BitDepth::from_clf_str("8i"), Some(BitDepth::U8));
        assert_eq!(BitDepth::from_clf_str("32f"), Some(BitDepth::F32));
        assert_eq!(BitDepth::from_clf_str("invalid"), None);
    }

    #[test]
    fn test_range() {
        let range = RangeParams {
            max_out: Some(0.5), // compress to half range
            ..Default::default()
        };
        let a = range.affine().unwrap();
        assert_eq!((a.scale, a.offset), (0.5, 0.0));
        assert_eq!((a.clamp_min, a.clamp_max), (Some(0.0), Some(0.5)));
    }

    /// B30: `<Range>` is read (it used to be dropped silently), bit-depth normalised, and
    /// written back so it reads the same. Values: PyOpenColorIO 2.5.1 on the same node.
    #[test]
    fn range_node_reads_and_round_trips() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="r" compCLFversion="3">
  <Range inBitDepth="10i" outBitDepth="12i" style="noClamp">
    <minInValue>64</minInValue><maxInValue>940</maxInValue>
    <minOutValue>256</minOutValue><maxOutValue>3760</maxOutValue>
  </Range>
</ProcessList>"#;
        let pl = parse_clf(xml.as_bytes()).unwrap();
        let [ProcessNode::Range(r)] = pl.nodes.as_slice() else {
            panic!("{:?}", pl.nodes)
        };
        assert_eq!(r.min_in, Some(64.0 / 1023.0));
        assert_eq!(r.max_out, Some(3760.0 / 4095.0));
        assert!(!r.clamp);
        // PyOCIO: [-0.5, 0.15, 2.0] -> [-0.4996337, 0.1498901, 1.9985348] (no clamp).
        let a = r.affine().unwrap();
        assert_eq!((a.clamp_min, a.clamp_max), (None, None));
        for (x, w) in [-0.5_f64, 0.15, 2.0]
            .into_iter()
            .zip([-0.4996337, 0.1498901, 1.9985348])
        {
            assert!((x * a.scale + a.offset - w).abs() < 2e-6);
        }

        let mut out = Vec::new();
        write_clf_to(&mut out, &pl).unwrap();
        let back = parse_clf(out.as_slice()).unwrap();
        let [ProcessNode::Range(b)] = back.nodes.as_slice() else {
            panic!("{:?}", back.nodes)
        };
        assert_eq!(b, r);
    }

    /// A document with every node kind the writer emits comes back equal, payloads included
    /// (these tests compared only the id, the description and the node count).
    fn every_node_kind(id: &str) -> ProcessList {
        let mut pl = ProcessList::new(id);
        pl.description = Some("Roundtrip test".into());
        pl.input_descriptor = Some("ACES".into());
        pl.output_descriptor = Some("sRGB".into());
        pl.nodes.push(ProcessNode::Lut1D {
            lut: Lut1D::gamma(64, 2.2),
            in_depth: BitDepth::F32,
            out_depth: BitDepth::F32,
        });
        pl.nodes.push(ProcessNode::Matrix {
            values: vec![
                0.9, 0.1, 0.0, 0.01, 0.05, 0.8, 0.15, -0.02, 0.0, 0.2, 0.7, 0.03,
            ],
            in_depth: BitDepth::F32,
            out_depth: BitDepth::F32,
        });
        let data: Vec<[f32; 3]> = (0..8)
            .map(|i| [i as f32 / 7.0, (i % 3) as f32 * 0.4, 1.0 - i as f32 / 9.0])
            .collect();
        pl.nodes.push(ProcessNode::Lut3D {
            lut: Lut3D::from_data(data, 2).unwrap(),
            in_depth: BitDepth::F32,
            out_depth: BitDepth::F32,
        });
        pl.nodes.push(ProcessNode::Cdl(CdlParams {
            slope: [1.1, 0.9, 1.05],
            offset: [0.01, -0.02, 0.0],
            power: [1.2, 1.0, 0.8],
            saturation: 0.85,
            ..Default::default()
        }));
        pl
    }

    #[test]
    fn test_clf_roundtrip() {
        let pl = every_node_kind("roundtrip_test");
        let mut buf = Vec::new();
        write_clf_to(&mut buf, &pl).unwrap();
        let parsed = parse_clf(std::io::Cursor::new(buf)).unwrap();
        assert_eq!(parsed, pl);
    }

    #[test]
    fn test_ctf_roundtrip() {
        let pl = every_node_kind("ctf_test");
        let mut buf = Vec::new();
        write_ctf_to(&mut buf, &pl).unwrap();
        assert!(String::from_utf8_lossy(&buf).contains("<ProcessList"));
        let parsed = parse_ctf(std::io::Cursor::new(buf)).unwrap();
        assert_eq!(parsed, pl);
    }

    #[test]
    fn test_parse_ctf_root_element() {
        // A CTF root is `ProcessList` (OCIO); this crate wrote `CtfFile` before 2026-09-25, and
        // the reader still takes those files.
        let ctf_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<CtfFile id="test_ctf" version="2.0">
    <Description>Test CTF file</Description>
    <Matrix inBitDepth="32f" outBitDepth="32f">
        <Array dim="3 3">
            1.0 0.0 0.0
            0.0 1.0 0.0
            0.0 0.0 1.0
        </Array>
    </Matrix>
</CtfFile>
"#;
        let parsed = parse_ctf(std::io::Cursor::new(ctf_content)).unwrap();
        assert_eq!(parsed.id, "test_ctf");
        assert_eq!(parsed.nodes.len(), 1);
    }

    /// `<ASC_CDL>`, `<Log>` and `<Exponent>` are read as OCIO reads them (they used to be
    /// write-only: the reader had no arm for them and dropped them), and what the writer emits
    /// reads back to the same node. The writer used to put three numbers in one `exponent`
    /// attribute and no bit depths, which OCIO rejects.
    #[test]
    fn cdl_log_exponent_nodes_read_and_round_trip() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="t" compCLFversion="3">
  <ASC_CDL inBitDepth="10i" outBitDepth="12i" style="RevNoClamp">
    <Description>grade</Description>
    <SOPNode><Slope>1.1 0.9 1.2</Slope><Offset>0.01 -0.02 0.03</Offset><Power>1.2 0.8 1.1</Power></SOPNode>
    <SatNode><Saturation>0.8</Saturation></SatNode>
  </ASC_CDL>
  <Exponent inBitDepth="32f" outBitDepth="32f" style="basicMirrorRev">
    <ExponentParams channel="R" exponent="2.2"/><ExponentParams channel="G" exponent="1.8"/>
    <ExponentParams channel="B" exponent="2.6"/>
  </Exponent>
  <Exponent inBitDepth="32f" outBitDepth="32f" style="monCurveFwd">
    <ExponentParams exponent="2.4" offset="0.055"/>
  </Exponent>
  <Log inBitDepth="32f" outBitDepth="32f" style="log10"/>
  <Log inBitDepth="32f" outBitDepth="32f" style="cameraLogToLin">
    <LogParams base="2" logSideSlope="0.07" logSideOffset="0.55" linSideSlope="1"
               linSideOffset="0.0078" linSideBreak="0.0078" linearSlope="5.2"/>
  </Log>
</ProcessList>"#;
        let pl = parse_clf(xml.as_bytes()).unwrap();
        let [
            ProcessNode::Cdl(cdl),
            ProcessNode::Exponent(e1),
            ProcessNode::Exponent(e2),
            ProcessNode::Log(l1),
            ProcessNode::Log(l2),
        ] = pl.nodes.as_slice()
        else {
            panic!("{:?}", pl.nodes)
        };
        assert_eq!(cdl.style, CdlStyle::RevNoClamp);
        assert_eq!(cdl.slope, [1.1, 0.9, 1.2]);
        assert_eq!(cdl.offset, [0.01, -0.02, 0.03]);
        assert_eq!(cdl.power, [1.2, 0.8, 1.1]);
        assert_eq!(cdl.saturation, 0.8);
        assert_eq!(e1.style, ExponentStyle::BasicMirrorRev);
        assert_eq!(e1.exponent, [2.2, 1.8, 2.6, 1.0]);
        assert_eq!(e2.style, ExponentStyle::MonCurveFwd);
        assert_eq!(
            (e2.exponent, e2.offset),
            ([2.4, 2.4, 2.4, 1.0], [0.055, 0.055, 0.055, 0.0])
        );
        assert_eq!((l1.style, l1.base), (LogStyle::Log10, 10.0));
        assert_eq!(l2.style, LogStyle::CameraLogToLin);
        assert_eq!(
            (l2.base, l2.log_side_slope, l2.lin_side_offset),
            (2.0, [0.07; 3], [0.0078; 3])
        );
        assert_eq!(
            (l2.lin_side_break, l2.linear_slope),
            (Some([0.0078; 3]), Some([5.2; 3]))
        );

        let mut out = Vec::new();
        write_clf_to(&mut out, &pl).unwrap();
        let back = parse_clf(out.as_slice()).unwrap();
        assert_eq!(back.nodes.len(), pl.nodes.len());
        for (a, b) in pl.nodes.iter().zip(&back.nodes) {
            match (a, b) {
                (ProcessNode::Cdl(a), ProcessNode::Cdl(b)) => assert_eq!(a, b),
                (ProcessNode::Exponent(a), ProcessNode::Exponent(b)) => assert_eq!(a, b),
                (ProcessNode::Log(a), ProcessNode::Log(b)) => assert_eq!(a, b),
                _ => panic!("{a:?} vs {b:?}"),
            }
        }
    }

    /// An OCIO node this reader does not implement fails loudly (OCIO would process it); an
    /// element OCIO does not know either is skipped, as OCIO does (it warns).
    #[test]
    fn unsupported_ocio_nodes_fail_and_unknown_elements_are_skipped() {
        let wrap = |body: &str| {
            format!(
                "<ProcessList id=\"t\" compCLFversion=\"3\">{body}<Log inBitDepth=\"32f\" outBitDepth=\"32f\" style=\"log2\"/></ProcessList>"
            )
        };
        for node in [
            "ExposureContrast",
            "FixedFunction",
            "GradingPrimary",
            "InverseLUT1D",
        ] {
            let xml = wrap(&format!(
                "<{node} inBitDepth=\"32f\" outBitDepth=\"32f\"></{node}>"
            ));
            let err = parse_ctf(xml.as_bytes())
                .err()
                .unwrap_or_else(|| panic!("{node} read"));
            assert!(matches!(err, LutError::Unsupported { .. }), "{node}: {err}");
        }
        let pl =
            parse_clf(wrap("<Frobnicate a=\"1\"/><Info><Anything>x</Anything></Info>").as_bytes())
                .unwrap();
        assert_eq!(pl.nodes.len(), 1);
    }

    #[test]
    fn test_clf_lut1d_10bit_values_are_normalised() {
        // COLOR-02: raw array values are expressed in outBitDepth's integer domain (10i =>
        // [0, 1023]) and must be divided down to [0,1]. Against the pre-fix parser, the LUT
        // would hold [0.0, 511.0, 1023.0] verbatim and apply(0.5) would return ~511, not ~0.5.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="bd10" compCLFversion="3.0">
  <LUT1D inBitDepth="10i" outBitDepth="10i">
    <Array dim="3 1">
      0 511 1023
    </Array>
  </LUT1D>
</ProcessList>
"#;
        let clf = parse_clf(std::io::Cursor::new(xml)).unwrap();
        assert_eq!(clf.nodes.len(), 1);
        match &clf.nodes[0] {
            ProcessNode::Lut1D { lut, .. } => {
                assert!((lut.r[0] - 0.0).abs() < 1e-4, "r[0]={}", lut.r[0]);
                assert!(
                    (lut.r[1] - 511.0 / 1023.0).abs() < 1e-4,
                    "r[1]={}",
                    lut.r[1]
                );
                assert!((lut.r[2] - 1.0).abs() < 1e-4, "r[2]={}", lut.r[2]);
                assert!(
                    (lut.apply(0.5) - 0.5).abs() < 0.01,
                    "apply(0.5)={}",
                    lut.apply(0.5)
                );
            }
            other => panic!("expected Lut1D node, got {other:?}"),
        }
    }

    #[test]
    fn test_clf_lut1d_12bit_values_are_normalised() {
        // Same as the 10-bit case but at 12i (max 4095), to confirm the scale factor tracks
        // the declared bit depth rather than being hardcoded.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="bd12" compCLFversion="3.0">
  <LUT1D inBitDepth="12i" outBitDepth="12i">
    <Array dim="3 1">
      0 2047 4095
    </Array>
  </LUT1D>
</ProcessList>
"#;
        let clf = parse_clf(std::io::Cursor::new(xml)).unwrap();
        match &clf.nodes[0] {
            ProcessNode::Lut1D { lut, .. } => {
                assert!(
                    (lut.r[1] - 2047.0 / 4095.0).abs() < 1e-4,
                    "r[1]={}",
                    lut.r[1]
                );
                assert!(
                    (lut.apply(0.5) - 0.5).abs() < 0.01,
                    "apply(0.5)={}",
                    lut.apply(0.5)
                );
            }
            other => panic!("expected Lut1D node, got {other:?}"),
        }
    }

    #[test]
    fn test_clf_lut1d_3channel_keeps_distinct_channels() {
        // COLOR-03: dim="2 3" is a 3-channel array; the old code flattened all 6 values into
        // a single mono `r` channel (losing g/b entirely) instead of building an RGB Lut1D.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="rgb1d" compCLFversion="3.0">
  <LUT1D inBitDepth="32f" outBitDepth="32f">
    <Array dim="2 3">
      0.0 0.25 0.5
      1.0 0.75 0.6
    </Array>
  </LUT1D>
</ProcessList>
"#;
        let clf = parse_clf(std::io::Cursor::new(xml)).unwrap();
        match &clf.nodes[0] {
            ProcessNode::Lut1D { lut, .. } => {
                assert!(
                    !lut.is_mono(),
                    "3-component Array must not collapse to mono"
                );
                assert_eq!(lut.r, vec![0.0, 1.0]);
                assert_eq!(lut.g.as_ref().unwrap(), &vec![0.25, 0.75]);
                assert_eq!(lut.b.as_ref().unwrap(), &vec![0.5, 0.6]);
                // Per-channel values at the same grid entry must remain distinct, not all
                // equal to the (would-be) flattened r value.
                assert_ne!(lut.r[1], lut.g.as_ref().unwrap()[1]);
                assert_ne!(lut.g.as_ref().unwrap()[1], lut.b.as_ref().unwrap()[1]);
            }
            other => panic!("expected Lut1D node, got {other:?}"),
        }
    }

    #[test]
    fn clf_description_keeps_references_and_inner_spaces() {
        // quick-xml reports `&lt;` etc. as separate events; the reader used to keep only the last
        // text piece (`current_text = text`) with each piece trimmed, so this read as "more".
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="d" compCLFversion="3.0">
  <Description>  a &lt;grade&gt; &amp; more  </Description>
  <InputDescriptor>scene &#x26; linear</InputDescriptor>
  <Matrix inBitDepth="32f" outBitDepth="32f">
    <Array dim="3 3">1 0 0 0 1 0 0 0 1</Array>
  </Matrix>
</ProcessList>
"#;
        let pl = parse_clf(std::io::Cursor::new(xml)).unwrap();
        assert_eq!(pl.description.as_deref(), Some("a <grade> & more"));
        assert_eq!(pl.input_descriptor.as_deref(), Some("scene & linear"));
    }

    #[test]
    fn test_clf_lut3d_hostile_dim_rejected_not_oom() {
        // A hostile "dim" cannot be allowed to reach size*size*size*3 unchecked — that must
        // be caught (checked arithmetic) before any Vec is sized from it.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="hostile3d" compCLFversion="3.0">
  <LUT3D inBitDepth="32f" outBitDepth="32f">
    <Array dim="999999999 999999999 999999999 3">
    </Array>
  </LUT3D>
</ProcessList>
"#;
        let err = parse_clf(std::io::Cursor::new(xml)).unwrap_err();
        assert!(
            matches!(err, LutError::SizeOutOfRange { .. }),
            "expected SizeOutOfRange, got {err:?}"
        );
    }

    #[test]
    fn test_clf_lut3d_value_count_mismatch_rejected() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="short3d" compCLFversion="3.0">
  <LUT3D inBitDepth="32f" outBitDepth="32f">
    <Array dim="2 2 2 3">
      0.0 0.0 0.0
      1.0 0.0 0.0
    </Array>
  </LUT3D>
</ProcessList>
"#;
        let err = parse_clf(std::io::Cursor::new(xml)).unwrap_err();
        assert!(matches!(err, LutError::ParseError(_)), "got {err:?}");
    }

    #[test]
    fn test_clf_matrix_bitdepth_scaling() {
        // COLOR-02 for Matrix: the 3x3 block scales by inMax/outMax; an 8i->16i identity
        // matrix must come out as (255/65535) on the diagonal, not literal 1.0.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="mtx" compCLFversion="3.0">
  <Matrix inBitDepth="8i" outBitDepth="16i">
    <Array dim="3 3">
      1 0 0
      0 1 0
      0 0 1
    </Array>
  </Matrix>
</ProcessList>
"#;
        let clf = parse_clf(std::io::Cursor::new(xml)).unwrap();
        match &clf.nodes[0] {
            ProcessNode::Matrix { values, .. } => {
                let expected = 255.0f32 / 65535.0f32;
                assert!(
                    (values[0] - expected).abs() < 1e-6,
                    "values[0]={}",
                    values[0]
                );
                assert!(
                    (values[4] - expected).abs() < 1e-6,
                    "values[4]={}",
                    values[4]
                );
                assert!(
                    (values[8] - expected).abs() < 1e-6,
                    "values[8]={}",
                    values[8]
                );
            }
            other => panic!("expected Matrix node, got {other:?}"),
        }
    }

    #[test]
    fn test_clf_matrix_offset_scales_by_out_depth_only() {
        // The 3x4 offset column scales by 1/outMax alone (not inMax/outMax like the 3x3
        // block) — a different rule from the matrix coefficients (MatrixOpData.cpp:871-877).
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="mtx_off" compCLFversion="3.0">
  <Matrix inBitDepth="10i" outBitDepth="10i">
    <Array dim="3 4">
      1 0 0 100
      0 1 0 100
      0 0 1 100
    </Array>
  </Matrix>
</ProcessList>
"#;
        let clf = parse_clf(std::io::Cursor::new(xml)).unwrap();
        match &clf.nodes[0] {
            ProcessNode::Matrix { values, .. } => {
                assert!(
                    (values[0] - 1.0).abs() < 1e-4,
                    "diag values[0]={}",
                    values[0]
                );
                let expected_offset = 100.0f32 / 1023.0f32;
                assert!(
                    (values[3] - expected_offset).abs() < 1e-4,
                    "offset values[3]={}",
                    values[3]
                );
            }
            other => panic!("expected Matrix node, got {other:?}"),
        }
    }

    #[test]
    fn test_clf_invalid_bit_depth_is_typed_error() {
        // A bit-depth string the format doesn't define must not silently fall back to F32
        // (which would silently pick the wrong scale factor for COLOR-02).
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="badbd" compCLFversion="3.0">
  <LUT1D inBitDepth="99i" outBitDepth="32f">
    <Array dim="2 1">
      0.0 1.0
    </Array>
  </LUT1D>
</ProcessList>
"#;
        let err = parse_clf(std::io::Cursor::new(xml)).unwrap_err();
        assert!(
            matches!(err, LutError::InvalidBitDepth { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn test_clf_half_domain_is_typed_unsupported_not_silent() {
        // COLOR-05: halfDomain must not be silently dropped — this crate's Lut1D has no
        // half-float domain indexing, so it must fail loudly rather than parse into a LUT
        // that quietly ignores the attribute.
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="halfdom" compCLFversion="3.0">
  <LUT1D inBitDepth="32f" outBitDepth="32f" halfDomain="true">
    <Array dim="2 1">
      0.0 1.0
    </Array>
  </LUT1D>
</ProcessList>
"#;
        let err = parse_clf(std::io::Cursor::new(xml)).unwrap_err();
        assert!(
            matches!(
                err,
                LutError::Unsupported {
                    feature: "halfDomain",
                    ..
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn test_clf_lut1d_two_component_array_is_typed_error() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ProcessList id="twocomp" compCLFversion="3.0">
  <LUT1D inBitDepth="32f" outBitDepth="32f">
    <Array dim="2 2">
      0.0 0.0
      1.0 1.0
    </Array>
  </LUT1D>
</ProcessList>
"#;
        let err = parse_clf(std::io::Cursor::new(xml)).unwrap_err();
        assert!(
            matches!(
                err,
                LutError::Unsupported {
                    feature: "LUT1D Array components",
                    ..
                }
            ),
            "got {err:?}"
        );
    }

    /// The writer keeps what the reader reads back: all three channels of an RGB LUT1D (it
    /// wrote only red, as a mono column), the values at full `f32` precision and in the
    /// out bit depth's code range (it wrote `{:.6}` normalised values, which a 10i reader
    /// divides by 1023), the LUT3D interpolation, and a non-unit input domain as a leading
    /// `Range` (uniform) or `Matrix` (per channel), since a CLF LUT has no domain of its own.
    #[test]
    fn writer_round_trips_rgb_lut1d_depths_domain_and_interpolation() {
        let r = vec![0.0, 0.123_456_79, 1.0];
        let g = vec![0.5, 0.25, 0.0];
        let b = vec![1.0, 0.75, 0.3];
        let lut1 = Lut1D::from_rgb(r.clone(), g.clone(), b.clone(), -0.5, 2.0).unwrap();
        let data: Vec<[f32; 3]> = (0..8)
            .map(|i| [i as f32 / 7.0, 0.1 * i as f32, 1.0 / (1.0 + i as f32)])
            .collect();
        let lut3 = Lut3D::from_data(data.clone(), 2)
            .unwrap()
            .with_interpolation(Interpolation::Tetrahedral)
            .with_domain([0.0, -1.0, 0.0], [1.0, 3.0, 2.0]);
        let mut pl = ProcessList::new("w");
        pl.nodes.push(ProcessNode::Lut1D {
            lut: lut1,
            in_depth: BitDepth::F32,
            out_depth: BitDepth::F32,
        });
        pl.nodes.push(ProcessNode::Lut3D {
            lut: lut3,
            in_depth: BitDepth::F32,
            out_depth: BitDepth::F32,
        });
        let mono = Lut1D::from_data(vec![0.0, 512.0 / 1023.0, 1.0], 0.0, 1.0).unwrap();
        pl.nodes.push(ProcessNode::Lut1D {
            lut: mono,
            in_depth: BitDepth::U10,
            out_depth: BitDepth::U10,
        });
        let mut buf = Vec::new();
        write_clf_to(&mut buf, &pl).unwrap();
        let back = parse_clf(std::io::Cursor::new(buf)).unwrap();
        assert_eq!(back.nodes.len(), 5, "{:?}", back.nodes);
        match &back.nodes[0] {
            ProcessNode::Range(p) => {
                assert_eq!(
                    (p.min_in, p.max_in, p.min_out, p.max_out),
                    (Some(-0.5), Some(2.0), Some(0.0), Some(1.0))
                )
            }
            n => panic!("{n:?}"),
        }
        match &back.nodes[1] {
            ProcessNode::Lut1D { lut, .. } => {
                assert_eq!(
                    (&lut.r, lut.g.as_ref(), lut.b.as_ref()),
                    (&r, Some(&g), Some(&b))
                );
            }
            n => panic!("{n:?}"),
        }
        match &back.nodes[2] {
            ProcessNode::Matrix { values, .. } => {
                let want = [1.0, 0.0, 0.0, 0.0, 0.0, 0.25, 0.0, 0.25, 0.0, 0.0, 0.5, 0.0];
                assert!(
                    values.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-7),
                    "{values:?}"
                );
            }
            n => panic!("{n:?}"),
        }
        match &back.nodes[3] {
            ProcessNode::Lut3D { lut, .. } => {
                assert_eq!(lut.data, data);
                assert_eq!(lut.interpolation, Interpolation::Tetrahedral);
            }
            n => panic!("{n:?}"),
        }
        match &back.nodes[4] {
            ProcessNode::Lut1D { lut, .. } => {
                assert!(lut.g.is_none());
                assert!(
                    lut.r
                        .iter()
                        .zip([0.0, 512.0 / 1023.0, 1.0])
                        .all(|(a, b)| (a - b).abs() < 1e-7),
                    "{:?}",
                    lut.r
                );
            }
            n => panic!("{n:?}"),
        }
    }

    /// Matrix nodes of every CLF shape round-trip through the writer, bit depths included (the
    /// writer used to write 3 rows only and the normalised values under integer depths, which
    /// the reader then scaled a second time).
    #[test]
    fn writer_round_trips_matrix_shapes_and_depths() {
        let mut pl = ProcessList::new("m");
        for n in [9usize, 12, 16, 20] {
            let values: Vec<f32> = (0..n).map(|i| 0.1 * i as f32 - 0.3).collect();
            pl.nodes.push(ProcessNode::Matrix {
                values,
                in_depth: BitDepth::U10,
                out_depth: BitDepth::U12,
            });
        }
        let mut buf = Vec::new();
        write_clf_to(&mut buf, &pl).unwrap();
        let back = parse_clf(std::io::Cursor::new(buf)).unwrap();
        assert_eq!(back.nodes.len(), 4);
        for (a, b) in pl.nodes.iter().zip(&back.nodes) {
            match (a, b) {
                (
                    ProcessNode::Matrix { values: va, .. },
                    ProcessNode::Matrix { values: vb, .. },
                ) => {
                    assert_eq!(va.len(), vb.len());
                    assert!(
                        va.iter().zip(vb).all(|(x, y)| (x - y).abs() < 1e-6),
                        "{va:?} vs {vb:?}"
                    );
                }
                n => panic!("{n:?}"),
            }
        }
    }
}
