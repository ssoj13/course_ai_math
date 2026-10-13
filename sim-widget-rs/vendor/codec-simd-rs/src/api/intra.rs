//! Safe public wrappers for the HEVC **intra-prediction** primitive family
//! (planar + angular fill kernels) on the `u16` picture plane.
//!
//! Byte-exact port of ffmpeg-rs `av-codec-h265/pred.rs` (`pred_planar` /
//! `pred_angular`). These are the DSP FILL kernels: the caller supplies the
//! substituted/filtered references and (for angular) the fully-built `ref_buf`,
//! and does the mode-26/10 boundary filter + any `clip_pixel` scalar AFTER the
//! call. Strides are in **u16 elements**.

use crate::dispatch;

/// HEVC **planar** prediction on a `u16` block: per pixel
/// `dst = ((size-1-x)·left[y] + (x+1)·top[size] + (size-1-y)·top[x] +
/// (y+1)·left[size] + size) >> (log2_size+1)`, `size = 1 << log2_size`.
///
/// `top`/`left` are the reference rows (index `0..=size`; `top[size]`/
/// `left[size]` are the far corner samples). `log2_size` is `2..=5` (block
/// sizes 4/8/16/32). The weighted sum accumulates in i32; the output is in
/// `[0, 2^bd)` so the `u16` store needs no clip.
///
/// # Panics
/// If `dst_stride < 0`, `log2_size` is not `2..=5`, `top`/`left` are shorter
/// than `size+1`, or `dst` is too short for the `size×size` block at `dst_stride`.
pub fn hevc_intra_planar_u16(
    dst: &mut [u16],
    dst_stride: isize,
    top: &[u16],
    left: &[u16],
    log2_size: u32,
) {
    // The kernel steps rows by `y*dst_stride` for `y in 0..size`; a negative
    // stride would write at offsets BELOW `dst.as_mut_ptr()` (out of the slice /
    // UB in the internal unsafe). HEVC plane strides are always positive.
    assert!(
        dst_stride >= 0,
        "codec-simd: hevc_intra_planar_u16 dst_stride must be >= 0 (got {dst_stride})"
    );
    assert!(
        (2..=5).contains(&log2_size),
        "codec-simd: hevc_intra_planar_u16 log2_size must be 2..=5 (got {log2_size})"
    );
    let size = 1usize << log2_size;
    assert!(
        top.len() > size,
        "codec-simd: hevc_intra_planar_u16 top too short ({} <= {size})",
        top.len()
    );
    assert!(
        left.len() > size,
        "codec-simd: hevc_intra_planar_u16 left too short ({} <= {size})",
        left.len()
    );
    let nd = (size - 1) * dst_stride.unsigned_abs() + size;
    assert!(
        dst.len() >= nd,
        "codec-simd: hevc_intra_planar_u16 dst too short ({} < {nd})",
        dst.len()
    );
    // SAFETY: geometry verified above.
    unsafe {
        (dispatch::get().hevc_intra_planar_u16)(
            dst.as_mut_ptr(),
            dst_stride,
            top.as_ptr(),
            left.as_ptr(),
            log2_size,
        );
    }
}

/// HEVC **angular** prediction FILL on a `u16` block (modes 2..=34). For each
/// projection index `r` (`r = y` if `vertical`, else `r = x`) the kernel
/// computes `idx = ((r+1)·angle) >> 5`, `fact = ((r+1)·angle) & 31` and the
/// 2-tap FIR `dst = ((32-fact)·ref[k+1] + fact·ref[k+2] + 16) >> 5`
/// (`k = idx`), reduced to a copy of `ref[k+1]` when `fact == 0`.
///
/// The caller builds `ref_buf` (the C `ref` array, incl. the negative-angle
/// `INV_ANGLE` back-projection) and passes its `bias`: `ref_buf[k + bias]` ==
/// C `ref[k]` (so negative `k` down to `last` map to `ref_buf[bias + k]`, which
/// is why the FULL slice + `bias` is passed, not `&ref_buf[bias..]`). `angle`
/// is `INTRA_PRED_ANGLE[mode-2]`. `vertical == mode >= 18`. `size` is the block
/// side (4/8/16/32). The 2-tap MAC accumulates in i32; the output is in
/// `[0, 2^bd)`. The horizontal family is computed with the same contiguous-row
/// kernel into an internal scratch and transpose-stored (byte-exact data move).
/// The mode-26/10 boundary filter + `clip_pixel` are the caller's, post-call.
///
/// # Panics
/// If `dst_stride < 0`, `size` is not a power of two in `4..=32`, `bias < size`,
/// `ref_buf` is shorter than the tap window (`bias + 2·size + 2`), or `dst` is
/// too short for the `size×size` block at `dst_stride`.
pub fn hevc_intra_angular_u16(
    dst: &mut [u16],
    dst_stride: isize,
    ref_buf: &[i32],
    bias: usize,
    size: usize,
    angle: i32,
    vertical: bool,
) {
    // Rows step by `y*dst_stride`; a negative stride writes below `dst` (UB).
    // HEVC plane strides are always positive.
    assert!(
        dst_stride >= 0,
        "codec-simd: hevc_intra_angular_u16 dst_stride must be >= 0 (got {dst_stride})"
    );
    assert!(
        matches!(size, 4 | 8 | 16 | 32),
        "codec-simd: hevc_intra_angular_u16 size must be 4/8/16/32 (got {size})"
    );
    // `bias >= size` guarantees the lowest back-projected read (`bias + last`,
    // `last >= -size`) stays non-negative; the highest read is `bias + 2·size+1`.
    assert!(
        bias >= size,
        "codec-simd: hevc_intra_angular_u16 bias must be >= size ({bias} < {size})"
    );
    let need = bias + 2 * size + 2;
    assert!(
        ref_buf.len() >= need,
        "codec-simd: hevc_intra_angular_u16 ref_buf too short ({} < {need})",
        ref_buf.len()
    );
    let nd = (size - 1) * dst_stride.unsigned_abs() + size;
    assert!(
        dst.len() >= nd,
        "codec-simd: hevc_intra_angular_u16 dst too short ({} < {nd})",
        dst.len()
    );
    // SAFETY: geometry verified above; the tap window is bounded by the asserts.
    unsafe {
        (dispatch::get().hevc_intra_angular_u16)(
            dst.as_mut_ptr(),
            dst_stride,
            ref_buf.as_ptr(),
            bias,
            size,
            angle,
            vertical,
        );
    }
}
