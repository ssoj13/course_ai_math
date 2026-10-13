//! Element-wise integer arithmetic family — absolute difference, saturating
//! add/sub/abs, Q15 rounding multiply-high, in-register transpose, and the
//! in-place dequant scale/round/narrow.

use crate::dispatch;

// =========================================================================
// Absolute difference u8 (v0.1.4).
// =========================================================================

/// Per lane: `dst[i] = |a[i] as i32 - b[i] as i32| as u8`.
///
/// Lane-wise absolute difference.  Always fits in `u8` since both inputs
/// are `u8` so the difference is in `[0, 255]`.
///
/// Used as the building block for SAD (Sum of Absolute Differences) and
/// related encoder-side block-matching primitives that will arrive in
/// v0.2.x.  Standalone use cases include simple motion-detection
/// heuristics, image diff visualisations, and pre-pass cost estimates.
///
/// # Panics
/// If `a.len() != b.len() || a.len() != dst.len()`.
pub fn abs_diff_u8(a: &[u8], b: &[u8], dst: &mut [u8]) {
    assert_eq!(
        a.len(),
        b.len(),
        "codec-simd: abs_diff_u8 a/b length mismatch ({} vs {})",
        a.len(),
        b.len()
    );
    assert_eq!(
        a.len(),
        dst.len(),
        "codec-simd: abs_diff_u8 a/dst length mismatch ({} vs {})",
        a.len(),
        dst.len()
    );
    unsafe {
        (dispatch::get().abs_diff_u8)(a.as_ptr(), b.as_ptr(), dst.as_mut_ptr(), a.len());
    }
}

// =========================================================================
// 4×4 / 8×8 transpose i16 (v0.1.6).
// =========================================================================

/// Row-major 4×4 transpose of `i16` block.  `dst[i][j] = src[j][i]`.
///
/// Used as the cross-over between vertical and horizontal stages of every
/// 2D inverse transform (DCT/IDCT/ADST/IADST 4×4 in H.264, VP9, AV1).
///
/// # Panics
/// If `src.len() != 16 || dst.len() != 16`.
pub fn transpose_4x4_i16(src: &[i16], dst: &mut [i16]) {
    assert_eq!(
        src.len(),
        16,
        "codec-simd: transpose_4x4_i16 src.len() must be 16"
    );
    assert_eq!(
        dst.len(),
        16,
        "codec-simd: transpose_4x4_i16 dst.len() must be 16"
    );
    unsafe {
        (dispatch::get().transpose_4x4_i16)(src.as_ptr(), dst.as_mut_ptr());
    }
}

/// Row-major 8×8 transpose of `i16` block.  `dst[i][j] = src[j][i]`.
///
/// Used in 8×8 DCT/IDCT (every codec since H.264) and 8×8 SATD inside
/// encoders.  Classic 3-stage in-register transpose.
///
/// # Panics
/// If `src.len() != 64 || dst.len() != 64`.
pub fn transpose_8x8_i16(src: &[i16], dst: &mut [i16]) {
    assert_eq!(
        src.len(),
        64,
        "codec-simd: transpose_8x8_i16 src.len() must be 64"
    );
    assert_eq!(
        dst.len(),
        64,
        "codec-simd: transpose_8x8_i16 dst.len() must be 64"
    );
    unsafe {
        (dispatch::get().transpose_8x8_i16)(src.as_ptr(), dst.as_mut_ptr());
    }
}

// =========================================================================
// Saturating add/sub/abs i16 (v0.1.7).
// =========================================================================

