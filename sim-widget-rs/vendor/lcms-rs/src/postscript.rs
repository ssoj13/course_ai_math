//! PostScript Level 2 colour resource generation — a pure-Rust port of `cmsps2.c`.
//!
//! Generates the two PostScript colour resources lcms2 emits from an ICC profile:
//!
//! * **CSA** (Color Space Array) — the device→PCS transform, used as a PostScript colour
//!   space. Depending on the profile it is emitted as `/CIEBasedA` (monochrome),
//!   `/CIEBasedABC` (matrix-shaper) or `/CIEBasedDEF[G]` (CLUT-based, via Lab).
//! * **CRD** (Color Rendering Dictionary) — the PCS→device transform, always a CLUT working
//!   in Lab, wrapped as a `ColorRendering` resource.
//!
//! PostScript uses XYZ as its internal PCS, but its interpolation tables are limited to 8
//! bits, so — exactly like the C reference — the CLUT paths convert through **Lab** (built by
//! a device-link transform `profile ↔ Lab4`) to favour perceptual accuracy, and the PS code
//! to convert Lab ↔ XYZ is emitted alongside. See `cmsps2.c` for the full algorithm notes.
//!
//! ## Fidelity: the two C mutable globals are gone
//!
//! The C source relies on two non-reentrant mutable globals:
//! * `_cmsPSActualColumn` (`cmsps2.c:287`) — the running output column used to wrap the hex
//!   CLUT tables at [`MAXPSCOLS`]. Here it lives as [`Emitter::column`], per-emitter state.
//! * `RemoveCR`'s `static char Buffer[2048]` (`cmsps2.c:320`) — here [`remove_cr`] returns an
//!   owned [`String`].
//!
//! Both are required refactors for a correct (re-entrant, thread-safe) Rust port, not
//! behavioural deviations: the emitted bytes are identical.
//!
//! ## The C two-pass size idiom
//!
//! `cmsGetPostScriptCSA`/`CRD` run serialization twice in C: once over a byte-counting
//! `NullIo` to size a caller buffer, then over a real `MemIo` to fill it. Since the Rust API
//! returns an owned [`Vec<u8>`], a single pass into a growable buffer suffices — the size is
//! just its `len()` — so [`get_postscript_csa`]/[`get_postscript_crd`] build and return a
//! `Vec` directly and [`get_postscript_color_resource`] writes into any [`std::io::Write`].
//!
//! ## Named-colour paths (`WriteNamedColorCSA`/`WriteNamedColorCRD`)
//!
//! The C code obtains the colour values through a `TYPE_NAMED_COLOR_INDEX` transform and
//! `cmsGetNamedColorList`. That named-colour transform glue (`_cmsStageAllocNamedColor`,
//! `cmsGetNamedColorList`) is deferred in this crate (see `DEFERRED.md` #2/#6). The values
//! that transform would produce are, by construction, exactly the list's stored PCS-Lab and
//! device-colorant entries, so these two functions read the `ncl2` tag's [`NamedColorList`]
//! directly and use `info().pcs` / `info().colorant`. This yields identical output without
//! the deferred machinery; the only nuance is that the stored PCS is decoded with the V4 Lab
//! encoding ([`cms_lab_encoded_to_float`]).

use std::io::Write;

use crate::cms_err;
use crate::cmm::blackpoint::detect_black_point;
use crate::cmm::optimize::{FLAGS_NOOPTIMIZE, FLAGS_NOWHITEONWHITEFIXUP};
use crate::cmm::pack::MAX_ENCODEABLE_XYZ;
use crate::cmm::transform::{create_multiprofile_transform, create_transform, FLAGS_BLACKPOINTCOMPENSATION};
use crate::color::d50_xyz;
use crate::color::gamma::ToneCurve;
use crate::color::pcs::{
    cms_lab_encoded_to_float, end_points_by_space, pt,
    reasonable_gridpoints_by_colorspace,
};
use crate::context::Context;
use crate::error::{ErrorCode, Result};
use crate::pipeline::stage::{StageData, SAMPLER_INSPECT, STAGE_CLUT, STAGE_CURVE_SET, STAGE_MATRIX};
use crate::pipeline::{Pipeline, Stage, StageLoc};
use crate::profile::read_hl::{
    read_input_lut, read_media_white_point, INTENT_ABSOLUTE_COLORIMETRIC, INTENT_PERCEPTUAL,
    INTENT_RELATIVE_COLORIMETRIC, INTENT_SATURATION,
};
use crate::profile::Profile;
use crate::tags::descriptor::tag_sig;
use crate::tags::TagValue;
use crate::types::color::CIEXYZ;
use crate::types::format::{FormatBuilder, PixelFormat};
use crate::types::num::quick_saturate_word;
use crate::types::sig::{class, colorspace, Signature};
use crate::virt::{create_lab4_profile, create_xyz_profile};
use crate::MAX_CHANNELS;

/// Columns on hex CLUT tables (`MAXPSCOLS`, `cmsps2.c:32`).
const MAXPSCOLS: i32 = 60;

/// `cmsFLAGS_NODEFAULTRESOURCEDEF` (`lcms2.h:1781`) — suppress the resource-definition
/// wrapper (`/Current exch … defineresource pop`) and the header/footer comments.
const FLAGS_NODEFAULTRESOURCEDEF: u32 = 0x0100_0000;

/// Which PostScript colour resource to generate (`cmsPSResourceType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsResourceType {
    /// `cmsPS_RESOURCE_CSA` — Color Space Array (device → PCS).
    Csa,
    /// `cmsPS_RESOURCE_CRD` — Color Rendering Dictionary (PCS → device).
    Crd,
}

// ==========================================================================================
// Emitter — the output stream + column counter (replaces cmsIOHANDLER + _cmsPSActualColumn)
// ==========================================================================================

/// The in-memory PostScript output being built.
///
/// Replaces the C `cmsIOHANDLER* m` plus the `_cmsPSActualColumn` global. `_cmsIOPrintf`
/// becomes [`Emitter::put`] (which never touches the column — matching the C code, where only
/// `WriteByte` and the CLUT sampler move the column) and `WriteByte` becomes
/// [`Emitter::write_byte`].
struct Emitter {
    out: Vec<u8>,
    /// Running output column for hex table wrapping (`_cmsPSActualColumn`).
    column: i32,
}

impl Emitter {
    fn new() -> Self {
        Self { out: Vec::new(), column: 0 }
    }

    /// `_cmsIOPrintf` — append already-formatted text. Unlike `WriteByte` it leaves the
    /// column counter untouched, exactly as the C `_cmsIOPrintf` does.
    fn put(&mut self, s: &str) {
        self.out.extend_from_slice(s.as_bytes());
    }

