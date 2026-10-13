//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/coding/ojph_block_encoder_avx512.cpp`

use std::sync::LazyLock;

use crate::openjph::ojph_defs::{Ui32, Ui64, Ui8};
use crate::openjph::ojph_mem::{mem_elastic_allocator, CodedLists};

use super::ojph_block_encoder::{
    mel_emit_bit, mel_encode, mel_init, mel_struct, ms_encode64, ms_init, ms_struct, ms_terminate,
    uvlc_tbl, vlc_tbl0, vlc_tbl1,
};

#[cfg(target_arch = "x86")]
use core::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

#[allow(non_upper_case_globals)]
static VLC_TBL0_AVX512: LazyLock<[Ui32; 2048]> = LazyLock::new(|| {
    let mut out = [0u32; 2048];
    for (dst, src) in out.iter_mut().zip(vlc_tbl0.iter()) {
        *dst = *src as Ui32;
    }
    out
});

#[allow(non_upper_case_globals)]
static VLC_TBL1_AVX512: LazyLock<[Ui32; 2048]> = LazyLock::new(|| {
    let mut out = [0u32; 2048];
    for (dst, src) in out.iter_mut().zip(vlc_tbl1.iter()) {
        *dst = *src as Ui32;
    }
    out
});

#[allow(non_upper_case_globals)]
static ULVC_CWD_PRE: LazyLock<[Ui32; 33]> = LazyLock::new(|| {
    let mut out = [0u32; 33];
    for i in 0..33 {
        out[i] = uvlc_tbl[i].pre as Ui32;
    }
    out
});

#[allow(non_upper_case_globals)]
static ULVC_CWD_PRE_LEN: LazyLock<[i32; 33]> = LazyLock::new(|| {
    let mut out = [0i32; 33];
    for i in 0..33 {
        out[i] = uvlc_tbl[i].pre_len as i32;
    }
    out
});

#[allow(non_upper_case_globals)]
static ULVC_CWD_SUF: LazyLock<[Ui32; 33]> = LazyLock::new(|| {
    let mut out = [0u32; 33];
    for i in 0..33 {
        out[i] = uvlc_tbl[i].suf as Ui32;
    }
    out
});

#[allow(non_upper_case_globals)]
static ULVC_CWD_SUF_LEN: LazyLock<[i32; 33]> = LazyLock::new(|| {
    let mut out = [0i32; 33];
    for i in 0..33 {
        out[i] = uvlc_tbl[i].suf_len as i32;
    }
    out
});

pub(crate) fn initialize_block_encoder_tables_avx512() -> bool {
    let _ = &*VLC_TBL0_AVX512;
    let _ = &*VLC_TBL1_AVX512;
    let _ = &*ULVC_CWD_PRE;
    let _ = &*ULVC_CWD_PRE_LEN;
    let _ = &*ULVC_CWD_SUF;
    let _ = &*ULVC_CWD_SUF_LEN;
    true
}

#[allow(non_camel_case_types)]
struct vlc_struct_avx512 {
    buf: *mut Ui8,
    pos: Ui32,
    buf_size: Ui32,
    used_bits: i32,
    tmp: Ui64,
    last_greater_than_8f: bool,
}

impl Default for vlc_struct_avx512 {
    fn default() -> Self {
        Self {
            buf: core::ptr::null_mut(),
            pos: 0,
            buf_size: 0,
            used_bits: 0,
            tmp: 0,
            last_greater_than_8f: false,
        }
    }
}

#[inline]
unsafe fn vlc_init(vlcp: &mut vlc_struct_avx512, buffer_size: Ui32, data: *mut Ui8) {
    vlcp.buf = data.add(buffer_size as usize - 1);
    vlcp.pos = 1;
    vlcp.buf_size = buffer_size;
    *vlcp.buf = 0xFF;
    vlcp.used_bits = 4;
    vlcp.tmp = 0xF;
    vlcp.last_greater_than_8f = true;
}

