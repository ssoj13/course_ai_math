use crate::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    Srgb,
    LinearSrgb,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authority {
    #[default]
    RustFp64Sequential,
    LegacyTs,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DisplayUnit {
    Dimensionless,
    Metres,
    Centimetres,
    Millimetres,
    Seconds,
    Milliseconds,
    Radians,
    Degrees,
    Derived { unit: Unit, scale: Finite },
}
impl DisplayUnit {
    pub fn unit(self) -> Unit {
        match self {
            Self::Derived { unit, .. } => unit,
            Self::Dimensionless => Unit::DIMENSIONLESS,
            Self::Metres | Self::Centimetres | Self::Millimetres => Unit::LENGTH,
            Self::Seconds | Self::Milliseconds => Unit {
                length: 0,
                time: 1,
                angle: 0,
            },
            Self::Radians | Self::Degrees => Unit::ANGLE,
        }
    }
    pub fn factor(self) -> f64 {
        match self {
            Self::Derived { scale, .. } => scale.get(),
            Self::Centimetres => 0.01,
            Self::Millimetres | Self::Milliseconds => 0.001,
            Self::Degrees => std::f64::consts::PI / 180.0,
            _ => 1.0,
        }
    }
    pub fn to_canonical(self, value: Finite, unit: Unit) -> Result<Finite> {
        self.check(unit)?;
        Finite::new(value.get() * self.factor())
    }
    pub fn from_canonical(self, value: Finite, unit: Unit) -> Result<Finite> {
        self.check(unit)?;
        Finite::new(value.get() / self.factor())
    }
    pub fn check(self, unit: Unit) -> Result<()> {
        if self.factor() <= 0.0 {
            return Err(Diagnostic::new(
                DiagnosticCode::Range,
                "display_unit/scale",
                "display scale must be positive",
            ));
        }
        if self.unit() == unit {
            Ok(())
        } else {
            Err(Diagnostic::new(
                DiagnosticCode::Unit,
                "display_unit",
                "display unit differs from canonical unit",
            ))
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualifiedAddress {
    pub document: DocumentId,
    pub instance: Option<InstanceId>,
    pub object: ObjectId,
    pub property: PropertyId,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyAddress {
    pub object: ObjectId,
    pub property: PropertyId,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegerRange {
    pub min: i64,
    pub max: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterMetadata {
    pub step: Option<Finite>,
    pub integer_step: Option<u64>,
    pub precision: Option<u8>,
    pub display_unit: Option<DisplayUnit>,
    pub integer_range: Option<IntegerRange>,
    pub enum_choices: Vec<String>,
    pub optional_file: bool,
    pub locked: bool,
    pub learner_exposed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearnerPermissions {
    pub overrides: bool,
    pub return_to_guide: bool,
    pub seek: bool,
    pub playback: bool,
}
impl Default for LearnerPermissions {
    fn default() -> Self {
        Self {
            overrides: true,
            return_to_guide: true,
            seek: true,
            playback: true,
        }
    }
}

pub fn deserialize_unique_map<'de, D, K, V>(
    deserializer: D,
) -> std::result::Result<std::collections::BTreeMap<K, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    K: serde::Deserialize<'de> + Ord,
    V: serde::Deserialize<'de>,
{
    struct Unique<K, V>(std::marker::PhantomData<(K, V)>);
    impl<'de, K: serde::Deserialize<'de> + Ord, V: serde::Deserialize<'de>> serde::de::Visitor<'de>
        for Unique<K, V>
    {
        type Value = std::collections::BTreeMap<K, V>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("map with unique typed keys")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut access: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut map = std::collections::BTreeMap::new();
            while let Some((k, v)) = access.next_entry()? {
                if map.insert(k, v).is_some() {
                    return Err(serde::de::Error::custom("duplicate map target"));
                }
            }
            Ok(map)
        }
    }
    deserializer.deserialize_map(Unique(std::marker::PhantomData))
}
