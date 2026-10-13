//! Encoder cost / metric family — SAD, SATD, SSE, block variance, SAD-x4,
//! and the 4×4 Walsh-Hadamard transform.

use crate::dispatch;

// =========================================================================
// === Tier-2 block kernels (v0.2.x onward) ===
// =========================================================================

/// Sum of absolute differences across a `w × h` `u8` block.
///
/// Per-block: `sum over row ∈ 0..h, col ∈ 0..w of |a[row*a_stride+col] - b[row*b_stride+col]|`.
///
/// Returns the total as `u32` (fits since 64×64 × 255 ≈ 1M).  The
/// classic motion-estimation cost function; the inner loop of every
/// hexagon/diamond/EPZS search in every encoder.
///
/// Strides are byte strides (signed `isize` to allow reverse-scan).
///
/// # Panics
/// If `a` does not contain `(h-1)*a_stride.abs() + w` bytes, or `b`
/// likewise.  (Best-effort bounds check; full geometric validity is
/// caller's responsibility.)
pub fn sad_block_u8(
    a: &[u8],
    a_stride: isize,
    b: &[u8],
    b_stride: isize,
    w: usize,
    h: usize,
) -> u32 {
    if h == 0 || w == 0 {
        return 0;
    }
    let needed = (h - 1) * (a_stride.unsigned_abs()) + w;
    assert!(
        a.len() >= needed,
        "codec-simd: sad_block_u8 a too short ({} < {})",
        a.len(),
        needed
    );
    let needed_b = (h - 1) * (b_stride.unsigned_abs()) + w;
    assert!(
        b.len() >= needed_b,
        "codec-simd: sad_block_u8 b too short ({} < {})",
        b.len(),
        needed_b
    );
    // SAFETY: ptrs valid for the block geometry verified above.
    unsafe { (dispatch::get().sad_block_u8)(a.as_ptr(), a_stride, b.as_ptr(), b_stride, w, h) }
}

/// Hadamard-transformed sum of absolute differences on a 4×4 u8 block.
///
/// Computes `sum |H × (a - b) × H'|` where `H` is the 4×4 Walsh-Hadamard
/// matrix, then `(sum + 1) >> 1` (the HEVC/ffmpeg `satd_4x4_8_c`
/// convention).  Returns the cost as `u32`.
///
/// The standard alternative to SAD for motion estimation when transform
/// quality matters more than pure pixel diff — captures spatial
/// frequency information that pure SAD misses.
///
/// # Panics
/// If slice lengths are insufficient for `(h-1)*stride + w` (h=w=4 here).
pub fn satd_block_4x4_u8(a: &[u8], a_stride: isize, b: &[u8], b_stride: isize) -> u32 {
    let needed = 3 * a_stride.unsigned_abs() + 4;
    assert!(
        a.len() >= needed,
        "codec-simd: satd_block_4x4_u8 a too short"
    );
    let needed_b = 3 * b_stride.unsigned_abs() + 4;
    assert!(
        b.len() >= needed_b,
        "codec-simd: satd_block_4x4_u8 b too short"
    );
    unsafe { (dispatch::get().satd_block_4x4_u8)(a.as_ptr(), a_stride, b.as_ptr(), b_stride) }
}

/// Hadamard-transformed sum of absolute differences on an 8×8 u8 block.
///
/// Computes `sum |H8 × (a - b) × H8'|` where `H8` is the 8×8 Walsh-Hadamard
/// matrix, then `(sum + 2) >> 2` (the HEVC/HM `satd_8x8` convention, as used by
/// kvazaar `satd_8x8_subblock`).  Returns the cost as `u32`.
///
/// The 8×8 SATD is the dominant rough-search / sub-pel-refinement cost in HEVC
/// encoders (blocks ≥ 8 are tiled into 8×8 sub-blocks).
///
/// # Panics
/// If slice lengths are insufficient for `7*stride + 8`.
pub fn satd_block_8x8_u8(a: &[u8], a_stride: isize, b: &[u8], b_stride: isize) -> u32 {
    let needed = 7 * a_stride.unsigned_abs() + 8;
    assert!(
        a.len() >= needed,
        "codec-simd: satd_block_8x8_u8 a too short"
    );
    let needed_b = 7 * b_stride.unsigned_abs() + 8;
    assert!(
        b.len() >= needed_b,
        "codec-simd: satd_block_8x8_u8 b too short"
    );
    unsafe { (dispatch::get().satd_block_8x8_u8)(a.as_ptr(), a_stride, b.as_ptr(), b_stride) }
}

