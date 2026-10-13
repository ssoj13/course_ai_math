//! Text-like tag types that decode to an [`Mlu`]: `cmsSigTextType` (`text`),
//! `cmsSigTextDescriptionType` (`desc`), `cmsSigMultiLocalizedUnicodeType` (`mluc`) and
//! `cmsSigCrdInfoType` (`crdi`). Also hosts `ReadEmbeddedText`, shared with the
//! profile-sequence serializers.

use std::io::{Read, Seek, Write};

use crate::error::{Error, ErrorCode, Result};
use crate::io::endian::{read_u16, read_u32, read_u8, write_u16, write_u32, write_u8};
use crate::named::mlu::Mlu;

use super::super::handler::{
    align_long, read_bytes, read_type_base, type_sig, wrong_variant, write_bytes,
};
use super::super::value::TagValue;

/// `cmsNoLanguage` / `cmsNoCountry` — the wildcard language/country pair.
const NO_LANG: [u8; 2] = [0, 0];
const NO_COUNTRY: [u8; 2] = [0, 0];
/// `cmsV2Unicode` — the language/country key used for the V2 Unicode block of `desc`.
const V2_UNICODE: [u8; 2] = [0xFF, 0xFF];

/// `MAX_MEMORY_FOR_ALLOC` (`cmserr.c:120`, non-`CMS_LARGE_FILE_SUPPORT` build) — the ceiling C's
/// default allocator enforces on every single block (`cmserr.c:139` returns NULL above it, and the
/// caller then rejects the profile). Rust's `Vec` cannot fail gracefully: an oversized request
/// reaches `handle_alloc_error` and **aborts the process**, which no caller can catch. So any
/// pre-reservation driven by a file-supplied size is clamped to this ceiling; the real bound stays
/// the read itself, which errors on a short stream.
pub(super) use super::super::handler::MAX_ALLOC_BYTES;

// --- cmsSigTextType ---------------------------------------------------------------------

/// `Type_Text_Read` — the whole tag body is a NUL-terminated 7-bit-ASCII string.
pub(crate) fn read_text<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    let bytes = read_bytes(io, size_of_tag as usize)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let s = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let mut mlu = Mlu::new();
    mlu.set_ascii(NO_LANG, NO_COUNTRY, &s)?;
    Ok(TagValue::Mlu(mlu))
}

/// `Type_Text_Write` — the ASCII string plus its NUL terminator.
pub(crate) fn write_text<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let mlu = as_mlu(value)?;
    // Latin-1 bytes, not UTF-8: C narrows one wide unit to one byte (`cmsnamed.c:583-588`).
    let s = mlu
        .get_ascii_bytes(NO_LANG, NO_COUNTRY)
        .ok_or_else(|| Error::new(ErrorCode::NotSuitable, "text tag has no ASCII string"))?;
    write_bytes(io, &s)?;
    write_u8(io, 0)
}

// --- cmsSigTextDescriptionType ----------------------------------------------------------

