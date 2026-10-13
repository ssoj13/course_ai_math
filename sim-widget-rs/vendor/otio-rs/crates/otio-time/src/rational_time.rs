//! [`RationalTime`] — a measure of time as `value` samples at a given `rate`.
//!
//! Faithful port of `opentime/rationalTime.{h,cpp}`. All arithmetic, rescaling,
//! SMPTE timecode (including drop-frame) and time-string handling reproduce the
//! reference behavior so values round-trip with OTIO.

use crate::error::TimeError;

/// The ten SMPTE timecode rates (ST 12-1:2014).
///
/// Note: the reference declares `std::array<double, 11>` but lists only these
/// ten values, leaving a trailing value-initialized `0.0`. That stray `0.0` is
/// an upstream array-size typo (it would make rate `0.0` report as a valid SMPTE
/// rate and could be picked as a "nearest" rate); we intentionally omit it.
const SMPTE_TIMECODE_RATES: [f64; 10] = [
    24000.0 / 1001.0,
    24.0,
    25.0,
    30000.0 / 1001.0,
    30.0,
    48000.0 / 1001.0,
    48.0,
    50.0,
    60000.0 / 1001.0,
    60.0,
];

/// The two drop-frame timecode rates.
const DROPFRAME_TIMECODE_RATES: [f64; 2] = [30000.0 / 1001.0, 60000.0 / 1001.0];

/// Options for drop-frame timecode generation (mirrors `IsDropFrameRate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsDropFrameRate {
    /// Infer drop-frame from the rate.
    InferFromRate,
    /// Force non-drop-frame.
    ForceNo,
    /// Force drop-frame (errors if the rate does not support it).
    ForceYes,
}

/// A time expressed as a `value` number of samples at a `rate` (samples/second).
///
/// Serde (feature `serde`) writes the two fields verbatim - a persisted time keeps its OWN rate,
/// which is the whole point: a 25 fps source stored inside a 24 fps timeline must still say 25, or
/// reloading the file silently reinterprets it.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RationalTime {
    value: f64,
    rate: f64,
}

impl Default for RationalTime {
    /// `value = 0`, `rate = 1` — matches the C++ default constructor.
    fn default() -> Self {
        RationalTime { value: 0.0, rate: 1.0 }
    }
}

impl RationalTime {
    /// Construct a time with the given `value` and `rate`.
    pub const fn new(value: f64, rate: f64) -> Self {
        RationalTime { value, rate }
    }

    /// The time value (number of samples).
    pub const fn value(self) -> f64 {
        self.value
    }

    /// The rate (samples per second).
    pub const fn rate(self) -> f64 {
        self.rate
    }

    /// True if `value`/`rate` is NaN, or `rate <= 0`.
    pub fn is_invalid_time(self) -> bool {
        if self.rate.is_nan() || self.value.is_nan() {
            true
        } else {
            self.rate <= 0.0
        }
    }

    /// True if both fields are finite numbers and `rate > 0`.
    pub fn is_valid_time(self) -> bool {
        !self.rate.is_nan() && !self.value.is_nan() && self.rate > 0.0
    }

    /// This time's `value` expressed at `new_rate`.
    pub fn value_rescaled_to(self, new_rate: f64) -> f64 {
        if new_rate == self.rate {
            self.value
        } else if self.rate > 0.0 {
            (self.value * new_rate) / self.rate
        } else {
            0.0
        }
    }

    /// This time's `value` expressed at another time's rate.
    pub fn value_rescaled_to_time(self, rt: RationalTime) -> f64 {
        self.value_rescaled_to(rt.rate)
    }

    /// This time converted to `new_rate`.
    pub fn rescaled_to(self, new_rate: f64) -> RationalTime {
        RationalTime::new(self.value_rescaled_to(new_rate), new_rate)
    }

    /// This time converted to another time's rate.
    pub fn rescaled_to_time(self, rt: RationalTime) -> RationalTime {
        RationalTime::new(self.value_rescaled_to(rt.rate), rt.rate)
    }

