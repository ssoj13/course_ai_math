//! Tag-type handler registry and the shared serialization plumbing.
//!
//! lcms2 keeps a linked list of `cmsTagTypeHandler` structs, each a bundle of
//! `Read`/`Write`/`Dup`/`Free` function pointers keyed by a `cmsTagTypeSignature`
//! (`GetHandler`). `Dup`/`Free` vanish in Rust (`Clone`/`Drop`), so a handler is just a
//! reader + writer + its signature.
//!
//! # Dispatch shape (deviation from the `dyn` sketch, and why)
//! A `dyn TagTypeHandler` trait object cannot expose generic `io: &mut (impl Read + Seek)`
//! methods (object safety), and the reused [`crate::io::endian`] primitives are generic over
//! `Read`/`Write` — they cannot be called through a `&mut dyn Read`. To keep **both** goals
//! (a signature-keyed registry *and* reuse of the endian I/O), the registry is the
//! [`TypeHandler`] enum with generic `read`/`write` methods that fan out to the per-family
//! functions in [`super::types`]. This is a faithful stand-in for lcms's function-pointer
//! table: [`get_handler`] is `GetHandler`, and each arm is one `TYPE_HANDLER(...)` entry.
//!
//! Writers take `&mut (impl Write + Seek)` because the position-table types (`mAB `, `mBA `,
//! `mpet`, `psid`, `dict`, `MHC2`) seek back to patch a directory they wrote as zeros.

use std::io::{Read, Seek, SeekFrom, Write};

use crate::context::Context;
use crate::error::{Error, ErrorCode, Result};
use crate::io::endian::{read_u32, write_u32};
use crate::types::sig::Signature;

use super::types;
use super::value::TagValue;

// ---------------------------------------------------------------------------------------
// Tag-type signatures (cmsTagTypeSignature, lcms2.h) — built from their 4CC bytes.
// ---------------------------------------------------------------------------------------

/// Tag-*type* signatures used by the built-in handlers.
pub mod type_sig {
    use crate::types::sig::Signature;

    pub const CHROMATICITY: Signature = Signature::new(b"chrm");
    pub const COLORANT_ORDER: Signature = Signature::new(b"clro");
    pub const S15FIXED16_ARRAY: Signature = Signature::new(b"sf32");
    pub const U16FIXED16_ARRAY: Signature = Signature::new(b"uf32");
    pub const TEXT: Signature = Signature::new(b"text");
    pub const TEXT_DESCRIPTION: Signature = Signature::new(b"desc");
    pub const CURVE: Signature = Signature::new(b"curv");
    pub const PARAMETRIC_CURVE: Signature = Signature::new(b"para");
    pub const DATE_TIME: Signature = Signature::new(b"dtim");
    pub const LUT8: Signature = Signature::new(b"mft1");
    pub const LUT16: Signature = Signature::new(b"mft2");
    pub const LUT_A2B: Signature = Signature::new(b"mAB ");
    pub const LUT_B2A: Signature = Signature::new(b"mBA ");
    pub const MEASUREMENT: Signature = Signature::new(b"meas");
    pub const MLU: Signature = Signature::new(b"mluc");
    pub const MPE: Signature = Signature::new(b"mpet");
    pub const NAMED_COLOR2: Signature = Signature::new(b"ncl2");
    pub const COLORANT_TABLE: Signature = Signature::new(b"clrt");
    pub const PROFILE_SEQUENCE_DESC: Signature = Signature::new(b"pseq");
    pub const PROFILE_SEQUENCE_ID: Signature = Signature::new(b"psid");
    pub const SIGNATURE: Signature = Signature::new(b"sig ");
    pub const DATA: Signature = Signature::new(b"data");
    pub const UCR_BG: Signature = Signature::new(b"bfd ");
    pub const CRD_INFO: Signature = Signature::new(b"crdi");
    pub const SCREENING: Signature = Signature::new(b"scrn");
    pub const VIEWING_CONDITIONS: Signature = Signature::new(b"view");
    pub const XYZ: Signature = Signature::new(b"XYZ ");
    pub const DICT: Signature = Signature::new(b"dict");
    pub const CICP: Signature = Signature::new(b"cicp");
    pub const VCGT: Signature = Signature::new(b"vcgt");
    pub const MHC2: Signature = Signature::new(b"MHC2");
    pub const UINT8_ARRAY: Signature = Signature::new(b"ui08");
    pub const UINT16_ARRAY: Signature = Signature::new(b"ui16");
    pub const UINT32_ARRAY: Signature = Signature::new(b"ui32");
    pub const UINT64_ARRAY: Signature = Signature::new(b"ui64");

