//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/coding/ojph_block_decoder_ssse3.cpp`

use core::ptr::read_unaligned;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ojph_warn;
use crate::openjph::ojph_arch::count_leading_zeros_u32;
use crate::openjph::ojph_defs::{Ui16, Ui32, Ui8};

use super::ojph_block_decoder32::decode_step1_vlc_mel_32;
use super::ojph_block_decoder::{rev_advance_mrp, rev_fetch_mrp, rev_init_mrp, rev_struct};


#[cfg(target_arch = "x86")]
use core::arch::x86::{
    __m128i, _mm_abs_epi8, _mm_add_epi16, _mm_add_epi32, _mm_add_epi8, _mm_and_si128,
    _mm_andnot_si128, _mm_bslli_si128, _mm_bsrli_si128, _mm_cmpeq_epi16, _mm_cmpeq_epi32,
    _mm_cmpeq_epi8, _mm_cmpgt_epi32, _mm_cmpgt_epi8, _mm_cvtsi32_si128, _mm_extract_epi16,
    _mm_load_si128, _mm_loadu_si128, _mm_max_epi16, _mm_min_epu8, _mm_movemask_epi8,
    _mm_mullo_epi16, _mm_or_si128, _mm_set1_epi16, _mm_set1_epi32, _mm_set1_epi64x, _mm_set1_epi8,
    _mm_set_epi16, _mm_set_epi32, _mm_set_epi64x, _mm_set_epi8, _mm_setzero_si128,
    _mm_shuffle_epi32, _mm_shuffle_epi8, _mm_sll_epi16, _mm_sll_epi32, _mm_sll_epi64,
    _mm_slli_epi16, _mm_slli_epi32, _mm_slli_si128, _mm_srl_epi64, _mm_srli_epi16, _mm_srli_epi32,
    _mm_srli_epi64, _mm_srli_si128, _mm_store_si128, _mm_storeu_si128, _mm_sub_epi16,
    _mm_sub_epi32, _mm_unpackhi_epi32, _mm_unpacklo_epi32, _mm_xor_si128,
};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{
    __m128i, _mm_abs_epi8, _mm_add_epi16, _mm_add_epi32, _mm_add_epi8, _mm_and_si128,
    _mm_andnot_si128, _mm_bslli_si128, _mm_bsrli_si128, _mm_cmpeq_epi16, _mm_cmpeq_epi32,
    _mm_cmpeq_epi8, _mm_cmpgt_epi32, _mm_cmpgt_epi8, _mm_cvtsi32_si128, _mm_extract_epi16,
    _mm_load_si128, _mm_loadu_si128, _mm_max_epi16, _mm_min_epu8, _mm_movemask_epi8,
    _mm_mullo_epi16, _mm_or_si128, _mm_set1_epi16, _mm_set1_epi32, _mm_set1_epi64x, _mm_set1_epi8,
    _mm_set_epi16, _mm_set_epi32, _mm_set_epi64x, _mm_set_epi8, _mm_setzero_si128,
    _mm_shuffle_epi32, _mm_shuffle_epi8, _mm_sll_epi16, _mm_sll_epi32, _mm_sll_epi64,
    _mm_slli_epi16, _mm_slli_epi32, _mm_slli_si128, _mm_srl_epi64, _mm_srli_epi16, _mm_srli_epi32,
    _mm_srli_epi64, _mm_srli_si128, _mm_store_si128, _mm_storeu_si128, _mm_sub_epi16,
    _mm_sub_epi32, _mm_unpackhi_epi32, _mm_unpacklo_epi32, _mm_xor_si128,
};

static INSUFFICIENT_PRECISION_SSSE3: AtomicBool = AtomicBool::new(false);
static MODIFY_CODE_SSSE3: AtomicBool = AtomicBool::new(false);
static TRUNCATE_SPP_MRP_SSSE3: AtomicBool = AtomicBool::new(false);

#[allow(non_camel_case_types)]
#[repr(C)]
pub(crate) struct frwd_struct_ssse3 {
    pub(crate) data: *const Ui8,
    pub(crate) tmp: [Ui8; 48],
    pub(crate) bits: Ui32,
    pub(crate) unstuff: Ui32,
    pub(crate) size: i32,
}

