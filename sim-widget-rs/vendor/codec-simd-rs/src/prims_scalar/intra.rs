//! Scalar reference implementations of the HEVC **intra-prediction** primitive
//! family (planar + angular fill kernels) on the reconstructed `u16` picture
//! plane.
//!
//! Byte-exact port of ffmpeg-rs `av-codec-h265/pred.rs` (`pred_planar` ~201,
//! `pred_angular` ~264), itself a port of the HEVC spec 8.4.4.2.4 / 8.4.4.2.6
//! prediction (`_ref/FFmpeg/libavcodec/h26x/h2656_pred_template.c`).
//!
//! # Scope — the DSP FILL kernels only
//! These primitives are the arithmetic inner loops. The CALLER is responsible
//! for everything around them:
//! - Planar: the caller supplies the `top`/`left` reference rows (already
//!   substituted/filtered per 8.4.4.2.1/8.4.4.2.3).
//! - Angular: the caller builds the whole `ref_buf` (the C `ref` array,
//!   including the negative-angle `INV_ANGLE` back-projection) and passes it in
//!   with its `bias` (`ref_buf[k + bias]` == C `ref[k]`). The mode-26/10 luma
//!   boundary filter and any `clip_pixel` are done SCALAR by the caller AFTER
//!   this call. This primitive never reads `top`/`left` for angular — only
//!   `ref_buf` — so its reads are bounded by the buffer the caller sized.
//!
//! # i32 accumulation (see the SIMD backends)
//! Both the planar weighted sum and the angular 2-tap MAC accumulate in **i32**
//! (planar numerator peaks ~262112 at 12-bit, angular 12-bit 31·4095 = 126945;
//! both overflow i16). The `u16` narrow is the only width reduction and happens
//! at the store — the outputs are provably in `[0, 2^bd)` (planar ÷2·size,
//! angular ÷32) so no clip is needed there.

/// Scalar reference for [`crate::hevc_intra_planar_u16`].
///
/// `POS(x,y) = ((size-1-x)·left[y] + (x+1)·top[size] + (size-1-y)·top[x] +
/// (y+1)·left[size] + size) >> (log2_size+1)`, `size = 1 << log2_size`.
/// Row-major write into `dst` at row stride `dst_stride` (in u16 elements).
///
/// # Safety
/// `dst` valid for the `size×size` block at `dst_stride`; `top`/`left` valid
/// for `size+1` reads (indices `0..=size`).
pub(crate) unsafe fn hevc_intra_planar_u16(
    dst: *mut u16,
    dst_stride: isize,
    top: *const u16,
    left: *const u16,
    log2_size: u32,
) {
    unsafe {
        let size = 1usize << log2_size;
        let isize_ = size as i32;
        let top_size = *top.add(size) as i32;
        let left_size = *left.add(size) as i32;
        let shift = log2_size + 1;
        for y in 0..size {
            let topy_left = *left.add(y) as i32;
            let d_row = dst.offset(y as isize * dst_stride);
            let wy = isize_ - 1 - y as i32;
            for x in 0..size {
                let v = ((isize_ - 1 - x as i32) * topy_left
                    + (x as i32 + 1) * top_size
                    + wy * (*top.add(x) as i32)
                    + (y as i32 + 1) * left_size
                    + isize_)
                    >> shift;
                *d_row.add(x) = v as u16;
            }
        }
    }
}

/// Contiguous-row angular fill (the shared inner kernel). For each row `r` it
/// computes the scalar `idx = ((r+1)·angle) >> 5` / `fact = ((r+1)·angle) & 31`
/// and writes a CONTIGUOUS run of `size` samples through the 2-tap FIR over the
/// contiguous `ref_buf` window at `idx+1+bias`. This is the vertical family's
/// body verbatim; the horizontal family reuses it into a scratch and transposes
/// (see [`hevc_intra_angular_u16`]).
///
/// # Safety
/// `out` valid for `size` rows at `out_stride`; `ref_buf` valid for the tap
/// window (the caller-sized buffer — see the module header / the API assert).
pub(crate) unsafe fn fill_rows_scalar(
    out: *mut u16,
    out_stride: isize,
    ref_buf: *const i32,
    bias: usize,
    size: usize,
    angle: i32,
) {
    unsafe {
        for r in 0..size {
            let ra = r as i32 + 1;
            let idx = (ra * angle) >> 5;
            let fact = (ra * angle) & 31;
            let base = (idx + 1 + bias as i32) as isize;
            let out_row = out.offset(r as isize * out_stride);
            if fact != 0 {
                for j in 0..size {
                    let r1 = *ref_buf.offset(base + j as isize);
                    let r2 = *ref_buf.offset(base + j as isize + 1);
                    *out_row.add(j) = (((32 - fact) * r1 + fact * r2 + 16) >> 5) as u16;
                }
            } else {
                for j in 0..size {
                    *out_row.add(j) = *ref_buf.offset(base + j as isize) as u16;
                }
            }
        }
    }
}

/// Transpose the `size×size` row-major `scratch` (u16) into `dst`:
/// `dst[j·dst_stride + r] = scratch[r·size + j]`. A pure data move (byte-exact
/// by construction) used to turn the horizontal family's per-column result —
/// computed with the SAME contiguous-row kernel as vertical — into the strided
/// column layout, WITHOUT a per-column strided scatter.
///
/// # Safety
/// `dst` valid for `size×size` at `dst_stride`; `scratch` valid for `size·size`.
pub(crate) unsafe fn transpose_to_dst(
    dst: *mut u16,
    dst_stride: isize,
    scratch: *const u16,
    size: usize,
) {
    unsafe {
        for r in 0..size {
            for j in 0..size {
                *dst.offset(j as isize * dst_stride).add(r) = *scratch.add(r * size + j);
            }
        }
    }
}

/// Scalar reference for [`crate::hevc_intra_angular_u16`].
///
/// `vertical == true` (mode ≥ 18): outer over rows `y`, contiguous-row write.
/// `vertical == false` (mode < 18): outer over columns `x` — computed into an
/// internal scratch with the SAME contiguous-row kernel, then transpose-stored
/// (see [`fill_rows_scalar`] / [`transpose_to_dst`]).
///
/// # Safety
/// `dst` valid for `size×size` at `dst_stride`; `ref_buf`/`bias`/`size` uphold
/// the tap-window bound (see the module header / the API assert).
pub(crate) unsafe fn hevc_intra_angular_u16(
    dst: *mut u16,
    dst_stride: isize,
    ref_buf: *const i32,
    bias: usize,
    size: usize,
    angle: i32,
    vertical: bool,
) {
    unsafe {
        if vertical {
            fill_rows_scalar(dst, dst_stride, ref_buf, bias, size, angle);
        } else {
            // MAX_TB_SIZE = 32, so a size×size scratch fits 32·32 u16 on the stack.
            let mut scratch = [0u16; 32 * 32];
            fill_rows_scalar(
                scratch.as_mut_ptr(),
                size as isize,
                ref_buf,
                bias,
                size,
                angle,
            );
            transpose_to_dst(dst, dst_stride, scratch.as_ptr(), size);
        }
    }
}