    /// `cmsCorbisBrokenXYZtype` — an out-of-spec XYZ type sig read as ordinary XYZ.
    pub const CORBIS_BROKEN_XYZ: Signature = Signature(0x17A5_05B8);
    /// `cmsMonacoBrokenCurveType` — an out-of-spec curve type sig read as ordinary `curv`.
    pub const MONACO_BROKEN_CURVE: Signature = Signature(0x9478_ee00);
}

/// MPE element type signatures (`cmsStageSignature`) and curve-segment signatures.
pub mod mpe_sig {
    use crate::types::sig::Signature;

    pub const CURVE_SET: Signature = Signature::new(b"cvst");
    pub const MATRIX: Signature = Signature::new(b"matf");
    pub const CLUT: Signature = Signature::new(b"clut");
    pub const B_ACS: Signature = Signature::new(b"bACS");
    pub const E_ACS: Signature = Signature::new(b"eACS");

    pub const SEGMENTED_CURVE: Signature = Signature::new(b"curf");
    pub const FORMULA_CURVE_SEG: Signature = Signature::new(b"parf");
    pub const SAMPLED_CURVE_SEG: Signature = Signature::new(b"samf");
}

// ---------------------------------------------------------------------------------------
// Type-base + alignment primitives (cmsplugin.c: _cmsRead/WriteTypeBase, _cmsRead/WriteAlignment)
// ---------------------------------------------------------------------------------------

/// `_cmsALIGNLONG` — round `x` up to the next 4-byte boundary.
#[inline]
pub(crate) const fn align_long(x: u32) -> u32 {
    (x + 3) & !3
}

/// `io->Tell(io)` — current absolute stream position as a `u32`.
#[inline]
pub(crate) fn tell<S: Seek + ?Sized>(io: &mut S) -> Result<u32> {
    let p = io
        .stream_position()
        .map_err(|e| Error::new(ErrorCode::Seek, format!("tell failed: {e}")))?;
    Ok(p as u32)
}

/// `io->Seek(io, pos)` — seek to an absolute stream position.
#[inline]
pub(crate) fn seek_to<S: Seek + ?Sized>(io: &mut S, pos: u32) -> Result<()> {
    io.seek(SeekFrom::Start(pos as u64))
        .map(|_| ())
        .map_err(|e| Error::new(ErrorCode::Seek, format!("seek failed: {e}")))
}

/// Add a tag-internal offset to a tag's `base` offset, rejecting an out-of-range sum.
///
/// C computes these sums in `cmsUInt32Number` and leans on defined unsigned wraparound: a garbage
/// file-supplied offset wraps to some small position, the following `io->Seek` fails, and the
/// reader returns NULL (e.g. `cmstypes.c:5892`,
/// `ReadDoublesAt(io, BaseOffset + OffsetRedTable + 8, ...)`). Rust would instead panic on overflow
/// in an overflow-checked build, so the wrap is made explicit here and surfaces as the same
/// `ErrorCode::Seek` failure that C's control flow already assumes.
#[inline]
pub(crate) fn rel_offset(base: u32, off: u32) -> Result<u32> {
    base.checked_add(off).ok_or_else(|| {
        Error::new(
            ErrorCode::Seek,
            format!("tag offset {off} overflows base offset {base}"),
        )
    })
}

/// `_cmsReadTypeBase` — read the 8-byte tag base (4-byte type sig + 4 reserved), returning
/// the type signature.
pub(crate) fn read_type_base<R: Read + Seek>(io: &mut R) -> Result<Signature> {
    let sig = read_u32(io)?;
    let _reserved = read_u32(io)?;
    Ok(Signature(sig))
}

/// `_cmsWriteTypeBase` — write the 8-byte tag base for `sig` (reserved zeroed).
pub(crate) fn write_type_base<W: Write>(io: &mut W, sig: Signature) -> Result<()> {
    write_u32(io, sig.as_u32())?;
    write_u32(io, 0)
}

