//! MIP-map generation: the ONE builder, OpenImageIO `maketx`'s.
//!
//! `maketx` builds each level from the previous one, halving each dimension with integer division
//! (`w > 1 ? w / 2 : w`, `maketexture.cpp:850-855`), and with its default `box` filter resamples
//! through `resize_block` (`:308-333`):
//!
//! - a level whose width and height are both even is a 2x2 box average, done as two passes
//!   (`resize_block_2pass`, `:261-305`: `0.5 * (a + b)` per row pair, then `0.5 * (r0 + r1)`);
//! - any other level (an odd side, or a side of 1) is a bilinear interpolation of the source at
//!   each destination pixel centre, clamped at the edges (`resize_block_` with
//!   `interppixel_NDC`, `:141-237`). 5x3 -> 2x1 is where this and a 2x2 box disagree.
//!
//! Every level is computed from the previous level in `f32` (`maketx:forcefloat`, default on,
//! `:858-860`), so a chain built here from a float level 0 reproduces `maketx`'s float output bit
//! for bit (gated by vfx-io's `maketx_chain_matches_maketx_files`). Deliberately not reproduced:
//! the other `maketx` filters (vfx-io's `make_texture` runs them through `ImageBufAlgo::resize`,
//! as `maketx` does), lat-long environment weighting, `maketx:allow_pixel_shift`, half-float
//! clamping, and one integer-format nuance: for an 8/16-bit input `maketx` reads level 0 as that
//! type, so its first halving takes the bilinear path even at even sizes; the two paths differ by
//! rounding only (1 ulp).
//!
//! [`maketx_halve_rect`] computes any window of a level from the window above it that
//! [`maketx_source_rect`] names, bit-identically to the whole-level halving, so a tiler can build
//! the part of a chain a tile needs from the part of level 0 below it.
//!
//! Users: vfx-io's `ImageCache` (levels past a file's own chain), vfx-io's `make_texture`, and
//! vfx-warp's EWA mode (the source's MIP chain, whole or per tile).

use crate::rect::Rect;

/// Number of levels in a full chain: the original plus one per halving until both sides reach 1,
/// `floor(log2(max(w, h))) + 1`. 1920x1080 has 11 levels, 3x3 has 2.
#[must_use]
pub const fn mip_level_count(width: u32, height: u32) -> u32 {
    let m = if width > height { width } else { height };
    32 - (if m == 0 { 1 } else { m }).leading_zeros()
}

/// Size of level `level` of a `width x height` chain: `max(1, side >> level)`.
#[must_use]
pub const fn mip_dimensions(width: u32, height: u32, level: u32) -> (u32, u32) {
    let w = if level >= 32 { 0 } else { width >> level };
    let h = if level >= 32 { 0 } else { height >> level };
    (if w == 0 { 1 } else { w }, if h == 0 { 1 } else { h })
}

/// One `maketx` box-filter halving of an interleaved `f32` level (`resize_block`,
/// `maketexture.cpp:308-333`). Returns the next level, `mip_dimensions(width, height, 1)` in size.
///
/// # Panics
///
/// If `src` is shorter than `width * height * nchannels`.
#[must_use]
pub fn maketx_halve(src: &[f32], width: u32, height: u32, nchannels: usize) -> Vec<f32> {
    let (dw, dh) = mip_dimensions(width, height, 1);
    maketx_halve_rect(
        src,
        Rect::new(0, 0, width, height),
        width,
        height,
        nchannels,
        Rect::new(0, 0, dw, dh),
    )
}

/// The texels of one destination index along one axis: `maketx`'s two taps and the weight of the
/// second. `boxed` is the two-pass 2x2 box (`resize_block_2pass`, `:261-305`), otherwise the
/// clamped bilinear tap of `resize_block_`/`interppixel_NDC` (`:206-237`, `:141-198`): the source
/// at the destination pixel centre `(d + 0.5) / dsize`, i.e. `(d + 0.5) / dsize * size - 0.5`.
fn axis_taps(size: u32, dsize: u32, d: u32, boxed: bool) -> (u32, u32, f32) {
    if boxed {
        return (2 * d, 2 * d + 1, 0.5);
    }
    let s = (d as f32 + 0.5) * (1.0 / dsize as f32);
    let f = s * size as f32 - 0.5;
    let fl = f.floor();
    let t = fl as i32;
    let clamp = |v: i32| v.clamp(0, size as i32 - 1) as u32;
    (clamp(t), clamp(t + 1), f - fl)
}

