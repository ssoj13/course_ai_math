//! Colour-conversion family — 3-input linear combination + clip and the
//! HTJ2K reversible colour transform (forward / inverse).

use crate::dispatch;

// =========================================================================
// 3-input linear combination + clip (colour-conversion plane) (v0.3.14).
// =========================================================================

/// Per element: `dst[i] = clip_u8((c0*p0[i] + c1*p1[i] + c2*p2[i] +
/// offset) >> shift)`.
///
/// The YUV→RGB output-plane building block: call once per channel with
/// that channel's coefficient row (folding the `-c*offset` chroma/luma
/// bias terms into `offset`).  E.g. limited-range BT.601 red:
/// `c0=298, c1=0, c2=409, offset=128 - 298*16 - 409*128, shift=8`.
/// Standard-agnostic (the caller supplies the matrix) and a general
/// 3-input linear blend.  Inputs and output are planar (no interleave).
///
/// # Panics
/// If `shift > 31`, or any of `p0`/`p1`/`p2` is shorter than `dst`.
#[allow(clippy::too_many_arguments)]
pub fn lincomb3_clip_u8(
    p0: &[u8],
    p1: &[u8],
    p2: &[u8],
    dst: &mut [u8],
    c0: i16,
    c1: i16,
    c2: i16,
    offset: i32,
    shift: u32,
) {
    let n = dst.len();
    if n == 0 {
        return;
    }
    assert!(
        shift <= 31,
        "codec-simd: lincomb3_clip_u8 shift must be 0..=31 (got {shift})"
    );
    assert!(
        p0.len() >= n && p1.len() >= n && p2.len() >= n,
        "codec-simd: lincomb3_clip_u8 input plane shorter than dst ({} / {} / {} < {})",
        p0.len(),
        p1.len(),
        p2.len(),
        n
    );
    // SAFETY: all three input planes are at least `n` long.
    unsafe {
        (dispatch::get().lincomb3_clip_u8)(
            p0.as_ptr(),
            p1.as_ptr(),
            p2.as_ptr(),
            dst.as_mut_ptr(),
            n,
            c0,
            c1,
            c2,
            offset,
            shift,
        );
    }
}

/// HTJ2K reversible colour transform, forward (RGB→YCbCr), `n` i32 samples:
/// `Y=(R+2G+B)>>2; Cb=B-G; Cr=R-G`.  Lossless / bit-exact (HTJ2K RCT,
/// jph-rs).  Inputs and outputs are separate planes of `n` elements; in-place
/// aliasing of an input with an output plane is not supported.
pub fn rct_fwd_i32(
    r: &[i32],
    g: &[i32],
    b: &[i32],
    y: &mut [i32],
    cb: &mut [i32],
    cr: &mut [i32],
    n: usize,
) {
    if n == 0 {
        return;
    }
    assert!(
        r.len() >= n,
        "codec-simd: rct_fwd_i32 r too short ({} < {n})",
        r.len()
    );
    assert!(
        g.len() >= n,
        "codec-simd: rct_fwd_i32 g too short ({} < {n})",
        g.len()
    );
    assert!(
        b.len() >= n,
        "codec-simd: rct_fwd_i32 b too short ({} < {n})",
        b.len()
    );
    assert!(
        y.len() >= n,
        "codec-simd: rct_fwd_i32 y too short ({} < {n})",
        y.len()
    );
    assert!(
        cb.len() >= n,
        "codec-simd: rct_fwd_i32 cb too short ({} < {n})",
        cb.len()
    );
    assert!(
        cr.len() >= n,
        "codec-simd: rct_fwd_i32 cr too short ({} < {n})",
        cr.len()
    );
    // SAFETY: all six planes verified to hold at least `n` elements.
    unsafe {
        (dispatch::get().rct_fwd_i32)(
            r.as_ptr(),
            g.as_ptr(),
            b.as_ptr(),
            y.as_mut_ptr(),
            cb.as_mut_ptr(),
            cr.as_mut_ptr(),
            n,
        );
    }
}

/// HTJ2K reversible colour transform, inverse (YCbCr→RGB), `n` i32 samples:
/// `G=Y-((Cb+Cr)>>2); R=Cr+G; B=Cb+G`.  Exact inverse of [`rct_fwd_i32`].
/// Separate planes; in-place aliasing of an input with an output plane is not
/// supported.
pub fn rct_bwd_i32(
    y: &[i32],
    cb: &[i32],
    cr: &[i32],
    r: &mut [i32],
    g: &mut [i32],
    b: &mut [i32],
    n: usize,
) {
    if n == 0 {
        return;
    }
    assert!(
        y.len() >= n,
        "codec-simd: rct_bwd_i32 y too short ({} < {n})",
        y.len()
    );
    assert!(
        cb.len() >= n,
        "codec-simd: rct_bwd_i32 cb too short ({} < {n})",
        cb.len()
    );
    assert!(
        cr.len() >= n,
        "codec-simd: rct_bwd_i32 cr too short ({} < {n})",
        cr.len()
    );
    assert!(
        r.len() >= n,
        "codec-simd: rct_bwd_i32 r too short ({} < {n})",
        r.len()
    );
    assert!(
        g.len() >= n,
        "codec-simd: rct_bwd_i32 g too short ({} < {n})",
        g.len()
    );
    assert!(
        b.len() >= n,
        "codec-simd: rct_bwd_i32 b too short ({} < {n})",
        b.len()
    );
    // SAFETY: all six planes verified to hold at least `n` elements.
    unsafe {
        (dispatch::get().rct_bwd_i32)(
            y.as_ptr(),
            cb.as_ptr(),
            cr.as_ptr(),
            r.as_mut_ptr(),
            g.as_mut_ptr(),
            b.as_mut_ptr(),
            n,
        );
    }
}
