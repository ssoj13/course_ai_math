//! Value and time scales, and the tick sets drawn against them.
//!
//! Kept apart from the painting so the arithmetic that decides *where the lines
//! go* can be tested without a window. Ugly axes are almost always a tick
//! problem — labels at 0.7331, 1.4662, 2.1993 rather than at 1, 2, 3 — and that
//! is a pure function of the range and the space available.

/// How a pane maps values to screen space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scale {
    /// Equal value steps. The default, and right for anything oscillating
    /// around zero (MACD, RSI, a histogram).
    #[default]
    Linear,
    /// Equal *ratio* steps. What a long price history wants: a move from 10 to
    /// 20 and one from 100 to 200 are both a doubling and should occupy the
    /// same height. Non-positive values are clamped away from zero rather than
    /// dropped, so a series that touches 0 still draws.
    Log,
    /// Percent change from the first visible value — how two instruments of
    /// very different price are compared on one pane.
    Percent,
}

/// A value range, in data units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub lo: f64,
    pub hi: f64,
}

impl Range {
    pub fn new(lo: f64, hi: f64) -> Self {
        Self { lo, hi }
    }

    /// An empty range that [`Range::include`] grows from.
    pub fn empty() -> Self {
        Self {
            lo: f64::INFINITY,
            hi: f64::NEG_INFINITY,
        }
    }

    /// True for the sentinel [`Self::empty`] leaves behind, and for a range
    /// that has taken in a NaN — neither can be drawn, and both must answer the
    /// same way or a chart renders against nonsense bounds.
    pub fn is_empty(&self) -> bool {
        self.lo.is_nan() || self.hi.is_nan() || self.lo > self.hi
    }

    pub fn include(&mut self, v: f64) {
        if v.is_finite() {
            self.lo = self.lo.min(v);
            self.hi = self.hi.max(v);
        }
    }

    pub fn span(&self) -> f64 {
        self.hi - self.lo
    }

    pub fn center(&self) -> f64 {
        (self.lo + self.hi) * 0.5
    }

    /// Grow by `frac` of the span on both sides, so candles do not touch the
    /// pane edge. A degenerate range (one flat value) is widened to something
    /// drawable instead of collapsing to a division by zero.
    pub fn padded(self, frac: f64) -> Self {
        if self.is_empty() {
            return Self::new(0.0, 1.0);
        }
        let span = self.span();
        if span <= f64::EPSILON * self.hi.abs().max(1.0) {
            let e = self.hi.abs().max(1.0) * 0.05;
            return Self::new(self.lo - e, self.hi + e);
        }
        let pad = span * frac;
        Self::new(self.lo - pad, self.hi + pad)
    }
}

/// Fraction of the way from `lo` to `hi`, under `scale`.
///
/// Returns `0..=1` for values inside the range; outside values return outside
/// that, which is what lets a series be clipped by the pane rather than by the
/// maths.
pub fn norm(scale: Scale, r: Range, v: f64) -> f64 {
    match scale {
        Scale::Linear | Scale::Percent => {
            let span = r.span();
            if span.abs() < f64::MIN_POSITIVE {
                0.5
            } else {
                (v - r.lo) / span
            }
        }
        Scale::Log => {
            // Clamped rather than skipped: a series touching zero must still
            // draw, and `ln(0)` would take the whole pane with it.
            let f = |x: f64| x.max(f64::MIN_POSITIVE).ln();
            let (lo, hi, v) = (f(r.lo), f(r.hi), f(v));
            let span = hi - lo;
            if span.abs() < f64::MIN_POSITIVE {
                0.5
            } else {
                (v - lo) / span
            }
        }
    }
}

/// One axis division: where it sits and what it says.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    pub value: f64,
    pub label: String,
    /// Major ticks get a label and a brighter line; minor ones only a line.
    pub major: bool,
}

