/// Scalar reference for [`crate::mc_avg_u8`].
///
/// Per pixel: `dst[i] = ((a[i] as u16 + b[i] as u16 + 1) >> 1) as u8`.
/// Rounding halving add (round-to-nearest, ties away from zero —
/// matches the `pavgb` / `vrhaddq` / `u8x16_avgr_u` semantics).
///
/// # Safety
/// Three pointers each valid for `(h-1)*stride + w` u8 reads/writes.
pub(crate) unsafe fn mc_avg_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    dst: *mut u8,
    dst_stride: isize,
    w: usize,
    h: usize,
) {
    for row in 0..h {
        let a_row = unsafe { a.offset(row as isize * a_stride) };
        let b_row = unsafe { b.offset(row as isize * b_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let av = unsafe { *a_row.add(col) } as u16;
            let bv = unsafe { *b_row.add(col) } as u16;
            unsafe {
                *d_row.add(col) = ((av + bv + 1) >> 1) as u8;
            }
        }
    }
}

// =========================================================================
// HEVC CHROMA 4-tap MC, i32-source -> raw i32 intermediate (v0.3.23).
//
// The 4-tap sibling of the luma 8-tap family above.  Byte-exact port of
// ffmpeg-rs `av-codec-h265/mc.rs` (`chroma_h_sample` / `chroma_filter_tmp` /
// `fill_chroma_htmp`, `EPEL_FILTERS`).  Same tap-alignment convention: `src` is
// pre-offset by the caller so tap 0 aligns with output column 0 (chroma
// `EPEL_EXTRA_BEFORE = 1`, i.e. `src_origin - 1` for h, `src_origin - 1*stride`
// for v); ffmpeg's centred `base-1..base+2` taps become `base..base+3` here.
// The output is the RAW `(acc >> shift)` i32 — NO narrow, NO saturation, NO
// rounding offset, exactly as the luma family (the wrapping-i16 `put` store and
// the uni/bi `clip_pixel` are separate downstream steps).
// =========================================================================

/// Scalar reference for [`crate::mc_4tap_h_i32_to_i32`].
///
/// HEVC chroma horizontal 4-tap over an **i32** source: per output pixel
/// `acc = Σ_{k=0..4} filter[k]·src[row*src_stride + col + k]` (i32), stored as
/// the RAW `(acc >> shift)` i32 — NO narrow, NO saturation, NO rounding offset
/// (ffmpeg-rs `chroma_h_sample`).  `shift = bit_depth - 8` (0/2/4 for
/// 8/10/12-bit).
///
/// # Safety
/// `src` valid for `src + row*src_stride + col + k` (`k ∈ 0..4`); `dst` valid
/// for `w×h` i32 writes; `filter` valid for 4 i8 reads; `shift ≤ 14`.
pub(crate) unsafe fn mc_4tap_h_i32_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const i32,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    let mut f = [0i32; 4];
    for i in 0..4 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    for row in 0..h {
        let s_row = unsafe { src.offset(row as isize * src_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..4 {
                acc += unsafe { *s_row.add(col + i) } * f[i];
            }
            unsafe {
                *d_row.add(col) = acc >> shift;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_4tap_v_i32_to_i32`].  The vertical sibling
/// of [`mc_4tap_h_i32_to_i32`]: taps stride down columns.  Per output pixel
/// `acc = Σ_{k=0..4} filter[k]·src[(row+k)*src_stride + col]`, stored as the
/// RAW `(acc >> shift)` i32 — NO narrow, NO saturation, NO rounding offset.
///
/// # Safety
/// `src` valid for `src + (row+k)*src_stride + col` (`k ∈ 0..4`); `dst` valid
/// for `w×h` i32 writes; `filter` valid for 4 i8 reads; `shift ≤ 14`.
pub(crate) unsafe fn mc_4tap_v_i32_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const i32,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    let mut f = [0i32; 4];
    for i in 0..4 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..4 {
                let s = unsafe { *src.offset((row + i) as isize * src_stride).add(col) };
                acc += s * f[i];
            }
            unsafe {
                *d_row.add(col) = acc >> shift;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_4tap_v_i16_to_i32`].  The HEVC chroma hv
/// two-pass **second** (vertical) pass over the i16 horizontal-pass intermediate
/// (`htmp`): `acc = Σ_{k=0..4} filter[k]·src[(row+k)*src_stride + col]`, stored
/// as the RAW `(acc >> shift)` i32.  NO narrow, NO saturation, NO rounding
/// offset (ffmpeg-rs `chroma_filter_tmp >> 6`, so the caller passes `shift = 6`).
/// Like the luma second pass this output CAN exceed i16 (for extreme `htmp`),
/// which is why it must stay i32.
///
/// # Safety
/// `src` valid for `src + (row+k)*src_stride + col` (`k ∈ 0..4`, i16 reads);
/// `dst` valid for `w×h` i32 writes; `filter` valid for 4 i8 reads;
/// `shift ≤ 14`.
pub(crate) unsafe fn mc_4tap_v_i16_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    let mut f = [0i32; 4];
    for i in 0..4 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..4 {
                let s = unsafe { *src.offset((row + i) as isize * src_stride).add(col) } as i32;
                acc += s * f[i];
            }
            unsafe {
                *d_row.add(col) = acc >> shift;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_8tap_h_u8`].
///
/// Per output pixel: `acc = Σ_{i=0..8} src[row*src_stride + col + i] *
/// filter[i]` (i32), then `dst = clip_u8((acc + 64) >> 7)`.  `src` is
/// pre-offset so tap 0 aligns with `col` (see the public wrapper).
///
/// # Safety
/// `src` valid for reads at `src + row*src_stride + col + i` for
/// `row ∈ 0..h`, `col ∈ 0..w`, `i ∈ 0..8`; `dst` valid for `w×h` writes;
/// `filter` valid for 8 reads.
pub(crate) unsafe fn mc_8tap_h_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
) {
    let mut f = [0i32; 8];
    for i in 0..8 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    for row in 0..h {
        let s_row = unsafe { src.offset(row as isize * src_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                acc += unsafe { *s_row.add(col + i) } as i32 * f[i];
            }
            let v = ((acc + 64) >> 7).clamp(0, 255) as u8;
            unsafe {
                *d_row.add(col) = v;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_8tap_v_u8`].
///
/// Vertical sibling of [`mc_8tap_h_u8`]: taps stride down columns.
/// Per output pixel: `acc = Σ_{i=0..8} src[(row+i)*src_stride + col] *
/// filter[i]` (i32), then `dst = clip_u8((acc + 64) >> 7)`.  `src` is
/// pre-offset so tap 0 aligns with output row 0.
///
/// # Safety
/// `src` valid for reads at `src + (row+i)*src_stride + col` for
/// `row ∈ 0..h`, `i ∈ 0..8`, `col ∈ 0..w`; `dst` valid for `w×h` writes;
/// `filter` valid for 8 reads.
pub(crate) unsafe fn mc_8tap_v_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
) {
    let mut f = [0i32; 8];
    for i in 0..8 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                let s = unsafe { *src.offset((row + i) as isize * src_stride).add(col) };
                acc += s as i32 * f[i];
            }
            let v = ((acc + 64) >> 7).clamp(0, 255) as u8;
            unsafe {
                *d_row.add(col) = v;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_4tap_h_u8`].
///
/// Horizontal 4-tap chroma filter with a runtime normalization `shift`.
/// Per output pixel: `acc = Σ_{i=0..4} src[col+i] * filter[i]` (i32),
/// then `dst = clip_u8((acc + (1 << (shift-1))) >> shift)`.  `src` is
/// pre-offset so tap 0 aligns with `col`.
///
/// # Safety
/// `src` valid for `src + row*src_stride + col + i` (`i ∈ 0..4`); `dst`
/// valid for `w×h`; `filter` valid for 4 reads; `shift ∈ 1..=14`.
pub(crate) unsafe fn mc_4tap_h_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    let mut f = [0i32; 4];
    for i in 0..4 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    let round = 1i32 << (shift - 1);
    for row in 0..h {
        let s_row = unsafe { src.offset(row as isize * src_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..4 {
                acc += unsafe { *s_row.add(col + i) } as i32 * f[i];
            }
            unsafe {
                *d_row.add(col) = ((acc + round) >> shift).clamp(0, 255) as u8;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_4tap_v_u8`].  Vertical 4-tap chroma
/// with runtime `shift`; taps stride down columns.
pub(crate) unsafe fn mc_4tap_v_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    let mut f = [0i32; 4];
    for i in 0..4 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    let round = 1i32 << (shift - 1);
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..4 {
                let s = unsafe { *src.offset((row + i) as isize * src_stride).add(col) };
                acc += s as i32 * f[i];
            }
            unsafe {
                *d_row.add(col) = ((acc + round) >> shift).clamp(0, 255) as u8;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_6tap_h_u8`].
///
/// Horizontal 6-tap luma filter (H.264 half-pel) with runtime `shift`.
/// Per output pixel: `acc = Σ_{i=0..6} src[col+i] * filter[i]` (i32),
/// then `dst = clip_u8((acc + (1 << (shift-1))) >> shift)`.  For H.264
/// half-pel the caller passes `[1,-5,20,20,-5,1]` and `shift = 5`.
///
/// # Safety
/// `src` valid for `src + row*src_stride + col + i` (`i ∈ 0..6`); `dst`
/// valid for `w×h`; `filter` valid for 6 reads; `shift ∈ 1..=14`.
pub(crate) unsafe fn mc_6tap_h_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    let mut f = [0i32; 6];
    for i in 0..6 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    let round = 1i32 << (shift - 1);
    for row in 0..h {
        let s_row = unsafe { src.offset(row as isize * src_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..6 {
                acc += unsafe { *s_row.add(col + i) } as i32 * f[i];
            }
            unsafe {
                *d_row.add(col) = ((acc + round) >> shift).clamp(0, 255) as u8;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_6tap_v_u8`].  Vertical 6-tap luma
/// with runtime `shift`; taps stride down columns.
pub(crate) unsafe fn mc_6tap_v_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    let mut f = [0i32; 6];
    for i in 0..6 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    let round = 1i32 << (shift - 1);
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..6 {
                let s = unsafe { *src.offset((row + i) as isize * src_stride).add(col) };
                acc += s as i32 * f[i];
            }
            unsafe {
                *d_row.add(col) = ((acc + round) >> shift).clamp(0, 255) as u8;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_bilinear_h_u8`].
///
/// Horizontal 2-tap bilinear filter with runtime `shift`.  Per output
/// pixel: `dst = clip_u8((filter[0]*src[col] + filter[1]*src[col+1]
/// + (1<<(shift-1))) >> shift)`.  For H.264 chroma the caller passes
/// `[8-frac, frac]` and `shift = 3`.
///
/// # Safety
/// `src` valid for `src + row*src_stride + col + i` (`i ∈ 0..2`); `dst`
/// valid for `w×h`; `filter` valid for 2 reads; `shift ∈ 1..=14`.
pub(crate) unsafe fn mc_bilinear_h_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    let f0 = unsafe { *filter } as i32;
    let f1 = unsafe { *filter.add(1) } as i32;
    let round = 1i32 << (shift - 1);
    for row in 0..h {
        let s_row = unsafe { src.offset(row as isize * src_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let a = unsafe { *s_row.add(col) } as i32;
            let b = unsafe { *s_row.add(col + 1) } as i32;
            let acc = f0 * a + f1 * b;
            unsafe {
                *d_row.add(col) = ((acc + round) >> shift).clamp(0, 255) as u8;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_bilinear_v_u8`].  Vertical 2-tap
/// bilinear with runtime `shift`; taps stride down columns.
pub(crate) unsafe fn mc_bilinear_v_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    let f0 = unsafe { *filter } as i32;
    let f1 = unsafe { *filter.add(1) } as i32;
    let round = 1i32 << (shift - 1);
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        let r0 = unsafe { src.offset(row as isize * src_stride) };
        let r1 = unsafe { src.offset((row + 1) as isize * src_stride) };
        for col in 0..w {
            let a = unsafe { *r0.add(col) } as i32;
            let b = unsafe { *r1.add(col) } as i32;
            let acc = f0 * a + f1 * b;
            unsafe {
                *d_row.add(col) = ((acc + round) >> shift).clamp(0, 255) as u8;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_8tap_h_u8_to_i16`].
///
/// i16-output horizontal 8-tap — the first pass of a high-precision 2D
/// filter.  `dst[r][c] = sat_i16((Σ_{i=0..8} src[r][c+i]·filter[i] +
/// round) >> shift)`, `round = if shift==0 {0} else {1<<(shift-1)}`.
/// The result is NOT clipped to pixel range — it stays signed i16 for
/// the vertical pass.  `dst_stride` is in i16 elements.
///
/// # Safety
/// `src` valid for `src + row*src_stride + col + i` (`i ∈ 0..8`); `dst`
/// valid for `w×h` i16 writes; `filter` valid for 8 reads;
/// `shift ∈ 0..=14`.
pub(crate) unsafe fn mc_8tap_h_u8_to_i16(
    dst: *mut i16,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    let mut f = [0i32; 8];
    for i in 0..8 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    let round = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
    for row in 0..h {
        let s_row = unsafe { src.offset(row as isize * src_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                acc += unsafe { *s_row.add(col + i) } as i32 * f[i];
            }
            let v = ((acc + round) >> shift).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            unsafe {
                *d_row.add(col) = v;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_8tap_v_i16_to_u8`].
///
/// Second pass of a high-precision 2D filter: vertical 8-tap over an
/// i16 intermediate, clipped back to u8.  `dst[r][c] = clip_u8((Σ_{i=0..8}
/// src[(r+i)*src_stride + c]·filter[i] + round) >> shift)`,
/// `round = if shift==0 {0} else {1<<(shift-1)}`.  `src_stride` is in i16
/// elements.
///
/// # Safety
/// `src` valid for `src + (row+i)*src_stride + col` (`i ∈ 0..8`, i16
/// reads); `dst` valid for `w×h` u8 writes; `filter` valid for 8 reads;
/// `shift ∈ 0..=14`.
pub(crate) unsafe fn mc_8tap_v_i16_to_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    let mut f = [0i32; 8];
    for i in 0..8 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    let round = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                let s = unsafe { *src.offset((row + i) as isize * src_stride).add(col) } as i32;
                acc += s * f[i];
            }
            unsafe {
                *d_row.add(col) = ((acc + round) >> shift).clamp(0, 255) as u8;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_4tap_h_u8_to_i16`].  i16-output
/// horizontal 4-tap (HEVC chroma 2D first pass); see
/// [`mc_8tap_h_u8_to_i16`] for the contract.
pub(crate) unsafe fn mc_4tap_h_u8_to_i16(
    dst: *mut i16,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    let mut f = [0i32; 4];
    for i in 0..4 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    let round = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
    for row in 0..h {
        let s_row = unsafe { src.offset(row as isize * src_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..4 {
                acc += unsafe { *s_row.add(col + i) } as i32 * f[i];
            }
            let v = ((acc + round) >> shift).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            unsafe {
                *d_row.add(col) = v;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_4tap_v_i16_to_u8`].  i16-input
/// vertical 4-tap (HEVC chroma 2D second pass).
pub(crate) unsafe fn mc_4tap_v_i16_to_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    let mut f = [0i32; 4];
    for i in 0..4 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    let round = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..4 {
                let s = unsafe { *src.offset((row + i) as isize * src_stride).add(col) } as i32;
                acc += s * f[i];
            }
            unsafe {
                *d_row.add(col) = ((acc + round) >> shift).clamp(0, 255) as u8;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_6tap_h_u8_to_i16`].  i16-output
/// horizontal 6-tap (H.264 centre 2D first pass).
pub(crate) unsafe fn mc_6tap_h_u8_to_i16(
    dst: *mut i16,
    dst_stride: isize,
    src: *const u8,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    let mut f = [0i32; 6];
    for i in 0..6 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    let round = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
    for row in 0..h {
        let s_row = unsafe { src.offset(row as isize * src_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..6 {
                acc += unsafe { *s_row.add(col + i) } as i32 * f[i];
            }
            let v = ((acc + round) >> shift).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            unsafe {
                *d_row.add(col) = v;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_6tap_v_i16_to_u8`].  i16-input
/// vertical 6-tap (H.264 centre 2D second pass).
pub(crate) unsafe fn mc_6tap_v_i16_to_u8(
    dst: *mut u8,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i16,
    shift: u32,
) {
    let mut f = [0i32; 6];
    for i in 0..6 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    let round = if shift == 0 { 0 } else { 1i32 << (shift - 1) };
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..6 {
                let s = unsafe { *src.offset((row + i) as isize * src_stride).add(col) } as i32;
                acc += s * f[i];
            }
            unsafe {
                *d_row.add(col) = ((acc + round) >> shift).clamp(0, 255) as u8;
            }
        }
    }
}

/// Scalar reference for [`crate::weighted_avg_u8`].
///
/// Per pixel: `dst = clip_u8((w0*a + w1*b + offset) >> shift)`.  The
/// explicit-weighted bi-prediction blend; the caller supplies
/// `w0,w1,offset,shift` from the codec's weighted-prediction parameters.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn weighted_avg_u8(
    a: *const u8,
    a_stride: isize,
    b: *const u8,
    b_stride: isize,
    dst: *mut u8,
    dst_stride: isize,
    w: usize,
    h: usize,
    w0: i16,
    w1: i16,
    offset: i32,
    shift: u32,
) {
    let (w0, w1) = (w0 as i32, w1 as i32);
    for row in 0..h {
        let a_row = unsafe { a.offset(row as isize * a_stride) };
        let b_row = unsafe { b.offset(row as isize * b_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let av = unsafe { *a_row.add(col) } as i32;
            let bv = unsafe { *b_row.add(col) } as i32;
            let acc = (w0 * av + w1 * bv + offset) >> shift;
            unsafe {
                *d_row.add(col) = acc.clamp(0, 255) as u8;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_avg_u16`].  Rounding halving add of
/// two u16 blocks: `dst = ((a + b + 1) >> 1)`.  High-bit-depth (10/12-bit)
/// bi-prediction.  Strides are in u16 elements.
pub(crate) unsafe fn mc_avg_u16(
    a: *const u16,
    a_stride: isize,
    b: *const u16,
    b_stride: isize,
    dst: *mut u16,
    dst_stride: isize,
    w: usize,
    h: usize,
) {
    for row in 0..h {
        let a_row = unsafe { a.offset(row as isize * a_stride) };
        let b_row = unsafe { b.offset(row as isize * b_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let av = unsafe { *a_row.add(col) } as u32;
            let bv = unsafe { *b_row.add(col) } as u32;
            unsafe {
                *d_row.add(col) = ((av + bv + 1) >> 1) as u16;
            }
        }
    }
}

/// Scalar reference for [`crate::weighted_avg_u16`].
///
/// `dst = clamp((w0*a + w1*b + offset) >> shift, 0, max)` on u16 blocks.
/// High-bit-depth explicit weighted bi-prediction; `max = (1<<bd)-1`.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn weighted_avg_u16(
    a: *const u16,
    a_stride: isize,
    b: *const u16,
    b_stride: isize,
    dst: *mut u16,
    dst_stride: isize,
    w: usize,
    h: usize,
    w0: i16,
    w1: i16,
    offset: i32,
    shift: u32,
    max: u16,
) {
    let (w0, w1, maxi) = (w0 as i32, w1 as i32, max as i32);
    for row in 0..h {
        let a_row = unsafe { a.offset(row as isize * a_stride) };
        let b_row = unsafe { b.offset(row as isize * b_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let av = unsafe { *a_row.add(col) } as i32;
            let bv = unsafe { *b_row.add(col) } as i32;
            let acc = (w0 * av + w1 * bv + offset) >> shift;
            unsafe {
                *d_row.add(col) = acc.clamp(0, maxi) as u16;
            }
        }
    }
}

// =========================================================================
// HEVC luma 8-tap MC, i32-source → i16-intermediate (v0.3.22).
//
// Byte-exact port of ffmpeg-rs `av-codec-h265/mc.rs` (`luma_h_sample` /
// `luma_filter` / `luma_filter_tmp` / `fill_luma_htmp`).  The HEVC `put`
// intermediate uses a PURE arithmetic right shift with NO rounding offset:
// h/v-only pass `Σ f[k]·src[k] >> (bit_depth-8)`, hv second pass `>> 6`.
// The pre-widened source is i32 (u16 pixels in `0..(1<<bit_depth)`); the
// output is the RAW i32 intermediate (`acc >> shift`) with NO narrow and NO
// saturation: it is the value ffmpeg-rs's `put_luma*` producers carry as a
// full `int32_t` before the uni/bi `clip_pixel` (and that `put_luma` truncates
// to `int16_t` when storing bi's src2).  Narrowing/clipping to the pixel range
// is a SEPARATE downstream step, not this kernel's job.  Coefficients are i8
// (the `QPEL_FILTERS` entries).  NOTE the tap-alignment convention: as with the
// crate's other MC kernels, `src` is pre-offset by the caller so tap 0 aligns
// with output column 0 (i.e. `src_origin - 3` for h, `src_origin - 3*stride`
// for v); ffmpeg's `base-3..base+4` centred taps become `base..base+7` here.
// =========================================================================

/// Scalar reference for [`crate::mc_8tap_h_i32_to_i32`].
///
/// HEVC luma horizontal 8-tap over an **i32** source: per output pixel
/// `acc = Σ_{k=0..8} filter[k]·src[row*src_stride + col + k]` (i32), stored as
/// the RAW `(acc >> shift)` i32 — NO narrow, NO saturation, NO rounding offset
/// (ffmpeg-rs `luma_h_sample`).  `shift = bit_depth - 8` (0/2/4 for
/// 8/10/12-bit).  Over pixel-range inputs the value provably lands in
/// `[-6142, 22522]`, but the primitive stores it as i32 so the same output can
/// feed both the wrapping-i16 `put` store and the full-i32 uni/bi `clip_pixel`.
///
/// # Safety
/// `src` valid for `src + row*src_stride + col + k` (`k ∈ 0..8`); `dst` valid
/// for `w×h` i32 writes; `filter` valid for 8 i8 reads; `shift ≤ 14`.
pub(crate) unsafe fn mc_8tap_h_i32_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const i32,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    let mut f = [0i32; 8];
    for i in 0..8 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    for row in 0..h {
        let s_row = unsafe { src.offset(row as isize * src_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                acc += unsafe { *s_row.add(col + i) } * f[i];
            }
            unsafe {
                *d_row.add(col) = acc >> shift;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_8tap_v_i32_to_i32`].  The vertical sibling
/// of [`mc_8tap_h_i32_to_i32`]: taps stride down columns.  Per output pixel
/// `acc = Σ_{k=0..8} filter[k]·src[(row+k)*src_stride + col]`, stored as the
/// RAW `(acc >> shift)` i32 — NO narrow, NO saturation, NO rounding offset.
///
/// # Safety
/// `src` valid for `src + (row+k)*src_stride + col` (`k ∈ 0..8`); `dst` valid
/// for `w×h` i32 writes; `filter` valid for 8 i8 reads; `shift ≤ 14`.
pub(crate) unsafe fn mc_8tap_v_i32_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const i32,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    let mut f = [0i32; 8];
    for i in 0..8 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                let s = unsafe { *src.offset((row + i) as isize * src_stride).add(col) };
                acc += s * f[i];
            }
            unsafe {
                *d_row.add(col) = acc >> shift;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_8tap_v_i16_to_i32`].  The HEVC hv two-pass
/// **second** (vertical) pass over the i16 horizontal-pass intermediate
/// (`htmp`): `acc = Σ_{k=0..8} filter[k]·src[(row+k)*src_stride + col]`, stored
/// as the RAW `(acc >> shift)` i32.  NO narrow, NO saturation, NO rounding
/// offset (ffmpeg-rs `luma_filter_tmp >> 6`, so the caller passes `shift = 6`).
/// This second-pass output CAN exceed i16 (e.g. 33271 for a half-pel filter
/// over extreme in-range `htmp`), which is exactly why it must stay i32: the
/// uni/bi path needs the full value for `clip_pixel`, and the `put` path
/// truncates it to `int16_t` (wrapping, not saturating) itself.
///
/// # Safety
/// `src` valid for `src + (row+k)*src_stride + col` (`k ∈ 0..8`, i16 reads);
/// `dst` valid for `w×h` i32 writes; `filter` valid for 8 i8 reads;
/// `shift ≤ 14`.
pub(crate) unsafe fn mc_8tap_v_i16_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const i16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    let mut f = [0i32; 8];
    for i in 0..8 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                let s = unsafe { *src.offset((row + i) as isize * src_stride).add(col) } as i32;
                acc += s * f[i];
            }
            unsafe {
                *d_row.add(col) = acc >> shift;
            }
        }
    }
}

// =========================================================================
// HEVC MC, u16-SOURCE -> raw i32 intermediate (v0.3.27).
//
// The u16 siblings of the i32-source tap filters above: ffmpeg-rs gathers the
// reference pixels as u16 (2 bytes) instead of i32 (4 bytes) so the SIMD load
// packs 2x the lanes per register.  The math is IDENTICAL — the u16 pixel value
// (`0..(1<<bit_depth)` ⊆ i16) is widened to i32 (`as i32`, zero-extend) then the
// SAME `Σ f[k]·src[k] >> shift`, so each kernel is byte-identical to its
// i32-source twin given the same pixel values.  Only the SOURCE-reading passes
// change; the i16-htmp hv 2nd pass (`mc_*tap_v_i16_to_i32`) and the finals are
// SHARED (not duplicated).
// =========================================================================

/// Scalar reference for [`crate::mc_8tap_h_u16_to_i32`].  The u16-source sibling
/// of [`mc_8tap_h_i32_to_i32`]: widen each u16 pixel to i32 then the SAME 8-tap
/// `Σ f[k]·src[c+k] >> shift`, RAW i32 (NO narrow, NO saturation, NO rounding).
///
/// # Safety
/// `src` valid for `src + row*src_stride + col + k` (`k ∈ 0..8`, u16 reads);
/// `dst` valid for `w×h` i32 writes; `filter` valid for 8 i8 reads; `shift ≤ 14`.
pub(crate) unsafe fn mc_8tap_h_u16_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const u16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    let mut f = [0i32; 8];
    for i in 0..8 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    for row in 0..h {
        let s_row = unsafe { src.offset(row as isize * src_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                acc += unsafe { *s_row.add(col + i) } as i32 * f[i];
            }
            unsafe {
                *d_row.add(col) = acc >> shift;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_8tap_v_u16_to_i32`].  The vertical u16-source
/// sibling of [`mc_8tap_v_i32_to_i32`]: taps stride down columns, u16 widened to
/// i32 then `Σ f[k]·src[(row+k)][col] >> shift`, RAW i32.
///
/// # Safety
/// `src` valid for `src + (row+k)*src_stride + col` (`k ∈ 0..8`, u16 reads);
/// `dst` valid for `w×h` i32 writes; `filter` valid for 8 i8 reads; `shift ≤ 14`.
pub(crate) unsafe fn mc_8tap_v_u16_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const u16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    let mut f = [0i32; 8];
    for i in 0..8 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..8 {
                let s = unsafe { *src.offset((row + i) as isize * src_stride).add(col) } as i32;
                acc += s * f[i];
            }
            unsafe {
                *d_row.add(col) = acc >> shift;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_4tap_h_u16_to_i32`].  The 4-tap chroma
/// u16-source sibling of [`mc_4tap_h_i32_to_i32`].
///
/// # Safety
/// `src` valid for `src + row*src_stride + col + k` (`k ∈ 0..4`, u16 reads);
/// `dst` valid for `w×h` i32 writes; `filter` valid for 4 i8 reads; `shift ≤ 14`.
pub(crate) unsafe fn mc_4tap_h_u16_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const u16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    let mut f = [0i32; 4];
    for i in 0..4 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    for row in 0..h {
        let s_row = unsafe { src.offset(row as isize * src_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..4 {
                acc += unsafe { *s_row.add(col + i) } as i32 * f[i];
            }
            unsafe {
                *d_row.add(col) = acc >> shift;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_4tap_v_u16_to_i32`].  The 4-tap chroma
/// vertical u16-source sibling of [`mc_4tap_v_i32_to_i32`].
///
/// # Safety
/// `src` valid for `src + (row+k)*src_stride + col` (`k ∈ 0..4`, u16 reads);
/// `dst` valid for `w×h` i32 writes; `filter` valid for 4 i8 reads; `shift ≤ 14`.
pub(crate) unsafe fn mc_4tap_v_u16_to_i32(
    dst: *mut i32,
    dst_stride: isize,
    src: *const u16,
    src_stride: isize,
    w: usize,
    h: usize,
    filter: *const i8,
    shift: u32,
) {
    let mut f = [0i32; 4];
    for i in 0..4 {
        f[i] = unsafe { *filter.add(i) } as i32;
    }
    for row in 0..h {
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let mut acc: i32 = 0;
            for i in 0..4 {
                let s = unsafe { *src.offset((row + i) as isize * src_stride).add(col) } as i32;
                acc += s * f[i];
            }
            unsafe {
                *d_row.add(col) = acc >> shift;
            }
        }
    }
}

// =========================================================================
// HEVC MC "final" clip/combine kernels: i32 intermediate -> u16 pixel /
// wrapping i16 (v0.3.26).
//
// Byte-exact port of ffmpeg-rs `av-codec-h265/mc.rs` `put_luma_uni`/
// `put_luma_uni_w`/`put_luma_bi`/`put_luma_bi_w` (+ `put_luma`'s `as i16`
// store).  These take the FULLY-FILTERED i32 intermediate `v` (produced by the
// `mc_*tap_*_i32_to_i32` families above) and clip/combine it into the final
// pixel.  They are SHARED by luma and chroma: the arithmetic is element-wise
// over the i32 intermediate and is independent of the tap count.  `clip_pixel`
// is `x.clamp(0, max)` with `max = (1<<bit_depth)-1` passed by the caller
// (`offset`/`shift`/`ox`/weights/`bias` are likewise pre-derived by the
// caller, exactly as ffmpeg-rs's kernels compute them from `bit_depth`/`denom`).
// All arithmetic is i32 to match the reference `int` math (the widest product
// `v*wx1 + src2*wx0 + bias` reaches ~1.0e7 ≪ i32::MAX; see the ffmpeg-rs
// module overflow note).  The `store` narrow is WRAPPING (`as i16`), NOT
// saturating, matching `put_luma`'s bi src2 store.
// =========================================================================

/// Scalar reference for [`crate::mc_uni_clip_i32_to_u16`].
///
/// HEVC uni-prediction final clip (ffmpeg-rs `put_luma_uni`):
/// `dst = clip((v + offset) >> shift)`, `clip(x) = x.clamp(0, max)`.
///
/// # Safety
/// `v` valid for `(h-1)*v_stride + w` i32 reads; `dst` valid for
/// `(h-1)*dst_stride + w` u16 writes.
pub(crate) unsafe fn mc_uni_clip_i32_to_u16(
    dst: *mut u16,
    dst_stride: isize,
    v: *const i32,
    v_stride: isize,
    w: usize,
    h: usize,
    offset: i32,
    shift: u32,
    max: u16,
) {
    let maxi = max as i32;
    for row in 0..h {
        let v_row = unsafe { v.offset(row as isize * v_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let x = unsafe { *v_row.add(col) };
            unsafe {
                *d_row.add(col) = ((x + offset) >> shift).clamp(0, maxi) as u16;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_uni_w_clip_i32_to_u16`].
///
/// HEVC weighted uni-prediction (ffmpeg-rs `put_luma_uni_w`):
/// `dst = clip(((v * wx + offset) >> shift) + ox)`.  The `+ ox` is applied
/// AFTER the shift (the reference order).
///
/// # Safety
/// `v` valid for `(h-1)*v_stride + w` i32 reads; `dst` valid for
/// `(h-1)*dst_stride + w` u16 writes.
pub(crate) unsafe fn mc_uni_w_clip_i32_to_u16(
    dst: *mut u16,
    dst_stride: isize,
    v: *const i32,
    v_stride: isize,
    w: usize,
    h: usize,
    wx: i32,
    offset: i32,
    shift: u32,
    ox: i32,
    max: u16,
) {
    let maxi = max as i32;
    for row in 0..h {
        let v_row = unsafe { v.offset(row as isize * v_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let x = unsafe { *v_row.add(col) };
            unsafe {
                *d_row.add(col) = (((x * wx + offset) >> shift) + ox).clamp(0, maxi) as u16;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_bi_clip_i32_to_u16`].
///
/// HEVC bi-prediction combine (ffmpeg-rs `put_luma_bi`):
/// `dst = clip((v + src2 + offset) >> shift)`, where `src2` is the other list's
/// i16 first-prediction intermediate (`s2 = src2 as i32`, sign-extended).
///
/// # Safety
/// `v` valid for `(h-1)*v_stride + w` i32 reads; `src2` valid for
/// `(h-1)*src2_stride + w` i16 reads; `dst` valid for `(h-1)*dst_stride + w`
/// u16 writes.
pub(crate) unsafe fn mc_bi_clip_i32_to_u16(
    dst: *mut u16,
    dst_stride: isize,
    v: *const i32,
    v_stride: isize,
    src2: *const i16,
    src2_stride: isize,
    w: usize,
    h: usize,
    offset: i32,
    shift: u32,
    max: u16,
) {
    let maxi = max as i32;
    for row in 0..h {
        let v_row = unsafe { v.offset(row as isize * v_stride) };
        let s2_row = unsafe { src2.offset(row as isize * src2_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let x = unsafe { *v_row.add(col) };
            let s2 = unsafe { *s2_row.add(col) } as i32;
            unsafe {
                *d_row.add(col) = ((x + s2 + offset) >> shift).clamp(0, maxi) as u16;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_bi_w_clip_i32_to_u16`].
///
/// HEVC weighted bi-prediction combine (ffmpeg-rs `put_luma_bi_w`):
/// `dst = clip((v * wx1 + src2 * wx0 + bias) >> rshift)`, `rshift = log2Wd + 1`
/// and `bias = (ox0 + ox1 + 1) * (1 << log2Wd)` are pre-derived by the caller.
///
/// # Safety
/// `v` valid for `(h-1)*v_stride + w` i32 reads; `src2` valid for
/// `(h-1)*src2_stride + w` i16 reads; `dst` valid for `(h-1)*dst_stride + w`
/// u16 writes.
pub(crate) unsafe fn mc_bi_w_clip_i32_to_u16(
    dst: *mut u16,
    dst_stride: isize,
    v: *const i32,
    v_stride: isize,
    src2: *const i16,
    src2_stride: isize,
    w: usize,
    h: usize,
    wx0: i32,
    wx1: i32,
    bias: i32,
    rshift: u32,
    max: u16,
) {
    let maxi = max as i32;
    for row in 0..h {
        let v_row = unsafe { v.offset(row as isize * v_stride) };
        let s2_row = unsafe { src2.offset(row as isize * src2_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let x = unsafe { *v_row.add(col) };
            let s2 = unsafe { *s2_row.add(col) } as i32;
            unsafe {
                *d_row.add(col) = ((x * wx1 + s2 * wx0 + bias) >> rshift).clamp(0, maxi) as u16;
            }
        }
    }
}

/// Scalar reference for [`crate::mc_store_i32_to_i16`].
///
/// HEVC bi src2 store (ffmpeg-rs `put_luma`'s `... as i16`): narrow the i32
/// intermediate to i16 by WRAPPING (truncating the low 16 bits), NOT
/// saturating.  The hv second pass can legitimately exceed i16, and the C
/// `int16_t` store wraps, so the byte-exact reproduction must wrap too.
///
/// # Safety
/// `v` valid for `(h-1)*v_stride + w` i32 reads; `dst` valid for
/// `(h-1)*dst_stride + w` i16 writes.
pub(crate) unsafe fn mc_store_i32_to_i16(
    dst: *mut i16,
    dst_stride: isize,
    v: *const i32,
    v_stride: isize,
    w: usize,
    h: usize,
) {
    for row in 0..h {
        let v_row = unsafe { v.offset(row as isize * v_stride) };
        let d_row = unsafe { dst.offset(row as isize * dst_stride) };
        for col in 0..w {
            let x = unsafe { *v_row.add(col) };
            unsafe {
                *d_row.add(col) = x as i16;
            }
        }
    }
}