/// Per lane: `dst[i] = (a[i] as i32 + b[i] as i32).clamp(i16::MIN..i16::MAX)`.
///
/// Saturating addition of two `i16` lanes.  The basis of deblock filter
/// inner loops (delta computation), accumulated transform stages, and
/// any DSP path that adds signed coefficients without wrapping.
///
/// # Panics
/// If `a.len() != b.len() || a.len() != dst.len()`.
pub fn sat_add_i16(a: &[i16], b: &[i16], dst: &mut [i16]) {
    assert_eq!(
        a.len(),
        b.len(),
        "codec-simd: sat_add_i16 a/b length mismatch ({} vs {})",
        a.len(),
        b.len()
    );
    assert_eq!(
        a.len(),
        dst.len(),
        "codec-simd: sat_add_i16 a/dst length mismatch ({} vs {})",
        a.len(),
        dst.len()
    );
    unsafe {
        (dispatch::get().sat_add_i16)(a.as_ptr(), b.as_ptr(), dst.as_mut_ptr(), a.len());
    }
}

/// Per lane: `dst[i] = (a[i] as i32 - b[i] as i32).clamp(i16::MIN..i16::MAX)`.
///
/// Saturating subtraction of two `i16` lanes.
///
/// # Panics
/// If `a.len() != b.len() || a.len() != dst.len()`.
pub fn sat_sub_i16(a: &[i16], b: &[i16], dst: &mut [i16]) {
    assert_eq!(
        a.len(),
        b.len(),
        "codec-simd: sat_sub_i16 a/b length mismatch ({} vs {})",
        a.len(),
        b.len()
    );
    assert_eq!(
        a.len(),
        dst.len(),
        "codec-simd: sat_sub_i16 a/dst length mismatch ({} vs {})",
        a.len(),
        dst.len()
    );
    unsafe {
        (dispatch::get().sat_sub_i16)(a.as_ptr(), b.as_ptr(), dst.as_mut_ptr(), a.len());
    }
}

/// Per lane: `dst[i] = src[i].abs()`, with `abs(i16::MIN) = i16::MAX`
/// (saturating).
///
/// Used in deblock filter strength tests and any abs-of-signed pixel
/// difference path.
///
/// # Panics
/// If `src.len() != dst.len()`.
pub fn abs_i16(src: &[i16], dst: &mut [i16]) {
    assert_eq!(
        src.len(),
        dst.len(),
        "codec-simd: abs_i16 length mismatch ({} vs {})",
        src.len(),
        dst.len()
    );
    unsafe {
        (dispatch::get().abs_i16)(src.as_ptr(), dst.as_mut_ptr(), src.len());
    }
}

// =========================================================================
// Q15 rounding multiply-high i16 (v0.1.8).
// =========================================================================

/// Per lane: `dst[i] = saturating(((a[i] as i32 * b[i] as i32) + (1 << 14)) >> 15)`.
///
/// Q15 fixed-point multiply with rounding.  The fundamental operation
/// of every fixed-point separable filter (MC tap × sample, polyphase
/// resampling, IIR coefficient).  Saturates `i16::MIN × i16::MIN` (the
/// only overflow case) to `i16::MAX`.
///
/// Backend names: x86 `_mm_mulhrs_epi16` (SSSE3+), NEON `vqrdmulhq_s16`,
/// wasm `i16x8_q15mulr_sat`.  SSE2 falls back to a 4-instruction
/// emulation (pmullw + pmulhw + unpack + srai + packs).
///
/// # Panics
/// If `a.len() != b.len() || a.len() != dst.len()`.
pub fn mul_hi_round_i16(a: &[i16], b: &[i16], dst: &mut [i16]) {
    assert_eq!(
        a.len(),
        b.len(),
        "codec-simd: mul_hi_round_i16 a/b length mismatch ({} vs {})",
        a.len(),
        b.len()
    );
    assert_eq!(
        a.len(),
        dst.len(),
        "codec-simd: mul_hi_round_i16 a/dst length mismatch ({} vs {})",
        a.len(),
        dst.len()
    );
    unsafe {
        (dispatch::get().mul_hi_round_i16)(a.as_ptr(), b.as_ptr(), dst.as_mut_ptr(), a.len());
    }
}

