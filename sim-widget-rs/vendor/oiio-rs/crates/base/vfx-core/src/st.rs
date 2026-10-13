//! ST-map coordinates: the one definition of how a normalised `(s, t)` maps to a source pixel.
//!
//! Every warp in the stack decodes ST through this module: `vfx-warp`'s CPU kernel and its tiler,
//! `vfx-compute`'s tiler, and (by construction of the same formula) the WGSL/CUDA kernels. The
//! tiled paths re-encode a tile's ST map into a cropped source's coordinates and then let the
//! kernel decode it again; that is only exact when the re-encode is the kernel's decode run
//! backwards, which is what [`StDecode::from_px`] is. Four copies of this arithmetic used to
//! exist, two with an endpoint (`s * (w - 1)`) convention, and tiled output drifted from
//! single-pass output by up to a quarter of the value range.
//!
//! Convention: ST is a texel-centre parameterisation. The centre of pixel `i` in a dimension of
//! `size` pixels is at `st = (i + 0.5) / size`, so decoding is `px = st * size - 0.5`. With
//! `pixel_center_offset` the `- 0.5` is dropped (for maps that already point at pixel-index
//! space). OIIO's `st_warp` computes `st * size` in its own continuous space, whose pixel centres
//! sit at `i + 0.5`; that is the same point.

/// Which way S/T are mirrored before use: OIIO's `st_warp` parameters (`flip_s`, `flip_t`), named
/// so callers can pass a documented preset instead of two bare booleans.
///
/// [`StConvention::NUKE`] is `vfx_stmap`'s bottom-up V (`v = 1 - (row + 0.5) / height`), which
/// needs the T flip to recover the raster row; it is the default for historical callers.
/// [`StConvention::OIIO`] is OIIO's literal `st_warp` default (no flips), for maps that carry
/// top-down T.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StConvention {
    /// Mirror the S coordinate (`s = 1 - s`) before sampling.
    pub flip_s: bool,
    /// Mirror the T coordinate (`t = 1 - t`) before sampling.
    pub flip_t: bool,
}

impl StConvention {
    /// `flip_s: false, flip_t: true`: `vfx_stmap`'s bottom-up V, Nuke's STMap convention.
    pub const NUKE: Self = Self {
        flip_s: false,
        flip_t: true,
    };
    /// `flip_s: false, flip_t: false`: OIIO's literal `ImageBufAlgo::st_warp` default.
    pub const OIIO: Self = Self {
        flip_s: false,
        flip_t: false,
    };
}

impl Default for StConvention {
    /// [`StConvention::NUKE`], the default every existing warp caller relies on.
    fn default() -> Self {
        Self::NUKE
    }
}

/// One normalised coordinate to a pixel coordinate: `st * size - 0.5` (texel centres).
#[inline]
#[must_use]
pub fn st_to_px(st: f32, size: u32) -> f32 {
    st * size as f32 - 0.5
}

/// Exact inverse of [`st_to_px`]. `size` must be non-zero.
#[inline]
#[must_use]
pub fn px_to_st(px: f32, size: u32) -> f32 {
    (px + 0.5) / size as f32
}

/// The pixel nearest to pixel-index position `px`, OIIO's closest-texel rule.
///
/// `sample_closest` (`texturesys.cpp:2555-2562`) takes the texel at `floor(px)` and moves to the
/// next one only when the fraction is greater than 0.5, so an exact tie goes to the lower pixel on
/// both sides of zero (`f32::round` would go away from zero, `floor(px + 0.5)` up). The warp
/// kernels' nearest filter on every backend (Rust, WGSL, CUDA) uses this rule.
#[inline]
#[must_use]
pub fn nearest_px(px: f32) -> i32 {
    let f = px.floor();
    f as i32 + i32::from(px - f > 0.5)
}

