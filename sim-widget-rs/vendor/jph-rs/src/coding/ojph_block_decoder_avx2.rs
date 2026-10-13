//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/coding/ojph_block_decoder_avx2.cpp`
//!
//! **Round 10 (BUG1.md): partial 0.31.0 port.** The 0.31.0 rewrite of this file
//! (`ojph_block_decoder_avx2.cpp`, 1854 changed lines ignoring whitespace) is two genuinely
//! separate pieces:
//! - **`uvlc_tbl1_wide` fast path (ported here)**: `decode_step1_vlc_mel_32`'s non-initial-row
//!   loop now tries one `uvlc_tbl1_wide` lookup (`ojph_block_common.rs`) that decodes BOTH
//!   quads' U-values at once, falling back to the pre-existing `uvlc_tbl1`-based per-quad decode
//!   (unchanged, kept verbatim below) only when the combined codeword length would exceed what
//!   the wide table's 10 index bits can address (`total_bits == 0x1F`). Small, self-contained,
//!   low-risk: the fallback path is the exact code this file already had and already passed
//!   every existing decode-parity gate, so this addition can only ever be exercised as a strict
//!   superset (same output, more table-driven, fewer scalar decode steps on the common path).
//! - **`destuff_frwd`/`dfetch`/`dfetch64` (NOT ported this round)**: a new bitstream
//!   architecture — flattening the MagSgn/SPP bitstreams into a destuffed byte buffer upfront,
//!   then fetching bits at absolute positions (random access, no serial fetch/advance state) —
//!   used across the MagSgn decode, SPP decode, and (per the diff) at least part of the MRP
//!   decode. This is a genuinely new algorithm, not a mechanical translation of the existing
//!   `frwd_struct`-based serial reader ported here, and needs its own dedicated, carefully
//!   fuzz-verified pass rather than being folded into the same round as the smaller, provably
//!   safe `uvlc_tbl1_wide` addition above. See BUG1.md Round 10 step 2 for the scoping writeup.

use core::ptr::{read_unaligned, write_unaligned};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ojph_warn;
use crate::openjph::ojph_defs::{Ui16, Ui32, Ui64, Ui8};

use super::ojph_block_common::{uvlc_tbl0, uvlc_tbl1, uvlc_tbl1_wide, vlc_tbl0, vlc_tbl1};
use super::ojph_block_decoder::{dec_mel_st, mel_get_run, mel_init};

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

    // 0.31.0: de-stuff the VLC segment up front instead of reading it serially through
    // `rev_struct`/`rev_fetch`/`rev_advance`. Its size is at most `scup - 1 < 4095` bytes (`scup`
    // is a 12-bit value), and consumption per quad pair is at most 7 + 7 + 30 bits, so 4096
    // bytes always suffice (matches the C++ original's comment/sizing exactly).
    const VLC_CAP: usize = 4096;
    let mut vlc_buf = [0u8; VLC_CAP + 72];
    let vlc_limit = unsafe {
        destuff_vlc(
            coded_data as *const Ui8,
            lcup,
            scup,
            vlc_buf.as_mut_ptr(),
            VLC_CAP as Ui32,
        )
    };
    let mut vlc_off: Ui32 = 0;
    let mut vlc_bits: Ui32 = 0;
    let mut vlc_val: Ui64 = 0;

    let mut run = unsafe { mel_get_run(&mut mel) };
    let mut c_q: Ui32 = 0;
    let mut sp = 0usize;

    let mut x = 0u32;
    while x < width {
        unsafe {
            drefill(
                &mut vlc_val,
                &mut vlc_bits,
                &mut vlc_off,
                vlc_buf.as_ptr(),
                vlc_limit,
            )
        };
        let mut t0 = vlc_tbl0[(c_q + (vlc_val & 0x7F) as Ui32) as usize];

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
        dconsume(&mut vlc_val, &mut vlc_bits, t0 as Ui32 & 0x7);

        let mut t1 = vlc_tbl0[(c_q + (vlc_val & 0x7F) as Ui32) as usize];
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
        dconsume(&mut vlc_val, &mut vlc_bits, t1 as Ui32 & 0x7);

        let mut uvlc_mode = ((t0 as Ui32 & 0x8) << 3) | ((t1 as Ui32 & 0x8) << 4);
        if uvlc_mode == 0xC0 {
            run -= 2;
            uvlc_mode += if run == -1 { 0x40 } else { 0 };
            if run < 0 {
                run = unsafe { mel_get_run(&mut mel) };
            }
        }

        let mut uvlc_entry = uvlc_tbl0[(uvlc_mode + (vlc_val & 0x3F) as Ui32) as usize] as Ui32;
        dconsume(&mut vlc_val, &mut vlc_bits, uvlc_entry & 0x7);
        uvlc_entry >>= 3;
        let mut len = uvlc_entry & 0xF;
        let tmp = (vlc_val as Ui32) & ((1_u32 << len) - 1);
        dconsume(&mut vlc_val, &mut vlc_bits, len);
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

            unsafe {
                drefill(
                    &mut vlc_val,
                    &mut vlc_bits,
                    &mut vlc_off,
                    vlc_buf.as_ptr(),
                    vlc_limit,
                )
            };
            let mut t0 = vlc_tbl1[(c_q + (vlc_val & 0x7F) as Ui32) as usize];

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

            dconsume(&mut vlc_val, &mut vlc_bits, t0 as Ui32 & 0x7);

            let mut t1 = vlc_tbl1[(c_q + (vlc_val & 0x7F) as Ui32) as usize];
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

            dconsume(&mut vlc_val, &mut vlc_bits, t1 as Ui32 & 0x7);

            // 0.31.0 fast path: one `uvlc_tbl1_wide` lookup decodes both quads' U-values when
            // the combined codeword fits the table's 10 index bits (`total_bits != 0x1F`,
            // covering all but pathologically long codewords). `uvlc_tbl1_wide` is indexed by
            // the SAME `vlc_val`/`uvlc_mode` bits the pre-existing narrow-table path below reads
            // (`(t0 & 0x8)`/`(t1 & 0x8)` are the u_off flags, just packed into a different bit
            // layout: `(mode << 10) | (vlc_val & 0x3FF)` vs the narrow table's
            // `uvlc_mode + (vlc_val & 0x3F)`), so falling through to the unchanged narrow-table
            // code when the fast path can't cover it is exactly the pre-rewrite decode — no new
            // state, no new invariant, and the fast path is provably a strict subset of what the
            // fallback already computes (proven by `coding::codeblock_fuzz_tests` decoder
            // agreement, extended to force both paths).
            let wide_mode = ((t0 as Ui32 >> 3) & 1) | (((t1 as Ui32 >> 3) & 1) << 1);
            let wide_entry =
                uvlc_tbl1_wide[((wide_mode << 10) | (vlc_val & 0x3FF) as Ui32) as usize];
            let total_bits = wide_entry & 0x1F;
            if total_bits < 0x1F {
                scratch[sp + 1] = ((wide_entry >> 5) & 0xFF) as Ui16;
                scratch[sp + 3] = ((wide_entry >> 13) & 0xFF) as Ui16;
                dconsume(&mut vlc_val, &mut vlc_bits, total_bits);
            } else {
                let uvlc_mode = ((t0 as Ui32 & 0x8) << 3) | ((t1 as Ui32 & 0x8) << 4);
                let mut uvlc_entry =
                    uvlc_tbl1[(uvlc_mode + (vlc_val & 0x3F) as Ui32) as usize] as Ui32;
                dconsume(&mut vlc_val, &mut vlc_bits, uvlc_entry & 0x7);
                uvlc_entry >>= 3;
                let mut len = uvlc_entry & 0xF;
                let tmp = (vlc_val as Ui32) & ((1_u32 << len) - 1);
                dconsume(&mut vlc_val, &mut vlc_bits, len);
                uvlc_entry >>= 4;
                len = uvlc_entry & 0x7;
                uvlc_entry >>= 3;
                let mut u_q = ((uvlc_entry & 7) + (tmp & !(0xFF_u32 << len))) as Ui16;
                scratch[sp + 1] = u_q;
                u_q = ((uvlc_entry >> 3) + (tmp >> len)) as Ui16;
                scratch[sp + 3] = u_q;
            }

            sp += 4;
        }
        scratch[sp] = 0;
        scratch[sp + 1] = 0;
    }
}

