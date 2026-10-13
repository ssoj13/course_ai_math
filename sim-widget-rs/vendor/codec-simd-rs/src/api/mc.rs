//! Motion-compensation family — bi-prediction averaging, separable N-tap
//! sub-pel filters (u8 and i16-intermediate), and explicit weighted
//! bi-prediction (u8 / u16).

use crate::dispatch;

/// Per pixel: `dst[i] = ((a[i] as u16 + b[i] as u16 + 1) >> 1) as u8`.
///
/// Rounding halving add: average of two u8 blocks with round-to-nearest,
/// ties away from zero (matches `pavgb` / `vrhaddq` / `u8x16_avgr_u`
/// semantics across all five backends).
///
/// The primary B-frame bi-prediction primitive: after forward and
/// backward motion compensation produce two reference blocks, this fn
/// averages them into the bipred output.  Called for every bipred
/// macroblock in every codec since H.261.
///
/// Single-instruction on all five backends (x86 `_mm_avg_epu8`, NEON
/// `vrhaddq_u8`, wasm `u8x16_avgr_u`).
///
/// # Panics
/// If slices are too short for the block geometry.
pub fn mc_avg_u8(
    a: &[u8],
    a_stride: isize,
    b: &[u8],
    b_stride: isize,
    dst: &mut [u8],
    dst_stride: isize,
    w: usize,
    h: usize,
) {
    if h == 0 || w == 0 {
        return;
    }
    let needed_a = (h - 1) * a_stride.unsigned_abs() + w;
    assert!(
        a.len() >= needed_a,
        "codec-simd: mc_avg_u8 a too short ({} < {})",
        a.len(),
        needed_a
    );
    let needed_b = (h - 1) * b_stride.unsigned_abs() + w;
    assert!(
        b.len() >= needed_b,
        "codec-simd: mc_avg_u8 b too short ({} < {})",
        b.len(),
        needed_b
    );
    let needed_d = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_d,
        "codec-simd: mc_avg_u8 dst too short ({} < {})",
        dst.len(),
        needed_d
    );
    unsafe {
        (dispatch::get().mc_avg_u8)(
            a.as_ptr(),
            a_stride,
            b.as_ptr(),
            b_stride,
            dst.as_mut_ptr(),
            dst_stride,
            w,
            h,
        );
    }
}

// =========================================================================
// 8-tap horizontal motion compensation (v0.3.0).
// =========================================================================

/// Horizontal 8-tap separable motion-compensation filter, u8 → u8.
///
/// For each output pixel:
/// `dst[r][c] = clip_u8((Σ_{i=0..8} src[r][c+i] * filter[i] + 64) >> 7)`.
///
/// `filter` is the 8-tap kernel (e.g. a VP9/AV1 sub-pel filter whose taps
/// sum to 128).  The fixed `>> 7` matches that normalization; the result
/// is rounded (`+64`) and clamped to `[0, 255]`.
///
/// **Edge convention:** `src` must already be offset so that tap 0 aligns
/// with output column 0 — i.e. the caller passes `src_origin - 3` for the
/// centered half-pel position.  Each output column `c` reads `src[c..c+8)`,
/// so every source row must have at least `w + 7` readable bytes from the
/// passed pointer.  Padding the reference plane is the caller's
/// responsibility (standard MC convention, as in libvpx / dav1d).
///
/// Together with its vertical sibling this is the hottest decoder kernel
/// in VP9/AV1/HEVC (≈ 30-40% of decode time).
///
/// # Panics
/// If `src.len() < (h-1)*|src_stride| + w + 7`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_8tap_h_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 8],
) {
    if w == 0 || h == 0 {
        return;
    }
    let needed_src = (h - 1) * src_stride.unsigned_abs() + w + 7;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_h_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_h_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 8 i16.
    unsafe {
        (dispatch::get().mc_8tap_h_u8)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
        );
    }
}

/// Vertical 8-tap separable motion-compensation filter, u8 → u8.
///
/// The vertical sibling of [`mc_8tap_h_u8`].  For each output pixel:
/// `dst[r][c] = clip_u8((Σ_{i=0..8} src[r+i][c] * filter[i] + 64) >> 7)`.
///
/// **Edge convention:** `src` must already be offset so that tap 0 aligns
/// with output row 0 — i.e. the caller passes `src_origin - 3*src_stride`
/// for the centered half-pel position.  Output row `r` reads source rows
/// `r..r+8`, so the source plane must have at least `h + 7` rows readable
/// from the passed pointer (columns need only `w`).  Padding the
/// reference plane is the caller's responsibility (libvpx / dav1d
/// convention).
///
/// Together with [`mc_8tap_h_u8`] this forms the separable luma MC pair;
/// the 2D combiner (`mc_8tap_2d_u8`) chains the two through an i16
/// intermediate.
///
/// # Panics
/// If `src.len() < (h+6)*|src_stride| + w`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_8tap_v_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 8],
) {
    if w == 0 || h == 0 {
        return;
    }
    // Output row r reads source rows r..r+8, so the last byte touched is
    // ((h-1)+7)*|stride| + (w-1) → need (h+6)*|stride| + w bytes.
    let needed_src = (h + 6) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_v_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_v_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 8 i16.
    unsafe {
        (dispatch::get().mc_8tap_v_u8)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
        );
    }
}

/// Separable 2D 8-tap motion compensation (horizontal then vertical),
/// u8 → u8, with a u8 intermediate (the libvpx `convolve8` convention).
///
/// `out[r][c] = Vfilter( Hfilter(src) )` — the horizontal pass filters
/// `h + 7` rows into a u8 scratch plane (each value rounded `+64`,
/// shifted `>> 7` and clipped to `[0,255]`), then the vertical pass
/// filters that scratch into the final `w × h` output.  Bit-identical to
/// running [`mc_8tap_h_u8`] then [`mc_8tap_v_u8`] — which is exactly what
/// it does, so there is no separate SIMD path: the work happens inside
/// the two dispatched kernels.
///
/// **Edge convention:** `src` is pre-offset by `-3*src_stride - 3` (tap 0
/// aligns with output `[0][0]`).  The horizontal pass needs `w + 7`
/// readable columns and the result feeds 8 vertical taps, so the source
/// plane must expose `h + 7` rows × `w + 7` columns from the pointer.
///
/// This is a convenience wrapper: callers wanting to avoid the per-call
/// scratch allocation should compose [`mc_8tap_h_u8`] and
/// [`mc_8tap_v_u8`] directly with their own reusable buffer.  Higher-
/// precision (i16-intermediate) 2D MC — the AV1/HEVC convention — is a
/// separate future primitive with its own rounding contract.
///
/// # Panics
/// If `src.len() < (h+6)*|src_stride| + w + 7`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_8tap_2d_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter_h: &[i16; 8],
    filter_v: &[i16; 8],
) {
    if w == 0 || h == 0 {
        return;
    }
    // Horizontal pass reads w+7 cols across h+7 rows.
    let needed_src = (h + 6) * src_stride.unsigned_abs() + w + 7;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_2d_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_2d_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // u8 intermediate: h+7 horizontally-filtered rows, tightly packed.
    let temp_rows = h + 7;
    let mut temp = vec![0u8; temp_rows * w];
    mc_8tap_h_u8(
        &mut temp, w as isize, src, src_stride, w, temp_rows, filter_h,
    );
    // Vertical pass consumes the scratch (its tap-0 row is scratch row 0).
    mc_8tap_v_u8(dst, dst_stride, &temp, w as isize, w, h, filter_v);
}

// =========================================================================
// 4-tap chroma motion compensation (v0.3.3).
// =========================================================================

