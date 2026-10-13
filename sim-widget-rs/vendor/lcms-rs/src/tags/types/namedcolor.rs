//! `cmsSigColorantTableType` (`clrt`) and `cmsSigNamedColor2Type` (`ncl2`) — both backed by
//! a [`NamedColorList`] (`Type_ColorantTable_*` / `Type_NamedColor_*` in `cmstypes.c`).

use std::io::{Read, Seek, Write};

use crate::error::{Error, ErrorCode, Result};
use crate::io::endian::{read_u16, read_u16_array, read_u32, write_u16_array, write_u32};
use crate::named::namedcolor::NamedColorList;
use crate::MAX_CHANNELS;

use super::super::handler::{read_bytes, wrong_variant, write_bytes};
use super::super::value::TagValue;

/// Read a fixed-width name field and decode it up to its first NUL.
fn read_fixed_name<R: Read>(io: &mut R, width: usize) -> Result<String> {
    let bytes = read_bytes(io, width)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
}

/// Write a name into a `width`-byte, NUL-padded field (`memset` + `strncpy` semantics).
fn write_fixed_name<W: Write>(io: &mut W, name: &str, width: usize) -> Result<()> {
    let mut buf = vec![0u8; width];
    let src = name.as_bytes();
    let n = src.len().min(width);
    buf[..n].copy_from_slice(&src[..n]);
    write_bytes(io, &buf)
}

// --- cmsSigColorantTableType ------------------------------------------------------------

/// `Type_ColorantTable_Read`.
pub(crate) fn read_colorant_table<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    let count = read_u32(io)?;
    if count as usize > MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, format!("Too many colorants '{count}'")));
    }
    // A colorant table carries no device colorants (ColorantCount = 0).
    let mut list = NamedColorList::new(0, "", "")?;
    for _ in 0..count {
        let name = read_fixed_name(io, 32)?;
        let mut pcs = [0u16; 3];
        for p in pcs.iter_mut() {
            *p = read_u16(io)?;
        }
        list.append(&name, pcs, [0u16; MAX_CHANNELS]);
    }
    Ok(TagValue::ColorantTable(list))
}

/// `Type_ColorantTable_Write`.
pub(crate) fn write_colorant_table<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let list = match value {
        TagValue::ColorantTable(l) => l,
        _ => return Err(wrong_variant("colorant table")),
    };
    let n_colors = list.count() as u32;
    write_u32(io, n_colors)?;
    for i in 0..list.count() {
        let info = list.info(i).ok_or_else(|| Error::new(ErrorCode::Internal, "colorant vanished"))?;
        write_fixed_name(io, info.name, 32)?;
        write_u16_array(io, &info.pcs)?;
    }
    Ok(())
}

// --- cmsSigNamedColor2Type --------------------------------------------------------------

/// `Type_NamedColor_Read`.
pub(crate) fn read_named_color<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    let _vendor_flag = read_u32(io)?;
    let count = read_u32(io)?;
    let n_device_coords = read_u32(io)?;

    let prefix = read_prefix_suffix(io)?;
    let suffix = read_prefix_suffix(io)?;

    if n_device_coords as usize > MAX_CHANNELS {
        return Err(Error::new(
            ErrorCode::Range,
            format!("Too many device coordinates '{n_device_coords}'"),
        ));
    }
    let mut list = NamedColorList::new(n_device_coords, &prefix, &suffix)?;

    for _ in 0..count {
        let name = read_fixed_name(io, 32)?;
        let mut pcs = [0u16; 3];
        for p in pcs.iter_mut() {
            *p = read_u16(io)?;
        }
        let coords = read_u16_array(io, n_device_coords as usize)?;
        let mut colorant = [0u16; MAX_CHANNELS];
        colorant[..coords.len()].copy_from_slice(&coords);
        list.append(&name, pcs, colorant);
    }

    Ok(TagValue::NamedColor(list))
}

/// Read a 32-byte prefix/suffix field, forcing a terminator at index 31 as the C code does.
fn read_prefix_suffix<R: Read>(io: &mut R) -> Result<String> {
    let mut bytes = read_bytes(io, 32)?;
    bytes[31] = 0; // prefix[31] = suffix[31] = 0
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
}

/// `Type_NamedColor_Write`.
pub(crate) fn write_named_color<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let list = match value {
        TagValue::NamedColor(l) => l,
        _ => return Err(wrong_variant("named color")),
    };
    let n_colors = list.count() as u32;

    write_u32(io, 0)?; // vendor flag
    write_u32(io, n_colors)?;
    write_u32(io, list.colorant_count() as u32)?;

    write_fixed_name(io, list.prefix(), 32)?;
    write_fixed_name(io, list.suffix(), 32)?;

    let cc = list.colorant_count();
    for i in 0..list.count() {
        let info = list.info(i).ok_or_else(|| Error::new(ErrorCode::Internal, "color vanished"))?;
        write_fixed_name(io, info.name, 32)?;
        write_u16_array(io, &info.pcs)?;
        write_u16_array(io, &info.colorant[..cc])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::handler::TypeHandler;
    use crate::tags::testutil::round_trip_stable;

    #[test]
    fn colorant_table_round_trip() {
        let mut list = NamedColorList::new(0, "", "").unwrap();
        list.append("Cyan", [100, 200, 300], [0u16; MAX_CHANNELS]);
        list.append("Magenta", [400, 500, 600], [0u16; MAX_CHANNELS]);
        round_trip_stable(TypeHandler::ColorantTable, &TagValue::ColorantTable(list));
    }

    #[test]
    fn named_color_round_trip() {
        let mut list = NamedColorList::new(4, "pre", "suf").unwrap();
        let mut cmyk = [0u16; MAX_CHANNELS];
        cmyk[..4].copy_from_slice(&[10, 20, 30, 40]);
        list.append("Red", [1, 2, 3], cmyk);
        list.append("Green", [4, 5, 6], [0u16; MAX_CHANNELS]);
        round_trip_stable(TypeHandler::NamedColor, &TagValue::NamedColor(list));
    }
}