/// Whether `maketx` halves a `width x height` level with the two-pass 2x2 box (both sides even)
/// rather than the bilinear path (`maketexture.cpp:264-267`, `maketx:allow_pixel_shift` off).
const fn boxed(width: u32, height: u32) -> bool {
    width.is_multiple_of(2) && height.is_multiple_of(2)
}

/// The window of a `width x height` level that [`maketx_halve_rect`] reads to produce window
/// `dst` of the next level. Empty for an empty `dst`.
#[must_use]
pub fn maketx_source_rect(width: u32, height: u32, dst: Rect) -> Rect {
    if dst.width == 0 || dst.height == 0 {
        return Rect::new(0, 0, 0, 0);
    }
    let (dw, dh) = mip_dimensions(width, height, 1);
    let b = boxed(width, height);
    // The taps are non-decreasing in the destination index, so the first and last destination
    // pixels bound the window.
    let (x0, _, _) = axis_taps(width, dw, dst.x, b);
    let (_, x1, _) = axis_taps(width, dw, dst.x + dst.width - 1, b);
    let (y0, _, _) = axis_taps(height, dh, dst.y, b);
    let (_, y1, _) = axis_taps(height, dh, dst.y + dst.height - 1, b);
    Rect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
}

/// Window `dst` of the next level after a `width x height` level, from window `src_rect` of that
/// level (interleaved, `src_rect.width * src_rect.height * nchannels` floats). Exactly the pixels
/// [`maketx_halve`] computes there, so a tiler can build any part of the chain from the part of
/// level 0 below it ([`maketx_source_rect`] says which part).
///
/// # Panics
///
/// If `src_rect` does not contain [`maketx_source_rect`]`(width, height, dst)`, or `src` is
/// shorter than `src_rect` says.
#[must_use]
pub fn maketx_halve_rect(
    src: &[f32],
    src_rect: Rect,
    width: u32,
    height: u32,
    nchannels: usize,
    dst: Rect,
) -> Vec<f32> {
    let n = nchannels;
    let (sw, sh) = (src_rect.width as usize, src_rect.height as usize);
    assert!(
        src.len() >= sw * sh * n,
        "window is {} floats, expected {sw}x{sh}x{n}",
        src.len()
    );
    let (dw, dh) = mip_dimensions(width, height, 1);
    let b = boxed(width, height);
    let at = |x: u32, y: u32, c: usize| {
        let lx = x.checked_sub(src_rect.x).filter(|&v| (v as usize) < sw);
        let ly = y.checked_sub(src_rect.y).filter(|&v| (v as usize) < sh);
        let (Some(lx), Some(ly)) = (lx, ly) else {
            panic!("maketx_halve_rect: texel ({x},{y}) outside source window {src_rect:?}");
        };
        src[(ly as usize * sw + lx as usize) * n + c]
    };
    let mut out = vec![0.0f32; dst.width as usize * dst.height as usize * n];
    let mut o = 0;
    for y in dst.y..dst.y + dst.height {
        let (y0, y1, yfrac) = axis_taps(height, dh, y, b);
        for x in dst.x..dst.x + dst.width {
            let (x0, x1, xfrac) = axis_taps(width, dw, x, b);
            for c in 0..n {
                out[o] = if b {
                    // `halve_scanline` of two rows, then `0.5 * (s0 + s1)`.
                    let s0 = 0.5 * (at(x0, y0, c) + at(x1, y0, c));
                    let s1 = 0.5 * (at(x0, y1, c) + at(x1, y1, c));
                    0.5 * (s0 + s1)
                } else {
                    // `bilerp(p0..p3, xfrac, yfrac, n, pixel)` (`fmath.h:432-441`).
                    let (s1, t1) = (1.0 - xfrac, 1.0 - yfrac);
                    let (v0, v1) = (at(x0, y0, c), at(x1, y0, c));
                    let (v2, v3) = (at(x0, y1, c), at(x1, y1, c));
                    t1 * (v0 * s1 + v1 * xfrac) + yfrac * (v2 * s1 + v3 * xfrac)
                };
                o += 1;
            }
        }
    }
    out
}

