//! 3×3 matrix and 3-vector algebra — a direct port of `cmsmtrx.c`.
//!
//! lcms2 represents a matrix as three row vectors (`cmsMAT3 { cmsVEC3 v[3] }`) and a
//! vector as `cmsVEC3 { double n[3] }` indexed by `VX/VY/VZ`. We keep that exact shape
//! (`Vec3([f64; 3])`, `Mat3([Vec3; 3])` = rows) so the formulas transcribe 1:1 and the
//! floating-point order of operations — and therefore the results — match the C bit for
//! bit.

/// Singularity threshold for inversion (`MATRIX_DET_TOLERANCE` in `lcms2_internal.h`).
pub const MATRIX_DET_TOLERANCE: f64 = 0.0001;

/// A 3-component vector (`cmsVEC3`). Components are `[x, y, z]` (`VX=0, VY=1, VZ=2`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3(pub [f64; 3]);

impl Vec3 {
    /// `_cmsVEC3init`.
    #[inline]
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3([x, y, z])
    }

    /// `_cmsVEC3minus` — component-wise `a - b`.
    #[inline]
    pub fn minus(a: Vec3, b: Vec3) -> Vec3 {
        Vec3([a.0[0] - b.0[0], a.0[1] - b.0[1], a.0[2] - b.0[2]])
    }

    /// `_cmsVEC3cross`.
    #[inline]
    pub fn cross(u: Vec3, v: Vec3) -> Vec3 {
        Vec3([
            u.0[1] * v.0[2] - v.0[1] * u.0[2],
            u.0[2] * v.0[0] - v.0[2] * u.0[0],
            u.0[0] * v.0[1] - v.0[0] * u.0[1],
        ])
    }

    /// `_cmsVEC3dot`.
    #[inline]
    pub fn dot(u: Vec3, v: Vec3) -> f64 {
        u.0[0] * v.0[0] + u.0[1] * v.0[1] + u.0[2] * v.0[2]
    }

    /// `_cmsVEC3length` — Euclidean length.
    #[inline]
    pub fn length(&self) -> f64 {
        (self.0[0] * self.0[0] + self.0[1] * self.0[1] + self.0[2] * self.0[2]).sqrt()
    }

    /// `_cmsVEC3distance`.
    #[inline]
    pub fn distance(a: Vec3, b: Vec3) -> f64 {
        let d1 = a.0[0] - b.0[0];
        let d2 = a.0[1] - b.0[1];
        let d3 = a.0[2] - b.0[2];
        (d1 * d1 + d2 * d2 + d3 * d3).sqrt()
    }
}

/// A 3×3 matrix (`cmsMAT3`), stored as three **row** vectors.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Mat3(pub [Vec3; 3]);

impl Mat3 {
    /// Build from three row vectors.
    #[inline]
    pub const fn from_rows(r0: Vec3, r1: Vec3, r2: Vec3) -> Self {
        Mat3([r0, r1, r2])
    }

    /// `_cmsMAT3identity`.
    #[inline]
    pub fn identity() -> Self {
        Mat3([
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ])
    }

    /// `_cmsMAT3isIdentity` — true if every element is within `1/65535` of identity
    /// (`CloseEnough`).
    pub fn is_identity(&self) -> bool {
        let id = Mat3::identity();
        for i in 0..3 {
            for j in 0..3 {
                if (self.0[i].0[j] - id.0[i].0[j]).abs() >= 1.0 / 65535.0 {
                    return false;
                }
            }
        }
        true
    }

