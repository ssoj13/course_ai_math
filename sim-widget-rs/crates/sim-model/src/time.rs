use crate::{Diagnostic, DiagnosticCode, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr};

pub const TICKS_PER_SECOND: i64 = 254_016_000_000;

/// Exact signed logical time; JSON always uses a canonical decimal string.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeTicks(i64);
impl TimeTicks {
    pub const ZERO: Self = Self(0);
    pub const fn new(ticks: i64) -> Self {
        Self(ticks)
    }
    pub const fn ticks(self) -> i64 {
        self.0
    }
    pub fn checked_add(self, rhs: Self) -> Result<Self> {
        self.0.checked_add(rhs.0).map(Self).ok_or_else(overflow)
    }
    pub fn checked_sub(self, rhs: Self) -> Result<Self> {
        self.0.checked_sub(rhs.0).map(Self).ok_or_else(overflow)
    }
    /// Exact conversion only. An unrepresentable rational fails rather than rounding.
    pub fn from_rational(count: i64, numerator: i64, denominator: i64) -> Result<Self> {
        if numerator <= 0 || denominator <= 0 {
            return Err(Diagnostic::new(
                DiagnosticCode::Time,
                "time",
                "timebase numerator and denominator must be positive",
            ));
        }
        let n = i128::from(count)
            .checked_mul(i128::from(numerator))
            .and_then(|x| x.checked_mul(i128::from(TICKS_PER_SECOND)))
            .ok_or_else(overflow)?;
        let d = i128::from(denominator);
        if n % d != 0 {
            return Err(Diagnostic::new(
                DiagnosticCode::Time,
                "time",
                "rational time is not an exact tick",
            ));
        }
        i64::try_from(n / d).map(Self).map_err(|_| overflow())
    }
    /// Explicit floor quantization into a rational timebase.
    pub fn to_rational_floor(self, numerator: i64, denominator: i64) -> Result<i64> {
        if numerator <= 0 || denominator <= 0 {
            return Err(Diagnostic::new(
                DiagnosticCode::Time,
                "time",
                "timebase must be positive",
            ));
        }
        let n = i128::from(self.0)
            .checked_mul(i128::from(denominator))
            .ok_or_else(overflow)?;
        let d = i128::from(numerator)
            .checked_mul(i128::from(TICKS_PER_SECOND))
            .ok_or_else(overflow)?;
        i64::try_from(n.div_euclid(d)).map_err(|_| overflow())
    }
}
fn overflow() -> Diagnostic {
    Diagnostic::new(DiagnosticCode::Overflow, "time", "time arithmetic overflow")
}
impl fmt::Display for TimeTicks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl FromStr for TimeTicks {
    type Err = Diagnostic;
    fn from_str(s: &str) -> Result<Self> {
        let t = s.parse::<i64>().map_err(|_| {
            Diagnostic::new(
                DiagnosticCode::Time,
                "time",
                "expected an i64 decimal string",
            )
        })?;
        if s != t.to_string() {
            return Err(Diagnostic::new(
                DiagnosticCode::Time,
                "time",
                "expected canonical decimal string",
            ));
        }
        Ok(Self(t))
    }
}
impl Serialize for TimeTicks {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for TimeTicks {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
