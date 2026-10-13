//! `cmsSigProfileSequenceDescType` (`pseq`) and `cmsSigProfileSequenceIdType` (`psid`) —
//! both backed by a [`Seq`] (`Type_ProfileSequenceDesc_*` / `Type_ProfileSequenceId_*`).
//!
//! The embedded per-profile description strings are serialized by `SaveDescription`, which
//! chooses `desc` (ICC < 4.0) or `mluc` (ICC ≥ 4.0) — the sole place a handler consults the
//! profile's ICC version.

use std::io::{Read, Seek, Write};

use crate::error::{Error, ErrorCode, Result};
use crate::io::endian::{read_u32, read_u64, write_u32, write_u64};
use crate::named::dict::Seq;
use crate::named::mlu::Mlu;
use crate::types::sig::Signature;

use super::super::handler::{
    read_bytes, read_position_table, tell, type_sig, write_bytes, write_position_table,
    write_type_base, wrong_variant,
};
use super::super::value::TagValue;
use super::mlu as mlutypes;

/// `SaveDescription` — write an embedded text description as `desc` (V2) or `mluc` (V4).
///
/// A missing description (`None`) writes the empty form of the chosen type, matching the C
/// handling of a NULL `cmsMLU*`.
fn save_description<W: Write + Seek>(io: &mut W, mlu: Option<&Mlu>, icc_version: f64) -> Result<()> {
    let empty = Mlu::new();
    let m = mlu.unwrap_or(&empty);
    if icc_version < 4.0 {
        write_type_base(io, type_sig::TEXT_DESCRIPTION)?;
        mlutypes::write_text_description_object(io, m)
    } else {
        write_type_base(io, type_sig::MLU)?;
        mlutypes::write_mlu_object(io, m)
    }
}

// --- cmsSigProfileSequenceDescType ------------------------------------------------------

/// `Type_ProfileSequenceDesc_Read`.
pub(crate) fn read_profile_sequence_desc<R: Read + Seek>(
    io: &mut R,
    size_of_tag: u32,
) -> Result<TagValue> {
    let count = read_u32(io)?;
    if size_of_tag < 4 {
        return Err(Error::new(ErrorCode::Read, "pseq too small"));
    }
    let mut remaining = size_of_tag - 4;

    let mut out = Seq::new(count as usize)?;

    for i in 0..count as usize {
        let device_mfg = read_u32(io)?;
        remaining = dec(remaining, 4)?;
        let device_model = read_u32(io)?;
        remaining = dec(remaining, 4)?;
        let attributes = read_u64(io)?;
        remaining = dec(remaining, 8)?;
        let technology = read_u32(io)?;
        remaining = dec(remaining, 4)?;

        let manufacturer = mlutypes::read_embedded_text(io, remaining)?;
        let model = mlutypes::read_embedded_text(io, remaining)?;

        let sec = &mut out.seq[i];
        sec.device_mfg = Signature(device_mfg);
        sec.device_model = Signature(device_model);
        sec.attributes = attributes;
        sec.technology = Signature(technology);
        sec.manufacturer = Some(manufacturer);
        sec.model = Some(model);
    }

    Ok(TagValue::Seq(out))
}

/// Decrement `remaining` by `n`, erroring on underflow (the C `SizeOfTag < n` guard).
#[inline]
fn dec(remaining: u32, n: u32) -> Result<u32> {
    if remaining < n {
        return Err(Error::new(ErrorCode::Read, "pseq record truncated"));
    }
    Ok(remaining - n)
}

/// `Type_ProfileSequenceDesc_Write`.
pub(crate) fn write_profile_sequence_desc<W: Write + Seek>(
    io: &mut W,
    value: &TagValue,
    icc_version: f64,
) -> Result<()> {
    let seq = as_seq(value)?;
    write_u32(io, seq.seq.len() as u32)?;
    for sec in &seq.seq {
        write_u32(io, sec.device_mfg.as_u32())?;
        write_u32(io, sec.device_model.as_u32())?;
        write_u64(io, sec.attributes)?;
        write_u32(io, sec.technology.as_u32())?;
        save_description(io, sec.manufacturer.as_ref(), icc_version)?;
        save_description(io, sec.model.as_ref(), icc_version)?;
    }
    Ok(())
}

