//! Alpha association (premultiply/un-premultiply) — the single implementation
//! shared by every crate in this workspace that needs it.
//!
//! OIIO's convention: in-memory pixels are ASSOCIATED alpha (premultiplied,
//! `RGB` already scaled by `A`). Formats that store UNASSOCIATED (straight)
//! alpha on disk convert on the way in and out. The *core* half of that
//! conversion — the actual per-pixel arithmetic, independent of any file
//! format — is [`OIIO::premult`](https://github.com/AcademySoftwareFoundation/OpenImageIO)
//! (`libOpenImageIO/imageio.cpp:1280-1310`, `premult_impl`):
//!
//! ```text
//! for each pixel:
//!     alpha = val[alpha_channel]
//!     for c in chbegin..chend, c != alpha_channel, c != z_channel:
//!         val[c] = alpha * val[c]
//! ```
//!
//! Two things worth calling out about that reference, since they shape the
//! API here:
//! - There is **no zero-alpha special case** in `premult_impl` — multiplying
//!   by `alpha == 0` is always safe (it just zeroes the channel), so
//!   [`associate_alpha`] doesn't special-case it either.
//! - Un-association has two rules in OIIO, and both live here (ROADMAP B15/G4;
//!   the workspace used to carry four, each slightly different):
//!   - [`unpremult_value`] / [`unpremult_pixel`] / [`unassociate_alpha`]:
//!     `ImageBufAlgo::unpremult` (`imagebufalgo_pixelmath.cpp` `unpremult_`).
//!     `alpha == 0` or `alpha == 1` leaves the pixel alone; any other alpha,
//!     negative and denormal included, DIVIDES (`r / alpha`, not
//!     `r * (1 / alpha)`: the reciprocal of a denormal overflows to `inf`).
//!     The Z channel is never touched. The format plugins' `deassociateAlpha`
//!     (`pngoutput.cpp`, `targaoutput.cpp`) guard the same `alpha == 0`.
//!   - [`color_op_alpha`]: the colour operations with `unpremult=true`
//!     (`color_ocio.cpp` `colorconvert_impl`: `colorconvert`,
//!     `colormatrixtransform`, `ociodisplay`, `ociolook`, `ociofiletransform`,
//!     `ocionamedtransform`). Alpha below `FLT_MIN` (zero, denormal, negative)
//!     counts as 1; RGB is divided by it before the transform and multiplied by
//!     the same (original) value after.
//!
//! Per-pixel value conversion goes through [`PixelFormat::to_f32`]/
//! [`PixelFormat::from_f32`] — the same normalized-`[0,1]` + `quantize_u8`/
//! `quantize_u16` (`+0.5`-before-truncate) convention already used
//! everywhere else in this crate, matching OIIO's own `DataArrayProxy<T,
//! float>` (the `T`<->`float` proxy `premult_impl` reads/writes through).
//!
//! This is the SSOT for this math in the workspace: `vfx-ops`'s
//! `premultiply`/`unpremultiply` (fixed 4-channel f32 RGBA, used by
//! compositing) and `vfx-io`'s per-format reader/writer alpha-association
//! policy (`vfx-io/src/alpha.rs`, gating on `oiio:UnassociatedAlpha`) both
//! call through to these functions rather than reimplementing the loop.

use crate::pixel::PixelFormat;

/// Associates (premultiplies) alpha into every non-alpha channel of an
/// interleaved pixel buffer, in place: `channel *= alpha`.
///
/// `data.len()` must be a multiple of `channels`; `alpha_index` must be
/// `< channels`. Out-of-range `alpha_index`, or `channels < 2` (nothing to
/// associate into), is a no-op — mirrors `OIIO::premult`'s own
/// `alpha_channel < 0 || alpha_channel > nchannels` early return.
///
/// # Example
///
/// ```
/// use vfx_core::alpha::associate_alpha;
///
/// // One RGBA pixel, alpha = 0.5.
/// let mut px = [1.0f32, 0.8, 0.4, 0.5];
/// associate_alpha(&mut px, 4, 3);
/// assert_eq!(px, [0.5, 0.4, 0.2, 0.5]);
/// ```
pub fn associate_alpha<T: PixelFormat>(data: &mut [T], channels: usize, alpha_index: usize) {
    if channels < 2 || alpha_index >= channels {
        return;
    }
    for px in data.chunks_exact_mut(channels) {
        let alpha = px[alpha_index].to_f32();
        for (c, v) in px.iter_mut().enumerate() {
            if c == alpha_index {
                continue;
            }
            *v = T::from_f32(v.to_f32() * alpha);
        }
    }
}

