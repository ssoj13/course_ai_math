use crate::FrameSnapshot;
use sim_model::*;
use std::collections::BTreeMap;

/// Presentation values have no path back into Session's mathematical truth.
#[derive(Clone, Debug, PartialEq)]
pub struct PresentationFrame {
    target_revision: u64,
    shown: BTreeMap<ValueId, Value>,
}
impl PresentationFrame {
    pub fn target_revision(&self) -> u64 {
        self.target_revision
    }
    pub fn shown(&self) -> &BTreeMap<ValueId, Value> {
        &self.shown
    }
    pub fn cut(truth: &FrameSnapshot) -> Self {
        Self {
            target_revision: truth.revision(),
            shown: truth.truth().clone(),
        }
    }
    /// Host presentation clocks supply normalized progress; seeking math never supplies wall time.
    pub fn interpolate(from: &Self, target: &FrameSnapshot, progress: Finite) -> Result<Self> {
        let t = progress.get();
        if !(0.0..=1.0).contains(&t) {
            return Err(Diagnostic::new(
                DiagnosticCode::Range,
                "presentation/progress",
                "progress must be in 0..1",
            ));
        }
        let mut shown = BTreeMap::new();
        for (id, value) in target.truth() {
            let next = match from.shown.get(id) {
                Some(previous) if previous.shape() == value.shape() && t < 1. => {
                    let blend = |a: f64, b: f64| a * (1. - t) + b * t;
                    match (previous, value) {
                        (Value::Scalar(a), Value::Scalar(b)) => {
                            Value::scalar(blend(a.get(), b.get()))?
                        }
                        (Value::Vector(a), Value::Vector(b)) => Value::vector(
                            a.iter()
                                .zip(b.iter())
                                .map(|(a, b)| blend(a.get(), b.get()))
                                .collect(),
                        )?,
                        (
                            Value::Matrix { values: a, .. },
                            Value::Matrix {
                                rows,
                                cols,
                                values: b,
                            },
                        ) => Value::matrix(
                            *rows,
                            *cols,
                            a.iter()
                                .zip(b.iter())
                                .map(|(a, b)| blend(a.get(), b.get()))
                                .collect(),
                        )?,
                        // Discrete values and tagged colours have no arithmetic presentation blend.
                        _ => value.clone(),
                    }
                }
                _ => value.clone(),
            };
            shown.insert(id.clone(), next);
        }
        Ok(Self {
            target_revision: target.revision(),
            shown,
        })
    }
}
