//! `cmsSigCurveType` (`curv`) and `cmsSigParametricCurveType` (`para`) tone-curve tags,
//! plus the embedded-curve read/write helpers reused by the LUT A2B/B2A serializers
//! (`ReadEmbeddedCurve` / `WriteSetOfCurves` in `cmstypes.c`).

use std::io::{Read, Seek, Write};

use crate::color::gamma::ToneCurve;
use crate::error::{Error, ErrorCode, Result};
use crate::io::endian::{
    read_s15f16, read_u16, read_u16_array, read_u32, write_s15f16, write_u16, write_u16_array,
    write_u32,
};
use crate::types::num::{f64_to_u8f8, u8f8_to_f64};

use super::super::handler::{
    read_alignment, read_type_base, type_sig, wrong_variant, write_alignment, write_type_base,
};
use super::super::value::TagValue;

/// `Curve->nSegments` — number of parametric/sampled segments (0 for a pure 16-bit table).
///
/// Probes [`ToneCurve::segment`] since the field is private; the count is tiny in practice.
pub(crate) fn n_segments(c: &ToneCurve) -> usize {
    let mut n = 0i32;
    while c.segment(n).is_some() {
        n += 1;
    }
    n as usize
}

// --- cmsSigCurveType --------------------------------------------------------------------

/// `Type_Curve_Read` — 0 → gamma-1 parametric, 1 → 8.8 gamma, else a 16-bit table.
pub(crate) fn read_curve_tc<R: Read>(io: &mut R) -> Result<ToneCurve> {
    let count = read_u32(io)?;
    match count {
        0 => ToneCurve::build_parametric(1, &[1.0]),
        1 => {
            let fixed = read_u16(io)?;
            let single_gamma = u8f8_to_f64(fixed);
            ToneCurve::build_parametric(1, &[single_gamma])
        }
        _ => {
            if count > 0x7FFF {
                return Err(Error::new(ErrorCode::Range, "curve table too large"));
            }
            let table = read_u16_array(io, count as usize)?;
            ToneCurve::build_tabulated_16(count as usize, &table)
        }
    }
}

/// `Type_Curve_Write` — single-`Type-1` gamma preserved as 8.8; otherwise the 16-bit table.
pub(crate) fn write_curve_tc<W: Write>(io: &mut W, c: &ToneCurve) -> Result<()> {
    if c.parametric_type() == 1 {
        // Single gamma, preserve the compact 8.8 encoding (`parametric_type() == 1` guarantees
        // one segment, so `segment(0)` is present).
        let seg = c
            .segment(0)
            .ok_or_else(|| Error::new(ErrorCode::Internal, "single-gamma curve missing segment"))?;
        let single_gamma_fixed = f64_to_u8f8(seg.params[0]);
        write_u32(io, 1)?;
        write_u16(io, single_gamma_fixed)?;
        return Ok(());
    }
    write_u32(io, c.estimated_table_entries())?;
    write_u16_array(io, c.estimated_table())
}

/// `cmsSigCurveType` tag handler read.
pub(crate) fn read_curve<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    Ok(TagValue::Curve(read_curve_tc(io)?))
}

/// `cmsSigCurveType` tag handler write.
pub(crate) fn write_curve<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    match value {
        TagValue::Curve(c) => write_curve_tc(io, c),
        _ => Err(wrong_variant("curve")),
    }
}

// --- cmsSigParametricCurveType ----------------------------------------------------------

/// Parameter count per on-disk parametric type (0..=4) — `ParamsByType` in `Type_ParametricCurve_Read`.
const PARAMS_BY_TYPE_READ: [usize; 5] = [1, 3, 4, 5, 7];
/// Parameter count per stored `Segments[0].Type` (1..=5) — `ParamsByType` in the writer.
const PARAMS_BY_TYPE_WRITE: [usize; 6] = [0, 1, 3, 4, 5, 7];

/// `Type_ParametricCurve_Read`.
pub(crate) fn read_parametric_tc<R: Read>(io: &mut R) -> Result<ToneCurve> {
    let type_ = read_u16(io)?;
    let _reserved = read_u16(io)?;
    if type_ > 4 {
        return Err(Error::new(
            ErrorCode::UnknownExtension,
            format!("Unknown parametric curve type '{type_}'"),
        ));
    }
    let n = PARAMS_BY_TYPE_READ[type_ as usize];
    let mut params = [0.0f64; 10];
    for p in params.iter_mut().take(n) {
        *p = read_s15f16(io)?;
    }
    // On disk type is 0-based; the tone-curve type is 1-based (`Type + 1`).
    ToneCurve::build_parametric(type_ as i32 + 1, &params[..n])
}

