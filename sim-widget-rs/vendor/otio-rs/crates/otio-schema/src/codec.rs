//! Value-type codec: OTIO `RationalTime`/`TimeRange`/`TimeTransform`/`Color`
//! schemas <-> [`otio_time`] types and JSON.
//!
//! These schemas are serialized inline (as tagged JSON objects) rather than as
//! tree nodes, so otio-core stores them verbatim as [`serde_json::Value`]. The
//! decoders here turn that JSON into strong types on demand; the encoders produce
//! the canonical JSON form (integral numbers emitted as integers, matching OTIO).

use otio_time::{RationalTime, TimeRange, TimeTransform};
use serde_json::{json, Value};

/// An RGBA color (the OTIO `Color.1` schema). Kept here as otio-time has no color.
///
/// OTIO serializes a 5th `name` field (used for UI color labels like marker
/// colors) and its deserializer **requires** it, so the codec carries it too.
/// Not `Copy` because of the owned `name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Color {
    /// Red channel.
    pub r: f64,
    /// Green channel.
    pub g: f64,
    /// Blue channel.
    pub b: f64,
    /// Alpha channel.
    pub a: f64,
    /// Optional human-readable color name (e.g. `"RED"`), empty when unnamed.
    pub name: String,
}

/// Emit an `f64` as a JSON integer when it is integral and in `i64` range,
/// otherwise as a float — matching OTIO's int/float distinction (`"rate": 24`).
fn num(v: f64) -> Value {
    if v.is_finite() && v.fract() == 0.0 && v >= i64::MIN as f64 && v <= i64::MAX as f64 {
        json!(v as i64)
    } else {
        json!(v)
    }
}

/// Decode a `RationalTime.1` object. Returns `None` if the shape is wrong.
pub fn rational_time(v: &Value) -> Option<RationalTime> {
    let o = v.as_object()?;
    let rate = o.get("rate")?.as_f64()?;
    let value = o.get("value")?.as_f64()?;
    Some(RationalTime::new(value, rate))
}

/// Encode a [`RationalTime`] to its `RationalTime.1` JSON form.
pub fn rational_time_value(rt: RationalTime) -> Value {
    json!({
        "OTIO_SCHEMA": "RationalTime.1",
        "rate": num(rt.rate()),
        "value": num(rt.value()),
    })
}

/// Decode a `TimeRange.1` object. Returns `None` if the shape is wrong.
pub fn time_range(v: &Value) -> Option<TimeRange> {
    let o = v.as_object()?;
    let start = rational_time(o.get("start_time")?)?;
    let duration = rational_time(o.get("duration")?)?;
    Some(TimeRange::new(start, duration))
}

/// Encode a [`TimeRange`] to its `TimeRange.1` JSON form.
pub fn time_range_value(tr: TimeRange) -> Value {
    json!({
        "OTIO_SCHEMA": "TimeRange.1",
        "duration": rational_time_value(tr.duration()),
        "start_time": rational_time_value(tr.start_time()),
    })
}

/// Decode a `TimeTransform.1` object. Returns `None` if the shape is wrong.
pub fn time_transform(v: &Value) -> Option<TimeTransform> {
    let o = v.as_object()?;
    let offset = rational_time(o.get("offset")?)?;
    let scale = o.get("scale")?.as_f64()?;
    let rate = o.get("rate")?.as_f64()?;
    Some(TimeTransform::new(offset, scale, rate))
}

/// Decode a `Color.1` object. Returns `None` if the shape is wrong.
pub fn color(v: &Value) -> Option<Color> {
    let o = v.as_object()?;
    Some(Color {
        r: o.get("r")?.as_f64()?,
        g: o.get("g")?.as_f64()?,
        b: o.get("b")?.as_f64()?,
        a: o.get("a")?.as_f64()?,
        // Default to empty when absent; the value is preserved verbatim by
        // otio-core regardless, this is just the typed view.
        name: o.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
    })
}

/// Encode a [`Color`] to its `Color.1` JSON form.
///
/// The `name` field is **mandatory**: OTIO's deserializer fails the whole Color
/// if `name` is absent, so it is always emitted (empty string when unnamed).
pub fn color_value(c: &Color) -> Value {
    json!({
        "OTIO_SCHEMA": "Color.1",
        "r": num(c.r),
        "g": num(c.g),
        "b": num(c.b),
        "a": num(c.a),
        "name": c.name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_time_roundtrip() {
        let v = json!({"OTIO_SCHEMA": "RationalTime.1", "rate": 24, "value": 12});
        let rt = rational_time(&v).unwrap();
        assert_eq!(rt, RationalTime::new(12.0, 24.0));
        // Re-encode: integral numbers stay integers, matching OTIO.
        assert_eq!(rational_time_value(rt), v);
    }

    #[test]
    fn time_range_roundtrip() {
        let v = json!({
            "OTIO_SCHEMA": "TimeRange.1",
            "duration": {"OTIO_SCHEMA": "RationalTime.1", "rate": 24, "value": 3},
            "start_time": {"OTIO_SCHEMA": "RationalTime.1", "rate": 24, "value": 3},
        });
        let tr = time_range(&v).unwrap();
        assert_eq!(tr.duration(), RationalTime::new(3.0, 24.0));
        assert_eq!(time_range_value(tr), v);
    }

    #[test]
    fn color_includes_name() {
        let v = json!({"OTIO_SCHEMA": "Color.1", "r": 1, "g": 0, "b": 0, "a": 1, "name": "RED"});
        let c = color(&v).unwrap();
        assert_eq!(c.name, "RED");
        // Re-encode keeps the mandatory `name` and integral channels as integers.
        assert_eq!(color_value(&c), v);
    }
}