/// A "nice" step at or above `raw`: 1, 2, 2.5 or 5 times a power of ten.
///
/// 2.5 is included because without it a range that wants ~4 divisions per
/// decade has to choose between 2 (too many lines) and 5 (too few).
fn nice_step(raw: f64) -> f64 {
    // `is_finite` first, so NaN and the infinities are out before the ordering
    // question is asked at all — a comparison against NaN answers `false` both
    // ways round and would let one through.
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let step = if norm <= 1.0 {
        1.0
    } else if norm <= 2.0 {
        2.0
    } else if norm <= 2.5 {
        2.5
    } else if norm <= 5.0 {
        5.0
    } else {
        10.0
    };
    step * mag
}

/// Decimals needed to tell consecutive ticks apart at `step`.
///
/// Without this an axis over 0.001-wide data prints the same label several
/// times, which reads as a broken chart rather than as a rounding choice.
fn decimals_for(step: f64) -> usize {
    if !step.is_finite() || step <= 0.0 {
        return 2;
    }
    let d = -step.log10().floor();
    d.clamp(0.0, 8.0) as usize
}

/// Format a value for an axis label at `step` resolution.
pub fn format_value(v: f64, step: f64) -> String {
    let d = decimals_for(step);
    // Negative zero prints as "-0", which on an axis reads as a mistake. It
    // arrives easily: a tick at exactly zero computed as `ceil(lo/step)*step`
    // keeps the sign of a negative `lo`.
    let v = if v == 0.0 { 0.0 } else { v };
    let a = v.abs();
    // Thousands get a suffix rather than eight digits: an axis is read at a
    // glance, and 1.09B is the number a person wanted from 1,090,666,062.
    if a >= 1e9 {
        format!("{:.2}B", v / 1e9)
    } else if a >= 1e6 {
        format!("{:.2}M", v / 1e6)
    } else if a >= 1e5 {
        format!("{:.1}K", v / 1e3)
    } else {
        format!("{v:.d$}")
    }
}

/// Value ticks across `r`, aiming for about `target` of them.
///
/// `target` comes from the pane's height divided by a comfortable label
/// spacing, so a short pane gets fewer lines instead of a solid block of text.
pub fn value_ticks(scale: Scale, r: Range, target: usize) -> Vec<Tick> {
    let target = target.clamp(2, 40);
    if r.is_empty() {
        return Vec::new();
    }
    match scale {
        Scale::Log if r.lo > 0.0 && r.hi / r.lo >= 10.0 => log_ticks(r, target),
        _ => linear_ticks(r, target),
    }
}

fn linear_ticks(r: Range, target: usize) -> Vec<Tick> {
    let step = nice_step(r.span() / target as f64);
    if !step.is_finite() || step <= 0.0 {
        return Vec::new();
    }
    let first = (r.lo / step).ceil() * step;
    let mut out = Vec::new();
    let mut v = first;
    // Bounded by the target rather than by the loop condition alone: a
    // pathological range must not spin.
    while v <= r.hi && out.len() <= target * 4 {
        out.push(Tick {
            value: v,
            label: format_value(v, step),
            major: true,
        });
        v += step;
    }
    out
}

/// Decade ticks (1, 10, 100…) with 2/5 subdivisions when there is room.
fn log_ticks(r: Range, target: usize) -> Vec<Tick> {
    let lo_e = r.lo.max(f64::MIN_POSITIVE).log10().floor() as i32;
    let hi_e = r.hi.max(f64::MIN_POSITIVE).log10().ceil() as i32;
    let decades = (hi_e - lo_e).max(1) as usize;
    let subs: &[f64] = if decades * 3 <= target {
        &[1.0, 2.0, 5.0]
    } else {
        &[1.0]
    };
    let mut out = Vec::new();
    for e in lo_e..=hi_e {
        let mag = 10f64.powi(e);
        for s in subs {
            let v = s * mag;
            if v >= r.lo && v <= r.hi {
                out.push(Tick {
                    value: v,
                    label: format_value(v, mag),
                    major: (*s - 1.0).abs() < f64::EPSILON,
                });
            }
        }
    }
    out
}