/// `ImageBufAlgo::unpremult`'s rule for one value (`imagebufalgo_pixelmath.cpp`
/// `unpremult_`): `alpha == 0` or `alpha == 1` returns `v`, otherwise `v / alpha`.
///
/// ```
/// use vfx_core::alpha::unpremult_value;
/// assert_eq!(unpremult_value(0.3, 0.0), 0.3);
/// assert_eq!(unpremult_value(0.3, 0.5), 0.6);
/// assert_eq!(unpremult_value(0.3, -0.5), -0.6);
/// assert!(unpremult_value(0.3, 1e-39).is_finite()); // divides; `0.3 * (1.0 / 1e-39)` is inf
/// ```
#[inline]
#[must_use]
pub fn unpremult_value(v: f32, alpha: f32) -> f32 {
    if alpha == 0.0 || alpha == 1.0 {
        v
    } else {
        v / alpha
    }
}

/// [`unpremult_value`] over one interleaved `f32` pixel in place, skipping the alpha channel and
/// the Z channel (`ImageBufAlgo::unpremult` excludes `z_channel`: depth is not coverage-scaled).
/// An out-of-range `alpha_index` is a no-op.
pub fn unpremult_pixel(px: &mut [f32], alpha_index: usize, z_index: Option<usize>) {
    let Some(&alpha) = px.get(alpha_index) else {
        return;
    };
    for (c, v) in px.iter_mut().enumerate() {
        if c != alpha_index && Some(c) != z_index {
            *v = unpremult_value(*v, alpha);
        }
    }
}

/// The alpha OIIO's colour operations divide by before the transform and multiply by after,
/// when asked to unpremultiply (`color_ocio.cpp` `colorconvert_impl`: `a >= FLT_MIN ? a : 1`).
/// Zero, denormal and negative alpha count as 1, so those pixels keep their colour.
///
/// ```
/// use vfx_core::alpha::color_op_alpha;
/// assert_eq!(color_op_alpha(0.25), 0.25);
/// assert_eq!(color_op_alpha(0.0), 1.0);
/// assert_eq!(color_op_alpha(-0.5), 1.0);
/// assert_eq!(color_op_alpha(1e-39), 1.0);
/// ```
#[inline]
#[must_use]
pub fn color_op_alpha(alpha: f32) -> f32 {
    if alpha >= f32::MIN_POSITIVE {
        alpha
    } else {
        1.0
    }
}

