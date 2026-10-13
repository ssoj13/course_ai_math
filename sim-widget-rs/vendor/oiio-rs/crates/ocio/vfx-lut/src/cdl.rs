//! ASC CDL file format parser and writer.
//!
//! Supports three ASC CDL file formats:
//! - `.cc` - Single ColorCorrection
//! - `.ccc` - ColorCorrectionCollection (multiple CCs)
//! - `.cdl` - ColorDecisionList (CCs wrapped in ColorDecisions)
//!
//! # Example
//!
//! ```rust,no_run
//! use vfx_lut::cdl::{read_cc, read_ccc, ColorCorrection};
//! use std::path::Path;
//!
//! // Read a single CC
//! let cc = read_cc(Path::new("grade.cc")).unwrap();
//! println!("Slope: {:?}", cc.slope);
//!
//! // Read a CCC collection
//! let ccc = read_ccc(Path::new("grades.ccc")).unwrap();
//! for cc in &ccc.corrections {
//!     println!("ID: {:?}", cc.id);
//! }
//! ```
//!
//! The reader is one walk ([`parse_cc`], [`parse_ccc`] and [`parse_cdl`] are views of it) that
//! follows OpenColorIO's `CDLParser`: the root element decides the format, a correction id must
//! be unique, text and attributes keep their entity references. The writer emits everything the
//! reader reads, escaped, with shortest round-trip numbers.
//!
//! # References
//!
//! - ASC CDL v1.2 Specification (2009-05-04)
//! - OpenColorIO `fileformats/cdl/CDLParser.cpp`, `ops/cdl/CDLOpCPU.cpp`, `ops/cdl/CDLOpData.cpp`

use vfx_core::luminance_rec709;

use crate::{LutError, LutResult};
use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

// ============================================================================
// THE ASC CDL of the stack (ROADMAP G7)
// ============================================================================
//
// vfx-lut sits below vfx-color, vfx-ocio and vfx-compute, so the CDL lives here once: the four
// OCIO styles (`apply_cdl_*`), the parameter check (`validate_cdl_params`) and the `.cc`/`.ccc`/
// `.cdl` reader and writer. The vfx-ocio processor, vfx-color's `Cdl` and vfx-compute's CPU
// backend call these; the GPU emitters (WGSL, CUDA) mirror them as shader text. The math is
// bit-exact with OpenColorIO's CPU, which uses the SSE polynomial `pow` below
// (`tests/cdl_pyocio.rs` pins 48 cases against PyOpenColorIO 2.5.1); a plain `powf` is ~1e-5 off.

/// Fast log2 using OCIO's polynomial approximation (scalar version).
///
/// Matches OCIO's sseLog2 algorithm for bit-exact compatibility.
#[inline]
#[allow(clippy::excessive_precision)] // OCIO's polynomial literals, verbatim
pub fn fast_log2(x: f32) -> f32 {
    const EXP_MASK: i32 = 0x7F800000;
    const EXP_BIAS: i32 = 127;
    const EXP_SHIFT: i32 = 23;
    // Chebyshev polynomial coefficients for log2() over [1.0, 2.0), ~15 bits of mantissa precision.
    // OCIO's literals verbatim (SSE.h), so a diff against the reference stays trivial.
    const PNLOG5: f32 = 4.487361286440374006195e-2;
    const PNLOG4: f32 = -4.165637071209677112635e-1;
    const PNLOG3: f32 = 1.631148826119436277100;
    const PNLOG2: f32 = -3.550793018041176193407;
    const PNLOG1: f32 = 5.091710879305474367557;
    const PNLOG0: f32 = -2.800364054395965731506;

    if x <= 0.0 {
        return f32::NEG_INFINITY;
    }

    let bits = x.to_bits() as i32;

    // Extract mantissa and set exponent to 0 (value in [1, 2)).
    let mantissa_bits = (bits & !EXP_MASK) | (EXP_BIAS << EXP_SHIFT);
    let mantissa = f32::from_bits(mantissa_bits as u32);

    let log2_mantissa = PNLOG0
        + mantissa
            * (PNLOG1
                + mantissa
                    * (PNLOG2 + mantissa * (PNLOG3 + mantissa * (PNLOG4 + mantissa * PNLOG5))));

    let exponent = ((bits & EXP_MASK) >> EXP_SHIFT) - EXP_BIAS;

    log2_mantissa + exponent as f32
}

/// Fast exp2 using OCIO's polynomial approximation (scalar version).
///
/// Matches OCIO's sseExp2 algorithm for bit-exact compatibility.
#[inline]
#[allow(clippy::excessive_precision)] // OCIO's polynomial literals, verbatim
pub fn fast_exp2(x: f32) -> f32 {
    const EXP_BIAS: i32 = 127;
    const EXP_SHIFT: i32 = 23;
    // Chebyshev polynomial coefficients for exp2() over [0.0, 1.0).
    const PNEXP4: f32 = 1.353416792833547468620e-2;
    const PNEXP3: f32 = 5.201146058412685018921e-2;
    const PNEXP2: f32 = 2.414427569091865207710e-1;
    const PNEXP1: f32 = 6.930038344665415134202e-1;
    const PNEXP0: f32 = 1.000002593370603213644;

    if x < -126.0 {
        return 0.0;
    }
    if x >= 128.0 {
        return f32::INFINITY;
    }

    // Use proper floor to handle negative numbers correctly (`x as i32` truncates toward zero).
    let floor_x = x.floor() as i32;
    let fraction = x - floor_x as f32;

    let mexp = PNEXP0
        + fraction * (PNEXP1 + fraction * (PNEXP2 + fraction * (PNEXP3 + fraction * PNEXP4)));

    let zf_bits = ((floor_x + EXP_BIAS) << EXP_SHIFT) as u32;
    let zf = f32::from_bits(zf_bits);

    zf * mexp
}