/// `Type_Text_Description_Read` — ASCII block, then a best-effort V2 Unicode block, then an
/// ignored ScriptCode block. Tolerant of truncation (matches the C `goto Done` fall-through).
pub(crate) fn read_text_description<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    if size_of_tag < 4 {
        return Err(Error::new(ErrorCode::Read, "textDescription too small"));
    }
    let mut remaining = size_of_tag;

    let ascii_count = read_u32(io)?;
    if ascii_count > 0x7ffff {
        return Err(Error::new(ErrorCode::Read, "textDescription ASCII too large"));
    }
    remaining -= 4;
    if remaining < ascii_count {
        return Err(Error::new(ErrorCode::Read, "textDescription ASCII truncated"));
    }

    let ascii_bytes = read_bytes(io, ascii_count as usize)?;
    remaining -= ascii_count;
    let end = ascii_bytes.iter().position(|&b| b == 0).unwrap_or(ascii_bytes.len());
    let ascii = String::from_utf8_lossy(&ascii_bytes[..end]).into_owned();

    let mut mlu = Mlu::new();
    mlu.set_ascii(NO_LANG, NO_COUNTRY, &ascii)?;

    // From here we are tolerant to missing/short data.
    'done: {
        if remaining < 2 * 4 {
            break 'done;
        }
        let _unicode_code = read_u32(io)?;
        let unicode_count = read_u32(io)?;
        remaining -= 2 * 4;

        if unicode_count == 0 || unicode_count > 0x7ffff || remaining < unicode_count * 2 {
            break 'done;
        }
        let mut wide = Vec::with_capacity(unicode_count as usize);
        for _ in 0..unicode_count {
            wide.push(read_u16(io)?);
        }
        // `cmsMLUsetWide` stops at the first NUL, so mirror `mywcslen`.
        let wend = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        mlu.set_wide(V2_UNICODE, V2_UNICODE, &wide[..wend])?;
        remaining -= unicode_count * 2;

        // Skip the ScriptCode block if present (2 + 1 + 67 bytes).
        if remaining >= 2 + 1 + 67 {
            let _sc_code = read_u16(io)?;
            let _sc_count = read_u8(io)?;
            let _ = read_bytes(io, 67)?;
        }
    }

    Ok(TagValue::Mlu(mlu))
}

/// `Type_Text_Description_Write` — the ASCII + V2-Unicode + ScriptCode layout, zero-padded to
/// a 4-byte boundary. The `wchar` block is truncated/zero-filled to `len_text` units exactly
/// as `cmsMLUgetWide` fills its buffer.
pub(crate) fn write_text_description<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    write_text_description_object(io, as_mlu(value)?)
}

/// The `desc` body writer at object granularity, reused by `SaveDescription` (pseq/psid).
pub(crate) fn write_text_description_object<W: Write>(io: &mut W, mlu: &Mlu) -> Result<()> {
    // Latin-1 bytes, not UTF-8. C's `cmsMLUgetASCII` narrows one wide unit to exactly one byte
    // (`cmsnamed.c:583-588`), so `len_text` counts both the ASCII bytes and, reused below, the
    // wide units. Building a `String` here would encode 0x80..=0xFE as two UTF-8 bytes and
    // desynchronise the two blocks - e.g. "Caf\u{e9}" would claim 6 where lcms writes 5.
    let ascii = mlu.get_ascii_bytes(NO_LANG, NO_COUNTRY).unwrap_or_default();
    let wide = mlu.get_wide(V2_UNICODE, V2_UNICODE).unwrap_or_default();

    // `len_text` = strlen + 1 (the ASCII length including its NUL terminator).
    let len_text = ascii.len() as u32 + 1;

    // Tag-size requirement includes the 8-byte type base (already written by the caller).
    let len_tag_requirement = 8 + 4 + len_text + 4 + 4 + 2 * len_text + 2 + 1 + 67;
    let len_aligned = align_long(len_tag_requirement);

    write_u32(io, len_text)?;
    write_bytes(io, &ascii)?;
    write_u8(io, 0)?; // ASCII NUL

    write_u32(io, 0)?; // ucLanguageCode
    write_u32(io, len_text)?;
    // Write `len_text` wide units: the string, zero-filled/truncated to the buffer length.
    for i in 0..len_text as usize {
        write_u16(io, wide.get(i).copied().unwrap_or(0))?;
    }

    write_u16(io, 0)?; // scCode
    write_u8(io, 0)?; // scCount
    write_bytes(io, &[0u8; 67])?; // ScriptCode description filler

    if len_aligned > len_tag_requirement {
        let pad = (len_aligned - len_tag_requirement) as usize;
        write_bytes(io, &vec![0u8; pad])?;
    }
    Ok(())
}

// --- cmsSigMultiLocalizedUnicodeType ----------------------------------------------------

