//! Scalar, array and small-struct tag types from `cmstypes.c` that need no cross-module
//! machinery: chromaticity, colorantOrder, the fixed-point/integer arrays, signature,
//! dateTime, data, measurement, screening, viewingConditions, ucrBg, cicp, vcgt and MHC2.

use std::io::{Read, Seek, Write};

use crate::color::gamma::ToneCurve;
use crate::error::{Error, ErrorCode, Result};
use crate::io::endian::{
    read_datetime as rd_datetime, read_s15f16, read_u16, read_u16_array, read_u32, read_u64,
    read_u8, read_xyz, write_datetime as wr_datetime, write_s15f16, write_u16, write_u32,
    write_u64, write_u8, write_u16_array, write_xyz,
};
use crate::types::color::{CIExyY, CIExyYTriple, IccData};
use crate::types::num::quick_saturate_word;
use crate::types::sig::Signature;
use crate::MAX_CHANNELS;

use super::super::handler::{read_bytes, rel_offset, seek_to, tell, wrong_variant, write_bytes};
use super::mlu::MAX_ALLOC_BYTES;
use super::super::value::{Mhc2, Measurement, Screening, ScreeningChannel, TagValue, VideoSignal, ViewingConditions};

// --- cmsSigChromaticityType -------------------------------------------------------------

/// `Type_Chromaticity_Read`.
pub(crate) fn read_chromaticity<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    let mut n_chans = read_u16(io)?;
    // Recover from a bug introduced in early lcms1 versions.
    if n_chans == 0 && size_of_tag == 32 {
        let _ = read_u16(io)?;
        n_chans = read_u16(io)?;
    }
    if n_chans != 3 {
        return Err(Error::new(ErrorCode::CorruptionDetected, "chromaticity channels != 3"));
    }
    let _table = read_u16(io)?;
    let red = CIExyY { x: read_s15f16(io)?, y: read_s15f16(io)?, yy: 1.0 };
    let green = CIExyY { x: read_s15f16(io)?, y: read_s15f16(io)?, yy: 1.0 };
    let blue = CIExyY { x: read_s15f16(io)?, y: read_s15f16(io)?, yy: 1.0 };
    Ok(TagValue::Chromaticity(CIExyYTriple { red, green, blue }))
}

/// `Type_Chromaticity_Write` (`SaveOneChromaticity` inlined).
pub(crate) fn write_chromaticity<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let chrm = match value {
        TagValue::Chromaticity(c) => c,
        _ => return Err(wrong_variant("chromaticity")),
    };
    write_u16(io, 3)?; // nChannels
    write_u16(io, 0)?; // Table
    for xy in [&chrm.red, &chrm.green, &chrm.blue] {
        write_s15f16(io, xy.x)?;
        write_s15f16(io, xy.y)?;
    }
    Ok(())
}

// --- cmsSigColorantOrderType ------------------------------------------------------------

/// `Type_ColorantOrderType_Read` — stores the full `cmsMAXCHANNELS`-length array with `0xFF`
/// markers so the write can recover the exact count.
pub(crate) fn read_colorant_order<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    let count = read_u32(io)?;
    if count as usize > MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "too many colorants"));
    }
    let mut order = vec![0xFFu8; MAX_CHANNELS];
    io.read_exact(&mut order[..count as usize])
        .map_err(|e| Error::new(ErrorCode::Read, format!("colorant order read: {e}")))?;
    Ok(TagValue::ColorantOrder(order))
}

/// `Type_ColorantOrderType_Write`.
pub(crate) fn write_colorant_order<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let order = match value {
        TagValue::ColorantOrder(o) => o,
        _ => return Err(wrong_variant("colorant order")),
    };
    let count = order.iter().take(MAX_CHANNELS).filter(|&&b| b != 0xFF).count();
    write_u32(io, count as u32)?;
    write_bytes(io, &order[..count])
}

// --- cmsSigS15Fixed16ArrayType / cmsSigU16Fixed16ArrayType ------------------------------

/// `Type_S15Fixed16_Read`.
pub(crate) fn read_s15fixed16<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    let n = size_of_tag as usize / 4;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(read_s15f16(io)?);
    }
    Ok(TagValue::S15Fixed16Array(v))
}

/// `Type_S15Fixed16_Write`.
pub(crate) fn write_s15fixed16<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    match value {
        TagValue::S15Fixed16Array(v) => {
            for &x in v {
                write_s15f16(io, x)?;
            }
            Ok(())
        }
        _ => Err(wrong_variant("s15Fixed16 array")),
    }
}

/// `Type_U16Fixed16_Read` — stored on disk as `u32`, decoded to `f64` via `v / 65536`.
pub(crate) fn read_u16fixed16<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    let n = size_of_tag as usize / 4;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let raw = read_u32(io)?;
        v.push(raw as f64 / 65536.0);
    }
    Ok(TagValue::U16Fixed16Array(v))
}

