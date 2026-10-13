use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct V1Document {
    schema_version: u32,
    id: DocumentId,
    revision: u64,
    required_capabilities: Vec<String>,
    parameters: Vec<V1Parameter>,
    constants: Vec<Constant>,
    operations: Vec<Operation>,
    tracks: Vec<ScalarTrack>,
    constraints: Vec<Constraint>,
    objects: Vec<V1Object>,
    assets: Vec<Asset>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct V1Parameter {
    id: ParameterId,
    value_id: ValueId,
    default: Value,
    port: Port,
    range: Option<NumericRange>,
    readonly: bool,
    source_refs: Vec<SourceRef>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct V1Object {
    id: ObjectId,
    label: String,
    #[serde(deserialize_with = "deserialize_unique_map")]
    bindings: BTreeMap<PropertyId, ValueId>,
    assets: Vec<AssetId>,
}
#[derive(Deserialize)]
struct Version {
    schema_version: u32,
}
pub(crate) fn migrate_document(bytes: &[u8]) -> Result<Document> {
    let version: Version = serde_json::from_slice(bytes)
        .map_err(|e| Diagnostic::new(DiagnosticCode::Schema, "schema_version", e.to_string()))?;
    if version.schema_version == SCHEMA_VERSION {
        return Document::parse_current(bytes);
    }
    if version.schema_version != 1 {
        return Err(Diagnostic::new(
            DiagnosticCode::Schema,
            "schema_version",
            "unsupported document version",
        ));
    }
    let old: V1Document = serde_json::from_slice(bytes)
        .map_err(|e| Diagnostic::new(DiagnosticCode::Schema, "migration/v1", e.to_string()))?;
    let ports: BTreeMap<_, _> = old
        .parameters
        .iter()
        .map(|p| (p.value_id.clone(), p.port.clone()))
        .chain(old.constants.iter().map(|c| (c.id.clone(), c.port.clone())))
        .chain(
            old.operations
                .iter()
                .map(|o| (o.output.clone(), o.output_port.clone())),
        )
        .collect();
    let objects = old
        .objects
        .into_iter()
        .map(|o| {
            let properties = o
                .bindings
                .iter()
                .map(|(property, source)| {
                    ports
                        .get(source)
                        .cloned()
                        .map(|port| (property.clone(), port))
                        .ok_or_else(|| {
                            Diagnostic::new(
                                DiagnosticCode::Reference,
                                format!("objects/{}/bindings/{}", o.id.as_str(), property.as_str()),
                                "v1 source missing",
                            )
                        })
                })
                .collect::<Result<_>>()?;
            Ok(SceneObject {
                id: o.id,
                label: o.label,
                bindings: o.bindings,
                properties,
                assets: o.assets,
            })
        })
        .collect::<Result<_>>()?;
    let mut parameters = old
        .parameters
        .into_iter()
        .map(|p| Parameter {
            id: p.id,
            value_id: p.value_id,
            default: p.default,
            port: p.port,
            range: p.range,
            readonly: p.readonly,
            source_refs: p.source_refs,
            metadata: ParameterMetadata::default(),
        })
        .collect::<Vec<_>>();
    for p in &mut parameters {
        p.metadata.learner_exposed = true;
        if p.port.shape == Shape::Integer
            && let Some(r) = p.range.take()
        {
            let min = r.min.get();
            let max = r.max.get();
            if min.abs() > 9007199254740991.0
                || max.abs() > 9007199254740991.0
                || min.fract() != 0.0
                || max.fract() != 0.0
            {
                return Err(Diagnostic::new(
                    DiagnosticCode::Schema,
                    "migration/integer_range",
                    "v1 integer range cannot be migrated exactly",
                ));
            }
            p.metadata.integer_range = Some(IntegerRange {
                min: min as i64,
                max: max as i64,
            });
        }
    }
    let _ = old.schema_version;
    Ok(Document {
        schema_version: SCHEMA_VERSION,
        id: old.id,
        revision: old.revision,
        required_capabilities: old.required_capabilities,
        authority: Authority::RustFp64Sequential,
        learner_permissions: LearnerPermissions::default(),
        parameters,
        constants: old.constants,
        operations: old.operations,
        tracks: old.tracks,
        constraints: old.constraints,
        objects,
        assets: old.assets,
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
impl Comparison {
    pub fn test(self, a: f64, b: f64) -> bool {
        match self {
            Self::Less => a < b,
            Self::LessEqual => a <= b,
            Self::Greater => a > b,
            Self::GreaterEqual => a >= b,
        }
    }
}
impl Constraint {
    pub fn parameters(&self) -> Vec<&ParameterId> {
        match self {
            Self::NonZero { parameter } => vec![parameter],
            Self::Equal { left, right } | Self::ScalarCompare { left, right, .. } => {
                vec![left, right]
            }
            Self::ScalarBounds {
                parameter,
                min,
                max,
            }
            | Self::NormBounds {
                parameter,
                min,
                max,
            } => std::iter::once(parameter)
                .chain(min.iter())
                .chain(max.iter())
                .collect(),
        }
    }
}
pub(crate) struct ByteCounter {
    pub count: usize,
    pub limit: usize,
}
impl std::io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let next = self
            .count
            .checked_add(buf.len())
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| std::io::Error::other("byte budget exceeded"))?;
        self.count = next;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn bounded_size<T: Serialize>(value: &T, limit: usize) -> Result<usize> {
    let mut c = ByteCounter { count: 0, limit };
    serde_json::to_writer(&mut c, value).map_err(|_| {
        Diagnostic::new(
            DiagnosticCode::Limit,
            "storage",
            "serialized byte budget exceeded",
        )
    })?;
    Ok(c.count)
}
