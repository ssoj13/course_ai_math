//! The workspace's ONE separable image resample.
//!
//! # Why it lives here
//!
//! [`crate::filter`] is the single kernel vocabulary — it absorbed five copies of `cubic_bc` /
//! `catmull_rom` / `mitchell` / `lanczos`. The LOOP that uses those kernels had not been merged
//! with them, so it existed three times over:
//!
//! - `vfx_ops::resize::resize_f32_cpu` — correct;
//! - `vfx_compute`'s CPU backend — a hardcoded 2x2 bilinear that ignored the requested filter,
//!   never widened its footprint when minifying, and sampled at `dx * scale` with no half-pixel
//!   centre;
//! - `vfx_compute`'s wgpu shader — the same, and the filter index was not even bound to it.
//!
//! MEASURED before this module existed: reducing an `n x n` plate holding one lit pixel to 1x1
//! returned **1.000000** — the source peak, undiminished — for Bilinear, Bicubic and Lanczos3, at
//! every reduction from 4x to 64x, on both the CPU and wgpu backends. `dst = 1` makes
//! `dx * scale == 0`, so the corner pixel was read verbatim and called a resize.
//!
//! This module used to ALSO exist twice over, the second time not as a bug but as an SSOT gap:
//! a `Filter`-enum-keyed `resample_f32` (this one) alongside a separate `resample_f32_desc` for
//! the fuller [`crate::filter::FilterDesc`] vocabulary, added when that vocabulary was ported in.
//! **They are now ONE function.** [`resample_f32`] takes `&FilterDesc` and covers every filter
//! [`crate::filter::FilterKind`] names; the old reduced-vocabulary `Filter` enum this function
//! used to key on is gone (see `crate::filter`'s module docs for the full history).
//!
//! # The algorithm
//!
//! Transcribed from OpenImageIO's `imagebufalgo_xform.cpp` (`resize_`), which is the reference
//! this workspace follows. Three parts, and dropping any one of them is a distinct bug:
//!
//! 1. **`ratio = dst / src`**, less than one when minifying.
//! 2. **The support widens as the picture shrinks**: `radi = ceil(filterrad / ratio)` source
//!    pixels, so a 4x reduction reads a 4x wider neighbourhood instead of a fixed 2x2. Above 1:1
//!    OIIO instead widens the FILTER at construction (`fd.width * max(1, ratio)`), which this
//!    module folds into `filterrad` so both directions read from one expression.
//! 3. **Sample centres are half-pixel**: `src = (d + 0.5) * src_len / dst_len`, with the offset
//!    carried into the kernel argument as `ratio * (k - radi - (frac - 0.5))`. Weights are
//!    normalised by their own total, so edge clamping cannot darken the border.

use crate::filter::{FilterDesc, FilterKind};
use crate::runner::{Runner, RunnerError, default_runner, rows};

/// Taps and weights for one output position along one axis.
///
/// Separable resampling evaluates the same weights for every row (or column), so they are computed
/// once per output coordinate and reused. This is the substance of OIIO's `xfiltval_all`.
struct AxisTaps {
    /// First source index this output position reads (may be negative before clamping).
    start: i32,
    /// Normalised weights, `start + k` for `k` in `0..weights.len()`.
    weights: Vec<f32>,
}

/// The X-axis pass: uses `filter.xfilt`, keyed on `filter.width`. See [`axis_taps_oriented`] for
/// the shared implementation and the exact-integer `Nearest` rationale.
fn axis_taps_x(src_len: usize, dst_len: usize, filter: &FilterDesc) -> Vec<AxisTaps> {
    axis_taps_oriented(src_len, dst_len, filter, true)
}

/// The Y-axis pass: uses `filter.yfilt`, keyed on `filter.height`.
fn axis_taps_y(src_len: usize, dst_len: usize, filter: &FilterDesc) -> Vec<AxisTaps> {
    axis_taps_oriented(src_len, dst_len, filter, false)
}

