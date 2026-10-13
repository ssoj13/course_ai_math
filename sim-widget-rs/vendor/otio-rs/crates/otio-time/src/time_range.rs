//! [`TimeRange`] — a `start_time` + `duration`, inclusive of start, exclusive of
//! end.
//!
//! Faithful port of `opentime/timeRange.h`. Includes James F. Allen's thirteen
//! interval relations, computed in double seconds with an `epsilon_s` tolerance.

use crate::rational_time::RationalTime;

/// Default comparison tolerance, in seconds.
///
/// Twice 192 kHz — half a sample at the fastest common audio rate.
pub const DEFAULT_EPSILON_S: f64 = 1.0 / (2.0 * 192000.0);

/// A contiguous span of time `[start_time, start_time + duration)`.
///
/// Serde (feature `serde`) persists both ends with their own rates; see [`RationalTime`].
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TimeRange {
    start_time: RationalTime,
    duration: RationalTime,
}

impl Default for TimeRange {
    /// Zero start and duration (each at rate 1).
    fn default() -> Self {
        TimeRange {
            start_time: RationalTime::default(),
            duration: RationalTime::default(),
        }
    }
}

/// `std::min` for `RationalTime` (returns `b` only when strictly less).
fn min_rt(a: RationalTime, b: RationalTime) -> RationalTime {
    if b < a {
        b
    } else {
        a
    }
}

/// `std::max` for `RationalTime` (returns `b` only when `a` is strictly less).
fn max_rt(a: RationalTime, b: RationalTime) -> RationalTime {
    if a < b {
        b
    } else {
        a
    }
}

impl TimeRange {
    /// Construct a range from a start time and duration.
    pub const fn new(start_time: RationalTime, duration: RationalTime) -> Self {
        TimeRange { start_time, duration }
    }

    /// Construct a range starting at `start_time` with zero duration (at its rate).
    pub fn from_start(start_time: RationalTime) -> Self {
        TimeRange {
            start_time,
            duration: RationalTime::new(0.0, start_time.rate()),
        }
    }

    /// Construct a range from raw `start`, `duration`, and a shared `rate`.
    pub fn from_floats(start: f64, duration: f64, rate: f64) -> Self {
        TimeRange {
            start_time: RationalTime::new(start, rate),
            duration: RationalTime::new(duration, rate),
        }
    }

    /// The start time.
    pub const fn start_time(self) -> RationalTime {
        self.start_time
    }

    /// The duration.
    pub const fn duration(self) -> RationalTime {
        self.duration
    }

    /// True if start/duration are invalid or the duration is negative.
    pub fn is_invalid_range(self) -> bool {
        self.start_time.is_invalid_time()
            || self.duration.is_invalid_time()
            || self.duration.value() < 0.0
    }

    /// True if start/duration are valid and the duration is `>= 0`.
    pub fn is_valid_range(self) -> bool {
        self.start_time.is_valid_time()
            && self.duration.is_valid_time()
            && self.duration.value() >= 0.0
    }

    /// The exclusive end time (`duration + start`, in the duration's rate).
    pub fn end_time_exclusive(self) -> RationalTime {
        self.duration + self.start_time.rescaled_to(self.duration.rate())
    }

    /// The inclusive end time (the last sample within the range).
    pub fn end_time_inclusive(self) -> RationalTime {
        let et = self.end_time_exclusive();
        if (et - self.start_time.rescaled_to(self.duration.rate())).value() > 1.0 {
            if self.duration.value() != self.duration.value().floor() {
                et.floor()
            } else {
                et - RationalTime::new(1.0, self.duration.rate())
            }
        } else {
            self.start_time
        }
    }

    /// This range with its duration extended by `other`.
    pub fn duration_extended_by(self, other: RationalTime) -> TimeRange {
        TimeRange::new(self.start_time, self.duration + other)
    }