#[inline]
unsafe fn vlc_encode(
    vlcp: &mut vlc_struct_avx512,
    cwd: Ui32,
    cwd_len: i32,
) -> crate::openjph::ojph_message::OjphResult<()> {
    vlcp.tmp |= (cwd as Ui64) << vlcp.used_bits;
    vlcp.used_bits += cwd_len;
    while vlcp.used_bits >= 8 {
        // Memory-safety guard for the SIMD VLC store path (see the matching
        // comment in `ojph_block_encoder_avx2.rs::vlc_encode`). The VLC bytes
        // grow downward from the end of the 2880-byte buffer; an out-of-budget
        // high-entropy code-block would otherwise drive `buf.sub(pos)` below
        // the buffer base → native 0xC0000005 access violation. The scalar
        // path guards every store (`ojph_block_encoder.rs:294`); we restore the
        // same guard here so the AVX512 path raises the identical recoverable
        // `ojph_error!`. One store + one `pos += 1` per iteration, so a single
        // top-of-body check is byte-output-neutral for in-budget blocks.
        if vlcp.pos >= vlcp.buf_size {
            return Err(crate::ojph_error!(
                0x00020002,
                "vlc encoder's buffer is full"
            ));
        }
        let tmp: Ui8;
        if vlcp.last_greater_than_8f {
            tmp = (vlcp.tmp & 0x7F) as Ui8;
            if tmp != 0x7F {
                let tmp = (vlcp.tmp & 0xFF) as Ui8;
                *vlcp.buf.sub(vlcp.pos as usize) = tmp;
                vlcp.last_greater_than_8f = tmp > 0x8F;
                vlcp.tmp >>= 8;
                vlcp.used_bits -= 8;
            } else {
                *vlcp.buf.sub(vlcp.pos as usize) = tmp;
                vlcp.last_greater_than_8f = false;
                vlcp.tmp >>= 7;
                vlcp.used_bits -= 7;
            }
        } else {
            tmp = (vlcp.tmp & 0xFF) as Ui8;
            *vlcp.buf.sub(vlcp.pos as usize) = tmp;
            vlcp.last_greater_than_8f = tmp > 0x8F;
            vlcp.tmp >>= 8;
            vlcp.used_bits -= 8;
        }
        vlcp.pos += 1;
    }
    Ok(())
}

#[inline]
unsafe fn terminate_mel_vlc_avx512(
    melp: &mut mel_struct,
    vlcp: &mut vlc_struct_avx512,
) -> crate::openjph::ojph_message::OjphResult<()> {
    if melp.run > 0 {
        mel_emit_bit(melp, 1)?;
    }
    if vlcp.last_greater_than_8f && (vlcp.tmp & 0x7F) == 0x7F {
        *vlcp.buf.sub(vlcp.pos as usize) = 0x7F;
        vlcp.pos += 1;
        vlcp.tmp >>= 7;
        vlcp.used_bits -= 7;
    }
    melp.tmp <<= melp.remaining_bits;
    let mel_mask = (0xFF << melp.remaining_bits) & 0xFF;
    let vlc_mask = 0xFF >> (8 - vlcp.used_bits);
    if (mel_mask | vlc_mask) == 0 {
        return Ok(());
    }
    if melp.pos >= melp.buf_size {
        return Err(crate::ojph_error!(
            0x00020003,
            "mel encoder's buffer is full"
        ));
    }
    let vlcp_tmp = vlcp.tmp as Ui8;
    let fuse = melp.tmp | (vlcp_tmp as i32);
    if ((((fuse ^ melp.tmp) & mel_mask) | ((fuse ^ (vlcp_tmp as i32)) & vlc_mask)) == 0)
        && fuse != 0xFF
        && vlcp.pos > 1
    {
        *melp.buf.add(melp.pos as usize) = fuse as Ui8;
        melp.pos += 1;
    } else {
        if vlcp.pos >= vlcp.buf_size {
            return Err(crate::ojph_error!(
                0x00020004,
                "vlc encoder's buffer is full"
            ));
        }
        *melp.buf.add(melp.pos as usize) = melp.tmp as Ui8;
        melp.pos += 1;
        *vlcp.buf.sub(vlcp.pos as usize) = vlcp_tmp;
        vlcp.pos += 1;
    }
    Ok(())
}

