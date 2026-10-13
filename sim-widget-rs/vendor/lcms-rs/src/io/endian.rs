//! Endian-aware ICC readers/writers — a port of the `_cmsRead*`/`_cmsWrite*` family in
//! `cmsplugin.c`.
//!
//! ICC data is **always big-endian** on disk. lcms2 reads host-endian words and then byte-
//! swaps them (`_cmsAdjustEndianess*`); Rust reads big-endian directly with
//! [`u32::from_be_bytes`], so the swap helpers disappear entirely and there is no `unsafe`
//! union punning (`f32::from_bits` replaces the `union { u32; f32 }`).
//!
//! These operate on any [`Read`]/[`Write`]; the Phase-3 `IoHandler` backends (mem/file/
//! counting) plug in through those std traits.

use std::io::{Read, Write};

use crate::error::{Error, ErrorCode};
use crate::types::color::{CIEXYZ, DateTime};
use crate::types::num::{f64_to_s15f16, s15f16_to_f64};

type Result<T> = crate::error::Result<T>;

#[inline]
fn read_err(e: std::io::Error) -> Error {
    Error::new(ErrorCode::Read, format!("short read: {e}"))
}
#[inline]
fn write_err(e: std::io::Error) -> Error {
    Error::new(ErrorCode::Write, format!("write failed: {e}"))
}

// --- scalar reads (big-endian) ---

/// Read a `u8` (`_cmsReadUInt8Number`).
#[inline]
pub fn read_u8<R: Read>(r: &mut R) -> Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b).map_err(read_err)?;
    Ok(b[0])
}

/// Read a big-endian `u16` (`_cmsReadUInt16Number`).
#[inline]
pub fn read_u16<R: Read>(r: &mut R) -> Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b).map_err(read_err)?;
    Ok(u16::from_be_bytes(b))
}

/// Read a big-endian `u32` (`_cmsReadUInt32Number`).
#[inline]
pub fn read_u32<R: Read>(r: &mut R) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).map_err(read_err)?;
    Ok(u32::from_be_bytes(b))
}

/// Read a big-endian `u64` (`_cmsReadUInt64Number`).
#[inline]
pub fn read_u64<R: Read>(r: &mut R) -> Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b).map_err(read_err)?;
    Ok(u64::from_be_bytes(b))
}

/// Read a big-endian IEEE `f32` (`_cmsReadFloat32Number`).
///
/// Preserves lcms2's guard against corrupt streams: values whose magnitude exceeds `1e20`,
/// or which are not either zero or normal (i.e. subnormal/inf/NaN), are rejected — matching
/// the `fpclassify() == FP_ZERO || FP_NORMAL` check.
pub fn read_f32<R: Read>(r: &mut R) -> Result<f32> {
    let bits = read_u32(r)?;
    let v = f32::from_bits(bits);
    // C: reject `v > 1e20 || v < -1e20` (equivalently outside the closed range) and any
    // value that is neither zero nor normal (fpclassify != FP_ZERO && != FP_NORMAL).
    if !(-1e20..=1e20).contains(&v) || !(v == 0.0 || v.is_normal()) {
        return Err(Error::new(ErrorCode::Read, "float32 out of sane range"));
    }
    Ok(v)
}

/// Read a big-endian 15.16 fixed-point value as `f64` (`_cmsRead15Fixed16Number`).
#[inline]
pub fn read_s15f16<R: Read>(r: &mut R) -> Result<f64> {
    Ok(s15f16_to_f64(read_u32(r)? as i32))
}

/// Read an encoded XYZ triple (`_cmsReadXYZNumber`): three big-endian 15.16 values.
pub fn read_xyz<R: Read>(r: &mut R) -> Result<CIEXYZ> {
    Ok(CIEXYZ { x: read_s15f16(r)?, y: read_s15f16(r)?, z: read_s15f16(r)? })
}

/// Read `n` big-endian `u16`s (`_cmsReadUInt16Array`).
pub fn read_u16_array<R: Read>(r: &mut R, n: usize) -> Result<Vec<u16>> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(read_u16(r)?);
    }
    Ok(out)
}

// --- scalar writes (big-endian) ---

/// Write a `u8` (`_cmsWriteUInt8Number`).
#[inline]
pub fn write_u8<W: Write>(w: &mut W, n: u8) -> Result<()> {
    w.write_all(&[n]).map_err(write_err)
}

/// Write a big-endian `u16` (`_cmsWriteUInt16Number`).
#[inline]
pub fn write_u16<W: Write>(w: &mut W, n: u16) -> Result<()> {
    w.write_all(&n.to_be_bytes()).map_err(write_err)
}

/// Write a big-endian `u32` (`_cmsWriteUInt32Number`).
#[inline]
pub fn write_u32<W: Write>(w: &mut W, n: u32) -> Result<()> {
    w.write_all(&n.to_be_bytes()).map_err(write_err)
}

/// Write a big-endian `u64` (`_cmsWriteUInt64Number`).
#[inline]
pub fn write_u64<W: Write>(w: &mut W, n: u64) -> Result<()> {
    w.write_all(&n.to_be_bytes()).map_err(write_err)
}

/// Write a big-endian IEEE `f32` (`_cmsWriteFloat32Number`).
#[inline]
pub fn write_f32<W: Write>(w: &mut W, n: f32) -> Result<()> {
    write_u32(w, n.to_bits())
}

/// Write an `f64` as a big-endian 15.16 fixed-point value (`_cmsWrite15Fixed16Number`).
#[inline]
pub fn write_s15f16<W: Write>(w: &mut W, n: f64) -> Result<()> {
    write_u32(w, f64_to_s15f16(n) as u32)
}