    /// The smallest range enclosing both `self` and `other`.
    pub fn extended_by(self, other: TimeRange) -> TimeRange {
        let new_start = min_rt(self.start_time, other.start_time);
        let new_end = max_rt(self.end_time_exclusive(), other.end_time_exclusive());
        TimeRange::new(
            new_start,
            RationalTime::duration_from_start_end_time(new_start, new_end),
        )
    }

    /// Clamp a time into this range (`[start, end_inclusive]`).
    pub fn clamped_time(self, other: RationalTime) -> RationalTime {
        min_rt(max_rt(other, self.start_time), self.end_time_inclusive())
    }

    /// Clamp a range into this range.
    pub fn clamped_range(self, other: TimeRange) -> TimeRange {
        let r_start = max_rt(other.start_time, self.start_time);
        let r = TimeRange::new(r_start, other.duration);
        let end = min_rt(r.end_time_exclusive(), self.end_time_exclusive());
        TimeRange::new(r.start_time, end - r.start_time)
    }

    // --- Allen's interval relations -------------------------------------
    // The `*_time` variants take a `RationalTime`; the others take a `TimeRange`.

    /// Whether this range contains the time `other` (C++ `contains(RationalTime)`).
    pub fn contains(self, other: RationalTime) -> bool {
        self.start_time <= other && other < self.end_time_exclusive()
    }

    /// Whether this range strictly contains `other` (C++ `contains(TimeRange)`).
    pub fn contains_range(self, other: TimeRange, epsilon_s: f64) -> bool {
        let this_start = self.start_time.to_seconds();
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        let other_end = other.end_time_exclusive().to_seconds();
        greater_than(other_start, this_start, epsilon_s)
            && lesser_than(other_end, this_end, epsilon_s)
    }

    /// Whether this range contains the time `other` (alias of [`Self::contains`]).
    pub fn overlaps_time(self, other: RationalTime) -> bool {
        self.contains(other)
    }

    /// Whether this range overlaps `other` (C++ `overlaps(TimeRange)`).
    pub fn overlaps(self, other: TimeRange, epsilon_s: f64) -> bool {
        let this_start = self.start_time.to_seconds();
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        let other_end = other.end_time_exclusive().to_seconds();
        lesser_than(this_start, other_start, epsilon_s)
            && greater_than(this_end, other_start, epsilon_s)
            && greater_than(other_end, this_end, epsilon_s)
    }

    /// Whether this range ends before the range `other` begins.
    pub fn before(self, other: TimeRange, epsilon_s: f64) -> bool {
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        greater_than(other_start, this_end, epsilon_s)
    }

    /// Whether this range ends before the time `other`.
    pub fn before_time(self, other: RationalTime, epsilon_s: f64) -> bool {
        let this_end = self.end_time_exclusive().to_seconds();
        lesser_than(this_end, other.to_seconds(), epsilon_s)
    }

    /// Whether this range meets `other` (its end touches the other's start).
    pub fn meets(self, other: TimeRange, epsilon_s: f64) -> bool {
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        other_start - this_end <= epsilon_s && other_start - this_end >= 0.0
    }

    /// Whether this range begins where `other` begins and ends earlier.
    pub fn begins(self, other: TimeRange, epsilon_s: f64) -> bool {
        let this_start = self.start_time.to_seconds();
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        let other_end = other.end_time_exclusive().to_seconds();
        (other_start - this_start).abs() <= epsilon_s && lesser_than(this_end, other_end, epsilon_s)
    }

    /// Whether this range begins at the time `other`.
    pub fn begins_time(self, other: RationalTime, epsilon_s: f64) -> bool {
        let this_start = self.start_time.to_seconds();
        (other.to_seconds() - this_start).abs() <= epsilon_s
    }

    /// Whether this range finishes where `other` finishes and starts later.
    pub fn finishes(self, other: TimeRange, epsilon_s: f64) -> bool {
        let this_start = self.start_time.to_seconds();
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        let other_end = other.end_time_exclusive().to_seconds();
        (this_end - other_end).abs() <= epsilon_s && greater_than(this_start, other_start, epsilon_s)
    }

