use effectcraft_keyframe::{Keyframe, Value as DonorValue};
use effectcraft_time::Tick;
use sim_model::*;

/// The only donor boundary. Rebase each segment to avoid absolute-time f64 cancellation.
#[derive(Clone, Debug)]
pub struct ScalarTrackAdapter {
    track: ScalarTrack,
}
impl ScalarTrackAdapter {
    pub(crate) fn cache_bytes(&self) -> Result<usize> {
        self.track
            .keys
            .capacity()
            .checked_mul(std::mem::size_of::<ScalarKey>())
            .and_then(|n| n.checked_add(self.track.parameter.as_str().len()))
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::Limit,
                    "tracks/storage",
                    "adapter storage overflow",
                )
            })
    }
    pub fn new(track: ScalarTrack) -> Result<Self> {
        if track.keys.is_empty() {
            return Err(Diagnostic::new(
                DiagnosticCode::Time,
                "tracks/keys",
                "empty track",
            ));
        }
        for pair in track.keys.windows(2) {
            let span = pair[1].time.checked_sub(pair[0].time)?.ticks();
            if span <= 0 || span > 9_007_199_254_740_992 || span > Tick::MAX.0 {
                return Err(Diagnostic::new(
                    DiagnosticCode::Time,
                    format!("tracks/{}", track.parameter.as_str()),
                    "segment duration exceeds exact donor adapter range",
                ));
            }
            Finite::new(pair[1].value.get() - pair[0].value.get())?;
        }
        Ok(Self { track })
    }
    pub fn parameter(&self) -> &ParameterId {
        &self.track.parameter
    }
    pub fn sample(&self, time: TimeTicks) -> Result<Value> {
        let keys = &self.track.keys;
        let first = &keys[0];
        let last = &keys[keys.len() - 1];
        if time <= first.time {
            return Ok(Value::Scalar(first.value));
        }
        if time >= last.time {
            return Ok(Value::Scalar(last.value));
        }
        let i = keys.partition_point(|k| k.time <= time) - 1;
        let a = &keys[i];
        let b = &keys[i + 1];
        if time == a.time || a.interpolation == Interpolation::Hold || a.value == b.value {
            return Ok(Value::Scalar(a.value));
        }
        let duration = b.time.checked_sub(a.time)?.ticks();
        let offset = time.checked_sub(a.time)?.ticks();
        let mut left = Keyframe::new(Tick(0), DonorValue::Scalar(a.value.get()));
        let mut right = Keyframe::new(Tick(duration), DonorValue::Scalar(b.value.get()));
        if a.interpolation == Interpolation::EasyEase {
            left = left.eased();
            right = right.eased();
        }
        let result = effectcraft_keyframe::evaluate(&[left, right], Tick(offset), false)
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::Time,
                    "tracks",
                    "donor returned no scalar sample",
                )
            })?;
        match result {
            DonorValue::Scalar(x) => Value::scalar(x),
            _ => Err(Diagnostic::new(
                DiagnosticCode::Type,
                "tracks",
                "donor returned unexpected value type",
            )),
        }
    }
}
