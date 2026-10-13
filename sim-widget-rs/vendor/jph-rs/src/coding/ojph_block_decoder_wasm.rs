//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/coding/ojph_block_decoder_wasm.cpp`

#[cfg(target_arch = "wasm32")]
use core::ptr::write_unaligned;
use core::ptr::{null_mut, read_unaligned};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ojph_warn;
use crate::openjph::ojph_arch::count_leading_zeros_u32;
use crate::openjph::ojph_defs::{Ui16, Ui32, Ui64, Ui8};

use super::ojph_block_common::{uvlc_tbl0, uvlc_tbl1, vlc_tbl0, vlc_tbl1};

#[cfg(target_arch = "wasm32")]
use core::arch::wasm32::{
    i16x8_add, i16x8_eq, i16x8_max, i16x8_mul, i16x8_shl, i16x8_shuffle, i16x8_splat, i16x8_sub,
    i32x4_add, i32x4_eq, i32x4_gt, i32x4_shl, i32x4_shuffle, i32x4_splat, i32x4_sub, i64x2_make,
    i64x2_shl, i64x2_shuffle, i8x16_abs, i8x16_add, i8x16_bitmask, i8x16_eq, i8x16_gt, i8x16_splat,
    i8x16_swizzle, u16x8_extract_lane, u16x8_shr, u32x4_extract_lane, u32x4_shr, u64x2_shr,
    u8x16_extract_lane, u8x16_min, v128, v128_and, v128_andnot, v128_load, v128_or, v128_store,
    v128_xor,
};

#[allow(non_camel_case_types)]
struct dec_mel_st {
    data: *mut Ui8,
    tmp: Ui64,
    bits: i32,
    size: i32,
    unstuff: bool,
    k: i32,
    num_runs: i32,
    runs: Ui64,
}

#[inline]
unsafe fn decode_step1_vlc_mel_wasm(
    coded_data: *mut Ui8,
    lcup: i32,
    scup: i32,
    width: Ui32,
    height: Ui32,
    sstr: Ui32,
    scratch: &mut [Ui16],
) {
    let mut mel = dec_mel_st::default();
    unsafe { mel_init(&mut mel, coded_data, lcup, scup) };
    let mut vlc = rev_struct::default();
    unsafe { rev_init(&mut vlc, coded_data, lcup, scup) };

    let mut run = unsafe { mel_get_run(&mut mel) };
    let mut vlc_val: Ui32;
    let mut c_q: Ui32 = 0;
    let mut sp = 0usize;

    let mut x = 0u32;
    while x < width {
        vlc_val = unsafe { rev_fetch(&mut vlc) };
        let mut t0 = vlc_tbl0[(c_q + (vlc_val & 0x7F)) as usize];

        if c_q == 0 {
            run -= 2;
            t0 = if run == -1 { t0 } else { 0 };
            if run < 0 {
                run = unsafe { mel_get_run(&mut mel) };
            }
        }
        scratch[sp] = t0;
        x += 2;

        c_q = ((t0 as Ui32 & 0x10) << 3) | ((t0 as Ui32 & 0xE0) << 2);
        vlc_val = rev_advance(&mut vlc, t0 as Ui32 & 0x7);

        let mut t1 = vlc_tbl0[(c_q + (vlc_val & 0x7F)) as usize];
        if c_q == 0 && x < width {
            run -= 2;
            t1 = if run == -1 { t1 } else { 0 };
            if run < 0 {
                run = unsafe { mel_get_run(&mut mel) };
            }
        }
        t1 = if x < width { t1 } else { 0 };
        scratch[sp + 2] = t1;
        x += 2;

        c_q = ((t1 as Ui32 & 0x10) << 3) | ((t1 as Ui32 & 0xE0) << 2);
        vlc_val = rev_advance(&mut vlc, t1 as Ui32 & 0x7);

        let mut uvlc_mode = ((t0 as Ui32 & 0x8) << 3) | ((t1 as Ui32 & 0x8) << 4);
        if uvlc_mode == 0xC0 {
            run -= 2;
            uvlc_mode += if run == -1 { 0x40 } else { 0 };
            if run < 0 {
                run = unsafe { mel_get_run(&mut mel) };
            }
        }

        let mut uvlc_entry = uvlc_tbl0[(uvlc_mode + (vlc_val & 0x3F)) as usize] as Ui32;
        vlc_val = rev_advance(&mut vlc, uvlc_entry & 0x7);
        uvlc_entry >>= 3;
        let mut len = uvlc_entry & 0xF;
        let tmp = vlc_val & ((1_u32 << len) - 1);
        vlc_val = rev_advance(&mut vlc, len);
        uvlc_entry >>= 4;
        len = uvlc_entry & 0x7;
        uvlc_entry >>= 3;
        let mut u_q = (1 + (uvlc_entry & 7) + (tmp & !(0xFF_u32 << len))) as Ui16;
        scratch[sp + 1] = u_q;
        u_q = (1 + (uvlc_entry >> 3) + (tmp >> len)) as Ui16;
        scratch[sp + 3] = u_q;

        sp += 4;
    }
    scratch[sp] = 0;
    scratch[sp + 1] = 0;

    for y in (2..height).step_by(2) {
        c_q = 0;
        let mut sp = ((y >> 1) * sstr) as usize;
        let prev = sp - sstr as usize;

        let mut x = 0u32;
        while x < width {
            c_q |= (scratch[prev + (sp - ((y >> 1) * sstr) as usize)] as Ui32 & 0xA0) << 2;
            c_q |= (scratch[prev + (sp - ((y >> 1) * sstr) as usize) + 2] as Ui32 & 0x20) << 4;

            vlc_val = unsafe { rev_fetch(&mut vlc) };
            let mut t0 = vlc_tbl1[(c_q + (vlc_val & 0x7F)) as usize];

            if c_q == 0 {
                run -= 2;
                t0 = if run == -1 { t0 } else { 0 };
                if run < 0 {
                    run = unsafe { mel_get_run(&mut mel) };
                }
            }
            scratch[sp] = t0;
            x += 2;

            c_q = ((t0 as Ui32 & 0x40) << 2) | ((t0 as Ui32 & 0x80) << 1);
            c_q |= scratch[sp - sstr as usize] as Ui32 & 0x80;
            c_q |= (scratch[sp - sstr as usize + 2] as Ui32 & 0xA0) << 2;
            c_q |= (scratch[sp - sstr as usize + 4] as Ui32 & 0x20) << 4;

            vlc_val = rev_advance(&mut vlc, t0 as Ui32 & 0x7);

            let mut t1 = vlc_tbl1[(c_q + (vlc_val & 0x7F)) as usize];
            if c_q == 0 && x < width {
                run -= 2;
                t1 = if run == -1 { t1 } else { 0 };
                if run < 0 {
                    run = unsafe { mel_get_run(&mut mel) };
                }
            }
            t1 = if x < width { t1 } else { 0 };
            scratch[sp + 2] = t1;
            x += 2;

            c_q = ((t1 as Ui32 & 0x40) << 2) | ((t1 as Ui32 & 0x80) << 1);
            c_q |= scratch[sp + 2 - sstr as usize] as Ui32 & 0x80;

            vlc_val = rev_advance(&mut vlc, t1 as Ui32 & 0x7);

            let uvlc_mode = ((t0 as Ui32 & 0x8) << 3) | ((t1 as Ui32 & 0x8) << 4);
            let mut uvlc_entry = uvlc_tbl1[(uvlc_mode + (vlc_val & 0x3F)) as usize] as Ui32;
            vlc_val = rev_advance(&mut vlc, uvlc_entry & 0x7);
            uvlc_entry >>= 3;
            let mut len = uvlc_entry & 0xF;
            let tmp = vlc_val & ((1_u32 << len) - 1);
            vlc_val = rev_advance(&mut vlc, len);
            uvlc_entry >>= 4;
            len = uvlc_entry & 0x7;
            uvlc_entry >>= 3;
            let mut u_q = ((uvlc_entry & 7) + (tmp & !(0xFF_u32 << len))) as Ui16;
            scratch[sp + 1] = u_q;
            u_q = ((uvlc_entry >> 3) + (tmp >> len)) as Ui16;
            scratch[sp + 3] = u_q;

            sp += 4;
        }
        scratch[sp] = 0;
        scratch[sp + 1] = 0;
    }
}