/// `_cmsReadAlignment` — advance the read cursor to the next 4-byte boundary.
pub(crate) fn read_alignment<R: Read + Seek>(io: &mut R) -> Result<()> {
    let at = tell(io)?;
    let next = align_long(at);
    let n = next - at;
    if n == 0 {
        return Ok(());
    }
    if n > 4 {
        return Err(Error::new(ErrorCode::Read, "alignment gap > 4 bytes"));
    }
    let mut buf = [0u8; 4];
    io.read_exact(&mut buf[..n as usize])
        .map_err(|e| Error::new(ErrorCode::Read, format!("alignment read: {e}")))
}

/// `_cmsWriteAlignment` — pad the write cursor with zeros to the next 4-byte boundary.
pub(crate) fn write_alignment<W: Write + Seek>(io: &mut W) -> Result<()> {
    let at = tell(io)?;
    let next = align_long(at);
    let n = next - at;
    if n == 0 {
        return Ok(());
    }
    if n > 4 {
        return Err(Error::new(ErrorCode::Write, "alignment gap > 4 bytes"));
    }
    let buf = [0u8; 4];
    io.write_all(&buf[..n as usize])
        .map_err(|e| Error::new(ErrorCode::Write, format!("alignment write: {e}")))
}

/// `MAX_MEMORY_FOR_ALLOC` (`cmserr.c:118-121`) — the ceiling C's `_cmsCalloc` refuses to exceed.
///
/// Anything sized from a file-supplied length is bounded by this. C returns NULL above it and
/// the profile is cleanly rejected (`cmserr.c:139`); Rust's `vec![_; n]` would instead call
/// `handle_alloc_error`, which ABORTS the process and cannot be caught.
///
/// This is the non-`CMS_LARGE_FILE_SUPPORT` value, matching how the reference is built here.
pub(crate) const MAX_ALLOC_BYTES: usize = 512 * 1024 * 1024;

/// Read exactly `n` raw bytes (`io->Read(io, buf, 1, n)`), erroring on a short read.
///
/// `n` is file-supplied at every call site, so the ceiling is enforced HERE rather than being
/// re-derived by each caller - one place to get right, and no caller can forget it. The
/// allocation itself goes through `try_reserve`, so even under the ceiling an allocation failure
/// surfaces as an error instead of aborting the process.
pub(crate) fn read_bytes<R: Read>(io: &mut R, n: usize) -> Result<Vec<u8>> {
    if n > MAX_ALLOC_BYTES {
        return Err(Error::new(
            ErrorCode::Read,
            format!("refusing a {n}-byte read above the {MAX_ALLOC_BYTES}-byte allocation ceiling"),
        ));
    }
    let mut buf = Vec::new();
    buf.try_reserve_exact(n)
        .map_err(|_| Error::new(ErrorCode::Read, format!("cannot allocate {n} bytes")))?;
    buf.resize(n, 0);
    io.read_exact(&mut buf)
        .map_err(|e| Error::new(ErrorCode::Read, format!("short read of {n} bytes: {e}")))?;
    Ok(buf)
}

/// Write raw bytes (`io->Write(io, n, buf)`).
pub(crate) fn write_bytes<W: Write>(io: &mut W, buf: &[u8]) -> Result<()> {
    io.write_all(buf)
        .map_err(|e| Error::new(ErrorCode::Write, format!("write failed: {e}")))
}

/// Total stream length in bytes (`io->ReportedSize`), leaving the cursor unmoved.
pub(crate) fn stream_len<S: Seek + ?Sized>(io: &mut S) -> Result<u32> {
    let cur = io
        .stream_position()
        .map_err(|e| Error::new(ErrorCode::Seek, format!("tell failed: {e}")))?;
    let end = io
        .seek(SeekFrom::End(0))
        .map_err(|e| Error::new(ErrorCode::Seek, format!("seek end failed: {e}")))?;
    io.seek(SeekFrom::Start(cur))
        .map_err(|e| Error::new(ErrorCode::Seek, format!("seek restore failed: {e}")))?;
    Ok(end as u32)
}