/// `Type_MLU_Read`. Works in canonical UTF-16 (`u16`) units; the C `wchar_t`↔`u16`
/// arithmetic collapses to identity here.
pub(crate) fn read_mlu<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    let count = read_u32(io)?;
    let rec_len = read_u32(io)?;
    if rec_len != 12 {
        return Err(Error::new(
            ErrorCode::UnknownExtension,
            "multiLocalizedUnicodeType of len != 12 is not supported.",
        ));
    }

    // H6/C5: hoist the loop's own bound so a hostile `Count` cannot drive the directory
    // allocation before any record has been validated. Every record needs 12 bytes, and the
    // per-record checks below (`Offset >= SizeOfHeader + 8` and `Offset + Len <= SizeOfTag + 8`,
    // `cmstypes.c:1724-1725`) already imply `12 * Count + 8 <= SizeOfTag`; asserting it up front
    // rejects exactly the same profiles, just before the allocation instead of after it.
    if count > 0 && (count as u64) * 12 + 8 > size_of_tag as u64 {
        return Err(Error::new(
            ErrorCode::CorruptionDetected,
            "MLU directory larger than the tag",
        ));
    }

    // SizeOfHeader = 12 * Count + sizeof(_cmsTagBase); bounded by the guard above.
    let size_of_header = 12 * count + 8;
    let mut largest_pos = 0u32; // in bytes

    // (language, country, begin_u16, len_u16)
    let mut dir: Vec<([u8; 2], [u8; 2], u32, u32)> =
        Vec::with_capacity((count as usize).min(MAX_ALLOC_BYTES / 12));

    for _ in 0..count {
        let language = read_u16(io)?.to_be_bytes();
        let country = read_u16(io)?.to_be_bytes();
        let len = read_u32(io)?;
        let offset = read_u32(io)?;

        // Offset must be even (it indexes UTF-16 code units).
        if offset & 1 != 0 {
            return Err(Error::new(ErrorCode::CorruptionDetected, "MLU odd offset"));
        }
        if (offset as u64) < size_of_header as u64 + 8 {
            return Err(Error::new(ErrorCode::CorruptionDetected, "MLU offset underflow"));
        }
        // M8: C relies on defined unsigned wraparound here (`(Offset + Len) < Len` is its own
        // overflow test, `cmstypes.c:1725`) and `SizeOfTag + 8` may itself wrap. Widening to u64
        // reproduces the intent without the debug-build overflow panic.
        let end = offset as u64 + len as u64;
        if end > size_of_tag as u64 + 8 {
            return Err(Error::new(ErrorCode::CorruptionDetected, "MLU string out of bounds"));
        }

        let begin_bytes = offset - size_of_header - 8;
        let end_bytes = begin_bytes + len;
        if end_bytes > largest_pos {
            largest_pos = end_bytes;
        }
        dir.push((language, country, begin_bytes / 2, len / 2));
    }

    // Read the whole string pool.
    let pool_bytes = largest_pos;
    if pool_bytes & 1 != 0 {
        return Err(Error::new(ErrorCode::CorruptionDetected, "MLU odd pool size"));
    }
    let n_u16 = (pool_bytes / 2) as usize;
    // C5: `pool_bytes` comes straight from the file, so reserve at most the C allocator's ceiling
    // and let the reads grow the vector; a truncated stream errors instead of aborting.
    let mut pool = Vec::with_capacity(n_u16.min(MAX_ALLOC_BYTES / 2));
    for _ in 0..n_u16 {
        pool.push(read_u16(io)?);
    }

    let mut mlu = Mlu::new();
    for (language, country, begin_u16, len_u16) in dir {
        let b = begin_u16 as usize;
        let l = len_u16 as usize;
        let text = pool
            .get(b..b + l)
            .ok_or_else(|| Error::new(ErrorCode::CorruptionDetected, "MLU slice out of range"))?;
        // C4: store the record's units verbatim. C copies `Len` into `Entries[i].Len`
        // (`cmstypes.c:1731`) and `Type_MLU_Write` emits it back unchanged (`:1796-1808`) — it
        // never routes read data through `cmsMLUsetWide`. Truncating at the first NUL here would
        // rewrite a `Len = 12` "Hello" record as `Len = 10` and shift every later `Offset`.
        mlu.add_raw(language, country, text.to_vec())?;
    }

    Ok(TagValue::Mlu(mlu))
}