/// Fast power function using OCIO's polynomial approximation: `pow(x, exp) = exp2(exp * log2(x))`.
///
/// Matches OCIO's ssePower algorithm for bit-exact CDL compatibility. Negative bases return 0
/// (OCIO behavior).
#[inline]
pub fn fast_pow(base: f32, exp: f32) -> f32 {
    if base <= 0.0 {
        return 0.0;
    }
    fast_exp2(exp * fast_log2(base))
}

/// OCIO `CDLOpData::validate`: the ASC CDL v1.2 (2009-05-04) parameter ranges, slope >= 0,
/// power > 0, saturation >= 0, offset unbounded. NaN fails, as OCIO's `!(value >= threshold)` does.
/// The messages are OCIO's. Every CDL that reaches pixels goes through this first (vfx-ocio
/// compiles `CdlTransform` and `.cc`/`.ccc`/`.cdl` files through it).
pub fn validate_cdl_params(slope: [f32; 3], power: [f32; 3], saturation: f32) -> LutResult<()> {
    let invalid = |prefix: &str, name: &str, v: f32| {
        Err(LutError::InvalidParameter(format!(
            "{prefix}: Invalid '{name}' {v} should be greater than 0."
        )))
    };
    for v in slope {
        if v.is_nan() || v < 0.0 {
            return invalid("CDL", "slope", v);
        }
    }
    for v in power {
        if v.is_nan() || v <= 0.0 {
            return invalid("CDLOpData", "power", v);
        }
    }
    if saturation.is_nan() || saturation < 0.0 {
        return invalid("CDL", "saturation", saturation);
    }
    Ok(())
}

/// OCIO `RcpMinValue` (`CDLOpCPU.cpp`): the floor under every reciprocal of a CDL parameter.
const RCP_MIN_VALUE: f32 = 1e-2;

/// OCIO `Reciprocal` (`CDLOpCPU.cpp`): `1 / max(x, 0.01)`. The reverse CDL takes the reciprocal
/// of slope, power and saturation this way, so a parameter of 0 (legal in a `.cdl` file) gives
/// 100, not infinity.
fn reciprocal(x: f32) -> f32 {
    1.0 / x.max(RCP_MIN_VALUE)
}

/// Which of OCIO's two CPU paths a CDL with this `power` takes.
///
/// `CDLOpCPU.cpp`: "if power is 1, the optimizer is able to convert the CDL op into a pair of
/// matrices and clamp", so with `power == (1, 1, 1)` no power is applied and saturation is an
/// exact identity at 1. Any other power goes through `CDLRendererFwd`/`Rev`, which raise EVERY
/// channel with the SSE power (`x^1` included, which is not exactly `x`) and apply saturation
/// unconditionally. PyOpenColorIO 2.5.1 shows both (`tests/cdl_pyocio.rs`).
fn renderer_path(power: [f32; 3]) -> bool {
    power != [1.0; 3]
}

/// OCIO `ApplySaturation`: Rec.709 luma, `luma + sat * (v - luma)`.
fn apply_saturation(rgb: &mut [f32; 3], saturation: f32) {
    let src = *rgb;
    let luma = luminance_rec709(src);
    for i in 0..3 {
        rgb[i] = luma + saturation * (src[i] - luma);
    }
}

/// OCIO `ApplyPower<false>`: NaN becomes 0, a negative value passes through unchanged.
fn power_no_clamp(v: f32, power: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else if v < 0.0 {
        v
    } else {
        fast_pow(v, power)
    }
}

fn clamp01(rgb: &mut [f32; 3]) {
    for v in rgb.iter_mut() {
        *v = v.clamp(0.0, 1.0);
    }
}

/// THE ASC CDL v1.2 forward WITH clamping, OCIO's `CDL_V1_2_FWD`, bit-exact with OpenColorIO's CPU.
///
/// Order: Slope -> Offset -> Clamp `[0,1]` -> Power -> Saturation -> Clamp `[0,1]`
/// (`CDLRendererFwd<true>`). Power and saturation follow `renderer_path`.
pub fn apply_cdl_clamped(
    rgb: &mut [f32; 3],
    slope: [f32; 3],
    offset: [f32; 3],
    power: [f32; 3],
    saturation: f32,
) {
    let renderer = renderer_path(power);
    for i in 0..3 {
        let v = (rgb[i] * slope[i] + offset[i]).clamp(0.0, 1.0);
        rgb[i] = if renderer { fast_pow(v, power[i]) } else { v };
    }
    if renderer || saturation != 1.0 {
        apply_saturation(rgb, saturation);
    }
    clamp01(rgb);
}

/// THE ASC CDL forward WITHOUT clamping, OCIO's `CDL_NO_CLAMP_FWD`, bit-exact with OpenColorIO's
/// CPU.
///
/// For scene-referred values outside `[0, 1]`. The power step is `ApplyPower<false>`: a negative
/// post-slope/offset value passes through unchanged and NaN becomes 0 (only on the renderer path;
/// see `renderer_path`).
pub fn apply_cdl_unclamped(
    rgb: &mut [f32; 3],
    slope: [f32; 3],
    offset: [f32; 3],
    power: [f32; 3],
    saturation: f32,
) {
    let renderer = renderer_path(power);
    for i in 0..3 {
        let v = rgb[i] * slope[i] + offset[i];
        rgb[i] = if renderer {
            power_no_clamp(v, power[i])
        } else {
            v
        };
    }
    if renderer || saturation != 1.0 {
        apply_saturation(rgb, saturation);
    }
}

