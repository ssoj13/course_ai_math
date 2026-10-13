//! Shared decoder helpers used by all block-decoder back-ends.
//!
//! Port targets:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/coding/ojph_block_decoder.h`
//! - Shared code from `ojph_block_decoder32.cpp`, `ojph_block_decoder64.cpp`,
//!   `ojph_block_decoder_avx2.cpp`, `ojph_block_decoder_ssse3.cpp`

use core::ptr::read_unaligned;

use crate::openjph::ojph_defs::{Ui32, Ui64, Ui8};

// MEL entropy decoder state (forward reading).
#[allow(non_camel_case_types)]
pub(crate) struct dec_mel_st {
    pub(crate) data: *mut Ui8,
    pub(crate) tmp: Ui64,
    pub(crate) bits: i32,
    pub(crate) size: i32,
    pub(crate) unstuff: bool,
    pub(crate) k: i32,
    pub(crate) num_runs: i32,
    pub(crate) runs: Ui64,
}

impl Default for dec_mel_st {
    fn default() -> Self {
        Self {
            data: core::ptr::null_mut(),
            tmp: 0,
            bits: 0,
            size: 0,
            unstuff: false,
            k: 0,
            num_runs: 0,
            runs: 0,
        }
    }
}

// Backward (reverse) VLC / MRP bitstream reader state.
#[allow(non_camel_case_types)]
pub(crate) struct rev_struct {
    pub(crate) data: *mut Ui8,
    pub(crate) tmp: Ui64,
    pub(crate) bits: Ui32,
    pub(crate) size: i32,
    pub(crate) unstuff: bool,
}

impl Default for rev_struct {
    fn default() -> Self {
        Self {
            data: core::ptr::null_mut(),
            tmp: 0,
            bits: 0,
            size: 0,
            unstuff: false,
        }
    }
}

// ---------------------------------------------------------------------------
// MEL functions
// ---------------------------------------------------------------------------

#[inline]
pub(crate) unsafe fn mel_read(melp: &mut dec_mel_st) {
    if melp.bits > 32 {
        return;
    }

    let mut val: Ui32 = 0xFFFF_FFFF;
    if melp.size > 4 {
        val = unsafe { read_unaligned(melp.data as *const Ui32) };
        melp.data = unsafe { melp.data.add(4) };
        melp.size -= 4;
    } else if melp.size > 0 {
        let mut i = 0;
        while melp.size > 1 {
            let v = unsafe { *melp.data } as Ui32;
            melp.data = unsafe { melp.data.add(1) };
            let m = !(0xFF_u32 << i);
            val = (val & m) | (v << i);
            melp.size -= 1;
            i += 8;
        }

        let mut v = unsafe { *melp.data } as Ui32;
        melp.data = unsafe { melp.data.add(1) };
        v |= 0xF;
        let m = !(0xFF_u32 << i);
        val = (val & m) | (v << i);
        melp.size -= 1;
    }

    let mut bits: i32 = 32 - if melp.unstuff { 1 } else { 0 };

    let mut t = val & 0xFF;
    let mut unstuff = (val & 0xFF) == 0xFF;
    bits -= if unstuff { 1 } else { 0 };
    t <<= 8 - if unstuff { 1 } else { 0 };

    t |= (val >> 8) & 0xFF;
    unstuff = ((val >> 8) & 0xFF) == 0xFF;
    bits -= if unstuff { 1 } else { 0 };
    t <<= 8 - if unstuff { 1 } else { 0 };

    t |= (val >> 16) & 0xFF;
    unstuff = ((val >> 16) & 0xFF) == 0xFF;
    bits -= if unstuff { 1 } else { 0 };
    t <<= 8 - if unstuff { 1 } else { 0 };

    t |= (val >> 24) & 0xFF;
    melp.unstuff = ((val >> 24) & 0xFF) == 0xFF;

    melp.tmp |= (t as Ui64) << ((64 - bits - melp.bits) as u32);
    melp.bits += bits;
}

pub(crate) unsafe fn mel_decode(melp: &mut dec_mel_st) {
    static MEL_EXP: [i32; 13] = [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 4, 5];

    if melp.bits < 6 {
        unsafe { mel_read(melp) };
    }

    while melp.bits >= 6 && melp.num_runs < 8 {
        let mut eval = MEL_EXP[melp.k as usize];
        let run: i32;

        if (melp.tmp & (1_u64 << 63)) != 0 {
            let mut next_run = 1 << eval;
            next_run -= 1;
            melp.k = if melp.k + 1 < 12 { melp.k + 1 } else { 12 };
            melp.tmp <<= 1;
            melp.bits -= 1;
            run = next_run << 1;
        } else {
            let mut next_run = ((melp.tmp >> (63 - eval)) as i32) & ((1 << eval) - 1);
            melp.k = if melp.k - 1 > 0 { melp.k - 1 } else { 0 };
            melp.tmp <<= (eval + 1) as u32;
            melp.bits -= eval + 1;
            next_run = (next_run << 1) + 1;
            run = next_run;
        }

        eval = melp.num_runs * 7;
        melp.runs &= !(0x3F_u64 << (eval as u32));
        melp.runs |= (run as Ui64) << (eval as u32);
        melp.num_runs += 1;
    }
}

