//! 3x3 matrix type for color transformations.
//!
//! [`Mat3`] is used for RGB-to-XYZ conversions, chromatic adaptation,
//! and other linear color space transforms.
//!
//! # Convention
//!
//! Matrices are stored in **row-major** order and use **column vectors**:
//!
//! ```text
//! | m00 m01 m02 |   | x |   | m00*x + m01*y + m02*z |
//! | m10 m11 m12 | * | y | = | m10*x + m11*y + m12*z |
//! | m20 m21 m22 |   | z |   | m20*x + m21*y + m22*z |
//! ```
//!
//! # Usage
//!
//! ```rust
//! use vfx_math::{Mat3, Vec3};
//!
//! // sRGB to XYZ (D65)
//! let rgb_to_xyz = Mat3::from_rows([
//!     [0.4124564, 0.3575761, 0.1804375],
//!     [0.2126729, 0.7151522, 0.0721750],
//!     [0.0193339, 0.1191920, 0.9503041],
//! ]);
//!
//! let rgb = Vec3::new(1.0, 0.0, 0.0);
//! let xyz = rgb_to_xyz * rgb;
//! ```

use crate::Vec3;
use std::ops::{Index, Mul};

/// A 3x3 matrix for color transformations.
///
/// Stored in row-major order. Use [`Mat3::from_rows`] or [`Mat3::from_cols`]
/// to construct from component arrays.
///
/// # Example
///
/// ```rust
/// use vfx_math::{Mat3, Vec3};
///
/// let identity = Mat3::IDENTITY;
/// let v = Vec3::new(1.0, 2.0, 3.0);
/// assert_eq!(identity * v, v);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Mat3 {
    /// Matrix elements in row-major order: [row0, row1, row2]
    pub m: [[f32; 3]; 3],
}

impl Mat3 {
    /// Zero matrix.
    pub const ZERO: Self = Self { m: [[0.0; 3]; 3] };

    /// Identity matrix.
    pub const IDENTITY: Self = Self {
        m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    };

    /// Creates a matrix from row arrays.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_math::Mat3;
    ///
    /// let m = Mat3::from_rows([
    ///     [1.0, 0.0, 0.0],
    ///     [0.0, 1.0, 0.0],
    ///     [0.0, 0.0, 1.0],
    /// ]);
    /// assert_eq!(m, Mat3::IDENTITY);
    /// ```
    #[inline]
    pub const fn from_rows(rows: [[f32; 3]; 3]) -> Self {
        Self { m: rows }
    }

    /// Creates a matrix from column arrays.
    ///
    /// Transposes the input (columns become rows internally).
    #[inline]
    pub const fn from_cols(cols: [[f32; 3]; 3]) -> Self {
        Self {
            m: [
                [cols[0][0], cols[1][0], cols[2][0]],
                [cols[0][1], cols[1][1], cols[2][1]],
                [cols[0][2], cols[1][2], cols[2][2]],
            ],
        }
    }

    /// Creates a matrix from Vec3 rows.
    #[inline]
    pub fn from_row_vecs(r0: Vec3, r1: Vec3, r2: Vec3) -> Self {
        Self::from_rows([r0.to_array(), r1.to_array(), r2.to_array()])
    }

    /// Creates a matrix from Vec3 columns.
    #[inline]
    pub fn from_col_vecs(c0: Vec3, c1: Vec3, c2: Vec3) -> Self {
        Self::from_cols([c0.to_array(), c1.to_array(), c2.to_array()])
    }

    /// Creates a diagonal matrix.
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_math::Mat3;
    ///
    /// let scale = Mat3::diagonal(2.0, 2.0, 2.0);
    /// ```
    #[inline]
    pub const fn diagonal(d0: f32, d1: f32, d2: f32) -> Self {
        Self::from_rows([[d0, 0.0, 0.0], [0.0, d1, 0.0], [0.0, 0.0, d2]])
    }

    /// Creates a uniform scale matrix.
    #[inline]
    pub const fn scale(s: f32) -> Self {
        Self::diagonal(s, s, s)
    }

    /// Returns a row as Vec3.
    #[inline]
    pub fn row(&self, i: usize) -> Vec3 {
        Vec3::from_array(self.m[i])
    }

    /// Returns a column as Vec3.
    #[inline]
    pub fn col(&self, i: usize) -> Vec3 {
        Vec3::new(self.m[0][i], self.m[1][i], self.m[2][i])
    }

    /// Returns the transpose of this matrix.
    #[inline]
    pub fn transpose(&self) -> Self {
        Self::from_rows([
            [self.m[0][0], self.m[1][0], self.m[2][0]],
            [self.m[0][1], self.m[1][1], self.m[2][1]],
            [self.m[0][2], self.m[1][2], self.m[2][2]],
        ])
    }