    /// Whether this time is within `delta` seconds-at-`other`-rate of `other`.
    pub fn almost_equal(self, other: RationalTime, delta: f64) -> bool {
        (self.value_rescaled_to(other.rate) - other.value).abs() <= delta
    }

    /// Whether `value` and `rate` are bit-for-bit equal (no rescaling).
    pub fn strictly_equal(self, other: RationalTime) -> bool {
        self.value == other.value && self.rate == other.rate
    }

    /// Largest integer time not greater than this one (same rate).
    pub fn floor(self) -> RationalTime {
        RationalTime::new(self.value.floor(), self.rate)
    }

    /// Smallest integer time not less than this one (same rate).
    pub fn ceil(self) -> RationalTime {
        RationalTime::new(self.value.ceil(), self.rate)
    }

    /// Nearest integer time to this one (same rate).
    pub fn round(self) -> RationalTime {
        RationalTime::new(self.value.round(), self.rate)
    }

    /// Duration `[start, end)` (exclusive), in the rate of `start_time`.
    pub fn duration_from_start_end_time(
        start_time: RationalTime,
        end_time_exclusive: RationalTime,
    ) -> RationalTime {
        if start_time.rate == end_time_exclusive.rate {
            RationalTime::new(end_time_exclusive.value - start_time.value, start_time.rate)
        } else {
            RationalTime::new(
                end_time_exclusive.value_rescaled_to_time(start_time) - start_time.value,
                start_time.rate,
            )
        }
    }

    /// Duration `[start, end]` (inclusive), in the rate of `start_time`.
    pub fn duration_from_start_end_time_inclusive(
        start_time: RationalTime,
        end_time_inclusive: RationalTime,
    ) -> RationalTime {
        if start_time.rate == end_time_inclusive.rate {
            RationalTime::new(
                end_time_inclusive.value - start_time.value + 1.0,
                start_time.rate,
            )
        } else {
            RationalTime::new(
                end_time_inclusive.value_rescaled_to_time(start_time) - start_time.value + 1.0,
                start_time.rate,
            )
        }
    }

    /// Whether `rate` is one of the SMPTE timecode rates.
    pub fn is_smpte_timecode_rate(rate: f64) -> bool {
        SMPTE_TIMECODE_RATES.contains(&rate)
    }

    /// The SMPTE timecode rate nearest to `rate`.
    pub fn nearest_smpte_timecode_rate(rate: f64) -> f64 {
        let mut nearest_rate = 0.0;
        let mut min_diff = f64::MAX;
        for &smpte_rate in &SMPTE_TIMECODE_RATES {
            if smpte_rate == rate {
                return rate;
            }
            let diff = (rate - smpte_rate).abs();
            if diff >= min_diff {
                continue;
            }
            min_diff = diff;
            nearest_rate = smpte_rate;
        }
        nearest_rate
    }

    /// Construct a time from a (truncated) `frame` number at `rate`.
    pub fn from_frames(frame: f64, rate: f64) -> RationalTime {
        RationalTime::new(frame.trunc(), rate)
    }

    /// Construct a time from `seconds` at `rate`.
    pub fn from_seconds_with_rate(seconds: f64, rate: f64) -> RationalTime {
        RationalTime::new(seconds, 1.0).rescaled_to(rate)
    }

    /// Construct a time from `seconds` at rate 1.
    pub fn from_seconds(seconds: f64) -> RationalTime {
        RationalTime::new(seconds, 1.0)
    }

    /// The (truncated) frame number at this time's own rate.
    pub fn to_frames(self) -> i32 {
        self.value as i32
    }

    /// The (truncated) frame number at `rate`.
    pub fn to_frames_at_rate(self, rate: f64) -> i32 {
        self.value_rescaled_to(rate) as i32
    }

    /// This time in seconds.
    pub fn to_seconds(self) -> f64 {
        self.value_rescaled_to(1.0)
    }