    /// `WriteByte` (`cmsps2.c:300`) — emit a byte as two lowercase hex digits, wrapping the
    /// line once the column passes [`MAXPSCOLS`].
    fn write_byte(&mut self, b: u8) {
        self.put(&format!("{b:02x}"));
        self.column += 2;
        if self.column > MAXPSCOLS {
            self.put("\n");
            self.column = 0;
        }
    }
}

/// `Word2Byte` (`cmsps2.c:292`) — quantise a 16-bit value to 8 bits with rounding.
fn word2byte(w: u16) -> u8 {
    (w as f64 / 257.0 + 0.5).floor() as u8
}

/// `RemoveCR` (`cmsps2.c:318`) — replace CR/LF with spaces and cap at 2047 bytes.
///
/// The C code copies into a `static char Buffer[2048]`; the Rust port returns an owned
/// [`String`], eliminating the non-reentrant global.
fn remove_cr(txt: &str) -> String {
    txt.chars()
        .take(2047)
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect()
}

/// `EmitPSEscaped` (`cmsps2.c:337`) — write a PostScript string-literal body, escaping
/// `\`, `(`, `)` and emitting non-printable / high-bit bytes as octal triples (PLRM 3.3.4.1).
/// The caller supplies the surrounding `(`…`)`.
fn emit_ps_escaped(em: &mut Emitter, txt: &str) {
    for &c in txt.as_bytes() {
        if c == b'\\' || c == b'(' || c == b')' {
            em.put(&format!("\\{}", c as char));
        } else if !(0x20..0x7F).contains(&c) {
            em.put(&format!("\\{c:03o}"));
        } else {
            em.put(&format!("{}", c as char));
        }
    }
}

// ==========================================================================================
// %g formatting — C printf's default-precision "%g"
// ==========================================================================================

/// Format `v` the way C's `printf("%g", v)` does (default precision 6): the shorter of `%e`
/// / `%f`, with trailing zeros and a dangling decimal point removed.
///
/// For the values this module feeds `%g` (gamma exponents ~0.1..3, media white points
/// ~0.8..1.0) the `%f` branch is always taken; the `%e` branch is implemented for
/// completeness and documented as unlikely to be hit.
fn fmt_g(v: f64) -> String {
    let prec = 6i32;
    if v == 0.0 {
        return "0".to_string();
    }
    if !v.is_finite() {
        // Match C's "inf"/"nan" spellings closely enough for a comment/argument context.
        return if v.is_nan() { "nan".to_string() } else if v > 0.0 { "inf".to_string() } else { "-inf".to_string() };
    }
    let exp = v.abs().log10().floor() as i32;
    if exp < -4 || exp >= prec {
        // %e with (prec-1) fractional digits, then strip trailing zeros in the mantissa.
        let s = format!("{:.*e}", (prec - 1) as usize, v);
        strip_g_exp(&s)
    } else {
        // %f with (prec-1-exp) fractional digits, then strip trailing zeros.
        let dec = (prec - 1 - exp).max(0) as usize;
        strip_trailing_zeros(&format!("{v:.dec$}"))
    }
}

/// Remove trailing fractional zeros (and a dangling `.`) from a plain decimal string.
fn strip_trailing_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let t = s.trim_end_matches('0');
    t.trim_end_matches('.').to_string()
}

/// Strip trailing mantissa zeros in a Rust `{:e}` string (`1.500000e2` → `1.5e2`).
fn strip_g_exp(s: &str) -> String {
    match s.split_once('e') {
        Some((mant, exp)) => format!("{}e{}", strip_trailing_zeros(mant), exp),
        None => s.to_string(),
    }
}

// ==========================================================================================
// Pixel-format helpers
// ==========================================================================================

/// `TYPE_Lab_DBL`.
fn type_lab_dbl() -> PixelFormat {
    FormatBuilder::new().float(true).colorspace(pt::LAB as u32).channels(3).bytes(0).build()
}
/// `TYPE_XYZ_DBL`.
fn type_xyz_dbl() -> PixelFormat {
    FormatBuilder::new().float(true).colorspace(pt::XYZ as u32).channels(3).bytes(0).build()
}
/// `TYPE_GRAY_8`.
fn type_gray_8() -> PixelFormat {
    FormatBuilder::new().colorspace(pt::GRAY as u32).channels(1).bytes(1).build()
}

/// `cmsFormatterForColorspaceOfProfile(hProfile, nBytes, isFloat)`.
///
/// Delegates to the canonical implementation in `cmm::pack` rather than re-deriving the
/// sig -> PT mapping. The local copy this replaced clamped the channel count with `.max(0)`,
/// so an unsupported colour space produced a word with zero channels but non-zero colourspace
/// bits; C returns the all-zero word (`cmspack.c:4026-4041`), which is what `pack` does.
fn formatter_for_colorspace(space: Signature, n_bytes: u32, is_float: bool) -> PixelFormat {
    crate::cmm::pack::formatter_for_colorspace(space, n_bytes, is_float)
}

// ==========================================================================================
// Simple emitters (headers, ranges, intents, Lab<->XYZ PostScript)
// ==========================================================================================

/// A minimal UTC `asctime`-style timestamp (`Www Mmm dd HH:MM:SS YYYY\n`).
///
/// The C `EmitHeader` uses `ctime(&timer)`, i.e. *local* time. A pure Rust port cannot match
/// libc's local-time output byte-for-byte without a timezone database, so this emits UTC via
/// Howard Hinnant's civil-from-days algorithm. It is a non-semantic comment line; only its
/// shape matters.
fn asctime_utc(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // civil_from_days (Hinnant): days since 1970-01-01 -> (year, month, day).
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y0 = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = y0 + if m <= 2 { 1 } else { 0 };

    let wday = (((days % 7) + 4).rem_euclid(7)) as usize; // 0 = Sunday
    const WD: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MO: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {} {:2} {:02}:{:02}:{:02} {}\n", WD[wday], MO[(m - 1) as usize], d, h, mi, s, y)
}

/// Read an ASCII string from an MLU tag (`cmsReadTag` + `cmsMLUgetASCII`), capped at 255
/// bytes like the C `DescASCII`/`CopyrightASCII` buffers. Missing / non-MLU tags → `""`.
fn read_mlu_ascii(prof: &mut Profile, sig: Signature) -> String {
    let s = match prof.read_tag(sig) {
        Ok(TagValue::Mlu(m)) => m.get_ascii([0, 0], [0, 0]).unwrap_or_default(),
        _ => String::new(),
    };
    s.chars().take(255).collect()
}