/// Un-associates (un-premultiplies) alpha out of every non-alpha channel of
/// an interleaved pixel buffer, in place, by [`unpremult_value`]'s rule.
///
/// Same shape/no-op rules as [`associate_alpha`]. `alpha == 0` leaves the
/// pixel's other channels untouched — matches `ImageBufAlgo::unpremult` and
/// every OIIO plugin's own `deassociateAlpha` guard (`pngoutput.cpp`,
/// `targaoutput.cpp`).
///
/// # Example
///
/// ```
/// use vfx_core::alpha::{associate_alpha, unassociate_alpha};
///
/// let mut px = [0.9f32, 0.3, 0.1, 0.6];
/// let original = px;
/// associate_alpha(&mut px, 4, 3);
/// unassociate_alpha(&mut px, 4, 3);
/// for (a, b) in px.iter().zip(original.iter()) {
///     assert!((a - b).abs() < 1e-5);
/// }
/// ```
pub fn unassociate_alpha<T: PixelFormat>(data: &mut [T], channels: usize, alpha_index: usize) {
    if channels < 2 || alpha_index >= channels {
        return;
    }
    for px in data.chunks_exact_mut(channels) {
        let alpha = px[alpha_index].to_f32();
        if alpha == 0.0 || alpha == 1.0 {
            continue;
        }
        for (c, v) in px.iter_mut().enumerate() {
            if c == alpha_index {
                continue;
            }
            *v = T::from_f32(unpremult_value(v.to_f32(), alpha));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use half::f16;

    #[test]
    fn associate_u8_half_alpha_halves_rgb() {
        let mut px: [u8; 4] = [255, 128, 64, 128];
        associate_alpha(&mut px, 4, 3);
        // alpha=128/255≈0.502; 255*0.502≈128, 128*0.502≈64, 64*0.502≈32
        assert_eq!(px, [128, 64, 32, 128]);
    }

    #[test]
    fn associate_then_unassociate_u8_round_trips_within_one_lsb() {
        let original: [u8; 4] = [200, 100, 50, 180];
        let mut px = original;
        associate_alpha(&mut px, 4, 3);
        unassociate_alpha(&mut px, 4, 3);
        for (a, b) in px.iter().zip(original.iter()) {
            assert!((*a as i32 - *b as i32).abs() <= 1, "{} vs {}", a, b);
        }
    }

    #[test]
    fn associate_full_alpha_is_a_no_op() {
        let mut px: [u8; 4] = [10, 20, 30, 255];
        associate_alpha(&mut px, 4, 3);
        assert_eq!(px, [10, 20, 30, 255]);
    }

    #[test]
    fn associate_zero_alpha_zeroes_other_channels() {
        let mut px: [u8; 4] = [200, 150, 100, 0];
        associate_alpha(&mut px, 4, 3);
        assert_eq!(px, [0, 0, 0, 0]);
    }

    /// `oiiotool rgba.exr --unpremult` on (0.3, 0.2, 0.1, a): see vfx-io
    /// `tests/alpha_oiio_parity.rs` for the full fixture.
    #[test]
    fn unpremult_matches_oiio_rule() {
        let cases: [([f32; 4], [f32; 4]); 4] = [
            ([0.3, 0.2, 0.1, 0.5], [0.6, 0.4, 0.2, 0.5]),
            ([0.3, 0.2, 0.1, 0.0], [0.3, 0.2, 0.1, 0.0]),
            ([0.3, 0.2, 0.1, -0.5], [-0.6, -0.4, -0.2, -0.5]),
            ([0.9, 0.25, 0.04, 0.25], [3.6, 1.0, 0.16, 0.25]),
        ];
        for (src, want) in cases {
            let mut px = src;
            unassociate_alpha(&mut px, 4, 3);
            assert_eq!(px, want, "{src:?}");
            let mut px = src;
            unpremult_pixel(&mut px, 3, None);
            assert_eq!(px, want, "{src:?}");
        }
        // Denormal alpha divides (finite, ~3e38) instead of multiplying by an infinite reciprocal.
        let mut px = [0.3f32, 0.2, 0.1, 1e-39];
        unpremult_pixel(&mut px, 3, None);
        assert!(px[..3].iter().all(|v| v.is_finite() && *v > 1e37), "{px:?}");
        // Z is not coverage.
        let mut px = [0.3f32, 0.2, 0.1, 0.5, 7.0];
        unpremult_pixel(&mut px, 3, Some(4));
        assert_eq!(px[4], 7.0);
    }

    #[test]
    fn unassociate_zero_alpha_leaves_channels_untouched() {
        let mut px: [u8; 4] = [7, 8, 9, 0];
        unassociate_alpha(&mut px, 4, 3);
        assert_eq!(px, [7, 8, 9, 0]);
    }

    #[test]
    fn associate_f32_matches_direct_multiply() {
        let mut px: [f32; 4] = [1.0, 0.5, 0.25, 0.5];
        associate_alpha(&mut px, 4, 3);
        assert_eq!(px, [0.5, 0.25, 0.125, 0.5]);
    }

    #[test]
    fn associate_f16_matches_direct_multiply() {
        let mut px: [f16; 4] = [
            f16::from_f32(1.0),
            f16::from_f32(0.5),
            f16::from_f32(0.25),
            f16::from_f32(0.5),
        ];
        associate_alpha(&mut px, 4, 3);
        assert!((px[0].to_f32() - 0.5).abs() < 1e-3);
        assert!((px[1].to_f32() - 0.25).abs() < 1e-3);
        assert!((px[2].to_f32() - 0.125).abs() < 1e-3);
    }

    #[test]
    fn single_channel_alpha_only_buffer_is_untouched() {
        let mut px: [u8; 3] = [10, 20, 30];
        associate_alpha(&mut px, 1, 0);
        assert_eq!(px, [10, 20, 30]);
    }

    #[test]
    fn out_of_range_alpha_index_is_a_no_op() {
        let mut px: [u8; 4] = [10, 20, 30, 128];
        associate_alpha(&mut px, 4, 9);
        assert_eq!(px, [10, 20, 30, 128]);
    }

    #[test]
    fn alpha_not_last_channel_still_associates_correctly() {
        // Alpha at index 0 instead of the usual last-channel convention —
        // the kernel is index-driven, not last-channel-assuming.
        let mut px: [u8; 4] = [128, 255, 128, 64];
        associate_alpha(&mut px, 4, 0);
        assert_eq!(px[0], 128); // alpha itself untouched
        assert_eq!(px[1], 128); // 255 * 0.502 ≈ 128
        assert_eq!(px[2], 64); // 128 * 0.502 ≈ 64
        assert_eq!(px[3], 32); // 64 * 0.502 ≈ 32
    }

    #[test]
    fn two_pixel_buffer_associates_each_pixel_independently() {
        let mut px: [u8; 8] = [255, 0, 0, 128, 0, 255, 0, 255];
        associate_alpha(&mut px, 4, 3);
        assert_eq!(&px[0..4], &[128, 0, 0, 128]);
        assert_eq!(&px[4..8], &[0, 255, 0, 255]);
    }
}