/// Horizontal 4-tap chroma motion-compensation filter, u8 → u8, with a
/// runtime normalization `shift`.
///
/// For each output pixel:
/// `dst[r][c] = clip_u8((Σ_{i=0..4} src[r][c+i] * filter[i] + (1<<(shift-1))) >> shift)`.
///
/// Unlike the luma 8-tap kernels (which hard-code the VP9/AV1 `>> 7`
/// normalization), the chroma shift varies by codec — HEVC chroma uses
/// `shift = 6` (taps sum to 64), other configurations use `7` — so it is
/// a parameter.  `shift` must be in `1..=14`.
///
/// **Edge convention:** `src` is pre-offset so tap 0 aligns with output
/// column 0 (caller passes `src_origin - 1`).  Each output column `c`
/// reads `src[c..c+4)`, so a source row needs `w + 3` readable bytes from
/// the pointer.
///
/// # Panics
/// If `shift` is not in `1..=14`, `src.len() < (h-1)*|src_stride| + w + 3`,
/// or `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_4tap_h_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 4],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        (1..=14).contains(&shift),
        "codec-simd: mc_4tap_h_u8 shift must be 1..=14 (got {shift})"
    );
    let needed_src = (h - 1) * src_stride.unsigned_abs() + w + 3;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_4tap_h_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_4tap_h_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 4 i16.
    unsafe {
        (dispatch::get().mc_4tap_h_u8)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// Vertical 4-tap chroma motion-compensation filter, u8 → u8, with a
/// runtime normalization `shift`.  The vertical sibling of
/// [`mc_4tap_h_u8`].
///
/// `dst[r][c] = clip_u8((Σ_{i=0..4} src[r+i][c] * filter[i] + (1<<(shift-1))) >> shift)`.
///
/// **Edge convention:** `src` is pre-offset by `-1*src_stride`; output
/// row `r` reads source rows `r..r+4`, so the plane needs `h + 3` rows
/// (columns only `w`).  `shift` must be in `1..=14`.
///
/// # Panics
/// If `shift` is not in `1..=14`, `src.len() < (h+2)*|src_stride| + w`,
/// or `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_4tap_v_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 4],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        (1..=14).contains(&shift),
        "codec-simd: mc_4tap_v_u8 shift must be 1..=14 (got {shift})"
    );
    let needed_src = (h + 2) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_4tap_v_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_4tap_v_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 4 i16.
    unsafe {
        (dispatch::get().mc_4tap_v_u8)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

// =========================================================================
// 6-tap luma motion compensation (H.264) (v0.3.4).
// =========================================================================

/// Horizontal 6-tap luma motion-compensation filter, u8 → u8, with a
/// runtime normalization `shift`.
///
/// For each output pixel:
/// `dst[r][c] = clip_u8((Σ_{i=0..6} src[r][c+i] * filter[i] + (1<<(shift-1))) >> shift)`.
///
/// The H.264 luma half-pel filter is `[1, -5, 20, 20, -5, 1]` with
/// `shift = 5` (round 16) — the 1D half-sample is a single shift.  (The
/// 2D centre half-pel `j` position, which needs an unrounded i16
/// intermediate and `>> 10`, is the deferred i16-intermediate case;
/// quarter-pel positions are integer/half-pel averages via
/// [`mc_avg_u8`].)  `shift` must be in `1..=14`.
///
/// **Edge convention:** `src` is pre-offset so tap 0 aligns with output
/// column 0 (caller passes `src_origin - 2`).  Each output column `c`
/// reads `src[c..c+6)`, so a source row needs `w + 5` readable bytes.
///
/// # Panics
/// If `shift` is not in `1..=14`, `src.len() < (h-1)*|src_stride| + w + 5`,
/// or `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_6tap_h_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 6],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        (1..=14).contains(&shift),
        "codec-simd: mc_6tap_h_u8 shift must be 1..=14 (got {shift})"
    );
    let needed_src = (h - 1) * src_stride.unsigned_abs() + w + 5;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_6tap_h_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_6tap_h_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 6 i16.
    unsafe {
        (dispatch::get().mc_6tap_h_u8)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// Vertical 6-tap luma motion-compensation filter, u8 → u8, with a
/// runtime normalization `shift`.  The vertical sibling of
/// [`mc_6tap_h_u8`].
///
/// `dst[r][c] = clip_u8((Σ_{i=0..6} src[r+i][c] * filter[i] + (1<<(shift-1))) >> shift)`.
///
/// **Edge convention:** `src` is pre-offset by `-2*src_stride`; output
/// row `r` reads source rows `r..r+6`, so the plane needs `h + 5` rows
/// (columns only `w`).  `shift` must be in `1..=14`.
///
/// # Panics
/// If `shift` is not in `1..=14`, `src.len() < (h+4)*|src_stride| + w`,
/// or `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_6tap_v_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 6],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        (1..=14).contains(&shift),
        "codec-simd: mc_6tap_v_u8 shift must be 1..=14 (got {shift})"
    );
    let needed_src = (h + 4) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_6tap_v_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_6tap_v_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 6 i16.
    unsafe {
        (dispatch::get().mc_6tap_v_u8)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

// =========================================================================
// 2-tap bilinear motion compensation (H.264 chroma / AVIF) (v0.3.5).
// =========================================================================

/// Horizontal 2-tap bilinear motion-compensation filter, u8 → u8, with a
/// runtime normalization `shift`.
///
/// `dst[r][c] = clip_u8((filter[0]*src[r][c] + filter[1]*src[r][c+1] + (1<<(shift-1))) >> shift)`.
///
/// For H.264 chroma the caller passes `[8-frac, frac]` with `shift = 3`.
/// `shift` must be in `1..=14`.
///
/// **Edge convention:** `src` aligns tap 0 with output column 0; each
/// column `c` reads `src[c..c+2)`, so a source row needs `w + 1` readable
/// bytes.
///
/// # Panics
/// If `shift` is not in `1..=14`, `src.len() < (h-1)*|src_stride| + w + 1`,
/// or `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_bilinear_h_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 2],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        (1..=14).contains(&shift),
        "codec-simd: mc_bilinear_h_u8 shift must be 1..=14 (got {shift})"
    );
    let needed_src = (h - 1) * src_stride.unsigned_abs() + w + 1;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_bilinear_h_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_bilinear_h_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 2 i16.
    unsafe {
        (dispatch::get().mc_bilinear_h_u8)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// Vertical 2-tap bilinear motion-compensation filter, u8 → u8, with a
/// runtime normalization `shift`.  The vertical sibling of
/// [`mc_bilinear_h_u8`].
///
/// `dst[r][c] = clip_u8((filter[0]*src[r][c] + filter[1]*src[r+1][c] + (1<<(shift-1))) >> shift)`.
///
/// **Edge convention:** output row `r` reads source rows `r..r+2`, so the
/// plane needs `h + 1` rows (columns only `w`).  `shift` in `1..=14`.
///
/// # Panics
/// If `shift` is not in `1..=14`, `src.len() < h*|src_stride| + w`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_bilinear_v_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 2],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        (1..=14).contains(&shift),
        "codec-simd: mc_bilinear_v_u8 shift must be 1..=14 (got {shift})"
    );
    let needed_src = h * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_bilinear_v_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_bilinear_v_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 2 i16.
    unsafe {
        (dispatch::get().mc_bilinear_v_u8)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

// =========================================================================
// High-precision 2D MC — i16-intermediate building blocks (v0.3.6+).
// =========================================================================

/// First pass of high-precision 2D motion compensation: horizontal 8-tap
/// filter, **u8 → i16**, NOT clipped to pixel range.
///
/// `dst[r][c] = sat_i16((Σ_{i=0..8} src[r][c+i]·filter[i] + round) >> shift)`,
/// where `round = if shift==0 {0} else {1<<(shift-1)}`.  The result keeps
/// signed i16 precision so a vertical pass can finish the 2D filter
/// without the intermediate u8 rounding that `mc_8tap_2d_u8` incurs.
///
/// Pair with the (forthcoming) i16-input vertical pass for codec-exact 2D
/// MC: the caller chooses `shift` per codec (e.g. HEVC luma first pass
/// `BD-8`, H.264 centre `0`).  `dst_stride` is in **i16 elements**.
/// `src` follows the 8-tap edge convention (`-3` offset, `w + 7`
/// readable columns).
///
/// # Panics
/// If `shift > 14`, `src.len() < (h-1)*|src_stride| + w + 7`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_8tap_h_u8_to_i16(
    dst: &mut [i16],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 8],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_8tap_h_u8_to_i16 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h - 1) * src_stride.unsigned_abs() + w + 7;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_h_u8_to_i16 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_h_u8_to_i16 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 8 i16.
    unsafe {
        (dispatch::get().mc_8tap_h_u8_to_i16)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// Second pass of high-precision 2D motion compensation: vertical 8-tap
/// over an **i16 intermediate**, clipped back to **u8**.
///
/// `dst[r][c] = clip_u8((Σ_{i=0..8} src[r+i][c]·filter[i] + round) >> shift)`,
/// `round = if shift==0 {0} else {1<<(shift-1)}`.
///
/// Consumes the i16 plane produced by [`mc_8tap_h_u8_to_i16`] (its tap-0
/// row is intermediate row 0) and applies the final rounding/clip.  The
/// caller picks `shift` per codec (HEVC luma second pass `20-BD`, H.264
/// centre `10`).  `src_stride` is in **i16 elements**; output row `r`
/// reads i16 rows `r..r+8`, so the intermediate needs `h + 7` rows.
///
/// # Panics
/// If `shift > 14`, `src.len() < (h+6)*|src_stride| + w` (i16 elements),
/// or `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_8tap_v_i16_to_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[i16],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 8],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_8tap_v_i16_to_u8 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h + 6) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_v_i16_to_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_v_i16_to_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 8 i16.
    unsafe {
        (dispatch::get().mc_8tap_v_i16_to_u8)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// High-precision separable 2D 8-tap motion compensation (HEVC/AV1
/// convention), u8 → u8, with an **i16 intermediate**.
///
/// Horizontal pass keeps i16 precision
/// ([`mc_8tap_h_u8_to_i16`], shift `shift1`), then the vertical pass
/// ([`mc_8tap_v_i16_to_u8`], shift `shift2`) applies the final rounding
/// and clips to u8.  Unlike [`mc_8tap_2d_u8`] (u8 intermediate, libvpx),
/// the intermediate is not clipped to pixel range — matching HEVC/AV1.
///
/// The caller picks the per-pass shifts: HEVC luma 2D uses
/// `shift1 = BD-8`, `shift2 = 20-BD` (so `0` and `12` for 8-bit).
///
/// **Edge convention:** as [`mc_8tap_2d_u8`] — `src` pre-offset by
/// `-3*src_stride - 3`, exposing `h + 7` rows × `w + 7` columns.  A
/// per-call i16 scratch `(h+7)×w` is allocated; callers avoiding the
/// allocation should compose [`mc_8tap_h_u8_to_i16`] +
/// [`mc_8tap_v_i16_to_u8`] with their own buffer.
///
/// # Panics
/// If `shift1 > 14` or `shift2 > 14`,
/// `src.len() < (h+6)*|src_stride| + w + 7`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
#[allow(clippy::too_many_arguments)]
pub fn mc_8tap_2d_hp_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter_h: &[i16; 8],
    filter_v: &[i16; 8],
    shift1: u32,
    shift2: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift1 <= 14 && shift2 <= 14,
        "codec-simd: mc_8tap_2d_hp_u8 shifts must be 0..=14 (got {shift1}, {shift2})"
    );
    let needed_src = (h + 6) * src_stride.unsigned_abs() + w + 7;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_2d_hp_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_2d_hp_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // i16 intermediate: h+7 horizontally-filtered rows.
    let tmp_rows = h + 7;
    let mut tmp = vec![0i16; tmp_rows * w];
    mc_8tap_h_u8_to_i16(
        &mut tmp, w as isize, src, src_stride, w, tmp_rows, filter_h, shift1,
    );
    mc_8tap_v_i16_to_u8(dst, dst_stride, &tmp, w as isize, w, h, filter_v, shift2);
}