/// THE ASC CDL v1.2 inverse WITH clamping, OCIO's `CDL_V1_2_REV`.
///
/// `slope`/`offset`/`power`/`saturation` are the FORWARD parameters. The reverse pipeline is
/// `CDLRendererRev<true>`: Clamp -> Saturation(1/sat) -> Power(1/power, clamped to `[0,1]` first
/// by `ApplyPower<true>`) -> Offset(-offset) -> Slope(1/slope) -> Clamp, with every reciprocal
/// floored by `reciprocal`. It is not the forward formula fed inverted parameters: that moves
/// the clamp relative to power and saturation (COLOR-13).
pub fn apply_cdl_inverse_clamped(
    rgb: &mut [f32; 3],
    slope: [f32; 3],
    offset: [f32; 3],
    power: [f32; 3],
    saturation: f32,
) {
    let renderer = renderer_path(power);
    clamp01(rgb);
    if renderer || saturation != 1.0 {
        apply_saturation(rgb, reciprocal(saturation));
    }
    clamp01(rgb);
    if renderer {
        for i in 0..3 {
            rgb[i] = fast_pow(rgb[i], reciprocal(power[i]));
        }
    }
    for i in 0..3 {
        rgb[i] = (rgb[i] + -offset[i]) * reciprocal(slope[i]);
    }
    clamp01(rgb);
}

/// THE ASC CDL inverse WITHOUT clamping, OCIO's `CDL_NO_CLAMP_REV`.
///
/// `CDLRendererRev<false>`: Saturation(1/sat) -> Power(1/power, `ApplyPower<false>`) ->
/// Offset(-offset) -> Slope(1/slope), no clamps, reciprocals as in [`apply_cdl_inverse_clamped`].
pub fn apply_cdl_inverse_unclamped(
    rgb: &mut [f32; 3],
    slope: [f32; 3],
    offset: [f32; 3],
    power: [f32; 3],
    saturation: f32,
) {
    let renderer = renderer_path(power);
    if renderer || saturation != 1.0 {
        apply_saturation(rgb, reciprocal(saturation));
    }
    if renderer {
        for i in 0..3 {
            rgb[i] = power_no_clamp(rgb[i], reciprocal(power[i]));
        }
    }
    for i in 0..3 {
        rgb[i] = (rgb[i] + -offset[i]) * reciprocal(slope[i]);
    }
}

// ============================================================================
// Data structures
// ============================================================================

/// A single ASC Color Correction (CC).
#[derive(Debug, Clone, PartialEq)]
pub struct ColorCorrection {
    /// Unique identifier for this correction.
    pub id: Option<String>,
    /// Description text (can have multiple).
    pub descriptions: Vec<String>,
    /// Input description.
    pub input_description: Option<String>,
    /// Viewing description.
    pub viewing_description: Option<String>,
    /// Slope (multiply) per channel [R, G, B].
    pub slope: [f32; 3],
    /// Offset (add) per channel [R, G, B].
    pub offset: [f32; 3],
    /// Power (gamma) per channel [R, G, B].
    pub power: [f32; 3],
    /// Saturation adjustment (1.0 = no change).
    pub saturation: f32,
}

impl Default for ColorCorrection {
    fn default() -> Self {
        Self {
            id: None,
            descriptions: Vec::new(),
            input_description: None,
            viewing_description: None,
            slope: [1.0, 1.0, 1.0],
            offset: [0.0, 0.0, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
        }
    }
}

impl ColorCorrection {
    /// Creates a new ColorCorrection with given SOP values.
    pub fn new(slope: [f32; 3], offset: [f32; 3], power: [f32; 3]) -> Self {
        Self {
            slope,
            offset,
            power,
            ..Default::default()
        }
    }

    /// Creates a new ColorCorrection with id and SOP values.
    pub fn with_id(id: &str, slope: [f32; 3], offset: [f32; 3], power: [f32; 3]) -> Self {
        Self {
            id: Some(id.to_string()),
            slope,
            offset,
            power,
            ..Default::default()
        }
    }

    /// Applies CDL to RGB values in-place (OCIO's `CDL_NO_CLAMP_FWD` - no `[0,1]` bound; a
    /// negative post-slope/offset value passes through unchanged rather than being raised to a
    /// power). Use [`Self::apply_clamped`] for the fully-clamped ASC CDL v1.2 formula.
    pub fn apply(&self, rgb: &mut [f32; 3]) {
        apply_cdl_unclamped(rgb, self.slope, self.offset, self.power, self.saturation);
    }

    /// Applies the full ASC CDL v1.2 formula, clamping to `[0, 1]` before `power` and again at
    /// the end (OCIO's `CDL_V1_2_FWD`) - the ASC CDL spec's documented order.
    pub fn apply_clamped(&self, rgb: &mut [f32; 3]) {
        apply_cdl_clamped(rgb, self.slope, self.offset, self.power, self.saturation);
    }
}

/// A ColorDecision containing a ColorCorrection and optional media reference.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ColorDecision {
    /// Description text.
    pub descriptions: Vec<String>,
    /// Input description.
    pub input_description: Option<String>,
    /// Viewing description.
    pub viewing_description: Option<String>,
    /// Media reference path.
    pub media_ref: Option<String>,
    /// The color correction.
    pub correction: ColorCorrection,
}

/// A ColorCorrectionCollection (CCC file).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ColorCorrectionCollection {
    /// Description text.
    pub descriptions: Vec<String>,
    /// Input description.
    pub input_description: Option<String>,
    /// Viewing description.
    pub viewing_description: Option<String>,
    /// List of color corrections.
    pub corrections: Vec<ColorCorrection>,
}

