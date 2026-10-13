//! [`TimeTransform`] — a one-dimensional affine time transform.
//!
//! Faithful port of `opentime/timeTransform.h`: an `offset`, a `scale`, and an
//! optional target `rate` (`< 0` meaning "keep the source rate").

use crate::rational_time::RationalTime;
use crate::time_range::TimeRange;

/// A 1-D time transform applied as `value * scale + offset`, optionally rescaled.
#[derive(Debug, Clone, Copy)]
pub struct TimeTransform {
    offset: RationalTime,
    scale: f64,
    rate: f64,
}

impl Default for TimeTransform {
    /// Identity: zero offset, unit scale, rate `-1` (keep source rate).
    fn default() -> Self {
        TimeTransform {
            offset: RationalTime::default(),
            scale: 1.0,
            rate: -1.0,
        }
    }
}

impl TimeTransform {
    /// Construct a transform. A `rate < 0` means "preserve the input's rate".
    pub const fn new(offset: RationalTime, scale: f64, rate: f64) -> Self {
        TimeTransform { offset, scale, rate }
    }

    /// The offset.
    pub const fn offset(self) -> RationalTime {
        self.offset
    }

    /// The scale.
    pub const fn scale(self) -> f64 {
        self.scale
    }

    /// The target rate (`< 0` keeps the source rate).
    pub const fn rate(self) -> f64 {
        self.rate
    }

    /// Apply this transform to a [`RationalTime`].
    pub fn applied_to_time(self, other: RationalTime) -> RationalTime {
        let result =
            RationalTime::new(other.value() * self.scale, other.rate()) + self.offset;
        let target_rate = if self.rate > 0.0 { self.rate } else { other.rate() };
        if target_rate > 0.0 {
            result.rescaled_to(target_rate)
        } else {
            result
        }
    }

    /// Apply this transform to a [`TimeRange`].
    pub fn applied_to_range(self, other: TimeRange) -> TimeRange {
        TimeRange::range_from_start_end_time(
            self.applied_to_time(other.start_time()),
            self.applied_to_time(other.end_time_exclusive()),
        )
    }

    /// Compose this transform with another (`self` applied after `other`).
    pub fn applied_to_transform(self, other: TimeTransform) -> TimeTransform {
        TimeTransform::new(
            self.offset + other.offset,
            self.scale * other.scale,
            if self.rate > 0.0 { self.rate } else { other.rate },
        )
    }
}

impl PartialEq for TimeTransform {
    fn eq(&self, other: &TimeTransform) -> bool {
        self.offset == other.offset && self.scale == other.scale && self.rate == other.rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_and_offset() {
        let id = TimeTransform::default();
        let t = RationalTime::new(10.0, 24.0);
        assert_eq!(id.applied_to_time(t), t);

        let shift = TimeTransform::new(RationalTime::new(5.0, 24.0), 1.0, -1.0);
        assert_eq!(shift.applied_to_time(t), RationalTime::new(15.0, 24.0));
    }

    #[test]
    fn scale_and_compose() {
        let double = TimeTransform::new(RationalTime::new(0.0, 24.0), 2.0, -1.0);
        assert_eq!(
            double.applied_to_time(RationalTime::new(10.0, 24.0)),
            RationalTime::new(20.0, 24.0)
        );
        let composed = double.applied_to_transform(double);
        assert_eq!(composed.scale(), 4.0);
    }
}
