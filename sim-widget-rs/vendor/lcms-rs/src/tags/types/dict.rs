//! `cmsSigDictType` (`dict`) — a metadata dictionary of name/value pairs with optional
//! localized display strings (`Type_Dictionary_*` in `cmstypes.c`).
//!
//! Name/Value are UTF-16 strings; DisplayName/DisplayValue are embedded `mluc` objects.
//! Column presence is encoded by the record `Length`: 16 (name+value), 24 (+displayName),
//! 32 (+displayValue).

use std::io::{Read, Seek, Write};

use crate::error::{Error, ErrorCode, Result};
use crate::io::endian::{read_u16, read_u32, write_u32};
use crate::named::dict::Dict;
use crate::named::mlu::Mlu;

use super::super::handler::{seek_to, tell, wrong_variant};
use super::super::value::TagValue;
use super::mlu::{read_mlu, write_mlu_object};

/// One directory column: parallel `(offset, size)` records, one per entry.
#[derive(Default)]
struct Column {
    off: Vec<u32>,
    size: Vec<u32>,
}

impl Column {
    fn new(count: usize) -> Self {
        Column { off: vec![0; count], size: vec![0; count] }
    }

    /// The `(offset, size)` pair for entry `i`.
    ///
    /// A column the record `Length` excludes is never allocated (C skips `AllocElem` for it,
    /// `cmstypes.c:5225-5231`) and reads as the ICC "undefined string" pair `(0, 0)` — the same
    /// value C's `memset`-zeroed `_cmsDICarray` yields.
    fn get(&self, i: usize) -> (u32, u32) {
        (
            self.off.get(i).copied().unwrap_or(0),
            self.size.get(i).copied().unwrap_or(0),
        )
    }
}

// --- Read -------------------------------------------------------------------------------

/// `ReadOneElem` — an `(offset, size)` record; a non-zero offset is biased by `BaseOffset`.
fn read_one_elem<R: Read + Seek>(io: &mut R, col: &mut Column, i: usize, base_offset: u32) -> Result<()> {
    let off = read_u32(io)?;
    let size = read_u32(io)?;
    col.off[i] = if off > 0 { off.wrapping_add(base_offset) } else { 0 };
    col.size[i] = size;
    Ok(())
}

/// `ReadOneWChar` — a UTF-16 string at `(off, size)`, or `None` for the special offset 0.
fn read_one_wchar<R: Read + Seek>(io: &mut R, off: u32, size: u32) -> Result<Option<String>> {
    if off == 0 {
        return Ok(None);
    }
    seek_to(io, off)?;
    let n_chars = size / 2;
    if n_chars > 0x7ffff {
        return Err(Error::new(ErrorCode::Range, "dictionary string too long"));
    }
    let mut wide = Vec::with_capacity(n_chars as usize);
    for _ in 0..n_chars {
        wide.push(read_u16(io)?);
    }
    Ok(Some(String::from_utf16_lossy(&wide)))
}

/// `ReadOneMLUC` — an embedded `mluc` at `(off, size)`, or `None` when either is 0.
fn read_one_mluc<R: Read + Seek>(io: &mut R, off: u32, size: u32) -> Result<Option<Mlu>> {
    if off == 0 || size == 0 {
        return Ok(None);
    }
    seek_to(io, off)?;
    match read_mlu(io, size)? {
        TagValue::Mlu(m) => Ok(Some(m)),
        _ => Err(Error::new(ErrorCode::Internal, "dictionary MLU decode failed")),
    }
}