/// `ReadPositionTable` (ICC 4.3 position tables) — read `count` (offset, size) records (each
/// offset biased by `base_offset`), then seek to each element and run `element_fn`.
pub(crate) fn read_position_table<R, F>(
    io: &mut R,
    count: u32,
    base_offset: u32,
    mut element_fn: F,
) -> Result<()>
where
    R: Read + Seek,
    F: FnMut(&mut R, u32, u32) -> Result<()>,
{
    let current = tell(io)?;
    let reported = stream_len(io)?;
    // Guard against a record count that cannot fit two u32s each in the remaining stream.
    if reported.saturating_sub(current) / (2 * 4) < count {
        return Err(Error::new(ErrorCode::Read, "position table larger than tag"));
    }

    let mut offsets = Vec::with_capacity(count as usize);
    let mut sizes = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let off = read_u32(io)?;
        let sz = read_u32(io)?;
        offsets.push(off.wrapping_add(base_offset));
        sizes.push(sz);
    }

    for i in 0..count as usize {
        seek_to(io, offsets[i])?;
        element_fn(io, i as u32, sizes[i])?;
    }
    Ok(())
}

/// `WritePositionTable` — write a placeholder directory, each element via `element_fn`, then
/// backpatch the (offset, size) records. Offsets are relative to `base_offset`.
pub(crate) fn write_position_table<W, F>(
    io: &mut W,
    size_of_tag: u32,
    count: u32,
    base_offset: u32,
    mut element_fn: F,
) -> Result<()>
where
    W: Write + Seek,
    F: FnMut(&mut W, u32, u32) -> Result<()>,
{
    let mut offsets = vec![0u32; count as usize];
    let mut sizes = vec![0u32; count as usize];

    let directory_pos = tell(io)?;
    for _ in 0..count {
        write_u32(io, 0)?; // Offset
        write_u32(io, 0)?; // size
    }

    for i in 0..count as usize {
        let before = tell(io)?;
        offsets[i] = before - base_offset;
        element_fn(io, i as u32, size_of_tag)?;
        sizes[i] = tell(io)? - before;
    }

    let current = tell(io)?;
    seek_to(io, directory_pos)?;
    for i in 0..count as usize {
        write_u32(io, offsets[i])?;
        write_u32(io, sizes[i])?;
    }
    seek_to(io, current)
}

/// `FROM_8_TO_16(rgb)` (`lcms2_internal.h`) — replicate the low byte into the high byte.
#[inline]
pub(crate) const fn from_8_to_16(rgb: u8) -> u16 {
    ((rgb as u16) << 8) | rgb as u16
}

/// `FROM_16_TO_8(rgb)` (`lcms2_internal.h`) — round a 16-bit value down to 8 bits.
#[inline]
pub(crate) const fn from_16_to_8(rgb: u16) -> u8 {
    (((rgb as u32 * 65281 + 8_388_608) >> 24) & 0xFF) as u8
}

// ---------------------------------------------------------------------------------------
// The handler registry (GetHandler + the SupportedTagTypes table)
// ---------------------------------------------------------------------------------------

/// One built-in tag-type handler (`cmsTagTypeHandler`), identified by the ICC type it (de)serializes.
///
/// The enum arms are exactly the 34 distinct handlers of the `SupportedTagTypes` table in
/// `cmstypes.c` (the two "broken" entries reuse [`TypeHandler::Xyz`] / [`TypeHandler::Curve`]).
#[derive(Debug, Clone, Copy)]
pub enum TypeHandler {
    Xyz,
    Chromaticity,
    ColorantOrder,
    S15Fixed16,
    U16Fixed16,
    UInt8,
    UInt32,
    UInt64,
    Signature,
    Text,
    TextDescription,
    Mlu,
    Curve,
    ParametricCurve,
    DateTime,
    Data,
    Measurement,
    Lut8,
    Lut16,
    LutA2B,
    LutB2A,
    ColorantTable,
    NamedColor,
    ProfileSequenceDesc,
    ProfileSequenceId,
    UcrBg,
    CrdInfo,
    Mpe,
    Screening,
    ViewingConditions,
    Dict,
    Cicp,
    Vcgt,
    Mhc2,
    /// A plugin-registered tag type (`_cmsRegisterTagTypePlugin`), carrying its own
    /// signature and dynamic read/write functions.
    Plugin(crate::plugin::TagTypePlugin),
}

impl TypeHandler {
    /// `Handler.ReadPtr` — decode a value from `io` (already positioned past the type base).
    pub fn read<R: Read + Seek>(&self, io: &mut R, size_of_tag: u32) -> Result<TagValue> {
        self.read_for(None, io, size_of_tag)
    }

    /// [`Self::read`] resolving nested plugin lookups against `ctx`.
    ///
    /// Only the `mpet` handler consults the registry while decoding (`ReadMPEElem` walks the
    /// per-context MPE list), so this differs from [`Self::read`] for that variant alone.
    pub fn read_thr<R: Read + Seek>(
        &self,
        ctx: &Context,
        io: &mut R,
        size_of_tag: u32,
    ) -> Result<TagValue> {
        self.read_for(Some(ctx), io, size_of_tag)
    }