/// 4-tap i16-output horizontal pass (HEVC chroma 2D first pass).  See
/// [`mc_8tap_h_u8_to_i16`]; 4 taps, `w + 3` readable source columns.
///
/// # Panics
/// If `shift > 14`, `src.len() < (h-1)*|src_stride| + w + 3`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_4tap_h_u8_to_i16(
    dst: &mut [i16],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 4],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_4tap_h_u8_to_i16 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h - 1) * src_stride.unsigned_abs() + w + 3;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_4tap_h_u8_to_i16 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_4tap_h_u8_to_i16 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 4 i16.
    unsafe {
        (dispatch::get().mc_4tap_h_u8_to_i16)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// 4-tap i16-input vertical pass (HEVC chroma 2D second pass).  See
/// [`mc_8tap_v_i16_to_u8`]; 4 taps, `h + 3` i16 rows.
///
/// # Panics
/// If `shift > 14`, `src.len() < (h+2)*|src_stride| + w` (i16 elements),
/// or `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_4tap_v_i16_to_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[i16],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 4],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_4tap_v_i16_to_u8 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h + 2) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_4tap_v_i16_to_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_4tap_v_i16_to_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 4 i16.
    unsafe {
        (dispatch::get().mc_4tap_v_i16_to_u8)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// High-precision separable 2D 4-tap chroma motion compensation (HEVC
/// chroma), u8 → u8, with an i16 intermediate.  The 4-tap sibling of
/// [`mc_8tap_2d_hp_u8`]: horizontal i16 pass (`shift1`) then vertical
/// clip-to-u8 pass (`shift2`).
///
/// **Edge convention:** `src` pre-offset by `-1*src_stride - 1`, exposing
/// `h + 3` rows × `w + 3` columns.
///
/// # Panics
/// If `shift1 > 14` or `shift2 > 14`,
/// `src.len() < (h+2)*|src_stride| + w + 3`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
#[allow(clippy::too_many_arguments)]
pub fn mc_4tap_2d_hp_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter_h: &[i16; 4],
    filter_v: &[i16; 4],
    shift1: u32,
    shift2: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift1 <= 14 && shift2 <= 14,
        "codec-simd: mc_4tap_2d_hp_u8 shifts must be 0..=14 (got {shift1}, {shift2})"
    );
    let needed_src = (h + 2) * src_stride.unsigned_abs() + w + 3;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_4tap_2d_hp_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_4tap_2d_hp_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    let tmp_rows = h + 3;
    let mut tmp = vec![0i16; tmp_rows * w];
    mc_4tap_h_u8_to_i16(
        &mut tmp, w as isize, src, src_stride, w, tmp_rows, filter_h, shift1,
    );
    mc_4tap_v_i16_to_u8(dst, dst_stride, &tmp, w as isize, w, h, filter_v, shift2);
}

/// 6-tap i16-output horizontal pass (H.264 centre 2D first pass).  See
/// [`mc_8tap_h_u8_to_i16`]; 6 taps, `w + 5` readable source columns.
///
/// # Panics
/// If `shift > 14`, `src.len() < (h-1)*|src_stride| + w + 5`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_6tap_h_u8_to_i16(
    dst: &mut [i16],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 6],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_6tap_h_u8_to_i16 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h - 1) * src_stride.unsigned_abs() + w + 5;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_6tap_h_u8_to_i16 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_6tap_h_u8_to_i16 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 6 i16.
    unsafe {
        (dispatch::get().mc_6tap_h_u8_to_i16)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// 6-tap i16-input vertical pass (H.264 centre 2D second pass).  See
/// [`mc_8tap_v_i16_to_u8`]; 6 taps, `h + 5` i16 rows.
///
/// # Panics
/// If `shift > 14`, `src.len() < (h+4)*|src_stride| + w` (i16 elements),
/// or `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_6tap_v_i16_to_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[i16],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i16; 6],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_6tap_v_i16_to_u8 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h + 4) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_6tap_v_i16_to_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_6tap_v_i16_to_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 6 i16.
    unsafe {
        (dispatch::get().mc_6tap_v_i16_to_u8)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// High-precision separable 2D 6-tap luma motion compensation — the
/// H.264 centre half-pel `j` position (`shift1 = 0`, `shift2 = 10`).
/// The 6-tap sibling of [`mc_8tap_2d_hp_u8`].
///
/// **Edge convention:** `src` pre-offset by `-2*src_stride - 2`, exposing
/// `h + 5` rows × `w + 5` columns.
///
/// # Panics
/// If `shift1 > 14` or `shift2 > 14`,
/// `src.len() < (h+4)*|src_stride| + w + 5`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
#[allow(clippy::too_many_arguments)]
pub fn mc_6tap_2d_hp_u8(
    dst: &mut [u8],
    dst_stride: isize,
    src: &[u8],
    src_stride: isize,
    w: usize,
    h: usize,
    filter_h: &[i16; 6],
    filter_v: &[i16; 6],
    shift1: u32,
    shift2: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift1 <= 14 && shift2 <= 14,
        "codec-simd: mc_6tap_2d_hp_u8 shifts must be 0..=14 (got {shift1}, {shift2})"
    );
    let needed_src = (h + 4) * src_stride.unsigned_abs() + w + 5;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_6tap_2d_hp_u8 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_6tap_2d_hp_u8 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    let tmp_rows = h + 5;
    let mut tmp = vec![0i16; tmp_rows * w];
    mc_6tap_h_u8_to_i16(
        &mut tmp, w as isize, src, src_stride, w, tmp_rows, filter_h, shift1,
    );
    mc_6tap_v_i16_to_u8(dst, dst_stride, &tmp, w as isize, w, h, filter_v, shift2);
}

// =========================================================================
// Explicit weighted bi-prediction (v0.3.13).
// =========================================================================

/// Explicit weighted bi-prediction blend, u8 → u8:
/// `dst[i] = clip_u8((w0*a[i] + w1*b[i] + offset) >> shift)`.
///
/// The generic affine blend behind H.264 / HEVC explicit weighted
/// prediction.  The caller supplies `w0, w1, offset, shift` from the
/// slice's weighted-prediction parameters (folding the codec's
/// pre/post-shift rounding into `offset` as needed) — nothing is
/// hard-coded.  For the unweighted average use [`mc_avg_u8`].
///
/// # Panics
/// If `shift > 31`, or any of `a`/`b`/`dst` is too short for the
/// `w × h` block geometry.
#[allow(clippy::too_many_arguments)]
pub fn weighted_avg_u8(
    a: &[u8],
    a_stride: isize,
    b: &[u8],
    b_stride: isize,
    dst: &mut [u8],
    dst_stride: isize,
    w: usize,
    h: usize,
    w0: i16,
    w1: i16,
    offset: i32,
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 31,
        "codec-simd: weighted_avg_u8 shift must be 0..=31 (got {shift})"
    );
    let na = (h - 1) * a_stride.unsigned_abs() + w;
    assert!(
        a.len() >= na,
        "codec-simd: weighted_avg_u8 a too short ({} < {})",
        a.len(),
        na
    );
    let nb = (h - 1) * b_stride.unsigned_abs() + w;
    assert!(
        b.len() >= nb,
        "codec-simd: weighted_avg_u8 b too short ({} < {})",
        b.len(),
        nb
    );
    let nd = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= nd,
        "codec-simd: weighted_avg_u8 dst too short ({} < {})",
        dst.len(),
        nd
    );
    // SAFETY: geometry verified above.
    unsafe {
        (dispatch::get().weighted_avg_u8)(
            a.as_ptr(),
            a_stride,
            b.as_ptr(),
            b_stride,
            dst.as_mut_ptr(),
            dst_stride,
            w,
            h,
            w0,
            w1,
            offset,
            shift,
        );
    }
}

// =========================================================================
// High-bit-depth bi-prediction average (v0.3.17).
// =========================================================================

/// Rounding halving add of two `u16` blocks:
/// `dst[i] = (a[i] + b[i] + 1) >> 1`.  The high-bit-depth (10/12-bit)
/// sibling of [`mc_avg_u8`] —
/// B-frame bi-prediction on `u16` sample planes.  Strides are in **u16
/// elements**.
///
/// # Panics
/// If any of `a`/`b`/`dst` is too short for the `w × h` block geometry.
pub fn mc_avg_u16(
    a: &[u16],
    a_stride: isize,
    b: &[u16],
    b_stride: isize,
    dst: &mut [u16],
    dst_stride: isize,
    w: usize,
    h: usize,
) {
    if w == 0 || h == 0 {
        return;
    }
    let na = (h - 1) * a_stride.unsigned_abs() + w;
    assert!(
        a.len() >= na,
        "codec-simd: mc_avg_u16 a too short ({} < {})",
        a.len(),
        na
    );
    let nb = (h - 1) * b_stride.unsigned_abs() + w;
    assert!(
        b.len() >= nb,
        "codec-simd: mc_avg_u16 b too short ({} < {})",
        b.len(),
        nb
    );
    let nd = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= nd,
        "codec-simd: mc_avg_u16 dst too short ({} < {})",
        dst.len(),
        nd
    );
    // SAFETY: geometry verified above.
    unsafe {
        (dispatch::get().mc_avg_u16)(
            a.as_ptr(),
            a_stride,
            b.as_ptr(),
            b_stride,
            dst.as_mut_ptr(),
            dst_stride,
            w,
            h,
        );
    }
}