/// Every level below level 0 of `maketx`'s box chain: element `m - 1` is level `m`, with its size.
#[must_use]
pub fn maketx_chain(
    level0: &[f32],
    width: u32,
    height: u32,
    nchannels: usize,
) -> Vec<(u32, u32, Vec<f32>)> {
    let mut out: Vec<(u32, u32, Vec<f32>)> = Vec::new();
    let (mut w, mut h) = (width, height);
    while w > 1 || h > 1 {
        let prev = out.last().map_or(level0, |(_, _, d)| d.as_slice());
        let next = maketx_halve(prev, w, h, nchannels);
        (w, h) = mip_dimensions(w, h, 1);
        out.push((w, h, next));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_counts_and_sizes_are_maketx_s() {
        // `iinfo -v` of OIIO 3.1's `maketx` output.
        assert_eq!(mip_level_count(1024, 1024), 11);
        assert_eq!(mip_level_count(1920, 1080), 11);
        assert_eq!(mip_level_count(3, 3), 2);
        assert_eq!(mip_level_count(1, 1), 1);
        assert_eq!(mip_dimensions(5, 3, 1), (2, 1));
        assert_eq!(mip_dimensions(1920, 1080, 5), (60, 33));
        assert_eq!(mip_dimensions(4, 4, 40), (1, 1));
    }

    #[test]
    fn even_levels_are_2x2_box_averages() {
        let src: Vec<f32> = (0..16).map(|v| v as f32).collect(); // 4x4, 1 channel
        let l1 = maketx_halve(&src, 4, 4, 1);
        assert_eq!(l1, vec![2.5, 4.5, 10.5, 12.5]);
    }

    #[test]
    fn odd_levels_interpolate_at_destination_centres() {
        // 3x1 -> 1x1: the centre of the only destination pixel is source pixel 1 exactly.
        assert_eq!(maketx_halve(&[1.0, 5.0, 9.0], 3, 1, 1), vec![5.0]);
        // 5x3 -> 2x1: destination x = 0 samples source x = 0.75, y = 1 (the middle row).
        let src: Vec<f32> = (0..15).map(|v| v as f32).collect();
        let l1 = maketx_halve(&src, 5, 3, 1);
        assert_eq!(l1.len(), 2);
        assert!(
            (l1[0] - 5.75).abs() < 1e-6 && (l1[1] - 8.25).abs() < 1e-6,
            "{l1:?}"
        );
    }

    #[test]
    fn windows_equal_the_whole_level() {
        // Every window of every level, built from exactly its source window, equals the same
        // pixels of the whole-level halving: the even (box) and odd (bilinear) paths, clamped
        // edges, one-pixel sides.
        for (w, h) in [(8u32, 6u32), (7, 5), (5, 3), (1, 9), (12, 1), (9, 9)] {
            let n = 2;
            let src: Vec<f32> = (0..w * h * n as u32)
                .map(|v| (v * 37 % 101) as f32 / 7.0)
                .collect();
            let whole = maketx_halve(&src, w, h, n);
            let (dw, dh) = mip_dimensions(w, h, 1);
            for y0 in 0..dh {
                for x0 in 0..dw {
                    for (rw, rh) in [(1, 1), (dw - x0, 1), (1, dh - y0), (dw - x0, dh - y0)] {
                        let dst = Rect::new(x0, y0, rw, rh);
                        let sr = maketx_source_rect(w, h, dst);
                        let window: Vec<f32> = (sr.y..sr.y + sr.height)
                            .flat_map(|y| (sr.x..sr.x + sr.width).map(move |x| (x, y)))
                            .flat_map(|(x, y)| {
                                let i = ((y * w + x) as usize) * n;
                                src[i..i + n].to_vec()
                            })
                            .collect();
                        let part = maketx_halve_rect(&window, sr, w, h, n, dst);
                        for (i, v) in part.iter().enumerate() {
                            let (px, c) = (i / n, i % n);
                            let (x, y) = (x0 + px as u32 % rw, y0 + px as u32 / rw);
                            let want = whole[((y * dw + x) as usize) * n + c];
                            assert_eq!(v.to_bits(), want.to_bits(), "{w}x{h} {dst:?} ({x},{y})");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn chain_ends_at_one_by_one() {
        let src = vec![1.0f32; 7 * 5 * 2];
        let chain = maketx_chain(&src, 7, 5, 2);
        let sizes: Vec<(u32, u32)> = chain.iter().map(|(w, h, _)| (*w, *h)).collect();
        assert_eq!(sizes, [(3, 2), (1, 1)]);
        assert!(chain.iter().all(|(_, _, d)| d.iter().all(|&v| v == 1.0)));
    }
}
