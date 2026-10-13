//! Safe public wrappers for the HEVC inverse-transform family.
//!
//! Byte-exact port of ffmpeg-rs `av-codec-h265/transform/idct.rs`.  Each
//! transform operates **in-place on an `i16` coefficient block** (the
//! add-to-plane is the separate, already-SIMD `add_clip_to_pixel_*` step).
//! `bit_depth` is an HEVC profile depth `∈ {8,9,10,12}`.

use crate::dispatch;

/// HEVC size-4 inverse DCT-II (`idct_nxn` size-4 path).  Two passes (columns
/// `>> 7`, rows `>> (20 - bit_depth)`) of the DCT-4 butterfly with per-element
/// `clip_int16` SCALE, in-place on the 4×4 (row-major) `i16` block.
///
/// # Panics
/// If `coeffs.len() < 16`.
pub fn hevc_idct4_i16_inplace(coeffs: &mut [i16], bit_depth: u8) {
    assert!(
        coeffs.len() >= 16,
        "codec-simd: hevc_idct4_i16_inplace needs 16 coeffs (got {})",
        coeffs.len()
    );
    debug_assert!(
        (8..=12).contains(&bit_depth),
        "codec-simd: hevc_idct4_i16_inplace bit_depth must be 8..=12 (got {bit_depth})"
    );
    // SAFETY: length verified; the kernel touches exactly coeffs[0..16].
    unsafe {
        (dispatch::get().hevc_idct4_i16_inplace)(coeffs.as_mut_ptr(), bit_depth);
    }
}

/// HEVC 4×4 inverse DST-VII for intra luma (`transform_4x4_luma`).  Same
/// two-pass SCALE model as [`hevc_idct4_i16_inplace`] with the DST-VII
/// (29/55/74) butterfly, in-place on the 4×4 `i16` block.
///
/// # Panics
/// If `coeffs.len() < 16`.
pub fn hevc_dst4_i16_inplace(coeffs: &mut [i16], bit_depth: u8) {
    assert!(
        coeffs.len() >= 16,
        "codec-simd: hevc_dst4_i16_inplace needs 16 coeffs (got {})",
        coeffs.len()
    );
    debug_assert!(
        (8..=12).contains(&bit_depth),
        "codec-simd: hevc_dst4_i16_inplace bit_depth must be 8..=12 (got {bit_depth})"
    );
    // SAFETY: length verified; the kernel touches exactly coeffs[0..16].
    unsafe {
        (dispatch::get().hevc_dst4_i16_inplace)(coeffs.as_mut_ptr(), bit_depth);
    }
}

/// HEVC DC-only inverse-transform fast path (`idct_NxN_dc`).  Fills the whole
/// `size×size` block (`size = 1 << log2_size`, `log2_size ∈ {2,3,4,5}` for
/// sizes 4/8/16/32) with `clip_int16((((coeffs[0] + 1) >> 1) + add) >> (14 -
/// bit_depth))`.
///
/// # Panics
/// If `log2_size` is not in `2..=5`, or `coeffs.len() < size*size`.
pub fn hevc_idct_dc_i16_inplace(coeffs: &mut [i16], log2_size: u32, bit_depth: u8) {
    assert!(
        (2..=5).contains(&log2_size),
        "codec-simd: hevc_idct_dc_i16_inplace log2_size must be 2..=5 (got {log2_size})"
    );
    let size = 1usize << log2_size;
    let need = size * size;
    assert!(
        coeffs.len() >= need,
        "codec-simd: hevc_idct_dc_i16_inplace needs {} coeffs (got {})",
        need,
        coeffs.len()
    );
    debug_assert!(
        (8..=12).contains(&bit_depth),
        "codec-simd: hevc_idct_dc_i16_inplace bit_depth must be 8..=12 (got {bit_depth})"
    );
    // SAFETY: length verified; the kernel touches exactly coeffs[0..size*size].
    unsafe {
        (dispatch::get().hevc_idct_dc_i16_inplace)(coeffs.as_mut_ptr(), log2_size, bit_depth);
    }
}

/// HEVC size-8 inverse DCT-II (`idct_nxn` size-8 path).  Two passes (columns
/// `>> 7`, rows `>> (20 - bit_depth)`) of the DCT-8 partial (even/odd)
/// butterfly with per-element `clip_int16` SCALE, in-place on the 8×8
/// (row-major) `i16` block.  `col_limit` is the `last_significant_coeff`-derived
/// odd-coefficient pruning bound (pass `2 * size` = fully-populated).
///
/// # Panics
/// If `coeffs.len() < 64`.
pub fn hevc_idct8_i16_inplace(coeffs: &mut [i16], col_limit: i32, bit_depth: u8) {
    assert!(
        coeffs.len() >= 64,
        "codec-simd: hevc_idct8_i16_inplace needs 64 coeffs (got {})",
        coeffs.len()
    );
    debug_assert!(
        (8..=12).contains(&bit_depth),
        "codec-simd: hevc_idct8_i16_inplace bit_depth must be 8..=12 (got {bit_depth})"
    );
    // SAFETY: length verified; the kernel touches exactly coeffs[0..64].
    unsafe {
        (dispatch::get().hevc_idct8_i16_inplace)(coeffs.as_mut_ptr(), col_limit, bit_depth);
    }
}

/// HEVC size-16 inverse DCT-II (`idct_nxn` size-16 path).  Same two-pass SCALE
/// model as [`hevc_idct8_i16_inplace`] with the DCT-16 partial butterfly,
/// in-place on the 16×16 `i16` block.
///
/// # Panics
/// If `coeffs.len() < 256`.
pub fn hevc_idct16_i16_inplace(coeffs: &mut [i16], col_limit: i32, bit_depth: u8) {
    assert!(
        coeffs.len() >= 256,
        "codec-simd: hevc_idct16_i16_inplace needs 256 coeffs (got {})",
        coeffs.len()
    );
    debug_assert!(
        (8..=12).contains(&bit_depth),
        "codec-simd: hevc_idct16_i16_inplace bit_depth must be 8..=12 (got {bit_depth})"
    );
    // SAFETY: length verified; the kernel touches exactly coeffs[0..256].
    unsafe {
        (dispatch::get().hevc_idct16_i16_inplace)(coeffs.as_mut_ptr(), col_limit, bit_depth);
    }
}

/// HEVC size-32 inverse DCT-II (`idct_nxn` size-32 path).  Same two-pass SCALE
/// model as [`hevc_idct8_i16_inplace`] with the DCT-32 partial butterfly (the
/// only level whose even sub-transform prunes at `end/2`), in-place on the
/// 32×32 `i16` block.
///
/// # Panics
/// If `coeffs.len() < 1024`.
pub fn hevc_idct32_i16_inplace(coeffs: &mut [i16], col_limit: i32, bit_depth: u8) {
    assert!(
        coeffs.len() >= 1024,
        "codec-simd: hevc_idct32_i16_inplace needs 1024 coeffs (got {})",
        coeffs.len()
    );
    debug_assert!(
        (8..=12).contains(&bit_depth),
        "codec-simd: hevc_idct32_i16_inplace bit_depth must be 8..=12 (got {bit_depth})"
    );
    // SAFETY: length verified; the kernel touches exactly coeffs[0..1024].
    unsafe {
        (dispatch::get().hevc_idct32_i16_inplace)(coeffs.as_mut_ptr(), col_limit, bit_depth);
    }
}