/// Explicit weighted bi-prediction on `u16` blocks:
/// `dst[i] = clamp((w0*a[i] + w1*b[i] + offset) >> shift, 0, max)`.
///
/// The high-bit-depth sibling of [`weighted_avg_u8`]; `max = (1<<bd)-1`
/// is the pixel ceiling for the bit depth.  The caller supplies the
/// weighted-prediction parameters.  Strides are in u16 elements.  (The
/// `i32` accumulator assumes realistic weights — `|w0*a| + |w1*b|` within
/// `i32`, true for codec weighted prediction.)
///
/// SIMD on SSE4.1+/AVX2; SSE2 / NEON / wasm use the scalar fallback (a
/// SIMD backfill is roadmapped).
///
/// # Panics
/// If `shift > 31`, or any of `a`/`b`/`dst` is too short for the
/// `w × h` block geometry.
#[allow(clippy::too_many_arguments)]
pub fn weighted_avg_u16(
    a: &[u16],
    a_stride: isize,
    b: &[u16],
    b_stride: isize,
    dst: &mut [u16],
    dst_stride: isize,
    w: usize,
    h: usize,
    w0: i16,
    w1: i16,
    offset: i32,
    shift: u32,
    max: u16,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 31,
        "codec-simd: weighted_avg_u16 shift must be 0..=31 (got {shift})"
    );
    let na = (h - 1) * a_stride.unsigned_abs() + w;
    assert!(
        a.len() >= na,
        "codec-simd: weighted_avg_u16 a too short ({} < {})",
        a.len(),
        na
    );
    let nb = (h - 1) * b_stride.unsigned_abs() + w;
    assert!(
        b.len() >= nb,
        "codec-simd: weighted_avg_u16 b too short ({} < {})",
        b.len(),
        nb
    );
    let nd = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= nd,
        "codec-simd: weighted_avg_u16 dst too short ({} < {})",
        dst.len(),
        nd
    );
    // SAFETY: geometry verified above.
    unsafe {
        (dispatch::get().weighted_avg_u16)(
            a.as_ptr(),
            a_stride,
            b.as_ptr(),
            b_stride,
            dst.as_mut_ptr(),
            dst_stride,
            w,
            h,
            w0,
            w1,
            offset,
            shift,
            max,
        );
    }
}

// =========================================================================
// HEVC luma 8-tap MC, i32-source → raw i32 intermediate (v0.3.22).
//
// Byte-exact port of ffmpeg-rs `av-codec-h265/mc.rs` (`luma_h_sample` /
// `luma_filter` / `luma_filter_tmp` / `fill_luma_htmp`).  These are the tap
// filters BEFORE any pixel narrowing: the pixel source is carried as i32 (u16
// pixels pre-widened, in `0..(1<<bit_depth)`) and the output is the RAW i32 MC
// intermediate — NO narrow, NO saturation, NO rounding offset.  Downstream the
// SAME i32 output feeds two separate steps: `put_luma` truncates it to
// `int16_t` (WRAPPING, not saturating — mc.rs:474) when storing bi's src2,
// while `put_luma_uni/_bi/_uni_w/_bi_w` keep the full i32 and `clip_pixel`.
// Because those two consumers disagree (wrap vs full-i32), the tap filters
// MUST NOT narrow — they emit i32 and let each consumer finish.  h/v-only is
// `Σ f[k]·src[k] >> (bit_depth-8)`; the hv second pass is `>> 6` (and CAN exceed
// i16, e.g. 33271).
// =========================================================================

/// HEVC intermediate/temp-buffer stride, `MAX_PB_SIZE` (largest HEVC
/// prediction block is 64×64 luma).  The hv scratch (`htmp`) supplied to
/// [`mc_8tap_2d_hp_i32_to_i32`] is strided by this.
pub const MC_MAX_PB_SIZE: usize = 64;

/// Extra luma rows the hv horizontal pass must produce
/// (`QPEL_EXTRA = QPEL_EXTRA_BEFORE(3) + QPEL_EXTRA_AFTER(4)`).  The hv scratch
/// needs `height + MC_LUMA_EXTRA` rows.
pub const MC_LUMA_EXTRA: usize = 7;

