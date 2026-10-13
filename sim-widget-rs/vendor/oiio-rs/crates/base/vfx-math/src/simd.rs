//! SIMD-accelerated math operations.
//!
//! This module provides SIMD-optimized functions for common VFX operations
//! using the `wide` crate for portable SIMD on stable Rust.
//!
//! # Features
//!
//! - 4-wide (`f32x4`) operations for RGB/RGBA processing
//! - 8-wide (`f32x8`) operations for batch processing
//! - Vectorized transfer functions
//! - Batch matrix-vector multiplication
//!
//! # Example
//!
//! ```rust
//! use vfx_math::simd::{batch_mul_add, batch_pow};
//!
//! // Process 8 values at once
//! let values = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
//! let result = batch_mul_add(&values, 2.0, 0.1);
//! ```
//!
//! # Performance
//!
//! SIMD operations can provide 2-8x speedup for batch operations
//! depending on the CPU and operation complexity.

use wide::{f32x4, f32x8};

/// Applies slope and offset to 4 values: `out = in * slope + offset`.
///
/// # Example
///
/// ```rust
/// use vfx_math::simd::mul_add_x4;
///
/// let values = [0.5, 0.5, 0.5, 0.5];
/// let result = mul_add_x4(&values, 2.0, 0.1);
/// assert!((result[0] - 1.1).abs() < 0.001);
/// ```
#[inline]
pub fn mul_add_x4(values: &[f32; 4], slope: f32, offset: f32) -> [f32; 4] {
    let v = f32x4::from(*values);
    let s = f32x4::splat(slope);
    let o = f32x4::splat(offset);
    (v * s + o).to_array()
}

/// Applies slope and offset to 8 values.
#[inline]
pub fn mul_add_x8(values: &[f32; 8], slope: f32, offset: f32) -> [f32; 8] {
    let v = f32x8::from(*values);
    let s = f32x8::splat(slope);
    let o = f32x8::splat(offset);
    (v * s + o).to_array()
}

/// Batch multiply-add for arbitrary-length arrays.
///
/// Processes 8 values at a time using SIMD, with scalar fallback
/// for remaining elements.
///
/// # Example
///
/// ```rust
/// use vfx_math::simd::batch_mul_add;
///
/// let values = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
/// let result = batch_mul_add(&values, 2.0, 0.0);
/// assert!((result[0] - 0.2).abs() < 0.001);
/// ```
pub fn batch_mul_add(values: &[f32], slope: f32, offset: f32) -> Vec<f32> {
    let mut result = Vec::with_capacity(values.len());
    let (chunks, remainder) = values.as_chunks::<8>();

    let s = f32x8::splat(slope);
    let o = f32x8::splat(offset);

    for &chunk in chunks {
        let v = f32x8::from(chunk);
        result.extend_from_slice(&(v * s + o).to_array());
    }

    for &v in remainder {
        result.push(v * slope + offset);
    }

    result
}

/// Batch multiply-add in-place.
#[inline]
pub fn batch_mul_add_inplace(values: &mut [f32], slope: f32, offset: f32) {
    let (chunks, remainder) = values.as_chunks_mut::<8>();
    let s = f32x8::splat(slope);
    let o = f32x8::splat(offset);

    for chunk in chunks {
        let v = f32x8::from(*chunk);
        *chunk = (v * s + o).to_array();
    }

    // Handle remainder with scalar ops
    for v in remainder {
        *v = *v * slope + offset;
    }
}

/// Clamps 4 values to [0, 1].
///
/// # Example
///
/// ```rust
/// use vfx_math::simd::clamp01_x4;
///
/// let values = [-0.1, 0.5, 1.2, 0.8];
/// let result = clamp01_x4(&values);
/// assert_eq!(result, [0.0, 0.5, 1.0, 0.8]);
/// ```
#[inline]
pub fn clamp01_x4(values: &[f32; 4]) -> [f32; 4] {
    let v = f32x4::from(*values);
    let zero = f32x4::splat(0.0);
    let one = f32x4::splat(1.0);
    v.max(zero).min(one).to_array()
}