    /// `_cmsMAT3per` — matrix product `a * b`.
    ///
    /// The macro `ROWCOL(i,j)` expands to `a[i,0]*b[0,j] + a[i,1]*b[1,j] + a[i,2]*b[2,j]`;
    /// the summation order is preserved for bit-exactness.
    pub fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
        let rowcol = |i: usize, j: usize| {
            a.0[i].0[0] * b.0[0].0[j] + a.0[i].0[1] * b.0[1].0[j] + a.0[i].0[2] * b.0[2].0[j]
        };
        Mat3([
            Vec3::new(rowcol(0, 0), rowcol(0, 1), rowcol(0, 2)),
            Vec3::new(rowcol(1, 0), rowcol(1, 1), rowcol(1, 2)),
            Vec3::new(rowcol(2, 0), rowcol(2, 1), rowcol(2, 2)),
        ])
    }

    /// `_cmsMAT3inverse` — returns `None` for a singular matrix (`|det| < tolerance`).
    ///
    /// Cofactor/adjugate layout copied verbatim from the C so rounding matches.
    pub fn inverse(&self) -> Option<Mat3> {
        let a = &self.0;
        let c0 = a[1].0[1] * a[2].0[2] - a[1].0[2] * a[2].0[1];
        let c1 = -a[1].0[0] * a[2].0[2] + a[1].0[2] * a[2].0[0];
        let c2 = a[1].0[0] * a[2].0[1] - a[1].0[1] * a[2].0[0];

        let det = a[0].0[0] * c0 + a[0].0[1] * c1 + a[0].0[2] * c2;
        if det.abs() < MATRIX_DET_TOLERANCE {
            return None;
        }

        Some(Mat3([
            Vec3::new(
                c0 / det,
                (a[0].0[2] * a[2].0[1] - a[0].0[1] * a[2].0[2]) / det,
                (a[0].0[1] * a[1].0[2] - a[0].0[2] * a[1].0[1]) / det,
            ),
            Vec3::new(
                c1 / det,
                (a[0].0[0] * a[2].0[2] - a[0].0[2] * a[2].0[0]) / det,
                (a[0].0[2] * a[1].0[0] - a[0].0[0] * a[1].0[2]) / det,
            ),
            Vec3::new(
                c2 / det,
                (a[0].0[1] * a[2].0[0] - a[0].0[0] * a[2].0[1]) / det,
                (a[0].0[0] * a[1].0[1] - a[0].0[1] * a[1].0[0]) / det,
            ),
        ]))
    }

    /// `_cmsMAT3eval` — matrix-times-vector, `r = a * v`.
    pub fn eval(&self, v: Vec3) -> Vec3 {
        let a = &self.0;
        Vec3::new(
            a[0].0[0] * v.0[0] + a[0].0[1] * v.0[1] + a[0].0[2] * v.0[2],
            a[1].0[0] * v.0[0] + a[1].0[1] * v.0[1] + a[1].0[2] * v.0[2],
            a[2].0[0] * v.0[0] + a[2].0[1] * v.0[1] + a[2].0[2] * v.0[2],
        )
    }

    /// `_cmsMAT3solve` — solve `A x = b`, returning `None` if `A` is singular.
    pub fn solve(a: &Mat3, b: Vec3) -> Option<Vec3> {
        let inv = a.inverse()?;
        Some(inv.eval(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_eval() {
        let id = Mat3::identity();
        assert!(id.is_identity());
        let v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(id.eval(v), v);
    }

    #[test]
    fn inverse_roundtrip() {
        let m = Mat3::from_rows(
            Vec3::new(2.0, 0.0, 1.0),
            Vec3::new(1.0, 3.0, 2.0),
            Vec3::new(1.0, 0.0, 3.0),
        );
        let inv = m.inverse().expect("non-singular");
        let prod = Mat3::mul(&m, &inv);
        assert!(prod.is_identity(), "M * M^-1 should be identity, got {prod:?}");
    }

    #[test]
    fn singular_matrix_has_no_inverse() {
        // Two identical rows -> det 0.
        let m = Mat3::from_rows(
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(4.0, 5.0, 6.0),
        );
        assert!(m.inverse().is_none());
        assert!(Mat3::solve(&m, Vec3::new(1.0, 1.0, 1.0)).is_none());
    }

    #[test]
    fn solve_linear_system() {
        // A x = b with a known solution.
        let a = Mat3::from_rows(
            Vec3::new(2.0, 1.0, 1.0),
            Vec3::new(1.0, 3.0, 2.0),
            Vec3::new(1.0, 0.0, 0.0),
        );
        let x = Vec3::new(1.0, 2.0, 3.0);
        let b = a.eval(x); // construct b so the answer is x
        let solved = Mat3::solve(&a, b).expect("non-singular");
        for k in 0..3 {
            assert!((solved.0[k] - x.0[k]).abs() < 1e-9);
        }
    }

    #[test]
    fn cross_and_dot() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        assert_eq!(Vec3::cross(x, y), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(Vec3::dot(x, y), 0.0);
        assert_eq!(Vec3::dot(x, x), 1.0);
    }
}