/// `EmitHeader` (`cmsps2.c:359`).
fn emit_header(em: &mut Emitter, title: &str, prof: &mut Profile) {
    let desc = remove_cr(&read_mlu_ascii(prof, tag_sig::PROFILE_DESCRIPTION));
    let copyright = remove_cr(&read_mlu_ascii(prof, tag_sig::COPYRIGHT));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    em.put("%!PS-Adobe-3.0\n");
    em.put("%\n");
    em.put(&format!("% {title}\n"));
    em.put(&format!("% Source: {desc}\n"));
    em.put(&format!("%         {copyright}\n"));
    em.put(&format!("% Created: {}", asctime_utc(now))); // asctime appends a \n
    em.put("%\n");
    em.put("%%BeginResource\n");
}

/// `EmitWhiteBlackD50` (`cmsps2.c:392`) — black point (device, adapted to D50) + D50 white.
fn emit_white_black_d50(em: &mut Emitter, black_point: &CIEXYZ) {
    em.put(&format!(
        "/BlackPoint [{:.6} {:.6} {:.6}]\n",
        black_point.x, black_point.y, black_point.z
    ));
    let d50 = d50_xyz();
    em.put(&format!("/WhitePoint [{:.6} {:.6} {:.6}]\n", d50.x, d50.y, d50.z));
}

/// `EmitRangeCheck` (`cmsps2.c:406`) — clamp the top-of-stack value to `0..1`.
fn emit_range_check(em: &mut Emitter) {
    em.put("dup 0.0 lt { pop 0.0 } if dup 1.0 gt { pop 1.0 } if ");
}

/// `EmitIntent` (`cmsps2.c:416`).
fn emit_intent(em: &mut Emitter, rendering_intent: u32) {
    let intent = match rendering_intent {
        INTENT_PERCEPTUAL => "Perceptual",
        INTENT_RELATIVE_COLORIMETRIC => "RelativeColorimetric",
        INTENT_ABSOLUTE_COLORIMETRIC => "AbsoluteColorimetric",
        INTENT_SATURATION => "Saturation",
        _ => "Undefined",
    };
    em.put(&format!("/RenderingIntent ({intent})\n"));
}

/// `EmitLab2XYZ` (`cmsps2.c:443`) — the PS stages converting Lab → XYZ.
fn emit_lab2xyz(em: &mut Emitter) {
    em.put("/RangeABC [ 0 1 0 1 0 1]\n");
    em.put("/DecodeABC [\n");
    em.put("{100 mul  16 add 116 div } bind\n");
    em.put("{255 mul 128 sub 500 div } bind\n");
    em.put("{255 mul 128 sub 200 div } bind\n");
    em.put("]\n");
    em.put("/MatrixABC [ 1 1 1 1 0 0 0 0 -1]\n");
    em.put("/RangeLMN [ -0.236 1.254 0 1 -0.635 1.640 ]\n");
    em.put("/DecodeLMN [\n");
    em.put("{dup 6 29 div ge {dup dup mul mul} {4 29 div sub 108 841 div mul} ifelse 0.964200 mul} bind\n");
    em.put("{dup 6 29 div ge {dup dup mul mul} {4 29 div sub 108 841 div mul} ifelse } bind\n");
    em.put("{dup 6 29 div ge {dup dup mul mul} {4 29 div sub 108 841 div mul} ifelse 0.824900 mul} bind\n");
    em.put("]\n");
}

/// `EmitXYZ2Lab` (`cmsps2.c:1266`) — the PS stages converting XYZ → Lab.
fn emit_xyz2lab(em: &mut Emitter) {
    em.put("/RangeLMN [ -0.635 2.0 0 2 -0.635 2.0 ]\n");
    em.put("/EncodeLMN [\n");
    em.put("{ 0.964200  div dup 0.008856 le {7.787 mul 16 116 div add}{1 3 div exp} ifelse } bind\n");
    em.put("{ 1.000000  div dup 0.008856 le {7.787 mul 16 116 div add}{1 3 div exp} ifelse } bind\n");
    em.put("{ 0.824900  div dup 0.008856 le {7.787 mul 16 116 div add}{1 3 div exp} ifelse } bind\n");
    em.put("]\n");
    em.put("/MatrixABC [ 0 1 0 1 -1 1 0 0 -1 ]\n");
    em.put("/EncodeABC [\n");
    em.put("{ 116 mul  16 sub 100 div  } bind\n");
    em.put("{ 500 mul 128 add 256 div  } bind\n");
    em.put("{ 200 mul 128 add 256 div  } bind\n");
    em.put("]\n");
}

// ==========================================================================================
// Gamma / tone-curve emitters
// ==========================================================================================

/// `Emit1Gamma` (`cmsps2.c:465`) — emit one tone curve as a PostScript procedure.
///
/// Empty / linear curves (or `None`) collapse to `{ 1 } bind`; a curve that looks like a pure
/// exponential is emitted as `{ g exp }`; otherwise the full 16-bit table is dumped with the
/// 1-D interpolation procedure. `cmsGetToneCurveEstimatedTable` maps to
/// [`ToneCurve::estimated_table`] and `nEntries` to [`ToneCurve::estimated_table_entries`].
fn emit_1gamma(em: &mut Emitter, table: Option<&ToneCurve>) {
    let curve = match table {
        Some(c) if c.estimated_table_entries() > 0 && !c.is_linear() => c,
        // On error, empty tables or linear assume gamma 1.0.
        _ => {
            em.put("{ 1 } bind ");
            return;
        }
    };

    // Check if is really an exponential. If so, emit "exp".
    let gamma = curve.estimate_gamma(0.001);
    if gamma > 0.0 {
        em.put(&format!("{{ {} exp }} bind ", fmt_g(gamma)));
        return;
    }

    em.put("{ ");
    emit_range_check(em);

    // Emit interpolation code (the table, then the fixed 1-D interpolation procedure).
    let tab = curve.estimated_table();
    em.put(" [");
    for (i, v) in tab.iter().enumerate() {
        if i % 10 == 0 {
            em.put("\n  ");
        }
        em.put(&format!("{v} "));
    }
    em.put("] ");

    em.put("dup length 1 sub 3 -1 roll mul dup dup floor cvi exch ceiling cvi 3 index exch get\n  ");
    em.put("4 -1 roll 3 -1 roll get dup 3 1 roll sub 3 -1 roll dup floor cvi sub mul add 65535 div\n");
    em.put(" } bind ");
}

