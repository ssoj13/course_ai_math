//! # curves
//!
//! A pure-Rust, dependency-free **keyframe track** library in the Nuke / Fusion / Maya style
//! (design note: `docs/KEYS.md`).
//!
//! * [`Track`] — one animated scalar: keys with per-side tangent types ([`Tan`]), one cubic-Bezier
//!   evaluator for every segment, pre/post [`Extrap`]olation, allocation-free evaluation.
//! * [`Channels`] — `N` independent tracks (vectors, colours, rotations).
//! * [`Fitter`] — key reduction: baked per-frame samples to few keys within a tolerance.
//! * [`legacy::LegacyKey`] / [`Track::from_legacy`] — loading of old scenes that stored one `CurveKind` per key.
//!
//! A *ramp* (colour or position over `[0,1]`) is simply a [`Channels`] / [`Track`] on that interval with
//! [`Tan::Smooth`] keys, which never overshoot the neighbouring key values.
//!
//! Every type is generic over [`Real`] (`f32` or `f64`, default `f64`). No UI, no FFI, no unsafe.

mod fit;
mod keys;
pub mod legacy;

pub use fit::{FitStats, Fitter};
pub use keys::{Channels, Extrap, Key, KeysError, Side, SideId, Tan, Track, unwind_slice};

use std::fmt::Debug;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// The scalar a track is stored and evaluated in: `f32` or `f64`.
///
/// Only what the maths need: arithmetic, ordering, a few constants and `floor`.
pub trait Real:
    Copy
    + Debug
    + Default
    + PartialEq
    + PartialOrd
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
    + 'static
{
    /// `0`.
    const ZERO: Self;
    /// `1`.
    const ONE: Self;
    /// The machine epsilon of the type.
    const EPSILON: Self;
    /// Not a number.
    const NAN: Self;
    /// `v` rounded to this type (literal constants of the maths).
    fn of(v: f64) -> Self;
    /// This value widened to `f64`.
    fn to_f64(self) -> f64;
    /// Absolute value.
    fn abs(self) -> Self;
    /// Largest integer not above `self`.
    fn floor(self) -> Self;
    /// `true` unless NaN or infinite.
    fn is_finite(self) -> bool;
    /// Total order (sorting without NaN surprises).
    fn total_cmp(&self, other: &Self) -> std::cmp::Ordering;

    /// The smaller of `self` and `other`.
    fn min(self, other: Self) -> Self {
        if other < self { other } else { self }
    }
    /// The larger of `self` and `other`.
    fn max(self, other: Self) -> Self {
        if other > self { other } else { self }
    }
    /// `self` limited to `[lo, hi]`.
    fn clamp(self, lo: Self, hi: Self) -> Self {
        self.max(lo).min(hi)
    }
}

macro_rules! impl_real {
    ($t:ident) => {
        impl Real for $t {
            const ZERO: Self = 0.0;
            const ONE: Self = 1.0;
            const EPSILON: Self = $t::EPSILON;
            const NAN: Self = $t::NAN;
            fn of(v: f64) -> Self {
                v as $t
            }
            fn to_f64(self) -> f64 {
                f64::from(self)
            }
            fn abs(self) -> Self {
                $t::abs(self)
            }
            fn floor(self) -> Self {
                $t::floor(self)
            }
            fn is_finite(self) -> bool {
                $t::is_finite(self)
            }
            fn total_cmp(&self, other: &Self) -> std::cmp::Ordering {
                $t::total_cmp(self, other)
            }
        }
    };
}
impl_real!(f32);
impl_real!(f64);