    /// Parse a SMPTE timecode string (`"HH:MM:SS:FF"` / `"HH:MM:SS;FF"`) at `rate`.
    pub fn from_timecode(timecode: &str, rate: f64) -> Result<RationalTime, TimeError> {
        if !Self::is_smpte_timecode_rate(rate) {
            return Err(TimeError::InvalidTimecodeRate(rate));
        }

        let mut rate_is_dropframe = is_dropframe_rate(rate);

        if timecode.contains(';') {
            if !rate_is_dropframe {
                return Err(TimeError::InvalidRateForDropFrameTimecode(rate));
            }
        } else {
            rate_is_dropframe = false;
        }

        // Fields live at fixed offsets: HH:MM:SS<sep>FF (separators at 2/5/8).
        let b = timecode.as_bytes();
        if b.len() < 11 {
            return Err(TimeError::InvalidTimecodeString(timecode.to_string()));
        }
        let field = |s: usize| -> Result<i32, TimeError> {
            std::str::from_utf8(&b[s..s + 2])
                .ok()
                .and_then(|t| t.trim().parse::<i32>().ok())
                .ok_or_else(|| TimeError::InvalidTimecodeString(timecode.to_string()))
        };
        let hours = field(0)?;
        let minutes = field(3)?;
        let seconds = field(6)?;
        let frames = field(9)?;

        let nominal_fps = rate.ceil() as i32;
        if frames >= nominal_fps {
            return Err(TimeError::TimecodeRateMismatch {
                timecode: timecode.to_string(),
                max_frame: nominal_fps - 1,
            });
        }

        let mut dropframes = 0;
        if rate_is_dropframe {
            if rate == 30000.0 / 1001.0 {
                dropframes = 2;
            } else if rate == 60000.0 / 1001.0 {
                dropframes = 4;
            }
        }

        let total_minutes = hours * 60 + minutes;
        let value = (((total_minutes * 60) + seconds) * nominal_fps + frames)
            - (dropframes * (total_minutes - (total_minutes / 10)));

        Ok(RationalTime::new(value as f64, rate))
    }

    /// Parse a time string (`"[-]HH:MM:SS.sss"`, fractional seconds allowed) at `rate`.
    ///
    /// Up to three colon-separated fields are accepted (seconds, minutes, hours
    /// from the right). Minutes/seconds fields must be `< 60`. No sign or exponent
    /// is accepted in any field (the reference's right-to-left scan rejects a `-`
    /// or `+` before it ever reaches a signed parse — verified against Python
    /// `opentime`). This reproduces `from_time_string` for all well-formed inputs.
    pub fn from_time_string(time_string: &str, rate: f64) -> Result<RationalTime, TimeError> {
        if !Self::is_smpte_timecode_rate(rate) {
            return Err(TimeError::InvalidTimecodeRate(rate));
        }

        let err = || TimeError::InvalidTimeString(time_string.to_string());
        let parts: Vec<&str> = time_string.split(':').collect();
        if parts.len() > 3 {
            return Err(err());
        }
        const POWER: [f64; 3] = [1.0, 60.0, 3600.0]; // seconds, minutes, hours

        let n = parts.len();
        let mut accumulator = 0.0;
        for (i, part) in parts.iter().enumerate() {
            let radix = n - 1 - i; // rightmost field has radix 0
            let leftmost = i == 0;

            let val = if part.is_empty() {
                0.0
            } else {
                // No sign or exponent: ASCII digits with at most one '.'.
                if !is_plain_decimal(part) {
                    return Err(err());
                }
                let v = part.parse::<f64>().map_err(|_| err())?;
                // Minutes/seconds (radix < 2) must be < 60, except the leftmost
                // field, which carries the unbounded high-order value.
                if !leftmost && radix < 2 && v >= 60.0 {
                    return Err(err());
                }
                v
            };
            accumulator += val * POWER[radix];
        }

        Ok(Self::from_seconds(accumulator).rescaled_to(rate))
    }