/// `Type_ParametricCurve_Write`.
pub(crate) fn write_parametric_tc<W: Write>(io: &mut W, c: &ToneCurve) -> Result<()> {
    if n_segments(c) != 1 {
        return Err(Error::new(
            ErrorCode::UnknownExtension,
            "Multisegment or Inverted parametric curves cannot be written",
        ));
    }
    let seg = c
        .segment(0)
        .ok_or_else(|| Error::new(ErrorCode::Internal, "parametric curve missing segment"))?;
    let typen = seg.seg_type;
    if typen < 1 {
        return Err(Error::new(
            ErrorCode::UnknownExtension,
            "Multisegment or Inverted parametric curves cannot be written",
        ));
    }
    if typen > 5 {
        return Err(Error::new(ErrorCode::UnknownExtension, "Unsupported parametric curve"));
    }
    let n_params = PARAMS_BY_TYPE_WRITE[typen as usize];
    write_u16(io, (typen - 1) as u16)?;
    write_u16(io, 0)?; // Reserved
    for i in 0..n_params {
        write_s15f16(io, seg.params[i])?;
    }
    Ok(())
}

/// `cmsSigParametricCurveType` tag handler read.
pub(crate) fn read_parametric<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    Ok(TagValue::Curve(read_parametric_tc(io)?))
}

/// `cmsSigParametricCurveType` tag handler write.
pub(crate) fn write_parametric<W: Write>(io: &mut W, value: &TagValue) -> Result<()> {
    match value {
        TagValue::Curve(c) => write_parametric_tc(io, c),
        _ => Err(wrong_variant("curve")),
    }
}

// --- Embedded curves (inside LutAtoB / LutBtoA) -----------------------------------------

/// `ReadEmbeddedCurve` — read a type base then dispatch to the `curv`/`para` body reader.
pub(crate) fn read_embedded_curve<R: Read + Seek>(io: &mut R) -> Result<ToneCurve> {
    let base = read_type_base(io)?;
    if base == type_sig::CURVE {
        read_curve_tc(io)
    } else if base == type_sig::PARAMETRIC_CURVE {
        read_parametric_tc(io)
    } else {
        Err(Error::new(ErrorCode::UnknownExtension, format!("Unknown curve type '{base}'")))
    }
}

/// One iteration of `WriteSetOfCurves` (the caller always passes `cmsSigParametricCurveType`):
/// pick `curv` for tabulated/inverted curves, else `para`; write the type base, the body, and
/// the trailing 4-byte alignment.
pub(crate) fn write_embedded_curve<W: Write + Seek>(io: &mut W, c: &ToneCurve) -> Result<()> {
    let nseg = n_segments(c);
    // Table-based (16-bit tabulated OR float tabulated) and inverted curves fall back to `curv`.
    let use_curve = nseg == 0
        || (nseg == 3 && c.segment(1).map(|s| s.seg_type).unwrap_or(0) == 0)
        || c.segment(0).map(|s| s.seg_type).unwrap_or(0) < 0;

    if use_curve {
        write_type_base(io, type_sig::CURVE)?;
        write_curve_tc(io, c)?;
    } else {
        write_type_base(io, type_sig::PARAMETRIC_CURVE)?;
        write_parametric_tc(io, c)?;
    }
    write_alignment(io)
}

/// `ReadSetOfCurves` companion for one curve: [`read_embedded_curve`] then `_cmsReadAlignment`.
pub(crate) fn read_embedded_curve_aligned<R: Read + Seek>(io: &mut R) -> Result<ToneCurve> {
    let c = read_embedded_curve(io)?;
    read_alignment(io)?;
    Ok(c)
}

#[cfg(test)]
mod tests {
    use crate::color::gamma::ToneCurve;
    use crate::tags::handler::TypeHandler;
    use crate::tags::testutil::round_trip_stable;
    use crate::tags::value::TagValue;

    #[test]
    fn curve_gamma_round_trip() {
        let v = TagValue::Curve(ToneCurve::build_gamma(2.2).unwrap());
        round_trip_stable(TypeHandler::Curve, &v);
    }

    #[test]
    fn curve_tabulated_round_trip() {
        let table: Vec<u16> = (0..256).map(|i| (i * 257) as u16).collect();
        let v = TagValue::Curve(ToneCurve::build_tabulated_16(256, &table).unwrap());
        round_trip_stable(TypeHandler::Curve, &v);
    }

    #[test]
    fn parametric_round_trip() {
        // sRGB-like type 4 (7 params).
        let params = [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045, 0.0, 0.0];
        let v = TagValue::Curve(ToneCurve::build_parametric(4, &params).unwrap());
        round_trip_stable(TypeHandler::ParametricCurve, &v);
    }
}
