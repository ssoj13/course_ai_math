//! Interpolation utilities for color processing.
//!
//! This module provides various interpolation functions commonly used
//! in VFX color pipelines:
//!
//! - Linear interpolation ([`lerp`])
//! - Smooth interpolation ([`smoothstep`], [`smootherstep`])
//! - Clamping utilities
//!
//! # Usage
//!
//! ```rust
//! use vfx_math::{lerp, smoothstep, remap};
//!
//! // Linear interpolation
//! let mid = lerp(0.0, 10.0, 0.5);
//! assert_eq!(mid, 5.0);
//!
//! // Smooth transition
//! let smooth = smoothstep(0.0, 1.0, 0.5);
//! ```

/// Linear interpolation between two values.
///
/// Returns `a` when `t = 0.0`, and `b` when `t = 1.0`.
/// For values outside [0, 1], the result is extrapolated.
///
/// # Formula
///
/// `a + (b - a) * t`
///
/// # Example
///
/// ```rust
/// use vfx_math::lerp;
///
/// assert_eq!(lerp(0.0, 10.0, 0.0), 0.0);
/// assert_eq!(lerp(0.0, 10.0, 0.5), 5.0);
/// assert_eq!(lerp(0.0, 10.0, 1.0), 10.0);
/// ```
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Inverse linear interpolation.
///
/// Given a value between `a` and `b`, returns the corresponding `t` value.
///
/// # Formula
///
/// `(value - a) / (b - a)`
///
/// # Example
///
/// ```rust
/// use vfx_math::inverse_lerp;
///
/// assert_eq!(inverse_lerp(0.0, 10.0, 5.0), 0.5);
/// ```
#[inline]
pub fn inverse_lerp(a: f32, b: f32, value: f32) -> f32 {
    if (b - a).abs() < 1e-10 {
        0.0
    } else {
        (value - a) / (b - a)
    }
}

/// Remaps a value from one range to another.
///
/// # Example
///
/// ```rust
/// use vfx_math::remap;
///
/// // Map 0.5 from [0,1] to [0,100]
/// assert_eq!(remap(0.5, 0.0, 1.0, 0.0, 100.0), 50.0);
/// ```
#[inline]
pub fn remap(value: f32, in_min: f32, in_max: f32, out_min: f32, out_max: f32) -> f32 {
    let t = inverse_lerp(in_min, in_max, value);
    lerp(out_min, out_max, t)
}

/// Clamps a value to the range [min, max].
///
/// # Example
///
/// ```rust
/// use vfx_math::clamp;
///
/// assert_eq!(clamp(-0.5, 0.0, 1.0), 0.0);
/// assert_eq!(clamp(0.5, 0.0, 1.0), 0.5);
/// assert_eq!(clamp(1.5, 0.0, 1.0), 1.0);
/// ```
#[inline]
pub fn clamp(value: f32, min: f32, max: f32) -> f32 {
    value.max(min).min(max)
}

/// Clamps a value to [0, 1].
///
/// Shorthand for `clamp(value, 0.0, 1.0)`.
#[inline]
pub fn saturate(value: f32) -> f32 {
    clamp(value, 0.0, 1.0)
}

/// Hermite smoothstep interpolation.
///
/// Returns 0 for `x <= edge0`, 1 for `x >= edge1`, and smoothly
/// interpolates between using a cubic polynomial.
///
/// # Formula
///
/// `t * t * (3 - 2 * t)` where `t = (x - edge0) / (edge1 - edge0)`
///
/// # Properties
///
/// - First derivative is zero at both edges (smooth transition)
/// - Continuous but second derivative is not smooth
///
/// # Example
///
/// ```rust
/// use vfx_math::smoothstep;
///
/// assert_eq!(smoothstep(0.0, 1.0, 0.0), 0.0);
/// assert_eq!(smoothstep(0.0, 1.0, 1.0), 1.0);
/// // Midpoint is still 0.5 but curve is smooth
/// assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
/// ```
#[inline]
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = saturate(inverse_lerp(edge0, edge1, x));
    t * t * (3.0 - 2.0 * t)
}

/// Ken Perlin's smootherstep interpolation.
///
/// Like [`smoothstep`] but with zero second derivative at edges,
/// producing an even smoother transition.
///
/// # Formula
///
/// `t * t * t * (t * (t * 6 - 15) + 10)`
///
/// # Example
///
/// ```rust
/// use vfx_math::smootherstep;
///
/// assert_eq!(smootherstep(0.0, 1.0, 0.0), 0.0);
/// assert_eq!(smootherstep(0.0, 1.0, 1.0), 1.0);
/// ```
#[inline]
pub fn smootherstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = saturate(inverse_lerp(edge0, edge1, x));
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Step function.
///
/// Returns 0 for `x < edge`, 1 for `x >= edge`.
#[inline]
pub fn step(edge: f32, x: f32) -> f32 {
    if x < edge { 0.0 } else { 1.0 }
}