/// `Type_U16Fixed16_Write` — `floor(value * 65536 + 0.5)` as `u32`.
pub(crate) fn write_u16fixed16<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    match value {
        TagValue::U16Fixed16Array(v) => {
            for &x in v {
                let raw = (x * 65536.0 + 0.5).floor() as u32;
                write_u32(io, raw)?;
            }
            Ok(())
        }
        _ => Err(wrong_variant("u16Fixed16 array")),
    }
}

// --- cmsSigUInt8ArrayType / UInt32 / UInt64 ---------------------------------------------

/// `Type_UInt8_Read`.
pub(crate) fn read_uint8<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    Ok(TagValue::UInt8Array(read_bytes(io, size_of_tag as usize)?))
}

/// `Type_UInt8_Write`.
pub(crate) fn write_uint8<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    match value {
        TagValue::UInt8Array(v) => write_bytes(io, v),
        _ => Err(wrong_variant("uint8 array")),
    }
}

/// `Type_UInt32_Read`.
pub(crate) fn read_uint32<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    let n = size_of_tag as usize / 4;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(read_u32(io)?);
    }
    Ok(TagValue::UInt32Array(v))
}

/// `Type_UInt32_Write`.
pub(crate) fn write_uint32<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    match value {
        TagValue::UInt32Array(v) => {
            for &x in v {
                write_u32(io, x)?;
            }
            Ok(())
        }
        _ => Err(wrong_variant("uint32 array")),
    }
}

/// `Type_UInt64_Read`.
pub(crate) fn read_uint64<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    let n = size_of_tag as usize / 8;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(read_u64(io)?);
    }
    Ok(TagValue::UInt64Array(v))
}

/// `Type_UInt64_Write`.
pub(crate) fn write_uint64<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    match value {
        TagValue::UInt64Array(v) => {
            for &x in v {
                write_u64(io, x)?;
            }
            Ok(())
        }
        _ => Err(wrong_variant("uint64 array")),
    }
}

// --- cmsSigSignatureType ----------------------------------------------------------------

/// `Type_Signature_Read`.
pub(crate) fn read_signature<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    Ok(TagValue::Signature(Signature(read_u32(io)?)))
}

/// `Type_Signature_Write`.
pub(crate) fn write_signature<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    match value {
        TagValue::Signature(s) => write_u32(io, s.as_u32()),
        _ => Err(wrong_variant("signature")),
    }
}

// --- cmsSigDateTimeType -----------------------------------------------------------------

/// `Type_DateTime_Read`.
pub(crate) fn read_datetime<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    Ok(TagValue::DateTime(rd_datetime(io)?))
}

/// `Type_DateTime_Write`.
pub(crate) fn write_datetime<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    match value {
        TagValue::DateTime(dt) => wr_datetime(io, dt),
        _ => Err(wrong_variant("dateTime")),
    }
}

// --- cmsSigDataType ---------------------------------------------------------------------

/// `Type_Data_Read` — a `u32` flag followed by `SizeOfTag - 4` payload bytes.
pub(crate) fn read_data<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    if size_of_tag < 4 {
        return Err(Error::new(ErrorCode::Read, "data tag too small"));
    }
    let len = size_of_tag - 4;
    // C5: C rejects `LenOfData > INT_MAX` outright (`cmstypes.c:1039`) and then allocates through
    // `_cmsMalloc`, which refuses anything over `MAX_MEMORY_FOR_ALLOC` and returns NULL
    // (`cmserr.c:139`). Both bounds matter here: `read_bytes` allocates eagerly, and an oversized
    // `Vec` aborts the process rather than returning an error. A `dataType` tag declaring
    // `0xF0000000` would otherwise trigger a ~3.7 GB allocation.
    if len > i32::MAX as u32 || len as usize > MAX_ALLOC_BYTES {
        return Err(Error::new(ErrorCode::Read, "data tag payload too large"));
    }
    let flag = read_u32(io)?;
    let data = read_bytes(io, len as usize)?;
    Ok(TagValue::Data(IccData { flag, data }))
}

/// `Type_Data_Write`.
pub(crate) fn write_data<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    match value {
        TagValue::Data(d) => {
            write_u32(io, d.flag)?;
            write_bytes(io, &d.data)
        }
        _ => Err(wrong_variant("data")),
    }
}

// --- cmsSigMeasurementType --------------------------------------------------------------

/// `Type_Measurement_Read`.
pub(crate) fn read_measurement<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    Ok(TagValue::Measurement(Measurement {
        observer: read_u32(io)?,
        backing: read_xyz(io)?,
        geometry: read_u32(io)?,
        flare: read_s15f16(io)?,
        illuminant_type: read_u32(io)?,
    }))
}

/// `Type_Measurement_Write`.
pub(crate) fn write_measurement<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let mc = match value {
        TagValue::Measurement(m) => m,
        _ => return Err(wrong_variant("measurement")),
    };
    write_u32(io, mc.observer)?;
    write_xyz(io, &mc.backing)?;
    write_u32(io, mc.geometry)?;
    write_s15f16(io, mc.flare)?;
    write_u32(io, mc.illuminant_type)
}

// --- cmsSigScreeningType ----------------------------------------------------------------