/// `GammaTableEquals` (`cmsps2.c:542`) — compare two curves' 16-bit tables.
fn gamma_table_equals(a: &ToneCurve, b: &ToneCurve) -> bool {
    a.estimated_table_entries() == b.estimated_table_entries()
        && a.estimated_table() == b.estimated_table()
}

/// `EmitNGamma` (`cmsps2.c:552`) — emit a set of curves, collapsing a run of identical
/// consecutive curves to `dup`.
fn emit_ngamma(em: &mut Emitter, curves: &[ToneCurve]) {
    for i in 0..curves.len() {
        if i > 0 && gamma_table_equals(&curves[i - 1], &curves[i]) {
            em.put("dup ");
        } else {
            emit_1gamma(em, Some(&curves[i]));
        }
    }
}

// ==========================================================================================
// CLUT dumping (WriteCLUT + OutputValueSampler)
// ==========================================================================================

/// Per-row state carried through the CLUT sampler (`cmsPsSamplerCargo` minus the `m`/pipeline
/// pointers, which the closure captures directly).
struct SamplerCargo<'a> {
    first_component: i32,
    second_component: i32,
    pre_maj: &'a str,
    post_maj: &'a str,
    pre_min: &'a str,
    post_min: &'a str,
    fix_white: bool,
    color_space: Signature,
}

/// `OutputValueSampler` (`cmsps2.c:589`) — SAMPLER_INSPECT callback: emit one CLUT knot.
///
/// Rows are bracketed with the `PreMaj`/`PostMaj`/`PreMin`/`PostMin` delimiters; the "fix
/// white" logic forces pure white for the `L*=100, a≈b≈0` slice so the Lab encoding's off-node
/// zero does not cause a scum dot.
/// Returns `false` to stop the sampling loop, mirroring C's `return 0`.
fn output_value_sampler(
    em: &mut Emitter,
    sc: &mut SamplerCargo,
    in_: &[u16],
    out: &mut [u16],
) -> bool {
    if sc.fix_white && in_[0] == 0xFFFF {
        // Only in L* = 100, ab = [-8..8].
        if (0x7800..=0x8800).contains(&in_[1]) && (0x7800..=0x8800).contains(&in_[2]) {
            let Some(ep) = end_points_by_space(sc.color_space) else {
                // C bails out of the whole sampling loop here (`cmsps2.c:606-607`:
                // `if (!_cmsEndPointsBySpace(...)) return 0;`), leaving `WriteCLUT` to close a
                // partially written /RenderTable. Reachable for a colour space
                // `_cmsEndPointsBySpace` does not cover, e.g. a 2-channel or MCHn device space.
                return false;
            };
            let n = (ep.n_outputs as usize).min(out.len());
            out[..n].copy_from_slice(&ep.white[..n]);
        }
    }

    // Handle the parenthesis on rows.
    if in_[0] as i32 != sc.first_component {
        if sc.first_component != -1 {
            em.put(sc.post_min);
            sc.second_component = -1;
            em.put(sc.post_maj);
        }
        // Begin block.
        em.column = 0;
        em.put(sc.pre_maj);
        sc.first_component = in_[0] as i32;
    }

    if in_[1] as i32 != sc.second_component {
        if sc.second_component != -1 {
            em.put(sc.post_min);
        }
        em.put(sc.pre_min);
        sc.second_component = in_[1] as i32;
    }

    // Dump table. We always deal with Lab4, so every output is a byte.
    for &w in out.iter() {
        em.write_byte(word2byte(w));
    }
    true
}

/// `WriteCLUT` (`cmsps2.c:668`) — dump a CLUT stage's grid dims + hex sample table.
///
/// `mpe` is cloned by the caller so it can be sampled (`cmsStageSampleCLut16bit` takes
/// `&mut`); SAMPLER_INSPECT means the table is read, never written back.
#[allow(clippy::too_many_arguments)]
fn write_clut(
    em: &mut Emitter,
    mpe: &mut Stage,
    pre_maj: &str,
    post_maj: &str,
    pre_min: &str,
    post_min: &str,
    fix_white: bool,
    color_space: Signature,
) {
    // Read the grid geometry before the mutable sample borrow.
    let dims: Vec<u32> = match &mpe.data {
        StageData::Clut(c) => {
            // C bounds this loop with `if (i < MAX_INPUT_DIMENSIONS)` (`cmsps2.c:688-693`)
            // precisely because `nSamples[]` is a fixed-size array; a corrupted profile can
            // report more inputs than it has slots. Clamp rather than slice-panic.
            let n = c.params.n_inputs.min(c.params.n_samples.len());
            c.params.n_samples[..n].to_vec()
        }
        _ => return,
    };

    em.put("[");
    for &d in &dims {
        em.put(&format!(" {d} "));
    }
    em.put(" [\n");

    let mut sc = SamplerCargo {
        first_component: -1,
        second_component: -1,
        pre_maj,
        post_maj,
        pre_min,
        post_min,
        fix_white,
        color_space,
    };
    mpe.sample_clut_16(
        |in_, out| output_value_sampler(em, &mut sc, in_, out),
        SAMPLER_INSPECT,
    );

    em.put(post_min);
    em.put(post_maj);
    em.put("] ");
}

/// Build a genuine 16-bit CLUT stage by sampling a device-link pipeline at reasonable grid
/// points.
///
/// This mirrors what the C `_cmsOptimizePipeline(..., cmsFLAGS_FORCE_CLUT)` does internally in
/// `OptimizeByResampling`: it samples the device link (`v->Lut`) onto a fresh CLUT of
/// [`reasonable_gridpoints_by_colorspace`] resolution. We do it explicitly here because this
/// crate's resampling optimizer fuses pre-linearization + CLUT + post-linearization into a
/// single opaque `Prelin16` evaluator stage (`single_stage_pipeline`), which exposes no
/// samplable `cmsSigCLutElemType` for `WriteCLUT` to dump. Sampling the pipeline directly
/// yields the same table the C code emits.
///
/// The one thing not reproduced is the optimizer-internal white-point *misalignment* fixup
/// (`fix_white_misalignment`); the `ps2` code's own per-knot `FixWhite` in
/// [`output_value_sampler`] (used by the CRD path) is fully ported and still applies.
fn build_clut_from_pipeline(dl: &Pipeline, grid_space: Signature, flags: u32) -> Result<Stage> {
    let n_in = dl.input_channels();
    let n_out = dl.output_channels();
    let n_grid = reasonable_gridpoints_by_colorspace(grid_space, flags);
    let dims = vec![n_grid; n_in as usize];
    let mut stage = Stage::alloc_clut_16bit_granular(&dims, n_in, n_out, None)?;
    stage.sample_clut_16(
        |in_, out| {
            dl.eval_16(in_, out);
            true
        },
        0,
    );
    Ok(stage)
}