#[inline]
pub(crate) unsafe fn mel_init(melp: &mut dec_mel_st, bbuf: *mut Ui8, lcup: i32, scup: i32) {
    melp.data = unsafe { bbuf.add((lcup - scup) as usize) };
    melp.bits = 0;
    melp.tmp = 0;
    melp.unstuff = false;
    melp.size = scup - 1;
    melp.k = 0;
    melp.num_runs = 0;
    melp.runs = 0;

    let num = 4 - ((melp.data as usize) & 0x3) as i32;
    for _ in 0..(num as usize) {
        debug_assert!(!melp.unstuff || unsafe { *melp.data } <= 0x8F);

        let mut d = if melp.size > 0 {
            unsafe { *melp.data as Ui64 }
        } else {
            0xFF
        };
        if melp.size == 1 {
            d |= 0xF;
        }

        let advance = melp.size > 0;
        melp.size -= 1;
        if advance {
            melp.data = unsafe { melp.data.add(1) };
        }

        let d_bits: i32 = 8 - if melp.unstuff { 1 } else { 0 };
        melp.tmp = (melp.tmp << (d_bits as u32)) | d;
        melp.bits += d_bits;
        melp.unstuff = (d & 0xFF) == 0xFF;
    }

    melp.tmp <<= (64 - melp.bits) as u32;
}

#[inline]
pub(crate) unsafe fn mel_get_run(melp: &mut dec_mel_st) -> i32 {
    if melp.num_runs == 0 {
        unsafe { mel_decode(melp) };
    }

    let t = (melp.runs & 0x7F) as i32;
    melp.runs >>= 7;
    melp.num_runs -= 1;
    t
}

// ---------------------------------------------------------------------------
// REV (backward VLC) functions
// ---------------------------------------------------------------------------

#[inline]
pub(crate) unsafe fn rev_read(vlcp: &mut rev_struct) {
    if vlcp.bits > 32 {
        return;
    }

    let mut val: Ui32 = 0xFFFF_FFFF;
    if vlcp.size > 3 {
        val = unsafe { read_unaligned(vlcp.data.sub(3) as *const Ui32) };
        vlcp.data = unsafe { vlcp.data.sub(4) };
        vlcp.size -= 4;
    } else if vlcp.size > 0 {
        let mut i = 24;
        while vlcp.size > 0 {
            let v = unsafe { *vlcp.data } as Ui32;
            vlcp.data = unsafe { vlcp.data.sub(1) };
            let m = !(0xFF_u32 << i);
            val = (val & m) | (v << i);
            vlcp.size -= 1;
            i -= 8;
        }
    }

    let mut tmp = val >> 24;
    let mut bits: Ui32 = 8 - if vlcp.unstuff && (((val >> 24) & 0x7F) == 0x7F) {
        1
    } else {
        0
    };
    let mut unstuff = (val >> 24) > 0x8F;

    tmp |= ((val >> 16) & 0xFF) << bits;
    bits += 8 - if unstuff && (((val >> 16) & 0x7F) == 0x7F) {
        1
    } else {
        0
    };
    unstuff = ((val >> 16) & 0xFF) > 0x8F;

    tmp |= ((val >> 8) & 0xFF) << bits;
    bits += 8 - if unstuff && (((val >> 8) & 0x7F) == 0x7F) {
        1
    } else {
        0
    };
    unstuff = ((val >> 8) & 0xFF) > 0x8F;

    tmp |= (val & 0xFF) << bits;
    bits += 8 - if unstuff && ((val & 0x7F) == 0x7F) {
        1
    } else {
        0
    };
    unstuff = (val & 0xFF) > 0x8F;

    vlcp.tmp |= (tmp as Ui64) << vlcp.bits;
    vlcp.bits += bits;
    vlcp.unstuff = unstuff;
}

#[inline]
pub(crate) unsafe fn rev_init(vlcp: &mut rev_struct, data: *mut Ui8, lcup: i32, scup: i32) {
    // First byte has only the upper 4 bits (lower 4 are part of scup)
    vlcp.data = unsafe { data.add((lcup - 2) as usize) };
    vlcp.size = scup - 2;

    // Read first byte (half-byte of VLC data)
    let d = unsafe { *vlcp.data } as Ui32;
    vlcp.data = unsafe { vlcp.data.sub(1) };
    vlcp.tmp = (d >> 4) as Ui64;
    vlcp.bits = 4 - if (vlcp.tmp as Ui32 & 7) == 7 { 1 } else { 0 };
    vlcp.unstuff = (d | 0xF) > 0x8F;

    let num = 1 + ((vlcp.data as usize) & 0x3) as i32;
    let tnum = if num < vlcp.size { num } else { vlcp.size };
    for _ in 0..(tnum as usize) {
        let d = unsafe { *vlcp.data as Ui64 };
        vlcp.data = unsafe { vlcp.data.sub(1) };
        let d_bits: Ui32 = 8 - if vlcp.unstuff && ((d & 0x7F) == 0x7F) {
            1
        } else {
            0
        };
        vlcp.tmp |= d << vlcp.bits;
        vlcp.bits += d_bits;
        vlcp.unstuff = d > 0x8F;
    }
    vlcp.size -= tnum;
    unsafe { rev_read(vlcp) };
}