impl ColorCorrectionCollection {
    /// Finds a ColorCorrection by id.
    pub fn find(&self, id: &str) -> Option<&ColorCorrection> {
        self.corrections
            .iter()
            .find(|cc| cc.id.as_deref() == Some(id))
    }

    /// Gets the first ColorCorrection (if any).
    pub fn first(&self) -> Option<&ColorCorrection> {
        self.corrections.first()
    }
}

/// A ColorDecisionList (CDL file).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ColorDecisionList {
    /// Description text.
    pub descriptions: Vec<String>,
    /// Input description.
    pub input_description: Option<String>,
    /// Viewing description.
    pub viewing_description: Option<String>,
    /// List of color decisions.
    pub decisions: Vec<ColorDecision>,
}

impl ColorDecisionList {
    /// Finds a ColorCorrection by id.
    pub fn find(&self, id: &str) -> Option<&ColorCorrection> {
        self.decisions
            .iter()
            .find(|cd| cd.correction.id.as_deref() == Some(id))
            .map(|cd| &cd.correction)
    }

    /// Converts to a ColorCorrectionCollection.
    pub fn to_collection(&self) -> ColorCorrectionCollection {
        ColorCorrectionCollection {
            descriptions: self.descriptions.clone(),
            input_description: self.input_description.clone(),
            viewing_description: self.viewing_description.clone(),
            corrections: self
                .decisions
                .iter()
                .map(|cd| cd.correction.clone())
                .collect(),
        }
    }
}

// ============================================================================
// Parsing helpers
// ============================================================================

fn parse_rgb(s: &str) -> LutResult<[f32; 3]> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(LutError::ParseError(format!(
            "Expected 3 values, got {}",
            parts.len()
        )));
    }
    Ok([
        parts[0]
            .parse()
            .map_err(|e| LutError::ParseError(format!("Invalid R: {}", e)))?,
        parts[1]
            .parse()
            .map_err(|e| LutError::ParseError(format!("Invalid G: {}", e)))?,
        parts[2]
            .parse()
            .map_err(|e| LutError::ParseError(format!("Invalid B: {}", e)))?,
    ])
}

fn get_attr(e: &quick_xml::events::BytesStart, key: &str) -> LutResult<Option<String>> {
    // Unescaped and whitespace-normalized as XML 1.0 specifies for attribute values (what expat,
    // OCIO's parser, returns): `id="a&amp;b"` is `a&b`.
    for attr in e.attributes() {
        let attr = attr.map_err(|e| LutError::ParseError(format!("XML attribute: {e}")))?;
        if attr.key.as_ref() == key {
            let v = attr
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map_err(|e| LutError::ParseError(format!("XML attribute: {e}")))?;
            return Ok((!v.is_empty()).then(|| v.into_owned()));
        }
    }
    Ok(None)
}

// ============================================================================
// Parsing: one walk for .cc, .ccc and .cdl
// ============================================================================

/// What one walk over a `.cc`, `.ccc` or `.cdl` document collects. The three readers used to be
/// three copies of the same event loop; they are now views of this.
#[derive(Default)]
struct CdlDocument {
    /// The root element's name, which says which of the three formats this is (OCIO
    /// `CDLParser::Impl::initializeHandlers` picks the handler by it).
    root: String,
    descriptions: Vec<String>,
    input_description: Option<String>,
    viewing_description: Option<String>,
    /// Every correction in document order; a `.ccc`/`.cc` correction gets a default decision.
    decisions: Vec<ColorDecision>,
}