// ==========================================================================================
// CSA — Color Space Array
// ==========================================================================================

/// `EmitCIEBasedA` (`cmsps2.c:714`) — monochrome CSA.
fn emit_ciebased_a(em: &mut Emitter, curve: Option<&ToneCurve>, black_point: &CIEXYZ) {
    em.put("[ /CIEBasedA\n");
    em.put("  <<\n");
    em.put("/DecodeA ");
    emit_1gamma(em, curve);
    em.put(" \n");
    em.put("/MatrixA [ 0.9642 1.0000 0.8249 ]\n");
    em.put("/RangeLMN [ 0.0 0.9642 0.0 1.0000 0.0 0.8249 ]\n");
    emit_white_black_d50(em, black_point);
    emit_intent(em, INTENT_PERCEPTUAL);
    em.put(">>\n");
    em.put("]\n");
}

/// `EmitCIEBasedABC` (`cmsps2.c:742`) — matrix-shaper CSA. `matrix` is a row-major 3×3
/// (`cmsMAT3` memory layout); the PostScript `/MatrixABC` is column-major, so it is emitted
/// transposed exactly as the C indexing `Matrix[i + 3*j]` does.
fn emit_ciebased_abc(
    em: &mut Emitter,
    matrix: &[f64; 9],
    curve_set: &[ToneCurve],
    black_point: &CIEXYZ,
) {
    em.put("[ /CIEBasedABC\n");
    em.put("<<\n");
    em.put("/DecodeABC [ ");
    emit_ngamma(em, curve_set);
    em.put("]\n");
    em.put("/MatrixABC [ ");
    // Column-major emission of the row-major matrix (the C `Matrix[i + 3*j]` transpose):
    // column j holds elements `matrix[i]`, `matrix[i + 3]`, `matrix[i + 6]`.
    for i in 0..3 {
        em.put(&format!("{:.6} {:.6} {:.6} ", matrix[i], matrix[i + 3], matrix[i + 6]));
    }
    em.put("]\n");
    em.put("/RangeLMN [ 0.0 0.9642 0.0 1.0000 0.0 0.8249 ]\n");
    emit_white_black_d50(em, black_point);
    emit_intent(em, INTENT_PERCEPTUAL);
    em.put(">>\n");
    em.put("]\n");
}

/// `EmitCIEBasedDEF` (`cmsps2.c:780`) — CLUT-based CSA (`/CIEBasedDEF` or `/CIEBasedDEFG`).
fn emit_ciebased_def(
    em: &mut Emitter,
    pipeline: &Pipeline,
    intent: u32,
    black_point: &CIEXYZ,
) -> Result<()> {
    let stages = pipeline.stages();
    let first = stages
        .first()
        .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "empty device link for CSA"))?;

    let (pre_maj, post_maj, pre_min, post_min) = match first.input_channels {
        3 => {
            em.put("[ /CIEBasedDEF\n");
            ("<", ">\n", "", "")
        }
        4 => {
            em.put("[ /CIEBasedDEFG\n");
            ("[", "]\n", "<", ">\n")
        }
        _ => return Err(cms_err!(ErrorCode::ColorspaceCheck, "CSA supports only 3 or 4 input channels")),
    };

    em.put("<<\n");

    // Walk stages: optional prelinearization CurveSet, then the CLUT.
    let mut idx = 0usize;
    if first.type_signature() == STAGE_CURVE_SET {
        if let StageData::ToneCurves(curves) = &first.data {
            em.put("/DecodeDEF [ ");
            emit_ngamma(em, curves);
            em.put("]\n");
        }
        idx = 1;
    }

    if let Some(clut_stage) = stages.get(idx) {
        if clut_stage.type_signature() == STAGE_CLUT {
            em.put("/Table ");
            let mut cs = clut_stage.clone();
            write_clut(em, &mut cs, pre_maj, post_maj, pre_min, post_min, false, Signature(0));
            em.put("]\n");
        }
    }

    emit_lab2xyz(em);
    emit_white_black_d50(em, black_point);
    emit_intent(em, intent);
    em.put("   >>\n");
    em.put("]\n");
    Ok(())
}

/// `ExtractGray2Y` (`cmsps2.c:841`) — build a gray→Y tone curve from a monochrome profile by
/// transforming the 256 gray levels through a device→XYZ transform and keeping `Y`.
fn extract_gray2y(ctx: &Context, prof: &mut Profile, intent: u32) -> Result<ToneCurve> {
    let mut xyz = create_xyz_profile()?;
    let xform =
        create_transform(ctx, prof, type_gray_8(), &mut xyz, type_xyz_dbl(), intent, FLAGS_NOOPTIMIZE)?;

    let mut table = [0u16; 256];
    let mut dst = [0u8; 24]; // 3 × f64 (X, Y, Z)
    for (i, slot) in table.iter_mut().enumerate() {
        let src = [i as u8];
        xform.do_transform(&src, &mut dst, 1);
        // TYPE_XYZ_DBL packs X, Y, Z as native-endian f64; Y is the middle double.
        let y = f64::from_ne_bytes(dst[8..16].try_into().unwrap());
        *slot = quick_saturate_word(y * 65535.0);
    }
    ToneCurve::build_tabulated_16(256, &table)
}

/// `WriteInputLUT` (`cmsps2.c:871`) — CLUT/monochrome CSA via a device→Lab4 device link.
fn write_input_lut(
    em: &mut Emitter,
    ctx: &Context,
    prof: &mut Profile,
    intent: u32,
    flags: u32,
) -> Result<()> {
    let input_format = formatter_for_colorspace(prof.color_space(), 2, false);
    let n_channels = input_format.channels();

    let black_point = detect_black_point(prof, intent, 0).unwrap_or_default();

    // Device-link transform profile -> Lab4, dumped as the working CSA.
    let mut lab = create_lab4_profile(None)?;
    let xform = create_multiprofile_transform(
        ctx,
        &mut [&mut *prof, &mut lab],
        input_format,
        type_lab_dbl(),
        intent,
        0,
    )?;

    // Only 1, 3 and 4 channels are allowed.
    match n_channels {
        1 => {
            let gray2y = extract_gray2y(ctx, prof, intent)?;
            emit_ciebased_a(em, Some(&gray2y), &black_point);
            Ok(())
        }
        3 | 4 => {
            let dl = xform
                .pipeline()
                .cloned()
                .ok_or_else(|| cms_err!(ErrorCode::Null, "transform has no pipeline"))?;

            // Force a CLUT (grid sized by the device colour space) and dump it as CIEBasedDEF.
            let clut = build_clut_from_pipeline(&dl, prof.color_space(), flags)?;
            let mut device_link = Pipeline::alloc(clut.input_channels, clut.output_channels)?;
            device_link.insert_stage(StageLoc::End, clut);

            emit_ciebased_def(em, &device_link, intent, &black_point)
        }
        n => Err(cms_err!(
            ErrorCode::ColorspaceCheck,
            "Only 3, 4 channels are supported for CSA. This profile has {n} channels."
        )),
    }
}