    /// Convert to timecode at `rate` with the given drop-frame policy.
    pub fn to_timecode(
        self,
        rate: f64,
        drop_frame: IsDropFrameRate,
    ) -> Result<String, TimeError> {
        let frames_in_target_rate = self.value_rescaled_to(rate);
        if frames_in_target_rate < 0.0 {
            return Err(TimeError::NegativeValue);
        }

        // Snap a near-SMPTE rate (e.g. 29.97) onto the exact SMPTE rate.
        let nearest = Self::nearest_smpte_timecode_rate(rate);
        if (nearest - rate).abs() > 0.1 {
            return Err(TimeError::InvalidTimecodeRate(rate));
        }
        let mut rate = nearest;

        let mut rate_is_dropframe = is_dropframe_rate(rate);
        if drop_frame == IsDropFrameRate::ForceYes && !rate_is_dropframe {
            return Err(TimeError::InvalidRateForDropFrameTimecode(rate));
        }
        match drop_frame {
            IsDropFrameRate::InferFromRate => {}
            IsDropFrameRate::ForceYes => rate_is_dropframe = true,
            IsDropFrameRate::ForceNo => rate_is_dropframe = false,
        }

        let mut dropframes = 0;
        let mut div = ':';
        if !rate_is_dropframe {
            if rate.round() == 24.0 {
                rate = 24.0;
            }
        } else {
            if rate == 30000.0 / 1001.0 {
                dropframes = 2;
            } else if rate == 60000.0 / 1001.0 {
                dropframes = 4;
            }
            div = ';';
        }

        let frames_per_hour = (rate * 60.0 * 60.0).round() as i64;
        let frames_per_24_hours = frames_per_hour * 24;
        let frames_per_10_minutes = (rate * 60.0 * 10.0).round() as i64;
        let frames_per_minute = ((rate.round() * 60.0) as i64) - dropframes;

        // Roll the clock over after 24 hours.
        let mut value = frames_in_target_rate % (frames_per_24_hours as f64);

        if rate_is_dropframe {
            let ten_minute_chunks = (value / frames_per_10_minutes as f64).floor();
            let frames_over_ten_minutes = value % frames_per_10_minutes as f64;

            if frames_over_ten_minutes > dropframes as f64 {
                value += (dropframes as f64) * 9.0 * ten_minute_chunks
                    + (dropframes as f64)
                        * ((frames_over_ten_minutes - dropframes as f64)
                            / frames_per_minute as f64)
                            .floor();
            } else {
                value += (dropframes as f64) * 9.0 * ten_minute_chunks;
            }
        }

        let nominal_fps = rate.ceil() as i64;
        let frames = (value % nominal_fps as f64) as i64;
        let seconds_total = (value / nominal_fps as f64).floor() as i64;
        let seconds = seconds_total % 60;
        let minutes = ((seconds_total / 60) as f64 % 60.0) as i64;
        let hours = (seconds_total / 60) / 60;

        Ok(format!(
            "{:02}:{:02}:{:02}{}{:02}",
            hours, minutes, seconds, div, frames
        ))
    }

    /// Convert to timecode using this time's own rate and inferred drop-frame.
    pub fn to_timecode_default(self) -> Result<String, TimeError> {
        self.to_timecode(self.rate, IsDropFrameRate::InferFromRate)
    }

    /// Convert to timecode, snapping `rate` to the nearest SMPTE rate first.
    pub fn to_nearest_timecode(
        self,
        rate: f64,
        drop_frame: IsDropFrameRate,
    ) -> Result<String, TimeError> {
        self.to_timecode(Self::nearest_smpte_timecode_rate(rate), drop_frame)
    }

    /// Convert to a time string of the form `"[-]HH:MM:SS.ssssss"`.
    pub fn to_time_string(self) -> String {
        let total_seconds = self.to_seconds();
        let is_negative = total_seconds.is_sign_negative();
        let total_seconds = total_seconds.abs();

        const PER_MINUTE: f64 = 60.0;
        const PER_HOUR: f64 = PER_MINUTE * 60.0;
        const PER_DAY: f64 = PER_HOUR * 24.0;

        let hour_units = total_seconds % PER_DAY;
        let hours = (hour_units / PER_HOUR).floor() as i32;
        let minute_units = hour_units % PER_HOUR;
        let minutes = (minute_units / PER_MINUTE).floor() as i32;
        let seconds = minute_units % PER_MINUTE;

        let intpart = seconds.trunc();
        let fractpart = seconds - intpart;

        let seconds_str = format!("{:02}", intpart as i32);

        // Reference: printf("%.7g", frac), drop the leading '0', cap to 7 chars,
        // and use ".0" when there is no fraction.
        let mut micro: String = fmt_g(fractpart, 7).chars().skip(1).collect();
        if micro.is_empty() {
            micro = ".0".to_string();
        } else if micro.len() > 7 {
            micro.truncate(7);
        }

        let sign = if is_negative { "-" } else { "" };
        format!("{}{:02}:{:02}:{}{}", sign, hours, minutes, seconds_str, micro)
    }
}

