//! Half-domain 1D LUT on the GPU, written once for all four languages: port of OCIO
//! `Lut1DOpGPU.cpp` (`GetLut1DGPUShaderProgram` with `isInputHalfDomain()`).
//!
//! The 65536 entries (one per half code) live in a 2D texture `width` x `height` whose rows
//! overlap by one texel (the last texel of a row repeats the first of the next,
//! [`padded_texture`]), so bilinear filtering along a row interpolates between adjacent codes.
//! The shader rebuilds each channel's half code from its float value (exponent and mantissa, or
//! the denormal step below `HALF_NRM_MIN`, plus 32768 for negatives) and samples at that code.
//! This is exactly OCIO's forward evaluation; the inverse is baked into such a table first
//! (`ProcessorOp::gpu_baked_inverse`, OCIO `MakeFastLut1DFromInverse`).

use super::dialect::{Dialect, n};

/// OCIO's default `GpuShaderCreator::getTextureMaxWidth()`.
pub const MAX_WIDTH: usize = 4096;

/// Smallest normal half (`HALF_NRM_MIN`, 2^-14).
const HALF_NRM_MIN: f64 = 6.103_515_625e-5;
/// Largest finite half (`HALF_MAX`).
const HALF_MAX: f64 = 65504.0;

/// Texture size for a table of `len` entries: OCIO `width = min(len, maxWidth)`,
/// `height = len / maxWidth + 1`.
pub fn texture_size(len: usize) -> (usize, usize) {
    (len.min(MAX_WIDTH), len / MAX_WIDTH + 1)
}

/// The RGBA texture data for `lut` (`channels` interleaved per entry, 1 or 3), padded as OCIO
/// `CreatePaddedLutChannels` / `CreatePaddedRedChannel` (a mono table is replicated to RGB).
pub fn padded_texture(lut: &[f32], channels: usize) -> (Vec<f32>, usize, usize) {
    let len = lut.len() / channels;
    let (width, height) = texture_size(len);
    let rgb = |i: usize| -> [f32; 3] {
        let sanitize = |v: f32| {
            if v.is_nan() {
                0.0
            } else {
                v.clamp(-f32::MAX, f32::MAX)
            }
        };
        std::array::from_fn(|c| sanitize(lut[i * channels + c.min(channels - 1)]))
    };
    let mut rows: Vec<[f32; 3]> = Vec::with_capacity(width * height);
    if height > 1 {
        let step = width - 1;
        let mut leftover = len;
        let mut i = 0;
        while i < len - step {
            rows.extend((i..=i + step).map(rgb));
            leftover -= step;
            i += step;
        }
        if leftover > 0 {
            rows.extend((len - leftover..len).map(rgb));
        }
    } else {
        rows.extend((0..len).map(rgb));
    }
    let last = rgb(len - 1);
    rows.resize(width * height, last);
    let data = rows.iter().flat_map(|p| [p[0], p[1], p[2], 1.0]).collect();
    (data, width, height)
}

/// Shader lines for one half-domain Lut1D op. `sample(uv)` is this language's 2D texture read
/// (a four-component expression) at a two-component coordinate expression.
pub fn emit(
    d: &Dialect,
    width: usize,
    height: usize,
    sample: &dyn Fn(&str) -> String,
) -> Vec<String> {
    let v2 = d.v2;
    let (w, h, step) = (width as f64, height as f64, (width - 1) as f64);
    let mut out = vec!["{ // Lut1D, half domain (OCIO Lut1DOpGPU)".to_string()];
    for c in ["r", "g", "b"] {
        let f = format!("color.{c}");
        out.push(d.decl_f(&format!("abs_{c}"), &format!("abs({f})")));
        out.push(d.decl_f(
            &format!("absarr_{c}"),
            &format!("min(abs_{c}, {})", n(HALF_MAX)),
        ));
        // floor(log2(x)) off by one at a power of two still gives the same exponent + mantissa
        // sum; the argument is floored so the unused denormal branch stays finite.
        out.push(d.decl_f(
            &format!("e_{c}"),
            &format!("floor(log2(max(absarr_{c}, {})))", n(HALF_NRM_MIN)),
        ));
        out.push(d.decl_f(&format!("lower_{c}"), &format!("exp2(e_{c})")));
        let normal = format!("(e_{c} + (absarr_{c} - lower_{c}) / lower_{c} + 15.0) * 1024.0");
        let denormal = format!("abs_{c} * 16777216.0");
        out.push(d.decl_f(
            &format!("dep_{c}"),
            &format!(
                "{} + {}",
                d.sel(
                    &format!("abs_{c} > {}", n(HALF_NRM_MIN)),
                    &normal,
                    &denormal
                ),
                d.sel(&format!("{f} < 0.0"), "32768.0", "0.0")
            ),
        ));
        out.push(d.decl_f(
            &format!("row_{c}"),
            &format!("floor(dep_{c} / {})", n(step)),
        ));
        out.push(d.decl_f(
            &format!("col_{c}"),
            &format!("dep_{c} - row_{c} * {}", n(step)),
        ));
        out.push(d.decl_f(
            &format!("out_{c}"),
            &format!(
                "{}.{c}",
                sample(&format!(
                    "{v2}((col_{c} + 0.5) / {}, (row_{c} + 0.5) / {})",
                    n(w),
                    n(h)
                ))
            ),
        ));
    }
    out.push(d.set(&format!("{}(out_r, out_g, out_b)", d.v3)));
    out.push("}".to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OCIO's padding: 65536 entries in a 4096 x 17 texture, row `k` starts at entry `4095 k`,
    /// and the tail is padded with the last entry.
    #[test]
    fn padding_is_ocios() {
        let lut: Vec<f32> = (0..65536).map(|i| i as f32).collect();
        let (data, w, h) = padded_texture(&lut, 1);
        assert_eq!((w, h), (4096, 17));
        assert_eq!(data.len(), 4096 * 17 * 4);
        let texel = |x: usize, y: usize| data[(y * w + x) * 4];
        assert_eq!(texel(0, 1), 4095.0);
        assert_eq!(texel(4095, 0), 4095.0);
        assert_eq!(texel(5, 16), (16 * 4095 + 5) as f32);
        assert_eq!(texel(4095, 16), 65535.0);
    }

    #[test]
    fn wgsl_body_parses() {
        let lines = emit(&super::super::dialect::WGSL, 4096, 17, &|uv| {
            format!("textureSampleLevel(t, t_sampler, {uv}, 0.0)")
        });
        let src = format!(
            "@group(0) @binding(0) var t: texture_2d<f32>;\n@group(0) @binding(1) var t_sampler: sampler;\nfn f(c: vec4<f32>) -> vec4<f32> {{\n var color = c;\n{}\n return color;\n}}\n",
            lines.join("\n")
        );
        let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{e}\n{src}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}\n{src}"));
    }
}