    /// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
    fn read_for<R: Read + Seek>(
        &self,
        ctx: Option<&Context>,
        io: &mut R,
        size_of_tag: u32,
    ) -> Result<TagValue> {
        use TypeHandler::*;
        match self {
            Xyz => types::xyz::read(io),
            Chromaticity => types::misc::read_chromaticity(io, size_of_tag),
            ColorantOrder => types::misc::read_colorant_order(io),
            S15Fixed16 => types::misc::read_s15fixed16(io, size_of_tag),
            U16Fixed16 => types::misc::read_u16fixed16(io, size_of_tag),
            UInt8 => types::misc::read_uint8(io, size_of_tag),
            UInt32 => types::misc::read_uint32(io, size_of_tag),
            UInt64 => types::misc::read_uint64(io, size_of_tag),
            Signature => types::misc::read_signature(io),
            Text => types::mlu::read_text(io, size_of_tag),
            TextDescription => types::mlu::read_text_description(io, size_of_tag),
            Mlu => types::mlu::read_mlu(io, size_of_tag),
            Curve => types::curve::read_curve(io),
            ParametricCurve => types::curve::read_parametric(io),
            DateTime => types::misc::read_datetime(io),
            Data => types::misc::read_data(io, size_of_tag),
            Measurement => types::misc::read_measurement(io),
            Lut8 => types::lut::read_lut8(io),
            Lut16 => types::lut::read_lut16(io),
            LutA2B => types::lut::read_lut_a2b(io),
            LutB2A => types::lut::read_lut_b2a(io),
            ColorantTable => types::namedcolor::read_colorant_table(io),
            NamedColor => types::namedcolor::read_named_color(io),
            ProfileSequenceDesc => types::seq::read_profile_sequence_desc(io, size_of_tag),
            ProfileSequenceId => types::seq::read_profile_sequence_id(io),
            UcrBg => types::misc::read_ucr_bg(io, size_of_tag),
            CrdInfo => types::mlu::read_crd_info(io, size_of_tag),
            Mpe => types::mpe::read_mpe(ctx, io),
            Screening => types::misc::read_screening(io),
            ViewingConditions => types::misc::read_viewing_conditions(io),
            Dict => types::dict::read_dictionary(io, size_of_tag),
            Cicp => types::misc::read_cicp(io, size_of_tag),
            Vcgt => types::misc::read_vcgt(io, size_of_tag),
            Mhc2 => types::misc::read_mhc2(io),
            Plugin(p) => (p.read)(&mut *io, size_of_tag),
        }
    }

    /// `Handler.WritePtr` — encode `value` to `io` (the caller writes the type base first).
    ///
    /// `icc_version` mirrors `self->ICCVersion`: only the profile-sequence writers consult it
    /// (via `SaveDescription`, to pick `desc` for V2 vs `mluc` for V4 embedded text); every
    /// other handler ignores it.
    pub fn write<W: Write + Seek>(&self, io: &mut W, value: &TagValue, icc_version: f64) -> Result<()> {
        use TypeHandler::*;
        match self {
            Xyz => types::xyz::write(io, value),
            Chromaticity => types::misc::write_chromaticity(io, value),
            ColorantOrder => types::misc::write_colorant_order(io, value),
            S15Fixed16 => types::misc::write_s15fixed16(io, value),
            U16Fixed16 => types::misc::write_u16fixed16(io, value),
            UInt8 => types::misc::write_uint8(io, value),
            UInt32 => types::misc::write_uint32(io, value),
            UInt64 => types::misc::write_uint64(io, value),
            Signature => types::misc::write_signature(io, value),
            Text => types::mlu::write_text(io, value),
            TextDescription => types::mlu::write_text_description(io, value),
            Mlu => types::mlu::write_mlu(io, value),
            Curve => types::curve::write_curve(io, value),
            ParametricCurve => types::curve::write_parametric(io, value),
            DateTime => types::misc::write_datetime(io, value),
            Data => types::misc::write_data(io, value),
            Measurement => types::misc::write_measurement(io, value),
            Lut8 => types::lut::write_lut8(io, value),
            Lut16 => types::lut::write_lut16(io, value),
            LutA2B => types::lut::write_lut_a2b(io, value),
            LutB2A => types::lut::write_lut_b2a(io, value),
            ColorantTable => types::namedcolor::write_colorant_table(io, value),
            NamedColor => types::namedcolor::write_named_color(io, value),
            ProfileSequenceDesc => types::seq::write_profile_sequence_desc(io, value, icc_version),
            ProfileSequenceId => types::seq::write_profile_sequence_id(io, value, icc_version),
            UcrBg => types::misc::write_ucr_bg(io, value),
            CrdInfo => types::mlu::write_crd_info(io, value),
            Mpe => types::mpe::write_mpe(io, value),
            Screening => types::misc::write_screening(io, value),
            ViewingConditions => types::misc::write_viewing_conditions(io, value),
            Dict => types::dict::write_dictionary(io, value),
            Cicp => types::misc::write_cicp(io, value),
            Vcgt => types::misc::write_vcgt(io, value),
            Mhc2 => types::misc::write_mhc2(io, value),
            Plugin(p) => (p.write)(&mut *io, value),
        }
    }