/// `WriteInputMatrixShaper` (`cmsps2.c:963`) — matrix-shaper CSA (gray or RGB).
fn write_input_matrix_shaper(
    em: &mut Emitter,
    prof: &mut Profile,
    matrix: &Stage,
    shaper: &Stage,
) -> Result<()> {
    let color_space = prof.color_space();
    let black_point = detect_black_point(prof, INTENT_RELATIVE_COLORIMETRIC, 0).unwrap_or_default();

    let curves = match &shaper.data {
        StageData::ToneCurves(c) => c.as_slice(),
        _ => return Err(cms_err!(ErrorCode::NotSuitable, "expected a curve-set shaper stage")),
    };

    if color_space == colorspace::GRAY {
        emit_ciebased_a(em, curves.first(), &black_point);
        Ok(())
    } else if color_space == colorspace::RGB {
        let m = match &matrix.data {
            StageData::Matrix { m, .. } if m.len() == 9 => m,
            _ => return Err(cms_err!(ErrorCode::NotSuitable, "expected a 3x3 matrix stage")),
        };
        // Scale the D50-relative matrix into encodeable XYZ, matching the C `MAX_ENCODEABLE_XYZ`.
        let mut mat = [0f64; 9];
        for (dst, &src) in mat.iter_mut().zip(m.iter()) {
            *dst = src * MAX_ENCODEABLE_XYZ;
        }
        emit_ciebased_abc(em, &mat, curves, &black_point);
        Ok(())
    } else {
        Err(cms_err!(ErrorCode::ColorspaceCheck, "Profile is not suitable for CSA. Unsupported colorspace."))
    }
}

/// `WriteNamedColorCSA` (`cmsps2.c:1010`) — HP named-colour CSA (works in Lab).
///
/// See the module header: the C named-colour→Lab transform's output equals the list's stored
/// PCS-Lab, so we read the `ncl2` [`NamedColorList`] directly and decode `info().pcs`.
fn write_named_color_csa(em: &mut Emitter, prof: &mut Profile) -> Result<()> {
    let list = match prof.read_tag(tag_sig::NAMED_COLOR2)? {
        TagValue::NamedColor(l) => l,
        _ => return Err(cms_err!(ErrorCode::NotSuitable, "profile has no named colour list")),
    };

    em.put("<<\n");
    em.put("(colorlistcomment) (Named color CSA)\n");
    em.put("(Prefix) [ (Pantone ) (PANTONE ) ]\n");
    em.put("(Suffix) [ ( CV) ( CVC) ( C) ]\n");

    for i in 0..list.count() {
        let info = match list.info(i) {
            Some(v) => v,
            None => continue,
        };
        let lab = cms_lab_encoded_to_float(&info.pcs);
        em.put("  (");
        emit_ps_escaped(em, info.name);
        em.put(&format!(") [ {:.3} {:.3} {:.3} ]\n", lab.l, lab.a, lab.b));
    }

    em.put(">>\n");
    Ok(())
}

/// `GenerateCSA` (`cmsps2.c:1063`) — Color Space Array on the XYZ colour space.
fn generate_csa(
    em: &mut Emitter,
    ctx: &Context,
    prof: &mut Profile,
    intent: u32,
    flags: u32,
) -> Result<()> {
    // Is a named color profile?
    if prof.device_class() == class::NAMED_COLOR {
        return write_named_color_csa(em, prof);
    }

    // Any profile class is allowed (including devicelink), but output (PCS) must be XYZ or Lab.
    let pcs = prof.pcs();
    if pcs != colorspace::XYZ && pcs != colorspace::LAB {
        return Err(cms_err!(ErrorCode::ColorspaceCheck, "Invalid output color space"));
    }

    // Read the LUT with all necessary conversion stages.
    let lut = read_input_lut(prof, intent)?;

    // Tone curves + matrix can be implemented without any LUT.
    let is_matrix_shaper = {
        let s = lut.stages();
        s.len() == 2
            && s[0].type_signature() == STAGE_CURVE_SET
            && s[1].type_signature() == STAGE_MATRIX
    };

    if is_matrix_shaper {
        let s = lut.stages();
        write_input_matrix_shaper(em, prof, &s[1], &s[0])
    } else {
        // We need a LUT for the rest.
        write_input_lut(em, ctx, prof, intent, flags)
    }
}

// ==========================================================================================
// CRD — Color Rendering Dictionary
// ==========================================================================================