/// `Type_Dictionary_Read`.
pub(crate) fn read_dictionary<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    let base_offset = tell(io)?.wrapping_sub(8);
    let mut signed = size_of_tag as i64;

    signed -= 4;
    if signed < 0 {
        return Err(Error::new(ErrorCode::Read, "dict header truncated"));
    }
    let count = read_u32(io)?;

    signed -= 4;
    if signed < 0 {
        return Err(Error::new(ErrorCode::Read, "dict header truncated"));
    }
    let length = read_u32(io)?;

    if length != 16 && length != 24 && length != 32 {
        return Err(Error::new(
            ErrorCode::UnknownExtension,
            format!("Unknown record length in dictionary '{length}'"),
        ));
    }

    // H6/C5: hoist the read loop's own guard. Every record consumes exactly `length` bytes of the
    // remaining tag, so a `Count` that does not fit is rejected here — before eight vectors are
    // sized straight from the file. C survives this because `_cmsCalloc` refuses blocks over
    // `MAX_MEMORY_FOR_ALLOC` and returns NULL (`cmserr.c:139`); a failed Rust `Vec` allocation
    // aborts the process instead, which no caller can catch. Same profiles rejected, one step
    // earlier.
    if (count as u64) * (length as u64) > signed as u64 {
        return Err(Error::new(ErrorCode::Read, "dict offset array truncated"));
    }

    let n = count as usize;
    let mut name = Column::new(n);
    let mut value = Column::new(n);
    // Only the columns the record `Length` declares exist (`AllocArray`, `cmstypes.c:5225-5231`).
    let mut display_name = if length > 16 { Column::new(n) } else { Column::default() };
    let mut display_value = if length > 24 { Column::new(n) } else { Column::default() };

    // ReadOffsetArray, with the running signed-size guard.
    for i in 0..n {
        if signed < 4 * 4 {
            return Err(Error::new(ErrorCode::Read, "dict offset array truncated"));
        }
        signed -= 4 * 4;
        read_one_elem(io, &mut name, i, base_offset)?;
        read_one_elem(io, &mut value, i, base_offset)?;

        if length > 16 {
            if signed < 2 * 4 {
                return Err(Error::new(ErrorCode::Read, "dict displayName column truncated"));
            }
            signed -= 2 * 4;
            read_one_elem(io, &mut display_name, i, base_offset)?;
        }
        if length > 24 {
            if signed < 2 * 4 {
                return Err(Error::new(ErrorCode::Read, "dict displayValue column truncated"));
            }
            signed -= 2 * 4;
            read_one_elem(io, &mut display_value, i, base_offset)?;
        }
    }

    let mut dict = Dict::new();
    for i in 0..n {
        let (name_off, name_size) = name.get(i);
        let (value_off, value_size) = value.get(i);
        let name_wcs = read_one_wchar(io, name_off, name_size)?;
        let value_wcs = read_one_wchar(io, value_off, value_size)?;
        let dname = if length > 16 {
            let (off, size) = display_name.get(i);
            read_one_mluc(io, off, size)?
        } else {
            None
        };
        let dvalue = if length > 24 {
            let (off, size) = display_value.get(i);
            read_one_mluc(io, off, size)?
        } else {
            None
        };

        let name_wcs = name_wcs.ok_or_else(|| {
            Error::new(ErrorCode::CorruptionDetected, "Bad dictionary Name/Value")
        })?;
        // Value must be present (a NULL value is a corrupt record, per the C check).
        if value_wcs.is_none() {
            return Err(Error::new(ErrorCode::CorruptionDetected, "Bad dictionary Name/Value"));
        }
        dict.add_entry(&name_wcs, value_wcs.as_deref(), dname, dvalue);
    }

    Ok(TagValue::Dict(dict))
}

// --- Write ------------------------------------------------------------------------------

/// `WriteOneElem` — write an `(offset, size)` record.
fn write_one_elem<W: Write>(io: &mut W, col: &Column, i: usize) -> Result<()> {
    write_u32(io, col.off[i])?;
    write_u32(io, col.size[i])
}

/// `WriteOffsetArray` — the directory of `(offset, size)` records for all present columns.
fn write_offset_array<W: Write>(
    io: &mut W,
    name: &Column,
    value: &Column,
    display_name: &Column,
    display_value: &Column,
    count: usize,
    length: u32,
) -> Result<()> {
    for i in 0..count {
        write_one_elem(io, name, i)?;
        write_one_elem(io, value, i)?;
        if length > 16 {
            write_one_elem(io, display_name, i)?;
        }
        if length > 24 {
            write_one_elem(io, display_value, i)?;
        }
    }
    Ok(())
}

/// `WriteOneWChar` — a UTF-16 string, recording its offset/size (offset 0 for a `None` string).
fn write_one_wchar<W: Write + Seek>(
    io: &mut W,
    col: &mut Column,
    i: usize,
    wcstr: Option<&str>,
    base_offset: u32,
) -> Result<()> {
    let before = tell(io)?;
    col.off[i] = before - base_offset;
    let s = match wcstr {
        None => {
            col.size[i] = 0;
            col.off[i] = 0;
            return Ok(());
        }
        Some(s) => s,
    };
    // M9/C4: C measures the string with `mywcslen` (`cmstypes.c:5385`), so an embedded NUL ends
    // it — the units past it are never written and never counted in `Sizes[i]`. Emitting them
    // would inflate this record's size and shift the offset of every string written after it.
    let end = s.find('\0').unwrap_or(s.len());
    for u in s[..end].encode_utf16() {
        crate::io::endian::write_u16(io, u)?;
    }
    col.size[i] = tell(io)? - before;
    Ok(())
}

/// `WriteOneMLUC` — an embedded `mluc`, recording its offset/size.
fn write_one_mluc<W: Write + Seek>(
    io: &mut W,
    col: &mut Column,
    i: usize,
    mlu: &Mlu,
    base_offset: u32,
) -> Result<()> {
    let before = tell(io)?;
    col.off[i] = before - base_offset;
    write_mlu_object(io, mlu)?;
    col.size[i] = tell(io)? - before;
    Ok(())
}