    /// `Handler.Signature` — the canonical ICC type signature this handler serializes.
    ///
    /// The two broken aliases report their *canonical* type (`XYZ ` / `curv`), matching the
    /// C handlers which reuse the real reader/writer.
    pub fn signature(&self) -> Signature {
        use type_sig as t;
        use TypeHandler::*;
        match self {
            Xyz => t::XYZ,
            Chromaticity => t::CHROMATICITY,
            ColorantOrder => t::COLORANT_ORDER,
            S15Fixed16 => t::S15FIXED16_ARRAY,
            U16Fixed16 => t::U16FIXED16_ARRAY,
            UInt8 => t::UINT8_ARRAY,
            UInt32 => t::UINT32_ARRAY,
            UInt64 => t::UINT64_ARRAY,
            Signature => t::SIGNATURE,
            Text => t::TEXT,
            TextDescription => t::TEXT_DESCRIPTION,
            Mlu => t::MLU,
            Curve => t::CURVE,
            ParametricCurve => t::PARAMETRIC_CURVE,
            DateTime => t::DATE_TIME,
            Data => t::DATA,
            Measurement => t::MEASUREMENT,
            Lut8 => t::LUT8,
            Lut16 => t::LUT16,
            LutA2B => t::LUT_A2B,
            LutB2A => t::LUT_B2A,
            ColorantTable => t::COLORANT_TABLE,
            NamedColor => t::NAMED_COLOR2,
            ProfileSequenceDesc => t::PROFILE_SEQUENCE_DESC,
            ProfileSequenceId => t::PROFILE_SEQUENCE_ID,
            UcrBg => t::UCR_BG,
            CrdInfo => t::CRD_INFO,
            Mpe => t::MPE,
            Screening => t::SCREENING,
            ViewingConditions => t::VIEWING_CONDITIONS,
            Dict => t::DICT,
            Cicp => t::CICP,
            Vcgt => t::VCGT,
            Mhc2 => t::MHC2,
            Plugin(p) => p.sig,
        }
    }
}

/// `GetHandler` / `_cmsGetTagTypeHandler` — the handler for a type signature.
///
/// Plugin-registered tag types (`_cmsRegisterTagTypePlugin`) are searched first — newest
/// first — then the built-in `SupportedTagTypes` (including the two broken aliases), matching
/// lcms's walk of the context type list ahead of the stock table.
pub fn get_handler(sig: Signature) -> Option<TypeHandler> {
    get_handler_for(None, sig)
}

/// [`get_handler`] against the tag-type plugins visible to `ctx`.
///
/// C's `_cmsGetTagTypeHandler(ContextID, sig)` takes the context because the type list it walks
/// is a per-context chunk; the plain form is the `ContextID == NULL` call.
pub fn get_handler_thr(ctx: &Context, sig: Signature) -> Option<TypeHandler> {
    get_handler_for(Some(ctx), sig)
}

/// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
fn get_handler_for(ctx: Option<&Context>, sig: Signature) -> Option<TypeHandler> {
    use type_sig as t;

    if let Some(p) = crate::plugin::find_tag_type(ctx, sig) {
        return Some(TypeHandler::Plugin(p));
    }
    let h = match sig {
        s if s == t::XYZ => TypeHandler::Xyz,
        s if s == t::CORBIS_BROKEN_XYZ => TypeHandler::Xyz,
        s if s == t::CHROMATICITY => TypeHandler::Chromaticity,
        s if s == t::COLORANT_ORDER => TypeHandler::ColorantOrder,
        s if s == t::S15FIXED16_ARRAY => TypeHandler::S15Fixed16,
        s if s == t::U16FIXED16_ARRAY => TypeHandler::U16Fixed16,
        s if s == t::UINT8_ARRAY => TypeHandler::UInt8,
        s if s == t::UINT32_ARRAY => TypeHandler::UInt32,
        s if s == t::UINT64_ARRAY => TypeHandler::UInt64,
        s if s == t::SIGNATURE => TypeHandler::Signature,
        s if s == t::TEXT => TypeHandler::Text,
        s if s == t::TEXT_DESCRIPTION => TypeHandler::TextDescription,
        s if s == t::MLU => TypeHandler::Mlu,
        s if s == t::CURVE => TypeHandler::Curve,
        s if s == t::MONACO_BROKEN_CURVE => TypeHandler::Curve,
        s if s == t::PARAMETRIC_CURVE => TypeHandler::ParametricCurve,
        s if s == t::DATE_TIME => TypeHandler::DateTime,
        s if s == t::DATA => TypeHandler::Data,
        s if s == t::MEASUREMENT => TypeHandler::Measurement,
        s if s == t::LUT8 => TypeHandler::Lut8,
        s if s == t::LUT16 => TypeHandler::Lut16,
        s if s == t::LUT_A2B => TypeHandler::LutA2B,
        s if s == t::LUT_B2A => TypeHandler::LutB2A,
        s if s == t::COLORANT_TABLE => TypeHandler::ColorantTable,
        s if s == t::NAMED_COLOR2 => TypeHandler::NamedColor,
        s if s == t::PROFILE_SEQUENCE_DESC => TypeHandler::ProfileSequenceDesc,
        s if s == t::PROFILE_SEQUENCE_ID => TypeHandler::ProfileSequenceId,
        s if s == t::UCR_BG => TypeHandler::UcrBg,
        s if s == t::CRD_INFO => TypeHandler::CrdInfo,
        s if s == t::MPE => TypeHandler::Mpe,
        s if s == t::SCREENING => TypeHandler::Screening,
        s if s == t::VIEWING_CONDITIONS => TypeHandler::ViewingConditions,
        s if s == t::DICT => TypeHandler::Dict,
        s if s == t::CICP => TypeHandler::Cicp,
        s if s == t::VCGT => TypeHandler::Vcgt,
        s if s == t::MHC2 => TypeHandler::Mhc2,
        _ => return None,
    };
    Some(h)
}

// ---------------------------------------------------------------------------------------
// Small shared error helper
// ---------------------------------------------------------------------------------------

/// The `cmsERROR_NOT_SUITABLE` a handler raises when handed the wrong [`TagValue`] variant.
pub(crate) fn wrong_variant(expected: &str) -> Error {
    Error::new(ErrorCode::NotSuitable, format!("tag value is not a {expected}"))
}

#[cfg(test)]
mod tests {
    use super::{read_bytes, MAX_ALLOC_BYTES};
    use crate::error::ErrorCode;

    /// An absurd file-supplied length must be refused BEFORE the allocation, not discovered
    /// afterwards by a short read. C caps at `MAX_MEMORY_FOR_ALLOC` inside `_cmsCalloc`
    /// (`cmserr.c:139`) and returns NULL; `vec![0u8; n]` would instead abort the process, which
    /// no caller can catch. Enforced here so no tag reader has to remember to do it.
    #[test]
    fn read_bytes_refuses_an_absurd_length_before_allocating() {
        let mut io = std::io::Cursor::new(vec![0u8; 16]);
        let err = read_bytes(&mut io, MAX_ALLOC_BYTES + 1).expect_err("must refuse");
        assert_eq!(err.code, ErrorCode::Read);
        assert!(
            err.message.contains("allocation ceiling"),
            "must fail up front, not as a short read: {}",
            err.message
        );
        assert_eq!(io.position(), 0, "nothing may be consumed from the stream");

        // A sane read still works, and a short one still reports a short read.
        assert_eq!(read_bytes(&mut io, 4).expect("short read ok").len(), 4);
        let err = read_bytes(&mut io, 999).expect_err("past the end");
        assert!(err.message.contains("short read"), "{}", err.message);
    }
}
