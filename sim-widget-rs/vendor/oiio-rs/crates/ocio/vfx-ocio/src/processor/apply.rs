//! Pixel application: walk the compiled `ProcessorOp` list and transform pixels.
//!
//! Extracted from `processor.rs` during STRUCT.2.next (Step 3/3). The
//! methods here only **read** `self.ops` and write into caller-owned
//! pixel buffers — no mutation of `Processor` state, no `compile_*`
//! calls. Naturally decoupled from `processor/compile.rs` and
//! `processor/accessors.rs`.
//!
//! Public surface (unchanged location, just moved to a sibling file):
//! - `apply_rgb` / `apply_rgba` — safe, op-outer bulk dispatchers (PLAN9.md O9: for each op in
//!   turn, apply it to every pixel, rather than looping every op per pixel — a fixed op inside
//!   the inner loop is what actually lets LLVM auto-vectorise across pixels)
//! - `apply_packed` / `apply_planar` — unsafe, stride-walking bulk paths; visit one pixel at a
//!   time (arbitrary stride precludes the op-outer shape), through `apply_one_stored`
//!
//! Alpha follows OCIO (B28): `apply_rgba` and images with an alpha channel/plane run
//! `apply_op_to_rgba`, where the 4x4 matrix and the exponent ops read and write alpha; RGB-only
//! paths are OCIO's alpha-0 case.
//!
//! Private inner kernels:
//! - `apply_op_to_rgb` (~1600 LOC) — the master match over `ProcessorOp`, one arm per op kind;
//!   a pure function of `(op, pixel)`, called once per (op, pixel) pair by every path above
//! - `apply_op_to_rgba` — the alpha-aware wrapper around it
//! - `apply_one_stored` — loads, walks `self.ops` and stores one pixel of a strided image

use super::*;

impl super::Processor {
    /// Applies the transform to RGB pixels in-place.
    ///
    /// Iterates op-OUTER, pixel-INNER (PLAN9.md O9): for each op in turn, apply it to every
    /// pixel, rather than looping every op per pixel (what `apply_one_stored` still does, for the
    /// stride-walking `apply_packed`/`apply_planar` paths that visit one pixel at a time).
    /// Identical result either way - each op is a pure per-pixel function, see
    /// `apply_op_to_rgb` - but this shape gives LLVM a single, statically-known op across the
    /// whole inner loop instead of a per-pixel `match`, and (with the `parallel` feature) lets
    /// the pixel loop run across threads: every op reads only `op` and its one `pixel`, so there
    /// is no cross-pixel state to race on.
    pub fn apply_rgb(&self, pixels: &mut [[f32; 3]]) {
        for op in &self.ops {
            #[cfg(feature = "parallel")]
            {
                use rayon::prelude::*;
                pixels
                    .par_iter_mut()
                    .for_each(|pixel| Self::apply_op_to_rgb(op, pixel));
            }
            #[cfg(not(feature = "parallel"))]
            {
                for pixel in pixels.iter_mut() {
                    Self::apply_op_to_rgb(op, pixel);
                }
            }
        }
    }

    /// Applies the transform to RGBA pixels in-place, with OCIO's alpha semantics: the 4x4
    /// matrix and the exponent ops read and write alpha, every other op passes it through
    /// (see `apply_op_to_rgba`). Op-outer for the same reason as `apply_rgb`.
    pub fn apply_rgba(&self, pixels: &mut [[f32; 4]]) {
        for op in &self.ops {
            #[cfg(feature = "parallel")]
            {
                use rayon::prelude::*;
                pixels
                    .par_iter_mut()
                    .for_each(|pixel| Self::apply_op_to_rgba(op, pixel));
            }
            #[cfg(not(feature = "parallel"))]
            {
                for pixel in pixels.iter_mut() {
                    Self::apply_op_to_rgba(op, pixel);
                }
            }
        }
    }

    // ========================================================================
    // Stride-aware apply methods (OCIO parity)
    // ========================================================================