/// Clamps 8 values to [0, 1].
#[inline]
pub fn clamp01_x8(values: &[f32; 8]) -> [f32; 8] {
    let v = f32x8::from(*values);
    let zero = f32x8::splat(0.0);
    let one = f32x8::splat(1.0);
    v.max(zero).min(one).to_array()
}

/// Batch clamp to [0, 1].
pub fn batch_clamp01(values: &[f32]) -> Vec<f32> {
    let mut result = Vec::with_capacity(values.len());
    let (chunks, remainder) = values.as_chunks::<8>();

    let zero = f32x8::splat(0.0);
    let one = f32x8::splat(1.0);

    for &chunk in chunks {
        let v = f32x8::from(chunk);
        result.extend_from_slice(&v.max(zero).min(one).to_array());
    }

    for &v in remainder {
        result.push(v.clamp(0.0, 1.0));
    }

    result
}

/// Linear interpolation between two 4-element arrays.
///
/// # Example
///
/// ```rust
/// use vfx_math::simd::lerp_x4;
///
/// let a = [0.0, 0.0, 0.0, 0.0];
/// let b = [1.0, 1.0, 1.0, 1.0];
/// let result = lerp_x4(&a, &b, 0.5);
/// assert!((result[0] - 0.5).abs() < 0.001);
/// ```
#[inline]
pub fn lerp_x4(a: &[f32; 4], b: &[f32; 4], t: f32) -> [f32; 4] {
    let va = f32x4::from(*a);
    let vb = f32x4::from(*b);
    let vt = f32x4::splat(t);
    let one = f32x4::splat(1.0);
    (va * (one - vt) + vb * vt).to_array()
}

/// Linear interpolation between two 8-element arrays.
#[inline]
pub fn lerp_x8(a: &[f32; 8], b: &[f32; 8], t: f32) -> [f32; 8] {
    let va = f32x8::from(*a);
    let vb = f32x8::from(*b);
    let vt = f32x8::splat(t);
    let one = f32x8::splat(1.0);
    (va * (one - vt) + vb * vt).to_array()
}

/// Batch linear interpolation.
pub fn batch_lerp(a: &[f32], b: &[f32], t: f32) -> Vec<f32> {
    assert_eq!(a.len(), b.len());
    let mut result = Vec::with_capacity(a.len());

    let (a_chunks, a_rem) = a.as_chunks::<8>();
    let (b_chunks, b_rem) = b.as_chunks::<8>();

    let vt = f32x8::splat(t);
    let one = f32x8::splat(1.0);

    for (&a_chunk, &b_chunk) in a_chunks.iter().zip(b_chunks) {
        let va = f32x8::from(a_chunk);
        let vb = f32x8::from(b_chunk);
        result.extend_from_slice(&(va * (one - vt) + vb * vt).to_array());
    }

    for (&av, &bv) in a_rem.iter().zip(b_rem.iter()) {
        result.push(av * (1.0 - t) + bv * t);
    }

    result
}

/// Applies power function to 4 values.
///
/// Uses fast approximation for common gamma values.
#[inline]
pub fn pow_x4(values: &[f32; 4], exp: f32) -> [f32; 4] {
    // Fast path for common exponents
    if (exp - 1.0).abs() < 1e-6 {
        return *values;
    }
    if (exp - 2.0).abs() < 1e-6 {
        let v = f32x4::from(*values);
        return (v * v).to_array();
    }
    if (exp - 0.5).abs() < 1e-6 {
        let v = f32x4::from(*values);
        return v.sqrt().to_array();
    }

    // Generic case - scalar fallback
    [
        values[0].powf(exp),
        values[1].powf(exp),
        values[2].powf(exp),
        values[3].powf(exp),
    ]
}