#[inline]
#[target_feature(enable = "avx512f")]
#[target_feature(enable = "avx512cd")]
unsafe fn proc_pixel(
    src_vec: *const __m512i,
    p: Ui32,
    eq_vec: *mut __m512i,
    s_vec: *mut __m512i,
    rho_vec: &mut __m512i,
    e_qmax_vec: &mut __m512i,
) {
    let zero = _mm512_setzero_si512();
    let one = _mm512_set1_epi32(1);
    let mut val_vec = [zero; 4];
    let mut eq_tmp = [zero; 4];
    let mut s_tmp = [zero; 4];
    let mut rho_tmp = [zero; 4];
    let mut val_mask = [0u16; 4];

    for i in 0..4usize {
        val_vec[i] = _mm512_add_epi32(*src_vec.add(i), *src_vec.add(i));
        val_vec[i] = _mm512_srl_epi32(val_vec[i], _mm_cvtsi32_si128(p as i32));
        val_vec[i] = _mm512_and_epi32(val_vec[i], _mm512_set1_epi32(!1u32 as i32));
        val_mask[i] = _mm512_cmpneq_epi32_mask(val_vec[i], zero);
        val_vec[i] = _mm512_mask_sub_epi32(zero, val_mask[i], val_vec[i], one);
        eq_tmp[i] = _mm512_mask_lzcnt_epi32(zero, val_mask[i], val_vec[i]);
        eq_tmp[i] = _mm512_mask_sub_epi32(zero, val_mask[i], _mm512_set1_epi32(32), eq_tmp[i]);
        val_vec[i] = _mm512_mask_sub_epi32(zero, val_mask[i], val_vec[i], one);
        s_tmp[i] = _mm512_mask_srli_epi32(zero, val_mask[i], *src_vec.add(i), 31);
        s_tmp[i] = _mm512_mask_add_epi32(zero, val_mask[i], s_tmp[i], val_vec[i]);
    }

    val_vec[0] = _mm512_mask_mov_epi32(zero, val_mask[0], one);
    val_vec[1] = _mm512_mask_mov_epi32(zero, val_mask[1], one);
    val_vec[2] = _mm512_mask_mov_epi32(zero, val_mask[2], one);
    val_vec[3] = _mm512_mask_mov_epi32(zero, val_mask[3], one);
    *e_qmax_vec = zero;
    let idx = [
        _mm512_set_epi32(14, 12, 10, 8, 6, 4, 2, 0, 14, 12, 10, 8, 6, 4, 2, 0),
        _mm512_set_epi32(15, 13, 11, 9, 7, 5, 3, 1, 15, 13, 11, 9, 7, 5, 3, 1),
    ];

    for i in 0..4usize {
        let e_idx = i >> 1;
        let o_idx = i & 1;
        *eq_vec.add(i) = _mm512_permutexvar_epi32(idx[e_idx], eq_tmp[o_idx]);
        *eq_vec.add(i) =
            _mm512_mask_permutexvar_epi32(*eq_vec.add(i), 0xFF00, idx[e_idx], eq_tmp[o_idx + 2]);
        *s_vec.add(i) = _mm512_permutexvar_epi32(idx[e_idx], s_tmp[o_idx]);
        *s_vec.add(i) =
            _mm512_mask_permutexvar_epi32(*s_vec.add(i), 0xFF00, idx[e_idx], s_tmp[o_idx + 2]);
        rho_tmp[i] = _mm512_permutexvar_epi32(idx[e_idx], val_vec[o_idx]);
        rho_tmp[i] =
            _mm512_mask_permutexvar_epi32(rho_tmp[i], 0xFF00, idx[e_idx], val_vec[o_idx + 2]);
        rho_tmp[i] = _mm512_sll_epi32(rho_tmp[i], _mm_cvtsi32_si128(i as i32));
        *e_qmax_vec = _mm512_max_epi32(*e_qmax_vec, *eq_vec.add(i));
    }

    *rho_vec = _mm512_or_epi32(rho_tmp[0], rho_tmp[1]);
    *rho_vec = _mm512_or_epi32(*rho_vec, rho_tmp[2]);
    *rho_vec = _mm512_or_epi32(*rho_vec, rho_tmp[3]);
}

#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn rotate_matrix(matrix: *mut __m512i) {
    let mut tmp = [_mm512_setzero_si512(); 4];
    tmp[0] = _mm512_unpacklo_epi32(*matrix.add(0), *matrix.add(1));
    tmp[1] = _mm512_unpackhi_epi32(*matrix.add(0), *matrix.add(1));
    tmp[2] = _mm512_unpacklo_epi32(*matrix.add(2), *matrix.add(3));
    tmp[3] = _mm512_unpackhi_epi32(*matrix.add(2), *matrix.add(3));
    *matrix.add(0) = _mm512_unpacklo_epi64(tmp[0], tmp[2]);
    *matrix.add(1) = _mm512_unpackhi_epi64(tmp[0], tmp[2]);
    *matrix.add(2) = _mm512_unpacklo_epi64(tmp[1], tmp[3]);
    *matrix.add(3) = _mm512_unpackhi_epi64(tmp[1], tmp[3]);
    tmp[0] = _mm512_shuffle_i32x4(*matrix.add(0), *matrix.add(1), 0x88);
    tmp[1] = _mm512_shuffle_i32x4(*matrix.add(2), *matrix.add(3), 0x88);
    tmp[2] = _mm512_shuffle_i32x4(*matrix.add(0), *matrix.add(1), 0xDD);
    tmp[3] = _mm512_shuffle_i32x4(*matrix.add(2), *matrix.add(3), 0xDD);
    *matrix.add(0) = _mm512_shuffle_i32x4(tmp[0], tmp[1], 0x88);
    *matrix.add(1) = _mm512_shuffle_i32x4(tmp[2], tmp[3], 0x88);
    *matrix.add(2) = _mm512_shuffle_i32x4(tmp[0], tmp[1], 0xDD);
    *matrix.add(3) = _mm512_shuffle_i32x4(tmp[2], tmp[3], 0xDD);
}