/// `Type_MLU_Write`.
pub(crate) fn write_mlu<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let mlu = as_mlu(value)?;
    write_mlu_object(io, mlu)
}

/// Serialize an [`Mlu`] object (`Type_MLU_Write` body), reused for dictionary display strings.
pub(crate) fn write_mlu_object<W: Write>(io: &mut W, mlu: &Mlu) -> Result<()> {
    let count = mlu.translations_count();
    write_u32(io, count)?;
    write_u32(io, 12)?;

    // Gather (codes, text) in stored order and lay out a contiguous UTF-16 pool.
    let header_size = 12 * count + 8;
    let mut entries: Vec<([u8; 2], [u8; 2], Vec<u16>)> = Vec::with_capacity(count as usize);
    for i in 0..count {
        let (lang, country) = mlu
            .translations_codes(i)
            .ok_or_else(|| Error::new(ErrorCode::Internal, "MLU entry vanished"))?;
        let text = mlu.get_wide(lang, country).unwrap_or_default();
        entries.push((lang, country, text));
    }

    let mut pool_offset_u16 = 0u32;
    for (lang, country, text) in &entries {
        let len_bytes = text.len() as u32 * 2;
        let offset_bytes = pool_offset_u16 * 2 + header_size + 8;
        write_u16(io, u16::from_be_bytes(*lang))?;
        write_u16(io, u16::from_be_bytes(*country))?;
        write_u32(io, len_bytes)?;
        write_u32(io, offset_bytes)?;
        pool_offset_u16 += text.len() as u32;
    }

    for (_, _, text) in &entries {
        for &u in text {
            write_u16(io, u)?;
        }
    }
    Ok(())
}

// --- cmsSigCrdInfoType ------------------------------------------------------------------

/// The five MLU country codes of a CRD-info record (`nm`, `#0`..`#3`), language `"PS"`.
const CRD_SECTIONS: [[u8; 2]; 5] = [*b"nm", *b"#0", *b"#1", *b"#2", *b"#3"];
const CRD_LANG: [u8; 2] = *b"PS";

/// `ReadCountAndString` — a `u32` length + that many ASCII bytes (NUL included in the count).
fn read_count_and_string<R: Read + Seek>(
    io: &mut R,
    mlu: &mut Mlu,
    remaining: &mut u32,
    section: [u8; 2],
) -> Result<()> {
    if *remaining < 4 {
        return Err(Error::new(ErrorCode::Read, "crdInfo section header truncated"));
    }
    let count = read_u32(io)?;
    if count > u32::MAX - 4 || *remaining < count + 4 {
        return Err(Error::new(ErrorCode::Read, "crdInfo section too large"));
    }
    let bytes = read_bytes(io, count as usize)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let text = String::from_utf8_lossy(&bytes[..end]).into_owned();
    mlu.set_ascii(CRD_LANG, section, &text)?;
    *remaining -= count + 4;
    Ok(())
}

/// `Type_CrdInfo_Read`.
pub(crate) fn read_crd_info<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    let mut mlu = Mlu::new();
    let mut remaining = size_of_tag;
    for section in CRD_SECTIONS {
        read_count_and_string(io, &mut mlu, &mut remaining, section)?;
    }
    Ok(TagValue::CrdInfo(mlu))
}

/// `WriteCountAndString` — a `u32` length (strlen + 1) then the ASCII bytes + NUL.
fn write_count_and_string<W: Write>(io: &mut W, mlu: &Mlu, section: [u8; 2]) -> Result<()> {
    // Latin-1 bytes: the declared count must match the bytes actually written.
    let text = mlu.get_ascii_bytes(CRD_LANG, section).unwrap_or_default();
    let text_size = text.len() as u32 + 1;
    write_u32(io, text_size)?;
    write_bytes(io, &text)?;
    write_u8(io, 0)
}

