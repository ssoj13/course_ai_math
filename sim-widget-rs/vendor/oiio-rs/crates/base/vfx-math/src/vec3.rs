//! 3D vector type for color triplets.
//!
//! [`Vec3`] represents RGB, XYZ, or other 3-component color values.
//! It wraps [`glam::Vec3`] with color-specific operations.
//!
//! # Usage
//!
//! ```rust
//! use vfx_math::Vec3;
//!
//! let rgb = Vec3::new(1.0, 0.5, 0.25);
//! let scaled = rgb * 2.0;
//! let clamped = scaled.clamp01();
//! ```

use std::ops::{Add, Div, Index, IndexMut, Mul, Sub};

/// A 3D vector for color triplets (RGB, XYZ, etc.).
///
/// Internally uses [`glam::Vec3`] for SIMD acceleration on supported platforms.
///
/// # Components
///
/// Access via `.x`, `.y`, `.z` or index `[0]`, `[1]`, `[2]`.
/// For RGB: x=R, y=G, z=B. For XYZ: x=X, y=Y, z=Z.
///
/// # Example
///
/// ```rust
/// use vfx_math::Vec3;
///
/// let color = Vec3::new(0.5, 0.5, 0.5);
/// assert_eq!(color.x, 0.5);
/// assert_eq!(color[0], 0.5);
///
/// // Color operations
/// let luminance = color.dot(Vec3::new(0.2126, 0.7152, 0.0722));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[repr(C)]
pub struct Vec3 {
    /// X component (R for RGB, X for XYZ)
    pub x: f32,
    /// Y component (G for RGB, Y for XYZ)
    pub y: f32,
    /// Z component (B for RGB, Z for XYZ)
    pub z: f32,
}

impl Vec3 {
    /// Zero vector (0, 0, 0).
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    /// One vector (1, 1, 1).
    pub const ONE: Self = Self::new(1.0, 1.0, 1.0);

    /// Unit X vector (1, 0, 0).
    pub const X: Self = Self::new(1.0, 0.0, 0.0);

    /// Unit Y vector (0, 1, 0).
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);

    /// Unit Z vector (0, 0, 1).
    pub const Z: Self = Self::new(0.0, 0.0, 1.0);

    /// Creates a new vector.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_math::Vec3;
    ///
    /// let v = Vec3::new(1.0, 2.0, 3.0);
    /// ```
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Creates a vector with all components set to the same value.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_math::Vec3;
    ///
    /// let gray = Vec3::splat(0.5);
    /// assert_eq!(gray, Vec3::new(0.5, 0.5, 0.5));
    /// ```
    #[inline]
    pub const fn splat(v: f32) -> Self {
        Self::new(v, v, v)
    }

    /// Creates from an array.
    #[inline]
    pub const fn from_array(a: [f32; 3]) -> Self {
        Self::new(a[0], a[1], a[2])
    }

    /// Converts to an array.
    #[inline]
    pub const fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    /// Dot product with another vector.
    ///
    /// Commonly used for computing luminance:
    /// ```rust
    /// use vfx_math::Vec3;
    ///
    /// let rgb = Vec3::new(1.0, 0.5, 0.25);
    /// let luma_coeffs = Vec3::new(0.2126, 0.7152, 0.0722);
    /// let luminance = rgb.dot(luma_coeffs);
    /// ```
    #[inline]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Cross product.
    #[inline]
    pub fn cross(self, other: Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Length (magnitude) of the vector.
    #[inline]
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// Squared length (avoids sqrt).
    #[inline]
    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    /// Normalizes the vector to unit length.
    ///
    /// Returns zero vector if length is zero.
    #[inline]
    pub fn normalize(self) -> Self {
        let len = self.length();
        if len > 0.0 { self / len } else { Self::ZERO }
    }

    /// Component-wise minimum.
    #[inline]
    pub fn min(self, other: Self) -> Self {
        Self::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
        )
    }

    /// Component-wise maximum.
    #[inline]
    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
        )
    }

    /// Clamps each component to [0, 1].
    ///
    /// Essential for color processing to keep values in valid range.
    #[inline]
    pub fn clamp01(self) -> Self {
        self.min(Self::ONE).max(Self::ZERO)
    }

    /// Clamps each component to [min, max].
    #[inline]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        self.min(max).max(min)
    }

    /// Component-wise absolute value.
    #[inline]
    pub fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs(), self.z.abs())
    }

    /// Component-wise floor.
    #[inline]
    pub fn floor(self) -> Self {
        Self::new(self.x.floor(), self.y.floor(), self.z.floor())
    }

    /// Component-wise ceiling.
    #[inline]
    pub fn ceil(self) -> Self {
        Self::new(self.x.ceil(), self.y.ceil(), self.z.ceil())
    }

    /// Component-wise power.
    #[inline]
    pub fn powf(self, exp: f32) -> Self {
        Self::new(self.x.powf(exp), self.y.powf(exp), self.z.powf(exp))
    }

    /// Component-wise power with per-component exponents.
    #[inline]
    pub fn pow(self, exp: Self) -> Self {
        Self::new(self.x.powf(exp.x), self.y.powf(exp.y), self.z.powf(exp.z))
    }

    /// Linear interpolation between self and other.
    ///
    /// `t = 0.0` returns self, `t = 1.0` returns other.
    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }

    /// Returns the smallest component.
    #[inline]
    pub fn min_element(self) -> f32 {
        self.x.min(self.y).min(self.z)
    }

    /// Returns the largest component.
    #[inline]
    pub fn max_element(self) -> f32 {
        self.x.max(self.y).max(self.z)
    }

    /// Returns true if any component is NaN.
    #[inline]
    pub fn is_nan(self) -> bool {
        self.x.is_nan() || self.y.is_nan() || self.z.is_nan()
    }

    /// Returns true if any component is infinite.
    #[inline]
    pub fn is_infinite(self) -> bool {
        self.x.is_infinite() || self.y.is_infinite() || self.z.is_infinite()
    }

    /// Returns true if all components are finite (not NaN or infinite).
    #[inline]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    /// Converts to glam Vec3.
    #[inline]
    pub fn to_glam(self) -> glam::Vec3 {
        glam::Vec3::new(self.x, self.y, self.z)
    }

    /// Creates from glam Vec3.
    #[inline]
    pub fn from_glam(v: glam::Vec3) -> Self {
        Self::new(v.x, v.y, v.z)
    }
}