/// `EmitPQRStage` (`cmsps2.c:1193`) — the PQR chromatic-adaptation stage, in three flavours:
/// absolute-colorimetric (re-encode to relative), plain Von-Kries, or Von-Kries + BPC.
fn emit_pqr_stage(
    em: &mut Emitter,
    prof: &mut Profile,
    do_bpc: bool,
    is_absolute: bool,
) -> Result<()> {
    if is_absolute {
        // For absolute colorimetric intent, encode back to relative and generate a relative
        // pipeline. Relative encoding is obtained across XYZpcs*(D50/WhitePoint).
        let white = read_media_white_point(prof)?;

        em.put("/MatrixPQR [1 0 0 0 1 0 0 0 1 ]\n");
        em.put("/RangePQR [ -0.5 2 -0.5 2 -0.5 2 ]\n");
        em.put("% Absolute colorimetric -- encode to relative to maximize LUT usage\n");
        em.put("/TransformPQR [\n");
        em.put(&format!("{{0.9642 mul {} div exch pop exch pop exch pop exch pop}} bind\n", fmt_g(white.x)));
        em.put(&format!("{{1.0000 mul {} div exch pop exch pop exch pop exch pop}} bind\n", fmt_g(white.y)));
        em.put(&format!("{{0.8249 mul {} div exch pop exch pop exch pop exch pop}} bind\n]\n", fmt_g(white.z)));
        return Ok(());
    }

    em.put("% Bradford Cone Space\n");
    em.put("/MatrixPQR [0.8951 -0.7502 0.0389 0.2664 1.7135 -0.0685 -0.1614 0.0367 1.0296 ] \n");
    em.put("/RangePQR [ -0.5 2 -0.5 2 -0.5 2 ]\n");

    if !do_bpc {
        em.put("% VonKries-like transform in Bradford Cone Space\n");
        em.put("/TransformPQR [\n");
        em.put("{exch pop exch 3 get mul exch pop exch 3 get div} bind\n");
        em.put("{exch pop exch 4 get mul exch pop exch 4 get div} bind\n");
        em.put("{exch pop exch 5 get mul exch pop exch 5 get div} bind\n]\n");
    } else {
        em.put("% VonKries-like transform in Bradford Cone Space plus BPC\n");
        em.put("/TransformPQR [\n");
        em.put(
            "{4 index 3 get div 2 index 3 get mul \
             2 index 3 get 2 index 3 get sub mul \
             2 index 3 get 4 index 3 get 3 index 3 get sub mul sub \
             3 index 3 get 3 index 3 get exch sub div \
             exch pop exch pop exch pop exch pop } bind\n",
        );
        em.put(
            "{4 index 4 get div 2 index 4 get mul \
             2 index 4 get 2 index 4 get sub mul \
             2 index 4 get 4 index 4 get 3 index 4 get sub mul sub \
             3 index 4 get 3 index 4 get exch sub div \
             exch pop exch pop exch pop exch pop } bind\n",
        );
        em.put(
            "{4 index 5 get div 2 index 5 get mul \
             2 index 5 get 2 index 5 get sub mul \
             2 index 5 get 4 index 5 get 3 index 5 get sub mul sub \
             3 index 5 get 3 index 5 get exch sub div \
             exch pop exch pop exch pop exch pop } bind\n]\n",
        );
    }
    Ok(())
}

/// `BuildColorantList` (`cmsps2.c:1418`) — the device colorant values in `0..1`, space-joined.
fn build_colorant_list(n_colorant: usize, out: &[u16]) -> String {
    let n = n_colorant.min(MAX_CHANNELS).min(out.len());
    (0..n)
        .map(|j| format!("{:.3}", out[j] as f64 / 65535.0))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `WriteOutputLUT` (`cmsps2.c:1295`) — the CRD body: a Lab→device CLUT plus the PQR/XYZ↔Lab
/// stages. CRDs are always CLUT-based, always in Lab.
fn write_output_lut(
    em: &mut Emitter,
    ctx: &Context,
    prof: &mut Profile,
    intent: u32,
    flags: u32,
) -> Result<()> {
    let output_format = formatter_for_colorspace(prof.color_space(), 2, false);
    let n_channels = output_format.channels();
    let color_space = prof.color_space();
    let do_bpc = flags & FLAGS_BLACKPOINTCOMPENSATION != 0;
    let mut fix_white = flags & FLAGS_NOWHITEONWHITEFIXUP == 0;

    // For absolute colorimetric, encode the LUT as relative to preserve precision.
    let relative_intent =
        if intent == INTENT_ABSOLUTE_COLORIMETRIC { INTENT_RELATIVE_COLORIMETRIC } else { intent };

    // Use V4 Lab always: transform Lab4 -> profile.
    let mut lab = create_lab4_profile(None)?;
    let xform = create_multiprofile_transform(
        ctx,
        &mut [&mut lab, &mut *prof],
        type_lab_dbl(),
        output_format,
        relative_intent,
        0,
    )?;

    let dl = xform
        .pipeline()
        .cloned()
        .ok_or_else(|| cms_err!(ErrorCode::CorruptionDetected, "Cannot access link for CRD"))?;

    // We need a CLUT: sample the Lab -> device link onto a Lab-resolution grid.
    let mut clut = build_clut_from_pipeline(&dl, colorspace::LAB, flags)
        .map_err(|_| cms_err!(ErrorCode::CorruptionDetected, "Cannot create CLUT table for CRD"))?;

    em.put("<<\n");
    em.put("/ColorRenderingType 1\n");

    let black_point = detect_black_point(prof, intent, 0).unwrap_or_default();

    // Emit headers, etc.
    emit_white_black_d50(em, &black_point);
    emit_pqr_stage(em, prof, do_bpc, intent == INTENT_ABSOLUTE_COLORIMETRIC)?;
    emit_xyz2lab(em);

    // FIXUP: map Lab (100,0,0) to perfect white — the Lab encoding puts a=b=0 slightly off a
    // node, so without this the L=100 slice would show a scum dot. Disabled for absolute.
    if intent == INTENT_ABSOLUTE_COLORIMETRIC {
        fix_white = false;
    }

    em.put("/RenderTable ");

    if clut.type_signature() != STAGE_CLUT {
        return Err(cms_err!(ErrorCode::CorruptionDetected, "Cannot create CLUT, revise your flags!"));
    }
    write_clut(em, &mut clut, "<", ">\n", "", "", fix_white, color_space);

    em.put(&format!(" {n_channels} {{}} bind "));
    for _ in 1..n_channels {
        em.put("dup ");
    }
    em.put("]\n");

    emit_intent(em, intent);
    em.put(">>\n");

    if flags & FLAGS_NODEFAULTRESOURCEDEF == 0 {
        em.put("/Current exch /ColorRendering defineresource pop\n");
    }
    Ok(())
}

/// `WriteNamedColorCRD` (`cmsps2.c:1443`) — HP named-colour CRD.
///
/// As with the CSA variant, the C named-colour→device transform's output equals the list's
/// stored device colorant, so the `ncl2` list is read directly (`info().colorant`).
fn write_named_color_crd(
    em: &mut Emitter,
    prof: &mut Profile,
    flags: u32,
) -> Result<()> {
    let output_format = formatter_for_colorspace(prof.color_space(), 2, false);
    let n_colorant = output_format.channels() as usize;

    let list = match prof.read_tag(tag_sig::NAMED_COLOR2)? {
        TagValue::NamedColor(l) => l,
        _ => return Err(cms_err!(ErrorCode::NotSuitable, "profile has no named colour list")),
    };

    em.put("<<\n");
    em.put("(colorlistcomment) (Named profile) \n");
    em.put("(Prefix) [ (Pantone ) (PANTONE ) ]\n");
    em.put("(Suffix) [ ( CV) ( CVC) ( C) ]\n");

    for i in 0..list.count() {
        let info = match list.info(i) {
            Some(v) => v,
            None => continue,
        };
        let colorant = build_colorant_list(n_colorant, info.colorant);
        em.put("  (");
        emit_ps_escaped(em, info.name);
        em.put(&format!(") [ {colorant} ]\n"));
    }

    em.put("   >>");
    if flags & FLAGS_NODEFAULTRESOURCEDEF == 0 {
        em.put(" /Current exch /HPSpotTable defineresource pop\n");
    }
    Ok(())
}

/// `GenerateCRD` (`cmsps2.c:1510`) — Color Rendering Dictionary (always LUT-based).
fn generate_crd(
    em: &mut Emitter,
    ctx: &Context,
    prof: &mut Profile,
    intent: u32,
    flags: u32,
) -> Result<()> {
    if flags & FLAGS_NODEFAULTRESOURCEDEF == 0 {
        emit_header(em, "Color Rendering Dictionary (CRD)", prof);
    }

    // Is a named color profile?
    if prof.device_class() == class::NAMED_COLOR {
        write_named_color_crd(em, prof, flags)?;
    } else {
        // CRDs are always implemented as LUT.
        write_output_lut(em, ctx, prof, intent, flags)?;
    }

    if flags & FLAGS_NODEFAULTRESOURCEDEF == 0 {
        em.put("%%EndResource\n");
        em.put("\n% CRD End\n");
    }
    Ok(())
}

// ==========================================================================================
// Public API
// ==========================================================================================

/// `cmsGetPostScriptColorResource` — write a CSA or CRD for `prof` into `out`, returning the
/// number of bytes written.
pub fn get_postscript_color_resource(
    ctx: &Context,
    ty: PsResourceType,
    prof: &mut Profile,
    intent: u32,
    flags: u32,
    out: &mut impl Write,
) -> Result<usize> {
    let mut em = Emitter::new();
    match ty {
        PsResourceType::Csa => generate_csa(&mut em, ctx, prof, intent, flags)?,
        PsResourceType::Crd => generate_crd(&mut em, ctx, prof, intent, flags)?,
    }
    out.write_all(&em.out)
        .map_err(|e| cms_err!(ErrorCode::Write, "failed to write PostScript resource: {e}"))?;
    Ok(em.out.len())
}

/// `cmsGetPostScriptCSA` — the Color Space Array for `prof` as owned bytes.
///
/// The C two-pass (size then fill) idiom collapses to a single pass here: the returned
/// `Vec`'s length *is* the byte count the C API reports.
pub fn get_postscript_csa(ctx: &Context, prof: &mut Profile, intent: u32, flags: u32) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    get_postscript_color_resource(ctx, PsResourceType::Csa, prof, intent, flags, &mut buf)?;
    Ok(buf)
}