/// `Type_Screening_Read` — `nChannels` is clamped to `cmsMAXCHANNELS - 1`.
pub(crate) fn read_screening<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    let flag = read_u32(io)?;
    let mut n_channels = read_u32(io)?;
    if n_channels > (MAX_CHANNELS as u32 - 1) {
        n_channels = MAX_CHANNELS as u32 - 1;
    }
    let mut channels = Vec::with_capacity(n_channels as usize);
    for _ in 0..n_channels {
        channels.push(ScreeningChannel {
            frequency: read_s15f16(io)?,
            screen_angle: read_s15f16(io)?,
            spot_shape: read_u32(io)?,
        });
    }
    Ok(TagValue::Screening(Screening { flag, n_channels, channels }))
}

/// `Type_Screening_Write`.
pub(crate) fn write_screening<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let sc = match value {
        TagValue::Screening(s) => s,
        _ => return Err(wrong_variant("screening")),
    };
    write_u32(io, sc.flag)?;
    write_u32(io, sc.n_channels)?;
    // The declared count governs the payload: C loops to `nChannels` over a fixed
    // `Channels[cmsMAXCHANNELS]` array, so a short `channels` vector must still produce
    // `n_channels` records rather than a body that contradicts its own header. The missing ones
    // are zeros — exactly what C's zero-initialized struct holds.
    for i in 0..sc.n_channels as usize {
        let ch = sc.channels.get(i);
        write_s15f16(io, ch.map_or(0.0, |c| c.frequency))?;
        write_s15f16(io, ch.map_or(0.0, |c| c.screen_angle))?;
        write_u32(io, ch.map_or(0, |c| c.spot_shape))?;
    }
    Ok(())
}

// --- cmsSigViewingConditionsType --------------------------------------------------------

/// `Type_ViewingConditions_Read`.
pub(crate) fn read_viewing_conditions<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    Ok(TagValue::ViewingConditions(ViewingConditions {
        illuminant_xyz: read_xyz(io)?,
        surround_xyz: read_xyz(io)?,
        illuminant_type: read_u32(io)?,
    }))
}

/// `Type_ViewingConditions_Write`.
pub(crate) fn write_viewing_conditions<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let vc = match value {
        TagValue::ViewingConditions(v) => v,
        _ => return Err(wrong_variant("viewing conditions")),
    };
    write_xyz(io, &vc.illuminant_xyz)?;
    write_xyz(io, &vc.surround_xyz)?;
    write_u32(io, vc.illuminant_type)
}

// --- cmsSigUcrBgType --------------------------------------------------------------------

/// `Type_UcrBg_Read` — two tabulated curves (UCR then BG) plus a trailing ASCII description
/// whose length is the remaining tag size.
pub(crate) fn read_ucr_bg<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    use crate::named::mlu::Mlu;

    let mut remaining = size_of_tag as i64;

    if remaining < 4 {
        return Err(Error::new(ErrorCode::Read, "ucrBg too small"));
    }
    let count_ucr = read_u32(io)?;
    remaining -= 4;
    if remaining < (count_ucr as i64) * 2 {
        return Err(Error::new(ErrorCode::Read, "ucrBg UCR truncated"));
    }
    let ucr_table = read_u16_array(io, count_ucr as usize)?;
    remaining -= (count_ucr as i64) * 2;
    let ucr = ToneCurve::build_tabulated_16(count_ucr as usize, &ucr_table)?;

    if remaining < 4 {
        return Err(Error::new(ErrorCode::Read, "ucrBg missing BG count"));
    }
    let count_bg = read_u32(io)?;
    remaining -= 4;
    if remaining < (count_bg as i64) * 2 {
        return Err(Error::new(ErrorCode::Read, "ucrBg BG truncated"));
    }
    let bg_table = read_u16_array(io, count_bg as usize)?;
    remaining -= (count_bg as i64) * 2;
    let bg = ToneCurve::build_tabulated_16(count_bg as usize, &bg_table)?;

    if !(0..=32000).contains(&remaining) {
        return Err(Error::new(ErrorCode::Read, "ucrBg description length out of range"));
    }
    let bytes = read_bytes(io, remaining as usize)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let text = String::from_utf8_lossy(&bytes[..end]).into_owned();
    let mut desc = Mlu::new();
    desc.set_ascii([0, 0], [0, 0], &text)?;

    Ok(TagValue::UcrBg(super::super::value::UcrBg { ucr, bg, desc }))
}