impl Default for frwd_struct_ssse3 {
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

/// Read up to 16 bytes from the forward bitstream into the SSSE3 accumulator.
///
/// Handles JPEG 2000 byte-stuffing: after each 0xFF byte, the MSB of the
/// next byte is a stuff bit that must be removed. This is done via a 128-bit
/// right-shift-by-1 that bridges the two 64-bit lanes:
///
/// ```text
/// carry = (hi_part & mask) >> 1          // shift right within each lane
/// bridge = hi_qword_to_low >> 64, << 63  // carry bit 0 of high into bit 63 of low
/// result = carry | bridge
/// ```
///
/// BUG HISTORY: a spurious `_mm_slli_si128(hi, 8)` after `_mm_srli_si128(hi, 8)`
/// was putting the carry bit back into the high qword instead of the low one,
/// corrupting the unstuffed bitstream. This caused decoded codeblock values to
/// differ from C++ by powers of 2. Same bug existed in the AVX2 variant.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
pub(crate) unsafe fn frwd_read_ssse3<const X: i32>(msp: &mut frwd_struct_ssse3) {
    debug_assert!(msp.bits <= 128);

    let mut val = _mm_loadu_si128(msp.data as *const __m128i);
    let bytes = if msp.size >= 16 { 16 } else { msp.size };
    let mut validity = _mm_set1_epi8(bytes as i8);
    msp.data = msp.data.add(bytes as usize);
    msp.size -= bytes;
    let mut bits = 128;
    let offset = _mm_set_epi64x(0x0F0E0D0C0B0A0908, 0x0706050403020100);
    validity = _mm_cmpgt_epi8(validity, offset);
    let all_xff = _mm_set1_epi8(-1);
    if X == 0xFF {
        let t = _mm_xor_si128(validity, all_xff);
        val = _mm_or_si128(t, val);
    } else if X == 0 {
        val = _mm_and_si128(validity, val);
    } else {
        debug_assert!(false);
    }

    let mut ff_bytes = _mm_cmpeq_epi8(val, all_xff);
    ff_bytes = _mm_and_si128(ff_bytes, validity);
    let mut flags = _mm_movemask_epi8(ff_bytes) as Ui32;
    flags <<= 1;
    let next_unstuff = flags >> 16;
    flags |= msp.unstuff;
    flags &= 0xFFFF;
    while flags != 0 {
        bits -= 1;

        let loc = 31 - count_leading_zeros_u32(flags);
        flags ^= 1 << loc;

        let t = _mm_set1_epi8(loc as i8);
        let m = _mm_cmpgt_epi8(offset, t);

        // 128-bit right-shift by 1 across 64-bit lane boundary.
        // Steps: (1) shift each lane right by 1, (2) move high qword
        // to low position, (3) isolate bit 0 as bit 63 (the carry),
        // (4) OR carry into the shifted result.
        let mut hi = _mm_and_si128(m, val);
        let carry = _mm_srli_epi64(hi, 1);
        hi = _mm_srli_si128(hi, 8); // high qword → low position
                                    // NOTE: no _mm_slli_si128 here — that would undo the move!
        hi = _mm_sll_epi64(hi, _mm_set1_epi64x(63)); // carry bit at bit 63
        hi = _mm_or_si128(hi, carry);

        val = _mm_or_si128(hi, _mm_andnot_si128(m, val));
    }

    let mut cur_bytes = (msp.bits >> 3) as usize;
    let cur_bits = (msp.bits & 7) as i64;
    let mut b1 = _mm_sll_epi64(val, _mm_set1_epi64x(cur_bits));
    let mut b2 = _mm_slli_si128(val, 8);
    b2 = _mm_srl_epi64(b2, _mm_set1_epi64x(64 - cur_bits));
    b1 = _mm_or_si128(b1, b2);
    b2 = _mm_loadu_si128(msp.tmp.as_ptr().add(cur_bytes) as *const __m128i);
    b2 = _mm_or_si128(b1, b2);
    _mm_storeu_si128(msp.tmp.as_mut_ptr().add(cur_bytes) as *mut __m128i, b2);

    let consumed_bits = if bits < 128 - cur_bits as i32 {
        bits
    } else {
        128 - cur_bits as i32
    };
    cur_bytes = ((msp.bits + consumed_bits as Ui32 + 7) >> 3) as usize;
    let mut upper = _mm_extract_epi16(val, 7);
    upper >>= consumed_bits - 128 + 16;
    msp.tmp[cur_bytes] = upper as Ui8;

    msp.bits += bits as Ui32;
    msp.unstuff = next_unstuff;
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
pub(crate) unsafe fn frwd_init_ssse3<const X: i32>(
    msp: &mut frwd_struct_ssse3,
    data: *const Ui8,
    size: i32,
) {
    msp.data = data;
    _mm_storeu_si128(msp.tmp.as_mut_ptr() as *mut __m128i, _mm_setzero_si128());
    _mm_storeu_si128(
        msp.tmp.as_mut_ptr().add(16) as *mut __m128i,
        _mm_setzero_si128(),
    );
    _mm_storeu_si128(
        msp.tmp.as_mut_ptr().add(32) as *mut __m128i,
        _mm_setzero_si128(),
    );
    msp.bits = 0;
    msp.unstuff = 0;
    msp.size = size;
    frwd_read_ssse3::<X>(msp);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
pub(crate) unsafe fn frwd_advance_ssse3(msp: &mut frwd_struct_ssse3, mut num_bits: Ui32) {
    debug_assert!(num_bits > 0 && num_bits <= msp.bits && num_bits < 128);
    msp.bits -= num_bits;

    let p = msp.tmp.as_mut_ptr().add(((num_bits >> 3) & 0x18) as usize) as *mut __m128i;
    num_bits &= 63;

    let v0 = _mm_loadu_si128(p);
    let v1 = _mm_loadu_si128(p.add(1));

    let mut c0 = _mm_srl_epi64(v0, _mm_set1_epi64x(num_bits as i64));
    let mut t = _mm_srli_si128(v0, 8);
    t = _mm_sll_epi64(t, _mm_set1_epi64x(64 - num_bits as i64));
    c0 = _mm_or_si128(c0, t);
    t = _mm_slli_si128(v1, 8);
    t = _mm_sll_epi64(t, _mm_set1_epi64x(64 - num_bits as i64));
    c0 = _mm_or_si128(c0, t);
    _mm_storeu_si128(msp.tmp.as_mut_ptr() as *mut __m128i, c0);

    let mut c1 = _mm_srl_epi64(v1, _mm_set1_epi64x(num_bits as i64));
    t = _mm_srli_si128(v1, 8);
    t = _mm_sll_epi64(t, _mm_set1_epi64x(64 - num_bits as i64));
    c1 = _mm_or_si128(c1, t);
    _mm_storeu_si128(msp.tmp.as_mut_ptr().add(16) as *mut __m128i, c1);
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
pub(crate) unsafe fn frwd_fetch_ssse3<const X: i32>(msp: &mut frwd_struct_ssse3) -> __m128i {
    if msp.bits <= 128 {
        frwd_read_ssse3::<X>(msp);
        if msp.bits <= 128 {
            frwd_read_ssse3::<X>(msp);
        }
    }
    _mm_loadu_si128(msp.tmp.as_ptr() as *const __m128i)
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
pub(crate) unsafe fn decode_one_quad32_ssse3<const N: i32>(
    inf_u_q: __m128i,
    mut u_q: __m128i,
    magsgn: &mut frwd_struct_ssse3,
    p: Ui32,
    vn: &mut __m128i,
) -> __m128i {
    let mut row = _mm_setzero_si128();
    let mut w0 = _mm_shuffle_epi32(inf_u_q, N);
    let mut flags = _mm_and_si128(w0, _mm_set_epi32(0x8880, 0x4440, 0x2220, 0x1110));
    let insig = _mm_cmpeq_epi32(flags, _mm_setzero_si128());
    if _mm_movemask_epi8(insig) != 0xFFFF {
        u_q = _mm_shuffle_epi32(u_q, N);
        flags = _mm_mullo_epi16(flags, _mm_set_epi16(1, 1, 2, 2, 4, 4, 8, 8));
        let mut ms_vec = frwd_fetch_ssse3::<0xFF>(magsgn);

        w0 = _mm_srli_epi32(flags, 15);
        let mut m_n = _mm_sub_epi32(u_q, w0);
        m_n = _mm_andnot_si128(insig, m_n);

        let mut inc_sum = m_n;
        inc_sum = _mm_add_epi32(inc_sum, _mm_bslli_si128(inc_sum, 4));
        inc_sum = _mm_add_epi32(inc_sum, _mm_bslli_si128(inc_sum, 8));
        let total_mn = _mm_extract_epi16(inc_sum, 6);
        let ex_sum = _mm_bslli_si128(inc_sum, 4);

        let mut byte_idx = _mm_srli_epi32(ex_sum, 3);
        let mut bit_idx = _mm_and_si128(ex_sum, _mm_set1_epi32(7));
        byte_idx = _mm_shuffle_epi8(
            byte_idx,
            _mm_set_epi32(0x0C0C0C0C, 0x08080808, 0x04040404, 0x00000000),
        );
        byte_idx = _mm_add_epi32(byte_idx, _mm_set1_epi32(0x03020100));
        let mut d0 = _mm_shuffle_epi8(ms_vec, byte_idx);
        byte_idx = _mm_add_epi32(byte_idx, _mm_set1_epi32(0x01010101));
        let mut d1 = _mm_shuffle_epi8(ms_vec, byte_idx);

        bit_idx = _mm_or_si128(bit_idx, _mm_slli_epi32(bit_idx, 16));
        let mut bit_shift = _mm_shuffle_epi8(
            _mm_set_epi8(1, 3, 7, 15, 31, 63, 127, -1, 1, 3, 7, 15, 31, 63, 127, -1),
            bit_idx,
        );
        bit_shift = _mm_add_epi16(bit_shift, _mm_set1_epi16(0x0101));
        d0 = _mm_mullo_epi16(d0, bit_shift);
        d0 = _mm_srli_epi16(d0, 8);
        d1 = _mm_mullo_epi16(d1, bit_shift);
        d1 = _mm_and_si128(d1, _mm_set1_epi32(0xFF00FF00_u32 as i32));
        d0 = _mm_or_si128(d0, d1);

        let ones = _mm_set1_epi32(1);
        let twos = _mm_set1_epi32(2);
        let mut u_q_m1 = _mm_sub_epi32(u_q, ones);
        u_q_m1 = _mm_and_si128(u_q_m1, _mm_set_epi32(0, 0, 0, 0x1F));
        w0 = _mm_sub_epi32(twos, w0);
        let shift = _mm_sll_epi32(w0, u_q_m1);
        ms_vec = _mm_and_si128(d0, _mm_sub_epi32(shift, ones));

        w0 = _mm_and_si128(flags, _mm_set1_epi32(0x800));
        w0 = _mm_cmpeq_epi32(w0, _mm_setzero_si128());
        w0 = _mm_andnot_si128(w0, shift);
        ms_vec = _mm_or_si128(ms_vec, w0);
        w0 = _mm_slli_epi32(ms_vec, 31);
        ms_vec = _mm_or_si128(ms_vec, ones);
        let mut tvn = ms_vec;
        ms_vec = _mm_add_epi32(ms_vec, twos);
        ms_vec = _mm_sll_epi32(ms_vec, _mm_cvtsi32_si128(p as i32 - 1));
        ms_vec = _mm_or_si128(ms_vec, w0);
        row = _mm_andnot_si128(insig, ms_vec);

        ms_vec = _mm_andnot_si128(insig, tvn);
        if N == 0x00 {
            tvn = _mm_shuffle_epi8(ms_vec, _mm_set_epi32(-1, -1, 0x0F0E0D0C, 0x07060504));
        } else if N == 0x55 {
            tvn = _mm_shuffle_epi8(ms_vec, _mm_set_epi32(-1, 0x0F0E0D0C, 0x07060504, -1));
        }
        *vn = _mm_or_si128(*vn, tvn);

        if total_mn != 0 {
            frwd_advance_ssse3(magsgn, total_mn as Ui32);
        }
    }
    row
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
pub(crate) unsafe fn decode_two_quad16_ssse3(
    inf_u_q: __m128i,
    mut u_q: __m128i,
    magsgn: &mut frwd_struct_ssse3,
    p: Ui32,
    vn: &mut __m128i,
) -> __m128i {
    let mut row = _mm_setzero_si128();
    let mut w0 = _mm_shuffle_epi8(
        inf_u_q,
        _mm_set_epi16(
            0x0504, 0x0504, 0x0504, 0x0504, 0x0100, 0x0100, 0x0100, 0x0100,
        ),
    );
    let mut flags = _mm_and_si128(
        w0,
        _mm_set_epi16(
            0x8880_u16 as i16,
            0x4440,
            0x2220,
            0x1110,
            0x8880_u16 as i16,
            0x4440,
            0x2220,
            0x1110,
        ),
    );
    let insig = _mm_cmpeq_epi16(flags, _mm_setzero_si128());
    if _mm_movemask_epi8(insig) != 0xFFFF {
        u_q = _mm_shuffle_epi8(
            u_q,
            _mm_set_epi16(
                0x0504, 0x0504, 0x0504, 0x0504, 0x0100, 0x0100, 0x0100, 0x0100,
            ),
        );
        flags = _mm_mullo_epi16(flags, _mm_set_epi16(1, 2, 4, 8, 1, 2, 4, 8));
        let mut ms_vec = frwd_fetch_ssse3::<0xFF>(magsgn);

        w0 = _mm_srli_epi16(flags, 15);
        let mut m_n = _mm_sub_epi16(u_q, w0);
        m_n = _mm_andnot_si128(insig, m_n);

        let mut inc_sum = m_n;
        inc_sum = _mm_add_epi16(inc_sum, _mm_bslli_si128(inc_sum, 2));
        inc_sum = _mm_add_epi16(inc_sum, _mm_bslli_si128(inc_sum, 4));
        inc_sum = _mm_add_epi16(inc_sum, _mm_bslli_si128(inc_sum, 8));
        let total_mn = _mm_extract_epi16(inc_sum, 7);
        let ex_sum = _mm_bslli_si128(inc_sum, 2);

        let mut byte_idx = _mm_srli_epi16(ex_sum, 3);
        let bit_idx = _mm_and_si128(ex_sum, _mm_set1_epi16(7));
        byte_idx = _mm_shuffle_epi8(
            byte_idx,
            _mm_set_epi16(
                0x0E0E, 0x0C0C, 0x0A0A, 0x0808, 0x0606, 0x0404, 0x0202, 0x0000,
            ),
        );
        byte_idx = _mm_add_epi16(byte_idx, _mm_set1_epi16(0x0100));
        let mut d0 = _mm_shuffle_epi8(ms_vec, byte_idx);
        byte_idx = _mm_add_epi16(byte_idx, _mm_set1_epi16(0x0101));
        let mut d1 = _mm_shuffle_epi8(ms_vec, byte_idx);

        let mut bit_shift = _mm_shuffle_epi8(
            _mm_set_epi8(1, 3, 7, 15, 31, 63, 127, -1, 1, 3, 7, 15, 31, 63, 127, -1),
            bit_idx,
        );
        bit_shift = _mm_add_epi16(bit_shift, _mm_set1_epi16(0x0101));
        d0 = _mm_mullo_epi16(d0, bit_shift);
        d0 = _mm_srli_epi16(d0, 8);
        d1 = _mm_mullo_epi16(d1, bit_shift);
        d1 = _mm_and_si128(d1, _mm_set1_epi16(0xFF00_u16 as i16));
        d0 = _mm_or_si128(d0, d1);

        let ones = _mm_set1_epi16(1);
        let twos = _mm_set1_epi16(2);
        let u_q_m1 = _mm_sub_epi32(u_q, ones);
        let uq0 = _mm_and_si128(u_q_m1, _mm_set_epi32(0, 0, 0, 0x1F));
        let uq1 = _mm_bsrli_si128(u_q_m1, 14);
        w0 = _mm_sub_epi16(twos, w0);
        let mut t0 = _mm_and_si128(w0, _mm_set_epi64x(0, -1));
        let mut t1 = _mm_and_si128(w0, _mm_set_epi64x(-1, 0));
        t0 = _mm_sll_epi16(t0, uq0);
        t1 = _mm_sll_epi16(t1, uq1);
        let shift = _mm_or_si128(t0, t1);
        ms_vec = _mm_and_si128(d0, _mm_sub_epi16(shift, ones));

        w0 = _mm_and_si128(flags, _mm_set1_epi16(0x800));
        w0 = _mm_cmpeq_epi16(w0, _mm_setzero_si128());
        w0 = _mm_andnot_si128(w0, shift);
        ms_vec = _mm_or_si128(ms_vec, w0);
        w0 = _mm_slli_epi16(ms_vec, 15);
        ms_vec = _mm_or_si128(ms_vec, ones);
        let tvn = ms_vec;
        ms_vec = _mm_add_epi16(ms_vec, twos);
        ms_vec = _mm_sll_epi16(ms_vec, _mm_cvtsi32_si128(p as i32 - 1));
        ms_vec = _mm_or_si128(ms_vec, w0);
        row = _mm_andnot_si128(insig, ms_vec);

        ms_vec = _mm_andnot_si128(insig, tvn);
        w0 = _mm_shuffle_epi8(
            ms_vec,
            _mm_set_epi16(-1, -1, -1, -1, -1, -1, 0x0706, 0x0302),
        );
        *vn = _mm_or_si128(*vn, w0);
        w0 = _mm_shuffle_epi8(
            ms_vec,
            _mm_set_epi16(-1, -1, -1, -1, -1, 0x0F0E, 0x0B0A, -1),
        );
        *vn = _mm_or_si128(*vn, w0);

        if total_mn != 0 {
            frwd_advance_ssse3(magsgn, total_mn as Ui32);
        }
    }
    row
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
unsafe fn decode_step2_magsgn_ssse3(
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
    scratch: &[Ui16; 8 * 513],
) -> bool {
    if mmsbp2 >= 16 {
        const V_N_SIZE: usize = 512 + 8;
        let mut v_n_scratch = [0u32; 2 * V_N_SIZE];
        let mut magsgn = frwd_struct_ssse3::default();
        frwd_init_ssse3::<0xFF>(&mut magsgn, coded_data as *const Ui8, lcup - scup);

        {
            let mut sp = scratch.as_ptr();
            let mut vp = v_n_scratch.as_mut_ptr();
            let mut dp = decoded_data;
            *vp = 2;

            let mut x = 0;
            while x < width {
                let inf_u_q = _mm_loadu_si128(sp as *const __m128i);
                let u_q = _mm_srli_epi32(inf_u_q, 16);
                let w0 = _mm_cmpgt_epi32(u_q, _mm_set1_epi32(mmsbp2 as i32));
                if (_mm_movemask_epi8(w0) & 0xFF) != 0 {
                    return false;
                }

                let mut vn = _mm_set1_epi32(2);
                let mut row0 =
                    decode_one_quad32_ssse3::<0x00>(inf_u_q, u_q, &mut magsgn, p, &mut vn);
                let mut row1 =
                    decode_one_quad32_ssse3::<0x55>(inf_u_q, u_q, &mut magsgn, p, &mut vn);
                let mut w0 = _mm_loadu_si128(vp as *const __m128i);
                w0 = _mm_and_si128(w0, _mm_set_epi32(0, 0, 0, -1));
                w0 = _mm_or_si128(w0, vn);
                _mm_storeu_si128(vp as *mut __m128i, w0);

                w0 = _mm_unpacklo_epi32(row0, row1);
                let w1 = _mm_unpackhi_epi32(row0, row1);
                row0 = _mm_unpacklo_epi32(w0, w1);
                row1 = _mm_unpackhi_epi32(w0, w1);
                _mm_store_si128(dp as *mut __m128i, row0);
                _mm_store_si128(dp.add(stride as usize) as *mut __m128i, row1);

                x += 4;
                sp = sp.add(4);
                vp = vp.add(2);
                dp = dp.add(4);
            }
        }

        let mut y = 2;
        while y < height {
            {
                let mut vp = v_n_scratch.as_mut_ptr();
                let lut_lo = _mm_set_epi8(4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 6, 6, 7, 31);
                let lut_hi = _mm_set_epi8(0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 31);
                let nibble_mask = _mm_set1_epi8(0x0F);
                let byte_offset8 = _mm_set1_epi16(8);
                let byte_offset16 = _mm_set1_epi16(16);
                let cc = _mm_set1_epi32(31);

                let mut x = 0;
                while x <= width {
                    let mut v = _mm_loadu_si128(vp as *const __m128i);
                    let mut t = _mm_and_si128(nibble_mask, v);
                    v = _mm_and_si128(_mm_srli_epi16(v, 4), nibble_mask);
                    t = _mm_shuffle_epi8(lut_lo, t);
                    v = _mm_shuffle_epi8(lut_hi, v);
                    v = _mm_min_epu8(v, t);

                    t = _mm_srli_epi16(v, 8);
                    v = _mm_or_si128(v, byte_offset8);
                    v = _mm_min_epu8(v, t);

                    t = _mm_srli_epi32(v, 16);
                    v = _mm_or_si128(v, byte_offset16);
                    v = _mm_min_epu8(v, t);

                    v = _mm_sub_epi16(cc, v);
                    _mm_storeu_si128(vp.add(V_N_SIZE) as *mut __m128i, v);

                    x += 8;
                    vp = vp.add(4);
                }
            }

            let mut vp = v_n_scratch.as_mut_ptr();
            let mut sp = scratch.as_ptr().add(((y >> 1) * sstr) as usize);
            let mut dp = decoded_data.add((y * stride) as usize);
            *vp = 2;

            let mut x = 0;
            while x < width {
                let inf_u_q = _mm_loadu_si128(sp as *const __m128i);
                let gamma = _mm_and_si128(inf_u_q, _mm_set1_epi32(0xF0));
                let mut w0 = _mm_sub_epi32(gamma, _mm_set1_epi32(1));
                let gamma = _mm_cmpeq_epi32(_mm_and_si128(gamma, w0), _mm_setzero_si128());

                let mut emax = _mm_loadu_si128(vp.add(V_N_SIZE) as *const __m128i);
                w0 = _mm_bsrli_si128(emax, 4);
                emax = _mm_max_epi16(w0, emax);
                emax = _mm_andnot_si128(gamma, emax);

                let mut kappa = _mm_set1_epi32(1);
                kappa = _mm_max_epi16(emax, kappa);

                let u_q = _mm_srli_epi32(inf_u_q, 16);
                let u_q = _mm_add_epi32(u_q, kappa);

                let w0 = _mm_cmpgt_epi32(u_q, _mm_set1_epi32(mmsbp2 as i32));
                if (_mm_movemask_epi8(w0) & 0xFF) != 0 {
                    return false;
                }

                let mut vn = _mm_set1_epi32(2);
                let mut row0 =
                    decode_one_quad32_ssse3::<0x00>(inf_u_q, u_q, &mut magsgn, p, &mut vn);
                let mut row1 =
                    decode_one_quad32_ssse3::<0x55>(inf_u_q, u_q, &mut magsgn, p, &mut vn);
                let mut w0 = _mm_loadu_si128(vp as *const __m128i);
                w0 = _mm_and_si128(w0, _mm_set_epi32(0, 0, 0, -1));
                w0 = _mm_or_si128(w0, vn);
                _mm_storeu_si128(vp as *mut __m128i, w0);

                w0 = _mm_unpacklo_epi32(row0, row1);
                let w1 = _mm_unpackhi_epi32(row0, row1);
                row0 = _mm_unpacklo_epi32(w0, w1);
                row1 = _mm_unpackhi_epi32(w0, w1);
                _mm_store_si128(dp as *mut __m128i, row0);
                _mm_store_si128(dp.add(stride as usize) as *mut __m128i, row1);

                x += 4;
                sp = sp.add(4);
                vp = vp.add(2);
                dp = dp.add(4);
            }

            y += 2;
        }
    } else {
        p -= 16;

        const V_N_SIZE: usize = 512 + 8;
        let mut v_n_scratch = [0u16; 2 * V_N_SIZE];
        let mut magsgn = frwd_struct_ssse3::default();
        frwd_init_ssse3::<0xFF>(&mut magsgn, coded_data as *const Ui8, lcup - scup);

        {
            let mut sp = scratch.as_ptr();
            let mut vp = v_n_scratch.as_mut_ptr();
            let mut dp = decoded_data;
            *vp = 2;

            let mut x = 0;
            while x < width {
                let inf_u_q = _mm_loadu_si128(sp as *const __m128i);
                let u_q = _mm_srli_epi32(inf_u_q, 16);
                let w0 = _mm_cmpgt_epi32(u_q, _mm_set1_epi32(mmsbp2 as i32));
                if (_mm_movemask_epi8(w0) & 0xFF) != 0 {
                    return false;
                }

                let mut vn = _mm_set1_epi16(2);
                let row = decode_two_quad16_ssse3(inf_u_q, u_q, &mut magsgn, p, &mut vn);
                let mut w0 = _mm_loadu_si128(vp as *const __m128i);
                w0 = _mm_and_si128(w0, _mm_set_epi16(0, 0, 0, 0, 0, 0, 0, -1));
                w0 = _mm_or_si128(w0, vn);
                _mm_storeu_si128(vp as *mut __m128i, w0);

                w0 = _mm_shuffle_epi8(
                    row,
                    _mm_set_epi16(0x0D0C, -1, 0x0908, -1, 0x0504, -1, 0x0100, -1),
                );
                _mm_store_si128(dp as *mut __m128i, w0);
                let w1 = _mm_shuffle_epi8(
                    row,
                    _mm_set_epi16(0x0F0E, -1, 0x0B0A, -1, 0x0706, -1, 0x0302, -1),
                );
                _mm_store_si128(dp.add(stride as usize) as *mut __m128i, w1);

                x += 4;
                sp = sp.add(4);
                vp = vp.add(2);
                dp = dp.add(4);
            }
        }

        let mut y = 2;
        while y < height {
            {
                let mut vp = v_n_scratch.as_mut_ptr();
                let lut_lo = _mm_set_epi8(4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 6, 6, 7, 15);
                let lut_hi = _mm_set_epi8(0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 3, 15);
                let nibble_mask = _mm_set1_epi8(0x0F);
                let byte_offset8 = _mm_set1_epi16(8);
                let cc = _mm_set1_epi16(15);

                let mut x = 0;
                while x <= width {
                    let mut v = _mm_loadu_si128(vp as *const __m128i);
                    let mut t = _mm_and_si128(nibble_mask, v);
                    v = _mm_and_si128(_mm_srli_epi16(v, 4), nibble_mask);
                    t = _mm_shuffle_epi8(lut_lo, t);
                    v = _mm_shuffle_epi8(lut_hi, v);
                    v = _mm_min_epu8(v, t);

                    t = _mm_srli_epi16(v, 8);
                    v = _mm_or_si128(v, byte_offset8);
                    v = _mm_min_epu8(v, t);

                    v = _mm_sub_epi16(cc, v);
                    _mm_storeu_si128(vp.add(V_N_SIZE) as *mut __m128i, v);

                    x += 16;
                    vp = vp.add(8);
                }
            }

            let mut vp = v_n_scratch.as_mut_ptr();
            let mut sp = scratch.as_ptr().add(((y >> 1) * sstr) as usize);
            let mut dp = decoded_data.add((y * stride) as usize);
            *vp = 2;

            let mut x = 0;
            while x < width {
                let inf_u_q = _mm_loadu_si128(sp as *const __m128i);
                let gamma = _mm_and_si128(inf_u_q, _mm_set1_epi32(0xF0));
                let mut w0 = _mm_sub_epi32(gamma, _mm_set1_epi32(1));
                let gamma = _mm_cmpeq_epi32(_mm_and_si128(gamma, w0), _mm_setzero_si128());

                let mut emax = _mm_loadu_si128(vp.add(V_N_SIZE) as *const __m128i);
                w0 = _mm_bsrli_si128(emax, 2);
                emax = _mm_max_epi16(w0, emax);
                emax = _mm_shuffle_epi8(
                    emax,
                    _mm_set_epi16(-1, 0x0706, -1, 0x0504, -1, 0x0302, -1, 0x0100),
                );
                emax = _mm_andnot_si128(gamma, emax);

                let mut kappa = _mm_set1_epi32(1);
                kappa = _mm_max_epi16(emax, kappa);

                let u_q = _mm_srli_epi32(inf_u_q, 16);
                let u_q = _mm_add_epi32(u_q, kappa);

                let w0 = _mm_cmpgt_epi32(u_q, _mm_set1_epi32(mmsbp2 as i32));
                if (_mm_movemask_epi8(w0) & 0xFF) != 0 {
                    return false;
                }

                let mut vn = _mm_set1_epi16(2);
                let row = decode_two_quad16_ssse3(inf_u_q, u_q, &mut magsgn, p, &mut vn);
                let mut w0 = _mm_loadu_si128(vp as *const __m128i);
                w0 = _mm_and_si128(w0, _mm_set_epi16(0, 0, 0, 0, 0, 0, 0, -1));
                w0 = _mm_or_si128(w0, vn);
                _mm_storeu_si128(vp as *mut __m128i, w0);

                w0 = _mm_shuffle_epi8(
                    row,
                    _mm_set_epi16(0x0D0C, -1, 0x0908, -1, 0x0504, -1, 0x0100, -1),
                );
                _mm_store_si128(dp as *mut __m128i, w0);
                let w1 = _mm_shuffle_epi8(
                    row,
                    _mm_set_epi16(0x0F0E, -1, 0x0B0A, -1, 0x0706, -1, 0x0302, -1),
                );
                _mm_store_si128(dp.add(stride as usize) as *mut __m128i, w1);

                x += 4;
                sp = sp.add(4);
                vp = vp.add(2);
                dp = dp.add(4);
            }

            y += 2;
        }
    }

    true
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
unsafe fn rearrange_sigma_ssse3(
    width: Ui32,
    height: Ui32,
    sstr: Ui32,
    scratch: &mut [Ui16],
) -> Ui32 {
    let sigma = scratch.as_mut_ptr();
    let mut mstr = (width + 3) >> 2;
    mstr = ((mstr + 2) + 7) & !7;

    let mask_3 = _mm_set1_epi32(0x30);
    let mask_c = _mm_set1_epi32(0xC0);
    let shuffle_mask = _mm_set_epi32(-1, -1, -1, 0x0C080400);

    let mut y = 0;
    while y < height {
        let mut sp = scratch.as_ptr().add(((y >> 1) * sstr) as usize);
        let mut dp = sigma.add(((y >> 2) * mstr) as usize);
        let next_row = sstr as usize;

        let mut x = 0;
        while x < width {
            let s0 = _mm_loadu_si128(sp as *const __m128i);
            let mut u3 = _mm_and_si128(s0, mask_3);
            u3 = _mm_srli_epi32(u3, 4);
            let mut uc = _mm_and_si128(s0, mask_c);
            uc = _mm_srli_epi32(uc, 2);
            let t0 = _mm_or_si128(u3, uc);

            let s1 = _mm_loadu_si128(sp.add(next_row) as *const __m128i);
            u3 = _mm_and_si128(s1, mask_3);
            u3 = _mm_srli_epi32(u3, 2);
            uc = _mm_and_si128(s1, mask_c);
            let t1 = _mm_or_si128(u3, uc);

            let mut r = _mm_or_si128(t0, t1);
            r = _mm_shuffle_epi8(r, shuffle_mask);

            *dp = _mm_extract_epi16(r, 0) as Ui16;
            *dp.add(1) = _mm_extract_epi16(r, 1) as Ui16;

            x += 8;
            sp = sp.add(8);
            dp = dp.add(2);
        }
        *dp = 0;

        y += 4;
    }
    // Zero the guard row one stripe past the last codeblock row (y == height here).
    // Mirrors C++ reference: `ui16* dp = sigma + (y >> 2) * mstr` after the y-loop.
    {
        let mut dp = sigma.add(((y >> 2) * mstr) as usize);
        let zero = _mm_setzero_si128();
        let mut x = 0;
        while x < width {
            _mm_storeu_si128(dp as *mut __m128i, zero);
            x += 32;
            dp = dp.add(8);
        }
        *dp = 0;
    }

    mstr
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
unsafe fn decode_spp_ssse3(
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
    let mut sigprop = frwd_struct_ssse3::default();
    frwd_init_ssse3::<0>(
        &mut sigprop,
        coded_data.add(lengths1 as usize) as *const Ui8,
        lengths2 as i32,
    );

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
        let mut prev_sig = prev_row_sig.as_mut_ptr();
        let mut cur_sig = sigma.as_mut_ptr().add(((y >> 2) * mstr) as usize);
        let mut dpp = decoded_data.add((y * stride) as usize);

        for x in (0..width).step_by(4) {
            let mut s = x as i32 + 4 - width as i32;
            if s < 0 {
                s = 0;
            }
            pattern >>= (s * 4) as u32;

            let ps = read_unaligned(prev_sig as *const Ui32);
            let ns = read_unaligned(cur_sig.add(mstr as usize) as *const Ui32);
            let mut u = (ps & 0x8888_8888) >> 3;
            if !stripe_causal {
                u |= (ns & 0x1111_1111) << 3;
            }

            let cs = read_unaligned(cur_sig as *const Ui32);
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
                let cwd_vec = frwd_fetch_ssse3::<0>(&mut sigprop);
                let mut cwd = _mm_extract_epi16(cwd_vec, 0) as Ui32;

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
                    cwd |= (_mm_extract_epi16(cwd_vec, 1) as Ui32) << (16 - cnt);

                    let mut new_sig_vec = _mm_set1_epi16(new_sig as i16);
                    new_sig_vec = _mm_shuffle_epi8(
                        new_sig_vec,
                        _mm_set_epi8(1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0),
                    );
                    new_sig_vec = _mm_and_si128(
                        new_sig_vec,
                        _mm_set1_epi64x(0x8040_2010_0804_0201u64 as i64),
                    );
                    new_sig_vec = _mm_cmpeq_epi8(
                        new_sig_vec,
                        _mm_set1_epi64x(0x8040_2010_0804_0201u64 as i64),
                    );

                    let mut inc_sum = new_sig_vec;
                    inc_sum = _mm_abs_epi8(inc_sum);
                    inc_sum = _mm_add_epi8(inc_sum, _mm_bslli_si128(inc_sum, 1));
                    inc_sum = _mm_add_epi8(inc_sum, _mm_bslli_si128(inc_sum, 2));
                    inc_sum = _mm_add_epi8(inc_sum, _mm_bslli_si128(inc_sum, 4));
                    inc_sum = _mm_add_epi8(inc_sum, _mm_bslli_si128(inc_sum, 8));
                    cnt += (_mm_extract_epi16(inc_sum, 7) as Ui32) >> 8;
                    let ex_sum = _mm_bslli_si128(inc_sum, 1);

                    let mut cwd_vec = _mm_set1_epi16(cwd as i16);
                    cwd_vec = _mm_shuffle_epi8(
                        cwd_vec,
                        _mm_set_epi8(1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0),
                    );
                    cwd_vec =
                        _mm_and_si128(cwd_vec, _mm_set1_epi64x(0x8040_2010_0804_0201u64 as i64));
                    cwd_vec =
                        _mm_cmpeq_epi8(cwd_vec, _mm_set1_epi64x(0x8040_2010_0804_0201u64 as i64));
                    cwd_vec = _mm_abs_epi8(cwd_vec);

                    let v = _mm_shuffle_epi8(cwd_vec, ex_sum);
                    let mut m =
                        _mm_set_epi8(-1, -1, -1, 12, -1, -1, -1, 8, -1, -1, -1, 4, -1, -1, -1, 0);
                    let val = _mm_set1_epi32((3u32 << (p - 2)) as i32);
                    let mut dp = dpp;
                    for _ in 0..4 {
                        let mut s0 = _mm_load_si128(dp as *const __m128i);
                        let mut s0_ns = _mm_shuffle_epi8(new_sig_vec, m);
                        s0_ns = _mm_cmpeq_epi32(s0_ns, _mm_set1_epi32(0xFF));

                        let mut s0_val = _mm_shuffle_epi8(v, m);
                        s0_val = _mm_slli_epi32(s0_val, 31);
                        s0_val = _mm_or_si128(s0_val, val);
                        s0_val = _mm_and_si128(s0_val, s0_ns);

                        s0 = _mm_or_si128(s0, s0_val);
                        _mm_store_si128(dp as *mut __m128i, s0);
                        dp = dp.add(stride as usize);
                        m = _mm_add_epi32(m, _mm_set1_epi32(1));
                    }
                }

                frwd_advance_ssse3(&mut sigprop, cnt);
            }

            new_sig |= cs;
            *prev_sig = new_sig as Ui16;
            let t = new_sig;
            new_sig |= (t & 0x7777) << 1;
            new_sig |= (t & 0xEEEE) >> 1;
            prev = new_sig | u;
            prev &= 0xF000;

            dpp = dpp.add(4);
            cur_sig = cur_sig.add(1);
            prev_sig = prev_sig.add(1);
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
unsafe fn decode_mrp_ssse3(
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
    rev_init_mrp(&mut magref, coded_data, lengths1 as i32, lengths2 as i32);

    for y in (0..height).step_by(4) {
        let mut cur_sig = sigma.as_ptr().add(((y >> 2) * mstr) as usize);
        let dpp = decoded_data.add((y * stride) as usize);
        for i in (0..width).step_by(4) {
            let cwd = rev_fetch_mrp(&mut magref);
            let sig = *cur_sig;
            cur_sig = cur_sig.add(1);
            let mut total_bits = 0;

            if sig != 0 {
                let mut sig_vec = _mm_set1_epi16(sig as i16);
                sig_vec = _mm_shuffle_epi8(
                    sig_vec,
                    _mm_set_epi8(1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0),
                );
                sig_vec = _mm_and_si128(sig_vec, _mm_set1_epi64x(0x8040_2010_0804_0201u64 as i64));
                sig_vec = _mm_cmpeq_epi8(sig_vec, _mm_set1_epi64x(0x8040_2010_0804_0201u64 as i64));
                sig_vec = _mm_abs_epi8(sig_vec);

                let mut inc_sum = sig_vec;
                inc_sum = _mm_add_epi8(inc_sum, _mm_bslli_si128(inc_sum, 1));
                inc_sum = _mm_add_epi8(inc_sum, _mm_bslli_si128(inc_sum, 2));
                inc_sum = _mm_add_epi8(inc_sum, _mm_bslli_si128(inc_sum, 4));
                inc_sum = _mm_add_epi8(inc_sum, _mm_bslli_si128(inc_sum, 8));
                total_bits = (_mm_extract_epi16(inc_sum, 7) >> 8) as Ui32;
                let ex_sum = _mm_bslli_si128(inc_sum, 1);

                let mut cwd_vec = _mm_set1_epi16(cwd as i16);
                cwd_vec = _mm_shuffle_epi8(
                    cwd_vec,
                    _mm_set_epi8(1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0),
                );
                cwd_vec = _mm_and_si128(cwd_vec, _mm_set1_epi64x(0x8040_2010_0804_0201u64 as i64));
                cwd_vec = _mm_cmpeq_epi8(cwd_vec, _mm_set1_epi64x(0x8040_2010_0804_0201u64 as i64));
                cwd_vec = _mm_add_epi8(cwd_vec, _mm_set1_epi8(1));
                cwd_vec = _mm_add_epi8(cwd_vec, cwd_vec);
                cwd_vec = _mm_or_si128(cwd_vec, _mm_set1_epi8(1));

                let mut m =
                    _mm_set_epi8(-1, -1, -1, 12, -1, -1, -1, 8, -1, -1, -1, 4, -1, -1, -1, 0);
                let mut dp = dpp.add(i as usize);
                for _ in 0..4 {
                    let mut s0 = _mm_load_si128(dp as *const __m128i);
                    let mut s0_sig = _mm_shuffle_epi8(sig_vec, m);
                    s0_sig = _mm_cmpeq_epi8(s0_sig, _mm_setzero_si128());
                    let s0_idx = _mm_shuffle_epi8(ex_sum, m);
                    let mut s0_val = _mm_shuffle_epi8(cwd_vec, s0_idx);
                    s0_val = _mm_andnot_si128(s0_sig, s0_val);
                    s0_val = _mm_sll_epi32(s0_val, _mm_cvtsi32_si128(p as i32 - 2));
                    s0 = _mm_xor_si128(s0, s0_val);
                    _mm_store_si128(dp as *mut __m128i, s0);
                    dp = dp.add(stride as usize);
                    m = _mm_add_epi32(m, _mm_set1_epi32(1));
                }
            }

            rev_advance_mrp(&mut magref, total_bits);
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "ssse3")]
pub(crate) unsafe fn ojph_decode_codeblock_ssse3(
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
        if !INSUFFICIENT_PRECISION_SSSE3.swap(true, Ordering::Relaxed) {
            ojph_warn!(
                0x00010003,
                "32 bits are not enough to decode this codeblock. This message will not be displayed again."
            );
        }
        return false;
    } else if missing_msbs == 30 {
        if !MODIFY_CODE_SSSE3.swap(true, Ordering::Relaxed) {
            ojph_warn!(
                0x00010004,
                "Not enough precision to decode the cleanup pass. The code can be modified to support this case. This message will not be displayed again."
            );
        }
        return false;
    } else if missing_msbs == 29 && num_passes > 1 {
        num_passes = 1;
        if !TRUNCATE_SPP_MRP_SSSE3.swap(true, Ordering::Relaxed) {
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

    decode_step1_vlc_mel_32(coded_data, lcup, scup, width, height, sstr, &mut scratch);

    if !decode_step2_magsgn_ssse3(
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

    let mstr = rearrange_sigma_ssse3(width, height, sstr, &mut scratch);

    if num_passes > 1 {
        decode_spp_ssse3(
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
            decode_mrp_ssse3(
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
