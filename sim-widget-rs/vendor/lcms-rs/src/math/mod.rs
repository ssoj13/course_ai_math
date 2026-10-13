//! Numeric kernels shared across the engine.
//!
//! Phase 0 provides the 3×3 matrix / 3-vector algebra ([`mat3`], porting `cmsmtrx.c`),
//! which white-point adaptation, RGB→XYZ construction and the absolute-intent CHAD math
//! all depend on. Half-float tables (`cmshalf`) and the interpolation kernels
//! (`cmsintrp`) join this module later in Phase 0.

pub mod mat3;

/// IEEE half-float conversion via the lcms2 lookup tables (`cmshalf.c`). Gated behind the
/// `half-tables` feature (on by default) so the ~10 KB of tables can be dropped if a
/// rounding `f16` fallback is ever preferred.
#[cfg(feature = "half-tables")]
pub mod half;

pub mod interp;

pub use interp::InterpParams;
pub use mat3::{Mat3, Vec3};

/// `cmsmin(a, b)` — `((a) < (b)) ? (a) : (b)` (`cmssamp.c:30`).
///
/// Deliberately NOT [`f64::min`]. The std versions are IEEE `minNum`/`maxNum`, which *discard*
/// NaN and return the other operand, so `50.0.min(NaN) == 50.0`. C's ternary macro propagates
/// it: `50 < NaN` is false, so the macro yields NaN and the caller ends up with a visibly
/// broken result instead of a plausible-looking wrong one. Reproducing the C semantics keeps a
/// NaN loud rather than silently clamping it into range — see the black-point detector, where a
/// swallowed NaN would hand black-point compensation a believable `L* = 50`.
#[inline]
pub fn cms_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// `cmsmax(a, b)` — `((a) > (b)) ? (a) : (b)` (`cmssamp.c:31`). See [`cms_min`] on NaN.
#[inline]
pub fn cms_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::{cms_max, cms_min};

    /// Locks the contract that separates these from `f64::min`/`f64::max`.
    ///
    /// The std versions implement IEEE `minNum`/`maxNum` and drop NaN; C's ternary macros keep
    /// it. Anyone "simplifying" this back to `a.min(b)` reintroduces a silent-wrong black point
    /// in `cmm::blackpoint` - a NaN would be clamped into a believable `L* = 50`.
    #[test]
    fn cms_min_max_propagate_nan_like_the_c_ternary() {
        let nan = f64::NAN;

        // `50 < NaN` is false -> the ternary yields the second operand, NaN.
        assert!(cms_min(50.0, nan).is_nan());
        // `0 > NaN` is false -> likewise.
        assert!(cms_max(0.0, nan).is_nan());

        // With NaN first, both comparisons are false, so the second operand wins - also C.
        assert_eq!(cms_min(nan, 50.0), 50.0);
        assert_eq!(cms_max(nan, 0.0), 0.0);

        // The clamp shape actually used by the black-point detector must stay NaN-transparent.
        assert!(cms_max(0.0, cms_min(50.0, nan)).is_nan());

        // std would swallow it - this is the divergence being guarded against.
        assert_eq!(50.0_f64.min(nan), 50.0);

        // Ordinary values behave identically to the std versions.
        assert_eq!(cms_min(3.0, 7.0), 3.0);
        assert_eq!(cms_max(3.0, 7.0), 7.0);
        assert_eq!(cms_max(0.0, cms_min(50.0, 62.5)), 50.0);
        assert_eq!(cms_max(0.0, cms_min(50.0, -1.0)), 0.0);
    }
}
