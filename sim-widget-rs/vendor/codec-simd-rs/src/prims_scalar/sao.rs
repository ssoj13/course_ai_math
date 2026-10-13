//! Scalar reference implementations of the HEVC **SAO** (Sample Adaptive
//! Offset) band + edge filter primitives, operating on the reconstructed
//! `u16` picture plane.
//!
//! Byte-exact port of ffmpeg-rs `av-codec-h265/filter/sao.rs` (`sao_band` /
//! `sao_edge`), itself a port of the SAO DSP kernels in
//! `_ref/FFmpeg/libavcodec/h26x/h2656_sao_template.c`
//! (`sao_band_filter` / `sao_edge_filter`).  Both filters read a SNAPSHOT
//! (`src`) of the post-deblock plane and write the final samples (`dst`); the
//! edge filter reads spatial neighbours, so — like the C — `src` must be an
//! unmodified snapshot distinct from `dst` (the band filter reads only the
//! pixel itself, so `src == dst` in-place is safe there).
//!
//! # Self-contained region contract
//! Each primitive processes one `w × h` region.  The band filter is a pure
//! per-pixel LUT add+clip.  The edge filter classifies each pixel against two
//! neighbours; a pixel whose neighbour falls OUTSIDE the `[0,w) × [0,h)`
//! region keeps its (deblocked) `src` value — exactly the C
//! `sao_edge_restore_0` picture-boundary rule when the region is the whole
//! plane.  (Cross-slice restoration and sub-CTB tiling are the caller's
//! concern; this primitive is the DSP kernel + picture-edge handling only.)
//!
//! Every backend (SSE2/AVX2 for edge) is fuzz-compared against this scalar
//! reference AND against an independent naive golden in `tests/cross_tier.rs`.

/// `edge_idx[]` (`h2656_sao_template.c:48`): the category→offset-index remap,
/// indexed by `2 + sign(p-n0) + sign(p-n1)` ∈ `[0,4]`; the value selects
/// `offsets[edge_idx]`.
pub(crate) const EDGE_IDX: [usize; 5] = [1, 2, 0, 3, 4];

/// `pos[4][2]` (`h2656_sao_template.c:49`): the two neighbour `(dx,dy)` offsets
/// per `eo_class` (`0`=horiz, `1`=vert, `2`=135°, `3`=45°).
pub(crate) const POS: [[(i32, i32); 2]; 4] = [
    [(-1, 0), (1, 0)],  // horizontal
    [(0, -1), (0, 1)],  // vertical
    [(-1, -1), (1, 1)], // 45 degree (SAO_EO_135D index)
    [(1, -1), (-1, 1)], // 135 degree (SAO_EO_45D index)
];

/// `sign3` = `CMP(a,b) = (a>b) - (a<b)` (`h2656_sao_template.c:46`): −1/0/+1.
#[inline]
pub(crate) fn cmp(a: i32, b: i32) -> i32 {
    (a > b) as i32 - (a < b) as i32
}

/// `av_clip_pixel`: clip to `[0, max]`.
#[inline]
fn clip(x: i32, max: i32) -> u16 {
    x.clamp(0, max) as u16
}

/// Scalar reference for [`crate::sao_band_u16`].
///
/// Per pixel: `band = (src >> band_shift) & 31`, then
/// `dst = clip(src + offset_table[band], 0, max)`.  `offset_table` is the
/// caller-built 32-entry band LUT (`band_shift = bit_depth - 5`; only the
/// ≤4-entry band window is non-zero, the rest 0).  Strides are in **u16
/// elements**.
///
/// # Safety
/// `src`/`dst` valid for the `w × h` region at the given strides;
/// `offset_table` valid for 32 `i16` reads; `band_shift < 16`.
pub(crate) unsafe fn sao_band_u16(
    dst: *mut u16,
    dst_stride: isize,
    src: *const u16,
    src_stride: isize,
    w: usize,
    h: usize,
    offset_table: *const i16,
    band_shift: u32,
    max: u16,
) {
    unsafe {
        let maxi = max as i32;
        for y in 0..h {
            let s_row = src.offset(y as isize * src_stride);
            let d_row = dst.offset(y as isize * dst_stride);
            for x in 0..w {
                let s = *s_row.add(x) as i32;
                let band = ((s >> band_shift) & 31) as usize;
                let off = *offset_table.add(band) as i32;
                *d_row.add(x) = clip(s + off, maxi);
            }
        }
    }
}

/// Scalar reference for [`crate::sao_edge_u16`].
///
/// Per pixel: classify by comparing `src[x]` to its two `eo_class` neighbours,
/// `category = 2 + cmp(p,n0) + cmp(p,n1)` ∈ `[0,4]`, then
/// `dst = clip(src + offsets[edge_idx[category]], 0, max)`.  A pixel whose
/// neighbour is outside `[0,w) × [0,h)` keeps its `src` value.  Strides are in
/// **u16 elements**; `src` must not alias `dst` (snapshot semantics).
///
/// # Safety
/// `src`/`dst` valid for the `w × h` region at the given strides; `offsets`
/// valid for 5 `i16` reads.
pub(crate) unsafe fn sao_edge_u16(
    dst: *mut u16,
    dst_stride: isize,
    src: *const u16,
    src_stride: isize,
    w: usize,
    h: usize,
    eo_class: u32,
    offsets: *const i16,
    max: u16,
) {
    unsafe {
        let maxi = max as i32;
        let eo = (eo_class as usize).min(3);
        let (dx0, dy0) = POS[eo][0];
        let (dx1, dy1) = POS[eo][1];
        let wi = w as i32;
        let hi = h as i32;
        for y in 0..h {
            let s_row = src.offset(y as isize * src_stride);
            let d_row = dst.offset(y as isize * dst_stride);
            for x in 0..w {
                let s = *s_row.add(x) as i32;
                let (xi, yi) = (x as i32, y as i32);
                let (n0x, n0y) = (xi + dx0, yi + dy0);
                let (n1x, n1y) = (xi + dx1, yi + dy1);
                let off_region = n0x < 0
                    || n0y < 0
                    || n1x < 0
                    || n1y < 0
                    || n0x >= wi
                    || n0y >= hi
                    || n1x >= wi
                    || n1y >= hi;
                let v = if off_region {
                    s as u16
                } else {
                    let s0 = *src.offset(n0y as isize * src_stride).add(n0x as usize) as i32;
                    let s1 = *src.offset(n1y as isize * src_stride).add(n1x as usize) as i32;
                    let cat = (2 + cmp(s, s0) + cmp(s, s1)) as usize;
                    let offv = *offsets.add(EDGE_IDX[cat]) as i32;
                    clip(s + offv, maxi)
                };
                *d_row.add(x) = v;
            }
        }
    }
}
