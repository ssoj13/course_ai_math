//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_bitbuffer_read.h`

use crate::codestream::ojph_codeblock::CodedCbHeader;
use crate::codestream::ojph_params_local::Jp2kMarker;
use crate::openjph::ojph_arch::swap_byte_u16;
use crate::openjph::ojph_defs::{Si64, Ui16, Ui32, ojph_min};
use crate::openjph::ojph_file::{InfileBase, InfileSeek};
use crate::openjph::ojph_mem::{CodedLists, mem_elastic_allocator};
use crate::openjph::ojph_message::OjphResult;

#[repr(C)]
pub struct BitReadBuf<'a> {
    pub file: &'a mut dyn InfileBase,
    pub tmp: Ui32,
    pub avail_bits: i32,
    pub unstuff: bool,
    pub bytes_left: Ui32,
}

pub fn bb_init<'a>(bytes_left: Ui32, file: &'a mut dyn InfileBase) -> BitReadBuf<'a> {
    BitReadBuf {
        file,
        tmp: 0,
        avail_bits: 0,
        unstuff: false,
        bytes_left,
    }
}

#[inline]
pub fn bb_read(bbp: &mut BitReadBuf<'_>) -> OjphResult<bool> {
    if bbp.bytes_left > 0 {
        let mut t = 0u8;
        if bbp.file.read(core::slice::from_mut(&mut t)) != 1 {
            return Err(crate::ojph_error!(0x00030072, "error reading from file"));
        }
        bbp.tmp = t as Ui32;
        bbp.avail_bits = 8 - if bbp.unstuff { 1 } else { 0 };
        bbp.unstuff = t == 0xFF;
        bbp.bytes_left -= 1;
        Ok(true)
    } else {
        bbp.tmp = 0;
        bbp.avail_bits = 8 - if bbp.unstuff { 1 } else { 0 };
        bbp.unstuff = false;
        Ok(false)
    }
}

#[inline]
pub fn bb_read_bit(bbp: &mut BitReadBuf<'_>, bit: &mut Ui32) -> OjphResult<bool> {
    let mut result = true;
    if bbp.avail_bits == 0 {
        result = bb_read(bbp)?;
    }
    bbp.avail_bits -= 1;
    *bit = (bbp.tmp >> bbp.avail_bits) & 1;
    Ok(result)
}

#[inline]
pub fn bb_read_bits(bbp: &mut BitReadBuf<'_>, mut num_bits: i32, bits: &mut Ui32) -> OjphResult<bool> {
    // Widths come from packet-header fields (`Lblock + log2(passes)`), so a corrupt stream can ask
    // for more than a `Ui32` holds; that must be an error, not a wrapped shift in release builds.
    if !(0..=32).contains(&num_bits) {
        return Err(crate::ojph_error!(
            0x000300E0,
            "packet header: cannot read {num_bits} bits at once (corrupt codestream)"
        ));
    }
    *bits = 0;
    let mut result = true;
    while num_bits != 0 {
        if bbp.avail_bits == 0 {
            result = bb_read(bbp)?;
        }
        let tx_bits = ojph_min(bbp.avail_bits as Ui32, num_bits as Ui32) as i32;
        *bits <<= tx_bits;
        bbp.avail_bits -= tx_bits;
        num_bits -= tx_bits;
        *bits |= (bbp.tmp >> bbp.avail_bits) & ((1u32 << tx_bits) - 1);
    }
    Ok(result)
}

/// Upper bound on the coded bytes of one code-block. A block has at most 4096 samples, so even
/// incompressible data is tens of KiB (HT segments are additionally capped at 65535 by the packet
/// parsers); 1 MiB leaves a wide margin while stopping a corrupt length from driving a multi-GiB
/// allocation or a `u32` wrap in the buffer-size arithmetic below.
const MAX_CB_BYTES: Ui32 = 1 << 20;

/// Read one code-block's `num_bytes` of coded data into an elastic buffer (zero-padded when the
/// tile-part is truncated). Returns `Ok(false)` when fewer than `num_bytes` bytes could be read,
/// and `Err` when `num_bytes` is impossible for a code-block (corrupt packet header).
#[inline]
pub fn bb_read_chunk(
    bbp: &mut BitReadBuf<'_>,
    num_bytes: Ui32,
    cur_coded_list: &mut *mut CodedLists,
    elastic: *mut mem_elastic_allocator,
) -> OjphResult<bool> {
    debug_assert!(bbp.avail_bits == 0 && !bbp.unstuff);
    if num_bytes > MAX_CB_BYTES {
        return Err(crate::ojph_error!(
            0x000300E1,
            "code-block length {num_bytes} exceeds the {MAX_CB_BYTES}-byte limit (corrupt packet header)"
        ));
    }
    unsafe {
        (*elastic).get_buffer(
            num_bytes
                + CodedCbHeader::PREFIX_BUF_SIZE as Ui32
                + CodedCbHeader::SUFFIX_BUF_SIZE as Ui32,
            cur_coded_list,
        );
        let bytes = ojph_min(num_bytes, bbp.bytes_left);
        let bytes_read = bbp.file.read(core::slice::from_raw_parts_mut(
            (**cur_coded_list)
                .buf
                .add(CodedCbHeader::PREFIX_BUF_SIZE as usize),
            bytes as usize,
        )) as Ui32;
        if num_bytes > bytes_read {
            core::ptr::write_bytes(
                (**cur_coded_list)
                    .buf
                    .add(CodedCbHeader::PREFIX_BUF_SIZE as usize + bytes_read as usize),
                0,
                (num_bytes - bytes_read) as usize,
            );
        }
        bbp.bytes_left -= bytes_read;
        Ok(bytes_read == bytes)
    }
}