#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn proc_ms_encode(
    msp: &mut ms_struct,
    tuple_vec: __m512i,
    uq_vec: __m512i,
    rho_vec: __m512i,
    s_vec: *mut __m512i,
) -> crate::openjph::ojph_message::OjphResult<()> {
    let zero = _mm512_setzero_si512();
    let one = _mm512_set1_epi32(1);
    let mut m_vec = [zero; 4];
    let mut tmp = _mm512_sub_epi32(uq_vec, _mm512_and_epi32(tuple_vec, one));
    let mut tmp1 = _mm512_and_epi32(rho_vec, one);
    let mut mask = _mm512_cmpneq_epi32_mask(tmp1, zero);
    m_vec[0] = _mm512_mask_mov_epi32(zero, mask, tmp);
    tmp = _mm512_sub_epi32(
        uq_vec,
        _mm512_srli_epi32(_mm512_and_epi32(tuple_vec, _mm512_set1_epi32(2)), 1),
    );
    tmp1 = _mm512_and_epi32(rho_vec, _mm512_set1_epi32(2));
    mask = _mm512_cmpneq_epi32_mask(tmp1, zero);
    m_vec[1] = _mm512_mask_mov_epi32(zero, mask, tmp);
    tmp = _mm512_sub_epi32(
        uq_vec,
        _mm512_srli_epi32(_mm512_and_epi32(tuple_vec, _mm512_set1_epi32(4)), 2),
    );
    tmp1 = _mm512_and_epi32(rho_vec, _mm512_set1_epi32(4));
    mask = _mm512_cmpneq_epi32_mask(tmp1, zero);
    m_vec[2] = _mm512_mask_mov_epi32(zero, mask, tmp);
    tmp = _mm512_sub_epi32(
        uq_vec,
        _mm512_srli_epi32(_mm512_and_epi32(tuple_vec, _mm512_set1_epi32(8)), 3),
    );
    tmp1 = _mm512_and_epi32(rho_vec, _mm512_set1_epi32(8));
    mask = _mm512_cmpneq_epi32_mask(tmp1, zero);
    m_vec[3] = _mm512_mask_mov_epi32(zero, mask, tmp);
    rotate_matrix(m_vec.as_mut_ptr());
    rotate_matrix(s_vec);
    let mut cwd = [0u32; 16];
    let mut cwd_len = [0i32; 16];
    for i in 0..4usize {
        _mm512_storeu_si512(cwd_len.as_mut_ptr() as *mut _, m_vec[i]);
        tmp = _mm512_sub_epi32(_mm512_sllv_epi32(one, m_vec[i]), one);
        tmp = _mm512_and_epi32(tmp, *s_vec.add(i));
        _mm512_storeu_si512(cwd.as_mut_ptr() as *mut _, tmp);
        for j in 0..8usize {
            let idx = j * 2;
            let mut packed_cwd = cwd[idx] as Ui64;
            let mut packed_len = cwd_len[idx];
            packed_cwd |= (cwd[idx + 1] as Ui64) << packed_len;
            packed_len += cwd_len[idx + 1];
            ms_encode64(msp, packed_cwd, packed_len)?;
        }
    }
    Ok(())
}

#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn cal_eps_vec(eq_vec: *const __m512i, u_q_vec: __m512i, e_qmax_vec: __m512i) -> __m512i {
    let zero = _mm512_setzero_si512();
    let one = _mm512_set1_epi32(1);
    let u_q_mask = _mm512_cmpgt_epi32_mask(u_q_vec, zero);
    let mut mask = _mm512_cmpeq_epi32_mask(*eq_vec.add(0), e_qmax_vec);
    let mut tmp = _mm512_mask_mov_epi32(zero, mask, one);
    let mut eps_vec = _mm512_mask_mov_epi32(zero, u_q_mask, tmp);
    mask = _mm512_cmpeq_epi32_mask(*eq_vec.add(1), e_qmax_vec);
    tmp = _mm512_slli_epi32(_mm512_mask_mov_epi32(zero, mask, one), 1);
    eps_vec = _mm512_mask_or_epi32(zero, u_q_mask, eps_vec, tmp);
    mask = _mm512_cmpeq_epi32_mask(*eq_vec.add(2), e_qmax_vec);
    tmp = _mm512_slli_epi32(_mm512_mask_mov_epi32(zero, mask, one), 2);
    eps_vec = _mm512_mask_or_epi32(zero, u_q_mask, eps_vec, tmp);
    mask = _mm512_cmpeq_epi32_mask(*eq_vec.add(3), e_qmax_vec);
    tmp = _mm512_slli_epi32(_mm512_mask_mov_epi32(zero, mask, one), 3);
    _mm512_mask_or_epi32(zero, u_q_mask, eps_vec, tmp)
}