/// `Type_UcrBg_Write` — the description is written as ASCII including its NUL terminator.
pub(crate) fn write_ucr_bg<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let v = match value {
        TagValue::UcrBg(u) => u,
        _ => return Err(wrong_variant("ucrBg")),
    };
    write_u32(io, v.ucr.estimated_table_entries())?;
    write_u16_array(io, v.ucr.estimated_table())?;
    write_u32(io, v.bg.estimated_table_entries())?;
    write_u16_array(io, v.bg.estimated_table())?;

    // Latin-1 bytes, not UTF-8: `cmsMLUgetASCII` narrows one wide unit to exactly one byte
    // (`cmsnamed.c:583-588`), so `get_ascii` — which widens 0x80..=0xFE into two UTF-8 bytes —
    // would desynchronise the payload from the tag size.
    //
    // When the MLU has no matching entry, `_cmsMLUgetWide` returns NULL, `cmsMLUgetASCII` returns
    // 0 (`cmsnamed.c:565`) and `TextSize` is therefore 0 — C writes **nothing at all**
    // (`cmstypes.c:3873-3877`), not a lone NUL.
    if let Some(text) = v.desc.get_ascii_bytes([0, 0], [0, 0]) {
        write_bytes(io, &text)?;
        write_u8(io, 0)?; // the NUL is part of the reported `TextSize` (`cmsnamed.c:570`, `:591`)
    }
    Ok(())
}

// --- cmsSigcicpType (CICP video signal) -------------------------------------------------

/// `Type_VideoSignal_Read` — requires exactly a 4-byte payload.
pub(crate) fn read_cicp<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    if size_of_tag != 4 {
        return Err(Error::new(ErrorCode::Read, "cicp tag must be 4 bytes"));
    }
    Ok(TagValue::Cicp(VideoSignal {
        colour_primaries: read_u8(io)?,
        transfer_characteristics: read_u8(io)?,
        matrix_coefficients: read_u8(io)?,
        video_full_range_flag: read_u8(io)?,
    }))
}

/// `Type_VideoSignal_Write`.
pub(crate) fn write_cicp<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let c = match value {
        TagValue::Cicp(c) => c,
        _ => return Err(wrong_variant("cicp")),
    };
    write_u8(io, c.colour_primaries)?;
    write_u8(io, c.transfer_characteristics)?;
    write_u8(io, c.matrix_coefficients)?;
    write_u8(io, c.video_full_range_flag)
}

// --- cmsSigVcgtType ---------------------------------------------------------------------

const VCGT_TABLE_TYPE: u32 = 0;
const VCGT_FORMULA_TYPE: u32 = 1;

/// `Type_vcgt_Read` — either 3 tabulated curves or 3 formula (type-5) curves.
pub(crate) fn read_vcgt<R: Read + Seek>(io: &mut R, size_of_tag: u32) -> Result<TagValue> {
    let tag_type = read_u32(io)?;
    match tag_type {
        VCGT_TABLE_TYPE => {
            let n_channels = read_u16(io)?;
            if n_channels != 3 {
                return Err(Error::new(
                    ErrorCode::UnknownExtension,
                    format!("Unsupported number of channels for VCGT '{n_channels}'"),
                ));
            }
            let n_elems = read_u16(io)?;
            let mut n_bytes = read_u16(io)?;
            // Adobe quirk fixup for broken profiles.
            if n_elems == 256 && n_bytes == 1 && size_of_tag == 1576 {
                n_bytes = 2;
            }
            let mut curves = Vec::with_capacity(3);
            for _ in 0..3 {
                let table: Vec<u16> = match n_bytes {
                    1 => {
                        let mut t = Vec::with_capacity(n_elems as usize);
                        for _ in 0..n_elems {
                            t.push(super::super::handler::from_8_to_16(read_u8(io)?));
                        }
                        t
                    }
                    2 => read_u16_array(io, n_elems as usize)?,
                    _ => {
                        return Err(Error::new(
                            ErrorCode::UnknownExtension,
                            format!("Unsupported bit depth for VCGT '{}'", n_bytes as u32 * 8),
                        ));
                    }
                };
                curves.push(ToneCurve::build_tabulated_16(n_elems as usize, &table)?);
            }
            Ok(TagValue::Vcgt(curves))
        }
        VCGT_FORMULA_TYPE => {
            let mut curves = Vec::with_capacity(3);
            for _ in 0..3 {
                let gamma = read_s15f16(io)?;
                let min = read_s15f16(io)?;
                let max = read_s15f16(io)?;
                // vcgt: Y = (Max-Min)*X^Gamma + Min, mapped to parametric type 5.
                let params = [gamma, (max - min).powf(1.0 / gamma), 0.0, 0.0, 0.0, min, 0.0];
                curves.push(ToneCurve::build_parametric(5, &params)?);
            }
            Ok(TagValue::Vcgt(curves))
        }
        _ => Err(Error::new(
            ErrorCode::UnknownExtension,
            format!("Unsupported tag type for VCGT '{tag_type}'"),
        )),
    }
}