/// Time steps a person reads: seconds, minutes, hours, days, months, years.
///
/// Wall-clock steps rather than nice round *milliseconds*, because 86_400_000
/// is a day and 100_000_000 is nothing at all.
const TIME_STEPS_MS: &[i64] = &[
    1_000,
    5_000,
    15_000,
    30_000,
    60_000,
    5 * 60_000,
    15 * 60_000,
    30 * 60_000,
    3_600_000,
    4 * 3_600_000,
    12 * 3_600_000,
    86_400_000,
    7 * 86_400_000,
    30 * 86_400_000,
    90 * 86_400_000,
    365 * 86_400_000,
];

/// Time ticks across `[t0, t1]` in epoch millis, aiming for about `target`.
///
/// Labels adapt to the step: a 5-minute chart is labelled by clock time, a
/// multi-year one by year, because printing the full stamp at every division is
/// what makes an axis unreadable.
pub fn time_ticks(t0: i64, t1: i64, target: usize) -> Vec<Tick> {
    let target = target.clamp(2, 24) as i64;
    if t1 <= t0 {
        return Vec::new();
    }
    let raw = (t1 - t0) / target.max(1);
    let step = TIME_STEPS_MS
        .iter()
        .copied()
        .find(|s| *s >= raw)
        .unwrap_or_else(|| {
            // Past a year, keep going in whole years so labels stay on
            // boundaries a person expects.
            let years = (raw / (365 * 86_400_000)).max(1);
            years * 365 * 86_400_000
        });
    let first = t0.div_euclid(step) * step;
    let first = if first < t0 { first + step } else { first };
    let mut out = Vec::new();
    let mut t = first;
    while t <= t1 && out.len() <= target as usize * 4 {
        out.push(Tick {
            value: t as f64,
            label: format_time(t, step),
            major: true,
        });
        t += step;
    }
    out
}