#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn update_lep(
    x: Ui32,
    prev_e_val_vec: &mut __m512i,
    eq_vec: *const __m512i,
    e_val_vec: *mut __m512i,
    left_shift: __m512i,
) {
    let zero = _mm512_setzero_si512();
    let tmp = _mm512_mask_permutexvar_epi32(*prev_e_val_vec, 0xFFFE, left_shift, *eq_vec.add(3));
    *prev_e_val_vec = _mm512_mask_permutexvar_epi32(zero, 0x1, left_shift, *eq_vec.add(3));
    *e_val_vec.add(x as usize) = _mm512_max_epi32(*eq_vec.add(1), tmp);
}

#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn update_lcxp(
    x: Ui32,
    prev_cx_val_vec: &mut __m512i,
    rho_vec: __m512i,
    cx_val_vec: *mut __m512i,
    left_shift: __m512i,
) {
    let zero = _mm512_setzero_si512();
    let mut tmp = _mm512_mask_permutexvar_epi32(*prev_cx_val_vec, 0xFFFE, left_shift, rho_vec);
    *prev_cx_val_vec = _mm512_mask_permutexvar_epi32(zero, 0x1, left_shift, rho_vec);
    tmp = _mm512_srli_epi32(_mm512_and_epi32(tmp, _mm512_set1_epi32(8)), 3);
    let tmp1 = _mm512_srli_epi32(_mm512_and_epi32(rho_vec, _mm512_set1_epi32(2)), 1);
    *cx_val_vec.add(x as usize) = _mm512_or_epi32(tmp, tmp1);
}

#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn cal_tuple(
    cq_vec: __m512i,
    rho_vec: __m512i,
    eps_vec: __m512i,
    vlc_tbl: *const Ui32,
) -> __m512i {
    let mut tmp = _mm512_slli_epi32(cq_vec, 8);
    tmp = _mm512_add_epi32(tmp, _mm512_slli_epi32(rho_vec, 4));
    tmp = _mm512_add_epi32(tmp, eps_vec);
    _mm512_i32gather_epi32(tmp, vlc_tbl as *const _, 4)
}

#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn proc_cq1(
    _x: Ui32,
    _cx_val_vec: *const __m512i,
    rho_vec: __m512i,
    _right_shift: __m512i,
) -> __m512i {
    _mm512_or_epi32(
        _mm512_srli_epi32(rho_vec, 1),
        _mm512_and_epi32(rho_vec, _mm512_set1_epi32(1)),
    )
}

#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn proc_cq2(
    x: Ui32,
    cx_val_vec: *const __m512i,
    rho_vec: __m512i,
    right_shift: __m512i,
) -> __m512i {
    let lcxp1_vec = _mm512_permutexvar_epi32(right_shift, *cx_val_vec.add(x as usize));
    let lcxp2_vec = _mm512_permutexvar_epi32(right_shift, *cx_val_vec.add(x as usize + 1));
    let mut tmp = _mm512_permutexvar_epi32(right_shift, lcxp1_vec);
    tmp = _mm512_mask_permutexvar_epi32(tmp, 0xC000, right_shift, lcxp2_vec);
    tmp = _mm512_slli_epi32(tmp, 2);
    let tmp1 = _mm512_mask_mov_epi32(lcxp1_vec, 0x8000, lcxp2_vec);
    tmp = _mm512_add_epi32(tmp1, tmp);
    let tmp1 = _mm512_srli_epi32(_mm512_and_epi32(rho_vec, _mm512_set1_epi32(4)), 1);
    tmp = _mm512_or_epi32(tmp, tmp1);
    let tmp1 = _mm512_srli_epi32(_mm512_and_epi32(rho_vec, _mm512_set1_epi32(8)), 2);
    _mm512_or_epi32(tmp, tmp1)
}

type FnProcCq = unsafe fn(Ui32, *const __m512i, __m512i, __m512i) -> __m512i;

#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn proc_mel_encode1(
    melp: &mut mel_struct,
    cq_vec: __m512i,
    rho_vec: __m512i,
    u_q_vec: __m512i,
    ignore: Ui32,
    right_shift: __m512i,
) -> crate::openjph::ojph_message::OjphResult<()> {
    let zero = _mm512_setzero_si512();
    let mel_need_encode = _mm512_cmpeq_epi32_mask(cq_vec, zero);
    let mel_bit = _mm512_cmpneq_epi32_mask(rho_vec, zero);
    let tmp = _mm512_permutexvar_epi32(right_shift, u_q_vec);
    let tmp1 = _mm512_min_epi32(u_q_vec, tmp);
    let mel_bit2 = _mm512_cmpgt_epi32_mask(tmp1, _mm512_set1_epi32(2));
    let mut mel_need_encode2 = _mm512_cmpgt_epi32_mask(u_q_vec, zero);
    mel_need_encode2 &= _mm512_cmpgt_epi32_mask(tmp, zero);
    let i_max = 16 - (ignore / 2);
    let mut i = 0;
    while i < i_max {
        let mask = 1u16 << i;
        if mel_need_encode & mask != 0 {
            mel_encode(melp, mel_bit & mask != 0)?;
        }
        if i + 1 < i_max {
            let mask = 1u16 << (i + 1);
            if mel_need_encode & mask != 0 {
                mel_encode(melp, mel_bit & mask != 0)?;
            }
        }
        if mel_need_encode2 & mask != 0 {
            mel_encode(melp, mel_bit2 & mask != 0)?;
        }
        i += 2;
    }
    Ok(())
}