/// Sum of squared errors across a `w × h` u8 block.
///
/// Per-pixel: `(a - b) * (a - b)`.  Returns total as `u64` for safety
/// at large blocks (max realistic block 128×128 fits comfortably in
/// `u32`; `u64` future-proofs).
///
/// Used as the rate-distortion cost in every encoder — measures
/// quality loss in transform domain after quantize+dequant.  Strictly
/// larger than SAD: penalizes outliers (squared error) where SAD
/// treats all errors linearly.
///
/// # Panics
/// If slices are too short for the block geometry.
pub fn sse_block_u8(
    a: &[u8],
    a_stride: isize,
    b: &[u8],
    b_stride: isize,
    w: usize,
    h: usize,
) -> u64 {
    if h == 0 || w == 0 {
        return 0;
    }
    let needed = (h - 1) * (a_stride.unsigned_abs()) + w;
    assert!(
        a.len() >= needed,
        "codec-simd: sse_block_u8 a too short ({} < {})",
        a.len(),
        needed
    );
    let needed_b = (h - 1) * (b_stride.unsigned_abs()) + w;
    assert!(
        b.len() >= needed_b,
        "codec-simd: sse_block_u8 b too short ({} < {})",
        b.len(),
        needed_b
    );
    unsafe { (dispatch::get().sse_block_u8)(a.as_ptr(), a_stride, b.as_ptr(), b_stride, w, h) }
}

// =========================================================================
// Block sum + sum-of-squares (variance metric) (v0.2.5).
// =========================================================================

/// Sum and sum-of-squares of a `w × h` u8 block: returns `(sum, sum_sq)`
/// where `sum = Σ src[i]` and `sum_sq = Σ src[i]²`.
///
/// The encoder-side activity metric.  Block variance is
/// `sum_sq - (sum * sum) / (w*h)`; mean is `sum / (w*h)`.  Used in:
/// - HEVC adaptive quantization (`vbaq`) — higher-variance blocks get
///   coarser quant.
/// - VP9/AV1 partition-cost / mode decisions.
/// - PSNR / MSE computation.
///
/// Fusing both reductions into one SIMD pass avoids scanning the block
/// twice.  `sum` fits `u32` (≤ 128×128×255 < `u32::MAX`); `sum_sq` is
/// `u64` for headroom.
///
/// `stride` is a signed byte stride.
///
/// # Panics
/// If `src` is too short for the block geometry `(h-1)*stride.abs() + w`.
pub fn variance_block_u8(src: &[u8], stride: isize, w: usize, h: usize) -> (u32, u64) {
    if h == 0 || w == 0 {
        return (0, 0);
    }
    let needed = (h - 1) * stride.unsigned_abs() + w;
    assert!(
        src.len() >= needed,
        "codec-simd: variance_block_u8 src too short ({} < {})",
        src.len(),
        needed
    );
    // SAFETY: ptr valid for the block geometry verified above.
    unsafe { (dispatch::get().variance_block_u8)(src.as_ptr(), stride, w, h) }
}

// =========================================================================
// Encoder SAD-x4 (4 MV candidates in parallel) (v0.3.11).
// =========================================================================

/// Sum of absolute differences of one `a` block against **four** candidate
/// blocks at once, returning `[sad0, sad1, sad2, sad3]`.
///
/// The motion-search inner loop: a hexagon/diamond step evaluates several
/// candidate MVs around a centre, and this fn computes all four SADs while
/// loading each `a` row only once.  `refs[k]` are the four candidate block
/// origins; they share `b_stride`.
///
/// # Panics
/// If `a` or any `refs[k]` is too short for the `w × h` block geometry
/// (`(h-1)*|stride| + w`).
pub fn sad_x4_block_u8(
    a: &[u8],
    a_stride: isize,
    refs: &[&[u8]; 4],
    b_stride: isize,
    w: usize,
    h: usize,
) -> [u32; 4] {
    if w == 0 || h == 0 {
        return [0; 4];
    }
    let needed_a = (h - 1) * a_stride.unsigned_abs() + w;
    assert!(
        a.len() >= needed_a,
        "codec-simd: sad_x4_block_u8 a too short ({} < {})",
        a.len(),
        needed_a
    );
    let needed_b = (h - 1) * b_stride.unsigned_abs() + w;
    for (i, r) in refs.iter().enumerate() {
        assert!(
            r.len() >= needed_b,
            "codec-simd: sad_x4_block_u8 ref[{i}] too short ({} < {})",
            r.len(),
            needed_b
        );
    }
    // SAFETY: all five blocks verified for the geometry above.
    unsafe {
        (dispatch::get().sad_x4_block_u8)(
            a.as_ptr(),
            a_stride,
            refs[0].as_ptr(),
            refs[1].as_ptr(),
            refs[2].as_ptr(),
            refs[3].as_ptr(),
            b_stride,
            w,
            h,
        )
    }
}

// =========================================================================
// 4×4 Walsh-Hadamard transform (v0.3.15).
// =========================================================================