    /// Apply the transform to a packed image in a byte buffer (OCIO `CPUProcessor::apply` with a
    /// `PackedImageDesc`).
    ///
    /// The sample of channel `c` of pixel `(x, y)` is at byte
    /// `origin + y*y_stride + x*x_stride + index(c)*chan_stride` of `data`, `index` from
    /// `chan_order`; strides are in bytes and may be negative (a bottom-up image), alignment is not
    /// required. Every addressed sample must lie inside `data`: this is checked once, before any
    /// pixel is touched, and an image that does not fit is an error.
    ///
    /// Integer depths are normalised by their maximum on read and written back rounded to nearest
    /// and clamped, as OCIO's `Converter<>::CastValue` does (`BitDepthUtils.h`). With an alpha
    /// channel in `chan_order` the pixel is processed as RGBA (see `apply_rgba`); without one, as
    /// RGB with alpha 0, like OCIO.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_packed(
        &self,
        data: &mut [u8],
        origin: usize,
        width: usize,
        height: usize,
        chan_order: ChannelOrdering,
        bit_depth: PackedBitDepth,
        chan_stride: isize,
        x_stride: isize,
        y_stride: isize,
    ) -> OcioResult<()> {
        let mut chans = vec![
            chan_order.r_index() as isize * chan_stride,
            chan_order.g_index() as isize * chan_stride,
            chan_order.b_index() as isize * chan_stride,
        ];
        let has_alpha = chan_order.a_index().is_some();
        if let Some(a) = chan_order.a_index() {
            chans.push(a as isize * chan_stride);
        }
        let layout = Layout::new(
            data.len(),
            origin,
            width,
            height,
            x_stride,
            y_stride,
            &chans,
            bit_depth,
        )?;
        for y in 0..height {
            for x in 0..width {
                let at = chans
                    .iter()
                    .map(|&c| layout.at(x, y, c))
                    .collect::<Vec<_>>();
                let mut px = [0.0f32; 4];
                for (v, &o) in px.iter_mut().zip(&at) {
                    *v = load_sample(&data[o..], bit_depth);
                }
                self.apply_one(&mut px, has_alpha);
                for (&v, &o) in px.iter().zip(&at) {
                    store_sample(&mut data[o..], bit_depth, v);
                }
            }
        }
        Ok(())
    }

    /// Apply the transform to a planar image (OCIO `CPUProcessor::apply` with a
    /// `PlanarImageDesc`): one byte buffer per channel, the sample of pixel `(x, y)` at byte
    /// `origin + y*y_stride + x*x_stride` of each (strides in bytes, possibly negative).
    ///
    /// With an alpha plane the pixel is processed as RGBA (see `apply_rgba`), otherwise as RGB with
    /// alpha 0, like OCIO. Integer depths are handled as in [`Self::apply_packed`]; every plane
    /// must hold every addressed sample (checked before any pixel is touched).
    #[allow(clippy::too_many_arguments)]
    pub fn apply_planar(
        &self,
        rgb: [&mut [u8]; 3],
        alpha: Option<&mut [u8]>,
        origin: usize,
        width: usize,
        height: usize,
        bit_depth: PackedBitDepth,
        x_stride: isize,
        y_stride: isize,
    ) -> OcioResult<()> {
        let [r, g, b] = rgb;
        let mut planes: Vec<&mut [u8]> = vec![r, g, b];
        let has_alpha = alpha.is_some();
        planes.extend(alpha);
        let layouts = planes
            .iter()
            .map(|p| {
                Layout::new(
                    p.len(),
                    origin,
                    width,
                    height,
                    x_stride,
                    y_stride,
                    &[0],
                    bit_depth,
                )
            })
            .collect::<OcioResult<Vec<_>>>()?;
        for y in 0..height {
            for x in 0..width {
                let mut px = [0.0f32; 4];
                for ((v, p), l) in px.iter_mut().zip(&planes).zip(&layouts) {
                    *v = load_sample(&p[l.at(x, y, 0)..], bit_depth);
                }
                self.apply_one(&mut px, has_alpha);
                for ((&v, p), l) in px.iter().zip(planes.iter_mut()).zip(&layouts) {
                    store_sample(&mut p[l.at(x, y, 0)..], bit_depth, v);
                }
            }
        }
        Ok(())
    }

    /// Runs every op on one loaded pixel: RGBA when the image has alpha, otherwise RGB with alpha
    /// 0 (what OCIO's ops see for an image without alpha: PyOCIO 2.5.1 on 3-channel images,
    /// `CPUProcessor::applyRGB`).
    #[inline]
    fn apply_one(&self, px: &mut [f32; 4], has_alpha: bool) {
        if has_alpha {
            for op in &self.ops {
                Self::apply_op_to_rgba(op, px);
            }
        } else {
            let mut rgb = [px[0], px[1], px[2]];
            for op in &self.ops {
                Self::apply_op_to_rgb(op, &mut rgb);
            }
            px[..3].copy_from_slice(&rgb);
        }
    }

    /// Applies one op to an RGBA pixel. OCIO's CPU ops that touch alpha are the matrix
    /// (`MatrixOpCPU.cpp`: the full 4x4 plus offset) and the gamma op (`GammaOpCPU.cpp`: channel 3
    /// with its own exponent / moncurve parameters and the same negative style); every other op
    /// copies `in[3]` to `out[3]`, so it runs on RGB with alpha set aside.
    fn apply_op_to_rgba(op: &ProcessorOp, px: &mut [f32; 4]) {
        match op {
            ProcessorOp::Matrix { matrix, offset } => {
                let v = *px;
                for (r, out) in px.iter_mut().enumerate() {
                    *out = v[0] * matrix[r * 4]
                        + v[1] * matrix[r * 4 + 1]
                        + v[2] * matrix[r * 4 + 2]
                        + v[3] * matrix[r * 4 + 3]
                        + offset[r];
                }
            }
            _ => {
                let mut rgb = [px[0], px[1], px[2]];
                Self::apply_op_to_rgb(op, &mut rgb);
                px[..3].copy_from_slice(&rgb);
                px[3] = match op {
                    ProcessorOp::Exponent {
                        value,
                        negative_style,
                    } => exponent_channel(px[3], value[3], *negative_style),
                    ProcessorOp::ExponentWithLinear {
                        gamma,
                        offset,
                        mirror,
                        forward,
                    } => moncurve_channel(px[3], gamma[3], offset[3], *mirror, *forward),
                    _ => px[3],
                };
            }
        }
    }

    /// Applies a single [`ProcessorOp`] to one RGB pixel (PLAN9.md O9). Extracted out of
    /// the per-pixel op loop so the bulk `apply_rgb`/`apply_rgba` paths can
    /// iterate op-OUTER instead of pixel-outer: a fixed, statically-known op inside the inner
    /// loop lets LLVM auto-vectorise across pixels, where a per-pixel `match` over the whole op
    /// list could not. Pure function of `op` and `pixel` - the match body below reads no
    /// `Processor` state (verified before this extraction: zero `self.` references anywhere in
    /// it), so lifting it out of the method changes nothing about what it computes.
    pub(crate) fn apply_op_to_rgb(op: &ProcessorOp, pixel: &mut [f32; 3]) {
        match op {
            ProcessorOp::Matrix { matrix, offset } => {
                // Matrix op (OCIO parity).
                // Row-major 4x4 layout: out[i] = sum(in[j] * matrix[i*4+j]) + offset[i]
                let [r, g, b] = *pixel;
                pixel[0] = r * matrix[0] + g * matrix[1] + b * matrix[2] + offset[0];
                pixel[1] = r * matrix[4] + g * matrix[5] + b * matrix[6] + offset[1];
                pixel[2] = r * matrix[8] + g * matrix[9] + b * matrix[10] + offset[2];
            }

            // THE CDL is `vfx_lut::cdl`, bit-exact with OpenColorIO's CPU (ROADMAP G7,
            // `vfx-lut/tests/cdl_pyocio.rs`). The fields are the forward parameters in both
            // directions; `inverse`'s op sequence is what the GPU emitters render.
            ProcessorOp::Cdl {
                slope,
                offset,
                power,
                saturation,
                style,
                inverse,
            } => {
                use vfx_lut::cdl;
                let f = match (style, inverse.is_some()) {
                    (CdlStyle::AscCdl, false) => cdl::apply_cdl_clamped,
                    (CdlStyle::NoClamp, false) => cdl::apply_cdl_unclamped,
                    (CdlStyle::AscCdl, true) => cdl::apply_cdl_inverse_clamped,
                    (CdlStyle::NoClamp, true) => cdl::apply_cdl_inverse_unclamped,
                };
                f(pixel, *slope, *offset, *power, *saturation);
            }

            ProcessorOp::Exponent {
                value,
                negative_style,
            } => {
                for (i, v) in pixel.iter_mut().enumerate() {
                    *v = exponent_channel(*v, value[i], *negative_style);
                }
            }

            ProcessorOp::Log { base, forward } => {
                // Simple Log op (OCIO parity).
                // Forward: out = log(base, max(LOG_MIN, in))
                // Inverse: out = pow(base, in)
                if *forward {
                    for v in pixel.iter_mut() {
                        *v = v.max(vfx_core::consts::LOG_MIN).log(*base);
                    }
                } else {
                    for v in pixel.iter_mut() {
                        *v = base.powf(*v);
                    }
                }
            }

            ProcessorOp::Range {
                scale,
                offset,
                clamp_min,
                clamp_max,
            } => {
                // Range op (OCIO parity).
                // out = clamp(scale * in + offset, min, max)
                for v in pixel.iter_mut() {
                    *v = *v * scale + offset;
                    if let Some(min) = clamp_min {
                        *v = v.max(*min);
                    }
                    if let Some(max) = clamp_max {
                        *v = v.min(*max);
                    }
                }
            }

            ProcessorOp::Lut1d {
                lut,
                size,
                channels,
                domain_min,
                domain_max,
                half_domain,
                hue_adjust,
                inverse,
                ..
            } => {
                if let Some(inv) = inverse {
                    for (c, v) in pixel.iter_mut().enumerate() {
                        *v = inv.eval(c, *v);
                    }
                    return;
                }
                // 1D LUT op (OCIO parity) - linear interpolation.
                // Per-channel domain support with clamping to [domain_min, domain_max].
                let orig_rgb = [pixel[0], pixel[1], pixel[2]];
                for (i, v) in pixel.iter_mut().enumerate() {
                    let ch = if *channels == 1 { 0 } else { i };
                    if *half_domain {
                        // Lut1DRendererHalfCode::apply (Lut1DOpCPU.cpp:531-560): interpolate
                        // between the two half codes that bracket the input.
                        let (a, b, frac) = super::math::half_domain_edges(*v);
                        let (va, vb) = (lut[a * channels + ch], lut[b * channels + ch]);
                        // `lerpf(lut[B], lut[A], 1 - fraction)`: interpolating from the far
                        // end keeps +/-inf * 0 out of the result.
                        *v = (va - vb) * (1.0 - frac) + vb;
                        continue;
                    }
                    let ch_idx = i.min(2); // Clamp to RGB (ignore alpha domain)
                    let d_min = domain_min[ch_idx];
                    let d_max = domain_max[ch_idx];
                    let range = d_max - d_min;
                    let scale = if range.abs() < 1e-10 {
                        0.0
                    } else {
                        (*size - 1) as f32 / range
                    };
                    let idx = ((*v - d_min) * scale).clamp(0.0, (*size - 1) as f32);
                    let idx_floor = idx.floor() as usize;
                    let idx_ceil = (idx_floor + 1).min(*size - 1);
                    let frac = idx - idx_floor as f32;

                    let v0 = lut[idx_floor * channels + ch];
                    let v1 = lut[idx_ceil * channels + ch];
                    *v = v0 + (v1 - v0) * frac;
                }

                if *channels == 3 && matches!(hue_adjust, crate::transform::Lut1DHueAdjust::Dw3) {
                    let order3 = |rgb: [f32; 3]| -> (usize, usize, usize) {
                        let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
                        let min = if r <= g {
                            if r <= b { 0 } else { 2 }
                        } else {
                            if g <= b { 1 } else { 2 }
                        };
                        let max = if r >= g {
                            if r >= b { 0 } else { 2 }
                        } else {
                            if g >= b { 1 } else { 2 }
                        };
                        let mid = 3 - min - max;
                        (min, mid, max)
                    };

                    let (min_idx, mid_idx, max_idx) = order3(orig_rgb);
                    let orig_chroma = orig_rgb[max_idx] - orig_rgb[min_idx];
                    let hue_factor = if orig_chroma.abs() < 1e-10 {
                        0.0
                    } else {
                        (orig_rgb[mid_idx] - orig_rgb[min_idx]) / orig_chroma
                    };

                    let new_chroma = pixel[max_idx] - pixel[min_idx];
                    pixel[mid_idx] = hue_factor * new_chroma + pixel[min_idx];
                }
            }

            ProcessorOp::ExposureContrast {
                exposure,
                contrast,
                gamma,
                pivot,
                log_exposure_step,
                log_mid_gray,
                style,
                forward,
                ..
            } => {
                // ExposureContrast (OCIO `ExposureContrastOpCPU.cpp`). The parameters are the
                // FORWARD ones; the reverse renderers (`ECLinearRevRenderer` and friends) are
                // the exact inverse of the forward ones, built from the same numbers:
                //   Linear/Video fwd  out = pow(max(0, in * scale / pivot), c) * pivot
                //                rev  out = pow(max(0, in / pivot), 1/c) * pivot / scale
                //   Logarithmic  fwd  out = in * c + (E*step - lp) * c + lp
                //                rev  out = in / c + lp - lp/c - E*step
                // with c = max(MIN_CONTRAST, contrast * gamma), scale = 2^E (Video: raised to
                // VIDEO_OETF_POWER, as is the pivot), lp the log pivot. OCIO's CPU Logarithmic
                // reverse never reads the op's `log_exposure_step` (its renderer keeps the
                // 0.088 default, `ExposureContrastOpCPU.cpp:634-672`), while its GPU shader
                // does; this is the GPU's, the one that really inverts the forward.
                use vfx_core::consts::exposure_contrast::{
                    MIN_CONTRAST, MIN_PIVOT, VIDEO_OETF_POWER,
                };

                let exp_mult = 2.0_f32.powf(*exposure);
                let c = (*contrast * *gamma).max(MIN_CONTRAST);
                // Every style divides by the pivot; OCIO floors it first
                // (`ExposureContrastOpData.h` `EC::MIN_PIVOT`).
                let pivot = pivot.max(MIN_PIVOT);

                match style {
                    ExposureContrastStyle::Linear | ExposureContrastStyle::Video => {
                        let (scale, piv) = if *style == ExposureContrastStyle::Linear {
                            (exp_mult, pivot)
                        } else {
                            (
                                exp_mult.powf(VIDEO_OETF_POWER),
                                pivot.powf(VIDEO_OETF_POWER),
                            )
                        };
                        // OCIO's renderers (and its GPU shader) take a plain scale when the
                        // contrast is exactly 1, which keeps negative values; the power path
                        // clamps them to 0. An exposure-only grade is that case.
                        match (*forward, c == 1.0) {
                            (true, true) => pixel.iter_mut().for_each(|v| *v *= scale),
                            (false, true) => pixel.iter_mut().for_each(|v| *v /= scale),
                            (true, false) => pixel
                                .iter_mut()
                                .for_each(|v| *v = (*v * scale / piv).max(0.0).powf(c) * piv),
                            (false, false) => pixel.iter_mut().for_each(|v| {
                                *v = (*v / piv).max(0.0).powf(1.0 / c) * piv / scale;
                            }),
                        }
                    }
                    ExposureContrastStyle::Logarithmic => {
                        let log_pivot =
                            ((pivot / 0.18).log2() * log_exposure_step + log_mid_gray).max(0.0);
                        let exposure_val = *exposure * log_exposure_step;
                        if *forward {
                            let offset = (exposure_val - log_pivot) * c + log_pivot;
                            pixel.iter_mut().for_each(|v| *v = *v * c + offset);
                        } else {
                            let offset = log_pivot - log_pivot / c - exposure_val;
                            pixel.iter_mut().for_each(|v| *v = *v / c + offset);
                        }
                    }
                }
            }
            ProcessorOp::FixedFunction {
                style,
                params,
                forward,
            } => {
                match style {
                    FixedFunctionStyle::RgbToHsv => super::ff_color::rgb_to_hsv(pixel),
                    FixedFunctionStyle::HsvToRgb => super::ff_color::hsv_to_rgb(pixel),
                    FixedFunctionStyle::AcesGamutComp13 => {
                        // Renderer_ACES_GamutComp13_Fwd/Inv (FixedFunctionOpCPU.cpp:984-1055).
                        let p = super::op::GamutComp13Params::from_params(params);
                        let ach = pixel[0].max(pixel[1]).max(pixel[2]);
                        for (c, v) in pixel.iter_mut().enumerate() {
                            *v = p.apply(*v, ach, c, *forward);
                        }
                    }
                    FixedFunctionStyle::AcesRedMod03 | FixedFunctionStyle::AcesRedMod10 => {
                        // ACES Red Modifier — ported from OCIO FixedFunctionOpCPU.cpp
                        // RedMod03: width=120deg, scale=0.85, has hue restoration
                        // RedMod10: width=135deg, scale=0.82, no hue restoration
                        //
                        // Sourced from vfx_core::consts::aces1, not hardcoded here: that
                        // module exists BECAUSE flattening `4.0 / (120deg in radians)` into
                        // the literal `1.909859...` is exactly how this workspace's worst
                        // constant drift happened elsewhere (see its module docs). This
                        // arm used to be one of the places doing that flattening.
                        use vfx_core::consts::aces1::{red_mod_03, red_mod_10};
                        let (one_minus_scale, pivot, inv_width, has_hue_restore) = match style {
                                FixedFunctionStyle::AcesRedMod03 => {
                                    (red_mod_03::ONE_MINUS_SCALE, red_mod_03::PIVOT, red_mod_03::INV_WIDTH, true)
                                }
                                _ /* RedMod10 */ => {
                                    (red_mod_10::ONE_MINUS_SCALE, red_mod_10::PIVOT, red_mod_10::INV_WIDTH, false)
                                }
                            };
                        const SQRT3: f32 = 1.732_050_8;
                        let [r, mut g, mut b] = *pixel;

                        // Hue weight via quadratic B-spline (OCIO CalcHueWeight)
                        let a = 2.0 * r - (g + b);
                        let bb = SQRT3 * (g - b);
                        let hue = bb.atan2(a);
                        let knot_coord = hue * inv_width + 2.0;
                        let j = knot_coord.floor() as i32;
                        #[allow(clippy::excessive_precision)]
                        const BASIS: [[f32; 4]; 4] = [
                            [0.25, 0.00, 0.00, 0.00],
                            [-0.75, 0.75, 0.75, 0.25],
                            [0.75, -1.50, 0.00, 1.00],
                            [-0.25, 0.75, -0.75, 0.25],
                        ];
                        let f_h = if (0..4).contains(&j) {
                            let t = knot_coord - j as f32;
                            let c = &BASIS[j as usize];
                            c[3] + t * (c[2] + t * (c[1] + t * c[0]))
                        } else {
                            0.0
                        };

                        if f_h > 0.0 {
                            // Saturation weight (OCIO CalcSatWeight)
                            let max_v = r.max(g).max(b);
                            let min_v = r.min(g).min(b);
                            let f_s = (max_v.max(1e-10) - min_v.max(1e-10)) / max_v.max(1e-2);

                            let new_red = if *forward {
                                // Forward: red = red + f_H * f_S * (pivot - red) * (1 - scale)
                                r + f_h * f_s * (pivot - r) * one_minus_scale
                            } else {
                                // Inverse: solve quadratic (OCIO uses minChan in formula)
                                let min_chan = g.min(b);
                                let aa = f_h * one_minus_scale - 1.0;
                                let bb = r - f_h * (pivot + min_chan) * one_minus_scale;
                                let cc = f_h * pivot * min_chan * one_minus_scale;
                                let disc = bb * bb - 4.0 * aa * cc;
                                // No clamp: OCIO takes `sqrt` of the raw discriminant
                                // (`FixedFunctionOpCPU.cpp:634,737`), a NaN when negative.
                                (-bb - disc.sqrt()) / (2.0 * aa)
                            };

                            // Hue restoration for RedMod03 only (adjusts G or B)
                            if has_hue_restore {
                                if g >= b {
                                    // red >= grn >= blu
                                    let hue_fac = (g - b) / (r - b).max(1e-10);
                                    g = hue_fac * (new_red - b) + b;
                                } else {
                                    // red >= blu >= grn
                                    let hue_fac = (b - g) / (r - g).max(1e-10);
                                    b = hue_fac * (new_red - g) + g;
                                }
                            }

                            pixel[0] = new_red;
                            pixel[1] = g;
                            pixel[2] = b;
                        }
                    }
                    FixedFunctionStyle::AcesGlow03 | FixedFunctionStyle::AcesGlow10 => {
                        // ACES Glow — ported from OCIO FixedFunctionOpCPU.cpp
                        use vfx_core::consts::aces1::{glow_03, glow_10};
                        let (glow_gain, glow_mid) = match style {
                                FixedFunctionStyle::AcesGlow03 => (glow_03::GLOW_GAIN, glow_03::GLOW_MID),
                                _ /* Glow10 */ => (glow_10::GLOW_GAIN, glow_10::GLOW_MID),
                            };
                        const YC_RADIUS_WEIGHT: f32 = 1.75;
                        let [r, g, b] = *pixel;

                        // YC luma with chroma weighting (OCIO rgbToYC)
                        let chroma = (b * (b - g) + g * (g - r) + r * (r - b)).max(0.0).sqrt();
                        let yc = (r + g + b + YC_RADIUS_WEIGHT * chroma) / 3.0;

                        // Saturation weight (OCIO CalcSatWeight)
                        let max_v = r.max(g).max(b);
                        let min_v = r.min(g).min(b);
                        let sat = (max_v.max(1e-10) - min_v.max(1e-10)) / max_v.max(1e-2);

                        // Sigmoid shaper (OCIO SigmoidShaper)
                        let x = (sat - 0.4) * 5.0;
                        let sign = x.signum();
                        let t = (1.0 - 0.5 * sign * x).max(0.0);
                        let s = (1.0 + sign * (1.0 - t * t)) * 0.5;

                        let glow_amount = glow_gain * s;

                        // Piecewise glow calculation - different for forward vs inverse!
                        let glow_out = if *forward {
                            // Forward glow
                            if yc >= glow_mid * 2.0 {
                                0.0
                            } else if yc <= glow_mid * 2.0 / 3.0 {
                                glow_amount
                            } else {
                                glow_amount * (glow_mid / yc - 0.5)
                            }
                        } else {
                            // Inverse glow (OCIO has different thresholds and formula!)
                            if yc >= glow_mid * 2.0 {
                                0.0
                            } else if yc <= (1.0 + glow_amount) * glow_mid * 2.0 / 3.0 {
                                -glow_amount / (1.0 + glow_amount)
                            } else {
                                glow_amount * (glow_mid / yc - 0.5) / (glow_amount * 0.5 - 1.0)
                            }
                        };

                        let multiplier = 1.0 + glow_out;
                        pixel[0] = r * multiplier;
                        pixel[1] = g * multiplier;
                        pixel[2] = b * multiplier;
                    }
                    FixedFunctionStyle::XyzToXyy => super::ff_color::xyz_to_xyy(pixel),
                    FixedFunctionStyle::XyyToXyz => super::ff_color::xyy_to_xyz(pixel),
                    FixedFunctionStyle::XyzToUvy => super::ff_color::xyz_to_uvy(pixel),
                    FixedFunctionStyle::UvyToXyz => super::ff_color::uvy_to_xyz(pixel),
                    FixedFunctionStyle::XyzToLuv => super::ff_color::xyz_to_luv(pixel),
                    FixedFunctionStyle::LuvToXyz => super::ff_color::luv_to_xyz(pixel),
                    FixedFunctionStyle::AcesDarkToDim10 => {
                        // AP1 luminance coefficients
                        const AP1_Y: [f32; 3] = [0.272_228_72, 0.674_081_74, 0.053_689_517];
                        let gamma = params.first().copied().unwrap_or(0.9811);
                        let m_gamma = if *forward {
                            gamma - 1.0
                        } else {
                            (1.0 / gamma) - 1.0
                        };
                        let [r, g, b] = *pixel;
                        let y = (AP1_Y[0] * r + AP1_Y[1] * g + AP1_Y[2] * b).max(1e-10);
                        let ypow = y.powf(m_gamma);
                        pixel[0] = r * ypow;
                        pixel[1] = g * ypow;
                        pixel[2] = b * ypow;
                    }
                    FixedFunctionStyle::Rec2100Surround => {
                        const REC2100_Y: [f32; 3] = [0.2627, 0.6780, 0.0593];
                        let gamma_in = params.first().copied().unwrap_or(0.78);
                        let (gamma, min_lum) = if *forward {
                            (gamma_in, 1e-4_f32)
                        } else {
                            (1.0 / gamma_in, (1e-4_f32).powf(gamma_in))
                        };
                        let m_gamma = gamma - 1.0;
                        let [r, g, b] = *pixel;
                        let y = (REC2100_Y[0] * r + REC2100_Y[1] * g + REC2100_Y[2] * b)
                            .abs()
                            .max(min_lum);
                        let ypow = y.powf(m_gamma);
                        pixel[0] = r * ypow;
                        pixel[1] = g * ypow;
                        pixel[2] = b * ypow;
                    }
                    FixedFunctionStyle::LinToPq => {
                        // OCIO LIN_TO_PQ: 1.0 = 100 nits, mirrored, unclamped. The curve is
                        // `vfx_transfer::pq`'s; the OCIO convention is its `ocio_*` pair.
                        for v in pixel.iter_mut() {
                            *v = vfx_transfer::pq::ocio_lin_to_pq(*v);
                        }
                    }
                    FixedFunctionStyle::PqToLin => {
                        for v in pixel.iter_mut() {
                            *v = vfx_transfer::pq::ocio_pq_to_lin(*v);
                        }
                    }
                    FixedFunctionStyle::LinToGammaLog => {
                        // Renderer_LIN_TO_GAMMA_LOG (FixedFunctionOpCPU.cpp:2230-2277).
                        let p = super::op::GammaLogParams::from_params(params);
                        for v in pixel.iter_mut() {
                            *v = p.lin_to_gamma_log(*v);
                        }
                    }
                    FixedFunctionStyle::GammaLogToLin => {
                        // Renderer_GAMMA_LOG_TO_LIN (FixedFunctionOpCPU.cpp:2279-2320).
                        let p = super::op::GammaLogParams::from_params(params);
                        for v in pixel.iter_mut() {
                            *v = p.gamma_log_to_lin(*v);
                        }
                    }
                    FixedFunctionStyle::LinToDoubleLog => {
                        // Renderer_LIN_TO_DOUBLE_LOG (FixedFunctionOpCPU.cpp:2322-2376).
                        let p = super::op::DoubleLogParams::from_params(params);
                        for v in pixel.iter_mut() {
                            *v = p.lin_to_double_log(*v);
                        }
                    }
                    FixedFunctionStyle::DoubleLogToLin => {
                        // Renderer_DOUBLE_LOG_TO_LIN (FixedFunctionOpCPU.cpp:2378-2420).
                        let p = super::op::DoubleLogParams::from_params(params);
                        for v in pixel.iter_mut() {
                            *v = p.double_log_to_lin(*v);
                        }
                    }
                    FixedFunctionStyle::RgbToHsyLin
                    | FixedFunctionStyle::RgbToHsyLog
                    | FixedFunctionStyle::RgbToHsyVid => {
                        // Each style is its own saturation model (Lin x1.4 / Log x4 / Vid
                        // x1.25); they are NOT interchangeable. The shared CPU HSY in
                        // `crate::hsy` is pinned against the GPU emitter.
                        use crate::hsy::HsyVariant;
                        let variant = match style {
                            FixedFunctionStyle::RgbToHsyLin => HsyVariant::Lin,
                            FixedFunctionStyle::RgbToHsyLog => HsyVariant::Log,
                            _ => HsyVariant::Vid,
                        };
                        *pixel = crate::hsy::rgb_to_hsy(*pixel, variant);
                    }
                    FixedFunctionStyle::HsyLinToRgb
                    | FixedFunctionStyle::HsyLogToRgb
                    | FixedFunctionStyle::HsyVidToRgb => {
                        use crate::hsy::HsyVariant;
                        let variant = match style {
                            FixedFunctionStyle::HsyLinToRgb => HsyVariant::Lin,
                            FixedFunctionStyle::HsyLogToRgb => HsyVariant::Log,
                            _ => HsyVariant::Vid,
                        };
                        *pixel = crate::hsy::hsy_to_rgb(*pixel, variant);
                    }
                    // ACES 2.0 styles handled by dedicated ProcessorOp variants
                    FixedFunctionStyle::AcesOutputTransform20
                    | FixedFunctionStyle::AcesRgbToJmh20
                    | FixedFunctionStyle::AcesJmhToRgb20
                    | FixedFunctionStyle::AcesTonescaleCompress20
                    | FixedFunctionStyle::AcesGamutCompress20 => {
                        unreachable!("ACES 2.0 ops use dedicated ProcessorOp variants");
                    }
                }
            }

            ProcessorOp::Aces2OutputTransform { state, forward } => {
                if *forward {
                    *pixel = crate::aces2::output_transform_fwd(pixel, state);
                } else {
                    *pixel = crate::aces2::output_transform_inv(pixel, state);
                }
            }

            ProcessorOp::Aces2RgbJmh { params, forward } => {
                if *forward {
                    *pixel = crate::aces2::rgb_to_jmh_degrees(pixel, params);
                } else {
                    *pixel = crate::aces2::jmh_degrees_to_rgb(pixel, params);
                }
            }

            ProcessorOp::Aces2TonescaleCompress {
                jmh_params,
                ts_params,
                shared,
                chroma,
                forward,
            } => {
                if *forward {
                    *pixel = crate::aces2::tonescale_compress_fwd(
                        pixel, jmh_params, ts_params, shared, chroma,
                    );
                } else {
                    *pixel = crate::aces2::tonescale_compress_inv(
                        pixel, jmh_params, ts_params, shared, chroma,
                    );
                }
            }

            ProcessorOp::Aces2GamutCompress {
                shared,
                gamut,
                forward,
            } => {
                if *forward {
                    *pixel = crate::aces2::gamut_compress_fwd_deg(pixel, shared, gamut);
                } else {
                    *pixel = crate::aces2::gamut_compress_inv_deg(pixel, shared, gamut);
                }
            }

            ProcessorOp::Allocation {
                allocation,
                vars,
                forward,
            } => {
                // Allocation op (OCIO parity).
                // Uniform: linear fit from [min, max] to [0, 1]
                // Log2: log2() then fit to [0, 1]
                let (min_val, max_val) = match allocation {
                    AllocationType::Unknown => unreachable!("refused when the transform compiles"),
                    AllocationType::Uniform => (
                        vars.first().copied().unwrap_or(0.0),
                        vars.get(1).copied().unwrap_or(1.0),
                    ),
                    AllocationType::Log2 => (
                        vars.first().copied().unwrap_or(-10.0),
                        vars.get(1).copied().unwrap_or(6.0),
                    ),
                };
                let lin_offset = vars.get(2).copied().unwrap_or(0.0);

                match allocation {
                    AllocationType::Unknown => unreachable!("refused when the transform compiles"),
                    AllocationType::Uniform => {
                        // OCIO: CreateFitOp from [min, max] to [0, 1]
                        if *forward {
                            for v in pixel.iter_mut() {
                                *v = (*v - min_val) / (max_val - min_val);
                            }
                        } else {
                            for v in pixel.iter_mut() {
                                *v = *v * (max_val - min_val) + min_val;
                            }
                        }
                    }
                    AllocationType::Log2 => {
                        // OCIO: Log2 + FitOp
                        if *forward {
                            for v in pixel.iter_mut() {
                                let log_v = (*v + lin_offset).max(vfx_core::consts::LOG_MIN).log2();
                                *v = (log_v - min_val) / (max_val - min_val);
                            }
                        } else {
                            for v in pixel.iter_mut() {
                                let log_v = *v * (max_val - min_val) + min_val;
                                *v = 2.0_f32.powf(log_v) - lin_offset;
                            }
                        }
                    }
                }
            }

            ProcessorOp::GradingPrimary {
                style,
                brightness,
                contrast,
                gamma,
                exposure,
                offset,
                slope,
                saturation,
                pivot,
                pivot_black,
                pivot_white,
                clamp_black,
                clamp_white,
                forward,
                ..
            } => {
                // OCIO GradingPrimary CPU implementation with exact parity.
                //
                // Key insight from OCIO GradingPrimaryPreRender::update():
                // - Forward gamma = 1/user_gamma (inverted once at precompute)
                // - Inverse gamma = user_gamma (NOT inverted, uses original)
                //
                // Forward order: ops -> saturation -> clamp
                // Inverse order: clamp -> saturation(1/sat) -> ops reversed

                // Helper for gamma with pivot normalization
                let apply_gamma = |pix: &mut [f32; 3], gam: &[f32; 3], pb: f32, pw: f32| {
                    let range = pw - pb;
                    for (i, v) in pix.iter_mut().enumerate() {
                        let normalized = ((*v - pb).abs() / range).max(1e-10);
                        let sign = (*v - pb).signum();
                        *v = normalized.powf(gam[i]) * sign * range + pb;
                    }
                };

                // `gamma` is ALREADY direction-resolved by `op::grading_primary_params`
                // at compile time (forward: 1/(master*channel); inverse:
                // master*channel — matching OCIO's `GradingPrimaryPreRender::update`
                // exactly), so this must use it as-is. Re-inverting here on top of
                // the compile-time inversion would double-invert — the same bug
                // class OCIO-06 already fixed once for this op's own `inverse()`.
                let gam = *gamma;
                let gamma_active = gam.iter().any(|g| (*g - 1.0).abs() > 1e-6);

                if *forward {
                    // === FORWARD: ops -> saturation -> clamp ===
                    match style {
                        GradingPrimaryStyle::Log => {
                            // brightness -> contrast -> gamma
                            for (i, v) in pixel.iter_mut().enumerate() {
                                *v += brightness[i];
                            }
                            for (i, v) in pixel.iter_mut().enumerate() {
                                *v = *pivot + (*v - *pivot) * contrast[i];
                            }
                            if gamma_active {
                                apply_gamma(pixel, &gam, *pivot_black, *pivot_white);
                            }
                        }
                        GradingPrimaryStyle::Linear => {
                            // offset -> exposure -> contrast. `exposure` is
                            // ALREADY `2^stops` (`op::grading_primary_params`,
                            // matching OCIO's `comp.getExposure()` -
                            // `GradingPrimary.cpp:167-168`), so this must
                            // multiply directly, not `2^exposure[i]` again
                            // (that computed `2^(2^stops)` - found while
                            // fixing the same double-application class of
                            // bug this whole pass is about, just on the
                            // FORWARD side this time instead of inverse).
                            for (i, v) in pixel.iter_mut().enumerate() {
                                *v += offset[i];
                            }
                            for (i, v) in pixel.iter_mut().enumerate() {
                                *v *= exposure[i];
                            }
                            if contrast.iter().any(|c| (*c - 1.0).abs() > 1e-6) {
                                for (i, v) in pixel.iter_mut().enumerate() {
                                    let sign = v.signum();
                                    *v = (v.abs() / *pivot).powf(contrast[i]) * sign * *pivot;
                                }
                            }
                        }
                        GradingPrimaryStyle::Video => {
                            // offset -> slope -> gamma
                            for (i, v) in pixel.iter_mut().enumerate() {
                                *v += offset[i];
                            }
                            for (i, v) in pixel.iter_mut().enumerate() {
                                *v = (*v - *pivot_black) * slope[i] + *pivot_black;
                            }
                            if gamma_active {
                                apply_gamma(pixel, &gam, *pivot_black, *pivot_white);
                            }
                        }
                    }
                    // Saturation (OCIO always applies, even if sat=1.0)
                    let luma = pixel[0] * REC709_LUMA_R
                        + pixel[1] * REC709_LUMA_G
                        + pixel[2] * REC709_LUMA_B;
                    for v in pixel.iter_mut() {
                        *v = luma + (*v - luma) * *saturation;
                    }
                    // Clamp
                    for v in pixel.iter_mut() {
                        *v = v.clamp(*clamp_black, *clamp_white);
                    }
                } else {
                    // === INVERSE: clamp -> saturation(1/sat) -> ops reversed ===
                    // Clamp first
                    for v in pixel.iter_mut() {
                        *v = v.clamp(*clamp_black, *clamp_white);
                    }
                    // Saturation inverse (divide by sat)
                    if *saturation != 0.0 && (*saturation - 1.0).abs() > 1e-6 {
                        let sat_inv = 1.0 / *saturation;
                        let luma = pixel[0] * REC709_LUMA_R
                            + pixel[1] * REC709_LUMA_G
                            + pixel[2] * REC709_LUMA_B;
                        for v in pixel.iter_mut() {
                            *v = luma + (*v - luma) * sat_inv;
                        }
                    }
                    // `brightness`/`contrast`/`gamma`/`exposure`/`offset`/`slope` are
                    // ALREADY the direction-resolved inverse values (negated / reciprocal,
                    // as OCIO's `GradingPrimaryPreRender::update` computes them for
                    // `TRANSFORM_DIR_INVERSE`) — apply them with the SAME formula shape
                    // forward uses, not a second reciprocal/negation on top (that was the
                    // double-inversion this block used to have, same bug class as OCIO-06).
                    match style {
                        GradingPrimaryStyle::Log => {
                            // gamma -> contrast -> brightness (ADD, already negated)
                            if gamma_active {
                                apply_gamma(pixel, &gam, *pivot_black, *pivot_white);
                            }
                            for (i, v) in pixel.iter_mut().enumerate() {
                                *v = *pivot + (*v - *pivot) * contrast[i];
                            }
                            for (i, v) in pixel.iter_mut().enumerate() {
                                *v += brightness[i];
                            }
                        }
                        GradingPrimaryStyle::Linear => {
                            // contrast (ApplyLinContrast, already reciprocal) ->
                            // exposure (MULTIPLY, already 1/2^e) -> offset (ADD, already negated)
                            if contrast.iter().any(|c| (*c - 1.0).abs() > 1e-6) {
                                for (i, v) in pixel.iter_mut().enumerate() {
                                    let sign = v.signum();
                                    *v = (v.abs() / *pivot).powf(contrast[i]) * sign * *pivot;
                                }
                            }
                            for (i, v) in pixel.iter_mut().enumerate() {
                                *v *= exposure[i];
                            }
                            for (i, v) in pixel.iter_mut().enumerate() {
                                *v += offset[i];
                            }
                        }
                        GradingPrimaryStyle::Video => {
                            // gamma -> slope (already reciprocal) -> offset (ADD, already negated)
                            if gamma_active {
                                apply_gamma(pixel, &gam, *pivot_black, *pivot_white);
                            }
                            for (i, v) in pixel.iter_mut().enumerate() {
                                *v = (*v - *pivot_black) * slope[i] + *pivot_black;
                            }
                            for (i, v) in pixel.iter_mut().enumerate() {
                                *v += offset[i];
                            }
                        }
                    }
                }
            }

            ProcessorOp::GradingRgbCurve {
                style,
                curve_data,
                bypass_lin_to_log,
                forward,
                ..
            } => {
                use crate::grading_bspline::{eval_curve_fwd, eval_curve_rev};
                use crate::transform::GradingToneStyle;

                // OCIO parity: GradingRGBCurveOpCPU.cpp. A set of identity curves bypasses.
                if curve_data.local_bypass {
                    return;
                }

                // For Linear style without bypass, apply Lin->Log before curves.
                // The LogLin block comes from the single source, not a local re-typing.
                // This was a seventh copy of constants `vfx_core::consts` exists to own; its
                // module doc records that this exact block had 6+ copies and two of them had
                // drifted. These were verified bit-identical to the source before migrating
                // (including `BASE2`, a hand-typed `1.442_695` that is `std::f32::consts::
                // LOG2_E` to the ulp) — so this is drift prevention, not a numeric change.
                use vfx_core::consts::loglin::{BASE2, GAIN, M, OFFS, SHIFT, XBRK, YBRK};

                // Lin -> Log (LinLog, GradingRGBCurveOpCPU.cpp:185)
                let lin_to_log = |v: f32| -> f32 {
                    if v < XBRK {
                        v * GAIN + OFFS
                    } else {
                        BASE2 * ((v + SHIFT) * M).ln()
                    }
                };

                // Log -> Lin (LogLin, GradingRGBCurveOpCPU.cpp:215)
                let log_to_lin = |v: f32| -> f32 {
                    if v < YBRK {
                        (v - OFFS) / GAIN
                    } else {
                        2.0_f32.powf(v) * (0.18 + SHIFT) - SHIFT
                    }
                };

                let linear = *style == GradingToneStyle::Linear && !*bypass_lin_to_log;
                if linear {
                    for v in pixel.iter_mut() {
                        *v = lin_to_log(*v);
                    }
                }

                let d = curve_data;
                let fwd = |c: usize, x: f32| {
                    eval_curve_fwd(
                        c,
                        x,
                        x,
                        &d.knots_offsets,
                        &d.coefs_offsets,
                        &d.knots,
                        &d.coefs,
                    )
                };
                let rev = |c: usize, x: f32| {
                    eval_curve_rev(c, x, &d.knots_offsets, &d.coefs_offsets, &d.knots, &d.coefs)
                };
                // Curve slots: 0 = red, 1 = green, 2 = blue, 3 = master.
                const MASTER: usize = 3;
                for (c, v) in pixel.iter_mut().enumerate() {
                    *v = if *forward {
                        // eval(): per-channel curve, then master (:30-43).
                        fwd(MASTER, fwd(c, *v))
                    } else {
                        // evalRev(): master first, then the channel (:44-53).
                        rev(c, rev(MASTER, *v))
                    };
                }

                if linear {
                    for v in pixel.iter_mut() {
                        *v = log_to_lin(*v);
                    }
                }
            }

            ProcessorOp::GradingTone {
                value,
                pre_render,
                forward,
                ..
            } => {
                // Faithful OCIO GradingTone CPU op (GradingToneOpCPU.cpp). The
                // precomputed curve tables live in `pre_render`; lin<->log for
                // the Linear style is applied inside apply_forward/apply_inverse.
                if *forward {
                    pre_render.apply_forward(value, pixel);
                } else {
                    pre_render.apply_inverse(value, pixel);
                }
            }

            ProcessorOp::GradingHueCurve {
                curves,
                rgb_to_hsy,
                curve_data,
                draw_curve_only,
                forward,
                ..
            } => {
                // CPU and GPU share `curve_data` (fitted B-spline + user slopes).
                // Linear-lerp `curves` is metadata only (accessors / dynamic props).
                if *draw_curve_only {
                    // Ref GradingHueCurveOpCPU.cpp:190-192 — HUE_SAT, even when bypassed.
                    for c in pixel.iter_mut().take(3) {
                        *c = crate::grading_hue_curve::eval_draw_curve_only(curve_data, *c);
                    }
                } else {
                    let do_hsy = matches!(rgb_to_hsy, crate::transform::HsyTransformStyle::Default);
                    let mut rgb = [pixel[0], pixel[1], pixel[2]];
                    if *forward {
                        crate::grading_hue_curve::apply_hue_curves_fwd_data(
                            curves.style,
                            do_hsy,
                            curve_data,
                            &mut rgb,
                        );
                    } else {
                        crate::grading_hue_curve::apply_hue_curves_rev_data(
                            curves.style,
                            do_hsy,
                            curve_data,
                            &mut rgb,
                        );
                    }
                    pixel[0] = rgb[0];
                    pixel[1] = rgb[1];
                    pixel[2] = rgb[2];
                }
            }

            ProcessorOp::Transfer { style, forward } => {
                for v in pixel.iter_mut() {
                    *v = apply_transfer(*v, *style, *forward);
                }
            }

            ProcessorOp::Lut3d {
                lut,
                size,
                interp,
                domain_min,
                domain_max,
                inverse,
                ..
            } => {
                if let Some(inv) = inverse {
                    *pixel = inv.eval(*pixel);
                    return;
                }
                // Domain -> lattice coordinates, then the shared kernel with OCIO's
                // concrete interpolation. `lut` is red-fastest (`compile_lut3d`).
                let n = (*size - 1) as f32;
                let p: [f32; 3] = std::array::from_fn(|c| {
                    let range = domain_max[c] - domain_min[c];
                    if range.abs() < 1e-10 {
                        0.0
                    } else {
                        (pixel[c] - domain_min[c]) / range * n
                    }
                });
                let s = *size;
                *pixel = vfx_lut::interpolate_lattice(s, p, interp.lut3d_method(), |r, g, b| {
                    let i = (b * s * s + g * s + r) * 3;
                    [lut[i], lut[i + 1], lut[i + 2]]
                });
            }

            ProcessorOp::LogAffine {
                base,
                log_side_slope,
                log_side_offset,
                lin_side_slope,
                lin_side_offset,
                forward,
            } => {
                // LogAffine (OCIO parity).
                // Forward: out = log_slope * log(base, max(MIN, lin_slope * x + lin_offset)) + log_offset
                // Inverse: out = (pow(base, (x - log_offset) / log_slope) - lin_offset) / lin_slope
                let log_base = base.ln();

                for (i, v) in pixel.iter_mut().enumerate() {
                    let ch = i.min(2);
                    if *forward {
                        // OCIO: ApplyScale(m) → ApplyAdd(b) → ApplyMax(minValue) → ApplyLog2 → ApplyScale(klog) → ApplyAdd(kb)
                        let lin = (lin_side_slope[ch] * *v + lin_side_offset[ch])
                            .max(vfx_core::consts::LOG_MIN);
                        *v = log_side_slope[ch] * lin.ln() / log_base + log_side_offset[ch];
                    } else {
                        // OCIO: ApplyAdd(-kb) → ApplyScale(kinv) → ApplyExp2 → ApplyAdd(-b) → ApplyScale(minv)
                        let exp_arg = (*v - log_side_offset[ch]) / log_side_slope[ch];
                        let lin = base.powf(exp_arg) - lin_side_offset[ch];
                        *v = lin / lin_side_slope[ch];
                    }
                }
            }

            ProcessorOp::LogCamera {
                base,
                log_base,
                log_side_slope,
                log_side_offset,
                lin_side_slope,
                lin_side_offset,
                lin_side_break,
                linear_slope,
                log_side_break,
                linear_offset,
                forward,
            } => {
                // LogCamera (OCIO parity) - piecewise log with linear segment.
                // Forward: if x >= lin_break: log formula, else: linear_slope * x + linear_offset
                // Inverse: if in >= log_break: inverse log, else: (in - linear_offset) / linear_slope
                // Note: log_side_break and linear_offset are precomputed (OCIO updateData)
                for (i, v) in pixel.iter_mut().enumerate() {
                    let ch = i.min(2);
                    if *forward {
                        if *v >= lin_side_break[ch] {
                            // Log region
                            let lin = (lin_side_slope[ch] * *v + lin_side_offset[ch])
                                .max(vfx_core::consts::LOG_MIN);
                            *v = log_side_slope[ch] * lin.ln() / log_base + log_side_offset[ch];
                        } else {
                            // Linear region
                            *v = linear_slope[ch] * *v + linear_offset[ch];
                        }
                    } else {
                        // Inverse
                        if *v >= log_side_break[ch] {
                            // Inverse log region
                            let exp_arg = (*v - log_side_offset[ch]) / log_side_slope[ch];
                            let lin = base.powf(exp_arg) - lin_side_offset[ch];
                            *v = lin / lin_side_slope[ch];
                        } else {
                            // Inverse linear region
                            *v = (*v - linear_offset[ch]) / linear_slope[ch];
                        }
                    }
                }
            }

            ProcessorOp::ExponentWithLinear {
                gamma,
                offset,
                mirror,
                forward,
            } => {
                for (i, v) in pixel.iter_mut().enumerate() {
                    *v = moncurve_channel(*v, gamma[i], offset[i], *mirror, *forward);
                }
            }
        }
    }
}