/// Horizontal 8-tap HEVC luma sub-pel filter, **i32 source → raw i32**
/// intermediate, NO narrow, NO saturation, NO rounding offset.
///
/// For each output pixel:
/// `dst[r][c] = (Σ_{i=0..8} src[r][c+i]·filter[i]) >> shift` (i32, unsaturated).
///
/// This is the h-only (or, transposed, v-only) HEVC tap filter (ffmpeg-rs
/// `luma_h_sample`): a pure arithmetic right shift with **no** rounding bias,
/// unlike the VP9/AV1 `mc_8tap_h_u8` (`+64 >> 7`).  The caller passes
/// `shift = bit_depth - 8` (0/2/4 for 8/10/12-bit).  The output is the RAW i32
/// intermediate: the wrapping-i16 `put` store and the uni/bi clip-to-pixel are
/// SEPARATE downstream steps, not this kernel's job.  For the h/v-only
/// positions the value provably lands in `[-6142, 22522]` (⊆ i16) over HEVC
/// qpel filters, but it is still emitted as i32 so one output serves both
/// consumers.  `filter` is the 8-tap `QPEL_FILTERS` row (i8 coefficients).
///
/// **Source contract (SSE2 backend):** the i32 `src` samples must be pixels in
/// `0..(1<<bit_depth)` (⊆ i16); the SSE2 path narrows the source to i16 before
/// the widening multiply, which is lossless only under that contract.
///
/// **Edge convention:** as with the crate's other MC kernels, `src` is
/// pre-offset so tap 0 aligns with output column 0 (caller passes
/// `src_origin - 3`).  Each output column reads `src[c..c+8)`, so a source
/// row needs `w + 7` readable i32 samples.
///
/// # Panics
/// If `shift > 14`, `src.len() < (h-1)*|src_stride| + w + 7`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_8tap_h_i32_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[i32],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i8; 8],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_8tap_h_i32_to_i32 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h - 1) * src_stride.unsigned_abs() + w + 7;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_h_i32_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_h_i32_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 8 i8.
    unsafe {
        (dispatch::get().mc_8tap_h_i32_to_i32)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// Vertical 8-tap HEVC luma sub-pel filter, **i32 source → raw i32**
/// intermediate, NO narrow, NO saturation, NO rounding offset.  The vertical
/// sibling of [`mc_8tap_h_i32_to_i32`].
///
/// `dst[r][c] = (Σ_{i=0..8} src[r+i][c]·filter[i]) >> shift` (i32, unsaturated).
///
/// **Source contract (SSE2 backend):** i32 `src` samples must be pixels in
/// `0..(1<<bit_depth)` (⊆ i16) — the SSE2 path narrows the source to i16 before
/// the widening multiply.
///
/// **Edge convention:** `src` is pre-offset by `-3*src_stride`; output row
/// `r` reads source rows `r..r+8`, so the plane needs `h + 7` rows.
///
/// # Panics
/// If `shift > 14`, `src.len() < (h+6)*|src_stride| + w`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_8tap_v_i32_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[i32],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i8; 8],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_8tap_v_i32_to_i32 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h + 6) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_v_i32_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_v_i32_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 8 i8.
    unsafe {
        (dispatch::get().mc_8tap_v_i32_to_i32)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// Vertical 8-tap HEVC luma filter over the **i16 hv intermediate** (`htmp`),
/// i16 → **raw i32**, NO narrow, NO saturation, NO rounding offset — the second
/// pass of the HEVC hv two-pass.
///
/// `dst[r][c] = (Σ_{i=0..8} src[r+i][c]·filter[i]) >> shift` (i32, unsaturated).
///
/// Consumes the i16 `htmp` plane produced by the horizontal pass
/// ([`mc_8tap_h_i32_to_i32`], narrowed to the in-range i16 `htmp`) and applies
/// the vertical qpel filter with `shift = 6` (ffmpeg-rs `luma_filter_tmp >> 6`).
/// This second-pass output CAN exceed i16 (e.g. 33271 for a half-pel filter
/// over extreme in-range `htmp`), so it MUST stay i32: `put_luma` truncates it
/// to `int16_t` (wrapping) itself when storing bi's src2, while the uni/bi
/// producers keep the full i32 for `clip_pixel`.  `src_stride` is in i16
/// elements; output row `r` reads i16 rows `r..r+8`, so the intermediate
/// needs `h + 7` rows.
///
/// # Panics
/// If `shift > 14`, `src.len() < (h+6)*|src_stride| + w` (i16 elements), or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_8tap_v_i16_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[i16],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i8; 8],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_8tap_v_i16_to_i32 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h + 6) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_v_i16_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_v_i16_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 8 i8.
    unsafe {
        (dispatch::get().mc_8tap_v_i16_to_i32)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// High-precision separable 2D 8-tap HEVC luma motion compensation, i32
/// pixels → **raw i32** output, with an **i16 `htmp`** intermediate and a
/// **caller-provided** `htmp` scratch (no internal allocation — mirrors
/// ffmpeg-rs's per-CTU scratch reuse).
///
/// Reproduces ffmpeg-rs `put_luma`'s hv path:
/// 1. Horizontal pass (the [`mc_8tap_h_i32_to_i32`] tap filter, `shift =
///    shift_h`) fills the first `h + MC_LUMA_EXTRA` rows of `htmp` (stride
///    `MC_MAX_PB_SIZE`).  The htmp values are provably in `[-6142, 22522] ⊆
///    i16` for the HEVC qpel filters over pixel-range input (min
///    `= -11·4095 <<0` cases bounded by `Sum |neg taps|·max` / `Sum pos
///    taps·max`), so storing them as i16 by **truncation (`as i16`, wrapping —
///    NOT saturation)** is byte-exact and matches ffmpeg-rs `fill_luma_htmp`'s
///    `int16_t` store.  `htmp` row 0 corresponds to source row `-3` (the caller
///    has pre-offset `src` by `-3*src_stride - 3`).  The i16 `htmp` also keeps
///    the vertical pass SIMD-friendly (8 i16 lanes/iter).
/// 2. Vertical pass ([`mc_8tap_v_i16_to_i32`], `shift = 6`) filters `htmp` down
///    each column into the **raw i32** output; output row `r` reads htmp rows
///    `r..r+8`, matching `fill_luma_htmp` + `luma_filter_tmp >> 6`.  This
///    output is NOT narrowed/saturated: `put_luma` truncates it to `int16_t`
///    itself, the uni/bi producers `clip_pixel` from the full i32 — both are
///    separate downstream steps.
///
/// `shift_h = bit_depth - 8`.  The vertical shift is hard-wired to 6 (the
/// HEVC hv normalization).  Only `htmp` cols `0..w` of rows `0..h+7` are
/// written/read, so a reused scratch is byte-identical across calls.
///
/// **Edge convention:** `src` pre-offset by `-3*src_stride - 3`, exposing
/// `h + 7` rows × `w + 7` columns.  `w, h ≤ MC_MAX_PB_SIZE`.
///
/// # Panics
/// If `shift_h > 14`, `w > MC_MAX_PB_SIZE` or `h > MC_MAX_PB_SIZE`,
/// `src.len() < (h+6)*|src_stride| + w + 7`,
/// `dst.len() < (h-1)*|dst_stride| + w`, or
/// `htmp.len() < (h + MC_LUMA_EXTRA) * MC_MAX_PB_SIZE`.
#[allow(clippy::too_many_arguments)]
pub fn mc_8tap_2d_hp_i32_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[i32],
    src_stride: isize,
    w: usize,
    h: usize,
    hfilter: &[i8; 8],
    vfilter: &[i8; 8],
    shift_h: u32,
    htmp: &mut [i16],
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift_h <= 14,
        "codec-simd: mc_8tap_2d_hp_i32_to_i32 shift_h must be 0..=14 (got {shift_h})"
    );
    assert!(
        w <= MC_MAX_PB_SIZE && h <= MC_MAX_PB_SIZE,
        "codec-simd: mc_8tap_2d_hp_i32_to_i32 block {w}x{h} exceeds MC_MAX_PB_SIZE={MC_MAX_PB_SIZE}"
    );
    let needed_src = (h + 6) * src_stride.unsigned_abs() + w + 7;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_2d_hp_i32_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_2d_hp_i32_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    let needed_htmp = (h + MC_LUMA_EXTRA) * MC_MAX_PB_SIZE;
    assert!(
        htmp.len() >= needed_htmp,
        "codec-simd: mc_8tap_2d_hp_i32_to_i32 htmp too short ({} < {})",
        htmp.len(),
        needed_htmp
    );
    // Pass 1: horizontal fill of h+7 rows into the MC_MAX_PB_SIZE-strided i16
    // `htmp`.  The tap filter emits raw i32; we narrow each row into the i16
    // htmp by TRUNCATION (`as i16`, byte-exact since the value ⊆ i16 — see the
    // range note above).  A small stack scratch keeps the wrapper alloc-free.
    let hfn = dispatch::get().mc_8tap_h_i32_to_i32;
    let mut row = [0i32; MC_MAX_PB_SIZE];
    for yt in 0..(h + MC_LUMA_EXTRA) {
        // SAFETY: `src` exposes (h+6)*|src_stride| + w + 7 i32 (asserted), so
        // row `yt` (≤ h+6) has ≥ w + 7 readable samples from `src + yt*stride`;
        // `row` holds w ≤ MC_MAX_PB_SIZE i32; `hfilter` is exactly 8 i8.
        unsafe {
            hfn(
                row.as_mut_ptr(),
                0,
                src.as_ptr().offset(yt as isize * src_stride),
                0,
                w,
                1,
                hfilter.as_ptr(),
                shift_h,
            );
        }
        let base = yt * MC_MAX_PB_SIZE;
        for x in 0..w {
            htmp[base + x] = row[x] as i16;
        }
    }
    // Pass 2: vertical qpel over the i16 htmp, `>> 6`, raw i32 output.
    mc_8tap_v_i16_to_i32(
        dst,
        dst_stride,
        htmp,
        MC_MAX_PB_SIZE as isize,
        w,
        h,
        vfilter,
        6,
    );
}

// =========================================================================
// HEVC CHROMA 4-tap MC, i32-source → raw i32 intermediate (v0.3.23).
//
// Byte-exact port of ffmpeg-rs `av-codec-h265/mc.rs` (`chroma_h_sample` /
// `chroma_filter_tmp` / `fill_chroma_htmp`, `EPEL_FILTERS`).  The 4-tap sibling
// of the luma 8-tap i32 family above — same RAW-i32 contract (NO narrow, NO
// saturation, NO rounding offset; the wrapping-i16 `put` store and the uni/bi
// `clip_pixel` are separate downstream steps).  h/v-only is
// `Σ f[k]·src[k] >> (bit_depth-8)`; the hv second pass is `>> 6`.  Chroma uses
// 4 taps (vs luma 8) and the smaller `EPEL_EXTRA_BEFORE = 1` / `EPEL_EXTRA = 3`
// edge context (vs luma 3 / 7).
// =========================================================================

/// Extra chroma rows the hv horizontal pass must produce
/// (`EPEL_EXTRA = EPEL_EXTRA_BEFORE(1) + EPEL_EXTRA_AFTER(2)`).  The chroma hv
/// scratch needs `height + MC_CHROMA_EXTRA` rows.
pub const MC_CHROMA_EXTRA: usize = 3;

/// Horizontal 4-tap HEVC chroma sub-pel filter, **i32 source → raw i32**
/// intermediate, NO narrow, NO saturation, NO rounding offset.
///
/// For each output pixel:
/// `dst[r][c] = (Σ_{i=0..4} src[r][c+i]·filter[i]) >> shift` (i32, unsaturated).
///
/// This is the chroma h-only (or, transposed, v-only) HEVC tap filter
/// (ffmpeg-rs `chroma_h_sample`): a pure arithmetic right shift with **no**
/// rounding bias.  The caller passes `shift = bit_depth - 8` (0/2/4 for
/// 8/10/12-bit).  The output is the RAW i32 intermediate (the wrapping-i16
/// `put` store and the uni/bi clip-to-pixel are SEPARATE downstream steps).
/// `filter` is the 4-tap `EPEL_FILTERS` row (i8 coefficients).
///
/// **Source contract (SSE2 backend):** the i32 `src` samples must be pixels in
/// `0..(1<<bit_depth)` (⊆ i16); the SSE2 path narrows the source to i16 before
/// the widening multiply, which is lossless only under that contract.
///
/// **Edge convention:** as with the luma kernels, `src` is pre-offset so tap 0
/// aligns with output column 0 (caller passes `src_origin - 1`, the chroma
/// `EPEL_EXTRA_BEFORE`).  Each output column reads `src[c..c+4)`, so a source
/// row needs `w + 3` readable i32 samples.
///
/// # Panics
/// If `shift > 14`, `src.len() < (h-1)*|src_stride| + w + 3`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_4tap_h_i32_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[i32],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i8; 4],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_4tap_h_i32_to_i32 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h - 1) * src_stride.unsigned_abs() + w + 3;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_4tap_h_i32_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_4tap_h_i32_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 4 i8.
    unsafe {
        (dispatch::get().mc_4tap_h_i32_to_i32)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// Vertical 4-tap HEVC chroma sub-pel filter, **i32 source → raw i32**
/// intermediate, NO narrow, NO saturation, NO rounding offset.  The vertical
/// sibling of [`mc_4tap_h_i32_to_i32`].
///
/// `dst[r][c] = (Σ_{i=0..4} src[r+i][c]·filter[i]) >> shift` (i32, unsaturated).
///
/// **Source contract (SSE2 backend):** i32 `src` samples must be pixels in
/// `0..(1<<bit_depth)` (⊆ i16) — the SSE2 path narrows the source to i16 before
/// the widening multiply.
///
/// **Edge convention:** `src` is pre-offset by `-1*src_stride`; output row `r`
/// reads source rows `r..r+4`, so the plane needs `h + 3` rows.
///
/// # Panics
/// If `shift > 14`, `src.len() < (h+2)*|src_stride| + w`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_4tap_v_i32_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[i32],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i8; 4],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_4tap_v_i32_to_i32 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h + 2) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_4tap_v_i32_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_4tap_v_i32_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 4 i8.
    unsafe {
        (dispatch::get().mc_4tap_v_i32_to_i32)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// Vertical 4-tap HEVC chroma filter over the **i16 hv intermediate** (`htmp`),
/// i16 → **raw i32**, NO narrow, NO saturation, NO rounding offset — the second
/// pass of the HEVC chroma hv two-pass.
///
/// `dst[r][c] = (Σ_{i=0..4} src[r+i][c]·filter[i]) >> shift` (i32, unsaturated).
///
/// Consumes the i16 `htmp` plane produced by the horizontal pass
/// ([`mc_4tap_h_i32_to_i32`], narrowed to the in-range i16 `htmp`) and applies
/// the vertical epel filter with `shift = 6` (ffmpeg-rs `chroma_filter_tmp >> 6`).
/// For real HEVC decode the chroma `htmp` lies in `[-2560, 18939] ⊆ i16` and
/// this second-pass output stays in i16 range, but the primitive is still RAW
/// i32 (a directed adversarial test drives it past `i16::MAX`): `put_chroma`
/// truncates it to `int16_t` (wrapping) itself, the uni/bi producers keep the
/// full i32 for `clip_pixel`.  `src_stride` is in i16 elements; output row `r`
/// reads i16 rows `r..r+4`, so the intermediate needs `h + 3` rows.
///
/// # Panics
/// If `shift > 14`, `src.len() < (h+2)*|src_stride| + w` (i16 elements), or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_4tap_v_i16_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[i16],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i8; 4],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_4tap_v_i16_to_i32 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h + 2) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_4tap_v_i16_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_4tap_v_i16_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 4 i8.
    unsafe {
        (dispatch::get().mc_4tap_v_i16_to_i32)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// High-precision separable 2D 4-tap HEVC chroma motion compensation, i32
/// pixels → **raw i32** output, with an **i16 `htmp`** intermediate and a
/// **caller-provided** `htmp` scratch (no internal allocation).  The 4-tap
/// sibling of [`mc_8tap_2d_hp_i32_to_i32`].
///
/// Reproduces ffmpeg-rs `put_chroma`'s hv path:
/// 1. Horizontal pass (the [`mc_4tap_h_i32_to_i32`] tap filter, `shift =
///    shift_h`) fills the first `h + MC_CHROMA_EXTRA` rows of `htmp` (stride
///    `MC_MAX_PB_SIZE`).  The chroma htmp values are provably in
///    `[-2560, 18939] ⊆ i16` for the HEVC epel filters over pixel-range input,
///    so storing them as i16 by **truncation (`as i16`, wrapping — NOT
///    saturation)** is byte-exact and matches ffmpeg-rs `fill_chroma_htmp`'s
///    `int16_t` store.  `htmp` row 0 corresponds to source row `-1` (the caller
///    has pre-offset `src` by `-1*src_stride - 1`).
/// 2. Vertical pass ([`mc_4tap_v_i16_to_i32`], `shift = 6`) filters `htmp` down
///    each column into the **raw i32** output; output row `r` reads htmp rows
///    `r..r+4`, matching `fill_chroma_htmp` + `chroma_filter_tmp >> 6`.  This
///    output is NOT narrowed/saturated (`put_chroma` truncates it to `int16_t`
///    itself, the uni/bi producers `clip_pixel` from the full i32).
///
/// `shift_h = bit_depth - 8`.  The vertical shift is hard-wired to 6 (the HEVC
/// hv normalization).  Only `htmp` cols `0..w` of rows `0..h+3` are
/// written/read, so a reused scratch is byte-identical across calls.
///
/// **Edge convention:** `src` pre-offset by `-1*src_stride - 1`, exposing
/// `h + 3` rows × `w + 3` columns.  `w, h ≤ MC_MAX_PB_SIZE`.
///
/// # Panics
/// If `shift_h > 14`, `w > MC_MAX_PB_SIZE` or `h > MC_MAX_PB_SIZE`,
/// `src.len() < (h+2)*|src_stride| + w + 3`,
/// `dst.len() < (h-1)*|dst_stride| + w`, or
/// `htmp.len() < (h + MC_CHROMA_EXTRA) * MC_MAX_PB_SIZE`.
#[allow(clippy::too_many_arguments)]
pub fn mc_4tap_2d_hp_i32_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[i32],
    src_stride: isize,
    w: usize,
    h: usize,
    hfilter: &[i8; 4],
    vfilter: &[i8; 4],
    shift_h: u32,
    htmp: &mut [i16],
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift_h <= 14,
        "codec-simd: mc_4tap_2d_hp_i32_to_i32 shift_h must be 0..=14 (got {shift_h})"
    );
    assert!(
        w <= MC_MAX_PB_SIZE && h <= MC_MAX_PB_SIZE,
        "codec-simd: mc_4tap_2d_hp_i32_to_i32 block {w}x{h} exceeds MC_MAX_PB_SIZE={MC_MAX_PB_SIZE}"
    );
    let needed_src = (h + 2) * src_stride.unsigned_abs() + w + 3;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_4tap_2d_hp_i32_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_4tap_2d_hp_i32_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    let needed_htmp = (h + MC_CHROMA_EXTRA) * MC_MAX_PB_SIZE;
    assert!(
        htmp.len() >= needed_htmp,
        "codec-simd: mc_4tap_2d_hp_i32_to_i32 htmp too short ({} < {})",
        htmp.len(),
        needed_htmp
    );
    // Pass 1: horizontal fill of h+3 rows into the MC_MAX_PB_SIZE-strided i16
    // `htmp` (truncating narrow `as i16`, byte-exact since chroma htmp ⊆ i16).
    let hfn = dispatch::get().mc_4tap_h_i32_to_i32;
    let mut row = [0i32; MC_MAX_PB_SIZE];
    for yt in 0..(h + MC_CHROMA_EXTRA) {
        // SAFETY: `src` exposes (h+2)*|src_stride| + w + 3 i32 (asserted), so
        // row `yt` (≤ h+2) has ≥ w + 3 readable samples from `src + yt*stride`;
        // `row` holds w ≤ MC_MAX_PB_SIZE i32; `hfilter` is exactly 4 i8.
        unsafe {
            hfn(
                row.as_mut_ptr(),
                0,
                src.as_ptr().offset(yt as isize * src_stride),
                0,
                w,
                1,
                hfilter.as_ptr(),
                shift_h,
            );
        }
        let base = yt * MC_MAX_PB_SIZE;
        for x in 0..w {
            htmp[base + x] = row[x] as i16;
        }
    }
    // Pass 2: vertical epel over the i16 htmp, `>> 6`, raw i32 output.
    mc_4tap_v_i16_to_i32(
        dst,
        dst_stride,
        htmp,
        MC_MAX_PB_SIZE as isize,
        w,
        h,
        vfilter,
        6,
    );
}

// =========================================================================
// HEVC MC "final" clip/combine (i32 intermediate -> u16 pixel / wrapping i16)
// (v0.3.26).
//
// Byte-exact port of ffmpeg-rs `av-codec-h265/mc.rs` `put_luma_uni` /
// `put_luma_uni_w` / `put_luma_bi` / `put_luma_bi_w` (+ `put_luma`'s `as i16`
// bi-src2 store).  These consume the fully-filtered i32 intermediate `v`
// produced by the `mc_*tap_*_i32_to_i32` families and clip/combine it into the
// final pixel.  They are SHARED by luma and chroma (element-wise over the i32
// intermediate, independent of tap count).  `clip_pixel(x) = x.clamp(0, max)`
// with `max = (1<<bit_depth)-1`; the caller pre-derives `offset` / `shift` /
// `ox` / weights / `bias` exactly as the reference kernels do from
// `bit_depth` / `denom`.  All arithmetic is i32 (matches the reference `int`).
// =========================================================================

/// HEVC uni-prediction final clip: `dst = clip((v + offset) >> shift)`,
/// `clip(x) = x.clamp(0, max)`.
///
/// `v` is the fully-filtered i32 intermediate (from an `mc_*tap_*_i32_to_i32`
/// kernel); `offset`/`shift`/`max` are the reference's per-`bit_depth` values
/// (`shift = 14 - bit_depth`, `offset = 1 << (shift-1)`, `max = (1<<bd)-1`).
/// Both `v` and `dst` are `w × h` blocks with independent strides.
///
/// # Panics
/// If `v.len() < (h-1)*|v_stride| + w` or `dst.len() < (h-1)*|dst_stride| + w`.
#[allow(clippy::too_many_arguments)]
pub fn mc_uni_clip_i32_to_u16(
    dst: &mut [u16],
    dst_stride: isize,
    v: &[i32],
    v_stride: isize,
    w: usize,
    h: usize,
    offset: i32,
    shift: u32,
    max: u16,
) {
    if w == 0 || h == 0 {
        return;
    }
    let needed_v = (h - 1) * v_stride.unsigned_abs() + w;
    assert!(
        v.len() >= needed_v,
        "codec-simd: mc_uni_clip_i32_to_u16 v too short ({} < {})",
        v.len(),
        needed_v
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_uni_clip_i32_to_u16 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above.
    unsafe {
        (dispatch::get().mc_uni_clip_i32_to_u16)(
            dst.as_mut_ptr(),
            dst_stride,
            v.as_ptr(),
            v_stride,
            w,
            h,
            offset,
            shift,
            max,
        );
    }
}

/// HEVC weighted uni-prediction final clip:
/// `dst = clip(((v * wx + offset) >> shift) + ox)` (`+ ox` applied after the
/// shift, matching the reference).
///
/// # Panics
/// If `v.len() < (h-1)*|v_stride| + w` or `dst.len() < (h-1)*|dst_stride| + w`.
#[allow(clippy::too_many_arguments)]
pub fn mc_uni_w_clip_i32_to_u16(
    dst: &mut [u16],
    dst_stride: isize,
    v: &[i32],
    v_stride: isize,
    w: usize,
    h: usize,
    wx: i32,
    offset: i32,
    shift: u32,
    ox: i32,
    max: u16,
) {
    if w == 0 || h == 0 {
        return;
    }
    let needed_v = (h - 1) * v_stride.unsigned_abs() + w;
    assert!(
        v.len() >= needed_v,
        "codec-simd: mc_uni_w_clip_i32_to_u16 v too short ({} < {})",
        v.len(),
        needed_v
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_uni_w_clip_i32_to_u16 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above.
    unsafe {
        (dispatch::get().mc_uni_w_clip_i32_to_u16)(
            dst.as_mut_ptr(),
            dst_stride,
            v.as_ptr(),
            v_stride,
            w,
            h,
            wx,
            offset,
            shift,
            ox,
            max,
        );
    }
}

/// HEVC bi-prediction combine: `dst = clip((v + src2 + offset) >> shift)`,
/// where `src2` is the other list's i16 first-prediction intermediate
/// (sign-extended to i32), produced by [`mc_store_i32_to_i16`].
///
/// # Panics
/// If `v.len() < (h-1)*|v_stride| + w`, `src2.len() < (h-1)*|src2_stride| + w`,
/// or `dst.len() < (h-1)*|dst_stride| + w`.
#[allow(clippy::too_many_arguments)]
pub fn mc_bi_clip_i32_to_u16(
    dst: &mut [u16],
    dst_stride: isize,
    v: &[i32],
    v_stride: isize,
    src2: &[i16],
    src2_stride: isize,
    w: usize,
    h: usize,
    offset: i32,
    shift: u32,
    max: u16,
) {
    if w == 0 || h == 0 {
        return;
    }
    let needed_v = (h - 1) * v_stride.unsigned_abs() + w;
    assert!(
        v.len() >= needed_v,
        "codec-simd: mc_bi_clip_i32_to_u16 v too short ({} < {})",
        v.len(),
        needed_v
    );
    let needed_s2 = (h - 1) * src2_stride.unsigned_abs() + w;
    assert!(
        src2.len() >= needed_s2,
        "codec-simd: mc_bi_clip_i32_to_u16 src2 too short ({} < {})",
        src2.len(),
        needed_s2
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_bi_clip_i32_to_u16 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above.
    unsafe {
        (dispatch::get().mc_bi_clip_i32_to_u16)(
            dst.as_mut_ptr(),
            dst_stride,
            v.as_ptr(),
            v_stride,
            src2.as_ptr(),
            src2_stride,
            w,
            h,
            offset,
            shift,
            max,
        );
    }
}

/// HEVC weighted bi-prediction combine:
/// `dst = clip((v * wx1 + src2 * wx0 + bias) >> rshift)`.  The caller pre-derives
/// `rshift = log2Wd + 1` and `bias = (ox0 + ox1 + 1) * (1 << log2Wd)`.
///
/// # Panics
/// If `v.len() < (h-1)*|v_stride| + w`, `src2.len() < (h-1)*|src2_stride| + w`,
/// or `dst.len() < (h-1)*|dst_stride| + w`.
#[allow(clippy::too_many_arguments)]
pub fn mc_bi_w_clip_i32_to_u16(
    dst: &mut [u16],
    dst_stride: isize,
    v: &[i32],
    v_stride: isize,
    src2: &[i16],
    src2_stride: isize,
    w: usize,
    h: usize,
    wx0: i32,
    wx1: i32,
    bias: i32,
    rshift: u32,
    max: u16,
) {
    if w == 0 || h == 0 {
        return;
    }
    let needed_v = (h - 1) * v_stride.unsigned_abs() + w;
    assert!(
        v.len() >= needed_v,
        "codec-simd: mc_bi_w_clip_i32_to_u16 v too short ({} < {})",
        v.len(),
        needed_v
    );
    let needed_s2 = (h - 1) * src2_stride.unsigned_abs() + w;
    assert!(
        src2.len() >= needed_s2,
        "codec-simd: mc_bi_w_clip_i32_to_u16 src2 too short ({} < {})",
        src2.len(),
        needed_s2
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_bi_w_clip_i32_to_u16 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above.
    unsafe {
        (dispatch::get().mc_bi_w_clip_i32_to_u16)(
            dst.as_mut_ptr(),
            dst_stride,
            v.as_ptr(),
            v_stride,
            src2.as_ptr(),
            src2_stride,
            w,
            h,
            wx0,
            wx1,
            bias,
            rshift,
            max,
        );
    }
}

/// HEVC bi-prediction src2 store: narrow the i32 intermediate `v` to i16 by
/// WRAPPING (`x as i16`, low 16 bits — NOT saturating), matching
/// `put_luma`'s `int16_t` store.  Produces the `src2` buffer consumed by
/// [`mc_bi_clip_i32_to_u16`] / [`mc_bi_w_clip_i32_to_u16`].
///
/// # Panics
/// If `v.len() < (h-1)*|v_stride| + w` or `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_store_i32_to_i16(
    dst: &mut [i16],
    dst_stride: isize,
    v: &[i32],
    v_stride: isize,
    w: usize,
    h: usize,
) {
    if w == 0 || h == 0 {
        return;
    }
    let needed_v = (h - 1) * v_stride.unsigned_abs() + w;
    assert!(
        v.len() >= needed_v,
        "codec-simd: mc_store_i32_to_i16 v too short ({} < {})",
        v.len(),
        needed_v
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_store_i32_to_i16 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above.
    unsafe {
        (dispatch::get().mc_store_i32_to_i16)(
            dst.as_mut_ptr(),
            dst_stride,
            v.as_ptr(),
            v_stride,
            w,
            h,
        );
    }
}

// =========================================================================
// HEVC MC, u16-SOURCE → raw i32 intermediate (v0.3.27).
//
// The u16 siblings of the `mc_*tap_*_i32_to_i32` luma/chroma tap filters above,
// for the ffmpeg-rs MC path that gathers reference pixels as **u16** (2 bytes)
// instead of i32 (4 bytes) so the SIMD load packs 2× the lanes per register.
// The math is IDENTICAL: a u16 pixel (`0..(1<<bit_depth)` ⊆ i16) is widened to
// i32 and fed to the SAME 8-tap/4-tap MAC + `>> shift` — so each kernel is
// BYTE-IDENTICAL to its i32-source twin given the same pixel values.  Only the
// SOURCE-reading passes are new; the i16-`htmp` hv 2nd pass
// ([`mc_8tap_v_i16_to_i32`] / [`mc_4tap_v_i16_to_i32`]) and the final
// clip/combine kernels are SHARED (reused, not duplicated).  The hv wrappers
// therefore run the u16 h-pass into the SAME in-range i16 `htmp`
// (`[-6142, 22522]` luma / `[-2560, 18939]` chroma ⊆ i16, truncation-narrowed
// byte-exact) and then call the existing `>> 6` i16 second pass.
// =========================================================================

/// Horizontal 8-tap HEVC luma sub-pel filter, **u16 source → raw i32**
/// intermediate.  The u16-source sibling of [`mc_8tap_h_i32_to_i32`]: the pixel
/// source is carried as u16 (2 bytes) instead of i32, halving the load
/// bandwidth; the arithmetic is identical (widen `u16 → i32`, then
/// `Σ f[k]·src[c+k] >> shift`, RAW i32 — NO narrow, NO saturation, NO rounding).
///
/// **Source contract:** u16 `src` samples must be pixels in `0..(1<<bit_depth)`
/// (⊆ i16).  Edge/tap convention matches [`mc_8tap_h_i32_to_i32`]: `src`
/// pre-offset by `-3`, each output column reads `src[c..c+8)` (`w + 7` samples
/// per row).
///
/// # Panics
/// If `shift > 14`, `src.len() < (h-1)*|src_stride| + w + 7`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_8tap_h_u16_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[u16],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i8; 8],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_8tap_h_u16_to_i32 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h - 1) * src_stride.unsigned_abs() + w + 7;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_h_u16_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_h_u16_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 8 i8.
    unsafe {
        (dispatch::get().mc_8tap_h_u16_to_i32)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// Vertical 8-tap HEVC luma sub-pel filter, **u16 source → raw i32**
/// intermediate.  The u16-source sibling of [`mc_8tap_v_i32_to_i32`].
///
/// **Source contract:** u16 `src` samples must be pixels in `0..(1<<bit_depth)`
/// (⊆ i16).  `src` pre-offset by `-3*src_stride`; output row `r` reads source
/// rows `r..r+8` (`h + 7` rows).
///
/// # Panics
/// If `shift > 14`, `src.len() < (h+6)*|src_stride| + w`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_8tap_v_u16_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[u16],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i8; 8],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_8tap_v_u16_to_i32 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h + 6) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_v_u16_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_v_u16_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 8 i8.
    unsafe {
        (dispatch::get().mc_8tap_v_u16_to_i32)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// High-precision separable 2D 8-tap HEVC luma motion compensation, **u16
/// pixels → raw i32** output, with an **i16 `htmp`** intermediate and a
/// caller-provided `htmp` scratch.  The u16-source sibling of
/// [`mc_8tap_2d_hp_i32_to_i32`].
///
/// 1. Horizontal pass ([`mc_8tap_h_u16_to_i32`], `shift = shift_h`) fills the
///    first `h + MC_LUMA_EXTRA` rows of `htmp` (stride `MC_MAX_PB_SIZE`),
///    truncation-narrowed `as i16` (byte-exact: the htmp values lie in
///    `[-6142, 22522] ⊆ i16`).
/// 2. Vertical pass reuses the SHARED [`mc_8tap_v_i16_to_i32`] (`shift = 6`) —
///    identical to the i32-source wrapper's 2nd pass — emitting the RAW i32
///    output (NOT narrowed/saturated).
///
/// `shift_h = bit_depth - 8`.  `src` pre-offset by `-3*src_stride - 3`,
/// exposing `h + 7` rows × `w + 7` columns.  `w, h ≤ MC_MAX_PB_SIZE`.
///
/// # Panics
/// If `shift_h > 14`, `w > MC_MAX_PB_SIZE` or `h > MC_MAX_PB_SIZE`,
/// `src.len() < (h+6)*|src_stride| + w + 7`,
/// `dst.len() < (h-1)*|dst_stride| + w`, or
/// `htmp.len() < (h + MC_LUMA_EXTRA) * MC_MAX_PB_SIZE`.
#[allow(clippy::too_many_arguments)]
pub fn mc_8tap_2d_hp_u16_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[u16],
    src_stride: isize,
    w: usize,
    h: usize,
    hfilter: &[i8; 8],
    vfilter: &[i8; 8],
    shift_h: u32,
    htmp: &mut [i16],
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift_h <= 14,
        "codec-simd: mc_8tap_2d_hp_u16_to_i32 shift_h must be 0..=14 (got {shift_h})"
    );
    assert!(
        w <= MC_MAX_PB_SIZE && h <= MC_MAX_PB_SIZE,
        "codec-simd: mc_8tap_2d_hp_u16_to_i32 block {w}x{h} exceeds MC_MAX_PB_SIZE={MC_MAX_PB_SIZE}"
    );
    let needed_src = (h + 6) * src_stride.unsigned_abs() + w + 7;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_8tap_2d_hp_u16_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_8tap_2d_hp_u16_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    let needed_htmp = (h + MC_LUMA_EXTRA) * MC_MAX_PB_SIZE;
    assert!(
        htmp.len() >= needed_htmp,
        "codec-simd: mc_8tap_2d_hp_u16_to_i32 htmp too short ({} < {})",
        htmp.len(),
        needed_htmp
    );
    // Pass 1: horizontal fill of h+7 rows into the MC_MAX_PB_SIZE-strided i16
    // `htmp` (truncating narrow `as i16`, byte-exact since htmp ⊆ i16 — same
    // range as the i32-source wrapper).
    let hfn = dispatch::get().mc_8tap_h_u16_to_i32;
    let mut row = [0i32; MC_MAX_PB_SIZE];
    for yt in 0..(h + MC_LUMA_EXTRA) {
        // SAFETY: `src` exposes (h+6)*|src_stride| + w + 7 u16 (asserted), so
        // row `yt` (≤ h+6) has ≥ w + 7 readable samples from `src + yt*stride`;
        // `row` holds w ≤ MC_MAX_PB_SIZE i32; `hfilter` is exactly 8 i8.
        unsafe {
            hfn(
                row.as_mut_ptr(),
                0,
                src.as_ptr().offset(yt as isize * src_stride),
                0,
                w,
                1,
                hfilter.as_ptr(),
                shift_h,
            );
        }
        let base = yt * MC_MAX_PB_SIZE;
        for x in 0..w {
            htmp[base + x] = row[x] as i16;
        }
    }
    // Pass 2: SHARED vertical qpel over the i16 htmp, `>> 6`, raw i32 output.
    mc_8tap_v_i16_to_i32(
        dst,
        dst_stride,
        htmp,
        MC_MAX_PB_SIZE as isize,
        w,
        h,
        vfilter,
        6,
    );
}

/// Horizontal 4-tap HEVC chroma sub-pel filter, **u16 source → raw i32**
/// intermediate.  The u16-source sibling of [`mc_4tap_h_i32_to_i32`].
///
/// **Source contract:** u16 `src` samples must be pixels in `0..(1<<bit_depth)`
/// (⊆ i16).  `src` pre-offset by `-1`; each output column reads `src[c..c+4)`
/// (`w + 3` samples per row).
///
/// # Panics
/// If `shift > 14`, `src.len() < (h-1)*|src_stride| + w + 3`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_4tap_h_u16_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[u16],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i8; 4],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_4tap_h_u16_to_i32 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h - 1) * src_stride.unsigned_abs() + w + 3;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_4tap_h_u16_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_4tap_h_u16_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 4 i8.
    unsafe {
        (dispatch::get().mc_4tap_h_u16_to_i32)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// Vertical 4-tap HEVC chroma sub-pel filter, **u16 source → raw i32**
/// intermediate.  The u16-source sibling of [`mc_4tap_v_i32_to_i32`].
///
/// **Source contract:** u16 `src` samples must be pixels in `0..(1<<bit_depth)`
/// (⊆ i16).  `src` pre-offset by `-1*src_stride`; output row `r` reads source
/// rows `r..r+4` (`h + 3` rows).
///
/// # Panics
/// If `shift > 14`, `src.len() < (h+2)*|src_stride| + w`, or
/// `dst.len() < (h-1)*|dst_stride| + w`.
pub fn mc_4tap_v_u16_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[u16],
    src_stride: isize,
    w: usize,
    h: usize,
    filter: &[i8; 4],
    shift: u32,
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift <= 14,
        "codec-simd: mc_4tap_v_u16_to_i32 shift must be 0..=14 (got {shift})"
    );
    let needed_src = (h + 2) * src_stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_4tap_v_u16_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_4tap_v_u16_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    // SAFETY: geometry verified above; `filter` is exactly 4 i8.
    unsafe {
        (dispatch::get().mc_4tap_v_u16_to_i32)(
            dst.as_mut_ptr(),
            dst_stride,
            src.as_ptr(),
            src_stride,
            w,
            h,
            filter.as_ptr(),
            shift,
        );
    }
}

/// High-precision separable 2D 4-tap HEVC chroma motion compensation, **u16
/// pixels → raw i32** output, with an **i16 `htmp`** intermediate and a
/// caller-provided `htmp` scratch.  The u16-source sibling of
/// [`mc_4tap_2d_hp_i32_to_i32`].
///
/// 1. Horizontal pass ([`mc_4tap_h_u16_to_i32`], `shift = shift_h`) fills the
///    first `h + MC_CHROMA_EXTRA` rows of `htmp`, truncation-narrowed `as i16`
///    (byte-exact: chroma htmp ⊆ `[-2560, 18939] ⊆ i16`).
/// 2. Vertical pass reuses the SHARED [`mc_4tap_v_i16_to_i32`] (`shift = 6`),
///    emitting the RAW i32 output.
///
/// `shift_h = bit_depth - 8`.  `src` pre-offset by `-1*src_stride - 1`,
/// exposing `h + 3` rows × `w + 3` columns.  `w, h ≤ MC_MAX_PB_SIZE`.
///
/// # Panics
/// If `shift_h > 14`, `w > MC_MAX_PB_SIZE` or `h > MC_MAX_PB_SIZE`,
/// `src.len() < (h+2)*|src_stride| + w + 3`,
/// `dst.len() < (h-1)*|dst_stride| + w`, or
/// `htmp.len() < (h + MC_CHROMA_EXTRA) * MC_MAX_PB_SIZE`.
#[allow(clippy::too_many_arguments)]
pub fn mc_4tap_2d_hp_u16_to_i32(
    dst: &mut [i32],
    dst_stride: isize,
    src: &[u16],
    src_stride: isize,
    w: usize,
    h: usize,
    hfilter: &[i8; 4],
    vfilter: &[i8; 4],
    shift_h: u32,
    htmp: &mut [i16],
) {
    if w == 0 || h == 0 {
        return;
    }
    assert!(
        shift_h <= 14,
        "codec-simd: mc_4tap_2d_hp_u16_to_i32 shift_h must be 0..=14 (got {shift_h})"
    );
    assert!(
        w <= MC_MAX_PB_SIZE && h <= MC_MAX_PB_SIZE,
        "codec-simd: mc_4tap_2d_hp_u16_to_i32 block {w}x{h} exceeds MC_MAX_PB_SIZE={MC_MAX_PB_SIZE}"
    );
    let needed_src = (h + 2) * src_stride.unsigned_abs() + w + 3;
    assert!(
        src.len() >= needed_src,
        "codec-simd: mc_4tap_2d_hp_u16_to_i32 src too short ({} < {})",
        src.len(),
        needed_src
    );
    let needed_dst = (h - 1) * dst_stride.unsigned_abs() + w;
    assert!(
        dst.len() >= needed_dst,
        "codec-simd: mc_4tap_2d_hp_u16_to_i32 dst too short ({} < {})",
        dst.len(),
        needed_dst
    );
    let needed_htmp = (h + MC_CHROMA_EXTRA) * MC_MAX_PB_SIZE;
    assert!(
        htmp.len() >= needed_htmp,
        "codec-simd: mc_4tap_2d_hp_u16_to_i32 htmp too short ({} < {})",
        htmp.len(),
        needed_htmp
    );
    // Pass 1: horizontal fill of h+3 rows into the MC_MAX_PB_SIZE-strided i16
    // `htmp` (truncating narrow `as i16`, byte-exact since chroma htmp ⊆ i16).
    let hfn = dispatch::get().mc_4tap_h_u16_to_i32;
    let mut row = [0i32; MC_MAX_PB_SIZE];
    for yt in 0..(h + MC_CHROMA_EXTRA) {
        // SAFETY: `src` exposes (h+2)*|src_stride| + w + 3 u16 (asserted), so
        // row `yt` (≤ h+2) has ≥ w + 3 readable samples from `src + yt*stride`;
        // `row` holds w ≤ MC_MAX_PB_SIZE i32; `hfilter` is exactly 4 i8.
        unsafe {
            hfn(
                row.as_mut_ptr(),
                0,
                src.as_ptr().offset(yt as isize * src_stride),
                0,
                w,
                1,
                hfilter.as_ptr(),
                shift_h,
            );
        }
        let base = yt * MC_MAX_PB_SIZE;
        for x in 0..w {
            htmp[base + x] = row[x] as i16;
        }
    }
    // Pass 2: SHARED vertical epel over the i16 htmp, `>> 6`, raw i32 output.
    mc_4tap_v_i16_to_i32(
        dst,
        dst_stride,
        htmp,
        MC_MAX_PB_SIZE as isize,
        w,
        h,
        vfilter,
        6,
    );
}