/// 2D Walsh-Hadamard transform of a row-major 4×4 `i16` block:
/// `Y = H·X·H`, where `H` is the 4×4 ±1 Hadamard matrix.
///
/// Codec-agnostic (no constants).  The building block inside 8×8 SATD and
/// a transform-domain analysis tool.  Output values are in
/// `[-16·max, 16·max]` of the input magnitude (no overflow for inputs in
/// the usual diff range).
///
/// Note: SSE2/AVX2 use a vectorized in-register transform; AArch64 NEON
/// and wasm currently use the (correct) scalar fallback — a SIMD backfill
/// for those is roadmapped (as the v0.2.2 SATD backfill was).
///
/// # Panics
/// If `src.len() != 16 || dst.len() != 16`.
pub fn hadamard_4x4_i16(src: &[i16], dst: &mut [i16]) {
    assert_eq!(
        src.len(),
        16,
        "codec-simd: hadamard_4x4_i16 src.len() must be 16"
    );
    assert_eq!(
        dst.len(),
        16,
        "codec-simd: hadamard_4x4_i16 dst.len() must be 16"
    );
    // SAFETY: both slices are exactly 16 i16.
    unsafe {
        (dispatch::get().hadamard_4x4_i16)(src.as_ptr(), dst.as_mut_ptr());
    }
}

// =========================================================================
// High-bit-depth (u16) encoder-cost siblings (Tier-D).
// Strides are signed ELEMENT (u16) strides, matching the u8 kernels.
// =========================================================================

/// Sum of absolute differences across a `w × h` u16 block — the 10/12/16-bit
/// sibling of [`sad_block_u8`].
///
/// # Panics
/// If `a`/`b` are shorter than `(h-1)*|stride| + w` elements.
pub fn sad_block_u16(
    a: &[u16],
    a_stride: isize,
    b: &[u16],
    b_stride: isize,
    w: usize,
    h: usize,
) -> u32 {
    if h == 0 || w == 0 {
        return 0;
    }
    let needed = (h - 1) * a_stride.unsigned_abs() + w;
    assert!(
        a.len() >= needed,
        "codec-simd: sad_block_u16 a too short ({} < {})",
        a.len(),
        needed
    );
    let needed_b = (h - 1) * b_stride.unsigned_abs() + w;
    assert!(
        b.len() >= needed_b,
        "codec-simd: sad_block_u16 b too short ({} < {})",
        b.len(),
        needed_b
    );
    // SAFETY: block geometry verified above.
    unsafe { (dispatch::get().sad_block_u16)(a.as_ptr(), a_stride, b.as_ptr(), b_stride, w, h) }
}

/// Hadamard-transformed SAD on a 4×4 u16 block — the sibling of
/// [`satd_block_4x4_u8`]. `(sum + 1) >> 1`.
///
/// # Panics
/// If slices are shorter than `3*|stride| + 4` elements.
pub fn satd_block_4x4_u16(a: &[u16], a_stride: isize, b: &[u16], b_stride: isize) -> u32 {
    let needed = 3 * a_stride.unsigned_abs() + 4;
    assert!(
        a.len() >= needed,
        "codec-simd: satd_block_4x4_u16 a too short"
    );
    let needed_b = 3 * b_stride.unsigned_abs() + 4;
    assert!(
        b.len() >= needed_b,
        "codec-simd: satd_block_4x4_u16 b too short"
    );
    // SAFETY: block geometry verified above.
    unsafe { (dispatch::get().satd_block_4x4_u16)(a.as_ptr(), a_stride, b.as_ptr(), b_stride) }
}

/// Hadamard-transformed SAD on an 8×8 u16 block — the sibling of
/// [`satd_block_8x8_u8`]. `(sum + 2) >> 2`.
///
/// # Panics
/// If slices are shorter than `7*|stride| + 8` elements.
pub fn satd_block_8x8_u16(a: &[u16], a_stride: isize, b: &[u16], b_stride: isize) -> u32 {
    let needed = 7 * a_stride.unsigned_abs() + 8;
    assert!(
        a.len() >= needed,
        "codec-simd: satd_block_8x8_u16 a too short"
    );
    let needed_b = 7 * b_stride.unsigned_abs() + 8;
    assert!(
        b.len() >= needed_b,
        "codec-simd: satd_block_8x8_u16 b too short"
    );
    // SAFETY: block geometry verified above.
    unsafe { (dispatch::get().satd_block_8x8_u16)(a.as_ptr(), a_stride, b.as_ptr(), b_stride) }
}

/// Sum of squared errors across a `w × h` u16 block — the sibling of
/// [`sse_block_u8`]. Returns `u64`.
///
/// # Panics
/// If `a`/`b` are shorter than `(h-1)*|stride| + w` elements.
pub fn sse_block_u16(
    a: &[u16],
    a_stride: isize,
    b: &[u16],
    b_stride: isize,
    w: usize,
    h: usize,
) -> u64 {
    if h == 0 || w == 0 {
        return 0;
    }
    let needed = (h - 1) * a_stride.unsigned_abs() + w;
    assert!(
        a.len() >= needed,
        "codec-simd: sse_block_u16 a too short ({} < {})",
        a.len(),
        needed
    );
    let needed_b = (h - 1) * b_stride.unsigned_abs() + w;
    assert!(
        b.len() >= needed_b,
        "codec-simd: sse_block_u16 b too short ({} < {})",
        b.len(),
        needed_b
    );
    // SAFETY: block geometry verified above.
    unsafe { (dispatch::get().sse_block_u16)(a.as_ptr(), a_stride, b.as_ptr(), b_stride, w, h) }
}