#[inline]
pub(crate) unsafe fn rev_fetch(vlcp: &mut rev_struct) -> Ui32 {
    if vlcp.bits < 32 {
        unsafe { rev_read(vlcp) };
        if vlcp.bits < 32 {
            unsafe { rev_read(vlcp) };
        }
    }
    vlcp.tmp as Ui32
}

#[inline]
pub(crate) fn rev_advance(vlcp: &mut rev_struct, num_bits: Ui32) -> Ui32 {
    debug_assert!(num_bits <= vlcp.bits);
    vlcp.tmp >>= num_bits;
    vlcp.bits -= num_bits;
    vlcp.tmp as Ui32
}

// ---------------------------------------------------------------------------
// REV MRP functions
// ---------------------------------------------------------------------------

#[inline]
pub(crate) unsafe fn rev_read_mrp(mrp: &mut rev_struct) {
    if mrp.bits > 32 {
        return;
    }

    let mut val = 0_u32;
    if mrp.size > 3 {
        val = unsafe { read_unaligned(mrp.data.sub(3) as *const Ui32) };
        mrp.data = unsafe { mrp.data.sub(4) };
        mrp.size -= 4;
    } else if mrp.size > 0 {
        let mut i = 24;
        while mrp.size > 0 {
            let v = unsafe { *mrp.data } as Ui32;
            mrp.data = unsafe { mrp.data.sub(1) };
            val |= v << i;
            mrp.size -= 1;
            i -= 8;
        }
    }

    let mut tmp = val >> 24;
    let mut bits: Ui32 = 8 - if mrp.unstuff && (((val >> 24) & 0x7F) == 0x7F) {
        1
    } else {
        0
    };
    let mut unstuff = (val >> 24) > 0x8F;

    tmp |= ((val >> 16) & 0xFF) << bits;
    bits += 8 - if unstuff && (((val >> 16) & 0x7F) == 0x7F) {
        1
    } else {
        0
    };
    unstuff = ((val >> 16) & 0xFF) > 0x8F;

    tmp |= ((val >> 8) & 0xFF) << bits;
    bits += 8 - if unstuff && (((val >> 8) & 0x7F) == 0x7F) {
        1
    } else {
        0
    };
    unstuff = ((val >> 8) & 0xFF) > 0x8F;

    tmp |= (val & 0xFF) << bits;
    bits += 8 - if unstuff && ((val & 0x7F) == 0x7F) {
        1
    } else {
        0
    };
    unstuff = (val & 0xFF) > 0x8F;

    mrp.tmp |= (tmp as Ui64) << mrp.bits;
    mrp.bits += bits;
    mrp.unstuff = unstuff;
}

#[inline]
pub(crate) unsafe fn rev_init_mrp(mrp: &mut rev_struct, data: *mut Ui8, lcup: i32, len2: i32) {
    mrp.data = unsafe { data.add((lcup + len2 - 1) as usize) };
    mrp.size = len2;
    mrp.unstuff = true;
    mrp.bits = 0;
    mrp.tmp = 0;

    let num = 1 + ((mrp.data as usize) & 0x3) as i32;
    for _ in 0..(num as usize) {
        let advance = mrp.size > 0;
        let d = if advance {
            unsafe { *mrp.data as Ui64 }
        } else {
            0
        };
        mrp.size -= 1;
        if advance {
            mrp.data = unsafe { mrp.data.sub(1) };
        }

        let d_bits: Ui32 = 8 - if mrp.unstuff && ((d & 0x7F) == 0x7F) {
            1
        } else {
            0
        };
        mrp.tmp |= d << mrp.bits;
        mrp.bits += d_bits;
        mrp.unstuff = d > 0x8F;
    }
    unsafe { rev_read_mrp(mrp) };
}

#[inline]
pub(crate) unsafe fn rev_fetch_mrp(mrp: &mut rev_struct) -> Ui32 {
    if mrp.bits < 32 {
        unsafe { rev_read_mrp(mrp) };
        if mrp.bits < 32 {
            unsafe { rev_read_mrp(mrp) };
        }
    }
    mrp.tmp as Ui32
}

#[inline]
pub(crate) fn rev_advance_mrp(mrp: &mut rev_struct, num_bits: Ui32) -> Ui32 {
    debug_assert!(num_bits <= mrp.bits);
    mrp.tmp >>= num_bits;
    mrp.bits -= num_bits;
    mrp.tmp as Ui32
}