/// Where the samples of a strided image lie in a buffer, validated once: `at` is then an in-range
/// byte index for every pixel and channel offset it was built with.
struct Layout {
    origin: isize,
    x_stride: isize,
    y_stride: isize,
}

impl Layout {
    #[allow(clippy::too_many_arguments)]
    fn new(
        len: usize,
        origin: usize,
        width: usize,
        height: usize,
        x_stride: isize,
        y_stride: isize,
        chan_offsets: &[isize],
        depth: PackedBitDepth,
    ) -> OcioResult<Self> {
        if depth == PackedBitDepth::Unknown {
            // OCIO refuses BIT_DEPTH_UNKNOWN in `CPUProcessor::apply` too.
            return Err(OcioError::Validation("image bit depth is unknown".into()));
        }
        let l = Self {
            origin: origin as isize,
            x_stride,
            y_stride,
        };
        if width == 0 || height == 0 {
            return Ok(l);
        }
        // The extremes of a linear function of (x, y, c) are at the corners.
        let (mut lo, mut hi) = (isize::MAX, isize::MIN);
        for x in [0, width - 1] {
            for y in [0, height - 1] {
                for &c in chan_offsets {
                    let o = l.origin + y as isize * y_stride + x as isize * x_stride + c;
                    lo = lo.min(o);
                    hi = hi.max(o);
                }
            }
        }
        let size = depth.bytes_per_channel() as isize;
        if lo < 0 || hi + size > len as isize {
            return Err(OcioError::Validation(format!(
                "image of {width}x{height} with strides ({x_stride}, {y_stride}) from byte {origin} \
                 addresses bytes {lo}..{} of a {len}-byte buffer",
                hi + size
            )));
        }
        Ok(l)
    }

