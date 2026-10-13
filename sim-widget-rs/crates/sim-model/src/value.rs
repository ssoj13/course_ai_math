use crate::{ColorSpace, Diagnostic, DiagnosticCode, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::Arc;

macro_rules! ids {
    ($($name:ident),+) => {$(
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            pub fn new(s: impl Into<String>) -> Result<Self> {
                let s = s.into();
                if s.is_empty() || s.len()>128 || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-.:/".contains(&b)) {
                    return Err(Diagnostic::new(DiagnosticCode::InvalidId, "id", "ID must be 1..128 ASCII identifier characters"));
                }
                Ok(Self(s.into_boxed_str().into_string()))
            }
            pub fn as_str(&self) -> &str { &self.0 }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
                Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }
    )+};
}
ids!(
    DocumentId,
    ObjectId,
    ParameterId,
    ValueId,
    OperationId,
    OperatorId,
    AssetId,
    PropertyId,
    SessionId,
    ViewId,
    InstanceId,
    GenerationId
);

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(transparent)]
pub struct Finite(f64);
impl PartialEq for Finite {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}
impl PartialOrd for Finite {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.0.total_cmp(&other.0))
    }
}
impl Finite {
    pub fn new(x: f64) -> Result<Self> {
        if !x.is_finite() {
            return Err(Diagnostic::new(
                DiagnosticCode::NonFinite,
                "value",
                "numeric value must be finite",
            ));
        }
        Ok(Self(x))
    }
    pub const fn get(self) -> f64 {
        self.0
    }
}
impl<'de> Deserialize<'de> for Finite {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::new(f64::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Value {
    Scalar(Finite),
    Integer(i64),
    Bool(bool),
    Enum(String),
    Color {
        space: ColorSpace,
        channels: [Finite; 4],
    },
    File(Option<AssetId>),
    Vector(Arc<Vec<Finite>>),
    Matrix {
        rows: usize,
        cols: usize,
        values: Arc<Vec<Finite>>,
    },
}
impl Value {
    pub fn scalar(x: f64) -> Result<Self> {
        Ok(Self::Scalar(Finite::new(x)?))
    }
    pub fn vector(xs: Vec<f64>) -> Result<Self> {
        let v = Self::Vector(Arc::new(
            xs.into_iter().map(Finite::new).collect::<Result<_>>()?,
        ));
        v.validate()?;
        Ok(v)
    }
    pub fn matrix(rows: usize, cols: usize, xs: Vec<f64>) -> Result<Self> {
        let v = Self::Matrix {
            rows,
            cols,
            values: Arc::new(xs.into_iter().map(Finite::new).collect::<Result<_>>()?),
        };
        v.validate()?;
        Ok(v)
    }
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Enum(key)
                if key.is_empty()
                    || key.len() > 128
                    || !key
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b)) =>
            {
                Err(Diagnostic::new(
                    DiagnosticCode::Type,
                    "value/enum",
                    "enum key must be stable ASCII",
                ))
            }
            Self::Color { channels, .. }
                if channels.iter().any(|x| !(0.0..=1.0).contains(&x.get())) =>
            {
                Err(Diagnostic::new(
                    DiagnosticCode::Range,
                    "value/color",
                    "RGBA channels must be in 0..1",
                ))
            }
            Self::Vector(xs) if xs.is_empty() => Err(Diagnostic::new(
                DiagnosticCode::Type,
                "value",
                "vector must not be empty",
            )),
            Self::Matrix { rows, cols, values }
                if *rows == 0 || *cols == 0 || rows.checked_mul(*cols) != Some(values.len()) =>
            {
                Err(Diagnostic::new(
                    DiagnosticCode::Type,
                    "value",
                    "matrix dimensions must match storage",
                ))
            }
            _ => Ok(()),
        }
    }
    pub fn as_scalar(&self) -> Result<f64> {
        if let Self::Scalar(x) = self {
            Ok(x.get())
        } else {
            Err(Diagnostic::new(
                DiagnosticCode::Type,
                "value",
                "expected scalar",
            ))
        }
    }
    /// Copies only logical storage, preserving every finite bit and all shape metadata.
    pub fn compact_storage(&self) -> Self {
        match self {
            Self::Vector(xs) if xs.capacity() != xs.len() => {
                Self::Vector(Arc::new(xs.as_slice().to_vec()))
            }
            Self::Matrix { rows, cols, values } if values.capacity() != values.len() => {
                Self::Matrix {
                    rows: *rows,
                    cols: *cols,
                    values: Arc::new(values.as_slice().to_vec()),
                }
            }
            _ => self.clone(),
        }
    }
    /// Retained heap storage, including spare capacity supplied by typed callers.
    pub fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::Vector(xs) | Self::Matrix { values: xs, .. } => xs
                .capacity()
                .checked_mul(std::mem::size_of::<Finite>())
                .ok_or_else(|| {
                    Diagnostic::new(
                        DiagnosticCode::Limit,
                        "value/storage",
                        "capacity byte overflow",
                    )
                }),
            Self::Enum(key) => Ok(key.capacity()),
            Self::File(Some(id)) => Ok(id.as_str().len()),
            _ => Ok(0),
        }
    }
    pub fn element_count(&self) -> usize {
        match self {
            Self::Color { .. } => 4,
            Self::Vector(x) => x.len(),
            Self::Matrix { values, .. } => values.len(),
            _ => 1,
        }
    }
    pub fn shape(&self) -> Shape {
        match self {
            Self::Scalar(_) => Shape::Scalar,
            Self::Integer(_) => Shape::Integer,
            Self::Bool(_) => Shape::Bool,
            Self::Enum(_) => Shape::Enum,
            Self::Color { space, .. } => Shape::Color { space: *space },
            Self::File(_) => Shape::File,
            Self::Vector(x) => Shape::Vector { len: x.len() },
            Self::Matrix { rows, cols, .. } => Shape::Matrix {
                rows: *rows,
                cols: *cols,
            },
        }
    }
    pub fn components(&self) -> Vec<f64> {
        match self {
            Self::Scalar(x) => vec![x.get()],
            Self::Vector(xs) => xs.iter().map(|x| x.get()).collect(),
            Self::Matrix { values, .. } => values.iter().map(|x| x.get()).collect(),
            _ => vec![],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    Scalar,
    Integer,
    Bool,
    Enum,
    Color { space: ColorSpace },
    File,
    Vector { len: usize },
    Matrix { rows: usize, cols: usize },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {
    pub length: i16,
    pub time: i16,
    pub angle: i16,
}
impl Unit {
    pub const DIMENSIONLESS: Self = Self {
        length: 0,
        time: 0,
        angle: 0,
    };
    pub const LENGTH: Self = Self {
        length: 1,
        time: 0,
        angle: 0,
    };
    /// Angle is in radians throughout the mathematical document.
    pub const ANGLE: Self = Self {
        length: 0,
        time: 0,
        angle: 1,
    };
    pub fn multiply(self, rhs: Self) -> Result<Self> {
        self.combine(rhs, false)
    }
    pub fn divide(self, rhs: Self) -> Result<Self> {
        self.combine(rhs, true)
    }
    fn combine(self, rhs: Self, subtract: bool) -> Result<Self> {
        let f = |a: i16, b: i16| {
            if subtract {
                a.checked_sub(b)
            } else {
                a.checked_add(b)
            }
        };
        let bad = || Diagnostic::new(DiagnosticCode::Unit, "unit", "unit exponent overflow");
        Ok(Self {
            length: f(self.length, rhs.length).ok_or_else(bad)?,
            time: f(self.time, rhs.time).ok_or_else(bad)?,
            angle: f(self.angle, rhs.angle).ok_or_else(bad)?,
        })
    }
    pub fn sqrt(self) -> Result<Self> {
        if [self.length, self.time, self.angle]
            .iter()
            .any(|x| x % 2 != 0)
        {
            return Err(Diagnostic::new(
                DiagnosticCode::Unit,
                "unit",
                "square root requires even unit exponents",
            ));
        }
        Ok(Self {
            length: self.length / 2,
            time: self.time / 2,
            angle: self.angle / 2,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Port {
    pub shape: Shape,
    pub unit: Unit,
}
impl Port {
    pub fn validate_shape(&self, max_elements: usize) -> Result<()> {
        let n = match self.shape {
            Shape::Vector { len } => len,
            Shape::Matrix { rows, cols } => rows.checked_mul(cols).ok_or_else(|| {
                Diagnostic::new(DiagnosticCode::Limit, "port", "shape size overflow")
            })?,
            Shape::Color { .. } => 4,
            _ => 1,
        };
        if n == 0 || n > max_elements {
            return Err(Diagnostic::new(
                DiagnosticCode::Limit,
                "port",
                "port shape exceeds limit or is empty",
            ));
        }
        if matches!(
            self.shape,
            Shape::Bool | Shape::Enum | Shape::Color { .. } | Shape::File
        ) && self.unit != Unit::DIMENSIONLESS
        {
            return Err(Diagnostic::new(
                DiagnosticCode::Unit,
                "port",
                "discrete/color port must be dimensionless",
            ));
        }
        Ok(())
    }
    pub fn validate(&self, value: &Value) -> Result<()> {
        value.validate()?;
        self.validate_shape(usize::MAX)?;
        if self.shape != value.shape() {
            return Err(Diagnostic::new(
                DiagnosticCode::Type,
                "port",
                "value shape differs from declared port",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumericRange {
    pub min: Finite,
    pub max: Finite,
}
impl NumericRange {
    pub fn validate(&self, value: &Value) -> Result<()> {
        if self.min > self.max {
            return Err(Diagnostic::new(
                DiagnosticCode::Range,
                "range",
                "range bounds are reversed",
            ));
        }
        let out = match value {
            Value::Integer(x) => {
                i128::from(*x) < self.min.get().ceil() as i128
                    || i128::from(*x) > self.max.get().floor() as i128
            }
            Value::Bool(_) | Value::Enum(_) | Value::File(_) | Value::Color { .. } => {
                return Err(Diagnostic::new(
                    DiagnosticCode::Type,
                    "range",
                    "bool cannot have a numeric range",
                ));
            }
            _ => value
                .components()
                .iter()
                .any(|x| *x < self.min.get() || *x > self.max.get()),
        };
        if out {
            Err(Diagnostic::new(
                DiagnosticCode::Range,
                "range",
                "effective value is outside range",
            ))
        } else {
            Ok(())
        }
    }
}