/// `Type_CrdInfo_Write`.
pub(crate) fn write_crd_info<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let mlu = match value {
        TagValue::CrdInfo(m) => m,
        _ => return Err(wrong_variant("crdInfo")),
    };
    for section in CRD_SECTIONS {
        write_count_and_string(io, mlu, section)?;
    }
    Ok(())
}

// --- Shared embedded-text reader (ReadEmbeddedText, used by pseq/psid) -------------------

/// `ReadEmbeddedText` — read a type base then dispatch to the `text`/`desc`/`mluc` reader,
/// returning the decoded [`Mlu`].
pub(crate) fn read_embedded_text<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<Mlu> {
    let base = read_type_base(io)?;
    let value = if base == type_sig::TEXT {
        read_text(io, size_of_tag)?
    } else if base == type_sig::TEXT_DESCRIPTION {
        read_text_description(io, size_of_tag)?
    } else if base == type_sig::MLU {
        read_mlu(io, size_of_tag)?
    } else {
        return Err(Error::new(ErrorCode::UnknownExtension, format!("embedded text type '{base}'")));
    };
    match value {
        TagValue::Mlu(m) => Ok(m),
        _ => Err(Error::new(ErrorCode::Internal, "embedded text did not decode to MLU")),
    }
}

/// Borrow the [`Mlu`] out of a text-like [`TagValue`].
fn as_mlu(value: &TagValue) -> Result<&Mlu> {
    match value {
        TagValue::Mlu(m) => Ok(m),
        _ => Err(wrong_variant("text/MLU")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::handler::TypeHandler;
    use crate::tags::testutil::round_trip_stable;

    fn mlu_ascii(s: &str) -> Mlu {
        let mut m = Mlu::new();
        m.set_ascii(NO_LANG, NO_COUNTRY, s).unwrap();
        m
    }

    /// H4 regression: `textDescription` framing must count Latin-1 bytes, not UTF-8.
    ///
    /// C's `cmsMLUgetASCII` narrows one wide unit to exactly one byte (`cmsnamed.c:583-588`),
    /// and `Type_Text_Description_Write` reuses that count for BOTH the ASCII block and the
    /// Unicode block. Building a `String` first turns U+00E9 into two UTF-8 bytes, so the
    /// declared length grows by one, the two blocks disagree about the string length, and the
    /// tag stops matching what lcms2 emits.
    ///
    /// "Caf\u{e9}" is 4 wide units, so lcms writes len_text = 5 and bytes 43 61 66 E9 00.
    #[test]
    fn text_description_counts_latin1_bytes_not_utf8() {
        use crate::tags::testutil::round_trip_bytes;

        let mut m = Mlu::new();
        m.set_wide(NO_LANG, NO_COUNTRY, &[0x43, 0x61, 0x66, 0x00e9]).unwrap(); // "Cafe" + U+00E9

        let bytes =
            round_trip_bytes(TypeHandler::TextDescription, &TagValue::Mlu(m), 4.3);

        // [0..8) is the type base; the ASCII count is the next u32.
        let len_text = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        assert_eq!(len_text, 5, "len_text must count Latin-1 bytes + NUL, not UTF-8 bytes");
        assert_eq!(&bytes[12..17], &[0x43, 0x61, 0x66, 0xe9, 0x00]);

        // The Unicode count follows the ASCII block + ucLanguageCode, and must agree.
        let uc_len = u32::from_be_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
        assert_eq!(uc_len, len_text, "ASCII and Unicode blocks must agree on the length");
    }

    #[test]
    fn text_round_trip() {
        round_trip_stable(TypeHandler::Text, &TagValue::Mlu(mlu_ascii("Copyright 2026")));
    }

    #[test]
    fn text_description_round_trip() {
        let mut m = Mlu::new();
        m.set_ascii(NO_LANG, NO_COUNTRY, "My Profile").unwrap();
        let wide: Vec<u16> = "My Profile".encode_utf16().collect();
        m.set_wide(V2_UNICODE, V2_UNICODE, &wide).unwrap();
        round_trip_stable(TypeHandler::TextDescription, &TagValue::Mlu(m));
    }

    #[test]
    fn mluc_round_trip() {
        let mut m = Mlu::new();
        m.set_ascii(*b"en", *b"US", "Hello").unwrap();
        m.set_ascii(*b"fr", *b"FR", "Bonjour").unwrap();
        m.set_utf8(*b"ja", *b"JP", "日本語").unwrap();
        round_trip_stable(TypeHandler::Mlu, &TagValue::Mlu(m));
    }

    /// C4 regression: an `mluc` record whose declared `Len` **includes** the NUL terminator must
    /// be rewritten byte-for-byte.
    ///
    /// This layout is common in the wild. C stores `Len` verbatim (`cmstypes.c:1731`) and emits it
    /// verbatim (`:1796-1808`). Routing the read through `cmsMLUsetWide` applies `mywcslen`,
    /// dropping the NUL: the record is rewritten with `Len = 10` and a 2-byte-shorter pool, which
    /// shifts the `Offset` of every later record in the tag.
    #[test]
    fn mluc_len_including_nul_is_rewritten_byte_identical() {
        use crate::tags::testutil::{read_back, round_trip_bytes};

        // 'mluc' base | Count = 1 | RecLen = 12 | ('en','US', Len = 12, Offset = 28) | pool.
        // Offset 28 = 8 (type base) + 8 (count + reclen) + 12 (the single record).
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"mluc");
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&12u32.to_be_bytes());
        bytes.extend_from_slice(b"enUS");
        bytes.extend_from_slice(&12u32.to_be_bytes()); // Len counts the NUL
        bytes.extend_from_slice(&28u32.to_be_bytes());
        for u in [0x48u16, 0x65, 0x6c, 0x6c, 0x6f, 0x0000] {
            bytes.extend_from_slice(&u.to_be_bytes());
        }

        let value = read_back(TypeHandler::Mlu, &bytes);
        let rewritten = round_trip_bytes(TypeHandler::Mlu, &value, 4.3);
        assert_eq!(rewritten, bytes, "mluc record with Len including the NUL was not preserved");
    }

    /// H6/C5: a `Count` that cannot fit in the tag is rejected before the directory is allocated.
    ///
    /// `Count = 0xFFFFFFFF` in a 16-byte body would otherwise reserve ~64 GB, and a failed `Vec`
    /// allocation aborts the process rather than returning an error. C's `_cmsCalloc` returns NULL
    /// (`cmserr.c:139`) and the reader rejects the profile.
    #[test]
    fn mluc_count_bounded_against_tag_size() {
        use std::io::Cursor;

        let mut body: Vec<u8> = Vec::new();
        body.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // Count
        body.extend_from_slice(&12u32.to_be_bytes()); // RecLen

        let err = read_mlu(&mut Cursor::new(body.clone()), body.len() as u32).unwrap_err();
        assert_eq!(err.code, ErrorCode::CorruptionDetected);
    }

    #[test]
    fn crd_info_round_trip() {
        let mut m = Mlu::new();
        m.set_ascii(CRD_LANG, CRD_SECTIONS[0], "ProductName").unwrap();
        m.set_ascii(CRD_LANG, CRD_SECTIONS[1], "CRD0").unwrap();
        m.set_ascii(CRD_LANG, CRD_SECTIONS[2], "CRD1").unwrap();
        m.set_ascii(CRD_LANG, CRD_SECTIONS[3], "CRD2").unwrap();
        m.set_ascii(CRD_LANG, CRD_SECTIONS[4], "CRD3").unwrap();
        round_trip_stable(TypeHandler::CrdInfo, &TagValue::CrdInfo(m));
    }
}