    #[inline]
    fn at(&self, x: usize, y: usize, chan: isize) -> usize {
        (self.origin + y as isize * self.y_stride + x as isize * self.x_stride + chan) as usize
    }
}

/// Reads one packed sample (native byte order, any alignment) as a float, normalising integer
/// depths by their maximum.
#[inline]
fn load_sample(b: &[u8], depth: PackedBitDepth) -> f32 {
    let inv = 1.0 / depth.max_value();
    let u16_at = |b: &[u8]| u16::from_ne_bytes([b[0], b[1]]);
    match depth {
        PackedBitDepth::F32 | PackedBitDepth::Unknown => {
            f32::from_ne_bytes([b[0], b[1], b[2], b[3]])
        }
        PackedBitDepth::F16 => f16::from_bits(u16_at(b)).to_f32(),
        PackedBitDepth::Uint8 => f32::from(b[0]) * inv,
        PackedBitDepth::Uint10 | PackedBitDepth::Uint12 | PackedBitDepth::Uint16 => {
            f32::from(u16_at(b)) * inv
        }
    }
}

/// Writes one float as a packed sample. Integer depths round to nearest and clamp to
/// `[0, max]`, matching OCIO `Converter<>::CastValue` (`BitDepthUtils.h`: `CLAMP(v + 0.5, 0, max)`
/// on the already-scaled value).
#[inline]
fn store_sample(b: &mut [u8], depth: PackedBitDepth, v: f32) {
    let max = depth.max_value();
    let quantize = |v: f32| (v * max + 0.5).clamp(0.0, max);
    match depth {
        PackedBitDepth::F32 | PackedBitDepth::Unknown => b[..4].copy_from_slice(&v.to_ne_bytes()),
        PackedBitDepth::F16 => b[..2].copy_from_slice(&f16::from_f32(v).to_bits().to_ne_bytes()),
        PackedBitDepth::Uint8 => b[0] = quantize(v) as u8,
        PackedBitDepth::Uint10 | PackedBitDepth::Uint12 | PackedBitDepth::Uint16 => {
            b[..2].copy_from_slice(&(quantize(v) as u16).to_ne_bytes())
        }
    }
}