#[cfg(target_arch = "x86")]
use core::arch::x86::{
    __m128i, __m256i, _mm256_add_epi16, _mm256_add_epi32, _mm256_and_si256, _mm256_andnot_si256,
    _mm256_bslli_epi128, _mm256_bsrli_epi128, _mm256_castps_si256, _mm256_castsi128_si256,
    _mm256_castsi256_si128, _mm256_cmpeq_epi16, _mm256_cmpeq_epi32, _mm256_cmpgt_epi32,
    _mm256_cvtepi32_ps, _mm256_cvtepu16_epi32, _mm256_extract_epi16, _mm256_extracti128_si256,
    _mm256_inserti128_si256, _mm256_loadu_si256, _mm256_max_epi32, _mm256_min_epi16,
    _mm256_movemask_epi8, _mm256_mullo_epi16, _mm256_or_si256, _mm256_permutevar8x32_epi32,
    _mm256_set1_epi16, _mm256_set1_epi32, _mm256_set1_epi8, _mm256_set_epi16, _mm256_set_epi32,
    _mm256_set_epi64x, _mm256_set_epi8, _mm256_setr_epi32, _mm256_setzero_si256,
    _mm256_shuffle_epi32, _mm256_shuffle_epi8, _mm256_sll_epi16, _mm256_sll_epi32,
    _mm256_slli_epi16, _mm256_slli_epi32, _mm256_sllv_epi32, _mm256_srli_epi16, _mm256_srli_epi32,
    _mm256_storeu_si256, _mm256_sub_epi16, _mm256_sub_epi32, _mm256_subs_epu16, _mm256_testz_si256,
    _mm_abs_epi8, _mm_add_epi32, _mm_add_epi8, _mm_and_si128, _mm_andnot_si128, _mm_bslli_si128,
    _mm_cmpeq_epi32, _mm_cmpeq_epi8, _mm_cmpgt_epi32, _mm_cvtsi32_si128, _mm_extract_epi16,
    _mm_extract_epi32, _mm_load_si128, _mm_loadl_epi64, _mm_loadu_si128, _mm_movemask_epi8,
    _mm_or_si128, _mm_set1_epi16, _mm_set1_epi32, _mm_set1_epi64x, _mm_set1_epi8, _mm_set_epi16,
    _mm_set_epi32, _mm_set_epi8, _mm_setzero_si128, _mm_shuffle_epi8, _mm_sll_epi16, _mm_sll_epi32,
    _mm_sll_epi64, _mm_slli_epi32, _mm_srl_epi64, _mm_srli_epi32, _mm_store_si128,
    _mm_storeu_si128, _mm_subs_epu8, _mm_testz_si128, _mm_xor_si128,
};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{
    __m128i, __m256i, _mm256_add_epi16, _mm256_add_epi32, _mm256_and_si256, _mm256_andnot_si256,
    _mm256_bslli_epi128, _mm256_bsrli_epi128, _mm256_castps_si256, _mm256_castsi128_si256,
    _mm256_castsi256_si128, _mm256_cmpeq_epi16, _mm256_cmpeq_epi32, _mm256_cmpgt_epi32,
    _mm256_cvtepi32_ps, _mm256_cvtepu16_epi32, _mm256_extract_epi16, _mm256_extracti128_si256,
    _mm256_inserti128_si256, _mm256_loadu_si256, _mm256_max_epi32, _mm256_min_epi16,
    _mm256_movemask_epi8, _mm256_mullo_epi16, _mm256_or_si256, _mm256_permutevar8x32_epi32,
    _mm256_set1_epi16, _mm256_set1_epi32, _mm256_set1_epi8, _mm256_set_epi16, _mm256_set_epi32,
    _mm256_set_epi64x, _mm256_set_epi8, _mm256_setr_epi32, _mm256_setzero_si256,
    _mm256_shuffle_epi32, _mm256_shuffle_epi8, _mm256_sll_epi16, _mm256_sll_epi32,
    _mm256_slli_epi16, _mm256_slli_epi32, _mm256_sllv_epi32, _mm256_srli_epi16, _mm256_srli_epi32,
    _mm256_storeu_si256, _mm256_sub_epi16, _mm256_sub_epi32, _mm256_subs_epu16, _mm256_testz_si256,
    _mm_abs_epi8, _mm_add_epi32, _mm_add_epi8, _mm_and_si128, _mm_andnot_si128, _mm_bslli_si128,
    _mm_cmpeq_epi32, _mm_cmpeq_epi8, _mm_cmpgt_epi32, _mm_cvtsi128_si64, _mm_cvtsi32_si128,
    _mm_extract_epi16, _mm_extract_epi64, _mm_load_si128, _mm_loadl_epi64, _mm_loadu_si128,
    _mm_movemask_epi8, _mm_or_si128, _mm_set1_epi16, _mm_set1_epi32, _mm_set1_epi64x,
    _mm_set1_epi8, _mm_set_epi16, _mm_set_epi32, _mm_set_epi8, _mm_setzero_si128, _mm_shuffle_epi8,
    _mm_sll_epi16, _mm_sll_epi32, _mm_sll_epi64, _mm_slli_epi32, _mm_srl_epi64, _mm_srli_epi32,
    _mm_store_si128, _mm_storeu_si128, _mm_subs_epu8, _mm_testz_si128, _mm_xor_si128,
};

static INSUFFICIENT_PRECISION_AVX2: AtomicBool = AtomicBool::new(false);
static MODIFY_CODE_AVX2: AtomicBool = AtomicBool::new(false);
static TRUNCATE_SPP_MRP_AVX2: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// 0.31.0 destuffed-bitstream reader: replaces the old `frwd_struct_avx2`
// (serial fetch/advance) and the AVX2 decoder's use of the generic scalar
// `rev_struct` for VLC and MRP. Ported from `ojph_block_decoder_avx2.cpp`'s
// `destuff_frwd`/`dfetch`/`dfetch64`/`drefill`/`dconsume`/`destuff_rev`/
// `destuff_vlc`/`destuff_mrp` (BUG1.md Round 10 step 2). Instead of a
// stateful reader consumed serially, the whole MagSgn/VLC/SPP/MRP segment is
// flattened up front into a destuffed byte buffer (still respecting the
// codec's byte-stuffing rule — a byte following 0xFF/backward-0x8F+0x7F
// contributes only 7 bits), and bits for any quad are then fetched at an
// absolute bit position with no carried state, so independent fetches can
// execute out of order / off the critical path.
// ---------------------------------------------------------------------------