/// Civil date/time from epoch millis, without pulling a date crate in.
///
/// A chart axis needs Y/M/D h:m and nothing else; `chrono` would be a
/// dependency for every consumer of this widget for the sake of six fields.
/// Proleptic Gregorian, UTC, via the days-from-civil algorithm.
fn civil(ms: i64) -> (i32, u32, u32, u32, u32) {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    let (h, m) = ((rem / 3_600_000) as u32, ((rem / 60_000) % 60) as u32);

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if mth <= 2 { y + 1 } else { y } as i32;
    (y, mth, d, h, m)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Label for a time tick, at a resolution matched to `step`.
pub fn format_time(ms: i64, step: i64) -> String {
    let (y, mo, d, h, mi) = civil(ms);
    let mon = MONTHS[(mo.clamp(1, 12) - 1) as usize];
    if step >= 300 * 86_400_000 {
        format!("{y}")
    } else if step >= 25 * 86_400_000 {
        // A month boundary carries the year, so a multi-year axis is readable
        // without every label repeating it.
        if mo == 1 {
            format!("{y}")
        } else {
            mon.to_string()
        }
    } else if step >= 86_400_000 {
        format!("{d} {mon}")
    } else {
        format!("{h:02}:{mi:02}")
    }
}

/// Full stamp, for the crosshair readout where precision beats brevity.
pub fn format_time_full(ms: i64) -> String {
    let (y, mo, d, h, mi) = civil(ms);
    let mon = MONTHS[(mo.clamp(1, 12) - 1) as usize];
    format!("{d} {mon} {y} {h:02}:{mi:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_round_numbers() {
        assert_eq!(nice_step(0.9), 1.0);
        assert_eq!(nice_step(1.1), 2.0);
        assert_eq!(nice_step(2.1), 2.5);
        assert_eq!(nice_step(4.0), 5.0);
        assert_eq!(nice_step(7.0), 10.0);
        assert_eq!(nice_step(230.0), 250.0);
        assert_eq!(nice_step(0.011), 0.02);
    }

    /// The point of nice steps: labels a person recognises.
    #[test]
    fn linear_ticks_land_on_round_values() {
        let t = value_ticks(Scale::Linear, Range::new(3.0, 97.0), 5);
        let v: Vec<f64> = t.iter().map(|t| t.value).collect();
        assert_eq!(v, vec![20.0, 40.0, 60.0, 80.0]);
        assert!(t.iter().all(|t| !t.label.contains('.')), "{t:?}");
    }

    /// Labels must differ from each other, or the axis reads as broken.
    #[test]
    fn narrow_ranges_keep_enough_decimals() {
        let t = value_ticks(Scale::Linear, Range::new(1.2340, 1.2350), 4);
        assert!(t.len() >= 2);
        let labels: Vec<&str> = t.iter().map(|t| t.label.as_str()).collect();
        let mut uniq = labels.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), labels.len(), "duplicate labels: {labels:?}");
    }

    #[test]
    fn a_flat_range_still_draws() {
        let r = Range::new(5.0, 5.0).padded(0.05);
        assert!(r.span() > 0.0);
        assert!(norm(Scale::Linear, r, 5.0).is_finite());
        // And an empty one does not produce NaN geometry.
        assert!(!Range::empty().padded(0.05).is_empty());
    }

    /// Log has to survive a series that touches zero rather than taking the
    /// pane with it.
    #[test]
    fn log_of_zero_is_bounded() {
        let n = norm(Scale::Log, Range::new(1.0, 100.0), 0.0);
        assert!(n.is_finite(), "log norm of 0 must not be -inf");
        assert!(n < 0.0, "and must still sort below the range");
    }

    #[test]
    fn log_ticks_are_decades() {
        let t = value_ticks(Scale::Log, Range::new(1.0, 1000.0), 4);
        let v: Vec<f64> = t.iter().filter(|t| t.major).map(|t| t.value).collect();
        assert_eq!(v, vec![1.0, 10.0, 100.0, 1000.0]);
    }

    /// Time ticks land on wall-clock boundaries, not on round millisecond
    /// counts — a division at 03:47:12 is noise.
    #[test]
    fn time_ticks_land_on_boundaries() {
        let day = 86_400_000i64;
        let t0 = 1_700_000_000_000;
        let ticks = time_ticks(t0, t0 + 7 * day, 7);
        assert!(!ticks.is_empty());
        for t in &ticks {
            let ms = t.value as i64;
            assert_eq!(ms % 86_400_000, 0, "not on a day boundary: {}", t.label);
        }
    }

    #[test]
    fn time_labels_match_the_zoom() {
        let day = 86_400_000i64;
        let t0 = 1_700_000_000_000;
        // Minutes: clock time.
        let m = time_ticks(t0, t0 + 3_600_000, 6);
        assert!(m.iter().all(|t| t.label.contains(':')), "{m:?}");
        // Years: just the year.
        let y = time_ticks(t0, t0 + 3650 * day, 6);
        assert!(
            y.iter()
                .all(|t| t.label.len() == 4 && !t.label.contains(':')),
            "{y:?}"
        );
    }

    /// The civil-date conversion has to be right, or every label is wrong in a
    /// way that looks plausible.
    #[test]
    fn epoch_converts_to_civil_time() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0));
        // 2024-02-29T12:34 — a leap day, which is where a hand-rolled
        // conversion usually breaks.
        let ms = 1_709_209_000_000i64 - 1_709_209_000_000 % 60_000;
        let (y, mo, d, _, _) = civil(ms);
        assert_eq!((y, mo, d), (2024, 2, 29));
    }

    /// An axis crossing zero must label it `0`, not `-0`.
    #[test]
    fn zero_has_no_sign() {
        assert_eq!(format_value(-0.0, 1.0), "0");
        assert_eq!(format_value(0.0, 0.01), "0.00");
        let t = value_ticks(Scale::Linear, Range::new(-50.0, 120.0), 5);
        assert!(
            t.iter()
                .all(|t| !t.label.starts_with("-0") || t.value < -0.5),
            "{:?}",
            t.iter().map(|t| &t.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn big_numbers_get_a_suffix() {
        assert_eq!(format_value(1_090_666_062.0, 1e8), "1.09B");
        assert_eq!(format_value(30_320_000.0, 1e6), "30.32M");
        assert_eq!(format_value(327.13, 0.01), "327.13");
    }
}