/// `Type_vcgt_Write` — formula form when all three are parametric type 5, else a 256-word table.
pub(crate) fn write_vcgt<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    let curves = match value {
        TagValue::Vcgt(c) => c,
        _ => return Err(wrong_variant("vcgt")),
    };
    if curves.len() != 3 {
        return Err(Error::new(ErrorCode::Range, "vcgt needs exactly 3 curves"));
    }

    if curves.iter().all(|c| c.parametric_type() == 5) {
        write_u32(io, VCGT_FORMULA_TYPE)?;
        for c in curves {
            let seg = c
                .segment(0)
                .ok_or_else(|| Error::new(ErrorCode::Internal, "vcgt type-5 curve missing segment"))?;
            let gamma = seg.params[0];
            let min = seg.params[5];
            let max = seg.params[1].powf(gamma) + min;
            write_s15f16(io, gamma)?;
            write_s15f16(io, min)?;
            write_s15f16(io, max)?;
        }
    } else {
        // Always store as a table of 256 words.
        write_u32(io, VCGT_TABLE_TYPE)?;
        write_u16(io, 3)?;
        write_u16(io, 256)?;
        write_u16(io, 2)?;
        for c in curves {
            for j in 0..256u32 {
                let v = c.eval_f32((j as f64 / 255.0) as f32);
                let n = quick_saturate_word(v as f64 * 65535.0);
                write_u16(io, n)?;
            }
        }
    }
    Ok(())
}

// --- cmsSigMHC2Type ---------------------------------------------------------------------

/// `SetIdentity` — the 3×4 XYZ→XYZ identity used when no matrix is present.
fn mhc2_identity() -> [[f64; 4]; 3] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]
}

/// `IsIdentity` — every element within `1/65535` of identity (`CloseEnough`).
fn mhc2_is_identity(m: &[[f64; 4]; 3]) -> bool {
    let id = mhc2_identity();
    for i in 0..3 {
        for j in 0..4 {
            if (m[i][j] - id[i][j]).abs() >= 1.0 / 65535.0 {
                return false;
            }
        }
    }
    true
}

/// `ReadDoublesAt` — seek to `at`, read `n` `s15Fixed16` doubles, then restore the cursor.
fn read_doubles_at<R: Read + Seek>(io: &mut R, at: u32, out: &mut [f64]) -> Result<()> {
    let current = tell(io)?;
    seek_to(io, at)?;
    for v in out.iter_mut() {
        *v = read_s15f16(io)?;
    }
    seek_to(io, current)
}

/// `Type_MHC2_Read`.
pub(crate) fn read_mhc2<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    let base_offset = tell(io)?.wrapping_sub(8); // BaseOffset = Tell - sizeof(_cmsTagBase)

    let curve_entries = read_u32(io)?;
    if curve_entries > 4096 {
        return Err(Error::new(ErrorCode::Range, "MHC2 curve entries > 4096"));
    }
    let min_luminance = read_s15f16(io)?;
    let peak_luminance = read_s15f16(io)?;

    let matrix_offset = read_u32(io)?;
    let offset_red = read_u32(io)?;
    let offset_green = read_u32(io)?;
    let offset_blue = read_u32(io)?;

    let mut matrix = mhc2_identity();
    if matrix_offset != 0 {
        let mut flat = [0.0f64; 12];
        read_doubles_at(io, rel_offset(base_offset, matrix_offset)?, &mut flat)?;
        for i in 0..3 {
            matrix[i].copy_from_slice(&flat[i * 4..i * 4 + 4]);
        }
    }

    let mut red_curve = vec![0.0f64; curve_entries as usize];
    let mut green_curve = vec![0.0f64; curve_entries as usize];
    let mut blue_curve = vec![0.0f64; curve_entries as usize];
    // Each table is preceded by an 8-byte sf32 type base which is skipped here; both additions go
    // through `rel_offset` so neither can overflow.
    let table_at = |off: u32| rel_offset(base_offset, rel_offset(off, 8)?);
    read_doubles_at(io, table_at(offset_red)?, &mut red_curve)?;
    read_doubles_at(io, table_at(offset_green)?, &mut green_curve)?;
    read_doubles_at(io, table_at(offset_blue)?, &mut blue_curve)?;

    Ok(TagValue::Mhc2(Mhc2 {
        curve_entries,
        min_luminance,
        peak_luminance,
        matrix,
        red_curve,
        green_curve,
        blue_curve,
    }))
}

/// `WriteDoubles` — write `n` doubles as `s15Fixed16`.
fn write_doubles<W: Write>(io: &mut W, values: &[f64]) -> Result<()> {
    for &v in values {
        write_s15f16(io, v)?;
    }
    Ok(())
}

/// `WriteDoubles(io, n, Values)` where the caller's declared `n` governs, not the slice length.
///
/// C passes `mhc2->CurveEntries` — the value already written into the header — as the count
/// (`cmstypes.c:5808`, `:5813`, `:5818`), so the payload always matches the header even when the
/// two disagree in the object. Missing entries are written as zero rather than read past the end.
fn write_doubles_n<W: Write>(io: &mut W, values: &[f64], n: u32) -> Result<()> {
    for i in 0..n as usize {
        write_s15f16(io, values.get(i).copied().unwrap_or(0.0))?;
    }
    Ok(())
}

