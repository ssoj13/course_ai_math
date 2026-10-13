//! Convert / narrow / clip family — pixel-range packing, clamping, widening
//! multiply-accumulate, and round-shift-narrow conversions.

use crate::dispatch;

// =========================================================================
// 8-bit pixel clip (v0.1.0).
// =========================================================================

/// Saturating-narrow `i16` lanes to `u8`, lane-wise.
///
/// # Panics
/// If `src.len() != dst.len()`.
pub fn sat_pack_i16_to_u8(src: &[i16], dst: &mut [u8]) {
    assert_eq!(
        src.len(),
        dst.len(),
        "codec-simd: sat_pack_i16_to_u8 length mismatch (src={}, dst={})",
        src.len(),
        dst.len()
    );
    unsafe {
        (dispatch::get().sat_pack_i16_to_u8)(src.as_ptr(), dst.as_mut_ptr(), src.len());
    }
}

// =========================================================================
// High-bit-depth pixel clip (v0.1.1).
// =========================================================================

/// Saturating-narrow `i32` lanes to `u16` clipped to `[0, max]`, lane-wise.
///
/// # Panics
/// If `src.len() != dst.len()`.
pub fn sat_pack_i32_to_u16(src: &[i32], dst: &mut [u16], max: u16) {
    assert_eq!(
        src.len(),
        dst.len(),
        "codec-simd: sat_pack_i32_to_u16 length mismatch (src={}, dst={})",
        src.len(),
        dst.len()
    );
    unsafe {
        (dispatch::get().sat_pack_i32_to_u16)(src.as_ptr(), dst.as_mut_ptr(), src.len(), max);
    }
}

// =========================================================================
// Fused residual-add + clip (v0.1.2).
// =========================================================================

/// Per lane: `dst[i] = clamp(pred[i] as i32 + res[i] as i32, 0, max) as u16`.
///
/// # Panics
/// If `pred.len() != res.len() || pred.len() != dst.len()`.
pub fn add_clip_to_pixel_u16(pred: &[u16], res: &[i16], dst: &mut [u16], max: u16) {
    assert_eq!(
        pred.len(),
        res.len(),
        "codec-simd: add_clip_to_pixel_u16 pred/res length mismatch ({} vs {})",
        pred.len(),
        res.len()
    );
    assert_eq!(
        pred.len(),
        dst.len(),
        "codec-simd: add_clip_to_pixel_u16 pred/dst length mismatch ({} vs {})",
        pred.len(),
        dst.len()
    );
    unsafe {
        (dispatch::get().add_clip_to_pixel_u16)(
            pred.as_ptr(),
            res.as_ptr(),
            dst.as_mut_ptr(),
            pred.len(),
            max,
        );
    }
}

/// Per lane: `dst[i] = clamp(pred[i] as i32 + res[i] as i32, 0, 255) as u8`.
///
/// The u8 sibling of [`add_clip_to_pixel_u16`] with a fixed clamp ceiling of
/// 255 — fused add-residual + clip for 8-bit reconstruction (H.264/HEVC IDCT
/// store, ProRes, AV1 recon).  Pass `dst == pred` for in-place reconstruction.
///
/// The residual is `i16` and the pixel is `0..=255`, so the sum can exceed
/// `i16` range (`255 + i16::MAX = 33022`); every backend accumulates in `i32`.
///
/// # Panics
/// If `pred.len() != res.len() || pred.len() != dst.len()`.
pub fn add_clip_to_pixel_u8(pred: &[u8], res: &[i16], dst: &mut [u8]) {
    assert_eq!(
        pred.len(),
        res.len(),
        "codec-simd: add_clip_to_pixel_u8 pred/res length mismatch ({} vs {})",
        pred.len(),
        res.len()
    );
    assert_eq!(
        pred.len(),
        dst.len(),
        "codec-simd: add_clip_to_pixel_u8 pred/dst length mismatch ({} vs {})",
        pred.len(),
        dst.len()
    );
    unsafe {
        (dispatch::get().add_clip_to_pixel_u8)(
            pred.as_ptr(),
            res.as_ptr(),
            dst.as_mut_ptr(),
            pred.len(),
        );
    }
}

// =========================================================================
// Fused residual-add + clip, IN-PLACE (v0.3.21).
// =========================================================================

/// In-place sibling of [`add_clip_to_pixel_u16`]; `pix` is both prediction
/// and destination.
///
/// Per lane: `pix[i] = clamp(pix[i] as i32 + res[i] as i32, 0, max) as u16`.
///
/// This is the exact shape decoder reconstruction needs — HEVC/H.264
/// `add_residual` accumulates the inverse-transform residual straight onto
/// the prediction buffer (`dst[x] = clip(dst[x] + res[x])`).  The
/// out-of-place [`add_clip_to_pixel_u16`] cannot express that in safe Rust,
/// because `pix` would have to be borrowed as both `pred: &[u16]` and
/// `dst: &mut [u16]` simultaneously; without this wrapper a caller must copy
/// the prediction into a scratch buffer.  Here the SAME buffer is handed to
/// the dispatched kernel as both `pred` and `dst`.
///
/// # Aliasing soundness
/// Passing `pred == dst` is sound because every `add_clip_to_pixel_u16`
/// kernel (scalar + SSE4.1 + AVX2 + NEON + wasm) is strictly *element-local*:
/// output index `i` reads only `pred[i]` and `res[i]` and writes only
/// `dst[i]`, and each SIMD block loads its whole `[i..i+N)` window before
/// storing to the same window.  No lane ever reads an index that an earlier
/// store has already overwritten (blocks cover disjoint index ranges and the
/// load precedes the store within a block), so aliasing `pred` onto `dst`
/// cannot change any result — the in-place output is byte-identical to the
/// out-of-place path at every SIMD tier.
///
/// # Panics
/// If `pix.len() != res.len()`.
pub fn add_clip_to_pixel_u16_inplace(pix: &mut [u16], res: &[i16], max: u16) {
    assert_eq!(
        pix.len(),
        res.len(),
        "codec-simd: add_clip_to_pixel_u16_inplace pix/res length mismatch ({} vs {})",
        pix.len(),
        res.len()
    );
    unsafe {
        (dispatch::get().add_clip_to_pixel_u16)(
            pix.as_ptr(),
            res.as_ptr(),
            pix.as_mut_ptr(),
            pix.len(),
            max,
        );
    }
}