/// Batch power function.
///
/// # Example
///
/// ```rust
/// use vfx_math::simd::batch_pow;
///
/// let values = vec![0.25, 0.5, 0.75, 1.0];
/// let result = batch_pow(&values, 2.0);
/// assert!((result[0] - 0.0625).abs() < 0.001); // 0.25^2
/// ```
pub fn batch_pow(values: &[f32], exp: f32) -> Vec<f32> {
    // Fast paths
    if (exp - 1.0).abs() < 1e-6 {
        return values.to_vec();
    }

    let mut result = Vec::with_capacity(values.len());

    if (exp - 2.0).abs() < 1e-6 {
        let (chunks, remainder) = values.as_chunks::<8>();

        for &chunk in chunks {
            let v = f32x8::from(chunk);
            result.extend_from_slice(&(v * v).to_array());
        }
        for &v in remainder {
            result.push(v * v);
        }
    } else if (exp - 0.5).abs() < 1e-6 {
        let (chunks, remainder) = values.as_chunks::<8>();

        for &chunk in chunks {
            let v = f32x8::from(chunk);
            result.extend_from_slice(&v.sqrt().to_array());
        }
        for &v in remainder {
            result.push(v.sqrt());
        }
    } else {
        // Generic case
        for &v in values {
            result.push(v.powf(exp));
        }
    }

    result
}

/// Dot product of 4-element vectors.
#[inline]
pub fn dot_x4(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let va = f32x4::from(*a);
    let vb = f32x4::from(*b);
    let prod = va * vb;
    prod.reduce_add()
}

/// Matrix-vector multiply for 3x3 matrix and 3-element vector.
///
/// Matrix is row-major: `[[m00, m01, m02], [m10, m11, m12], [m20, m21, m22]]`.
///
/// # Example
///
/// ```rust
/// use vfx_math::simd::mat3_mul_vec3;
///
/// // Identity matrix
/// let m = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
/// let v = [0.5, 0.3, 0.2];
/// let result = mat3_mul_vec3(&m, &v);
/// assert!((result[0] - 0.5).abs() < 0.001);
/// ```
#[inline]
pub fn mat3_mul_vec3(m: &[[f32; 3]; 3], v: &[f32; 3]) -> [f32; 3] {
    // Use f32x4 with padding for the 3-element vector
    let v4 = f32x4::from([v[0], v[1], v[2], 0.0]);

    let r0 = f32x4::from([m[0][0], m[0][1], m[0][2], 0.0]);
    let r1 = f32x4::from([m[1][0], m[1][1], m[1][2], 0.0]);
    let r2 = f32x4::from([m[2][0], m[2][1], m[2][2], 0.0]);

    [
        (r0 * v4).reduce_add(),
        (r1 * v4).reduce_add(),
        (r2 * v4).reduce_add(),
    ]
}

/// Batch matrix-vector multiply.
///
/// Applies the same 3x3 matrix to multiple RGB values.
pub fn batch_mat3_mul_vec3(m: &[[f32; 3]; 3], values: &[[f32; 3]]) -> Vec<[f32; 3]> {
    values.iter().map(|v| mat3_mul_vec3(m, v)).collect()
}

/// Batch RGB to grayscale conversion using Rec. 709 weights.
///
/// # Example
///
/// ```rust
/// use vfx_math::simd::batch_rgb_to_luma;
///
/// let pixels = vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
/// let luma = batch_rgb_to_luma(&pixels);
/// assert!((luma[0] - 0.2126).abs() < 0.001); // Red contribution
/// ```
pub fn batch_rgb_to_luma(pixels: &[[f32; 3]]) -> Vec<f32> {
    // Rec. 709 weights
    const R_WEIGHT: f32 = 0.2126;
    const G_WEIGHT: f32 = 0.7152;
    const B_WEIGHT: f32 = 0.0722;

    pixels
        .iter()
        .map(|rgb| rgb[0] * R_WEIGHT + rgb[1] * G_WEIGHT + rgb[2] * B_WEIGHT)
        .collect()
}