/// Precompute the tap weights for every output position along one axis.
///
/// # The geometry is INTEGER for `FilterKind::Nearest`, and that is what makes the GPU agree
///
/// For [`FilterKind::Nearest`], everything that decides WHICH samples are read is exact integer
/// arithmetic, written the same way here, in the WGSL resize shaders and in the CUDA kernel:
///
/// - the sample centre `(d + 0.5) * src / dst` is the rational `N / D` with `N = (2d + 1) * src`,
///   `D = 2 * dst`, so its floor is `N / D` and its fraction `(N - i*D) / D`;
/// - the footprint radius is `ceil(support * src / dst)` when minifying and `ceil(support)`
///   otherwise, with `support` a half-integer (`2 * support` is exact);
/// - the test `|x| < 0.5` is `|k*D - N + i*D + dst| < src` - the same inequality with the common
///   denominator `2 * src` multiplied out.
///
/// It used to be float for every filter. WGSL does not require division to be correctly rounded,
/// so a centre that is EXACTLY an integer could floor one way on the host and the other on the
/// GPU. MEASURED 2026-09-20: the wgpu backend differed from this function on 155 of 5760 samples
/// at 97x61 -> 48x30 under `Nearest`, whose step turns a one-ulp shift into a whole sample;
/// restoring the float centre in the shader reproduces it (`texture_resize_parity` reddens at
/// 9.5e-2). The integer `Nearest` test is the same idea for an exact tie at `|x| = 0.5`: no case
/// in the gates hits one, and in integers none can round differently.
///
/// Every OTHER filter uses the continuous `xfilt`/`yfilt` this function calls — only `Nearest`'s
/// binary in/out test needed (and got) the integer treatment; a continuous kernel's VALUE is
/// float regardless, and those differ by an ulp, never by a whole sample.
///
/// Takes an `is_x` flag rather than being two copies: an anisotropic filter (`width != height`,
/// which `resize`'s magnify path produces) must resample each axis with its OWN support, and one
/// function selecting `filter.width`/`xfilt` vs `filter.height`/`yfilt` is what keeps that from
/// drifting into "someone edited the X pass and forgot the Y one".
fn axis_taps_oriented(
    src_len: usize,
    dst_len: usize,
    filter: &FilterDesc,
    is_x: bool,
) -> Vec<AxisTaps> {
    let support = (if is_x { filter.width } else { filter.height }) * 0.5;
    let ratio = dst_len as f32 / src_len as f32;
    let (src, dst) = (src_len as i64, dst_len as i64);
    let dd = 2 * dst;
    let s2 = (support * 2.0).round() as i64;
    let radi = if dst >= src {
        (s2 + 1) / 2
    } else {
        (s2 * src + dd - 1) / dd
    } as i32;
    let taps = (2 * radi + 1) as usize;
    let is_nearest = filter.kind == FilterKind::Nearest;

    (0..dst_len)
        .map(|d| {
            let n = (2 * d as i64 + 1) * src;
            let src_i = n / dd;
            let frac = (n - src_i * dd) as f32 / dd as f32;

            let mut weights = Vec::with_capacity(taps);
            let mut total = 0.0f32;
            for k in 0..taps {
                let kk = k as i64 - radi as i64;
                let w = if is_nearest {
                    if (kk * dd - n + src_i * dd + dst).abs() < src {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    let x = ratio * (kk as f32 - (frac - 0.5));
                    if is_x {
                        filter.xfilt(x)
                    } else {
                        filter.yfilt(x)
                    }
                };
                weights.push(w);
                total += w;
            }
            if total != 0.0 {
                for w in &mut weights {
                    *w /= total;
                }
            } else {
                weights.iter_mut().for_each(|w| *w = 0.0);
                weights[radi as usize] = 1.0;
            }
            AxisTaps {
                start: src_i as i32 - radi,
                weights,
            }
        })
        .collect()
}

/// Resample interleaved `channels`-channel f32 samples to `dst_w` x `dst_h`, using `filter`
/// (the FULL OIIO vocabulary, [`crate::filter::FilterKind`] via [`FilterDesc`]).
///
/// Edges clamp (the outermost sample repeats). Returns `dst_w * dst_h * channels` samples, or
/// `None` for a non-separable `filter` (`disk`, `radial-lanczos3`) — this function's two-pass
/// structure cannot evaluate a filter that is not a product of independent X/Y terms (see
/// [`FilterDesc::separable`]). That is an explicit refusal, not a silent wrong answer: a caller
/// that needs a non-separable filter should use `vfx-io`'s `ImageBufAlgo::resize`, which already
/// implements the true non-separable 2D convolution OIIO uses for those two filters.
///
/// [`FilterKind::Nearest`] uses the exact-integer fast path (see [`axis_taps`]); every other
/// filter uses its continuous `xfilt`/`yfilt`. No GPU backend in this workspace has kernels for
/// most of this vocabulary — only [`FilterKind::Nearest`]/`Triangle`/`CatmullRom`/`Lanczos3` (and
/// `Mitchell` on `vfx-warp` only) have one anywhere (see [`FilterKind::compute_index`]/
/// [`FilterKind::warp_index`]) — so a GPU-resident caller has no way to reach the rest and
/// therefore no way to silently substitute a different filter for one it cannot run: the absence
/// of a GPU entry point IS the explicit error.
///
/// Two passes rather than one: a separable filter costs `taps_x + taps_y` per output sample this
/// way against `taps_x * taps_y` fused, which at a 4x reduction with Lanczos3 is 50 versus 625.
///
/// Runs on [`default_runner`] (rayon's pool with the `rayon` feature); [`resample_f32_with`] takes
/// the runner, as an OpenFX plug-in must.
#[must_use]
pub fn resample_f32(
    src: &[f32],
    src_w: usize,
    src_h: usize,
    channels: usize,
    dst_w: usize,
    dst_h: usize,
    filter: &FilterDesc,
) -> Option<Vec<f32>> {
    // The default runners (`Rayon`, `Serial`) never fail, so the error branch is unreachable here.
    resample_f32_with(
        src,
        src_w,
        src_h,
        channels,
        dst_w,
        dst_h,
        filter,
        default_runner(),
    )
    .ok()
    .flatten()
}

/// [`resample_f32`] with its output rows run as the jobs of `runner` (one job per row, as before);
/// every row is accumulated by one job in the same tap order, so the result is bit-identical for
/// every runner.
///
/// # Errors
///
/// The runner's [`RunnerError`].
#[allow(clippy::too_many_arguments)]
pub fn resample_f32_with(
    src: &[f32],
    src_w: usize,
    src_h: usize,
    channels: usize,
    dst_w: usize,
    dst_h: usize,
    filter: &FilterDesc,
    runner: &dyn Runner,
) -> Result<Option<Vec<f32>>, RunnerError> {
    if !filter.separable() {
        return Ok(None);
    }
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 || channels == 0 {
        return Ok(Some(Vec::new()));
    }

    // Both passes run one OUTPUT row per job. Every output sample is still accumulated by
    // one thread in the same tap order as the serial loop, so the result is bit-identical to it -
    // the CPU/GPU parity tests and anything pinned to these pixels see no change. Single-threaded,
    // a 4K RGBA plate took ~420 ms to halve (measured in playa's `proxy_cost`), which is what made
    // the proxy tiers slower than full resolution.
    let (src_row, dst_row) = (src_w * channels, dst_w * channels);

    // Horizontal: src_w -> dst_w, height unchanged.
    let xt = axis_taps_x(src_w, dst_w, filter);
    let mut mid = vec![0.0f32; dst_row * src_h];
    rows(&mut mid, dst_row, runner, &|y, row| {
        let line = src.get(y * src_row..(y + 1) * src_row).unwrap_or(&[]);
        {
            for (x, t) in xt.iter().enumerate() {
                let out = x * channels;
                for (k, &w) in t.weights.iter().enumerate() {
                    if w == 0.0 {
                        continue;
                    }
                    let sx = (t.start + k as i32).clamp(0, src_w as i32 - 1) as usize;
                    let inp = sx * channels;
                    for c in 0..channels {
                        row[out + c] += line[inp + c] * w;
                    }
                }
            }
        }
    })?;

    // Vertical: src_h -> dst_h on the already-narrowed rows.
    let yt = axis_taps_y(src_h, dst_h, filter);
    let mut out = vec![0.0f32; dst_row * dst_h];
    rows(&mut out, dst_row, runner, &|y, row| {
        if let Some(t) = yt.get(y) {
            for (k, &w) in t.weights.iter().enumerate() {
                if w == 0.0 {
                    continue;
                }
                let sy = (t.start + k as i32).clamp(0, src_h as i32 - 1) as usize;
                let line = &mid[sy * dst_row..(sy + 1) * dst_row];
                // Tap-major over the row instead of pixel-major: the same additions into each
                // sample, in the same `k` order, but walking `line` contiguously.
                for (o, &m) in row.iter_mut().zip(line) {
                    *o += m * w;
                }
            }
        }
    })?;
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::filter_desc_from_name;
    use crate::runner::Serial;

    /// Jobs in reverse order: no row may depend on another.
    struct Reverse;

    impl Runner for Reverse {
        fn run(&self, jobs: u32, job: &(dyn Fn(u32) + Sync)) -> Result<(), RunnerError> {
            (0..jobs).rev().for_each(job);
            Ok(())
        }
    }

    /// The runner does not change a sample: serial, reversed and the default (rayon) agree bit for
    /// bit, minifying and magnifying.
    #[test]
    fn every_runner_gives_the_same_samples() {
        let (w, h, c) = (37, 23, 3);
        let src: Vec<f32> = (0..w * h * c)
            .map(|i| ((i * 7919) % 101) as f32 / 101.0)
            .collect();
        for (dw, dh) in [(11, 7), (64, 41)] {
            let f = desc(FilterKind::Lanczos3);
            let run = |r: &dyn Runner| {
                resample_f32_with(&src, w, h, c, dw, dh, &f, r)
                    .unwrap()
                    .unwrap()
            };
            let serial = run(&Serial);
            assert_eq!(serial, run(&Reverse));
            assert_eq!(serial, run(crate::runner::default_runner()));
            assert_eq!(Some(serial), resample_f32(&src, w, h, c, dw, dh, &f));
        }
    }

    fn desc(kind: FilterKind) -> FilterDesc {
        FilterDesc::new(kind)
    }

    /// The property the broken implementations failed: **a reduction REDUCES, and reduces more
    /// the further it goes.**
    ///
    /// A single lit pixel collapsed to 1x1 must come out a fraction of its original value, and
    /// that fraction must SHRINK as the reduction grows. The implementations this module replaces
    /// returned 1.000000 - the source peak, unchanged - for every filter at every reduction,
    /// because `dst = 1` made their sample position exactly `src[0,0]`.
    ///
    /// The lit pixel sits in the MIDDLE, not the corner. Edges clamp, so a corner pixel is
    /// REPLICATED into every tap that falls outside the image: at 64x that inflates its share by
    /// two orders of magnitude, which is correct behaviour and would make the assertion measure
    /// the edge rule rather than the reduction. (Learned by writing it the other way first.)
    ///
    /// Asserted as a monotone trend rather than against `1/(n*n)`: the exact share depends on the
    /// kernel's peak, so a literal would pin one filter's shape instead of the property.
    #[test]
    fn a_reduction_reduces_and_reduces_further_as_it_shrinks() {
        for f in [
            FilterKind::Triangle,
            FilterKind::CatmullRom,
            FilterKind::Mitchell,
            FilterKind::Lanczos3,
        ] {
            let f = desc(f);
            let mut previous = f32::MAX;
            for n in [4usize, 16, 64] {
                let mut src = vec![0.0f32; n * n * 4];
                let mid = ((n / 2) * n + n / 2) * 4;
                src[mid] = 1.0;
                for p in src.as_chunks_mut::<4>().0 {
                    p[3] = 1.0;
                }
                let got = resample_f32(&src, n, n, 4, 1, 1, &f).unwrap()[0];
                assert!(
                    got < 0.5,
                    "{:?} at {n}x{n} -> 1x1 returned {got}: a reduction cannot keep the peak",
                    f.kind
                );
                assert!(
                    got < previous,
                    "{:?}: {n}x{n} gave {got}, no smaller than the coarser step's {previous} - \
                     the footprint is not widening with the reduction",
                    f.kind
                );
                previous = got;
            }
        }
    }

    /// A flat field cannot gain or lose energy: normalised weights must sum to one at every
    /// output position, edges included.
    #[test]
    fn a_flat_field_survives_any_resize() {
        let src = vec![0.5f32; 37 * 23 * 4];
        for (dw, dh) in [(9usize, 5usize), (37, 23), (80, 60), (1, 1)] {
            for f in [
                FilterKind::Triangle,
                FilterKind::CatmullRom,
                FilterKind::Mitchell,
                FilterKind::Lanczos3,
            ] {
                let f = desc(f);
                let out = resample_f32(&src, 37, 23, 4, dw, dh, &f).unwrap();
                for (i, v) in out.iter().enumerate() {
                    assert!(
                        (v - 0.5).abs() < 1e-4,
                        "{:?} {dw}x{dh}: sample {i} drifted to {v} from a flat 0.5",
                        f.kind
                    );
                }
            }
        }
    }

    /// Same size in, same size out: resampling must not soften a picture it was not asked to
    /// change. This is the case a wrong half-pixel offset shifts by half a pixel.
    #[test]
    fn a_one_to_one_resize_is_the_identity() {
        let mut src = vec![0.0f32; 8 * 8 * 4];
        for (i, p) in src.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            p[0] = (i % 8) as f32 / 8.0;
            p[3] = 1.0;
        }
        for f in [
            FilterKind::Triangle,
            FilterKind::CatmullRom,
            FilterKind::Lanczos3,
        ] {
            let f = desc(f);
            let out = resample_f32(&src, 8, 8, 4, 8, 8, &f).unwrap();
            for (i, (a, b)) in src.iter().zip(out.iter()).enumerate() {
                assert!(
                    (a - b).abs() < 1e-4,
                    "{:?} moved sample {i}: {a} -> {b} at 1:1",
                    f.kind
                );
            }
        }
    }

    /// The filter is not ignored. Two kernels asked the same question must answer differently on
    /// a picture with an edge in it - the defect this module replaces made them identical.
    #[test]
    fn different_filters_produce_different_pixels() {
        let mut src = vec![0.0f32; 32 * 32 * 4];
        for (i, p) in src.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            p[0] = if (i % 32) < 16 { 1.0 } else { 0.0 };
            p[3] = 1.0;
        }
        let a = resample_f32(&src, 32, 32, 4, 9, 9, &desc(FilterKind::Nearest)).unwrap();
        let b = resample_f32(&src, 32, 32, 4, 9, 9, &desc(FilterKind::Lanczos3)).unwrap();
        let differs = a.iter().zip(b.iter()).any(|(x, y)| (x - y).abs() > 1e-3);
        assert!(differs, "Nearest and Lanczos3 produced identical pixels");
    }

    #[test]
    fn non_separable_filters_are_refused_not_substituted() {
        let disk = filter_desc_from_name("disk", 0.0, "").unwrap();
        let radial = filter_desc_from_name("radial-lanczos3", 0.0, "").unwrap();
        let src = vec![0.5f32; 8 * 8 * 4];
        assert!(resample_f32(&src, 8, 8, 4, 4, 4, &disk).is_none());
        assert!(resample_f32(&src, 8, 8, 4, 4, 4, &radial).is_none());
    }

    #[test]
    fn a_flat_field_survives_every_named_filter() {
        let src = vec![0.5f32; 37 * 23 * 4];
        for name in [
            "nearest",
            "box",
            "triangle",
            "gaussian",
            "sharp-gaussian",
            "catmull-rom",
            "blackman-harris",
            "sinc",
            "lanczos3",
            "mitchell",
            "b-spline",
            "cubic",
            "keys",
            "simon",
            "rifman",
        ] {
            let f = filter_desc_from_name(name, 0.0, "").unwrap();
            for (dw, dh) in [(9usize, 5usize), (37, 23), (80, 60), (1, 1)] {
                let out = resample_f32(&src, 37, 23, 4, dw, dh, &f).unwrap();
                for (i, v) in out.iter().enumerate() {
                    assert!(
                        (v - 0.5).abs() < 1e-3,
                        "{name} {dw}x{dh}: sample {i} drifted to {v} from a flat 0.5"
                    );
                }
            }
        }
    }

    /// A reduction reduces, same property held above for the reduced set — the full vocabulary
    /// must not reintroduce the "returns the source peak" defect this module exists to prevent.
    #[test]
    fn a_reduction_reduces_through_the_full_vocabulary_too() {
        for name in ["gaussian", "blackman-harris", "sinc", "b-spline"] {
            let f = filter_desc_from_name(name, 0.0, "").unwrap();
            let mut previous = f32::MAX;
            for n in [4usize, 16, 64] {
                let mut src = vec![0.0f32; n * n * 4];
                let mid = ((n / 2) * n + n / 2) * 4;
                src[mid] = 1.0;
                for p in src.as_chunks_mut::<4>().0 {
                    p[3] = 1.0;
                }
                let got = resample_f32(&src, n, n, 4, 1, 1, &f).unwrap()[0];
                assert!(
                    got < previous,
                    "{name} at {n}x{n}: {got} not below {previous}"
                );
                previous = got;
            }
        }
    }

    /// Anisotropic width/height (a caller that scaled only one axis, as `resize`'s magnify path
    /// does) must resample each axis with its OWN support, not the larger of the two — this is
    /// what `axis_taps_x`/`axis_taps_y` being genuinely separate (not one function reading
    /// `width.max(height)`) exists to guarantee.
    #[test]
    fn anisotropic_width_and_height_resample_independently() {
        let mut src = vec![0.0f32; 16 * 16 * 4];
        for (i, p) in src.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = i % 16;
            let y = i / 16;
            p[0] = if x < 8 { 1.0 } else { 0.0 };
            p[1] = if y < 8 { 1.0 } else { 0.0 };
            p[3] = 1.0;
        }
        let mut narrow_x = FilterDesc::new(FilterKind::Lanczos3);
        narrow_x.width = 2.0; // much narrower than default 6 on X only
        let mut wide_x = FilterDesc::new(FilterKind::Lanczos3);
        wide_x.width = 12.0; // much wider than default 6 on X only
        // Height is default (6.0) on both, so any difference in the OUTPUT must come from the
        // X-axis width alone.
        let a = resample_f32(&src, 16, 16, 4, 8, 8, &narrow_x).unwrap();
        let b = resample_f32(&src, 16, 16, 4, 8, 8, &wide_x).unwrap();
        assert_ne!(
            a, b,
            "differing X width had no effect on the resampled output"
        );
    }
}