/// Walks a CDL-family XML document (port of OCIO's `CDLParser`, `fileformats/cdl/`). Text is
/// collected with [`crate::xml_text`], so entity and character references survive. A
/// correction `id` must be unique in the document, as OCIO's `getCDLTransforms` requires.
fn parse_document<R: BufRead>(reader: R) -> LutResult<CdlDocument> {
    let mut xml = Reader::from_reader(reader);
    let mut buf = Vec::new();
    let mut doc = CdlDocument::default();
    let mut stack: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut decision: Option<ColorDecision> = None;
    let mut cc: Option<ColorCorrection> = None;

    loop {
        let event = xml
            .read_event_into(&mut buf)
            .map_err(|e| LutError::ParseError(format!("XML error: {e}")))?;
        let (start, end) = match &event {
            Event::Start(e) => (Some(e.clone()), false),
            Event::Empty(e) => (Some(e.clone()), true),
            Event::End(_) => (None, true),
            Event::Text(t) => {
                crate::xml_text::push_text(&mut text, t);
                (None, false)
            }
            Event::GeneralRef(r) => {
                crate::xml_text::push_ref(&mut text, r)?;
                (None, false)
            }
            Event::CData(c) => {
                crate::xml_text::push_cdata(&mut text, c);
                (None, false)
            }
            Event::Eof => break,
            _ => (None, false),
        };
        if let Some(e) = start {
            let name = e.name().as_ref().to_owned();
            if doc.root.is_empty() {
                doc.root.clone_from(&name);
            }
            match name.as_str() {
                "ColorDecision" => decision = Some(ColorDecision::default()),
                "ColorCorrection" => {
                    cc = Some(ColorCorrection {
                        id: get_attr(&e, "id")?,
                        ..Default::default()
                    })
                }
                "MediaRef" => {
                    if let Some(d) = decision.as_mut() {
                        d.media_ref = get_attr(&e, "ref")?;
                    }
                }
                _ => {}
            }
            stack.push(name);
            text.clear();
        }
        if !end {
            buf.clear();
            continue;
        }
        let name = stack.pop().unwrap_or_default();
        let parent = stack.last().map(String::as_str);
        let value = text.trim();
        match name.as_str() {
            "Slope" | "Offset" | "Power" | "Saturation" => {
                let Some(c) = cc.as_mut() else {
                    return Err(LutError::ParseError(format!(
                        "<{name}> outside <ColorCorrection>"
                    )));
                };
                match name.as_str() {
                    "Slope" => c.slope = parse_rgb(value)?,
                    "Offset" => c.offset = parse_rgb(value)?,
                    "Power" => c.power = parse_rgb(value)?,
                    _ => {
                        c.saturation = value.parse().map_err(|e| {
                            LutError::ParseError(format!("Invalid saturation {value:?}: {e}"))
                        })?
                    }
                }
            }
            "Description" | "InputDescription" | "ViewingDescription" => {
                // SOPNode / SatNode descriptions are not kept (nor are they by OCIO's
                // CDLTransform model).
                let target = match parent {
                    Some("ColorCorrection") => cc.as_mut().map(|c| {
                        (
                            &mut c.descriptions,
                            &mut c.input_description,
                            &mut c.viewing_description,
                        )
                    }),
                    Some("ColorDecision") => decision.as_mut().map(|d| {
                        (
                            &mut d.descriptions,
                            &mut d.input_description,
                            &mut d.viewing_description,
                        )
                    }),
                    Some("ColorCorrectionCollection" | "ColorDecisionList") => Some((
                        &mut doc.descriptions,
                        &mut doc.input_description,
                        &mut doc.viewing_description,
                    )),
                    _ => None,
                };
                if let Some((descriptions, input, viewing)) = target {
                    match name.as_str() {
                        "Description" => descriptions.push(value.to_string()),
                        "InputDescription" => *input = Some(value.to_string()),
                        _ => *viewing = Some(value.to_string()),
                    }
                }
            }
            "ColorCorrection" => {
                let c = cc.take().unwrap_or_default();
                if let Some(id) = &c.id
                    && doc
                        .decisions
                        .iter()
                        .any(|d| d.correction.id.as_ref() == Some(id))
                {
                    return Err(LutError::ParseError(format!(
                        "Duplicate elements with '{id}' found. If id is specified, it must be unique."
                    )));
                }
                match decision.as_mut() {
                    Some(d) => d.correction = c,
                    None => doc.decisions.push(ColorDecision {
                        correction: c,
                        ..Default::default()
                    }),
                }
            }
            "ColorDecision" => {
                if let Some(d) = decision.take() {
                    doc.decisions.push(d);
                }
            }
            _ => {}
        }
        text.clear();
        buf.clear();
    }
    if let Some(open) = stack.last() {
        return Err(LutError::ParseError(format!("no closing tag for '{open}'")));
    }
    Ok(doc)
}

/// Reads a CC file (single ColorCorrection).
pub fn read_cc(path: &Path) -> LutResult<ColorCorrection> {
    let file = File::open(path)?;
    parse_cc(BufReader::new(file))
}

/// Parses a `.cc`: the root must be `<ColorCorrection>` (OCIO: "No transform found." otherwise).
pub fn parse_cc<R: BufRead>(reader: R) -> LutResult<ColorCorrection> {
    let doc = parse_document(reader)?;
    if doc.root != "ColorCorrection" {
        return Err(LutError::ParseError(format!(
            "not a .cc file: root element is <{}>, expected <ColorCorrection>",
            doc.root
        )));
    }
    doc.decisions
        .into_iter()
        .next()
        .map(|d| d.correction)
        .ok_or_else(|| LutError::ParseError("No transform found.".into()))
}

/// Reads a CCC file (ColorCorrectionCollection).
pub fn read_ccc(path: &Path) -> LutResult<ColorCorrectionCollection> {
    let file = File::open(path)?;
    parse_ccc(BufReader::new(file))
}

/// Parses a `.ccc`: the root must be `<ColorCorrectionCollection>`.
pub fn parse_ccc<R: BufRead>(reader: R) -> LutResult<ColorCorrectionCollection> {
    let doc = parse_document(reader)?;
    if doc.root != "ColorCorrectionCollection" {
        return Err(LutError::ParseError(format!(
            "not a .ccc file: root element is <{}>, expected <ColorCorrectionCollection>",
            doc.root
        )));
    }
    Ok(ColorCorrectionCollection {
        descriptions: doc.descriptions,
        input_description: doc.input_description,
        viewing_description: doc.viewing_description,
        corrections: doc.decisions.into_iter().map(|d| d.correction).collect(),
    })
}

/// Reads a CDL file (ColorDecisionList).
pub fn read_cdl(path: &Path) -> LutResult<ColorDecisionList> {
    let file = File::open(path)?;
    parse_cdl(BufReader::new(file))
}

/// Parses a `.cdl`: the root must be `<ColorDecisionList>`.
pub fn parse_cdl<R: BufRead>(reader: R) -> LutResult<ColorDecisionList> {
    let doc = parse_document(reader)?;
    if doc.root != "ColorDecisionList" {
        return Err(LutError::ParseError(format!(
            "not a .cdl file: root element is <{}>, expected <ColorDecisionList>",
            doc.root
        )));
    }
    Ok(ColorDecisionList {
        descriptions: doc.descriptions,
        input_description: doc.input_description,
        viewing_description: doc.viewing_description,
        decisions: doc.decisions,
    })
}