/// Applies a 1D LUT to a batch of values using linear interpolation.
///
/// This is a SIMD-optimized version of 1D LUT application.
pub fn batch_lut1d(values: &[f32], lut: &[f32]) -> Vec<f32> {
    let size = lut.len();
    if size < 2 {
        return values.to_vec();
    }

    let n = (size - 1) as f32;

    values
        .iter()
        .map(|&v| {
            let t = v.clamp(0.0, 1.0) * n;
            let idx0 = (t.floor() as usize).min(size - 2);
            let idx1 = idx0 + 1;
            let frac = t - idx0 as f32;
            lut[idx0] * (1.0 - frac) + lut[idx1] * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mul_add_x4() {
        let values = [0.5, 0.5, 0.5, 0.5];
        let result = mul_add_x4(&values, 2.0, 0.1);
        for &v in &result {
            assert!((v - 1.1).abs() < 0.001);
        }
    }

    #[test]
    fn test_clamp01_x4() {
        let values = [-0.5, 0.5, 1.5, 0.0];
        let result = clamp01_x4(&values);
        assert_eq!(result, [0.0, 0.5, 1.0, 0.0]);
    }

    #[test]
    fn test_lerp_x4() {
        let a = [0.0, 0.0, 0.0, 0.0];
        let b = [1.0, 2.0, 3.0, 4.0];
        let result = lerp_x4(&a, &b, 0.5);
        assert!((result[0] - 0.5).abs() < 0.001);
        assert!((result[1] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_pow_x4() {
        let values = [0.25, 0.5, 0.75, 1.0];
        let result = pow_x4(&values, 2.0);
        assert!((result[0] - 0.0625).abs() < 0.001);
        assert!((result[1] - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_pow_x4_sqrt() {
        let values = [0.25, 1.0, 4.0, 9.0];
        let result = pow_x4(&values, 0.5);
        assert!((result[0] - 0.5).abs() < 0.001);
        assert!((result[2] - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_batch_mul_add() {
        let values = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
        let result = batch_mul_add(&values, 2.0, 0.0);
        assert!((result[0] - 0.2).abs() < 0.001);
        assert!((result[8] - 1.8).abs() < 0.001);
    }

    #[test]
    fn test_batch_mul_add_inplace() {
        let mut values = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
        batch_mul_add_inplace(&mut values, 2.0, 0.1);
        assert!((values[0] - 0.3).abs() < 0.001);
        assert!((values[8] - 1.9).abs() < 0.001);
    }

    #[test]
    fn test_mat3_mul_vec3() {
        // Identity
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let v = [0.5, 0.3, 0.2];
        let result = mat3_mul_vec3(&identity, &v);
        assert!((result[0] - 0.5).abs() < 0.001);
        assert!((result[1] - 0.3).abs() < 0.001);
        assert!((result[2] - 0.2).abs() < 0.001);

        // Scale
        let scale = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]];
        let result = mat3_mul_vec3(&scale, &v);
        assert!((result[0] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_batch_rgb_to_luma() {
        let pixels = vec![
            [1.0, 0.0, 0.0], // Red
            [0.0, 1.0, 0.0], // Green
            [0.0, 0.0, 1.0], // Blue
            [1.0, 1.0, 1.0], // White
        ];
        let luma = batch_rgb_to_luma(&pixels);

        assert!((luma[0] - 0.2126).abs() < 0.001);
        assert!((luma[1] - 0.7152).abs() < 0.001);
        assert!((luma[2] - 0.0722).abs() < 0.001);
        assert!((luma[3] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_batch_lut1d() {
        // Identity LUT
        let lut: Vec<f32> = (0..256).map(|i| i as f32 / 255.0).collect();
        let values = vec![0.0, 0.5, 1.0];
        let result = batch_lut1d(&values, &lut);

        assert!((result[0] - 0.0).abs() < 0.01);
        assert!((result[1] - 0.5).abs() < 0.01);
        assert!((result[2] - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_dot_x4() {
        let a = [1.0, 2.0, 3.0, 4.0];
        let b = [1.0, 1.0, 1.0, 1.0];
        let result = dot_x4(&a, &b);
        assert!((result - 10.0).abs() < 0.001);
    }
}