// EPH is strict on purpose. p1_06.j2k (SOP+EPH, PPT) is spec-correct: with packed headers the EPH
// markers live inside the PPT payload, not the tile body, and it decodes (tests/conf_ref.rs).
#[inline]
pub fn bb_skip_eph(bbp: &mut BitReadBuf<'_>) -> OjphResult<()> {
    if bbp.bytes_left >= 2 {
        let mut marker = [0u8; 2];
        if bbp.file.read(&mut marker) != 2 {
            return Err(crate::ojph_error!(0x00030073, "error reading from file"));
        }
        bbp.bytes_left -= 2;
        if marker[0] as Ui16 != (Jp2kMarker::Eph as Ui16 >> 8)
            || marker[1] as Ui16 != (Jp2kMarker::Eph as Ui16 & 0xFF)
        {
            return Err(crate::ojph_error!(
                0x00030074,
                "should find EPH, but found something else"
            ));
        }
    }
    Ok(())
}

#[inline]
pub fn bb_terminate(bbp: &mut BitReadBuf<'_>, uses_eph: bool) -> OjphResult<bool> {
    let mut result = true;
    if bbp.unstuff {
        result = bb_read(bbp)?;
    }
    debug_assert!(!bbp.unstuff);
    if uses_eph {
        bb_skip_eph(bbp)?;
    }
    bbp.tmp = 0;
    bbp.avail_bits = 0;
    Ok(result)
}

#[inline]
pub fn bb_skip_sop(bbp: &mut BitReadBuf<'_>) -> OjphResult<bool> {
    if bbp.bytes_left >= 2 {
        let mut marker = [0u8; 2];
        if bbp.file.read(&mut marker) != 2 {
            return Err(crate::ojph_error!(0x00030075, "error reading from file"));
        }
        if marker[0] as Ui16 == (Jp2kMarker::Sop as Ui16 >> 8)
            && marker[1] as Ui16 == (Jp2kMarker::Sop as Ui16 & 0xFF)
        {
            bbp.bytes_left -= 2;
            if bbp.bytes_left >= 4 {
                let mut com_len = [0u8; 2];
                if bbp.file.read(&mut com_len) != 2 {
                    return Err(crate::ojph_error!(0x00030076, "error reading from file"));
                }
                let com_len = swap_byte_u16(Ui16::from_ne_bytes(com_len));
                if com_len != 4 {
                    return Err(crate::ojph_error!(
                        0x00030077,
                        "something is wrong with SOP length"
                    ));
                }
                let result = bbp
                    .file
                    .seek((com_len - 2) as Si64, InfileSeek::OjphSeekCur);
                if result != 0 {
                    return Err(crate::ojph_error!(0x00030078, "error seeking file"));
                }
                bbp.bytes_left -= com_len as Ui32;
            } else {
                return Err(crate::ojph_error!(0x00030079, "precinct truncated early"));
            }
            return Ok(true);
        } else {
            if bbp.file.seek(-2, InfileSeek::OjphSeekCur) != 0 {
                return Err(crate::ojph_error!(0x0003007A, "error seeking file"));
            }
            return Ok(false);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openjph::ojph_file::MemInfile;

    /// Header-derived bit widths above 32 are a corrupt-stream error, not a wrapped shift.
    #[test]
    fn read_bits_rejects_widths_over_32() {
        let data = [0xAAu8; 8];
        let mut infile = MemInfile::default();
        infile.open(&data);
        let mut bb = bb_init(data.len() as Ui32, &mut infile);
        let mut v = 0;
        assert!(bb_read_bits(&mut bb, 33, &mut v).is_err());
        assert!(bb_read_bits(&mut bb, -1, &mut v).is_err());
        assert!(bb_read_bits(&mut bb, 8, &mut v).unwrap());
        assert_eq!(v, 0xAA);
    }

    /// A claimed code-block length above the sanity limit is rejected *before* any allocation
    /// (the null allocator would crash otherwise) and before any byte is consumed.
    #[test]
    fn read_chunk_rejects_impossible_length_before_allocating() {
        let data = [0u8; 4];
        let mut infile = MemInfile::default();
        infile.open(&data);
        let mut bb = bb_init(data.len() as Ui32, &mut infile);
        let mut list: *mut CodedLists = core::ptr::null_mut();
        let r = bb_read_chunk(&mut bb, MAX_CB_BYTES + 1, &mut list, core::ptr::null_mut());
        assert!(r.is_err());
        assert_eq!(bb.bytes_left, 4);
    }
}