// --- cmsSigProfileSequenceIdType --------------------------------------------------------

/// `Type_ProfileSequenceId_Read` (via `ReadPositionTable` + `ReadSeqID`).
pub(crate) fn read_profile_sequence_id<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    let base_offset = tell(io)?.wrapping_sub(8);
    let count = read_u32(io)?;
    let mut out = Seq::new(count as usize)?;

    {
        let seq = &mut out.seq;
        read_position_table(io, count, base_offset, |io, n, size| {
            let pid = read_bytes(io, 16)?;
            let desc = mlutypes::read_embedded_text(io, size)?;
            let sec = &mut seq[n as usize];
            sec.profile_id.copy_from_slice(&pid);
            sec.description = Some(desc);
            Ok(())
        })?;
    }

    Ok(TagValue::Seq(out))
}

/// `Type_ProfileSequenceId_Write` (via `WritePositionTable` + `WriteSeqID`).
pub(crate) fn write_profile_sequence_id<W: Write + Seek>(
    io: &mut W,
    value: &TagValue,
    icc_version: f64,
) -> Result<()> {
    let seq = as_seq(value)?;
    let base_offset = tell(io)?.wrapping_sub(8);
    write_u32(io, seq.seq.len() as u32)?;

    write_position_table(io, 0, seq.seq.len() as u32, base_offset, |io, n, _size| {
        let sec = &seq.seq[n as usize];
        write_bytes(io, &sec.profile_id)?;
        save_description(io, sec.description.as_ref(), icc_version)
    })
}

/// Borrow the [`Seq`] out of a [`TagValue`].
fn as_seq(value: &TagValue) -> Result<&Seq> {
    match value {
        TagValue::Seq(s) => Ok(s),
        _ => Err(wrong_variant("profile sequence")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::handler::TypeHandler;
    use crate::tags::testutil::round_trip_stable_versioned;

    fn sample_seq() -> Seq {
        let mut seq = Seq::new(2).unwrap();
        for (i, sec) in seq.seq.iter_mut().enumerate() {
            sec.device_mfg = Signature::new(b"ADBE");
            sec.device_model = Signature::new(b"mdl0");
            sec.attributes = 0x0000_0001_0000_0002;
            sec.technology = Signature::new(b"CRT ");
            sec.profile_id = [(i as u8).wrapping_add(1); 16];
            // For the V2 path each embedded `desc` pads to a 4-byte boundary that
            // `Type_Text_Description_Read` (faithfully) does not consume; the pseq reader adds
            // no inter-record alignment, so byte-stable round-tripping requires ASCII lengths
            // with `len_text = strlen + 1 ≡ 2 (mod 4)` — i.e. strlen ≡ 1 (mod 4). Lengths 5
            // (`Maker`/`Model`/`Descr`) satisfy that. This mirrors lcms exactly, which is
            // likewise byte-fragile for V2 pseq with other lengths.
            let mut man = Mlu::new();
            man.set_ascii(*b"en", *b"US", "Maker").unwrap();
            sec.manufacturer = Some(man);
            let mut mdl = Mlu::new();
            mdl.set_ascii(*b"en", *b"US", "Model").unwrap();
            sec.model = Some(mdl);
            let mut desc = Mlu::new();
            desc.set_ascii(*b"en", *b"US", "Descr").unwrap();
            sec.description = Some(desc);
        }
        seq
    }

    #[test]
    fn profile_sequence_desc_v4_round_trip() {
        round_trip_stable_versioned(TypeHandler::ProfileSequenceDesc, &TagValue::Seq(sample_seq()), 4.3);
    }

    #[test]
    fn profile_sequence_desc_v2_round_trip() {
        round_trip_stable_versioned(TypeHandler::ProfileSequenceDesc, &TagValue::Seq(sample_seq()), 2.0);
    }

    #[test]
    fn profile_sequence_id_round_trip() {
        round_trip_stable_versioned(TypeHandler::ProfileSequenceId, &TagValue::Seq(sample_seq()), 4.3);
    }
}