/// `cmsGetPostScriptCRD` — the Color Rendering Dictionary for `prof` as owned bytes.
pub fn get_postscript_crd(ctx: &Context, prof: &mut Profile, intent: u32, flags: u32) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    get_postscript_color_resource(ctx, PsResourceType::Crd, prof, intent, flags, &mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_profile as load;

    #[test]
    fn remove_cr_strips_carriage_returns() {
        assert_eq!(remove_cr("a\r\nb"), "a  b");
        assert_eq!(remove_cr("no crs here"), "no crs here");
    }

    #[test]
    fn write_byte_wraps_at_60_columns() {
        let mut em = Emitter::new();
        // 30 bytes => exactly 60 columns, no wrap yet; the 31st pushes past MAXPSCOLS.
        for _ in 0..31 {
            em.write_byte(0xAB);
        }
        assert!(em.out.contains(&b'\n'), "a wrap newline must have been emitted");
        assert_eq!(em.column, 0, "column resets after a wrap");
        // WriteByte appends the two hex digits *then* wraps when the column passes MAXPSCOLS,
        // so a full line is at most MAXPSCOLS + 2 (= 62) characters — the exact C behaviour.
        let text = String::from_utf8(em.out.clone()).unwrap();
        for line in text.split('\n') {
            assert!(line.len() <= MAXPSCOLS as usize + 2, "line too long: {:?}", line);
        }
    }

    #[test]
    fn fmt_g_matches_printf() {
        assert_eq!(fmt_g(0.0), "0");
        assert_eq!(fmt_g(2.2), "2.2");
        assert_eq!(fmt_g(1.0), "1");
        assert_eq!(fmt_g(0.9642), "0.9642");
    }

    #[test]
    fn generate_csa_produces_ciebased() {
        let ctx = Context::new();
        let mut prof = load("ibm-t61.icc");
        let ps = get_postscript_csa(&ctx, &mut prof, INTENT_PERCEPTUAL, 0).expect("CSA");
        assert!(!ps.is_empty(), "CSA must be non-empty");
        let text = String::from_utf8(ps).expect("CSA is ASCII/UTF-8");
        assert!(text.starts_with("[ /CIEBased"), "CSA must start with a CIEBased header: {:?}", &text[..text.len().min(40)]);
        // ibm-t61 is an RGB matrix-shaper => /CIEBasedABC.
        assert!(text.contains("/CIEBasedABC"), "expected a matrix-shaper CSA");
        assert!(text.contains("/RenderingIntent (Perceptual)"));
    }

    #[test]
    fn generate_crd_produces_color_rendering_dictionary() {
        let ctx = Context::new();
        let mut prof = load("ibm-t61.icc");
        let ps = get_postscript_crd(&ctx, &mut prof, INTENT_PERCEPTUAL, 0).expect("CRD");
        assert!(!ps.is_empty(), "CRD must be non-empty");
        let text = String::from_utf8(ps).expect("CRD is ASCII/UTF-8");
        // Default resource definition => header comment is emitted first.
        assert!(text.starts_with("%!PS-Adobe-3.0"), "CRD must start with the PS header");
        assert!(text.contains("/ColorRenderingType 1"), "CRD must declare a ColorRendering dictionary");
        assert!(text.contains("/RenderTable "), "CRD must contain a render table");
        assert!(text.contains("/Current exch /ColorRendering defineresource pop"));
    }

    #[test]
    fn crd_no_default_resource_omits_wrapper() {
        let ctx = Context::new();
        let mut prof = load("ibm-t61.icc");
        let ps = get_postscript_crd(&ctx, &mut prof, INTENT_PERCEPTUAL, FLAGS_NODEFAULTRESOURCEDEF)
            .expect("CRD");
        let text = String::from_utf8(ps).unwrap();
        assert!(!text.starts_with("%!PS-Adobe-3.0"), "header must be suppressed");
        assert!(!text.contains("defineresource"), "resource wrapper must be suppressed");
        assert!(text.contains("/ColorRenderingType 1"));
    }
}