/// One channel of OCIO's basic gamma op (`GammaOpCPU.cpp` `GammaBasic*OpCPU`), the same for R, G,
/// B and alpha: CLAMP `pow(max(0, x), g)`, MIRROR `sign(x) * pow(|x|, g)`, PASS_THRU
/// `x > 0 ? pow(x, g) : x`. (`Linear` is not an OCIO basic style; it stays the mirror it was.)
fn exponent_channel(v: f32, g: f32, style: NegativeStyle) -> f32 {
    match style {
        NegativeStyle::Clamp => v.max(0.0).powf(g),
        NegativeStyle::Mirror | NegativeStyle::Linear => v.signum() * v.abs().powf(g),
        NegativeStyle::PassThru => {
            if v >= 0.0 {
                v.powf(g)
            } else {
                v
            }
        }
    }
}

/// One channel of OCIO's moncurve (`GammaOpCPU.cpp:525-860`); the parameters come from the one
/// derivation shared with the GPU emitters.
fn moncurve_channel(v: f32, gamma: f32, offset: f32, mirror: bool, forward: bool) -> f32 {
    let p = super::op::moncurve_params(gamma as f64, offset as f64, forward);
    if mirror {
        // MONCURVE_MIRROR: sign(x) * f(|x|).
        let r = super::op::moncurve_eval(v.abs(), &p, forward);
        if v < 0.0 { -r } else { r }
    } else {
        // MONCURVE (OCIO's default NEGATIVE_LINEAR): negatives lie below the break and take the
        // LINEAR segment. This used to mirror them, which agrees only while |x| stays below the
        // break: sRGB-encoding -0.93 gave -0.97 instead of OCIO's -10.92.
        super::op::moncurve_eval(v, &p, forward)
    }
}