impl Default for dec_mel_st {
    fn default() -> Self {
        Self {
            data: null_mut(),
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

#[allow(non_camel_case_types)]
struct rev_struct {
    data: *mut Ui8,
    tmp: Ui64,
    bits: Ui32,
    size: i32,
    unstuff: bool,
}

impl Default for rev_struct {
    fn default() -> Self {
        Self {
            data: null_mut(),
            tmp: 0,
            bits: 0,
            size: 0,
            unstuff: false,
        }
    }
}

#[allow(non_camel_case_types)]
struct frwd_struct {
    data: *const Ui8,
    tmp: [Ui8; 48],
    bits: Ui32,
    unstuff: Ui32,
    size: i32,
}

impl Default for frwd_struct {
    fn default() -> Self {
        Self {
            data: core::ptr::null(),
            tmp: [0; 48],
            bits: 0,
            unstuff: 0,
            size: 0,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(non_camel_case_types)]
struct frwd_struct32_scalar {
    data: *const Ui8,
    tmp: Ui64,
    bits: Ui32,
    unstuff: Ui32,
    size: i32,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for frwd_struct32_scalar {
    fn default() -> Self {
        Self {
            data: core::ptr::null(),
            tmp: 0,
            bits: 0,
            unstuff: 0,
            size: 0,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
unsafe fn frwd_read_32_scalar<const X: i32>(msp: &mut frwd_struct32_scalar) {
    debug_assert!(msp.bits <= 32);

    let val: Ui32;
    if msp.size > 3 {
        val = unsafe { read_unaligned(msp.data as *const Ui32) };
        msp.data = unsafe { msp.data.add(4) };
        msp.size -= 4;
    } else if msp.size > 0 {
        let mut i = 0;
        let mut v32 = if X != 0 { 0xFFFF_FFFF } else { 0 };
        while msp.size > 0 {
            let v = unsafe { *msp.data } as Ui32;
            msp.data = unsafe { msp.data.add(1) };
            let m = !(0xFF_u32 << i);
            v32 = (v32 & m) | (v << i);
            msp.size -= 1;
            i += 8;
        }
        val = v32;
    } else {
        val = if X != 0 { 0xFFFF_FFFF } else { 0 };
    }

    let mut bits: Ui32 = 8 - msp.unstuff;
    let mut t = val & 0xFF;
    let mut unstuff = (val & 0xFF) == 0xFF;

    t |= ((val >> 8) & 0xFF) << bits;
    bits += 8 - if unstuff { 1 } else { 0 };
    unstuff = ((val >> 8) & 0xFF) == 0xFF;

    t |= ((val >> 16) & 0xFF) << bits;
    bits += 8 - if unstuff { 1 } else { 0 };
    unstuff = ((val >> 16) & 0xFF) == 0xFF;

    t |= ((val >> 24) & 0xFF) << bits;
    bits += 8 - if unstuff { 1 } else { 0 };
    msp.unstuff = if ((val >> 24) & 0xFF) == 0xFF { 1 } else { 0 };

    msp.tmp |= (t as Ui64) << msp.bits;
    msp.bits += bits;
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
unsafe fn frwd_init_32_scalar<const X: i32>(
    msp: &mut frwd_struct32_scalar,
    data: *const Ui8,
    size: i32,
) {
    msp.data = data;
    msp.tmp = 0;
    msp.bits = 0;
    msp.unstuff = 0;
    msp.size = size;

    let num = 4 - ((msp.data as usize) & 0x3) as i32;
    for _ in 0..(num as usize) {
        let advance = msp.size > 0;
        let d = if advance {
            unsafe { *msp.data as Ui64 }
        } else {
            X as Ui64
        };
        msp.size -= 1;
        if advance {
            msp.data = unsafe { msp.data.add(1) };
        }
        msp.tmp |= d << msp.bits;
        msp.bits += 8 - msp.unstuff;
        msp.unstuff = if (d & 0xFF) == 0xFF { 1 } else { 0 };
    }

    unsafe { frwd_read_32_scalar::<X>(msp) };
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
fn frwd_advance_32_scalar(msp: &mut frwd_struct32_scalar, num_bits: Ui32) {
    debug_assert!(num_bits <= msp.bits);
    msp.tmp >>= num_bits;
    msp.bits -= num_bits;
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
unsafe fn frwd_fetch_32_scalar<const X: i32>(msp: &mut frwd_struct32_scalar) -> Ui32 {
    if msp.bits < 32 {
        unsafe { frwd_read_32_scalar::<X>(msp) };
        if msp.bits < 32 {
            unsafe { frwd_read_32_scalar::<X>(msp) };
        }
    }
    msp.tmp as Ui32
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
unsafe fn decode_magsgn_value_32_scalar(
    magsgn: &mut frwd_struct32_scalar,
    inf: Ui32,
    bit: Ui32,
    u_q: Ui32,
    p: Ui32,
) -> (Ui32, Ui32) {
    if (inf & (1 << (4 + bit))) == 0 {
        return (0, 0);
    }

    let ms_val = unsafe { frwd_fetch_32_scalar::<0xFF>(magsgn) };
    let m_n = u_q - ((inf >> (12 + bit)) & 1);
    frwd_advance_32_scalar(magsgn, m_n);

    let mut val = ms_val << 31;
    let mut v_n = ms_val & ((1 << m_n) - 1);
    v_n |= ((inf >> (8 + bit)) & 1) << m_n;
    v_n |= 1;
    val |= (v_n + 2) << (p - 1);
    (val, v_n)
}

#[inline]
unsafe fn mel_read(melp: &mut dec_mel_st) {
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

    let mut bits = 32 - if melp.unstuff { 1 } else { 0 };
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

#[inline]
unsafe fn mel_decode(melp: &mut dec_mel_st) {
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
unsafe fn mel_init(melp: &mut dec_mel_st, bbuf: *mut Ui8, lcup: i32, scup: i32) {
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

        let d_bits = 8 - if melp.unstuff { 1 } else { 0 };
        melp.tmp = (melp.tmp << (d_bits as u32)) | d;
        melp.bits += d_bits;
        melp.unstuff = (d & 0xFF) == 0xFF;
    }

    melp.tmp <<= (64 - melp.bits) as u32;
}

#[inline]
unsafe fn mel_get_run(melp: &mut dec_mel_st) -> i32 {
    if melp.num_runs == 0 {
        unsafe { mel_decode(melp) };
    }

    let t = (melp.runs & 0x7F) as i32;
    melp.runs >>= 7;
    melp.num_runs -= 1;
    t
}

#[inline]
unsafe fn rev_read(vlcp: &mut rev_struct) {
    if vlcp.bits > 32 {
        return;
    }

    let mut val = 0_u32;
    if vlcp.size > 3 {
        val = unsafe { read_unaligned(vlcp.data.sub(3) as *const Ui32) };
        vlcp.data = unsafe { vlcp.data.sub(4) };
        vlcp.size -= 4;
    } else if vlcp.size > 0 {
        let mut i = 24;
        while vlcp.size > 0 {
            let v = unsafe { *vlcp.data } as Ui32;
            vlcp.data = unsafe { vlcp.data.sub(1) };
            val |= v << i;
            vlcp.size -= 1;
            i -= 8;
        }
    }

    let mut tmp = val >> 24;
    let mut bits = 8 - if vlcp.unstuff && (((val >> 24) & 0x7F) == 0x7F) {
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
unsafe fn rev_init(vlcp: &mut rev_struct, data: *mut Ui8, lcup: i32, scup: i32) {
    vlcp.data = unsafe { data.add((lcup - 2) as usize) };
    vlcp.size = scup - 2;

    let d = unsafe { *vlcp.data } as Ui32;
    vlcp.data = unsafe { vlcp.data.sub(1) };
    vlcp.tmp = (d >> 4) as Ui64;
    vlcp.bits = (4 - if (vlcp.tmp & 7) == 7 { 1 } else { 0 }) as Ui32;
    vlcp.unstuff = (d | 0xF) > 0x8F;

    let num = 1 + ((vlcp.data as usize) & 0x3) as i32;
    let tnum = if num < vlcp.size { num } else { vlcp.size };
    for _ in 0..(tnum as usize) {
        let d = unsafe { *vlcp.data as Ui64 };
        vlcp.data = unsafe { vlcp.data.sub(1) };
        let d_bits = 8 - if vlcp.unstuff && ((d & 0x7F) == 0x7F) {
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
unsafe fn rev_fetch(vlcp: &mut rev_struct) -> Ui32 {
    if vlcp.bits < 32 {
        unsafe { rev_read(vlcp) };
        if vlcp.bits < 32 {
            unsafe { rev_read(vlcp) };
        }
    }
    vlcp.tmp as Ui32
}

#[inline]
fn rev_advance(vlcp: &mut rev_struct, num_bits: Ui32) -> Ui32 {
    debug_assert!(num_bits <= vlcp.bits);
    vlcp.tmp >>= num_bits;
    vlcp.bits -= num_bits;
    vlcp.tmp as Ui32
}

#[inline]
unsafe fn rev_read_mrp(mrp: &mut rev_struct) {
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
    let mut bits = 8 - if mrp.unstuff && (((val >> 24) & 0x7F) == 0x7F) {
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
unsafe fn rev_init_mrp(mrp: &mut rev_struct, data: *mut Ui8, lcup: i32, len2: i32) {
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

        let d_bits = 8 - if mrp.unstuff && ((d & 0x7F) == 0x7F) {
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
unsafe fn rev_fetch_mrp(mrp: &mut rev_struct) -> Ui32 {
    if mrp.bits < 32 {
        unsafe { rev_read_mrp(mrp) };
        if mrp.bits < 32 {
            unsafe { rev_read_mrp(mrp) };
        }
    }
    mrp.tmp as Ui32
}

#[inline]
fn rev_advance_mrp(mrp: &mut rev_struct, num_bits: Ui32) -> Ui32 {
    debug_assert!(num_bits <= mrp.bits);
    mrp.tmp >>= num_bits;
    mrp.bits -= num_bits;
    mrp.tmp as Ui32
}

#[cfg(target_arch = "wasm32")]
static FRWD_OFFSET_BYTES: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

#[cfg(target_arch = "wasm32")]
static FRWD_ALL_FF_BYTES: [u8; 16] = [0xFF; 16];

#[cfg(target_arch = "wasm32")]
static DECODE_ONE_Q32_FLAGS_MASK: [u32; 4] = [0x1110, 0x2220, 0x4440, 0x8880];
#[cfg(target_arch = "wasm32")]
static DECODE_ONE_Q32_BYTE_SWIZZLE0: [u32; 4] = [0x00000000, 0x04040404, 0x08080808, 0x0C0C0C0C];
#[cfg(target_arch = "wasm32")]
static DECODE_ONE_Q32_ADD_BASE: [u32; 4] = [0x03020100; 4];
#[cfg(target_arch = "wasm32")]
static DECODE_ONE_Q32_ADD_ONE: [u32; 4] = [0x01010101; 4];
#[cfg(target_arch = "wasm32")]
static DECODE_ONE_Q32_BITMASK7: [u32; 4] = [7; 4];
#[cfg(target_arch = "wasm32")]
static DECODE_ONE_Q32_SHIFT_LUT: [u8; 16] =
    [255, 127, 63, 31, 15, 7, 3, 1, 255, 127, 63, 31, 15, 7, 3, 1];
#[cfg(target_arch = "wasm32")]
static DECODE_ONE_Q32_SHIFT_ADD: [u16; 8] = [0x0101; 8];
#[cfg(target_arch = "wasm32")]
static DECODE_ONE_Q32_D1_MASK: [u32; 4] = [0xFF00FF00; 4];
#[cfg(target_arch = "wasm32")]
static DECODE_ONE_Q32_E1_MASK: [u32; 4] = [0x800; 4];
#[cfg(target_arch = "wasm32")]
static DECODE_ONE_Q32_VN_SWIZZLE0: [u32; 4] = [0x07060504, 0x0F0E0D0C, 0xFFFF_FFFF, 0xFFFF_FFFF];
#[cfg(target_arch = "wasm32")]
static DECODE_ONE_Q32_VN_SWIZZLE1: [u32; 4] = [0xFFFF_FFFF, 0x07060504, 0x0F0E0D0C, 0xFFFF_FFFF];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_INF_SWIZZLE: [u16; 8] = [
    0x0100, 0x0100, 0x0100, 0x0100, 0x0504, 0x0504, 0x0504, 0x0504,
];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_FLAGS_MASK: [u16; 8] = [
    0x1110, 0x2220, 0x4440, 0x8880, 0x1110, 0x2220, 0x4440, 0x8880,
];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_FLAGS_MUL: [u16; 8] = [8, 4, 2, 1, 8, 4, 2, 1];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_BITMASK7: [u16; 8] = [7; 8];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_BYTE_SWIZZLE0: [u16; 8] = [
    0x0000, 0x0202, 0x0404, 0x0606, 0x0808, 0x0A0A, 0x0C0C, 0x0E0E,
];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_ADD_0100: [u16; 8] = [0x0100; 8];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_ADD_0101: [u16; 8] = [0x0101; 8];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_D1_MASK: [u16; 8] = [0xFF00; 8];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_E1_MASK: [u16; 8] = [0x800; 8];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_ONES: [u16; 8] = [1; 8];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_TWOS: [u16; 8] = [2; 8];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_HALF0: [i64; 2] = [-1, 0];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_HALF1: [i64; 2] = [0, -1];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_VN_SWIZZLE0: [u16; 8] = [
    0x0302, 0x0706, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF,
];
#[cfg(target_arch = "wasm32")]
static DECODE_TWO_Q16_VN_SWIZZLE1: [u16; 8] = [
    0xFFFF, 0x0B0A, 0x0F0E, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF,
];
#[cfg(target_arch = "wasm32")]
static REARRANGE_SIGMA_WASM_SHUFFLE_MASK: [u8; 16] = [
    0x00, 0x04, 0x08, 0x0C, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];
#[cfg(target_arch = "wasm32")]
static WASM_EXPAND_BITS_SWIZZLE: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1];
#[cfg(target_arch = "wasm32")]
static WASM_EXPAND_BITS_MASK: [u64; 2] = [0x8040_2010_0804_0201, 0x8040_2010_0804_0201];
#[cfg(target_arch = "wasm32")]
static WASM_SCAN_SHIFT1: [u8; 16] = [0x80, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
#[cfg(target_arch = "wasm32")]
static WASM_SCAN_SHIFT2: [u8; 16] = [0x80, 0x80, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13];
#[cfg(target_arch = "wasm32")]
static WASM_SCAN_SHIFT4: [u8; 16] = [0x80, 0x80, 0x80, 0x80, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
#[cfg(target_arch = "wasm32")]
static WASM_SCAN_SHIFT8: [u8; 16] = [
    0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0, 1, 2, 3, 4, 5, 6, 7,
];
#[cfg(target_arch = "wasm32")]
static WASM_ROW_SWIZZLE0: [u8; 16] = [
    0, 0xFF, 0xFF, 0xFF, 4, 0xFF, 0xFF, 0xFF, 8, 0xFF, 0xFF, 0xFF, 12, 0xFF, 0xFF, 0xFF,
];
#[cfg(target_arch = "wasm32")]
static WASM_ROW_SWIZZLE1: [u8; 16] = [
    1, 0xFF, 0xFF, 0xFF, 5, 0xFF, 0xFF, 0xFF, 9, 0xFF, 0xFF, 0xFF, 13, 0xFF, 0xFF, 0xFF,
];
#[cfg(target_arch = "wasm32")]
static WASM_ROW_SWIZZLE2: [u8; 16] = [
    2, 0xFF, 0xFF, 0xFF, 6, 0xFF, 0xFF, 0xFF, 10, 0xFF, 0xFF, 0xFF, 14, 0xFF, 0xFF, 0xFF,
];
#[cfg(target_arch = "wasm32")]
static WASM_ROW_SWIZZLE3: [u8; 16] = [
    3, 0xFF, 0xFF, 0xFF, 7, 0xFF, 0xFF, 0xFF, 11, 0xFF, 0xFF, 0xFF, 15, 0xFF, 0xFF, 0xFF,
];

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
unsafe fn frwd_read<const X: u8>(msp: &mut frwd_struct) {
    debug_assert!(msp.bits <= 128);

    let offset = unsafe { v128_load(FRWD_OFFSET_BYTES.as_ptr() as *const v128) };
    let all_xff = unsafe { v128_load(FRWD_ALL_FF_BYTES.as_ptr() as *const v128) };

    let mut val = unsafe { v128_load(msp.data as *const v128) };
    let bytes = if msp.size >= 16 { 16 } else { msp.size };
    let mut validity = i8x16_splat(bytes as i8);
    msp.data = unsafe { msp.data.add(bytes as usize) };
    msp.size -= bytes;
    let mut bits: Ui32 = 128;

    validity = i8x16_gt(validity, offset);
    if X == 0xFF {
        let t = v128_xor(validity, all_xff);
        val = v128_or(t, val);
    } else {
        debug_assert!(X == 0);
        val = v128_and(validity, val);
    }

    let mut ff_bytes = i8x16_eq(val, all_xff);
    ff_bytes = v128_and(ff_bytes, validity);
    let mut flags = (i8x16_bitmask(ff_bytes) as Ui32) << 1;
    let next_unstuff = flags >> 16;
    flags |= msp.unstuff;
    flags &= 0xFFFF;

    while flags != 0 {
        bits -= 1;
        let loc = 31 - flags.leading_zeros();
        flags ^= 1 << loc;

        let t = i8x16_splat(loc as i8);
        let m = i8x16_gt(offset, t);

        let kept = v128_and(m, val);
        let carry = u64x2_shr(kept, 1);
        let shifted = i64x2_shl(i64x2_shuffle::<1, 2>(kept, i64x2_make(0, 0)), 63);
        let merged = v128_or(shifted, carry);

        val = v128_or(merged, v128_andnot(val, m));
    }

    let cur_bytes = (msp.bits >> 3) as usize;
    let cur_bits = msp.bits & 7;
    let mut b1 = i64x2_shl(val, cur_bits);
    let mut b2 = i64x2_shuffle::<1, 2>(i64x2_make(0, 0), val);
    b2 = u64x2_shr(b2, 64u32 - cur_bits);
    if cur_bits == 0 {
        b2 = i64x2_make(0, 0);
    }
    b1 = v128_or(b1, b2);
    let cur = unsafe { v128_load(msp.tmp.as_ptr().add(cur_bytes) as *const v128) };
    unsafe {
        v128_store(
            msp.tmp.as_mut_ptr().add(cur_bytes) as *mut v128,
            v128_or(b1, cur),
        )
    };

    let consumed_bits = if bits < 128u32 - cur_bits {
        bits
    } else {
        128u32 - cur_bits
    };
    let upper = (u16x8_extract_lane::<7>(val) as i32) >> (consumed_bits as i32 - 112);
    let upper_byte = ((msp.bits + consumed_bits + 7) >> 3) as usize;
    msp.tmp[upper_byte] = upper as Ui8;

    msp.bits += bits;
    msp.unstuff = next_unstuff;
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
unsafe fn frwd_init<const X: u8>(msp: &mut frwd_struct, data: *const Ui8, size: i32) {
    msp.data = data;
    unsafe {
        v128_store(msp.tmp.as_mut_ptr() as *mut v128, i64x2_make(0, 0));
        v128_store(msp.tmp.as_mut_ptr().add(16) as *mut v128, i64x2_make(0, 0));
        v128_store(msp.tmp.as_mut_ptr().add(32) as *mut v128, i64x2_make(0, 0));
    }
    msp.bits = 0;
    msp.unstuff = 0;
    msp.size = size;
    unsafe { frwd_read::<X>(msp) };
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
unsafe fn frwd_advance(msp: &mut frwd_struct, mut num_bits: Ui32) {
    debug_assert!(num_bits > 0 && num_bits <= msp.bits && num_bits < 128);
    msp.bits -= num_bits;

    let p = unsafe { msp.tmp.as_mut_ptr().add(((num_bits >> 3) & 0x18) as usize) as *mut v128 };
    num_bits &= 63;

    let v0 = unsafe { v128_load(p as *const v128) };
    let v1 = unsafe { v128_load(p.add(1) as *const v128) };

    let mut c0 = u64x2_shr(v0, num_bits);
    let mut t = i64x2_shl(i64x2_shuffle::<1, 2>(v0, i64x2_make(0, 0)), 64 - num_bits);
    if num_bits == 0 {
        t = i64x2_make(0, 0);
    }
    c0 = v128_or(c0, t);
    t = i64x2_shl(i64x2_shuffle::<1, 2>(i64x2_make(0, 0), v1), 64 - num_bits);
    if num_bits == 0 {
        t = i64x2_make(0, 0);
    }
    c0 = v128_or(c0, t);
    unsafe { v128_store(msp.tmp.as_mut_ptr() as *mut v128, c0) };

    let mut c1 = u64x2_shr(v1, num_bits);
    t = i64x2_shl(i64x2_shuffle::<1, 2>(v1, i64x2_make(0, 0)), 64 - num_bits);
    if num_bits == 0 {
        t = i64x2_make(0, 0);
    }
    c1 = v128_or(c1, t);
    unsafe { v128_store(msp.tmp.as_mut_ptr().add(16) as *mut v128, c1) };
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
unsafe fn frwd_fetch<const X: u8>(msp: &mut frwd_struct) -> v128 {
    if msp.bits <= 128 {
        unsafe { frwd_read::<X>(msp) };
        if msp.bits <= 128 {
            unsafe { frwd_read::<X>(msp) };
        }
    }
    unsafe { v128_load(msp.tmp.as_ptr() as *const v128) }
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
unsafe fn decode_one_quad32<const N: usize>(
    inf_u_q: v128,
    mut u_q: v128,
    magsgn: &mut frwd_struct,
    p: Ui32,
    vn: &mut v128,
) -> v128 {
    let zero = i64x2_make(0, 0);
    let mut row = zero;
    let mut w0 = i32x4_shuffle::<N, N, N, N>(inf_u_q, inf_u_q);
    let flags_mask = unsafe { v128_load(DECODE_ONE_Q32_FLAGS_MASK.as_ptr() as *const v128) };
    let mut flags = v128_and(w0, flags_mask);
    let insig = i32x4_eq(flags, zero);

    if i8x16_bitmask(insig) != 0xFFFF {
        u_q = i32x4_shuffle::<N, N, N, N>(u_q, u_q);
        let mul = unsafe { v128_load([8u16, 8, 4, 4, 2, 2, 1, 1].as_ptr() as *const v128) };
        flags = i16x8_mul(flags, mul);
        let mut ms_vec = unsafe { frwd_fetch::<0xFF>(magsgn) };

        w0 = u32x4_shr(flags, 15);
        let mut m_n = i32x4_sub(u_q, w0);
        m_n = v128_andnot(m_n, insig);

        let mut inc_sum = m_n;
        let mut shfl = i32x4_shuffle::<3, 4, 5, 6>(zero, inc_sum);
        inc_sum = i32x4_add(inc_sum, shfl);
        shfl = i64x2_shuffle::<1, 2>(zero, inc_sum);
        inc_sum = i32x4_add(inc_sum, shfl);
        let total_mn = u16x8_extract_lane::<6>(inc_sum) as Ui32;
        let ex_sum = i32x4_shuffle::<3, 4, 5, 6>(zero, inc_sum);

        let mut byte_idx = u32x4_shr(ex_sum, 3);
        let mut bit_idx = v128_and(ex_sum, unsafe {
            v128_load(DECODE_ONE_Q32_BITMASK7.as_ptr() as *const v128)
        });
        byte_idx = i8x16_swizzle(byte_idx, unsafe {
            v128_load(DECODE_ONE_Q32_BYTE_SWIZZLE0.as_ptr() as *const v128)
        });
        byte_idx = i32x4_add(byte_idx, unsafe {
            v128_load(DECODE_ONE_Q32_ADD_BASE.as_ptr() as *const v128)
        });
        let mut d0 = i8x16_swizzle(ms_vec, byte_idx);
        byte_idx = i32x4_add(byte_idx, unsafe {
            v128_load(DECODE_ONE_Q32_ADD_ONE.as_ptr() as *const v128)
        });
        let mut d1 = i8x16_swizzle(ms_vec, byte_idx);

        bit_idx = v128_or(bit_idx, i32x4_shl(bit_idx, 16));
        let mut bit_shift = i8x16_swizzle(
            unsafe { v128_load(DECODE_ONE_Q32_SHIFT_LUT.as_ptr() as *const v128) },
            bit_idx,
        );
        bit_shift = i16x8_add(bit_shift, unsafe {
            v128_load(DECODE_ONE_Q32_SHIFT_ADD.as_ptr() as *const v128)
        });
        d0 = i16x8_mul(d0, bit_shift);
        d0 = u16x8_shr(d0, 8);
        d1 = i16x8_mul(d1, bit_shift);
        d1 = v128_and(d1, unsafe {
            v128_load(DECODE_ONE_Q32_D1_MASK.as_ptr() as *const v128)
        });
        d0 = v128_or(d0, d1);

        let ones = i32x4_splat(1);
        let twos = i32x4_splat(2);
        let u_q_m1 = u32x4_extract_lane::<0>(u_q) - 1;
        w0 = i32x4_sub(twos, w0);
        let shift = i32x4_shl(w0, u_q_m1);
        ms_vec = v128_and(d0, i32x4_sub(shift, ones));

        w0 = v128_and(flags, unsafe {
            v128_load(DECODE_ONE_Q32_E1_MASK.as_ptr() as *const v128)
        });
        w0 = i32x4_eq(w0, zero);
        w0 = v128_andnot(shift, w0);
        ms_vec = v128_or(ms_vec, w0);
        w0 = i32x4_shl(ms_vec, 31);
        ms_vec = v128_or(ms_vec, ones);
        let mut tvn = ms_vec;
        ms_vec = i32x4_add(ms_vec, twos);
        ms_vec = i32x4_shl(ms_vec, p - 1);
        ms_vec = v128_or(ms_vec, w0);
        row = v128_andnot(ms_vec, insig);

        ms_vec = v128_andnot(tvn, insig);
        tvn = if N == 0 {
            i8x16_swizzle(ms_vec, unsafe {
                v128_load(DECODE_ONE_Q32_VN_SWIZZLE0.as_ptr() as *const v128)
            })
        } else {
            i8x16_swizzle(ms_vec, unsafe {
                v128_load(DECODE_ONE_Q32_VN_SWIZZLE1.as_ptr() as *const v128)
            })
        };
        *vn = v128_or(*vn, tvn);

        if total_mn != 0 {
            unsafe { frwd_advance(magsgn, total_mn) };
        }
    }

    row
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
unsafe fn decode_two_quad16(
    inf_u_q: v128,
    mut u_q: v128,
    magsgn: &mut frwd_struct,
    p: Ui32,
    vn: &mut v128,
) -> v128 {
    let zero = i64x2_make(0, 0);
    let mut row = zero;
    let mut w0 = i8x16_swizzle(inf_u_q, unsafe {
        v128_load(DECODE_TWO_Q16_INF_SWIZZLE.as_ptr() as *const v128)
    });
    let mut flags = v128_and(w0, unsafe {
        v128_load(DECODE_TWO_Q16_FLAGS_MASK.as_ptr() as *const v128)
    });
    let insig = i16x8_eq(flags, zero);

    if i8x16_bitmask(insig) != 0xFFFF {
        u_q = i8x16_swizzle(u_q, unsafe {
            v128_load(DECODE_TWO_Q16_INF_SWIZZLE.as_ptr() as *const v128)
        });
        flags = i16x8_mul(flags, unsafe {
            v128_load(DECODE_TWO_Q16_FLAGS_MUL.as_ptr() as *const v128)
        });
        let mut ms_vec = unsafe { frwd_fetch::<0xFF>(magsgn) };

        w0 = u16x8_shr(flags, 15);
        let mut m_n = i16x8_sub(u_q, w0);
        m_n = v128_andnot(m_n, insig);

        let mut inc_sum = m_n;
        let mut shfl = i16x8_shuffle::<7, 8, 9, 10, 11, 12, 13, 14>(zero, inc_sum);
        inc_sum = i16x8_add(inc_sum, shfl);
        shfl = i32x4_shuffle::<3, 4, 5, 6>(zero, inc_sum);
        inc_sum = i16x8_add(inc_sum, shfl);
        shfl = i64x2_shuffle::<1, 2>(zero, inc_sum);
        inc_sum = i16x8_add(inc_sum, shfl);
        let total_mn = u16x8_extract_lane::<7>(inc_sum) as Ui32;
        let ex_sum = i16x8_shuffle::<7, 8, 9, 10, 11, 12, 13, 14>(zero, inc_sum);

        let mut byte_idx = u16x8_shr(ex_sum, 3);
        let bit_idx = v128_and(ex_sum, unsafe {
            v128_load(DECODE_TWO_Q16_BITMASK7.as_ptr() as *const v128)
        });
        byte_idx = i8x16_swizzle(byte_idx, unsafe {
            v128_load(DECODE_TWO_Q16_BYTE_SWIZZLE0.as_ptr() as *const v128)
        });
        byte_idx = i16x8_add(byte_idx, unsafe {
            v128_load(DECODE_TWO_Q16_ADD_0100.as_ptr() as *const v128)
        });
        let mut d0 = i8x16_swizzle(ms_vec, byte_idx);
        byte_idx = i16x8_add(byte_idx, unsafe {
            v128_load(DECODE_TWO_Q16_ADD_0101.as_ptr() as *const v128)
        });
        let mut d1 = i8x16_swizzle(ms_vec, byte_idx);

        let mut bit_shift = i8x16_swizzle(
            unsafe { v128_load(DECODE_ONE_Q32_SHIFT_LUT.as_ptr() as *const v128) },
            bit_idx,
        );
        bit_shift = i16x8_add(bit_shift, unsafe {
            v128_load(DECODE_ONE_Q32_SHIFT_ADD.as_ptr() as *const v128)
        });
        d0 = i16x8_mul(d0, bit_shift);
        d0 = u16x8_shr(d0, 8);
        d1 = i16x8_mul(d1, bit_shift);
        d1 = v128_and(d1, unsafe {
            v128_load(DECODE_TWO_Q16_D1_MASK.as_ptr() as *const v128)
        });
        d0 = v128_or(d0, d1);

        let ones = unsafe { v128_load(DECODE_TWO_Q16_ONES.as_ptr() as *const v128) };
        let twos = unsafe { v128_load(DECODE_TWO_Q16_TWOS.as_ptr() as *const v128) };
        let uq0 = u16x8_extract_lane::<0>(u_q) as Ui32 - 1;
        let uq1 = u16x8_extract_lane::<4>(u_q) as Ui32 - 1;
        w0 = i16x8_sub(twos, w0);
        let mut t0 = v128_and(w0, unsafe {
            v128_load(DECODE_TWO_Q16_HALF0.as_ptr() as *const v128)
        });
        let mut t1 = v128_and(w0, unsafe {
            v128_load(DECODE_TWO_Q16_HALF1.as_ptr() as *const v128)
        });
        t0 = i32x4_shl(t0, uq0);
        t1 = i32x4_shl(t1, uq1);
        let shift = v128_or(t0, t1);
        ms_vec = v128_and(d0, i16x8_sub(shift, ones));

        w0 = v128_and(flags, unsafe {
            v128_load(DECODE_TWO_Q16_E1_MASK.as_ptr() as *const v128)
        });
        w0 = i16x8_eq(w0, zero);
        w0 = v128_andnot(shift, w0);
        ms_vec = v128_or(ms_vec, w0);
        w0 = i16x8_shl(ms_vec, 15);
        ms_vec = v128_or(ms_vec, ones);
        let mut tvn = ms_vec;
        ms_vec = i16x8_add(ms_vec, twos);
        ms_vec = i16x8_shl(ms_vec, p - 1);
        ms_vec = v128_or(ms_vec, w0);
        row = v128_andnot(ms_vec, insig);

        ms_vec = v128_andnot(tvn, insig);
        w0 = i8x16_swizzle(ms_vec, unsafe {
            v128_load(DECODE_TWO_Q16_VN_SWIZZLE0.as_ptr() as *const v128)
        });
        *vn = v128_or(*vn, w0);
        w0 = i8x16_swizzle(ms_vec, unsafe {
            v128_load(DECODE_TWO_Q16_VN_SWIZZLE1.as_ptr() as *const v128)
        });
        *vn = v128_or(*vn, w0);

        if total_mn != 0 {
            unsafe { frwd_advance(magsgn, total_mn) };
        }
    }

    row
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
unsafe fn decode_step2_magsgn_wasm(
    coded_data: *mut Ui8,
    decoded_data: *mut Ui32,
    mut p: Ui32,
    mmsbp2: Ui32,
    lcup: i32,
    scup: i32,
    width: Ui32,
    height: Ui32,
    stride: Ui32,
    sstr: Ui32,
    scratch: &[u16],
) -> bool {
    if mmsbp2 >= 16 {
        let v_n_size = 512 + 8;
        let mut v_n_scratch = [0u32; 2 * (512 + 8)];
        let mut magsgn = frwd_struct::default();
        unsafe { frwd_init::<0xFF>(&mut magsgn, coded_data as *const Ui8, lcup - scup) };

        {
            let mut sp = scratch.as_ptr();
            let mut vp = v_n_scratch.as_mut_ptr();
            let mut dp = decoded_data;
            unsafe { *vp = 2 };

            let mut x = 0u32;
            while x < width {
                let inf_u_q = unsafe { v128_load(sp as *const v128) };
                let u_q = u32x4_shr(inf_u_q, 16);
                let gt = i32x4_gt(u_q, i32x4_splat(mmsbp2 as i32));
                if (i8x16_bitmask(gt) as Ui32) & 0xFF != 0 {
                    return false;
                }

                let mut vn = i32x4_splat(2);
                let mut row0 =
                    unsafe { decode_one_quad32::<0>(inf_u_q, u_q, &mut magsgn, p, &mut vn) };
                let mut row1 =
                    unsafe { decode_one_quad32::<1>(inf_u_q, u_q, &mut magsgn, p, &mut vn) };
                let mut w0 = unsafe { v128_load(vp as *const v128) };
                w0 = v128_and(
                    w0,
                    i32x4_shuffle::<0, 4, 4, 4>(i32x4_splat(-1), i32x4_splat(0)),
                );
                w0 = v128_or(w0, vn);
                unsafe { v128_store(vp as *mut v128, w0) };

                let w0i = i32x4_shuffle::<0, 4, 1, 5>(row0, row1);
                let w1i = i32x4_shuffle::<2, 6, 3, 7>(row0, row1);
                row0 = i32x4_shuffle::<0, 4, 1, 5>(w0i, w1i);
                row1 = i32x4_shuffle::<2, 6, 3, 7>(w0i, w1i);
                unsafe {
                    v128_store(dp as *mut v128, row0);
                    v128_store(dp.add(stride as usize) as *mut v128, row1);
                }

                x += 4;
                sp = unsafe { sp.add(4) };
                vp = unsafe { vp.add(2) };
                dp = unsafe { dp.add(4) };
            }
        }

        let lut_lo = [31u8, 7, 6, 6, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 4];
        let lut_hi = [31u8, 3, 2, 2, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        let nibble_mask = [0x0Fu8; 16];
        let byte_offset8 = [8u16; 8];
        let byte_offset16 = [16u16; 8];
        let cc = [31u32; 4];

        let mut y = 2u32;
        while y < height {
            {
                let mut vp = v_n_scratch.as_mut_ptr();
                let lut_lo_v = unsafe { v128_load(lut_lo.as_ptr() as *const v128) };
                let lut_hi_v = unsafe { v128_load(lut_hi.as_ptr() as *const v128) };
                let nibble_mask_v = unsafe { v128_load(nibble_mask.as_ptr() as *const v128) };
                let byte_offset8_v = unsafe { v128_load(byte_offset8.as_ptr() as *const v128) };
                let byte_offset16_v = unsafe { v128_load(byte_offset16.as_ptr() as *const v128) };
                let cc_v = unsafe { v128_load(cc.as_ptr() as *const v128) };
                let mut x = 0u32;
                while x <= width {
                    let mut v = unsafe { v128_load(vp as *const v128) };
                    let mut t = v128_and(nibble_mask_v, v);
                    v = v128_and(u16x8_shr(v, 4), nibble_mask_v);
                    t = i8x16_swizzle(lut_lo_v, t);
                    v = i8x16_swizzle(lut_hi_v, v);
                    v = u8x16_min(v, t);
                    t = u16x8_shr(v, 8);
                    v = v128_or(v, byte_offset8_v);
                    v = u8x16_min(v, t);
                    t = u32x4_shr(v, 16);
                    v = v128_or(v, byte_offset16_v);
                    v = u8x16_min(v, t);
                    v = i16x8_sub(cc_v, v);
                    unsafe { v128_store(vp.add(v_n_size) as *mut v128, v) };
                    x += 8;
                    vp = unsafe { vp.add(4) };
                }
            }

            let mut vp = v_n_scratch.as_mut_ptr();
            let mut sp = unsafe { scratch.as_ptr().add(((y >> 1) * sstr) as usize) };
            let mut dp = unsafe { decoded_data.add((y * stride) as usize) };
            unsafe { *vp = 2 };

            let mut x = 0u32;
            while x < width {
                let inf_u_q;
                let u_q;
                let u_q_adj;
                {
                    inf_u_q = unsafe { v128_load(sp as *const v128) };
                    let mut gamma = v128_and(inf_u_q, i32x4_splat(0xF0));
                    let w0 = i32x4_sub(gamma, i32x4_splat(1));
                    gamma = v128_and(gamma, w0);
                    gamma = i32x4_eq(gamma, i64x2_make(0, 0));

                    let mut emax = unsafe { v128_load(vp.add(v_n_size) as *const v128) };
                    let w0 = i32x4_shuffle::<1, 2, 3, 4>(emax, i64x2_make(0, 0));
                    emax = i16x8_max(w0, emax);
                    emax = v128_andnot(emax, gamma);

                    let mut kappa = i32x4_splat(1);
                    kappa = i16x8_max(emax, kappa);

                    u_q = u32x4_shr(inf_u_q, 16);
                    u_q_adj = i32x4_add(u_q, kappa);
                    let gt = i32x4_gt(u_q_adj, i32x4_splat(mmsbp2 as i32));
                    if (i8x16_bitmask(gt) as Ui32) & 0xFF != 0 {
                        return false;
                    }
                }

                let mut vn = i32x4_splat(2);
                let mut row0 =
                    unsafe { decode_one_quad32::<0>(inf_u_q, u_q_adj, &mut magsgn, p, &mut vn) };
                let mut row1 =
                    unsafe { decode_one_quad32::<1>(inf_u_q, u_q_adj, &mut magsgn, p, &mut vn) };
                let mut w0 = unsafe { v128_load(vp as *const v128) };
                w0 = v128_and(
                    w0,
                    i32x4_shuffle::<0, 4, 4, 4>(i32x4_splat(-1), i32x4_splat(0)),
                );
                w0 = v128_or(w0, vn);
                unsafe { v128_store(vp as *mut v128, w0) };

                let w0i = i32x4_shuffle::<0, 4, 1, 5>(row0, row1);
                let w1i = i32x4_shuffle::<2, 6, 3, 7>(row0, row1);
                row0 = i32x4_shuffle::<0, 4, 1, 5>(w0i, w1i);
                row1 = i32x4_shuffle::<2, 6, 3, 7>(w0i, w1i);
                unsafe {
                    v128_store(dp as *mut v128, row0);
                    v128_store(dp.add(stride as usize) as *mut v128, row1);
                }

                x += 4;
                sp = unsafe { sp.add(4) };
                vp = unsafe { vp.add(2) };
                dp = unsafe { dp.add(4) };
            }

            y += 2;
        }
    } else {
        p -= 16;
        let v_n_size = 512 + 8;
        let mut v_n_scratch = [0u16; 2 * (512 + 8)];
        let mut magsgn = frwd_struct::default();
        unsafe { frwd_init::<0xFF>(&mut magsgn, coded_data as *const Ui8, lcup - scup) };

        {
            let mut sp = scratch.as_ptr();
            let mut vp = v_n_scratch.as_mut_ptr();
            let mut dp = decoded_data;
            unsafe { *vp = 2 };

            let mut x = 0u32;
            while x < width {
                let inf_u_q = unsafe { v128_load(sp as *const v128) };
                let u_q = u32x4_shr(inf_u_q, 16);
                let gt = i32x4_gt(u_q, i32x4_splat(mmsbp2 as i32));
                if (i8x16_bitmask(gt) as Ui32) & 0xFF != 0 {
                    return false;
                }

                let mut vn = unsafe { v128_load([2u16; 8].as_ptr() as *const v128) };
                let row = unsafe { decode_two_quad16(inf_u_q, u_q, &mut magsgn, p, &mut vn) };
                let mut w0 = unsafe { v128_load(vp as *const v128) };
                w0 = v128_and(w0, unsafe {
                    v128_load([0xFFFFu16, 0, 0, 0, 0, 0, 0, 0].as_ptr() as *const v128)
                });
                w0 = v128_or(w0, vn);
                unsafe { v128_store(vp as *mut v128, w0) };

                let row0 = i8x16_swizzle(row, unsafe {
                    v128_load(
                        [
                            0xFFFFu16, 0x0100, 0xFFFF, 0x0504, 0xFFFF, 0x0908, 0xFFFF, 0x0D0C,
                        ]
                        .as_ptr() as *const v128,
                    )
                });
                let row1 = i8x16_swizzle(row, unsafe {
                    v128_load(
                        [
                            0xFFFFu16, 0x0302, 0xFFFF, 0x0706, 0xFFFF, 0x0B0A, 0xFFFF, 0x0F0E,
                        ]
                        .as_ptr() as *const v128,
                    )
                });
                unsafe {
                    v128_store(dp as *mut v128, row0);
                    v128_store(dp.add(stride as usize) as *mut v128, row1);
                }

                x += 4;
                sp = unsafe { sp.add(4) };
                vp = unsafe { vp.add(2) };
                dp = unsafe { dp.add(4) };
            }
        }

        let lut_lo = [15u8, 7, 6, 6, 5, 5, 5, 5, 4, 4, 4, 4, 4, 4, 4, 4];
        let lut_hi = [15u8, 3, 2, 2, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0];
        let nibble_mask = [0x0Fu8; 16];
        let byte_offset8 = [8u16; 8];
        let cc = [15u16; 8];

        let mut y = 2u32;
        while y < height {
            {
                let mut vp = v_n_scratch.as_mut_ptr();
                let lut_lo_v = unsafe { v128_load(lut_lo.as_ptr() as *const v128) };
                let lut_hi_v = unsafe { v128_load(lut_hi.as_ptr() as *const v128) };
                let nibble_mask_v = unsafe { v128_load(nibble_mask.as_ptr() as *const v128) };
                let byte_offset8_v = unsafe { v128_load(byte_offset8.as_ptr() as *const v128) };
                let cc_v = unsafe { v128_load(cc.as_ptr() as *const v128) };
                let mut x = 0u32;
                while x <= width {
                    let mut v = unsafe { v128_load(vp as *const v128) };
                    let mut t = v128_and(nibble_mask_v, v);
                    v = v128_and(u16x8_shr(v, 4), nibble_mask_v);
                    t = i8x16_swizzle(lut_lo_v, t);
                    v = i8x16_swizzle(lut_hi_v, v);
                    v = u8x16_min(v, t);
                    t = u16x8_shr(v, 8);
                    v = v128_or(v, byte_offset8_v);
                    v = u8x16_min(v, t);
                    v = i16x8_sub(cc_v, v);
                    unsafe { v128_store(vp.add(v_n_size) as *mut v128, v) };
                    x += 16;
                    vp = unsafe { vp.add(8) };
                }
            }

            let mut vp = v_n_scratch.as_mut_ptr();
            let mut sp = unsafe { scratch.as_ptr().add(((y >> 1) * sstr) as usize) };
            let mut dp = unsafe { decoded_data.add((y * stride) as usize) };
            unsafe { *vp = 2 };

            let mut x = 0u32;
            while x < width {
                let inf_u_q;
                let u_q_adj;
                {
                    inf_u_q = unsafe { v128_load(sp as *const v128) };
                    let mut gamma = v128_and(inf_u_q, i32x4_splat(0xF0));
                    let w0 = i32x4_sub(gamma, i32x4_splat(1));
                    gamma = v128_and(gamma, w0);
                    gamma = i32x4_eq(gamma, i64x2_make(0, 0));

                    let mut emax = unsafe { v128_load(vp.add(v_n_size) as *const v128) };
                    let w0 = i16x8_shuffle::<1, 2, 3, 4, 5, 6, 7, 8>(emax, i64x2_make(0, 0));
                    emax = i16x8_max(w0, emax);
                    emax = i8x16_swizzle(emax, unsafe {
                        v128_load(
                            [
                                0x0100u16, 0xFFFF, 0x0302, 0xFFFF, 0x0504, 0xFFFF, 0x0706, 0xFFFF,
                            ]
                            .as_ptr() as *const v128,
                        )
                    });
                    emax = v128_andnot(emax, gamma);

                    let mut kappa = i32x4_splat(1);
                    kappa = i16x8_max(emax, kappa);

                    let u_q = u32x4_shr(inf_u_q, 16);
                    u_q_adj = i32x4_add(u_q, kappa);
                    let gt = i32x4_gt(u_q_adj, i32x4_splat(mmsbp2 as i32));
                    if (i8x16_bitmask(gt) as Ui32) & 0xFF != 0 {
                        return false;
                    }
                }

                let mut vn = unsafe { v128_load([2u16; 8].as_ptr() as *const v128) };
                let row = unsafe { decode_two_quad16(inf_u_q, u_q_adj, &mut magsgn, p, &mut vn) };
                let mut w0 = unsafe { v128_load(vp as *const v128) };
                w0 = v128_and(w0, unsafe {
                    v128_load([0xFFFFu16, 0, 0, 0, 0, 0, 0, 0].as_ptr() as *const v128)
                });
                w0 = v128_or(w0, vn);
                unsafe { v128_store(vp as *mut v128, w0) };

                let row0 = i8x16_swizzle(row, unsafe {
                    v128_load(
                        [
                            0xFFFFu16, 0x0100, 0xFFFF, 0x0504, 0xFFFF, 0x0908, 0xFFFF, 0x0D0C,
                        ]
                        .as_ptr() as *const v128,
                    )
                });
                let row1 = i8x16_swizzle(row, unsafe {
                    v128_load(
                        [
                            0xFFFFu16, 0x0302, 0xFFFF, 0x0706, 0xFFFF, 0x0B0A, 0xFFFF, 0x0F0E,
                        ]
                        .as_ptr() as *const v128,
                    )
                });
                unsafe {
                    v128_store(dp as *mut v128, row0);
                    v128_store(dp.add(stride as usize) as *mut v128, row1);
                }

                x += 4;
                sp = unsafe { sp.add(4) };
                vp = unsafe { vp.add(2) };
                dp = unsafe { dp.add(4) };
            }

            y += 2;
        }
    }

    true
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn decode_step2_magsgn_wasm(
    coded_data: *mut Ui8,
    decoded_data: *mut Ui32,
    p: Ui32,
    mmsbp2: Ui32,
    lcup: i32,
    scup: i32,
    width: Ui32,
    height: Ui32,
    stride: Ui32,
    sstr: Ui32,
    scratch: &[u16],
) -> bool {
    let mut v_n_scratch = [0u32; 512 + 4];
    let mut magsgn = frwd_struct32_scalar::default();
    unsafe { frwd_init_32_scalar::<0xFF>(&mut magsgn, coded_data as *const Ui8, lcup - scup) };

    let mut sp = 0usize;
    let mut vp = 0usize;
    let mut dp = decoded_data;
    let mut prev_v_n = 0u32;

    let mut x = 0u32;
    while x < width {
        let inf = scratch[sp] as Ui32;
        let u_q = scratch[sp + 1] as Ui32;
        if u_q > mmsbp2 {
            return false;
        }

        let (val0, _) = unsafe { decode_magsgn_value_32_scalar(&mut magsgn, inf, 0, u_q, p) };
        unsafe { *dp = val0 };

        let (val1, v1) = unsafe { decode_magsgn_value_32_scalar(&mut magsgn, inf, 1, u_q, p) };
        unsafe { *dp.add(stride as usize) = val1 };
        v_n_scratch[vp] = prev_v_n | v1;
        prev_v_n = 0;
        dp = unsafe { dp.add(1) };
        x += 1;
        if x >= width {
            vp += 1;
            break;
        }

        let (val2, _) = unsafe { decode_magsgn_value_32_scalar(&mut magsgn, inf, 2, u_q, p) };
        unsafe { *dp = val2 };

        let (val3, v3) = unsafe { decode_magsgn_value_32_scalar(&mut magsgn, inf, 3, u_q, p) };
        unsafe { *dp.add(stride as usize) = val3 };
        prev_v_n = v3;
        dp = unsafe { dp.add(1) };
        x += 1;

        sp += 2;
        vp += 1;
    }
    v_n_scratch[vp] = prev_v_n;

    for y in (2..height).step_by(2) {
        let mut sp = ((y >> 1) * sstr) as usize;
        let mut vp = 0usize;
        let mut dp = unsafe { decoded_data.add((y * stride) as usize) };
        let mut prev_v_n = 0u32;
        let mut x = 0u32;

        while x < width {
            let inf = scratch[sp] as Ui32;
            let u_q = scratch[sp + 1] as Ui32;

            let mut gamma = inf & 0xF0;
            // C++ unsigned wrap-around: gamma - 0x10 is defined even when gamma == 0.
            gamma &= gamma.wrapping_sub(0x10);
            let mut emax = v_n_scratch[vp] | v_n_scratch[vp + 1];
            emax = 31 - count_leading_zeros_u32(emax | 2);
            let kappa = if gamma != 0 { emax } else { 1 };

            let u_q = u_q + kappa;
            if u_q > mmsbp2 {
                return false;
            }

            let (val0, _) = unsafe { decode_magsgn_value_32_scalar(&mut magsgn, inf, 0, u_q, p) };
            unsafe { *dp = val0 };

            let (val1, v1) = unsafe { decode_magsgn_value_32_scalar(&mut magsgn, inf, 1, u_q, p) };
            unsafe { *dp.add(stride as usize) = val1 };
            v_n_scratch[vp] = prev_v_n | v1;
            prev_v_n = 0;
            dp = unsafe { dp.add(1) };
            x += 1;
            if x >= width {
                vp += 1;
                break;
            }

            let (val2, _) = unsafe { decode_magsgn_value_32_scalar(&mut magsgn, inf, 2, u_q, p) };
            unsafe { *dp = val2 };

            let (val3, v3) = unsafe { decode_magsgn_value_32_scalar(&mut magsgn, inf, 3, u_q, p) };
            unsafe { *dp.add(stride as usize) = val3 };
            prev_v_n = v3;
            dp = unsafe { dp.add(1) };
            x += 1;

            sp += 2;
            vp += 1;
        }
        v_n_scratch[vp] = prev_v_n;
    }

    true
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
unsafe fn rearrange_sigma_wasm(width: Ui32, height: Ui32, sstr: Ui32, scratch: &mut [u16]) -> Ui32 {
    let mstr = (((width + 3) >> 2) + 2 + 7) & !7;
    let mask_3 = i32x4_splat(0x30);
    let mask_c = i32x4_splat(0xC0);
    let shuffle_mask =
        unsafe { v128_load(REARRANGE_SIGMA_WASM_SHUFFLE_MASK.as_ptr() as *const v128) };

    let mut y = 0;
    while y < height {
        let mut sp = unsafe { scratch.as_ptr().add(((y >> 1) * sstr) as usize) };
        let mut dp = unsafe { scratch.as_mut_ptr().add(((y >> 2) * mstr) as usize) };

        let mut x = 0;
        while x < width {
            let s0 = unsafe { v128_load(sp as *const v128) };
            let mut u3 = v128_and(s0, mask_3);
            u3 = u32x4_shr(u3, 4);
            let mut u_c = v128_and(s0, mask_c);
            u_c = u32x4_shr(u_c, 2);
            let t0 = v128_or(u3, u_c);

            let s1 = unsafe { v128_load(sp.add(sstr as usize) as *const v128) };
            u3 = v128_and(s1, mask_3);
            u3 = u32x4_shr(u3, 2);
            u_c = v128_and(s1, mask_c);
            let t1 = v128_or(u3, u_c);

            let r = i8x16_swizzle(v128_or(t0, t1), shuffle_mask);
            unsafe { write_unaligned(dp as *mut Ui32, u32x4_extract_lane::<0>(r)) };

            x += 8;
            sp = unsafe { sp.add(8) };
            dp = unsafe { dp.add(2) };
        }

        unsafe { *dp = 0 };
        y += 4;
    }

    let mut dp = unsafe { scratch.as_mut_ptr().add(((y >> 2) * mstr) as usize) };
    let mut x = 0;
    while x < width {
        unsafe { *dp = 0 };
        x += 4;
        dp = unsafe { dp.add(1) };
    }
    unsafe { *dp = 0 };

    mstr
}

#[cfg(not(target_arch = "wasm32"))]
fn rearrange_sigma_wasm(width: Ui32, height: Ui32, sstr: Ui32, scratch: &mut [u16]) -> Ui32 {
    let mstr = (((width + 3) >> 2) + 2 + 7) & !7;

    let mut y = 0;
    while y < height {
        let mut sp = ((y >> 1) * sstr) as usize;
        let mut dp = ((y >> 2) * mstr) as usize;
        let next_row = sstr as usize;

        for _x in (0..width).step_by(4) {
            let mut t0 = 0u32;
            let mut t1 = 0u32;
            t0 = ((scratch[sp] as Ui32 & 0x30) >> 4) | ((scratch[sp] as Ui32 & 0xC0) >> 2);
            t0 |= ((scratch[sp + 2] as Ui32 & 0x30) << 4) | ((scratch[sp + 2] as Ui32 & 0xC0) << 6);
            t1 = ((scratch[sp + next_row] as Ui32 & 0x30) >> 2)
                | (scratch[sp + next_row] as Ui32 & 0xC0);
            t1 |= ((scratch[sp + 2 + next_row] as Ui32 & 0x30) << 6)
                | ((scratch[sp + 2 + next_row] as Ui32 & 0xC0) << 8);
            scratch[dp] = (t0 | t1) as Ui16;
            sp += 4;
            dp += 1;
        }
        scratch[dp] = 0;
        y += 4;
    }

    let mut dp = ((y >> 2) * mstr) as usize;
    for _x in (0..width).step_by(4) {
        scratch[dp] = 0;
        dp += 1;
    }
    scratch[dp] = 0;

    mstr
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
unsafe fn wasm_expand_bits16_eqmask(bits: Ui16) -> v128 {
    let mut v = i16x8_splat(bits as i16);
    v = i8x16_swizzle(v, unsafe {
        v128_load(WASM_EXPAND_BITS_SWIZZLE.as_ptr() as *const v128)
    });
    let m = unsafe { v128_load(WASM_EXPAND_BITS_MASK.as_ptr() as *const v128) };
    v = v128_and(v, m);
    i8x16_eq(v, m)
}

#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
unsafe fn wasm_exclusive_scan_eqmask(eqmask: v128) -> (v128, Ui32) {
    let mut inc = i8x16_abs(eqmask);
    let mut sh = i8x16_swizzle(inc, unsafe {
        v128_load(WASM_SCAN_SHIFT1.as_ptr() as *const v128)
    });
    inc = i8x16_add(inc, sh);
    sh = i8x16_swizzle(inc, unsafe {
        v128_load(WASM_SCAN_SHIFT2.as_ptr() as *const v128)
    });
    inc = i8x16_add(inc, sh);
    sh = i8x16_swizzle(inc, unsafe {
        v128_load(WASM_SCAN_SHIFT4.as_ptr() as *const v128)
    });
    inc = i8x16_add(inc, sh);
    sh = i8x16_swizzle(inc, unsafe {
        v128_load(WASM_SCAN_SHIFT8.as_ptr() as *const v128)
    });
    inc = i8x16_add(inc, sh);

    let ex = i8x16_swizzle(inc, unsafe {
        v128_load(WASM_SCAN_SHIFT1.as_ptr() as *const v128)
    });
    (ex, u8x16_extract_lane::<15>(inc) as Ui32)
}

#[cfg(target_arch = "wasm32")]
unsafe fn decode_spp_wasm(
    coded_data: *mut Ui8,
    decoded_data: *mut Ui32,
    p: Ui32,
    lengths1: Ui32,
    lengths2: Ui32,
    width: Ui32,
    height: Ui32,
    stride: Ui32,
    mstr: Ui32,
    stripe_causal: bool,
    sigma: &mut [u16],
) {
    let mut prev_row_sig = [0u16; 256 + 8];
    let mut sigprop = frwd_struct::default();
    unsafe {
        frwd_init::<0>(
            &mut sigprop,
            coded_data.add(lengths1 as usize) as *const Ui8,
            lengths2 as i32,
        )
    };

    for y in (0..height).step_by(4) {
        let mut pattern = 0xFFFFu32;
        if height - y < 4 {
            pattern = 0x7777;
            if height - y < 3 {
                pattern = 0x3333;
                if height - y < 2 {
                    pattern = 0x1111;
                }
            }
        }

        let mut prev = 0u32;
        let mut prev_sig = 0usize;
        let mut cur_sig = ((y >> 2) * mstr) as usize;
        let dpp = unsafe { decoded_data.add((y * stride) as usize) };

        for x in (0..width).step_by(4) {
            let over = x as i32 + 4 - width as i32;
            if over > 0 {
                pattern >>= (over as u32) * 4;
            }

            let ps = unsafe { read_unaligned(prev_row_sig.as_ptr().add(prev_sig) as *const Ui32) };
            let ns = unsafe {
                read_unaligned(sigma.as_ptr().add(cur_sig + mstr as usize) as *const Ui32)
            };
            let mut u = (ps & 0x8888_8888) >> 3;
            if !stripe_causal {
                u |= (ns & 0x1111_1111) << 3;
            }

            let cs = unsafe { read_unaligned(sigma.as_ptr().add(cur_sig) as *const Ui32) };
            let mut mbr = cs;
            mbr |= (cs & 0x7777_7777) << 1;
            mbr |= (cs & 0xEEEE_EEEE) >> 1;
            mbr |= u;
            let t = mbr;
            mbr |= t << 4;
            mbr |= t >> 4;
            mbr |= prev >> 12;
            mbr &= pattern;
            mbr &= !cs;

            let mut new_sig = mbr;
            if new_sig != 0 {
                let mut cwd = u32x4_extract_lane::<0>(unsafe { frwd_fetch::<0>(&mut sigprop) });
                let mut cnt = 0u32;
                let mut col_mask = 0xFu32;
                let inv_sig = !cs & pattern;

                for i in (0..16).step_by(4) {
                    if (col_mask & new_sig) == 0 {
                        col_mask <<= 4;
                        continue;
                    }

                    let mut sample_mask = 0x1111u32 & col_mask;
                    if (new_sig & sample_mask) != 0 {
                        new_sig &= !sample_mask;
                        if (cwd & 1) != 0 {
                            new_sig |= (0x33u32 << i) & inv_sig;
                        }
                        cwd >>= 1;
                        cnt += 1;
                    }

                    sample_mask <<= 1;
                    if (new_sig & sample_mask) != 0 {
                        new_sig &= !sample_mask;
                        if (cwd & 1) != 0 {
                            new_sig |= (0x76u32 << i) & inv_sig;
                        }
                        cwd >>= 1;
                        cnt += 1;
                    }

                    sample_mask <<= 1;
                    if (new_sig & sample_mask) != 0 {
                        new_sig &= !sample_mask;
                        if (cwd & 1) != 0 {
                            new_sig |= (0xECu32 << i) & inv_sig;
                        }
                        cwd >>= 1;
                        cnt += 1;
                    }

                    sample_mask <<= 1;
                    if (new_sig & sample_mask) != 0 {
                        new_sig &= !sample_mask;
                        if (cwd & 1) != 0 {
                            new_sig |= (0xC8u32 << i) & inv_sig;
                        }
                        cwd >>= 1;
                        cnt += 1;
                    }

                    col_mask <<= 4;
                }

                if new_sig != 0 {
                    let new_sig_vec = unsafe { wasm_expand_bits16_eqmask(new_sig as Ui16) };
                    let (ex_sum, add_bits) = unsafe { wasm_exclusive_scan_eqmask(new_sig_vec) };
                    cnt += add_bits;

                    let mut cwd_vec = unsafe { wasm_expand_bits16_eqmask(cwd as Ui16) };
                    cwd_vec = i8x16_abs(cwd_vec);
                    let signs = i8x16_swizzle(cwd_vec, ex_sum);
                    let val = i32x4_splat((3u32 << (p - 2)) as i32);
                    let mut dp = unsafe { dpp.add(x as usize) };

                    for row_mask in [
                        &WASM_ROW_SWIZZLE0,
                        &WASM_ROW_SWIZZLE1,
                        &WASM_ROW_SWIZZLE2,
                        &WASM_ROW_SWIZZLE3,
                    ] {
                        let mask = unsafe { v128_load(row_mask.as_ptr() as *const v128) };
                        let mut s0 = unsafe { v128_load(dp as *const v128) };
                        let mut s0_ns = i8x16_swizzle(new_sig_vec, mask);
                        s0_ns = i32x4_eq(s0_ns, i32x4_splat(0xFF));
                        let mut s0_val = i8x16_swizzle(signs, mask);
                        s0_val = i32x4_shl(s0_val, 31);
                        s0_val = v128_or(s0_val, val);
                        s0_val = v128_and(s0_val, s0_ns);
                        s0 = v128_or(s0, s0_val);
                        unsafe { v128_store(dp as *mut v128, s0) };
                        dp = unsafe { dp.add(stride as usize) };
                    }
                }

                unsafe { frwd_advance(&mut sigprop, cnt) };
            }

            new_sig |= cs;
            prev_row_sig[prev_sig] = new_sig as u16;
            let t = new_sig;
            new_sig |= (t & 0x7777) << 1;
            new_sig |= (t & 0xEEEE) >> 1;
            prev = (new_sig | u) & 0xF000;

            prev_sig += 1;
            cur_sig += 1;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn decode_spp_wasm(
    coded_data: *mut Ui8,
    decoded_data: *mut Ui32,
    p: Ui32,
    lengths1: Ui32,
    lengths2: Ui32,
    width: Ui32,
    height: Ui32,
    stride: Ui32,
    mstr: Ui32,
    stripe_causal: bool,
    sigma: &mut [u16],
) {
    let mut prev_row_sig = [0u16; 256 + 8];
    let mut sigprop = frwd_struct32_scalar::default();
    unsafe {
        frwd_init_32_scalar::<0>(
            &mut sigprop,
            coded_data.add(lengths1 as usize) as *const Ui8,
            lengths2 as i32,
        )
    };

    for y in (0..height).step_by(4) {
        let mut pattern = 0xFFFFu32;
        if height - y < 4 {
            pattern = 0x7777;
            if height - y < 3 {
                pattern = 0x3333;
                if height - y < 2 {
                    pattern = 0x1111;
                }
            }
        }

        let mut prev = 0u32;
        let mut prev_sig = 0usize;
        let mut cur_sig = ((y >> 2) * mstr) as usize;
        let dpp = unsafe { decoded_data.add((y * stride) as usize) };

        for x in (0..width).step_by(4) {
            let mut s = x as i32 + 4 - width as i32;
            if s < 0 {
                s = 0;
            }
            pattern >>= (s * 4) as u32;

            let ps = unsafe { read_unaligned(prev_row_sig.as_ptr().add(prev_sig) as *const Ui32) };
            let ns = unsafe {
                read_unaligned(sigma.as_ptr().add(cur_sig + mstr as usize) as *const Ui32)
            };
            let mut u = (ps & 0x8888_8888) >> 3;
            if !stripe_causal {
                u |= (ns & 0x1111_1111) << 3;
            }

            let cs = unsafe { read_unaligned(sigma.as_ptr().add(cur_sig) as *const Ui32) };
            let mut mbr = cs;
            mbr |= (cs & 0x7777_7777) << 1;
            mbr |= (cs & 0xEEEE_EEEE) >> 1;
            mbr |= u;
            let t = mbr;
            mbr |= t << 4;
            mbr |= t >> 4;
            mbr |= prev >> 12;
            mbr &= pattern;
            mbr &= !cs;

            let mut new_sig = mbr;
            if new_sig != 0 {
                let mut cwd = unsafe { frwd_fetch_32_scalar::<0>(&mut sigprop) };
                let mut cnt = 0u32;
                let mut col_mask = 0xFu32;
                let inv_sig = !cs & pattern;

                for i in (0..16).step_by(4) {
                    if (col_mask & new_sig) == 0 {
                        col_mask <<= 4;
                        continue;
                    }

                    let mut sample_mask = 0x1111u32 & col_mask;
                    if (new_sig & sample_mask) != 0 {
                        new_sig &= !sample_mask;
                        if (cwd & 1) != 0 {
                            let t = 0x33u32 << i;
                            new_sig |= t & inv_sig;
                        }
                        cwd >>= 1;
                        cnt += 1;
                    }

                    sample_mask <<= 1;
                    if (new_sig & sample_mask) != 0 {
                        new_sig &= !sample_mask;
                        if (cwd & 1) != 0 {
                            let t = 0x76u32 << i;
                            new_sig |= t & inv_sig;
                        }
                        cwd >>= 1;
                        cnt += 1;
                    }

                    sample_mask <<= 1;
                    if (new_sig & sample_mask) != 0 {
                        new_sig &= !sample_mask;
                        if (cwd & 1) != 0 {
                            let t = 0xECu32 << i;
                            new_sig |= t & inv_sig;
                        }
                        cwd >>= 1;
                        cnt += 1;
                    }

                    sample_mask <<= 1;
                    if (new_sig & sample_mask) != 0 {
                        new_sig &= !sample_mask;
                        if (cwd & 1) != 0 {
                            let t = 0xC8u32 << i;
                            new_sig |= t & inv_sig;
                        }
                        cwd >>= 1;
                        cnt += 1;
                    }

                    col_mask <<= 4;
                }

                if new_sig != 0 {
                    let mut dp = unsafe { dpp.add(x as usize) };
                    let val = 3u32 << (p - 2);
                    let mut col_mask = 0xFu32;
                    for _i in 0..4 {
                        if (col_mask & new_sig) == 0 {
                            dp = unsafe { dp.add(1) };
                            col_mask <<= 4;
                            continue;
                        }

                        let mut sample_mask = 0x1111u32 & col_mask;
                        if (new_sig & sample_mask) != 0 {
                            debug_assert!(unsafe { *dp } == 0);
                            unsafe { *dp = (cwd << 31) | val };
                            cwd >>= 1;
                            cnt += 1;
                        }

                        sample_mask <<= 1; // double the mask; high bit discarded (C++ uint wrap)
                        if (new_sig & sample_mask) != 0 {
                            debug_assert!(unsafe { *dp.add(stride as usize) } == 0);
                            unsafe { *dp.add(stride as usize) = (cwd << 31) | val };
                            cwd >>= 1;
                            cnt += 1;
                        }

                        sample_mask <<= 1; // double the mask; high bit discarded (C++ uint wrap)
                        if (new_sig & sample_mask) != 0 {
                            debug_assert!(unsafe { *dp.add((2 * stride) as usize) } == 0);
                            unsafe { *dp.add((2 * stride) as usize) = (cwd << 31) | val };
                            cwd >>= 1;
                            cnt += 1;
                        }

                        sample_mask <<= 1; // double the mask; high bit discarded (C++ uint wrap)
                        if (new_sig & sample_mask) != 0 {
                            debug_assert!(unsafe { *dp.add((3 * stride) as usize) } == 0);
                            unsafe { *dp.add((3 * stride) as usize) = (cwd << 31) | val };
                            cwd >>= 1;
                            cnt += 1;
                        }

                        dp = unsafe { dp.add(1) };
                        col_mask <<= 4;
                    }
                }

                frwd_advance_32_scalar(&mut sigprop, cnt);
            }

            new_sig |= cs;
            prev_row_sig[prev_sig] = new_sig as Ui16;
            let t = new_sig;
            new_sig |= (t & 0x7777) << 1;
            new_sig |= (t & 0xEEEE) >> 1;
            prev = new_sig | u;
            prev &= 0xF000;

            prev_sig += 1;
            cur_sig += 1;
        }
    }
}

#[cfg(target_arch = "wasm32")]
unsafe fn decode_mrp_wasm(
    coded_data: *mut Ui8,
    decoded_data: *mut Ui32,
    p: Ui32,
    lengths1: Ui32,
    lengths2: Ui32,
    width: Ui32,
    height: Ui32,
    stride: Ui32,
    mstr: Ui32,
    sigma: &[u16],
) {
    let mut magref = rev_struct::default();
    unsafe { rev_init_mrp(&mut magref, coded_data, lengths1 as i32, lengths2 as i32) };

    for y in (0..height).step_by(4) {
        let mut cur_sig = ((y >> 2) * mstr) as usize;
        let dpp = unsafe { decoded_data.add((y * stride) as usize) };
        let half = 1u32 << (p - 2);
        for i in (0..width).step_by(4) {
            let cwd = unsafe { rev_fetch_mrp(&mut magref) };
            let sig = unsafe { *sigma.as_ptr().add(cur_sig) };
            cur_sig += 1;
            let mut total_bits = 0u32;
            if sig != 0 {
                let sig_eq = unsafe { wasm_expand_bits16_eqmask(sig) };
                let sig_vec = i8x16_abs(sig_eq);
                let (ex_sum, total) = unsafe { wasm_exclusive_scan_eqmask(sig_eq) };
                total_bits = total;

                let mut cwd_vec = unsafe { wasm_expand_bits16_eqmask(cwd as Ui16) };
                cwd_vec = i8x16_add(cwd_vec, i8x16_splat(1));
                cwd_vec = i8x16_add(cwd_vec, cwd_vec);
                cwd_vec = v128_or(cwd_vec, i8x16_splat(1));

                let mut dp = unsafe { dpp.add(i as usize) };
                for row_mask in [
                    &WASM_ROW_SWIZZLE0,
                    &WASM_ROW_SWIZZLE1,
                    &WASM_ROW_SWIZZLE2,
                    &WASM_ROW_SWIZZLE3,
                ] {
                    let mask = unsafe { v128_load(row_mask.as_ptr() as *const v128) };
                    let mut s0 = unsafe { v128_load(dp as *const v128) };
                    let mut s0_sig = i8x16_swizzle(sig_vec, mask);
                    s0_sig = i8x16_eq(s0_sig, i64x2_make(0, 0));
                    let s0_idx = i8x16_swizzle(ex_sum, mask);
                    let mut s0_val = i8x16_swizzle(cwd_vec, s0_idx);
                    s0_val = v128_andnot(s0_val, s0_sig);
                    s0_val = i32x4_shl(s0_val, p - 2);
                    s0 = v128_xor(s0, s0_val);
                    unsafe { v128_store(dp as *mut v128, s0) };
                    dp = unsafe { dp.add(stride as usize) };
                }
            }
            rev_advance_mrp(&mut magref, total_bits);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
unsafe fn decode_mrp_wasm(
    coded_data: *mut Ui8,
    decoded_data: *mut Ui32,
    p: Ui32,
    lengths1: Ui32,
    lengths2: Ui32,
    width: Ui32,
    height: Ui32,
    stride: Ui32,
    mstr: Ui32,
    sigma: &[u16],
) {
    let mut magref = rev_struct::default();
    unsafe { rev_init_mrp(&mut magref, coded_data, lengths1 as i32, lengths2 as i32) };

    for y in (0..height).step_by(4) {
        let mut cur_sig = ((y >> 2) * mstr) as usize;
        let dpp = unsafe { decoded_data.add((y * stride) as usize) };
        let half = 1u32 << (p - 2);
        for i in (0..width).step_by(8) {
            let mut cwd = unsafe { rev_fetch_mrp(&mut magref) };
            let sig = unsafe { read_unaligned(sigma.as_ptr().add(cur_sig) as *const Ui32) };
            cur_sig += 2;
            let mut col_mask = 0xFu32;
            if sig != 0 {
                for j in 0..8 {
                    if (sig & col_mask) != 0 {
                        let mut dp = unsafe { dpp.add((i + j) as usize) };
                        let mut sample_mask = 0x1111_1111u32 & col_mask;
                        for _k in 0..4 {
                            if (sig & sample_mask) != 0 {
                                debug_assert!(unsafe { *dp } != 0);
                                debug_assert!((unsafe { *dp } & half) == 0);
                                let mut sym = cwd & 1;
                                sym = (1 - sym) << (p - 1);
                                sym |= half;
                                unsafe { *dp ^= sym };
                                cwd >>= 1;
                            }
                            sample_mask <<= 1; // double the mask; high bit discarded (C++ uint wrap)
                            dp = unsafe { dp.add(stride as usize) };
                        }
                    }
                    col_mask <<= 4;
                }
            }
            rev_advance_mrp(
                &mut magref,
                crate::openjph::ojph_arch::population_count(sig),
            );
        }
    }
}

static INSUFFICIENT_PRECISION32_WASM: AtomicBool = AtomicBool::new(false);
static MODIFY_CODE32_WASM: AtomicBool = AtomicBool::new(false);
static TRUNCATE_SPP_MRP32_WASM: AtomicBool = AtomicBool::new(false);

pub(crate) unsafe fn ojph_decode_codeblock_wasm(
    coded_data: *mut Ui8,
    decoded_data: *mut Ui32,
    missing_msbs: Ui32,
    mut num_passes: Ui32,
    lengths1: Ui32,
    lengths2: Ui32,
    width: Ui32,
    height: Ui32,
    stride: Ui32,
    stripe_causal: bool,
) -> bool {
    if num_passes > 1 && lengths2 == 0 {
        ojph_warn!(
            0x00010001,
            "A malformed codeblock that has more than one coding pass, but zero length for 2nd and potential 3rd pass."
        );
        num_passes = 1;
    }

    if num_passes > 3 {
        ojph_warn!(
            0x00010002,
            "We do not support more than 3 coding passes; This codeblocks has {} passes.",
            num_passes
        );
        return false;
    }

    if missing_msbs > 30 {
        if !INSUFFICIENT_PRECISION32_WASM.swap(true, Ordering::Relaxed) {
            ojph_warn!(
                0x00010003,
                "32 bits are not enough to decode this codeblock. This message will not be displayed again."
            );
        }
        return false;
    } else if missing_msbs == 30 {
        if !MODIFY_CODE32_WASM.swap(true, Ordering::Relaxed) {
            ojph_warn!(
                0x00010004,
                "Not enough precision to decode the cleanup pass. The code can be modified to support this case. This message will not be displayed again."
            );
        }
        return false;
    } else if missing_msbs == 29 && num_passes > 1 {
        num_passes = 1;
        if !TRUNCATE_SPP_MRP32_WASM.swap(true, Ordering::Relaxed) {
            ojph_warn!(
                0x00010005,
                "Not enough precision to decode the SgnProp nor MagRef passes; both will be skipped. This message will not be displayed again."
            );
        }
    }

    let p = 30 - missing_msbs;
    if lengths1 < 2 {
        ojph_warn!(0x00010006, "Wrong codeblock length.");
        return false;
    }

    let lcup = lengths1 as i32;
    let scup = ((*coded_data.add((lcup - 1) as usize) as i32) << 4)
        + (*coded_data.add((lcup - 2) as usize) as i32 & 0xF);
    if scup < 2 || scup > lcup || scup > 4079 {
        return false;
    }

    let mut scratch = [0u16; 8 * 513];
    let sstr = ((width + 2) + 7) & !7;
    let mmsbp2 = missing_msbs + 2;

    decode_step1_vlc_mel_wasm(coded_data, lcup, scup, width, height, sstr, &mut scratch);
    if !decode_step2_magsgn_wasm(
        coded_data,
        decoded_data,
        p,
        mmsbp2,
        lcup,
        scup,
        width,
        height,
        stride,
        sstr,
        &scratch,
    ) {
        return false;
    }

    let mstr = rearrange_sigma_wasm(width, height, sstr, &mut scratch);
    if num_passes > 1 {
        decode_spp_wasm(
            coded_data,
            decoded_data,
            p,
            lengths1,
            lengths2,
            width,
            height,
            stride,
            mstr,
            stripe_causal,
            &mut scratch,
        );

        if num_passes > 2 {
            decode_mrp_wasm(
                coded_data,
                decoded_data,
                p,
                lengths1,
                lengths2,
                width,
                height,
                stride,
                mstr,
                &scratch,
            );
        }
    }

    true
}