/// De-stuffs a forward-growing bitstream (MagSgn: `X = 0xFF`; SPP: `X = 0`) into a flat buffer.
/// `X` is the fill value used once the source is exhausted. Writes at most `cap + 1` destuffed
/// bytes followed by 64 bytes of `X` padding; `dst` must have room for `cap + 65` bytes (all
/// callers use a fixed stack buffer sized from the code-block's known worst-case bit
/// consumption, matching the C++ original's stack-array sizing — no separate bounds check is
/// needed beyond the `o < o_end` / `o + 24 <= o_end` loop conditions already ported below, since
/// `cap` itself is derived from the maximum possible consumption for a 64x64 code-block).
/// Returns the clamp offset for [`dfetch`]/[`dfetch64`]: bytes at or beyond it hold no stream
/// bits (they read as `X`).
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn destuff_frwd<const X: i32>(src: *const Ui8, size: i32, dst: *mut Ui8, cap: Ui32) -> Ui32 {
    let size = if size < 0 { 0 } else { size };
    let mut o = dst;
    let o_end = dst.add(cap as usize);
    let mut s = src;
    let s_end = src.add(size as usize);
    let mut acc: Ui64 = 0; // partial output byte, low nb bits are valid
    let mut nb: Ui32 = 0; // number of valid bits in acc; always < 8
    let mut prev_ff = false;

    // fast path; 16 source bytes at a time when they contain no 0xFF
    while s.add(16) <= s_end && o.add(24) <= o_end {
        let v = _mm_loadu_si128(s as *const __m128i);
        let ff = _mm_movemask_epi8(_mm_cmpeq_epi8(v, _mm_set1_epi8(-1)));
        if ff != 0 || prev_ff {
            // process these 16 bytes one at a time
            for _ in 0..16 {
                let b = *s;
                s = s.add(1);
                acc |= (b as Ui64) << nb;
                nb += if prev_ff { 7 } else { 8 };
                prev_ff = b == 0xFF;
                if nb >= 8 {
                    *o = acc as Ui8;
                    o = o.add(1);
                    acc >>= 8;
                    nb -= 8;
                }
            }
            continue;
        }
        let v0: Ui64 = read_unaligned(s as *const Ui64);
        let v1: Ui64 = read_unaligned(s.add(8) as *const Ui64);
        let w0 = acc | (v0 << nb);
        let w1 = (v1 << nb) | (if nb != 0 { v0 >> (64 - nb) } else { 0 });
        write_unaligned(o as *mut Ui64, w0);
        write_unaligned(o.add(8) as *mut Ui64, w1);
        acc = if nb != 0 { v1 >> (64 - nb) } else { 0 };
        o = o.add(16);
        s = s.add(16);
    }
    // tail; one byte at a time
    while s < s_end && o < o_end {
        let b = *s;
        s = s.add(1);
        acc |= (b as Ui64) << nb;
        nb += if prev_ff { 7 } else { 8 };
        prev_ff = b == 0xFF;
        if nb >= 8 {
            *o = acc as Ui8;
            o = o.add(1);
            acc >>= 8;
            nb -= 8;
        }
    }
    // fill the bits above nb with X, and pad with X bytes
    let fill: Ui32 = if X == 0xFF { 0xFFu32 << nb } else { 0 };
    *o = ((acc as Ui32) | fill) as Ui8;
    let pad = _mm256_set1_epi8(X as i8);
    _mm256_storeu_si256(o.add(1) as *mut __m256i, pad);
    _mm256_storeu_si256(o.add(33) as *mut __m256i, pad);
    (o.offset_from(dst) as Ui32) + 1
}

/// Fetches 128 bits from a destuffed MagSgn buffer starting at absolute bit position `pos`.
/// The byte offset is clamped to `limit` so positions past the end of the stream read as 1s
/// (matching the `X = 0xFF` padding `destuff_frwd` wrote) without leaving the buffer.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn dfetch(dbuf: *const Ui8, limit: Ui32, pos: Ui32) -> __m128i {
    let mut off = pos >> 3;
    off = if off < limit { off } else { limit };
    let p = dbuf.add(off as usize);
    let v = _mm_loadu_si128(p as *const __m128i);
    let w = _mm_loadu_si128(p.add(8) as *const __m128i);
    let k = (pos & 7) as i32;
    let r = _mm_srl_epi64(v, _mm_cvtsi32_si128(k));
    let c = _mm_sll_epi64(w, _mm_cvtsi32_si128(64 - k));
    _mm_or_si128(r, c)
}

/// Scalar counterpart of [`dfetch`]: returns at least 57 bits starting at bit position `pos` of
/// a buffer produced by `destuff_frwd`/`destuff_rev`, in the low bits of the result (bits above
/// `64 - (pos & 7)` are garbage).
#[inline]
unsafe fn dfetch64(dbuf: *const Ui8, limit: Ui32, pos: Ui32) -> Ui64 {
    let mut off = pos >> 3;
    off = if off < limit { off } else { limit };
    let v: Ui64 = read_unaligned(dbuf.add(off as usize) as *const Ui64);
    v >> (pos & 7)
}

/// Branchless refill of a register-resident bit window (VLC decode, [`decode_step1_vlc_mel_32`]):
/// loads 8 bytes from a destuffed buffer and inserts whole bytes above the bits remaining in the
/// window, leaving 56-63 valid bits. Must only be called when `*bits < 8` (its low bits are
/// combined into the refill via bitwise OR, which would corrupt a wider existing window). The
/// read offset is clamped to `limit` so over-consumption on a truncated code-block reads from
/// the zero/X padding instead of uninitialized memory.
#[inline]
unsafe fn drefill(val: &mut Ui64, bits: &mut Ui32, off: &mut Ui32, dbuf: *const Ui8, limit: Ui32) {
    let o = if *off < limit { *off } else { limit };
    let v: Ui64 = read_unaligned(dbuf.add(o as usize) as *const Ui64);
    *val |= v << *bits;
    *off += (63 - *bits) >> 3;
    *bits |= 56;
}

/// Consumes bits from a window refilled by [`drefill`].
#[inline]
fn dconsume(val: &mut Ui64, bits: &mut Ui32, num_bits: Ui32) {
    *val >>= num_bits;
    *bits -= num_bits;
}