/// Pulse function.
///
/// Returns 1 if `x` is in range [a, b], 0 otherwise.
#[inline]
pub fn pulse(a: f32, b: f32, x: f32) -> f32 {
    step(a, x) - step(b, x)
}

/// Fract: returns the fractional part of a value.
///
/// # Example
///
/// ```rust
/// use vfx_math::fract;
///
/// assert!((fract(1.75) - 0.75).abs() < 1e-6);
/// ```
#[inline]
pub fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// Mix: alias for lerp, commonly used in shader languages.
#[inline]
pub fn mix(a: f32, b: f32, t: f32) -> f32 {
    lerp(a, b, t)
}

/// Sign function.
///
/// Returns -1 for negative, 0 for zero, 1 for positive.
#[inline]
pub fn sign(x: f32) -> f32 {
    if x < 0.0 {
        -1.0
    } else if x > 0.0 {
        1.0
    } else {
        0.0
    }
}

/// Bias function for gamma-like adjustment.
///
/// `bias(b, t) = t^(log(b) / log(0.5))`
///
/// When b = 0.5, returns t unchanged. Values < 0.5 push the curve
/// down, values > 0.5 push it up.
///
/// # Example
///
/// ```rust
/// use vfx_math::bias;
///
/// // At b=0.5, function is identity
/// assert!((bias(0.5, 0.5) - 0.5).abs() < 1e-6);
/// ```
#[inline]
pub fn bias(b: f32, t: f32) -> f32 {
    if b <= 0.0 || b >= 1.0 {
        return t;
    }
    t.powf(b.ln() / 0.5_f32.ln())
}

/// Gain function for S-curve adjustment.
///
/// Attempt to combine two bias curves for an S-shaped response.
///
/// # Example
///
/// ```rust
/// use vfx_math::gain;
///
/// // At g=0.5, function is identity
/// assert!((gain(0.5, 0.5) - 0.5).abs() < 1e-6);
/// ```
#[inline]
pub fn gain(g: f32, t: f32) -> f32 {
    if t < 0.5 {
        bias(1.0 - g, 2.0 * t) / 2.0
    } else {
        1.0 - bias(1.0 - g, 2.0 - 2.0 * t) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lerp() {
        assert_eq!(lerp(0.0, 10.0, 0.0), 0.0);
        assert_eq!(lerp(0.0, 10.0, 0.5), 5.0);
        assert_eq!(lerp(0.0, 10.0, 1.0), 10.0);
    }

    #[test]
    fn test_inverse_lerp() {
        assert_eq!(inverse_lerp(0.0, 10.0, 0.0), 0.0);
        assert_eq!(inverse_lerp(0.0, 10.0, 5.0), 0.5);
        assert_eq!(inverse_lerp(0.0, 10.0, 10.0), 1.0);
    }

    #[test]
    fn test_remap() {
        assert_eq!(remap(0.5, 0.0, 1.0, 0.0, 100.0), 50.0);
        assert_eq!(remap(50.0, 0.0, 100.0, 0.0, 1.0), 0.5);
    }

    #[test]
    fn test_saturate() {
        assert_eq!(saturate(-0.5), 0.0);
        assert_eq!(saturate(0.5), 0.5);
        assert_eq!(saturate(1.5), 1.0);
    }

    #[test]
    fn test_smoothstep() {
        assert_eq!(smoothstep(0.0, 1.0, 0.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 1.0), 1.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);

        // Below edge0 and above edge1
        assert_eq!(smoothstep(0.0, 1.0, -1.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 2.0), 1.0);
    }

    #[test]
    fn test_smootherstep() {
        assert_eq!(smootherstep(0.0, 1.0, 0.0), 0.0);
        assert_eq!(smootherstep(0.0, 1.0, 1.0), 1.0);
    }

    #[test]
    fn test_step() {
        assert_eq!(step(0.5, 0.25), 0.0);
        assert_eq!(step(0.5, 0.5), 1.0);
        assert_eq!(step(0.5, 0.75), 1.0);
    }

    #[test]
    fn test_fract() {
        assert!((fract(1.75) - 0.75).abs() < 1e-6);
        assert!((fract(-0.25) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn test_bias_identity() {
        assert!((bias(0.5, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_gain_identity() {
        assert!((gain(0.5, 0.5) - 0.5).abs() < 1e-6);
        assert!((gain(0.5, 0.0) - 0.0).abs() < 1e-6);
        assert!((gain(0.5, 1.0) - 1.0).abs() < 1e-6);
    }
}
