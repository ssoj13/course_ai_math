//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/coding/ojph_block_decoder32.cpp`

use core::ptr::read_unaligned;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ojph_warn;
use crate::openjph::ojph_arch::{count_leading_zeros_u32, population_count};
use crate::openjph::ojph_defs::{ojph_max, Si32, Ui16, Ui32, Ui64, Ui8};

use super::ojph_block_common::{uvlc_tbl0, uvlc_tbl1, vlc_tbl0, vlc_tbl1};
use super::ojph_block_decoder::{
    dec_mel_st, mel_get_run, mel_init, rev_advance, rev_advance_mrp, rev_fetch, rev_fetch_mrp,
    rev_init, rev_init_mrp, rev_struct,
};

static INSUFFICIENT_PRECISION32: AtomicBool = AtomicBool::new(false);
static MODIFY_CODE32: AtomicBool = AtomicBool::new(false);
static TRUNCATE_SPP_MRP32: AtomicBool = AtomicBool::new(false);

#[allow(non_camel_case_types)]
pub(crate) struct frwd_struct32 {
    pub(crate) data: *const Ui8,
    pub(crate) tmp: Ui64,
    pub(crate) bits: Ui32,
    pub(crate) unstuff: Ui32,
    pub(crate) size: i32,
}

impl Default for frwd_struct32 {
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

#[inline]
pub(crate) unsafe fn frwd_read_32<const X: i32>(msp: &mut frwd_struct32) {
    debug_assert!(msp.bits <= 32);

    let mut val: u32;
    if msp.size > 3 {
        val = unsafe { read_unaligned(msp.data as *const Ui32) };
        msp.data = unsafe { msp.data.add(4) };
        msp.size -= 4;
    } else if msp.size > 0 {
        let mut i = 0;
        val = if X != 0 { 0xFFFF_FFFF } else { 0 };
        while msp.size > 0 {
            let v = unsafe { *msp.data } as Ui32;
            msp.data = unsafe { msp.data.add(1) };
            let m = !(0xFF_u32 << i);
            val = (val & m) | (v << i);
            msp.size -= 1;
            i += 8;
        }
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

#[inline]
pub(crate) unsafe fn frwd_init_32<const X: i32>(
    msp: &mut frwd_struct32,
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

    unsafe { frwd_read_32::<X>(msp) };
}

#[inline]
pub(crate) fn frwd_advance_32(msp: &mut frwd_struct32, num_bits: Ui32) {
    debug_assert!(num_bits <= msp.bits);
    msp.tmp >>= num_bits;
    msp.bits -= num_bits;
}

#[inline]
pub(crate) unsafe fn frwd_fetch_32<const X: i32>(msp: &mut frwd_struct32) -> Ui32 {
    if msp.bits < 32 {
        unsafe { frwd_read_32::<X>(msp) };
        if msp.bits < 32 {
            unsafe { frwd_read_32::<X>(msp) };
        }
    }
    msp.tmp as Ui32
}
#[inline]
pub(crate) unsafe fn decode_step1_vlc_mel_32(
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
        rev_advance(&mut vlc, len);
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
            rev_advance(&mut vlc, len);
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

#[inline]
pub(crate) fn rearrange_sigma_32(
    width: Ui32,
    height: Ui32,
    sstr: Ui32,
    scratch: &mut [Ui16],
) -> Ui32 {
    let mstr = (((width + 3) >> 2) + 2 + 7) & !7;

    let mut y = 0;
    while y < height {
        let mut sp = ((y >> 1) * sstr) as usize;
        let mut dp = ((y >> 2) * mstr) as usize;
        let next_row = sstr as usize;

        for _x in (0..width).step_by(4) {
            let mut t0 = ((scratch[sp] as Ui32 & 0x30) >> 4) | ((scratch[sp] as Ui32 & 0xC0) >> 2);
            t0 |= ((scratch[sp + 2] as Ui32 & 0x30) << 4) | ((scratch[sp + 2] as Ui32 & 0xC0) << 6);
            let mut t1 = ((scratch[sp + next_row] as Ui32 & 0x30) >> 2)
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

#[inline]
pub(crate) unsafe fn decode_magsgn_value_32(
    magsgn: &mut frwd_struct32,
    inf: Ui32,
    bit: Ui32,
    u_q: Ui32,
    p: Ui32,
) -> (Ui32, Ui32) {
    if (inf & (1 << (4 + bit))) == 0 {
        return (0, 0);
    }

    let ms_val = unsafe { frwd_fetch_32::<0xFF>(magsgn) };
    let m_n = u_q - ((inf >> (12 + bit)) & 1);
    frwd_advance_32(magsgn, m_n);

    let mut val = ms_val << 31;
    let mut v_n = ms_val & ((1 << m_n) - 1);
    v_n |= ((inf >> (8 + bit)) & 1) << m_n;
    v_n |= 1;
    val |= (v_n + 2) << (p - 1);
    (val, v_n)
}

#[inline]
pub(crate) unsafe fn decode_step2_magsgn_32(
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
    scratch: &[Ui16],
) -> bool {
    let mut v_n_scratch = [0u32; 512 + 4];
    let mut magsgn = frwd_struct32::default();
    unsafe { frwd_init_32::<0xFF>(&mut magsgn, coded_data as *const Ui8, lcup - scup) };

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

        let (val0, _) = unsafe { decode_magsgn_value_32(&mut magsgn, inf, 0, u_q, p) };
        unsafe { *dp = val0 };

        let (val1, v1) = unsafe { decode_magsgn_value_32(&mut magsgn, inf, 1, u_q, p) };
        unsafe { *dp.add(stride as usize) = val1 };
        v_n_scratch[vp] = prev_v_n | v1;
        prev_v_n = 0;
        dp = unsafe { dp.add(1) };
        x += 1;
        if x >= width {
            vp += 1;
            break;
        }

        let (val2, _) = unsafe { decode_magsgn_value_32(&mut magsgn, inf, 2, u_q, p) };
        unsafe { *dp = val2 };

        let (val3, v3) = unsafe { decode_magsgn_value_32(&mut magsgn, inf, 3, u_q, p) };
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

            let (val0, _) = unsafe { decode_magsgn_value_32(&mut magsgn, inf, 0, u_q, p) };
            unsafe { *dp = val0 };

            let (val1, v1) = unsafe { decode_magsgn_value_32(&mut magsgn, inf, 1, u_q, p) };
            unsafe { *dp.add(stride as usize) = val1 };
            v_n_scratch[vp] = prev_v_n | v1;
            prev_v_n = 0;
            dp = unsafe { dp.add(1) };
            x += 1;
            if x >= width {
                vp += 1;
                break;
            }

            let (val2, _) = unsafe { decode_magsgn_value_32(&mut magsgn, inf, 2, u_q, p) };
            unsafe { *dp = val2 };

            let (val3, v3) = unsafe { decode_magsgn_value_32(&mut magsgn, inf, 3, u_q, p) };
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

#[inline]
pub(crate) unsafe fn decode_spp_32(
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
    sigma: &mut [Ui16],
) {
    let mut prev_row_sig = [0u16; 256 + 8];
    let mut sigprop = frwd_struct32::default();
    unsafe {
        frwd_init_32::<0>(
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
            let mut s = x as Si32 + 4 - width as Si32;
            s = ojph_max(s, 0);
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
                let mut cwd = unsafe { frwd_fetch_32::<0>(&mut sigprop) };
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

                frwd_advance_32(&mut sigprop, cnt);
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

#[inline]
pub(crate) unsafe fn decode_mrp_32(
    coded_data: *mut Ui8,
    decoded_data: *mut Ui32,
    p: Ui32,
    lengths1: Ui32,
    lengths2: Ui32,
    width: Ui32,
    height: Ui32,
    stride: Ui32,
    mstr: Ui32,
    sigma: &[Ui16],
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
            rev_advance_mrp(&mut magref, population_count(sig));
        }
    }
}

pub(crate) unsafe fn ojph_decode_codeblock32(
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
        if !INSUFFICIENT_PRECISION32.swap(true, Ordering::Relaxed) {
            ojph_warn!(
                0x00010003,
                "32 bits are not enough to decode this codeblock. This message will not be displayed again."
            );
        }
        return false;
    } else if missing_msbs == 30 {
        if !MODIFY_CODE32.swap(true, Ordering::Relaxed) {
            ojph_warn!(
                0x00010004,
                "Not enough precision to decode the cleanup pass. The code can be modified to support this case. This message will not be displayed again."
            );
        }
        return false;
    } else if missing_msbs == 29 {
        if num_passes > 1 {
            num_passes = 1;
            if !TRUNCATE_SPP_MRP32.swap(true, Ordering::Relaxed) {
                ojph_warn!(
                    0x00010005,
                    "Not enough precision to decode the SgnProp nor MagRef passes; both will be skipped. This message will not be displayed again."
                );
            }
        }
    }

    let p = 30 - missing_msbs;

    if lengths1 < 2 {
        return false;
    }

    let lcup = lengths1 as i32;
    let scup = ((unsafe { *coded_data.add((lcup - 1) as usize) } as i32) << 4)
        + (unsafe { *coded_data.add((lcup - 2) as usize) } as i32 & 0xF);
    if scup < 2 || scup > lcup || scup > 4079 {
        return false;
    }

    let mut scratch = [0u16; 8 * 513];
    let sstr = ((width + 2) + 7) & !7;
    let mmsbp2 = missing_msbs + 2;

    unsafe { decode_step1_vlc_mel_32(coded_data, lcup, scup, width, height, sstr, &mut scratch) };

    if !unsafe {
        decode_step2_magsgn_32(
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
        )
    } {
        return false;
    }

    let mstr = rearrange_sigma_32(width, height, sstr, &mut scratch);

    if num_passes > 1 {
        unsafe {
            decode_spp_32(
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
            )
        };

        if num_passes > 2 {
            unsafe {
                decode_mrp_32(
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
                )
            };
        }
    }

    true
}