/// De-stuffs a backward-growing bitstream (VLC and MRP) into a flat buffer. Reading backward
/// from `p`, a byte contributes only 7 bits (MSB OR'ed with the following bit, guaranteed 0 by
/// the encoder) when the byte after it in memory is `> 0x8F` and its own low 7 bits are all 1s.
/// Bits beyond the end of the data read as 0s. The first byte uses the caller-provided `unstuff`
/// state (from `destuff_vlc`'s or `destuff_mrp`'s own initialization); afterwards the state
/// always equals `p[1] > 0x8F`, so the fast path detects stuffing with a pairwise byte compare.
/// Writes at most `cap + 1` destuffed bytes followed by 64 bytes of zero padding; same fixed
/// stack-buffer sizing convention as [`destuff_frwd`]. Returns the clamp offset for
/// [`dfetch`]/[`dfetch64`].
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn destuff_rev(
    mut p: *const Ui8,
    mut size: i32,
    mut unstuff: bool,
    mut acc: Ui64,
    mut nb: Ui32,
    dst: *mut Ui8,
    cap: Ui32,
) -> Ui32 {
    let mut o = dst;
    let o_end = dst.add(cap as usize);

    // process the first byte with the caller-provided unstuff state; afterwards the state is
    // implied by the byte at p[1], which the fast path checks vectorially (staying in-segment)
    if size > 0 && o < o_end {
        let d = *p as Ui32;
        p = p.sub(1);
        size -= 1;
        acc |= (d as Ui64) << nb;
        nb += 8 - u32::from(unstuff && (d & 0x7F) == 0x7F);
        unstuff = d > 0x8F;
        if nb >= 8 {
            *o = acc as Ui8;
            o = o.add(1);
            acc >>= 8;
            nb -= 8;
        }
    }

    // fast path; 16 source bytes at a time when none needs unstuffing
    while size >= 16 && o.add(24) <= o_end {
        let v = _mm_loadu_si128(p.sub(15) as *const __m128i);
        let nx = _mm_loadu_si128(p.sub(14) as *const __m128i);
        let is7f = _mm_cmpeq_epi8(_mm_and_si128(v, _mm_set1_epi8(0x7F)), _mm_set1_epi8(0x7F));
        // le8f is 0xFF where the byte after (in memory) is <= 0x8F
        let le8f = _mm_cmpeq_epi8(
            _mm_subs_epu8(nx, _mm_set1_epi8(0x8Fu8 as i8)),
            _mm_setzero_si128(),
        );
        let stuff = _mm_andnot_si128(le8f, is7f);
        if _mm_testz_si128(stuff, stuff) == 0 {
            // process these 16 bytes one at a time
            for _ in 0..16 {
                let d = *p as Ui32;
                p = p.sub(1);
                acc |= (d as Ui64) << nb;
                nb += 8 - u32::from(unstuff && (d & 0x7F) == 0x7F);
                unstuff = d > 0x8F;
                if nb >= 8 {
                    *o = acc as Ui8;
                    o = o.add(1);
                    acc >>= 8;
                    nb -= 8;
                }
            }
            size -= 16;
            continue;
        }
        let r = _mm_shuffle_epi8(
            v,
            _mm_set_epi8(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15), // reverse bytes
        );
        #[cfg(target_arch = "x86_64")]
        let (v0, v1): (Ui64, Ui64) = (
            _mm_cvtsi128_si64(r) as Ui64,
            _mm_extract_epi64(r, 1) as Ui64,
        );
        #[cfg(target_arch = "x86")]
        let (v0, v1): (Ui64, Ui64) = (
            (_mm_cvtsi128_si32(r) as Ui32 as Ui64)
                | ((_mm_extract_epi32(r, 1) as Ui32 as Ui64) << 32),
            (_mm_extract_epi32(r, 2) as Ui32 as Ui64)
                | ((_mm_extract_epi32(r, 3) as Ui32 as Ui64) << 32),
        );
        let w0 = acc | (v0 << nb);
        let w1 = (v1 << nb) | (if nb != 0 { v0 >> (64 - nb) } else { 0 });
        write_unaligned(o as *mut Ui64, w0);
        write_unaligned(o.add(8) as *mut Ui64, w1);
        acc = if nb != 0 { v1 >> (64 - nb) } else { 0 };
        o = o.add(16);
        p = p.sub(16);
        size -= 16;
        unstuff = *p.add(1) > 0x8F;
    }
    // tail; one byte at a time
    while size > 0 && o < o_end {
        let d = *p as Ui32;
        p = p.sub(1);
        size -= 1;
        acc |= (d as Ui64) << nb;
        nb += 8 - u32::from(unstuff && (d & 0x7F) == 0x7F);
        unstuff = d > 0x8F;
        if nb >= 8 {
            *o = acc as Ui8;
            o = o.add(1);
            acc >>= 8;
            nb -= 8;
        }
    }
    // the bits above nb are already 0 = fill; pad with zero bytes
    *o = acc as Ui8;
    let z = _mm256_setzero_si256();
    _mm256_storeu_si256(o.add(1) as *mut __m256i, z);
    _mm256_storeu_si256(o.add(33) as *mut __m256i, z);
    (o.offset_from(dst) as Ui32) + 1
}

/// De-stuffs the VLC segment into a flat buffer. Performs the same initialization the old
/// `rev_init` did — the first byte contributes only its upper 4 bits (3 if its lower 3 bits are
/// all 1s) — then de-stuffs the remaining `scup - 2` bytes backward via [`destuff_rev`].
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn destuff_vlc(data: *const Ui8, lcup: i32, scup: i32, dst: *mut Ui8, cap: Ui32) -> Ui32 {
    let p = data.add((lcup - 2) as usize);
    let d = *p as Ui32;
    let acc = (d >> 4) as Ui64;
    let nb: Ui32 = 4 - u32::from((acc & 7) == 7);
    let unstuff = (d | 0xF) > 0x8F;
    destuff_rev(p.sub(1), scup - 2, unstuff, acc, nb, dst, cap)
}

/// De-stuffs the MRP segment into a flat buffer. Performs the same initialization the old
/// `rev_init_mrp` did — reading starts at the last byte of the SPP+MRP segments with the
/// unstuff state set — then de-stuffs backward via [`destuff_rev`].
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn destuff_mrp(data: *const Ui8, lcup: i32, len2: i32, dst: *mut Ui8, cap: Ui32) -> Ui32 {
    destuff_rev(
        data.add((lcup + len2 - 1) as usize),
        len2,
        true,
        0,
        0,
        dst,
        cap,
    )
}