/// The source window `[x, y, w, h]` a warp kernel reads for these decoded source positions
/// (pixel-index space), with `pad` pixels of filter margin: the one crop rule of the tiled warps
/// (vfx-compute `tiling`, vfx-warp `tiling`).
///
/// - Every position counts. The tilers used to probe every `min(tile) / 64` pixels plus the
///   corners, so detail between probes (noise, a fisheye rim, one far jump) read taps outside the
///   crop, which came back clamped or black in tiled mode only (DISP-04). Non-finite positions
///   are skipped.
/// - The window is clipped to the source and holds at least one pixel per axis: a tile whose
///   positions all lie past one edge used to get an empty crop.
/// - With `wraps` (wrap/mirror edge modes) an axis whose taps leave the source keeps the whole
///   axis: the wrapped texels are on the other side of the source, not in a crop.
#[must_use]
pub fn source_window(
    positions: impl IntoIterator<Item = (f32, f32)>,
    src_w: u32,
    src_h: u32,
    pad: f32,
    wraps: bool,
) -> [u32; 4] {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in positions {
        if x.is_finite() && y.is_finite() {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if min_x > max_x {
        // No finite position: nothing sensible is read; one pixel keeps an upload valid.
        (min_x, min_y, max_x, max_y) = (0.0, 0.0, 0.0, 0.0);
    }
    let axis = |lo: f32, hi: f32, size: u32| -> (u32, u32) {
        let (lo, hi) = (lo - pad, (hi + pad + 1.0).ceil());
        if size == 0 || (wraps && (lo < 0.0 || hi > size as f32)) {
            return (0, size);
        }
        let start = (lo.max(0.0) as u32).min(size - 1);
        let end = (hi.min(size as f32) as u32).max(start + 1);
        (start, end - start)
    };
    let (x, w) = axis(min_x, max_x, src_w);
    let (y, h) = axis(min_y, max_y, src_h);
    [x, y, w, h]
}

/// The full decode a warp kernel applies: flips, then texel-centre (or, with
/// `pixel_center_offset`, pixel-index) scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StDecode {
    /// S/T mirroring.
    pub convention: StConvention,
    /// Decode as `st * size` (no `- 0.5`).
    pub pixel_center_offset: bool,
}

impl StDecode {
    /// A decode with the given flips and offset.
    #[must_use]
    pub const fn new(flip_s: bool, flip_t: bool, pixel_center_offset: bool) -> Self {
        Self {
            convention: StConvention { flip_s, flip_t },
            pixel_center_offset,
        }
    }

    #[inline]
    fn offset(self) -> f32 {
        if self.pixel_center_offset { 0.0 } else { -0.5 }
    }

    /// `(s, t)` to a source pixel position in a `width x height` image.
    #[inline]
    #[must_use]
    pub fn to_px(self, s: f32, t: f32, width: u32, height: u32) -> (f32, f32) {
        let s = if self.convention.flip_s { 1.0 - s } else { s };
        let t = if self.convention.flip_t { 1.0 - t } else { t };
        (
            s.mul_add(width as f32, self.offset()),
            t.mul_add(height as f32, self.offset()),
        )
    }

    /// Exact inverse of [`Self::to_px`]: the `(s, t)` that decodes to pixel position `(x, y)` in a
    /// `width x height` image. `width` and `height` must be non-zero.
    #[inline]
    #[must_use]
    pub fn from_px(self, x: f32, y: f32, width: u32, height: u32) -> (f32, f32) {
        let s = (x - self.offset()) / width as f32;
        let t = (y - self.offset()) / height as f32;
        (
            if self.convention.flip_s { 1.0 - s } else { s },
            if self.convention.flip_t { 1.0 - t } else { t },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_centres_decode_to_their_index() {
        for (i, size) in [(0u32, 7u32), (3, 7), (6, 7), (0, 1)] {
            assert_eq!(st_to_px((i as f32 + 0.5) / size as f32, size), i as f32);
        }
        let d = StDecode::new(false, true, false);
        // Bottom-up T: row 0 is at t = 1 - 0.5 / h.
        let (x, y) = d.to_px(0.5 / 4.0, 1.0 - 0.5 / 3.0, 4, 3);
        assert!(x.abs() < 1e-6 && y.abs() < 1e-6, "({x},{y})");
    }

    #[test]
    fn source_window_rules() {
        // Bounds every position, plus the pad.
        let w = source_window([(5.0, 5.0), (40.0, 7.0), (6.0, 30.0)], 64, 64, 2.0, false);
        assert_eq!(w, [3, 3, 40, 30]);
        // Past the right edge only: one pixel at the edge, not an empty window.
        assert_eq!(
            source_window([(80.0, 5.0)], 64, 64, 2.0, false),
            [63, 3, 1, 5]
        );
        // Wrap/mirror: an axis whose taps leave the source keeps the whole axis.
        assert_eq!(
            source_window([(62.5, 5.0)], 64, 64, 2.0, true),
            [0, 3, 64, 5]
        );
        // Non-finite positions are skipped.
        assert_eq!(
            source_window([(f32::NAN, 1.0), (10.0, 10.0)], 64, 64, 0.0, false),
            [10, 10, 1, 1]
        );
    }

    #[test]
    fn nearest_px_is_oiio_sample_closest() {
        // texturesys.cpp:2555-2562: stex = floor(x); ++stex only if frac > 0.5.
        for (px, want) in [
            (2.5, 2),
            (2.5001, 3),
            (2.4999, 2),
            (-0.5, -1),
            (-2.5, -3),
            (-2.4999, -2),
            (0.0, 0),
            (-0.0001, 0),
            (7.0, 7),
        ] {
            assert_eq!(nearest_px(px), want, "px = {px}");
        }
    }

    #[test]
    fn from_px_inverts_to_px_for_every_decode() {
        for flip_s in [false, true] {
            for flip_t in [false, true] {
                for pco in [false, true] {
                    let d = StDecode::new(flip_s, flip_t, pco);
                    for (s, t) in [(0.0, 0.0), (0.13, 0.91), (1.0, 0.5), (-0.2, 1.3)] {
                        let (x, y) = d.to_px(s, t, 37, 23);
                        let (s2, t2) = d.from_px(x, y, 37, 23);
                        assert!(
                            (s - s2).abs() < 1e-6 && (t - t2).abs() < 1e-6,
                            "{d:?} ({s},{t})"
                        );
                    }
                }
            }
        }
    }
}
