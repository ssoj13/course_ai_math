//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_bitbuffer_write.h`

use crate::openjph::ojph_defs::{Ui32, Ui64};
use crate::openjph::ojph_mem::{CodedLists, mem_elastic_allocator};

#[repr(C)]
pub struct BitWriteBuf {
    pub ccl: *mut CodedLists,
    pub avail_bits: i32,
    pub tmp: Ui64,
}

impl Default for BitWriteBuf {
    fn default() -> Self {
        Self {
            ccl: core::ptr::null_mut(),
            avail_bits: 0,
            tmp: 0,
        }
    }
}

impl BitWriteBuf {
    pub const NEEDED: i32 = 512;
}

#[inline]
pub fn bb_expand_buf(
    bbp: &mut BitWriteBuf,
    elastic: *mut mem_elastic_allocator,
    cur_coded_list: &mut *mut CodedLists,
) {
    debug_assert!((*cur_coded_list).is_null());
    unsafe {
        (*elastic).get_buffer(BitWriteBuf::NEEDED as Ui32, cur_coded_list);
    }
    bbp.ccl = *cur_coded_list;
    bbp.tmp = 0;
}

#[inline]
pub fn bb_init(
    bbp: &mut BitWriteBuf,
    elastic: *mut mem_elastic_allocator,
    cur_coded_list: &mut *mut CodedLists,
) {
    bb_expand_buf(bbp, elastic, cur_coded_list);
    bbp.avail_bits = 8;
}

#[inline]
pub fn bb_put_bit(
    bbp: &mut BitWriteBuf,
    bit: Ui32,
    elastic: *mut mem_elastic_allocator,
    cur_coded_list: &mut *mut CodedLists,
    ph_bytes: &mut Ui32,
) {
    bbp.avail_bits -= 1;
    bbp.tmp |= ((bit & 1) as Ui64) << bbp.avail_bits;
    if bbp.avail_bits <= 0 {
        unsafe {
            bbp.avail_bits = 8 - if bbp.tmp != 0xFF { 0 } else { 1 };
            let idx = (*bbp.ccl).buf_size - (*bbp.ccl).avail_size;
            *(*bbp.ccl).buf.add(idx as usize) = (bbp.tmp & 0xFF) as u8;
            bbp.tmp = 0;
            (*bbp.ccl).avail_size -= 1;
            if (*bbp.ccl).avail_size == 0 {
                bb_expand_buf(bbp, elastic, &mut (**cur_coded_list).next_list);
                *cur_coded_list = (**cur_coded_list).next_list;
                *ph_bytes += BitWriteBuf::NEEDED as Ui32;
            }
        }
    }
}

#[inline]
pub fn bb_put_zeros(
    bbp: &mut BitWriteBuf,
    num_zeros: i32,
    elastic: *mut mem_elastic_allocator,
    cur_coded_list: &mut *mut CodedLists,
    ph_bytes: &mut Ui32,
) {
    for _ in 0..num_zeros {
        bb_put_bit(bbp, 0, elastic, cur_coded_list, ph_bytes);
    }
}

#[inline]
pub fn bb_put_bits(
    bbp: &mut BitWriteBuf,
    data: Ui32,
    num_bits: i32,
    elastic: *mut mem_elastic_allocator,
    cur_coded_list: &mut *mut CodedLists,
    ph_bytes: &mut Ui32,
) {
    debug_assert!(num_bits <= 32);
    for i in (0..num_bits).rev() {
        bb_put_bit(bbp, data >> i, elastic, cur_coded_list, ph_bytes);
    }
}

#[inline]
pub fn bb_terminate(bbp: &mut BitWriteBuf) {
    if bbp.avail_bits < 8 {
        unsafe {
            let idx = (*bbp.ccl).buf_size - (*bbp.ccl).avail_size;
            *(*bbp.ccl).buf.add(idx as usize) = (bbp.tmp & 0xFF) as u8;
            (*bbp.ccl).avail_size -= 1;
        }
    }
}