#[inline]
#[target_feature(enable = "avx512f")]
unsafe fn proc_mel_encode2(
    melp: &mut mel_struct,
    cq_vec: __m512i,
    rho_vec: __m512i,
    _u_q_vec: __m512i,
    ignore: Ui32,
    _right_shift: __m512i,
) -> crate::openjph::ojph_message::OjphResult<()> {
    let zero = _mm512_setzero_si512();
    let mel_need_encode = _mm512_cmpeq_epi32_mask(cq_vec, zero);
    let mel_bit = _mm512_cmpneq_epi32_mask(rho_vec, zero);
    let i_max = 16 - (ignore / 2);
    for i in 0..i_max {
        let mask = 1u16 << i;
        if mel_need_encode & mask != 0 {
            mel_encode(melp, mel_bit & mask != 0)?;
        }
    }
    Ok(())
}

type FnProcMelEncode = unsafe fn(
    &mut mel_struct,
    __m512i,
    __m512i,
    __m512i,
    Ui32,
    __m512i,
) -> crate::openjph::ojph_message::OjphResult<()>;

#[inline]
unsafe fn proc_vlc_encode1(
    vlcp: &mut vlc_struct_avx512,
    tuple: *const Ui32,
    u_q: *const Ui32,
    ignore: Ui32,
) -> crate::openjph::ojph_message::OjphResult<()> {
    let i_max = 16 - (ignore / 2);
    let mut i = 0;
    while i < i_max {
        let mut val = *tuple.add(i as usize) >> 4;
        let mut size = (*tuple.add(i as usize) & 7) as i32;
        if i + 1 < i_max {
            val |= (*tuple.add((i + 1) as usize) >> 4) << size;
            size += (*tuple.add((i + 1) as usize) & 7) as i32;
        }
        let uq0 = *u_q.add(i as usize) as usize;
        let uq1 = *u_q.add((i + 1) as usize) as usize;
        if *u_q.add(i as usize) > 2 && *u_q.add((i + 1) as usize) > 2 {
            val |= ULVC_CWD_PRE[uq0 - 2] << size;
            size += ULVC_CWD_PRE_LEN[uq0 - 2];
            val |= ULVC_CWD_PRE[uq1 - 2] << size;
            size += ULVC_CWD_PRE_LEN[uq1 - 2];
            val |= ULVC_CWD_SUF[uq0 - 2] << size;
            size += ULVC_CWD_SUF_LEN[uq0 - 2];
            val |= ULVC_CWD_SUF[uq1 - 2] << size;
            size += ULVC_CWD_SUF_LEN[uq1 - 2];
        } else if *u_q.add(i as usize) > 2 && *u_q.add((i + 1) as usize) > 0 {
            val |= ULVC_CWD_PRE[uq0] << size;
            size += ULVC_CWD_PRE_LEN[uq0];
            val |= (*u_q.add((i + 1) as usize) - 1) << size;
            size += 1;
            val |= ULVC_CWD_SUF[uq0] << size;
            size += ULVC_CWD_SUF_LEN[uq0];
        } else {
            val |= ULVC_CWD_PRE[uq0] << size;
            size += ULVC_CWD_PRE_LEN[uq0];
            val |= ULVC_CWD_PRE[uq1] << size;
            size += ULVC_CWD_PRE_LEN[uq1];
            val |= ULVC_CWD_SUF[uq0] << size;
            size += ULVC_CWD_SUF_LEN[uq0];
            val |= ULVC_CWD_SUF[uq1] << size;
            size += ULVC_CWD_SUF_LEN[uq1];
        }
        vlc_encode(vlcp, val, size)?;
        i += 2;
    }
    Ok(())
}

#[inline]
unsafe fn proc_vlc_encode2(
    vlcp: &mut vlc_struct_avx512,
    tuple: *const Ui32,
    u_q: *const Ui32,
    ignore: Ui32,
) -> crate::openjph::ojph_message::OjphResult<()> {
    let i_max = 16 - (ignore / 2);
    let mut i = 0;
    while i < i_max {
        let mut val = *tuple.add(i as usize) >> 4;
        let mut size = (*tuple.add(i as usize) & 7) as i32;
        if i + 1 < i_max {
            val |= (*tuple.add((i + 1) as usize) >> 4) << size;
            size += (*tuple.add((i + 1) as usize) & 7) as i32;
        }
        let uq0 = *u_q.add(i as usize) as usize;
        let uq1 = *u_q.add((i + 1) as usize) as usize;
        val |= ULVC_CWD_PRE[uq0] << size;
        size += ULVC_CWD_PRE_LEN[uq0];
        val |= ULVC_CWD_PRE[uq1] << size;
        size += ULVC_CWD_PRE_LEN[uq1];
        val |= ULVC_CWD_SUF[uq0] << size;
        size += ULVC_CWD_SUF_LEN[uq0];
        val |= ULVC_CWD_SUF[uq1] << size;
        size += ULVC_CWD_SUF_LEN[uq1];
        vlc_encode(vlcp, val, size)?;
        i += 2;
    }
    Ok(())
}