// Indexing
impl Index<usize> for Vec3 {
    type Output = f32;

    #[inline]
    fn index(&self, i: usize) -> &f32 {
        match i {
            0 => &self.x,
            1 => &self.y,
            2 => &self.z,
            _ => panic!("Vec3 index out of bounds: {}", i),
        }
    }
}

impl IndexMut<usize> for Vec3 {
    #[inline]
    fn index_mut(&mut self, i: usize) -> &mut f32 {
        match i {
            0 => &mut self.x,
            1 => &mut self.y,
            2 => &mut self.z,
            _ => panic!("Vec3 index out of bounds: {}", i),
        }
    }
}

// Vec3 + Vec3
impl Add for Vec3 {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

// Vec3 - Vec3
impl Sub for Vec3 {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

// Vec3 * Vec3 (component-wise)
impl Mul for Vec3 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self {
        Self::new(self.x * rhs.x, self.y * rhs.y, self.z * rhs.z)
    }
}

// Vec3 * f32
impl Mul<f32> for Vec3 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

// f32 * Vec3
impl Mul<Vec3> for f32 {
    type Output = Vec3;

    #[inline]
    fn mul(self, rhs: Vec3) -> Vec3 {
        Vec3::new(self * rhs.x, self * rhs.y, self * rhs.z)
    }
}

// Vec3 / Vec3 (component-wise)
impl Div for Vec3 {
    type Output = Self;

    #[inline]
    fn div(self, rhs: Self) -> Self {
        Self::new(self.x / rhs.x, self.y / rhs.y, self.z / rhs.z)
    }
}

// Vec3 / f32
impl Div<f32> for Vec3 {
    type Output = Self;

    #[inline]
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x / rhs, self.y / rhs, self.z / rhs)
    }
}

impl From<[f32; 3]> for Vec3 {
    #[inline]
    fn from(a: [f32; 3]) -> Self {
        Self::from_array(a)
    }
}

impl From<Vec3> for [f32; 3] {
    #[inline]
    fn from(v: Vec3) -> [f32; 3] {
        v.to_array()
    }
}

impl From<glam::Vec3> for Vec3 {
    #[inline]
    fn from(v: glam::Vec3) -> Self {
        Self::from_glam(v)
    }
}

impl From<Vec3> for glam::Vec3 {
    #[inline]
    fn from(v: Vec3) -> glam::Vec3 {
        v.to_glam()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vec3_new() {
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 2.0);
        assert_eq!(v.z, 3.0);
    }

    #[test]
    fn test_vec3_splat() {
        let v = Vec3::splat(0.5);
        assert_eq!(v, Vec3::new(0.5, 0.5, 0.5));
    }

    #[test]
    fn test_vec3_dot() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(4.0, 5.0, 6.0);
        assert_eq!(a.dot(b), 32.0);
    }

    #[test]
    fn test_vec3_clamp01() {
        let v = Vec3::new(-0.5, 0.5, 1.5);
        let c = v.clamp01();
        assert_eq!(c, Vec3::new(0.0, 0.5, 1.0));
    }

    #[test]
    fn test_vec3_lerp() {
        let a = Vec3::ZERO;
        let b = Vec3::ONE;
        assert_eq!(a.lerp(b, 0.5), Vec3::splat(0.5));
    }

    #[test]
    fn test_vec3_ops() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(4.0, 5.0, 6.0);

        assert_eq!(a + b, Vec3::new(5.0, 7.0, 9.0));
        assert_eq!(b - a, Vec3::new(3.0, 3.0, 3.0));
        assert_eq!(a * 2.0, Vec3::new(2.0, 4.0, 6.0));
    }

    #[test]
    fn test_vec3_index() {
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(v[0], 1.0);
        assert_eq!(v[1], 2.0);
        assert_eq!(v[2], 3.0);
    }
}
