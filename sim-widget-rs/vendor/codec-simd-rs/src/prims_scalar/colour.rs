/// Scalar reference for [`crate::lincomb3_clip_u8`].
///
/// Per element: `dst = clip_u8((c0*p0 + c1*p1 + c2*p2 + offset) >> shift)`.
/// The YUV→RGB plane building block (one call per output channel) and a
/// general 3-input linear blend.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn lincomb3_clip_u8(
    p0: *const u8,
    p1: *const u8,
    p2: *const u8,
    dst: *mut u8,
    n: usize,
    c0: i16,
    c1: i16,
    c2: i16,
    offset: i32,
    shift: u32,
) {
    let (c0, c1, c2) = (c0 as i32, c1 as i32, c2 as i32);
    for i in 0..n {
        let v0 = unsafe { *p0.add(i) } as i32;
        let v1 = unsafe { *p1.add(i) } as i32;
        let v2 = unsafe { *p2.add(i) } as i32;
        let acc = (c0 * v0 + c1 * v1 + c2 * v2 + offset) >> shift;
        unsafe {
            *dst.add(i) = acc.clamp(0, 255) as u8;
        }
    }
}

/// Scalar reference for [`crate::rct_fwd_i32`].  HTJ2K reversible colour
/// (RGB→YCbCr): `Y=(R+2G+B)>>2; Cb=B-G; Cr=R-G`.
pub(crate) unsafe fn rct_fwd_i32(
    r: *const i32,
    g: *const i32,
    b: *const i32,
    y: *mut i32,
    cb: *mut i32,
    cr: *mut i32,
    n: usize,
) {
    for i in 0..n {
        let rr = unsafe { *r.add(i) };
        let gg = unsafe { *g.add(i) };
        let bb = unsafe { *b.add(i) };
        unsafe {
            *y.add(i) = (rr + (gg << 1) + bb) >> 2;
            *cb.add(i) = bb - gg;
            *cr.add(i) = rr - gg;
        }
    }
}

/// Scalar reference for [`crate::rct_bwd_i32`].  HTJ2K reversible colour
/// (YCbCr→RGB): `G=Y-((Cb+Cr)>>2); R=Cr+G; B=Cb+G`.
pub(crate) unsafe fn rct_bwd_i32(
    y: *const i32,
    cb: *const i32,
    cr: *const i32,
    r: *mut i32,
    g: *mut i32,
    b: *mut i32,
    n: usize,
) {
    for i in 0..n {
        let yy = unsafe { *y.add(i) };
        let cbb = unsafe { *cb.add(i) };
        let crr = unsafe { *cr.add(i) };
        let gg = yy - ((cbb + crr) >> 2);
        unsafe {
            *g.add(i) = gg;
            *r.add(i) = crr + gg;
            *b.add(i) = cbb + gg;
        }
    }
}