/// True if `s` is a plain non-negative decimal: ASCII digits with at most one
/// '.', no sign, no exponent. Matches the per-field character set accepted by
/// the reference `from_time_string`.
fn is_plain_decimal(s: &str) -> bool {
    let mut dots = 0;
    let mut digits = 0;
    for c in s.chars() {
        match c {
            '0'..='9' => digits += 1,
            '.' => dots += 1,
            _ => return false,
        }
    }
    dots <= 1 && digits >= 1
}

/// Whether `rate` is one of the two drop-frame rates (exact match).
fn is_dropframe_rate(rate: f64) -> bool {
    DROPFRAME_TIMECODE_RATES.contains(&rate)
}

/// Emulate C `printf("%.*g", precision, v)` for the non-negative fractional part.
///
/// `%g` uses the shorter of `%e`/`%f` with `precision` significant digits and
/// strips trailing zeros. We only ever feed it `fractpart` in `[0, 1)`, but the
/// full behavior is reproduced so [`RationalTime::to_time_string`] matches OTIO.
fn fmt_g(v: f64, precision: usize) -> String {
    let p = precision.max(1) as i32;
    if v == 0.0 {
        return "0".to_string();
    }
    let exp = v.abs().log10().floor() as i32;
    if exp < -4 || exp >= p {
        // Scientific notation, formatted like C (`e`, signed 2-digit exponent).
        let mant = format!("{:.*e}", (p - 1).max(0) as usize, v);
        if let Some(epos) = mant.find('e') {
            let (m, e) = mant.split_at(epos);
            let mut mstr = m.to_string();
            if mstr.contains('.') {
                while mstr.ends_with('0') {
                    mstr.pop();
                }
                if mstr.ends_with('.') {
                    mstr.pop();
                }
            }
            let exp_num: i32 = e[1..].parse().unwrap_or(0);
            return format!(
                "{}e{}{:02}",
                mstr,
                if exp_num < 0 { "-" } else { "+" },
                exp_num.abs()
            );
        }
        mant
    } else {
        let decimals = (p - 1 - exp).max(0) as usize;
        let mut s = format!("{:.*}", decimals, v);
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Operators (mirror the C++ friend operators).
// ---------------------------------------------------------------------------

impl std::ops::Add for RationalTime {
    type Output = RationalTime;
    fn add(self, rhs: RationalTime) -> RationalTime {
        if self.rate < rhs.rate {
            RationalTime::new(self.value_rescaled_to(rhs.rate) + rhs.value, rhs.rate)
        } else {
            RationalTime::new(rhs.value_rescaled_to(self.rate) + self.value, self.rate)
        }
    }
}

impl std::ops::Sub for RationalTime {
    type Output = RationalTime;
    fn sub(self, rhs: RationalTime) -> RationalTime {
        if self.rate < rhs.rate {
            RationalTime::new(self.value_rescaled_to(rhs.rate) - rhs.value, rhs.rate)
        } else {
            RationalTime::new(self.value - rhs.value_rescaled_to(self.rate), self.rate)
        }
    }
}

impl std::ops::Neg for RationalTime {
    type Output = RationalTime;
    fn neg(self) -> RationalTime {
        RationalTime::new(-self.value, self.rate)
    }
}

impl std::ops::AddAssign for RationalTime {
    fn add_assign(&mut self, other: RationalTime) {
        if self.rate < other.rate {
            self.value = other.value + self.value_rescaled_to(other.rate);
            self.rate = other.rate;
        } else {
            self.value += other.value_rescaled_to(self.rate);
        }
    }
}

impl std::ops::SubAssign for RationalTime {
    fn sub_assign(&mut self, other: RationalTime) {
        if self.rate < other.rate {
            self.value = self.value_rescaled_to(other.rate) - other.value;
            self.rate = other.rate;
        } else {
            self.value -= other.value_rescaled_to(self.rate);
        }
    }
}

impl PartialEq for RationalTime {
    /// Equality rescales `other` to this rate first (matches the C++ `operator==`).
    /// Use [`RationalTime::strictly_equal`] for exact field comparison.
    fn eq(&self, other: &RationalTime) -> bool {
        self.value_rescaled_to(other.rate) == other.value
    }
}

impl PartialOrd for RationalTime {
    /// Ordering of two times. Direction (`Less`/`Greater`) compares `value / rate`
    /// (seconds), matching the C++ `<`/`>` operators.
    ///
    /// Equality, however, is decided with the **exact same predicate** that
    /// [`PartialEq::eq`] uses (rescale-multiply: `self.value_rescaled_to(other.rate)
    /// == other.value`), *not* the seconds division. Rust's std contract requires
    /// `a == b` ⇔ `a.partial_cmp(&b) == Some(Equal)`; the rescale and the division
    /// disagree by up to one ULP at rate boundaries, and that disagreement silently
    /// corrupts `sort`, `binary_search`, `min`/`max`, `dedup` and `BTreeMap`/`BTreeSet`
    /// keyed on `RationalTime` (and, transitively, on `TimeRange`, whose `min`/`max`
    /// helpers compare via this `<`). Testing equality here with `eq`'s own predicate
    /// keeps the two in lock-step without touching `eq`'s C++-faithful semantics.
    ///
    /// `None` is returned only for genuinely unordered IEEE operands — i.e. when the
    /// seconds comparison sees a `NaN` (e.g. `0.0 / 0.0` from a zero rate with a zero
    /// value). That is the one unavoidable case where no total order exists; callers
    /// relying on `sort`/`min`/`max` must keep their times valid (see
    /// [`RationalTime::is_valid_time`]).
    fn partial_cmp(&self, other: &RationalTime) -> Option<std::cmp::Ordering> {
        use std::cmp::Ordering;
        // Equality first, via the SAME predicate `eq` uses — see the doc above.
        if self.value_rescaled_to(other.rate) == other.value {
            return Some(Ordering::Equal);
        }
        // `eq` already said "not equal": order by seconds, matching C++ `<`/`>`.
        match (self.value / self.rate).partial_cmp(&(other.value / other.rate)) {
            // Sub-ULP corner: seconds compare equal while `eq` did not. Reporting
            // `Equal` here would re-break the very contract we are upholding, so
            // resolve the direction with `eq`'s own metric (guaranteed non-equal,
            // hence `Less`/`Greater`).
            Some(Ordering::Equal) => {
                self.value_rescaled_to(other.rate).partial_cmp(&other.value)
            }
            ord => ord,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn rescale_and_seconds() {
        let t = RationalTime::new(48.0, 48.0);
        approx(t.to_seconds(), 1.0);
        approx(t.value_rescaled_to(24.0), 24.0);
        assert_eq!(RationalTime::new(24.0, 24.0), t); // equal after rescale
        assert!(!RationalTime::new(24.0, 24.0).strictly_equal(t));
    }

    #[test]
    fn arithmetic_keeps_finer_rate() {
        let a = RationalTime::new(1.0, 24.0);
        let b = RationalTime::new(1.0, 48.0);
        let sum = a + b;
        assert_eq!(sum.rate(), 48.0);
        approx(sum.value(), 3.0);
    }

    #[test]
    fn floor_ceil_round() {
        let t = RationalTime::new(1.5, 24.0);
        approx(t.floor().value(), 1.0);
        approx(t.ceil().value(), 2.0);
        approx(t.round().value(), 2.0);
    }

    #[test]
    fn timecode_non_dropframe_roundtrip() {
        for &rate in &[24.0, 25.0, 30.0, 48.0, 50.0, 60.0] {
            for frame in [0i32, 1, 100, 1001, 86399] {
                let t = RationalTime::new(frame as f64, rate);
                let tc = t.to_timecode(rate, IsDropFrameRate::InferFromRate).unwrap();
                let back = RationalTime::from_timecode(&tc, rate).unwrap();
                assert_eq!(back.to_frames(), frame, "rate {rate} frame {frame} tc {tc}");
            }
        }
    }

    #[test]
    fn timecode_dropframe_2997() {
        let rate = 30000.0 / 1001.0;
        // 00:00:00;00 is frame 0; first dropped frames happen at minute boundaries.
        let zero = RationalTime::from_timecode("00:00:00;00", rate).unwrap();
        approx(zero.value(), 0.0);
        // Round-trip a spread of frames.
        for frame in [0i32, 30, 1800, 17982, 100000] {
            let t = RationalTime::new(frame as f64, rate);
            let tc = t.to_timecode(rate, IsDropFrameRate::ForceYes).unwrap();
            assert!(tc.contains(';'), "expected drop-frame separator in {tc}");
            let back = RationalTime::from_timecode(&tc, rate).unwrap();
            assert_eq!(back.to_frames(), frame, "frame {frame} tc {tc}");
        }
    }

    #[test]
    fn timecode_errors() {
        assert_eq!(
            RationalTime::from_timecode("00:00:00:00", 29.0),
            Err(TimeError::InvalidTimecodeRate(29.0))
        );
        // ';' on a non-dropframe rate.
        assert!(matches!(
            RationalTime::from_timecode("00:00:00;00", 30.0),
            Err(TimeError::InvalidRateForDropFrameTimecode(_))
        ));
        // Frame field beyond nominal fps.
        assert!(matches!(
            RationalTime::from_timecode("00:00:00:30", 30.0),
            Err(TimeError::TimecodeRateMismatch { .. })
        ));
        // Negative to timecode.
        assert_eq!(
            RationalTime::new(-1.0, 24.0).to_timecode(24.0, IsDropFrameRate::InferFromRate),
            Err(TimeError::NegativeValue)
        );
    }

    #[test]
    fn nearest_smpte() {
        approx(RationalTime::nearest_smpte_timecode_rate(29.97), 30000.0 / 1001.0);
        approx(RationalTime::nearest_smpte_timecode_rate(23.976), 24000.0 / 1001.0);
        approx(RationalTime::nearest_smpte_timecode_rate(24.0), 24.0);
    }

    #[test]
    fn time_string_roundtrip() {
        let t = RationalTime::from_time_string("01:00:00.5", 24.0).unwrap();
        approx(t.to_seconds(), 3600.5);
        assert_eq!(t.to_time_string(), "01:00:00.5");

        let z = RationalTime::new(0.0, 24.0);
        assert_eq!(z.to_time_string(), "00:00:00.0");

        // Single field is seconds, may exceed 60.
        let s = RationalTime::from_time_string("90", 24.0).unwrap();
        approx(s.to_seconds(), 90.0);
        // Minutes/seconds >= 60 in multi-field form is rejected.
        assert!(RationalTime::from_time_string("1:90", 24.0).is_err());
    }

    #[test]
    fn time_string_rejects_signs() {
        // Reference parity (verified vs Python opentime): from_time_string rejects
        // ANY sign in ANY field — a single-field '-90' and '+01:00:00.0' included.
        for s in ["-90", "+90", "-1:30", "-01:00:00.0", "+01:00:00.0"] {
            assert!(
                RationalTime::from_time_string(s, 24.0).is_err(),
                "expected '{s}' to be rejected"
            );
        }
        // to_time_string is intentionally one-way: it still emits '-' for negative
        // times, even though from_time_string will not parse that back.
        let neg = RationalTime::new(-24.0, 24.0);
        assert_eq!(neg.to_time_string(), "-00:00:01.0");
        assert!(RationalTime::from_time_string(&neg.to_time_string(), 24.0).is_err());
    }
}