/// Write an encoded XYZ triple (`_cmsWriteXYZNumber`).
pub fn write_xyz<W: Write>(w: &mut W, xyz: &CIEXYZ) -> Result<()> {
    write_s15f16(w, xyz.x)?;
    write_s15f16(w, xyz.y)?;
    write_s15f16(w, xyz.z)
}

/// Write `n` big-endian `u16`s (`_cmsWriteUInt16Array`).
pub fn write_u16_array<W: Write>(w: &mut W, array: &[u16]) -> Result<()> {
    for &v in array {
        write_u16(w, v)?;
    }
    Ok(())
}

// --- date/time codec ---
//
// The on-disk `dateTimeNumber` is six big-endian u16s in the order
// year, month, day, hours, minutes, seconds. lcms2 converts to/from `struct tm` and there
// applies the -1900 (year) / -1 (month) offsets; our [`DateTime`] already stores natural
// full-year / 1-based-month values that equal the on-disk numbers, so the codec is a plain
// six-word transfer with no offsetting.

/// Read a `dateTimeNumber` (`_cmsDecodeDateTimeNumber`, natural-form target).
pub fn read_datetime<R: Read>(r: &mut R) -> Result<DateTime> {
    Ok(DateTime {
        year: read_u16(r)?,
        month: read_u16(r)?,
        day: read_u16(r)?,
        hours: read_u16(r)?,
        minutes: read_u16(r)?,
        seconds: read_u16(r)?,
    })
}

/// Write a `dateTimeNumber` (`_cmsEncodeDateTimeNumber`, natural-form source).
pub fn write_datetime<W: Write>(w: &mut W, dt: &DateTime) -> Result<()> {
    write_u16(w, dt.year)?;
    write_u16(w, dt.month)?;
    write_u16(w, dt.day)?;
    write_u16(w, dt.hours)?;
    write_u16(w, dt.minutes)?;
    write_u16(w, dt.seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn big_endian_scalars_roundtrip() {
        let mut buf = Vec::new();
        write_u8(&mut buf, 0x12).unwrap();
        write_u16(&mut buf, 0x1234).unwrap();
        write_u32(&mut buf, 0x1234_5678).unwrap();
        write_u64(&mut buf, 0x1122_3344_5566_7788).unwrap();
        // Confirm the actual byte order is big-endian.
        assert_eq!(&buf[0..1], &[0x12]);
        assert_eq!(&buf[1..3], &[0x12, 0x34]);
        assert_eq!(&buf[3..7], &[0x12, 0x34, 0x56, 0x78]);
        assert_eq!(&buf[7..15], &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);

        let mut r = Cursor::new(buf);
        assert_eq!(read_u8(&mut r).unwrap(), 0x12);
        assert_eq!(read_u16(&mut r).unwrap(), 0x1234);
        assert_eq!(read_u32(&mut r).unwrap(), 0x1234_5678);
        assert_eq!(read_u64(&mut r).unwrap(), 0x1122_3344_5566_7788);
    }

    #[test]
    fn short_read_errors() {
        let mut r = Cursor::new(vec![0x00]); // only 1 byte
        assert!(read_u32(&mut r).is_err());
    }

    #[test]
    fn s15f16_and_xyz() {
        let mut buf = Vec::new();
        // 1.0 in 15.16 is 0x00010000, big-endian.
        write_s15f16(&mut buf, 1.0).unwrap();
        assert_eq!(&buf, &[0x00, 0x01, 0x00, 0x00]);
        assert_eq!(read_s15f16(&mut Cursor::new(buf)).unwrap(), 1.0);

        // D50 XYZ round-trips through the 15.16 quantization.
        let d50 = CIEXYZ { x: 0.9642, y: 1.0, z: 0.8249 };
        let mut b = Vec::new();
        write_xyz(&mut b, &d50).unwrap();
        let back = read_xyz(&mut Cursor::new(b)).unwrap();
        // Quantization error is < 1/65536.
        assert!((back.x - d50.x).abs() < 1.0 / 65536.0);
        assert!((back.y - d50.y).abs() < 1.0 / 65536.0);
        assert!((back.z - d50.z).abs() < 1.0 / 65536.0);
    }

    #[test]
    fn float32_guard_rejects_insane() {
        // +inf bit pattern must be rejected.
        let mut b = Vec::new();
        write_u32(&mut b, f32::INFINITY.to_bits()).unwrap();
        assert!(read_f32(&mut Cursor::new(b)).is_err());
        // A normal value passes and round-trips.
        let mut b2 = Vec::new();
        write_f32(&mut b2, 0.5).unwrap();
        assert_eq!(read_f32(&mut Cursor::new(b2)).unwrap(), 0.5);
    }

    #[test]
    fn u16_array_roundtrip() {
        let data = [1u16, 2, 300, 0xFFFF];
        let mut b = Vec::new();
        write_u16_array(&mut b, &data).unwrap();
        assert_eq!(read_u16_array(&mut Cursor::new(b), 4).unwrap(), data);
    }

    #[test]
    fn datetime_roundtrip_big_endian() {
        let dt = DateTime { year: 2026, month: 7, day: 19, hours: 8, minutes: 30, seconds: 15 };
        let mut b = Vec::new();
        write_datetime(&mut b, &dt).unwrap();
        assert_eq!(&b[0..2], &2026u16.to_be_bytes()); // big-endian year
        assert_eq!(read_datetime(&mut Cursor::new(b)).unwrap(), dt);
    }
}
