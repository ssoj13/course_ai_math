//! Safe public wrappers for the HEVC **SAO** (Sample Adaptive Offset) band +
//! edge filter family.
//!
//! Byte-exact port of ffmpeg-rs `av-codec-h265/filter/sao.rs` (`sao_band` /
//! `sao_edge`).  Both operate on the reconstructed `u16` picture plane (8/10-
//! bit, up to 12-bit); `max = (1 << bit_depth) - 1`.  Strides are in **u16
//! elements**.  Each call filters one `w × h` region: the edge kernel reads
//! spatial neighbours, so a pixel whose neighbour is outside the region keeps
//! its value (the picture-boundary `sao_edge_restore_0` rule) — pass the whole
//! plane as the region for the picture-level filter.

use crate::dispatch;

/// SAO **band** offset on a `u16` region: per pixel,
/// `dst = clip(src + offset_table[(src >> band_shift) & 31], 0, max)`.
///
/// `offset_table` is the caller-built 32-entry band LUT (only the ≤4-entry
/// band window is non-zero); `band_shift = bit_depth - 5`.  Reads only the
/// pixel itself, so `src == dst` in-place is safe.  Strides in **u16
/// elements**.
///
/// # Panics
/// If `band_shift >= 16`, or `src`/`dst` is too short for the `w × h` geometry.
pub fn sao_band_u16(
    dst: &mut [u16],
    dst_stride: isize,
    src: &[u16],
    src_stride: isize,
    w: usize,
    h: usize,
    offset_table: &[i16; 32],
    band_shift: u32,
    max: u16,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        band_shift < 16,
        "codec-simd: sao_band_u16 band_shift must be 0..=15 (got {band_shift})"
    );
    let ns = (h - 1) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= ns,
        "codec-simd: sao_band_u16 src too short ({} < {})",
        src.len(),
        ns
    );
    let nd = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= nd,
        "codec-simd: sao_band_u16 dst too short ({} < {})",
        dst.len(),
        nd
    );
    // SAFETY: geometry verified above; `offset_table` is a `&[i16; 32]`.
    unsafe {
        (dispatch::get().sao_band_u16)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            offset_table.as_ptr(),
            band_shift,
            max,
        );
    }
}

/// SAO **edge** offset on a `u16` region: per pixel, `category = 2 +
/// cmp(p,n0) + cmp(p,n1)` (the two `eo_class` neighbours), then
/// `dst = clip(src + offsets[edge_idx[category]], 0, max)` where
/// `edge_idx = [1,2,0,3,4]`.  A pixel whose neighbour is outside the `w × h`
/// region keeps its value.
///
/// `eo_class`: `0`=horizontal, `1`=vertical, `2`=135°, `3`=45° (clamped to
/// `0..=3`).  `offsets` is the 5-entry `sao_offset_val` (already sign-applied
/// and scaled by the caller).  `src` **must not alias** `dst` (snapshot
/// semantics — the kernel reads neighbours).  Strides in **u16 elements**.
///
/// # Panics
/// If `src`/`dst` is too short for the `w × h` geometry.
pub fn sao_edge_u16(
    dst: &mut [u16],
    dst_stride: isize,
    src: &[u16],
    src_stride: isize,
    w: usize,
    h: usize,
    eo_class: u32,
    offsets: &[i16; 5],
    max: u16,
) {
    if w == 0 || h == 0 {
        return;
    }
    let ns = (h - 1) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= ns,
        "codec-simd: sao_edge_u16 src too short ({} < {})",
        src.len(),
        ns
    );
    let nd = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= nd,
        "codec-simd: sao_edge_u16 dst too short ({} < {})",
        dst.len(),
        nd
    );
    // SAFETY: geometry verified above; `offsets` is a `&[i16; 5]`.
    unsafe {
        (dispatch::get().sao_edge_u16)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            eo_class,
            offsets.as_ptr(),
            max,
        );
    }
}