/// Auto-detect format and read any CDL file (.cc, .ccc, .cdl).
pub fn read_any(path: &Path) -> LutResult<ColorCorrectionCollection> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "cc" => {
            let cc = read_cc(path)?;
            Ok(ColorCorrectionCollection {
                corrections: vec![cc],
                ..Default::default()
            })
        }
        "ccc" => read_ccc(path),
        "cdl" => Ok(read_cdl(path)?.to_collection()),
        _ => Err(LutError::ParseError(format!(
            "Unknown CDL extension: {}",
            ext
        ))),
    }
}

// ============================================================================
// Writing
// ============================================================================

/// Writes a ColorCorrection to a CC file.
pub fn write_cc(path: &Path, cc: &ColorCorrection) -> LutResult<()> {
    let mut file = File::create(path)?;
    write_cc_to(&mut file, cc)
}

/// Writes a ColorCorrection to a writer.
pub fn write_cc_to<W: Write>(w: &mut W, cc: &ColorCorrection) -> LutResult<()> {
    write_correction(w, cc, "")
}

/// Writes a ColorCorrectionCollection to a CCC file.
pub fn write_ccc(path: &Path, ccc: &ColorCorrectionCollection) -> LutResult<()> {
    let mut file = File::create(path)?;
    write_ccc_to(&mut file, ccc)
}

/// Writes a ColorCorrectionCollection to a writer.
pub fn write_ccc_to<W: Write>(w: &mut W, ccc: &ColorCorrectionCollection) -> LutResult<()> {
    writeln!(
        w,
        r#"<ColorCorrectionCollection xmlns="urn:ASC:CDL:v1.01">"#
    )?;
    write_descriptions(
        w,
        "  ",
        &ccc.descriptions,
        ccc.input_description.as_deref(),
        ccc.viewing_description.as_deref(),
    )?;
    for cc in &ccc.corrections {
        writeln!(w)?;
        write_correction(w, cc, "  ")?;
    }
    writeln!(w)?;
    writeln!(w, "</ColorCorrectionCollection>")?;
    Ok(())
}