// =========================================================================
// Dequantization (v0.3.12).
// =========================================================================

/// In-place uniform dequantization of an `i16` coefficient block:
/// `buf[i] = sat_i16((buf[i] * scale + round) >> shift)`,
/// `round = if shift==0 {0} else {1<<(shift-1)}`.
///
/// The decoder dequant inner loop with a uniform scale (the common case;
/// per-position scaling matrices are the caller's concern — apply this
/// per run of equal scale, or pre-multiply).  `scale` is `i16`, which
/// covers typical dequant multipliers; the `i16 × i16` product fits `i32`.
///
/// # Panics
/// If `shift > 14`.
pub fn dequant_block_i16(buf: &mut [i16], scale: i16, shift: u32) {
    assert!(
        shift <= 14,
        "codec-simd: dequant_block_i16 shift must be 0..=14 (got {shift})"
    );
    unsafe {
        (dispatch::get().dequant_block_i16)(buf.as_mut_ptr(), buf.len(), scale, shift);
    }
}

// =========================================================================
// OpenEXR ZIP/RLE predictor decode-side primitives (v0.3.35).
// =========================================================================

/// In-place biased running prefix sum over `buf`: `buf[0]` unchanged, then
/// for every later byte `buf[i] = buf[i-1] + buf[i] - 128` (wrapping `u8`
/// arithmetic), using the ALREADY-UPDATED `buf[i-1]` — a true carry-forward
/// prefix sum, not an elementwise diff of the original array.
///
/// This is the DECODE side of OpenEXR's ZIP/RLE first-difference byte
/// predictor (`reconstruct` in `OpenEXRCore/internal_zip.c`), paired with
/// [`interleave_halves_u8`] for the full decode transform. The matching
/// ENCODE-side operation (deinterleave + forward difference,
/// `internal_zip_deconstruct_bytes`) has no SIMD counterpart in OpenEXR
/// itself — it stays scalar there and in any consumer, since the encode
/// side measured at only ~3-4% of total codec time in exr-rs's own
/// profiling (the decode side, this function + [`interleave_halves_u8`],
/// was ~53% of total decode time before vectorizing).
///
/// No length restriction: works correctly (falls straight to the scalar
/// tail) for any `buf.len()`, including 0 and lengths not a multiple of the
/// SIMD chunk size.
pub fn prefix_sum_bias_u8_inplace(buf: &mut [u8]) {
    unsafe {
        (dispatch::get().prefix_sum_bias_u8_inplace)(buf.as_mut_ptr(), buf.len());
    }
}

/// Re-interleave two halves into `out`: `out[2k] = t1[k]`, `out[2k+1] =
/// t2[k]`, for every `k` in range.
///
/// This is the DECODE side of OpenEXR's ZIP/RLE byte-reorder predictor
/// (`interleave` in `OpenEXRCore/internal_zip.c`), paired with
/// [`prefix_sum_bias_u8_inplace`]. Typically called as
/// `interleave_halves_u8(&scratch[..ceil(n/2)], &scratch[ceil(n/2)..n],
/// &mut out[..n])` after the scratch buffer has been reconstructed in place.
///
/// # Panics
/// If `t1.len() < out.len().div_ceil(2)` or `t2.len() < out.len() / 2`.
pub fn interleave_halves_u8(t1: &[u8], t2: &[u8], out: &mut [u8]) {
    let n = out.len();
    assert!(
        t1.len() >= n.div_ceil(2),
        "codec-simd: interleave_halves_u8 t1 too short ({} < {})",
        t1.len(),
        n.div_ceil(2)
    );
    assert!(
        t2.len() >= n / 2,
        "codec-simd: interleave_halves_u8 t2 too short ({} < {})",
        t2.len(),
        n / 2
    );
    unsafe {
        (dispatch::get().interleave_halves_u8)(t1.as_ptr(), t2.as_ptr(), out.as_mut_ptr(), n);
    }
}