/// `Type_MHC2_Write`.
pub(crate) fn write_mhc2<W: Write + Seek>(io: &mut W, value: &TagValue) -> Result<()> {
    use super::super::handler::type_sig;

    let m = match value {
        TagValue::Mhc2(m) => m,
        _ => return Err(wrong_variant("MHC2")),
    };
    let base_offset = tell(io)?.wrapping_sub(8);

    write_u32(io, m.curve_entries)?;
    write_s15f16(io, m.min_luminance)?;
    write_s15f16(io, m.peak_luminance)?;

    let tables_offset_pos = tell(io)?;
    write_u32(io, 0)?; // Matrix
    write_u32(io, 0)?; // Curve R
    write_u32(io, 0)?; // Curve G
    write_u32(io, 0)?; // Curve B

    let matrix_offset = if mhc2_is_identity(&m.matrix) {
        0
    } else {
        let off = tell(io)? - base_offset;
        let flat: Vec<f64> = m.matrix.iter().flat_map(|r| r.iter().copied()).collect();
        write_doubles(io, &flat)?;
        off
    };

    let offset_red = tell(io)? - base_offset;
    write_u32(io, type_sig::S15FIXED16_ARRAY.as_u32())?;
    write_u32(io, 0)?;
    // M6: the header count governs the payload (`cmstypes.c:5808`).
    write_doubles_n(io, &m.red_curve, m.curve_entries)?;

    let offset_green = tell(io)? - base_offset;
    write_u32(io, type_sig::S15FIXED16_ARRAY.as_u32())?;
    write_u32(io, 0)?;
    write_doubles_n(io, &m.green_curve, m.curve_entries)?;

    let offset_blue = tell(io)? - base_offset;
    write_u32(io, type_sig::S15FIXED16_ARRAY.as_u32())?;
    write_u32(io, 0)?;
    write_doubles_n(io, &m.blue_curve, m.curve_entries)?;

    // Patch the offset directory. Matches C: no final seek-to-end (the mem handler tracks the
    // high-water mark, so the trailing bytes past this point remain part of the tag).
    seek_to(io, tables_offset_pos)?;
    write_u32(io, matrix_offset)?;
    write_u32(io, offset_red)?;
    write_u32(io, offset_green)?;
    write_u32(io, offset_blue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::handler::TypeHandler;
    use crate::tags::testutil::round_trip_stable;
    use crate::types::color::CIEXYZ;

    #[test]
    fn chromaticity_round_trip() {
        let t = CIExyYTriple {
            red: CIExyY { x: 0.64, y: 0.33, yy: 1.0 },
            green: CIExyY { x: 0.30, y: 0.60, yy: 1.0 },
            blue: CIExyY { x: 0.15, y: 0.06, yy: 1.0 },
        };
        round_trip_stable(TypeHandler::Chromaticity, &TagValue::Chromaticity(t));
    }

    #[test]
    fn colorant_order_round_trip() {
        let mut order = vec![0xFFu8; MAX_CHANNELS];
        order[..4].copy_from_slice(&[3, 0, 2, 1]);
        round_trip_stable(TypeHandler::ColorantOrder, &TagValue::ColorantOrder(order));
    }

    #[test]
    fn s15fixed16_round_trip() {
        let v = vec![1.0, -0.5, 0.9642, 3.375];
        round_trip_stable(TypeHandler::S15Fixed16, &TagValue::S15Fixed16Array(v));
    }

    #[test]
    fn u16fixed16_round_trip() {
        let v = vec![1.0, 0.5, 2.75];
        round_trip_stable(TypeHandler::U16Fixed16, &TagValue::U16Fixed16Array(v));
    }

    #[test]
    fn uint_arrays_round_trip() {
        round_trip_stable(TypeHandler::UInt8, &TagValue::UInt8Array(vec![1, 2, 3, 255]));
        round_trip_stable(TypeHandler::UInt32, &TagValue::UInt32Array(vec![1, 0xDEAD_BEEF]));
        round_trip_stable(TypeHandler::UInt64, &TagValue::UInt64Array(vec![1, 0x0102_0304_0506_0708]));
    }

    #[test]
    fn signature_round_trip() {
        round_trip_stable(TypeHandler::Signature, &TagValue::Signature(Signature::new(b"ADBE")));
    }

    #[test]
    fn datetime_round_trip() {
        use crate::types::color::DateTime;
        let dt = DateTime { year: 2026, month: 7, day: 19, hours: 8, minutes: 30, seconds: 15 };
        round_trip_stable(TypeHandler::DateTime, &TagValue::DateTime(dt));
    }

    #[test]
    fn data_round_trip() {
        round_trip_stable(TypeHandler::Data, &TagValue::Data(IccData { flag: 1, data: vec![9, 8, 7] }));
    }

    #[test]
    fn measurement_round_trip() {
        let m = Measurement {
            observer: 1,
            backing: CIEXYZ { x: 0.1, y: 0.2, z: 0.3 },
            geometry: 2,
            flare: 0.05,
            illuminant_type: 3,
        };
        round_trip_stable(TypeHandler::Measurement, &TagValue::Measurement(m));
    }

    #[test]
    fn screening_round_trip() {
        let s = Screening {
            flag: 3,
            n_channels: 2,
            channels: vec![
                ScreeningChannel { frequency: 120.0, screen_angle: 45.0, spot_shape: 1 },
                ScreeningChannel { frequency: 133.0, screen_angle: 15.0, spot_shape: 2 },
            ],
        };
        round_trip_stable(TypeHandler::Screening, &TagValue::Screening(s));
    }

    #[test]
    fn viewing_conditions_round_trip() {
        let v = ViewingConditions {
            illuminant_xyz: CIEXYZ { x: 0.9642, y: 1.0, z: 0.8249 },
            surround_xyz: CIEXYZ { x: 0.1, y: 0.1, z: 0.1 },
            illuminant_type: 1,
        };
        round_trip_stable(TypeHandler::ViewingConditions, &TagValue::ViewingConditions(v));
    }

    #[test]
    fn ucr_bg_round_trip() {
        let ucr = ToneCurve::build_tabulated_16(4, &[0, 100, 200, 300]).unwrap();
        let bg = ToneCurve::build_tabulated_16(3, &[10, 20, 30]).unwrap();
        let mut desc = crate::named::mlu::Mlu::new();
        desc.set_ascii([0, 0], [0, 0], "Method X").unwrap();
        round_trip_stable(TypeHandler::UcrBg, &TagValue::UcrBg(super::super::super::value::UcrBg { ucr, bg, desc }));
    }

    #[test]
    fn cicp_round_trip() {
        let c = VideoSignal {
            colour_primaries: 9,
            transfer_characteristics: 16,
            matrix_coefficients: 9,
            video_full_range_flag: 1,
        };
        round_trip_stable(TypeHandler::Cicp, &TagValue::Cicp(c));
    }

    #[test]
    fn vcgt_table_round_trip() {
        let t: Vec<u16> = (0..256).map(|i| (i * 257) as u16).collect();
        let curves = vec![
            ToneCurve::build_tabulated_16(256, &t).unwrap(),
            ToneCurve::build_tabulated_16(256, &t).unwrap(),
            ToneCurve::build_tabulated_16(256, &t).unwrap(),
        ];
        round_trip_stable(TypeHandler::Vcgt, &TagValue::Vcgt(curves));
    }

    #[test]
    fn vcgt_formula_round_trip() {
        let params = [2.2, 0.9, 0.0, 0.0, 0.0, 0.05, 0.0];
        let curves = vec![
            ToneCurve::build_parametric(5, &params).unwrap(),
            ToneCurve::build_parametric(5, &params).unwrap(),
            ToneCurve::build_parametric(5, &params).unwrap(),
        ];
        round_trip_stable(TypeHandler::Vcgt, &TagValue::Vcgt(curves));
    }

    #[test]
    fn mhc2_round_trip() {
        let m = Mhc2 {
            curve_entries: 3,
            min_luminance: 0.0,
            peak_luminance: 100.0,
            matrix: [
                [1.0, 0.0, 0.0, 0.1],
                [0.0, 1.0, 0.0, 0.2],
                [0.0, 0.0, 1.0, 0.3],
            ],
            red_curve: vec![0.0, 0.5, 1.0],
            green_curve: vec![0.0, 0.5, 1.0],
            blue_curve: vec![0.0, 0.5, 1.0],
        };
        round_trip_stable(TypeHandler::Mhc2, &TagValue::Mhc2(m));
    }

    #[test]
    fn mhc2_identity_matrix_round_trip() {
        let m = Mhc2 {
            curve_entries: 2,
            min_luminance: 0.0,
            peak_luminance: 80.0,
            matrix: mhc2_identity(),
            red_curve: vec![0.0, 1.0],
            green_curve: vec![0.0, 1.0],
            blue_curve: vec![0.0, 1.0],
        };
        round_trip_stable(TypeHandler::Mhc2, &TagValue::Mhc2(m));
    }

    /// A near-`u32::MAX` curve-table offset must surface as an error, never a panic. C relies on
    /// unsigned wraparound making the following seek fail; `rel_offset` turns that wrap into an
    /// explicit `ErrorCode::Seek` so overflow-checked builds behave like release ones.
    #[test]
    fn mhc2_absurd_table_offset_errors() {
        let mut body = Vec::new();
        body.extend_from_slice(&[0u8; 8]); // _cmsTagBase, already consumed when the reader runs
        body.extend_from_slice(&2u32.to_be_bytes()); // CurveEntries
        body.extend_from_slice(&0u32.to_be_bytes()); // MinLuminance (s15Fixed16)
        body.extend_from_slice(&0x0064_0000u32.to_be_bytes()); // PeakLuminance = 100.0
        body.extend_from_slice(&0u32.to_be_bytes()); // MatrixOffset = 0 -> identity, no seek
        body.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes()); // OffsetRedTable
        body.extend_from_slice(&0u32.to_be_bytes()); // OffsetGreenTable
        body.extend_from_slice(&0u32.to_be_bytes()); // OffsetBlueTable

        let mut io = std::io::Cursor::new(body);
        io.set_position(8); // BaseOffset = Tell - sizeof(_cmsTagBase) = 0
        let err = read_mhc2(&mut io).expect_err("absurd table offset must be rejected");
        assert_eq!(err.code, ErrorCode::Seek);
    }

    /// M6: the declared `CurveEntries` governs the payload, as in C (`cmstypes.c:5808`).
    ///
    /// With a shorter curve vector the body would otherwise contradict its own header, and the
    /// reader — which sizes the tables from `CurveEntries` — would consume the next table's type
    /// base as curve data.
    #[test]
    fn mhc2_header_count_governs_the_payload() {
        use crate::tags::testutil::round_trip_bytes;

        let m = Mhc2 {
            curve_entries: 3,
            min_luminance: 0.0,
            peak_luminance: 80.0,
            matrix: mhc2_identity(),
            red_curve: vec![0.0], // shorter than the declared count
            green_curve: vec![0.0, 0.5, 1.0],
            blue_curve: vec![0.0, 0.5, 1.0],
        };
        let bytes = round_trip_bytes(TypeHandler::Mhc2, &TagValue::Mhc2(m), 4.3);

        // 8 (type base) + 4 (CurveEntries) + 8 (luminances) + 16 (offsets) = 36, then each table
        // is an 8-byte sf32 base plus `CurveEntries` s15Fixed16 values.
        let expected = 36 + 3 * (8 + 3 * 4);
        assert_eq!(bytes.len(), expected, "payload must follow the declared CurveEntries");

        // Reading back yields three full-length curves, so the framing survives a round trip.
        match crate::tags::testutil::read_back(TypeHandler::Mhc2, &bytes) {
            TagValue::Mhc2(r) => {
                assert_eq!(r.red_curve, vec![0.0, 0.0, 0.0]);
                assert_eq!(r.blue_curve, vec![0.0, 0.5, 1.0]);
            }
            _ => panic!("expected an MHC2 value"),
        }
    }

    /// The `ucrBg` description is emitted as Latin-1 bytes, and an MLU with no matching entry
    /// produces **nothing** — `cmsMLUgetASCII` returns 0 (`cmsnamed.c:565`), so C's `TextSize` is
    /// 0 and `io->Write(io, 0, Text)` writes no bytes (`cmstypes.c:3873-3877`).
    #[test]
    fn ucr_bg_description_latin1_and_empty_mlu_writes_nothing() {
        use super::super::super::value::UcrBg;
        use crate::tags::testutil::round_trip_bytes;

        let curves = || ToneCurve::build_tabulated_16(2, &[0, 65535]).unwrap();
        let ucr_bg = |desc| TagValue::UcrBg(UcrBg { ucr: curves(), bg: curves(), desc });

        // An MLU with no entry contributes no bytes at all, so this is the header-only length.
        let head = round_trip_bytes(TypeHandler::UcrBg, &ucr_bg(crate::named::mlu::Mlu::new()), 4.3).len();

        // U+00E9 is one wide unit, so it must narrow to the single byte 0xE9, not two UTF-8 bytes.
        let mut desc = crate::named::mlu::Mlu::new();
        desc.set_wide([0, 0], [0, 0], &[0x43, 0x61, 0x66, 0x00e9]).unwrap();
        let bytes = round_trip_bytes(TypeHandler::UcrBg, &ucr_bg(desc), 4.3);

        assert_eq!(bytes.len(), head + 5, "the description is 4 Latin-1 bytes plus its NUL");
        assert_eq!(&bytes[head..], &[0x43, 0x61, 0x66, 0xe9, 0x00]);
    }

    /// The declared `nChannels` governs the payload: C loops to `nChannels` over a fixed-size
    /// array, so a short `channels` vector must not shorten the body.
    #[test]
    fn screening_header_count_governs_the_payload() {
        use crate::tags::testutil::round_trip_bytes;

        let s = Screening {
            flag: 3,
            n_channels: 2,
            channels: vec![ScreeningChannel { frequency: 120.0, screen_angle: 45.0, spot_shape: 1 }],
        };
        let bytes = round_trip_bytes(TypeHandler::Screening, &TagValue::Screening(s), 4.3);
        assert_eq!(bytes.len(), 8 + 4 + 4 + 2 * 12, "body must hold n_channels records");
    }

    /// C5: `Type_Data_Read` rejects `LenOfData > INT_MAX` before allocating (`cmstypes.c:1039`),
    /// and `_cmsMalloc` refuses anything over `MAX_MEMORY_FOR_ALLOC` (`cmserr.c:139`). Rust must
    /// reject too — `read_bytes` allocates eagerly and an oversized `Vec` aborts the process.
    #[test]
    fn data_payload_size_is_bounded() {
        let mut io = std::io::Cursor::new(vec![0u8; 8]);
        let err = read_data(&mut io, 0xF000_0000).expect_err("oversized dataType must be rejected");
        assert_eq!(err.code, ErrorCode::Read);
        // The size must be rejected up front, not by the read failing after a ~3.7 GB allocation
        // (which in Rust aborts rather than erroring). A short read reports a different message,
        // and the cursor stays put because the guard returns before the `flag` is consumed.
        assert!(
            err.message.contains("too large"),
            "expected an up-front size rejection, got: {}",
            err.message
        );
        assert_eq!(io.position(), 0, "nothing must be consumed before the size is validated");
    }
}