/// In-place sibling of [`add_clip_to_pixel_u8`]; `pix` is both prediction
/// and destination.
///
/// Per lane: `pix[i] = clamp(pix[i] as i32 + res[i] as i32, 0, 255) as u8` —
/// the 8-bit fixed-ceiling counterpart of [`add_clip_to_pixel_u16_inplace`]
/// for 8-bit decoder reconstruction (H.264/HEVC IDCT store, ProRes, AV1).
///
/// # Aliasing soundness
/// Identical reasoning to [`add_clip_to_pixel_u16_inplace`]: every
/// `add_clip_to_pixel_u8` kernel is element-local (output `i` reads only
/// `pred[i]`/`res[i]`, writes only `dst[i]`, load-before-store within each
/// disjoint block), so `pred == dst` aliasing is sound and byte-identical to
/// the out-of-place path at every SIMD tier.
///
/// # Panics
/// If `pix.len() != res.len()`.
pub fn add_clip_to_pixel_u8_inplace(pix: &mut [u8], res: &[i16]) {
    assert_eq!(
        pix.len(),
        res.len(),
        "codec-simd: add_clip_to_pixel_u8_inplace pix/res length mismatch ({} vs {})",
        pix.len(),
        res.len()
    );
    unsafe {
        (dispatch::get().add_clip_to_pixel_u8)(
            pix.as_ptr(),
            res.as_ptr(),
            pix.as_mut_ptr(),
            pix.len(),
        );
    }
}

// =========================================================================
// In-place clamp i32 → i16 range (v0.1.3).
// =========================================================================

/// Clamp every `i32` lane to `[i16::MIN, i16::MAX]`, in-place.
pub fn clamp_i32_to_i16(buf: &mut [i32]) {
    unsafe {
        (dispatch::get().clamp_i32_to_i16)(buf.as_mut_ptr(), buf.len());
    }
}

// =========================================================================
// Widening multiply-accumulate (v0.1.5).
// =========================================================================

/// Per lane: `acc[i] = acc[i].wrapping_add(a[i] as i32 * b[i] as i32)`.
///
/// Lane-wise widening multiply-accumulate.  The basis of every separable
/// MC convolution (filter coeffs × samples) and every DCT inner product.
///
/// `wrapping` because the caller controls saturation — in real codec
/// uses (bounded 8-tap filter coeffs × bounded samples × bounded tap
/// count) the accumulator never overflows `i32` so the wrap never fires.
///
/// # Panics
/// If `a.len() != b.len() || a.len() != acc.len()`.
pub fn mac_widen_i16_to_i32(a: &[i16], b: &[i16], acc: &mut [i32]) {
    assert_eq!(
        a.len(),
        b.len(),
        "codec-simd: mac_widen_i16_to_i32 a/b length mismatch ({} vs {})",
        a.len(),
        b.len()
    );
    assert_eq!(
        a.len(),
        acc.len(),
        "codec-simd: mac_widen_i16_to_i32 a/acc length mismatch ({} vs {})",
        a.len(),
        acc.len()
    );
    unsafe {
        (dispatch::get().mac_widen_i16_to_i32)(a.as_ptr(), b.as_ptr(), acc.as_mut_ptr(), a.len());
    }
}

// =========================================================================
// Round-shift-narrow i32 → i16 (v0.1.9).
// =========================================================================

/// Per lane: `dst[i] = saturating((src[i] + (1 << (shift-1))) >> shift) as i16`.
///
/// Combined "add rounding bias, arithmetic right shift, saturating
/// narrow to i16" — the standard final stage of every DCT/IDCT and
/// every fixed-point filter that produces i16-range coefficients from
/// i32 accumulators.
///
/// `shift` must be in `1..=31`.  Typical codec values: 6 (4×4 IDCT
/// final stage), 12 (8×8 IDCT), etc.
///
/// Note: on wasm `simd128` this falls back to scalar — wasm SIMD has no
/// runtime variable shift on `i32x4` (only compile-time-constant shifts
/// via `i32x4_shr::<N>`).  A const-generic sibling that gets full SIMD
/// on wasm is on the roadmap.
///
/// # Panics
/// If `src.len() != dst.len()` or `!(1..=31).contains(&shift)`.
pub fn round_shift_narrow_i32_to_i16(src: &[i32], dst: &mut [i16], shift: u32) {
    assert_eq!(
        src.len(),
        dst.len(),
        "codec-simd: round_shift_narrow_i32_to_i16 length mismatch ({} vs {})",
        src.len(),
        dst.len()
    );
    assert!(
        (1..=31).contains(&shift),
        "codec-simd: round_shift_narrow_i32_to_i16 shift must be 1..=31 (got {shift})"
    );
    unsafe {
        (dispatch::get().round_shift_narrow_i32_to_i16)(
            src.as_ptr(),
            dst.as_mut_ptr(),
            src.len(),
            shift,
        );
    }
}