type FnProcVlcEncode = unsafe fn(
    &mut vlc_struct_avx512,
    *const Ui32,
    *const Ui32,
    Ui32,
) -> crate::openjph::ojph_message::OjphResult<()>;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx512f")]
#[target_feature(enable = "avx512cd")]
pub(crate) unsafe fn ojph_encode_codeblock_avx512(
    buf: *mut Ui32,
    missing_msbs: Ui32,
    num_passes: Ui32,
    width: Ui32,
    height: Ui32,
    stride: Ui32,
    lengths: &mut [Ui32; 2],
    elastic: *mut mem_elastic_allocator,
    coded: &mut *mut CodedLists,
) -> crate::openjph::ojph_message::OjphResult<()> {
    let _ = initialize_block_encoder_tables_avx512();
    debug_assert!(num_passes == 1);

    let width_padded = (width + 31) & !31u32;
    let ignore = width_padded - width;
    let ms_size = (16384 * 16 + 14) / 15;
    let mel_vlc_size = 3072usize;
    let mel_size = 192usize;
    let vlc_size = mel_vlc_size - mel_size;
    let mut ms_buf = [0u8; (16384 * 16 + 14) / 15];
    let mut mel_vlc_buf = [0u8; 3072];
    let mel_buf = mel_vlc_buf.as_mut_ptr();
    let vlc_buf = mel_buf.add(mel_size);

    let mut mel = mel_struct::default();
    mel_init(&mut mel, mel_size as Ui32, mel_buf);
    let mut vlc = vlc_struct_avx512::default();
    vlc_init(&mut vlc, vlc_size as Ui32, vlc_buf);
    let mut ms = ms_struct::default();
    ms_init(&mut ms, ms_size as Ui32, ms_buf.as_mut_ptr());

    let p = 30 - missing_msbs;
    let zero = _mm512_setzero_si512();
    let one = _mm512_set1_epi32(1);
    let right_shift = _mm512_set_epi32(0, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1);
    let left_shift = _mm512_set_epi32(14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 15);
    let n_loop = (width_padded + 31) / 32;
    let mut e_val_vec = [zero; 33];
    let mut cx_val_vec = [zero; 33];
    let mut prev_e_val_vec = zero;
    let mut prev_cx_val_vec = zero;
    let mut prev_cq_vec = zero;
    let mut eq_vec = [zero; 4];
    let mut s_vec = [zero; 4];
    let mut src_vec = [zero; 4];
    let mut vlc_tbl = VLC_TBL0_AVX512.as_ptr();
    let mut proc_cq: FnProcCq = proc_cq1;
    let mut proc_mel_encode: FnProcMelEncode = proc_mel_encode1;
    let mut proc_vlc_encode: FnProcVlcEncode = proc_vlc_encode1;

    let mut y = 0;
    while y < height {
        e_val_vec[n_loop as usize] = prev_e_val_vec;
        let mut tmp = _mm512_and_epi32(prev_cx_val_vec, _mm512_set1_epi32(8));
        tmp = _mm512_srli_epi32(tmp, 3);
        cx_val_vec[n_loop as usize] = tmp;
        prev_e_val_vec = zero;
        prev_cx_val_vec = zero;
        let mut sp = buf.add((y * stride) as usize);

        for x in 0..n_loop as usize {
            let true_x = (x as i32) << 5;
            let mut mask32 = 0xFFFF_FFFFu32;
            let entries = true_x + 32 - width as i32;
            mask32 >>= if entries >= 0 { entries as u32 } else { 0 };
            let load_mask0 = _cvtu32_mask16(mask32);
            let load_mask1 = _cvtu32_mask16(mask32 >> 16);

            src_vec[0] = _mm512_maskz_loadu_epi32(load_mask0, sp as *const _);
            src_vec[2] = _mm512_maskz_loadu_epi32(load_mask1, sp.add(16) as *const _);
            if y + 1 < height {
                src_vec[1] =
                    _mm512_maskz_loadu_epi32(load_mask0, sp.add(stride as usize) as *const _);
                src_vec[3] = _mm512_maskz_loadu_epi32(
                    load_mask1,
                    sp.add((stride + 16) as usize) as *const _,
                );
            } else {
                src_vec[1] = zero;
                src_vec[3] = zero;
            }
            sp = sp.add(32);

            let mut rho_vec = zero;
            let mut e_qmax_vec = zero;
            proc_pixel(
                src_vec.as_ptr(),
                p,
                eq_vec.as_mut_ptr(),
                s_vec.as_mut_ptr(),
                &mut rho_vec,
                &mut e_qmax_vec,
            );

            tmp = _mm512_permutexvar_epi32(right_shift, e_val_vec[x]);
            tmp = _mm512_mask_permutexvar_epi32(tmp, 0x8000, right_shift, e_val_vec[x + 1]);
            let mut mask = _mm512_cmpgt_epi32_mask(e_val_vec[x], tmp);
            let max_e_vec = _mm512_sub_epi32(_mm512_mask_mov_epi32(tmp, mask, e_val_vec[x]), one);
            tmp = _mm512_max_epi32(max_e_vec, one);
            let tmp1 = _mm512_and_epi32(rho_vec, _mm512_sub_epi32(rho_vec, one));
            mask = _mm512_cmpneq_epi32_mask(tmp1, zero);
            let kappa_vec = _mm512_mask_mov_epi32(one, mask, tmp);

            tmp = proc_cq(x as Ui32, cx_val_vec.as_ptr(), rho_vec, right_shift);
            let cq_vec = _mm512_mask_permutexvar_epi32(prev_cq_vec, 0xFFFE, left_shift, tmp);
            prev_cq_vec = _mm512_mask_permutexvar_epi32(zero, 0x1, left_shift, tmp);

            update_lep(
                x as Ui32,
                &mut prev_e_val_vec,
                eq_vec.as_ptr(),
                e_val_vec.as_mut_ptr(),
                left_shift,
            );
            update_lcxp(
                x as Ui32,
                &mut prev_cx_val_vec,
                rho_vec,
                cx_val_vec.as_mut_ptr(),
                left_shift,
            );

            let uq_vec = _mm512_max_epi32(kappa_vec, e_qmax_vec);
            let u_q_vec = _mm512_sub_epi32(uq_vec, kappa_vec);
            let eps_vec = cal_eps_vec(eq_vec.as_ptr(), u_q_vec, e_qmax_vec);
            let mut tuple_vec = cal_tuple(cq_vec, rho_vec, eps_vec, vlc_tbl);
            let local_ignore = if x == n_loop as usize - 1 { ignore } else { 0 };

            proc_mel_encode(
                &mut mel,
                cq_vec,
                rho_vec,
                u_q_vec,
                local_ignore,
                right_shift,
            )?;
            proc_ms_encode(&mut ms, tuple_vec, uq_vec, rho_vec, s_vec.as_mut_ptr())?;

            let mut u_q = [0u32; 16];
            let mut tuple = [0u32; 16];
            tuple_vec = _mm512_srli_epi32(tuple_vec, 4);
            _mm512_storeu_si512(tuple.as_mut_ptr() as *mut _, tuple_vec);
            _mm512_storeu_si512(u_q.as_mut_ptr() as *mut _, u_q_vec);
            proc_vlc_encode(&mut vlc, tuple.as_ptr(), u_q.as_ptr(), local_ignore)?;
        }

        tmp = _mm512_permutexvar_epi32(right_shift, cx_val_vec[0]);
        tmp = _mm512_slli_epi32(tmp, 2);
        prev_cq_vec = _mm512_maskz_add_epi32(0x1, tmp, cx_val_vec[0]);

        proc_cq = proc_cq2;
        vlc_tbl = VLC_TBL1_AVX512.as_ptr();
        proc_mel_encode = proc_mel_encode2;
        proc_vlc_encode = proc_vlc_encode2;
        y += 2;
    }

    ms_terminate(&mut ms)?;
    terminate_mel_vlc_avx512(&mut mel, &mut vlc)?;
    let total = mel.pos + vlc.pos + ms.pos;
    lengths[0] = total;
    (*elastic).get_buffer(total, coded);
    core::ptr::copy_nonoverlapping(ms.buf, (**coded).buf, ms.pos as usize);
    core::ptr::copy_nonoverlapping(
        mel.buf,
        (**coded).buf.add(ms.pos as usize),
        mel.pos as usize,
    );
    core::ptr::copy_nonoverlapping(
        vlc.buf.sub(vlc.pos as usize - 1),
        (**coded).buf.add((ms.pos + mel.pos) as usize),
        vlc.pos as usize,
    );
    let num_bytes = mel.pos + vlc.pos;
    *((**coded).buf.add(total as usize - 1)) = (num_bytes >> 4) as Ui8;
    let last2 = (**coded).buf.add(total as usize - 2);
    *last2 &= 0xF0;
    *last2 |= (num_bytes & 0xF) as Ui8;
    (**coded).avail_size -= total;
    Ok(())
}