/// THE `<ColorCorrection>` writer (`write_cc_to` and `write_ccc_to` both use it): everything
/// [`parse_cc`] reads, with text and the id XML-escaped. Values use `{}`, Rust's shortest
/// round-trip `f32` form, so a file written here reads back bit-identical.
fn write_correction<W: Write>(w: &mut W, cc: &ColorCorrection, ind: &str) -> LutResult<()> {
    let id = escape(cc.id.as_deref().unwrap_or(""));
    writeln!(w, r#"{ind}<ColorCorrection id="{id}">"#)?;
    let inner = format!("{ind}  ");
    write_descriptions(
        w,
        &inner,
        &cc.descriptions,
        cc.input_description.as_deref(),
        cc.viewing_description.as_deref(),
    )?;
    let [s, o, p] = [cc.slope, cc.offset, cc.power];
    writeln!(w, "{inner}<SOPNode>")?;
    writeln!(w, "{inner}  <Slope>{} {} {}</Slope>", s[0], s[1], s[2])?;
    writeln!(w, "{inner}  <Offset>{} {} {}</Offset>", o[0], o[1], o[2])?;
    writeln!(w, "{inner}  <Power>{} {} {}</Power>", p[0], p[1], p[2])?;
    writeln!(w, "{inner}</SOPNode>")?;
    writeln!(w, "{inner}<SatNode>")?;
    writeln!(w, "{inner}  <Saturation>{}</Saturation>", cc.saturation)?;
    writeln!(w, "{inner}</SatNode>")?;
    writeln!(w, "{ind}</ColorCorrection>")?;
    Ok(())
}

/// The `<Description>`, `<InputDescription>` and `<ViewingDescription>` elements, escaped.
fn write_descriptions<W: Write>(
    w: &mut W,
    ind: &str,
    descriptions: &[String],
    input: Option<&str>,
    viewing: Option<&str>,
) -> LutResult<()> {
    for d in descriptions {
        writeln!(w, "{ind}<Description>{}</Description>", escape(d))?;
    }
    if let Some(d) = input {
        writeln!(w, "{ind}<InputDescription>{}</InputDescription>", escape(d))?;
    }
    if let Some(d) = viewing {
        writeln!(
            w,
            "{ind}<ViewingDescription>{}</ViewingDescription>",
            escape(d)
        )?;
    }
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const CC_SAMPLE: &str = r#"<ColorCorrection id="foo">
  <SOPNode>
    <Description>this is a description</Description>
    <Slope>1.1 1.2 1.3</Slope>
    <Offset>2.1 2.2 2.3</Offset>
    <Power>3.1 3.2 3.3</Power>
  </SOPNode>
  <SatNode>
    <Saturation>0.7</Saturation>
  </SatNode>
</ColorCorrection>"#;

    const CCC_SAMPLE: &str = r#"<ColorCorrectionCollection xmlns="urn:ASC:CDL:v1.01">
  <ColorCorrection id="cc0001">
    <SOPNode>
      <Slope>1.0 1.0 0.9</Slope>
      <Offset>-0.03 -0.02 0</Offset>
      <Power>1.25 1 1</Power>
    </SOPNode>
    <SatNode>
      <Saturation>1.7</Saturation>
    </SatNode>
  </ColorCorrection>
  <ColorCorrection id="cc0002">
    <SOPNode>
      <Slope>0.9 0.7 0.6</Slope>
      <Offset>0.1 0.1 0.1</Offset>
      <Power>0.9 0.9 0.9</Power>
    </SOPNode>
    <SatNode>
      <Saturation>0.7</Saturation>
    </SatNode>
  </ColorCorrection>
</ColorCorrectionCollection>"#;

    const CDL_SAMPLE: &str = r#"<ColorDecisionList xmlns="urn:ASC:CDL:v1.01">
  <ColorDecision>
    <MediaRef ref="some/path.dpx"/>
    <ColorCorrection id="shot001">
      <SOPNode>
        <Slope>1.1 1.0 0.9</Slope>
        <Offset>0.0 0.01 0.0</Offset>
        <Power>1.0 1.0 1.0</Power>
      </SOPNode>
      <SatNode>
        <Saturation>1.2</Saturation>
      </SatNode>
    </ColorCorrection>
  </ColorDecision>
</ColorDecisionList>"#;

    #[test]
    fn parse_cc_sample() {
        let cc = parse_cc(Cursor::new(CC_SAMPLE)).unwrap();
        assert_eq!(cc.id, Some("foo".to_string()));
        assert_eq!(cc.slope, [1.1, 1.2, 1.3]);
        assert_eq!(cc.offset, [2.1, 2.2, 2.3]);
        assert_eq!(cc.power, [3.1, 3.2, 3.3]);
        assert!((cc.saturation - 0.7).abs() < 1e-6);
    }

    #[test]
    fn parse_ccc_sample() {
        let ccc = parse_ccc(Cursor::new(CCC_SAMPLE)).unwrap();
        assert_eq!(ccc.corrections.len(), 2);

        let cc1 = &ccc.corrections[0];
        assert_eq!(cc1.id, Some("cc0001".to_string()));
        assert_eq!(cc1.slope, [1.0, 1.0, 0.9]);
        assert!((cc1.saturation - 1.7).abs() < 1e-6);

        let cc2 = &ccc.corrections[1];
        assert_eq!(cc2.id, Some("cc0002".to_string()));
        assert_eq!(cc2.slope, [0.9, 0.7, 0.6]);
    }

    #[test]
    fn parse_cdl_sample() {
        let cdl = parse_cdl(Cursor::new(CDL_SAMPLE)).unwrap();
        assert_eq!(cdl.decisions.len(), 1);

        let decision = &cdl.decisions[0];
        assert_eq!(decision.media_ref, Some("some/path.dpx".to_string()));

        let cc = &decision.correction;
        assert_eq!(cc.id, Some("shot001".to_string()));
        assert_eq!(cc.slope, [1.1, 1.0, 0.9]);
        assert!((cc.saturation - 1.2).abs() < 1e-6);
    }

    #[test]
    fn cc_apply() {
        let cc = ColorCorrection {
            slope: [1.1, 1.0, 0.9],
            offset: [0.0, 0.01, 0.0],
            power: [1.0, 1.0, 1.0],
            saturation: 1.0,
            ..Default::default()
        };
        let mut rgb = [0.5, 0.5, 0.5];
        cc.apply(&mut rgb);
        assert!((rgb[0] - 0.55).abs() < 1e-6);
        assert!((rgb[1] - 0.51).abs() < 1e-6);
        assert!((rgb[2] - 0.45).abs() < 1e-6);
    }

    #[test]
    fn cc_apply_with_power() {
        let cc = ColorCorrection {
            power: [2.0, 2.0, 2.0],
            ..Default::default()
        };
        let mut rgb = [0.5, 0.5, 0.5];
        cc.apply(&mut rgb);
        // `apply` now routes through `fast_pow` (a56faa7), OCIO's own polynomial
        // approximation, not exact `powf` - matches vfx-color's own test tolerance for it
        // (sse_math.rs: relative error < 0.001), not bit-exact.
        assert!((rgb[0] - 0.25).abs() < 1e-3);
    }

    /// `apply_clamped` must clamp to `[0,1]` BEFORE `power`, per the ASC CDL spec -
    /// this crate's own `ColorCorrection`/`clf::CdlParams` used to skip that clamp, so an
    /// out-of-range intermediate (slope=2.0 on input 1.0 -> v=2.0) was raised to the power
    /// directly (`2.0^2 = 4.0`) instead of clamping first (`1.0^2 = 1.0`).
    #[test]
    fn cc_apply_clamped_clamps_before_power() {
        let cc = ColorCorrection {
            slope: [2.0, 2.0, 2.0],
            power: [2.0, 2.0, 2.0],
            ..Default::default()
        };
        let mut rgb = [1.0, 1.0, 1.0];
        cc.apply_clamped(&mut rgb);
        assert!(
            (rgb[0] - 1.0).abs() < 1e-3,
            "expected clamp-before-power to give 1.0, got {}",
            rgb[0]
        );
    }

    #[test]
    fn cc_apply_with_saturation() {
        let cc = ColorCorrection {
            saturation: 0.0,
            ..Default::default()
        };
        let mut rgb = [1.0, 0.5, 0.0];
        cc.apply(&mut rgb);
        let luma = luminance_rec709([1.0, 0.5, 0.0]);
        assert!((rgb[0] - luma).abs() < 1e-6);
        assert!((rgb[1] - luma).abs() < 1e-6);
        assert!((rgb[2] - luma).abs() < 1e-6);
    }

    #[test]
    fn cc_roundtrip() {
        let original = ColorCorrection {
            id: Some("test".to_string()),
            slope: [1.1, 1.2, 1.3],
            offset: [0.01, 0.02, 0.03],
            power: [0.9, 1.0, 1.1],
            saturation: 0.8,
            ..Default::default()
        };
        let mut buf = Vec::new();
        write_cc_to(&mut buf, &original).unwrap();
        let parsed = parse_cc(Cursor::new(buf)).unwrap();
        assert_eq!(parsed.id, original.id);
        assert_eq!(parsed.slope, original.slope);
        assert_eq!(parsed.offset, original.offset);
        assert_eq!(parsed.power, original.power);
        assert!((parsed.saturation - original.saturation).abs() < 1e-6);
    }

    #[test]
    fn ccc_find() {
        let ccc = parse_ccc(Cursor::new(CCC_SAMPLE)).unwrap();
        let cc1 = ccc.find("cc0001").unwrap();
        assert_eq!(cc1.slope, [1.0, 1.0, 0.9]);
        let cc2 = ccc.find("cc0002").unwrap();
        assert_eq!(cc2.slope, [0.9, 0.7, 0.6]);
        assert!(ccc.find("nonexistent").is_none());
    }

    #[test]
    fn cdl_to_collection() {
        let cdl = parse_cdl(Cursor::new(CDL_SAMPLE)).unwrap();
        let ccc = cdl.to_collection();
        assert_eq!(ccc.corrections.len(), 1);
        assert_eq!(ccc.corrections[0].id, Some("shot001".to_string()));
    }

    #[test]
    fn cc_identity() {
        let cc = ColorCorrection::default();
        let mut rgb = [0.5, 0.3, 0.7];
        let original = rgb;
        cc.apply(&mut rgb);
        assert!((rgb[0] - original[0]).abs() < 1e-6);
        assert!((rgb[1] - original[1]).abs() < 1e-6);
        assert!((rgb[2] - original[2]).abs() < 1e-6);
    }

    #[test]
    fn parse_negative_offset() {
        let xml = r#"<ColorCorrection id="test">
  <SOPNode>
    <Slope>1.0 1.0 0.9</Slope>
    <Offset>-.03 -2e-2 0</Offset>
    <Power>1.25 1 1e0</Power>
  </SOPNode>
  <SatNode>
    <Saturation>1.0</Saturation>
  </SatNode>
</ColorCorrection>"#;
        let cc = parse_cc(Cursor::new(xml)).unwrap();
        assert!((cc.offset[0] - (-0.03)).abs() < 1e-6);
        assert!((cc.offset[1] - (-0.02)).abs() < 1e-6);
        assert!((cc.offset[2] - 0.0).abs() < 1e-6);
        assert!((cc.power[0] - 1.25).abs() < 1e-6);
    }

    /// Entity and character references in text and attributes survive (quick-xml reports them
    /// as separate events; the readers used to drop them and trim the pieces around them).
    #[test]
    fn references_in_text_and_attributes_are_kept() {
        let xml = r#"<ColorCorrection id="a&amp;b"><Description>x &lt;y&gt; &#x41; z</Description>
            <SOPNode><Slope>1 1 1</Slope><Offset>0 0 0</Offset><Power>1 1 1</Power></SOPNode>
            </ColorCorrection>"#;
        let cc = parse_cc(xml.as_bytes()).unwrap();
        assert_eq!(cc.id.as_deref(), Some("a&b"));
        assert_eq!(cc.descriptions, ["x <y> A z"]);
    }

    /// OCIO `CDLParser`: the root element says which format this is; a correction id must be
    /// unique; a `.cc` with no correction has "No transform found.".
    #[test]
    fn root_element_and_unique_ids_follow_ocio() {
        let cc = r#"<ColorCorrection id="x"></ColorCorrection>"#;
        let ccc =
            r#"<ColorCorrectionCollection><ColorCorrection id="x"/></ColorCorrectionCollection>"#;
        assert!(parse_ccc(cc.as_bytes()).is_err());
        assert!(parse_cdl(ccc.as_bytes()).is_err());
        assert!(parse_cc(ccc.as_bytes()).is_err());
        let dup = r#"<ColorCorrectionCollection><ColorCorrection id="x"/><ColorCorrection id="x"/></ColorCorrectionCollection>"#;
        assert!(parse_ccc(dup.as_bytes()).is_err());
        assert!(parse_ccc("<ColorCorrectionCollection><ColorCorrection>".as_bytes()).is_err());
    }

    /// A `.ccc` written by `write_ccc_to` reads back identical, descriptions and awkward text
    /// included; a `.cdl`'s decisions keep their media reference and descriptions.
    #[test]
    fn ccc_round_trip_and_cdl_decisions() {
        let ccc = ColorCorrectionCollection {
            descriptions: vec!["collection & co".into()],
            input_description: Some("in <put>".into()),
            corrections: vec![
                ColorCorrection {
                    id: Some("one".into()),
                    descriptions: vec!["first".into(), "second \"q\"".into()],
                    viewing_description: Some("view".into()),
                    slope: [1.1, 0.9, 1.0],
                    offset: [0.01, -0.02, 0.0],
                    power: [1.0, 1.2, 0.8],
                    saturation: 0.7,
                    ..Default::default()
                },
                ColorCorrection::with_id("two", [2.0; 3], [0.0; 3], [1.0; 3]),
            ],
            ..Default::default()
        };
        let mut buf = Vec::new();
        write_ccc_to(&mut buf, &ccc).unwrap();
        assert_eq!(parse_ccc(buf.as_slice()).unwrap(), ccc);

        let cdl = r#"<ColorDecisionList><Description>list</Description>
            <ColorDecision><MediaRef ref="shot&amp;1.exr"/><Description>d</Description>
            <ColorCorrection id="c"><SOPNode><Slope>2 2 2</Slope><Offset>0 0 0</Offset>
            <Power>1 1 1</Power></SOPNode></ColorCorrection></ColorDecision></ColorDecisionList>"#;
        let list = parse_cdl(cdl.as_bytes()).unwrap();
        assert_eq!(list.descriptions, ["list"]);
        assert_eq!(list.decisions[0].media_ref.as_deref(), Some("shot&1.exr"));
        assert_eq!(list.decisions[0].descriptions, ["d"]);
        assert_eq!(list.decisions[0].correction.slope, [2.0; 3]);
    }
}