/// `Type_Dictionary_Write`.
pub(crate) fn write_dictionary<W: Write + Seek>(io: &mut W, value: &TagValue) -> Result<()> {
    let dict = match value {
        TagValue::Dict(d) => d,
        _ => return Err(wrong_variant("dictionary")),
    };
    let base_offset = tell(io)?.wrapping_sub(8);
    let entries = dict.entries();
    let count = entries.len();

    let any_name = entries.iter().any(|e| e.display_name.is_some());
    let any_value = entries.iter().any(|e| e.display_value.is_some());
    let mut length = 16u32;
    if any_name {
        length += 8;
    }
    if any_value {
        length += 8;
    }

    write_u32(io, count as u32)?;
    write_u32(io, length)?;

    let directory_pos = tell(io)?;

    let mut name = Column::new(count);
    let mut value_col = Column::new(count);
    let mut display_name = Column::new(count);
    let mut display_value = Column::new(count);

    // Placeholder directory.
    write_offset_array(io, &name, &value_col, &display_name, &display_value, count, length)?;

    for (i, e) in entries.iter().enumerate() {
        write_one_wchar(io, &mut name, i, Some(&e.name), base_offset)?;
        write_one_wchar(io, &mut value_col, i, e.value.as_deref(), base_offset)?;
        if let Some(dn) = &e.display_name {
            write_one_mluc(io, &mut display_name, i, dn, base_offset)?;
        }
        if let Some(dv) = &e.display_value {
            write_one_mluc(io, &mut display_value, i, dv, base_offset)?;
        }
    }

    let current = tell(io)?;
    seek_to(io, directory_pos)?;
    write_offset_array(io, &name, &value_col, &display_name, &display_value, count, length)?;
    seek_to(io, current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::handler::TypeHandler;
    use crate::tags::testutil::round_trip_bytes;

    #[test]
    fn dictionary_round_trip() {
        let mut dict = Dict::new();
        let mut dn = Mlu::new();
        dn.set_ascii(*b"en", *b"US", "Author name").unwrap();
        let mut dv = Mlu::new();
        dv.set_ascii(*b"en", *b"US", "Marti").unwrap();
        dict.add_entry("Author", Some("Marti Maria"), Some(dn), Some(dv));
        dict.add_entry("Year", Some("2026"), None, None);

        let v = TagValue::Dict(dict);
        // The C dictionary reader prepends entries (`cmsDictAddEntry`), so a single read flips
        // entry order; byte-identity therefore holds over a *two*-cycle round trip.
        let b1 = round_trip_bytes(TypeHandler::Dict, &v, 4.3);
        let v1 = crate::tags::testutil::read_back(TypeHandler::Dict, &b1);
        let b2 = round_trip_bytes(TypeHandler::Dict, &v1, 4.3);
        let v2 = crate::tags::testutil::read_back(TypeHandler::Dict, &b2);
        let b3 = round_trip_bytes(TypeHandler::Dict, &v2, 4.3);
        assert_eq!(b1, b3, "dictionary serialization not stable over two cycles");
    }

    /// M9/C4: `WriteOneWChar` measures with `mywcslen` (`cmstypes.c:5385`), so an embedded NUL
    /// ends the string. Writing the tail as well would inflate `Sizes[i]` and shift every later
    /// offset in the tag.
    #[test]
    fn dict_name_is_truncated_at_the_embedded_nul() {
        let mut dict = Dict::new();
        dict.add_entry("Year\u{0}2026", Some("v"), None, None);

        let bytes = round_trip_bytes(TypeHandler::Dict, &TagValue::Dict(dict), 4.3);

        // Body: Count | Length | (nameOff, nameSize, valueOff, valueSize) | strings.
        let name_size = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        assert_eq!(name_size, 8, "name size must count only \"Year\", not the NUL or its tail");

        let v = crate::tags::testutil::read_back(TypeHandler::Dict, &bytes);
        match v {
            TagValue::Dict(d) => assert_eq!(d.entries()[0].name, "Year"),
            _ => panic!("expected a dictionary"),
        }
    }

    /// H6/C5: `Count` is bounded against the tag size before the eight column vectors are sized
    /// from it. `Count = 0xFFFFFFFF` would otherwise request ~137 GB, and a failed `Vec`
    /// allocation aborts the process; C's `_cmsCalloc` returns NULL (`cmserr.c:139`) instead.
    #[test]
    fn dict_count_bounded_against_tag_size() {
        use std::io::Cursor;

        let mut body: Vec<u8> = Vec::new();
        body.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // Count
        body.extend_from_slice(&16u32.to_be_bytes()); // Length

        let err = read_dictionary(&mut Cursor::new(body.clone()), body.len() as u32).unwrap_err();
        assert_eq!(err.code, ErrorCode::Read);
    }
}