    /// Whether this range finishes at the time `other`.
    pub fn finishes_time(self, other: RationalTime, epsilon_s: f64) -> bool {
        let this_end = self.end_time_exclusive().to_seconds();
        (this_end - other.to_seconds()).abs() <= epsilon_s
    }

    /// Whether this range intersects `other`.
    pub fn intersects(self, other: TimeRange, epsilon_s: f64) -> bool {
        let this_start = self.start_time.to_seconds();
        let this_end = self.end_time_exclusive().to_seconds();
        let other_start = other.start_time.to_seconds();
        let other_end = other.end_time_exclusive().to_seconds();
        lesser_than(this_start, other_end, epsilon_s)
            && greater_than(this_end, other_start, epsilon_s)
    }

    /// Construct a range from a start and an exclusive end time.
    pub fn range_from_start_end_time(
        start_time: RationalTime,
        end_time_exclusive: RationalTime,
    ) -> TimeRange {
        TimeRange::new(
            start_time,
            RationalTime::duration_from_start_end_time(start_time, end_time_exclusive),
        )
    }

    /// Construct a range from a start and an inclusive end time.
    pub fn range_from_start_end_time_inclusive(
        start_time: RationalTime,
        end_time_inclusive: RationalTime,
    ) -> TimeRange {
        TimeRange::new(
            start_time,
            RationalTime::duration_from_start_end_time_inclusive(start_time, end_time_inclusive),
        )
    }
}

/// `lhs - rhs >= epsilon`.
fn greater_than(lhs: f64, rhs: f64, epsilon: f64) -> bool {
    lhs - rhs >= epsilon
}

/// `rhs - lhs >= epsilon`.
fn lesser_than(lhs: f64, rhs: f64, epsilon: f64) -> bool {
    rhs - lhs >= epsilon
}

impl PartialEq for TimeRange {
    /// Equal when start and duration match within [`DEFAULT_EPSILON_S`] seconds.
    fn eq(&self, other: &TimeRange) -> bool {
        let start = self.start_time - other.start_time;
        let duration = self.duration - other.duration;
        start.to_seconds().abs() < DEFAULT_EPSILON_S
            && duration.to_seconds().abs() < DEFAULT_EPSILON_S
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_times() {
        let r = TimeRange::from_floats(10.0, 5.0, 24.0);
        assert_eq!(r.end_time_exclusive(), RationalTime::new(15.0, 24.0));
        assert_eq!(r.end_time_inclusive(), RationalTime::new(14.0, 24.0));
    }

    #[test]
    fn contains_and_overlaps() {
        let r = TimeRange::from_floats(0.0, 10.0, 24.0);
        assert!(r.contains(RationalTime::new(5.0, 24.0)));
        assert!(!r.contains(RationalTime::new(10.0, 24.0))); // exclusive end
        let other = TimeRange::from_floats(5.0, 10.0, 24.0);
        assert!(r.overlaps(other, DEFAULT_EPSILON_S));
    }

    #[test]
    fn extend_and_clamp() {
        let a = TimeRange::from_floats(0.0, 5.0, 24.0);
        let b = TimeRange::from_floats(10.0, 5.0, 24.0);
        let ext = a.extended_by(b);
        assert_eq!(ext.start_time(), RationalTime::new(0.0, 24.0));
        assert_eq!(ext.end_time_exclusive(), RationalTime::new(15.0, 24.0));

        let clamped = a.clamped_time(RationalTime::new(100.0, 24.0));
        assert_eq!(clamped, a.end_time_inclusive());
    }

    #[test]
    fn allen_before_meets() {
        let a = TimeRange::from_floats(0.0, 5.0, 24.0);
        let b = TimeRange::from_floats(10.0, 5.0, 24.0);
        let c = TimeRange::from_floats(5.0, 5.0, 24.0);
        assert!(a.before(b, DEFAULT_EPSILON_S));
        assert!(a.meets(c, DEFAULT_EPSILON_S));
        assert!(!a.meets(b, DEFAULT_EPSILON_S));
    }
}