// https://stackoverflow.com/a/58827596
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn avx2_lzcnt_epi32(mut v: __m256i) -> __m256i {
    v = _mm256_andnot_si256(_mm256_srli_epi32(v, 8), v);
    v = _mm256_castps_si256(_mm256_cvtepi32_ps(v));
    v = _mm256_srli_epi32(v, 23);
    v = _mm256_subs_epu16(_mm256_set1_epi32(158), v);
    _mm256_min_epi16(v, _mm256_set1_epi32(32))
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn decode_two_quad32_avx2(
    inf_u_q: __m256i,
    u_q: __m256i,
    dbuf: *const Ui8,
    limit: Ui32,
    pos: &mut Ui32,
    p: Ui32,
    vn: &mut __m128i,
) -> __m256i {
    let mut row = _mm256_setzero_si256();
    let mut flags = _mm256_and_si256(
        inf_u_q,
        _mm256_set_epi32(
            0x8880, 0x4440, 0x2220, 0x1110, 0x8880, 0x4440, 0x2220, 0x1110,
        ),
    );
    let insig = _mm256_cmpeq_epi32(flags, _mm256_setzero_si256());

    if _mm256_movemask_epi8(insig) as u32 != 0xFFFF_FFFF {
        flags = _mm256_mullo_epi16(
            flags,
            _mm256_set_epi16(1, 1, 2, 2, 4, 4, 8, 8, 1, 1, 2, 2, 4, 4, 8, 8),
        );

        let mut w0 = _mm256_srli_epi32(flags, 15);
        let mut m_n = _mm256_sub_epi32(u_q, w0);
        m_n = _mm256_andnot_si256(insig, m_n);

        let mut inc_sum = m_n;
        inc_sum = _mm256_add_epi32(inc_sum, _mm256_bslli_epi128(inc_sum, 4));
        inc_sum = _mm256_add_epi32(inc_sum, _mm256_bslli_epi128(inc_sum, 8));
        let total_mn1 = _mm256_extract_epi16(inc_sum, 6) as Ui32;
        let total_mn2 = _mm256_extract_epi16(inc_sum, 14) as Ui32;

        let ms_vec0 = dfetch(dbuf, limit, *pos);
        let ms_vec1 = dfetch(dbuf, limit, *pos + total_mn1);
        *pos += total_mn1 + total_mn2;

        let mut ms_vec = _mm256_inserti128_si256(_mm256_castsi128_si256(ms_vec0), ms_vec1, 0x1);
        let ex_sum = _mm256_bslli_epi128(inc_sum, 4);

        let mut byte_idx = _mm256_srli_epi32(ex_sum, 3);
        let mut bit_idx = _mm256_and_si256(ex_sum, _mm256_set1_epi32(7));
        byte_idx = _mm256_shuffle_epi8(
            byte_idx,
            _mm256_set_epi32(
                0x0C0C0C0C, 0x08080808, 0x04040404, 0x00000000, 0x0C0C0C0C, 0x08080808, 0x04040404,
                0x00000000,
            ),
        );
        byte_idx = _mm256_add_epi32(byte_idx, _mm256_set1_epi32(0x03020100));
        let mut d0 = _mm256_shuffle_epi8(ms_vec, byte_idx);
        byte_idx = _mm256_add_epi32(byte_idx, _mm256_set1_epi32(0x01010101));
        let mut d1 = _mm256_shuffle_epi8(ms_vec, byte_idx);

        bit_idx = _mm256_or_si256(bit_idx, _mm256_slli_epi32(bit_idx, 16));

        let a = _mm_set_epi8(1, 3, 7, 15, 31, 63, 127, -1, 1, 3, 7, 15, 31, 63, 127, -1);
        let aa = _mm256_inserti128_si256(_mm256_castsi128_si256(a), a, 0x1);
        let mut bit_shift = _mm256_shuffle_epi8(aa, bit_idx);
        bit_shift = _mm256_add_epi16(bit_shift, _mm256_set1_epi16(0x0101));
        d0 = _mm256_mullo_epi16(d0, bit_shift);
        d0 = _mm256_srli_epi16(d0, 8);
        d1 = _mm256_mullo_epi16(d1, bit_shift);
        d1 = _mm256_and_si256(d1, _mm256_set1_epi32(0xFF00FF00_u32 as i32));
        d0 = _mm256_or_si256(d0, d1);

        let ones = _mm256_set1_epi32(1);
        let twos = _mm256_set1_epi32(2);
        let mut u_q_m1 = _mm256_sub_epi32(u_q, ones);
        u_q_m1 = _mm256_and_si256(u_q_m1, _mm256_set_epi32(0, 0, 0, 0x1F, 0, 0, 0, 0x1F));
        u_q_m1 = _mm256_shuffle_epi32(u_q_m1, 0);
        w0 = _mm256_sub_epi32(twos, w0);
        let shift = _mm256_sllv_epi32(w0, u_q_m1);
        ms_vec = _mm256_and_si256(d0, _mm256_sub_epi32(shift, ones));

        w0 = _mm256_and_si256(flags, _mm256_set1_epi32(0x800));
        w0 = _mm256_cmpeq_epi32(w0, _mm256_setzero_si256());
        w0 = _mm256_andnot_si256(w0, shift);
        ms_vec = _mm256_or_si256(ms_vec, w0);
        w0 = _mm256_slli_epi32(ms_vec, 31);
        ms_vec = _mm256_or_si256(ms_vec, ones);
        let mut tvn = ms_vec;
        ms_vec = _mm256_add_epi32(ms_vec, twos);
        ms_vec = _mm256_sll_epi32(ms_vec, _mm_cvtsi32_si128(p as i32 - 1));
        ms_vec = _mm256_or_si256(ms_vec, w0);
        row = _mm256_andnot_si256(insig, ms_vec);

        ms_vec = _mm256_andnot_si256(insig, tvn);
        tvn = _mm256_shuffle_epi8(
            ms_vec,
            _mm256_set_epi32(
                -1, 0x0F0E0D0C, 0x07060504, -1, -1, -1, 0x0F0E0D0C, 0x07060504,
            ),
        );

        *vn = _mm_or_si128(*vn, _mm256_castsi256_si128(tvn));
        *vn = _mm_or_si128(*vn, _mm256_extracti128_si256(tvn, 0x1));
    }

    row
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn decode_four_quad16_avx2(
    inf_u_q: __m128i,
    u_q: __m128i,
    dbuf: *const Ui8,
    limit: Ui32,
    pos: &mut Ui32,
    p: Ui32,
    vn: &mut __m128i,
) -> __m256i {
    let mut row = _mm256_setzero_si256();
    let ddd = _mm_shuffle_epi8(
        inf_u_q,
        _mm_set_epi16(
            0x0d0c, 0x0d0c, 0x0908, 0x0908, 0x0504, 0x0504, 0x0100, 0x0100,
        ),
    );
    let mut w0 = _mm256_permutevar8x32_epi32(
        _mm256_castsi128_si256(ddd),
        _mm256_setr_epi32(0, 0, 1, 1, 2, 2, 3, 3),
    );
    let mut flags = _mm256_and_si256(
        w0,
        _mm256_set_epi16(
            0x8880_u16 as i16,
            0x4440,
            0x2220,
            0x1110,
            0x8880_u16 as i16,
            0x4440,
            0x2220,
            0x1110,
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
    let insig = _mm256_cmpeq_epi16(flags, _mm256_setzero_si256());
    if _mm256_movemask_epi8(insig) as u32 != 0xFFFF_FFFF {
        let ddd = _mm_or_si128(_mm_bslli_si128(u_q, 2), u_q);
        let u_q_avx = _mm256_permutevar8x32_epi32(
            _mm256_castsi128_si256(ddd),
            _mm256_setr_epi32(0, 0, 1, 1, 2, 2, 3, 3),
        );
        flags = _mm256_mullo_epi16(
            flags,
            _mm256_set_epi16(1, 2, 4, 8, 1, 2, 4, 8, 1, 2, 4, 8, 1, 2, 4, 8),
        );

        w0 = _mm256_srli_epi16(flags, 15);
        let mut m_n = _mm256_sub_epi16(u_q_avx, w0);
        m_n = _mm256_andnot_si256(insig, m_n);

        let mut inc_sum = m_n;
        inc_sum = _mm256_add_epi16(inc_sum, _mm256_bslli_epi128(inc_sum, 2));
        inc_sum = _mm256_add_epi16(inc_sum, _mm256_bslli_epi128(inc_sum, 4));
        inc_sum = _mm256_add_epi16(inc_sum, _mm256_bslli_epi128(inc_sum, 8));
        let total_mn1 = _mm256_extract_epi16(inc_sum, 7) as Ui32;
        let total_mn2 = _mm256_extract_epi16(inc_sum, 15) as Ui32;
        let ex_sum = _mm256_bslli_epi128(inc_sum, 2);

        let ms_vec0 = dfetch(dbuf, limit, *pos);
        let ms_vec1 = dfetch(dbuf, limit, *pos + total_mn1);
        *pos += total_mn1 + total_mn2;
        let mut ms_vec = _mm256_inserti128_si256(_mm256_castsi128_si256(ms_vec0), ms_vec1, 0x1);

        let mut byte_idx = _mm256_srli_epi16(ex_sum, 3);
        let bit_idx = _mm256_and_si256(ex_sum, _mm256_set1_epi16(7));
        byte_idx = _mm256_shuffle_epi8(
            byte_idx,
            _mm256_set_epi16(
                0x0E0E, 0x0C0C, 0x0A0A, 0x0808, 0x0606, 0x0404, 0x0202, 0x0000, 0x0E0E, 0x0C0C,
                0x0A0A, 0x0808, 0x0606, 0x0404, 0x0202, 0x0000,
            ),
        );
        byte_idx = _mm256_add_epi16(byte_idx, _mm256_set1_epi16(0x0100));
        let mut d0 = _mm256_shuffle_epi8(ms_vec, byte_idx);
        byte_idx = _mm256_add_epi16(byte_idx, _mm256_set1_epi16(0x0101));
        let mut d1 = _mm256_shuffle_epi8(ms_vec, byte_idx);

        let mut bit_shift = _mm256_shuffle_epi8(
            _mm256_set_epi8(
                1, 3, 7, 15, 31, 63, 127, -1, 1, 3, 7, 15, 31, 63, 127, -1, 1, 3, 7, 15, 31, 63,
                127, -1, 1, 3, 7, 15, 31, 63, 127, -1,
            ),
            bit_idx,
        );
        bit_shift = _mm256_add_epi16(bit_shift, _mm256_set1_epi16(0x0101));
        d0 = _mm256_mullo_epi16(d0, bit_shift);
        d0 = _mm256_srli_epi16(d0, 8);
        d1 = _mm256_mullo_epi16(d1, bit_shift);
        d1 = _mm256_and_si256(d1, _mm256_set1_epi16(0xFF00_u16 as i16));
        d0 = _mm256_or_si256(d0, d1);

        let ones = _mm256_set1_epi16(1);
        let twos = _mm256_set1_epi16(2);
        let u_q_m1 = _mm256_sub_epi32(u_q_avx, ones);
        let uq0 = _mm256_and_si256(u_q_m1, _mm256_set_epi32(0, 0, 0, 0x1F, 0, 0, 0, 0x1F));
        let uq1 = _mm256_bsrli_epi128(u_q_m1, 14);
        w0 = _mm256_sub_epi16(twos, w0);
        let mut t0 = _mm256_and_si256(w0, _mm256_set_epi64x(0, -1, 0, -1));
        let mut t1 = _mm256_and_si256(w0, _mm256_set_epi64x(-1, 0, -1, 0));
        let mut t_0_sse = _mm256_castsi256_si128(t0);
        t_0_sse = _mm_sll_epi16(t_0_sse, _mm256_castsi256_si128(uq0));
        let mut t_1_sse = _mm256_extracti128_si256(t0, 0x1);
        t_1_sse = _mm_sll_epi16(t_1_sse, _mm256_extracti128_si256(uq0, 0x1));
        t0 = _mm256_inserti128_si256(_mm256_castsi128_si256(t_0_sse), t_1_sse, 0x1);

        t_0_sse = _mm256_castsi256_si128(t1);
        t_0_sse = _mm_sll_epi16(t_0_sse, _mm256_castsi256_si128(uq1));
        t_1_sse = _mm256_extracti128_si256(t1, 0x1);
        t_1_sse = _mm_sll_epi16(t_1_sse, _mm256_extracti128_si256(uq1, 0x1));
        t1 = _mm256_inserti128_si256(_mm256_castsi128_si256(t_0_sse), t_1_sse, 0x1);

        let shift = _mm256_or_si256(t0, t1);
        ms_vec = _mm256_and_si256(d0, _mm256_sub_epi16(shift, ones));

        w0 = _mm256_and_si256(flags, _mm256_set1_epi16(0x800));
        w0 = _mm256_cmpeq_epi16(w0, _mm256_setzero_si256());
        w0 = _mm256_andnot_si256(w0, shift);
        ms_vec = _mm256_or_si256(ms_vec, w0);
        w0 = _mm256_slli_epi16(ms_vec, 15);
        ms_vec = _mm256_or_si256(ms_vec, ones);
        let tvn = ms_vec;
        ms_vec = _mm256_add_epi16(ms_vec, twos);
        ms_vec = _mm256_sll_epi16(ms_vec, _mm_cvtsi32_si128(p as i32 - 1));
        ms_vec = _mm256_or_si256(ms_vec, w0);
        row = _mm256_andnot_si256(insig, ms_vec);

        ms_vec = _mm256_andnot_si256(insig, tvn);
        let ms_vec_shuffle1 = _mm256_shuffle_epi8(
            ms_vec,
            _mm256_set_epi16(
                -1, -1, -1, -1, 0x0706, 0x0302, -1, -1, -1, -1, -1, -1, -1, -1, 0x0706, 0x0302,
            ),
        );
        let ms_vec_shuffle2 = _mm256_shuffle_epi8(
            ms_vec,
            _mm256_set_epi16(
                -1, -1, -1, 0x0F0E, 0x0B0A, -1, -1, -1, -1, -1, -1, -1, -1, 0x0F0E, 0x0B0A, -1,
            ),
        );
        ms_vec = _mm256_or_si256(ms_vec_shuffle1, ms_vec_shuffle2);

        *vn = _mm_or_si128(*vn, _mm256_castsi256_si128(ms_vec));
        *vn = _mm_or_si128(*vn, _mm256_extracti128_si256(ms_vec, 0x1));
    }

    row
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn decode_step2_magsgn_avx2(
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
        const V_N_SIZE: usize = 512 + 16;
        let mut v_n_scratch = [0u32; 2 * V_N_SIZE];
        // maximum consumable MagSgn bits: 4096 samples x (mmsbp2 <= 32) bits
        const DBUF_CAP: usize = 4096 * 32 / 8;
        let mut dbuf = [0u8; DBUF_CAP + 72];
        let limit = destuff_frwd::<0xFF>(
            coded_data as *const Ui8,
            lcup - scup,
            dbuf.as_mut_ptr(),
            DBUF_CAP as Ui32,
        );
        let mut pos: Ui32 = 0;
        let avx_mmsbp2 = _mm256_set1_epi32(mmsbp2 as i32);

        {
            let mut sp = scratch.as_ptr();
            let mut vp = v_n_scratch.as_mut_ptr();
            let mut dp = decoded_data;
            *vp = 2;

            let mut x = 0;
            while x < width {
                let mut vn = _mm_set1_epi32(2);
                let mut inf_u_q = _mm256_castsi128_si256(_mm_loadl_epi64(sp as *const __m128i));
                inf_u_q =
                    _mm256_permutevar8x32_epi32(inf_u_q, _mm256_setr_epi32(0, 0, 0, 0, 1, 1, 1, 1));
                let u_q = _mm256_srli_epi32(inf_u_q, 16);
                let w = _mm256_cmpgt_epi32(u_q, avx_mmsbp2);
                if _mm256_testz_si256(w, w) == 0 {
                    return false;
                }

                let mut row = decode_two_quad32_avx2(
                    inf_u_q,
                    u_q,
                    dbuf.as_ptr(),
                    limit,
                    &mut pos,
                    p,
                    &mut vn,
                );
                row = _mm256_permutevar8x32_epi32(row, _mm256_setr_epi32(0, 2, 4, 6, 1, 3, 5, 7));
                _mm_storeu_si128(dp as *mut __m128i, _mm256_castsi256_si128(row));
                _mm_storeu_si128(
                    dp.add(stride as usize) as *mut __m128i,
                    _mm256_extracti128_si256(row, 0x1),
                );

                let mut w0 = _mm_cvtsi32_si128(*(vp as *const i32));
                w0 = _mm_or_si128(w0, vn);
                _mm_storeu_si128(vp as *mut __m128i, w0);

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
                let mut sp = scratch.as_ptr().add(((y >> 1) * sstr) as usize);
                let avx_31 = _mm256_set1_epi32(31);
                let avx_f0 = _mm256_set1_epi32(0xF0);
                let avx_1 = _mm256_set1_epi32(1);
                let avx_0 = _mm256_setzero_si256();
                let mut x = 0;
                while x <= width {
                    let mut v = _mm256_loadu_si256(vp as *const __m256i);
                    let v_p1 = _mm256_loadu_si256(vp.add(1) as *const __m256i);
                    v = _mm256_or_si256(v, v_p1);
                    v = avx2_lzcnt_epi32(v);
                    v = _mm256_sub_epi32(avx_31, v);

                    let mut inf_u_q = _mm256_loadu_si256(sp as *const __m256i);
                    let mut gamma = _mm256_and_si256(inf_u_q, avx_f0);
                    let mut w0 = _mm256_sub_epi32(gamma, avx_1);
                    gamma = _mm256_and_si256(gamma, w0);
                    gamma = _mm256_cmpeq_epi32(gamma, avx_0);

                    v = _mm256_andnot_si256(gamma, v);
                    v = _mm256_max_epi32(v, avx_1);

                    inf_u_q = _mm256_srli_epi32(inf_u_q, 16);
                    v = _mm256_add_epi32(inf_u_q, v);

                    w0 = _mm256_cmpgt_epi32(v, avx_mmsbp2);
                    if _mm256_testz_si256(w0, w0) == 0 {
                        return false;
                    }

                    _mm256_storeu_si256(vp.add(V_N_SIZE) as *mut __m256i, v);

                    x += 16;
                    vp = vp.add(8);
                    sp = sp.add(16);
                }
            }

            let mut vp = v_n_scratch.as_mut_ptr();
            let mut sp = scratch.as_ptr().add(((y >> 1) * sstr) as usize);
            let mut dp = decoded_data.add((y * stride) as usize);
            *vp = 2;

            let mut x = 0;
            while x < width {
                let mut vn = _mm_set1_epi32(2);
                let mut inf_u_q = _mm256_castsi128_si256(_mm_loadl_epi64(sp as *const __m128i));
                inf_u_q =
                    _mm256_permutevar8x32_epi32(inf_u_q, _mm256_setr_epi32(0, 0, 0, 0, 1, 1, 1, 1));
                let mut u_q =
                    _mm256_castsi128_si256(_mm_loadl_epi64(vp.add(V_N_SIZE) as *const __m128i));
                u_q = _mm256_permutevar8x32_epi32(u_q, _mm256_setr_epi32(0, 0, 0, 0, 1, 1, 1, 1));

                let mut row = decode_two_quad32_avx2(
                    inf_u_q,
                    u_q,
                    dbuf.as_ptr(),
                    limit,
                    &mut pos,
                    p,
                    &mut vn,
                );
                row = _mm256_permutevar8x32_epi32(row, _mm256_setr_epi32(0, 2, 4, 6, 1, 3, 5, 7));
                _mm_storeu_si128(dp as *mut __m128i, _mm256_castsi256_si128(row));
                _mm_storeu_si128(
                    dp.add(stride as usize) as *mut __m128i,
                    _mm256_extracti128_si256(row, 0x1),
                );

                let mut w0 = _mm_cvtsi32_si128(*(vp as *const i32));
                w0 = _mm_or_si128(w0, vn);
                _mm_storeu_si128(vp as *mut __m128i, w0);

                x += 4;
                sp = sp.add(4);
                vp = vp.add(2);
                dp = dp.add(4);
            }

            y += 2;
        }
    } else {
        p -= 16;

        const V_N_SIZE: usize = 512 + 16;
        let mut v_n_scratch = [0u16; V_N_SIZE];
        let mut v_n_scratch_32 = [0u32; V_N_SIZE];
        // maximum consumable MagSgn bits: 4096 samples x (mmsbp2 < 16) bits
        const DBUF_CAP16: usize = 4096 * 15 / 8;
        let mut dbuf = [0u8; DBUF_CAP16 + 72];
        let limit = destuff_frwd::<0xFF>(
            coded_data as *const Ui8,
            lcup - scup,
            dbuf.as_mut_ptr(),
            DBUF_CAP16 as Ui32,
        );
        let mut pos: Ui32 = 0;

        {
            let mut sp = scratch.as_ptr();
            let mut vp = v_n_scratch.as_mut_ptr();
            let mut dp = decoded_data;
            *vp = 2;

            let mut x = 0;
            while x < width {
                let inf_u_q = _mm_loadu_si128(sp as *const __m128i);
                let u_q = _mm_srli_epi32(inf_u_q, 16);
                let w = _mm_cmpgt_epi32(u_q, _mm_set1_epi32(mmsbp2 as i32));
                if _mm_testz_si128(w, w) == 0 {
                    return false;
                }

                let mut vn = _mm_set1_epi16(2);
                let row = decode_four_quad16_avx2(
                    inf_u_q,
                    u_q,
                    dbuf.as_ptr(),
                    limit,
                    &mut pos,
                    p,
                    &mut vn,
                );

                let w = _mm_cvtsi32_si128(*vp as i32);
                _mm_storeu_si128(vp as *mut __m128i, _mm_or_si128(w, vn));

                let w0 = _mm256_shuffle_epi8(
                    row,
                    _mm256_set_epi16(
                        0x0D0C, -1, 0x0908, -1, 0x0504, -1, 0x0100, -1, 0x0D0C, -1, 0x0908, -1,
                        0x0504, -1, 0x0100, -1,
                    ),
                );
                let w1 = _mm256_shuffle_epi8(
                    row,
                    _mm256_set_epi16(
                        0x0F0E, -1, 0x0B0A, -1, 0x0706, -1, 0x0302, -1, 0x0F0E, -1, 0x0B0A, -1,
                        0x0706, -1, 0x0302, -1,
                    ),
                );

                _mm256_storeu_si256(dp as *mut __m256i, w0);
                _mm256_storeu_si256(dp.add(stride as usize) as *mut __m256i, w1);

                x += 8;
                sp = sp.add(8);
                vp = vp.add(4);
                dp = dp.add(8);
            }
        }

        let mut y = 2;
        while y < height {
            {
                let mut vp = v_n_scratch.as_ptr();
                let mut vp_32 = v_n_scratch_32.as_mut_ptr();
                let mut sp = scratch.as_ptr().add(((y >> 1) * sstr) as usize);
                let avx_mmsbp2 = _mm256_set1_epi32(mmsbp2 as i32);
                let avx_31 = _mm256_set1_epi32(31);
                let avx_f0 = _mm256_set1_epi32(0xF0);
                let avx_1 = _mm256_set1_epi32(1);
                let avx_0 = _mm256_setzero_si256();

                let mut x = 0;
                while x <= width {
                    let mut v = _mm_loadu_si128(vp as *const __m128i);
                    let v_p1 = _mm_loadu_si128(vp.add(1) as *const __m128i);
                    v = _mm_or_si128(v, v_p1);

                    let mut v_avx = _mm256_cvtepu16_epi32(v);
                    v_avx = avx2_lzcnt_epi32(v_avx);
                    v_avx = _mm256_sub_epi32(avx_31, v_avx);

                    let mut inf_u_q = _mm256_loadu_si256(sp as *const __m256i);
                    let mut gamma = _mm256_and_si256(inf_u_q, avx_f0);
                    let mut w0 = _mm256_sub_epi32(gamma, avx_1);
                    gamma = _mm256_and_si256(gamma, w0);
                    gamma = _mm256_cmpeq_epi32(gamma, avx_0);

                    v_avx = _mm256_andnot_si256(gamma, v_avx);
                    v_avx = _mm256_max_epi32(v_avx, avx_1);

                    inf_u_q = _mm256_srli_epi32(inf_u_q, 16);
                    v_avx = _mm256_add_epi32(inf_u_q, v_avx);

                    w0 = _mm256_cmpgt_epi32(v_avx, avx_mmsbp2);
                    if _mm256_testz_si256(w0, w0) == 0 {
                        return false;
                    }

                    _mm256_storeu_si256(vp_32 as *mut __m256i, v_avx);

                    x += 16;
                    vp = vp.add(8);
                    sp = sp.add(16);
                    vp_32 = vp_32.add(8);
                }
            }

            let mut vp = v_n_scratch.as_mut_ptr();
            let mut vp_32 = v_n_scratch_32.as_ptr();
            let mut sp = scratch.as_ptr().add(((y >> 1) * sstr) as usize);
            let mut dp = decoded_data.add((y * stride) as usize);
            *vp = 2;

            let mut x = 0;
            while x < width {
                let inf_u_q = _mm_loadu_si128(sp as *const __m128i);
                let u_q = _mm_loadu_si128(vp_32 as *const __m128i);
                let mut vn = _mm_set1_epi16(2);
                let row = decode_four_quad16_avx2(
                    inf_u_q,
                    u_q,
                    dbuf.as_ptr(),
                    limit,
                    &mut pos,
                    p,
                    &mut vn,
                );

                let w = _mm_cvtsi32_si128(*vp as i32);
                _mm_storeu_si128(vp as *mut __m128i, _mm_or_si128(w, vn));

                let w0 = _mm256_shuffle_epi8(
                    row,
                    _mm256_set_epi16(
                        0x0D0C, -1, 0x0908, -1, 0x0504, -1, 0x0100, -1, 0x0D0C, -1, 0x0908, -1,
                        0x0504, -1, 0x0100, -1,
                    ),
                );
                let w1 = _mm256_shuffle_epi8(
                    row,
                    _mm256_set_epi16(
                        0x0F0E, -1, 0x0B0A, -1, 0x0706, -1, 0x0302, -1, 0x0F0E, -1, 0x0B0A, -1,
                        0x0706, -1, 0x0302, -1,
                    ),
                );

                _mm256_storeu_si256(dp as *mut __m256i, w0);
                _mm256_storeu_si256(dp.add(stride as usize) as *mut __m256i, w1);

                x += 8;
                sp = sp.add(8);
                vp = vp.add(4);
                dp = dp.add(8);
                vp_32 = vp_32.add(4);
            }

            y += 2;
        }
    }

    true
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
unsafe fn rearrange_sigma_avx2(
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
#[target_feature(enable = "avx2")]
unsafe fn decode_spp_avx2(
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
    // maximum consumable SPP bits: 4096 samples x 2 bits (one significance bit and one sign
    // bit per sample)
    const SPP_CAP: usize = 4096 * 2 / 8;
    let mut spp_buf = [0u8; SPP_CAP + 72];
    let spp_limit = destuff_frwd::<0>(
        coded_data.add(lengths1 as usize) as *const Ui8,
        lengths2 as i32,
        spp_buf.as_mut_ptr(),
        SPP_CAP as Ui32,
    );
    let mut spp_pos: Ui32 = 0;

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
                let mut cwd = dfetch64(spp_buf.as_ptr(), spp_limit, spp_pos) as Ui32;

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
                    // 0.31.0: `dfetch64` already returned every bit this quad-group can ever
                    // need (>= 57, vs the old two-lane 32-bit `frwd_fetch_avx2` register), so
                    // there is no second lane to splice in here — the old
                    // `cwd |= (extract lane 1) << (16 - cnt)` reassembly step is gone entirely.
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

                spp_pos += cnt;
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
#[target_feature(enable = "avx2")]
unsafe fn decode_mrp_avx2(
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
    // de-stuff the MRP segment; consumption is at most 1 bit per sample = 512 bytes, so
    // 1024 bytes always suffice (matches the C++ original's comment/sizing exactly)
    const MRP_CAP: usize = 1024;
    let mut mrp_buf = [0u8; MRP_CAP + 72];
    let mrp_limit = destuff_mrp(
        coded_data as *const Ui8,
        lengths1 as i32,
        lengths2 as i32,
        mrp_buf.as_mut_ptr(),
        MRP_CAP as Ui32,
    );
    let mut mrp_pos: Ui32 = 0;

    for y in (0..height).step_by(4) {
        let mut cur_sig = sigma.as_ptr().add(((y >> 2) * mstr) as usize);
        let mut dpp = decoded_data.add((y * stride) as usize);
        for _ in (0..width).step_by(4) {
            let cwd = dfetch64(mrp_buf.as_ptr(), mrp_limit, mrp_pos) as Ui32;
            let sig = *cur_sig;
            cur_sig = cur_sig.add(1);
            let mut total_bits = 0u32;

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
                let mut dp = dpp;
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

            mrp_pos += total_bits;
            dpp = dpp.add(4);
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[target_feature(enable = "avx2")]
pub(crate) unsafe fn ojph_decode_codeblock_avx2(
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
        if !INSUFFICIENT_PRECISION_AVX2.swap(true, Ordering::Relaxed) {
            ojph_warn!(
                0x00010003,
                "32 bits are not enough to decode this codeblock. This message will not be displayed again."
            );
        }
        return false;
    } else if missing_msbs == 30 {
        if !MODIFY_CODE_AVX2.swap(true, Ordering::Relaxed) {
            ojph_warn!(
                0x00010004,
                "Not enough precision to decode the cleanup pass. The code can be modified to support this case. This message will not be displayed again."
            );
        }
        return false;
    } else if missing_msbs == 29 && num_passes > 1 {
        num_passes = 1;
        if !TRUNCATE_SPP_MRP_AVX2.swap(true, Ordering::Relaxed) {
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

    if !decode_step2_magsgn_avx2(
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

    let mstr = rearrange_sigma_avx2(width, height, sstr, &mut scratch);

    if num_passes > 1 {
        decode_spp_avx2(
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
            decode_mrp_avx2(
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