    /// Computes the determinant.
    #[inline]
    pub fn determinant(&self) -> f32 {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    /// Computes the inverse of this matrix.
    ///
    /// Returns `None` if the matrix is singular (determinant is zero).
    ///
    /// # Example
    ///
    /// ```rust
    /// use vfx_math::Mat3;
    ///
    /// let m = Mat3::scale(2.0);
    /// let inv = m.inverse().unwrap();
    /// let result = m * inv;
    /// // result is approximately identity
    /// ```
    pub fn inverse(&self) -> Option<Self> {
        let det = self.determinant();
        if det.abs() < 1e-10 {
            return None;
        }

        let m = &self.m;
        let inv_det = 1.0 / det;

        // Cofactor matrix, transposed and scaled by 1/det
        Some(Self::from_rows([
            [
                (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
                (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
                (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
            ],
            [
                (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
                (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
                (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
            ],
            [
                (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
                (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
                (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
            ],
        ]))
    }

    /// Transforms a Vec3 by this matrix.
    ///
    /// Equivalent to `matrix * vector`.
    #[inline]
    pub fn transform(&self, v: Vec3) -> Vec3 {
        Vec3::new(
            self.m[0][0] * v.x + self.m[0][1] * v.y + self.m[0][2] * v.z,
            self.m[1][0] * v.x + self.m[1][1] * v.y + self.m[1][2] * v.z,
            self.m[2][0] * v.x + self.m[2][1] * v.y + self.m[2][2] * v.z,
        )
    }

    /// Multiplies two matrices.
    #[inline]
    pub fn mul_mat(&self, other: &Self) -> Self {
        let mut result = Self::ZERO;
        for i in 0..3 {
            for j in 0..3 {
                result.m[i][j] = self.m[i][0] * other.m[0][j]
                    + self.m[i][1] * other.m[1][j]
                    + self.m[i][2] * other.m[2][j];
            }
        }
        result
    }

    /// Returns true if all elements are finite (not NaN or infinite).
    #[inline]
    pub fn is_finite(&self) -> bool {
        self.m.iter().flatten().all(|x| x.is_finite())
    }

    /// Converts to glam Mat3 (column-major).
    #[inline]
    pub fn to_glam(&self) -> glam::Mat3 {
        // glam uses column-major, so we transpose
        glam::Mat3::from_cols_array_2d(&[
            [self.m[0][0], self.m[1][0], self.m[2][0]],
            [self.m[0][1], self.m[1][1], self.m[2][1]],
            [self.m[0][2], self.m[1][2], self.m[2][2]],
        ])
    }

    /// Creates from glam Mat3.
    #[inline]
    pub fn from_glam(m: glam::Mat3) -> Self {
        let cols = m.to_cols_array_2d();
        Self::from_cols([cols[0], cols[1], cols[2]])
    }
}

impl Default for Mat3 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

// Mat3 * Vec3
impl Mul<Vec3> for Mat3 {
    type Output = Vec3;

    #[inline]
    fn mul(self, rhs: Vec3) -> Vec3 {
        self.transform(rhs)
    }
}

// Mat3 * Mat3
impl Mul for Mat3 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: Self) -> Self {
        self.mul_mat(&rhs)
    }
}

// Mat3 * f32
impl Mul<f32> for Mat3 {
    type Output = Self;

    #[inline]
    fn mul(self, rhs: f32) -> Self {
        Self::from_rows([
            [self.m[0][0] * rhs, self.m[0][1] * rhs, self.m[0][2] * rhs],
            [self.m[1][0] * rhs, self.m[1][1] * rhs, self.m[1][2] * rhs],
            [self.m[2][0] * rhs, self.m[2][1] * rhs, self.m[2][2] * rhs],
        ])
    }
}

impl Index<usize> for Mat3 {
    type Output = [f32; 3];

    #[inline]
    fn index(&self, i: usize) -> &[f32; 3] {
        &self.m[i]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mat3_identity() {
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(Mat3::IDENTITY * v, v);
    }

    #[test]
    fn test_mat3_scale() {
        let m = Mat3::scale(2.0);
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(m * v, Vec3::new(2.0, 4.0, 6.0));
    }

    #[test]
    fn test_mat3_transpose() {
        let m = Mat3::from_rows([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]);
        let t = m.transpose();
        assert_eq!(t.m[0][1], 4.0);
        assert_eq!(t.m[1][0], 2.0);
    }

    #[test]
    fn test_mat3_determinant() {
        let m = Mat3::from_rows([[1.0, 2.0, 3.0], [0.0, 1.0, 4.0], [5.0, 6.0, 0.0]]);
        assert!((m.determinant() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_mat3_inverse() {
        let m = Mat3::from_rows([[1.0, 2.0, 3.0], [0.0, 1.0, 4.0], [5.0, 6.0, 0.0]]);
        let inv = m.inverse().unwrap();
        let result = m * inv;

        // Should be close to identity
        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((result.m[i][j] - expected).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn test_mat3_singular() {
        let m = Mat3::from_rows([
            [1.0, 2.0, 3.0],
            [2.0, 4.0, 6.0], // Row 2 = 2 * Row 1
            [1.0, 1.0, 1.0],
        ]);
        assert!(m.inverse().is_none());
    }

    #[test]
    fn test_mat3_mul_mat() {
        let a = Mat3::scale(2.0);
        let b = Mat3::scale(3.0);
        let c = a * b;
        assert_eq!(c, Mat3::scale(6.0));
    }
}
