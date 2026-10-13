use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA_VERSION: u32 = 2;
pub const CAPABILITIES: &[&str] = &["math.scalar.v1", "math.vector.v1", "timeline.scalar.v1"];

#[derive(Clone, Copy, Debug)]
pub struct DecodeLimits {
    pub max_bytes: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_keys: usize,
    pub max_elements: usize,
    pub max_depth: usize,
}
impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1_048_576,
            max_nodes: 4096,
            max_edges: 16_384,
            max_keys: 16_384,
            max_elements: 1_048_576,
            max_depth: 64,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub schema_version: u32,
    pub id: DocumentId,
    pub revision: u64,
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub authority: Authority,
    #[serde(default)]
    pub learner_permissions: LearnerPermissions,
    pub parameters: Vec<Parameter>,
    pub constants: Vec<Constant>,
    pub operations: Vec<Operation>,
    pub tracks: Vec<ScalarTrack>,
    pub constraints: Vec<Constraint>,
    pub objects: Vec<SceneObject>,
    pub assets: Vec<Asset>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub id: ParameterId,
    pub value_id: ValueId,
    pub default: Value,
    pub port: Port,
    pub range: Option<NumericRange>,
    pub readonly: bool,
    #[serde(default)]
    pub metadata: ParameterMetadata,
    pub source_refs: Vec<SourceRef>,
}
impl Parameter {
    pub fn validate_metadata(&self) -> Result<()> {
        let m = &self.metadata;
        let numeric = matches!(
            self.port.shape,
            Shape::Scalar | Shape::Integer | Shape::Vector { .. } | Shape::Matrix { .. }
        );
        if !numeric
            && (self.range.is_some()
                || m.step.is_some()
                || m.integer_step.is_some()
                || m.integer_range.is_some()
                || m.precision.is_some()
                || m.display_unit.is_some())
        {
            return Err(err(
                DiagnosticCode::Type,
                "metadata",
                "discrete/color metadata cannot be numeric",
            ));
        }
        if let Some(s) = m.step
            && (s.get() <= 0.0 || self.port.shape == Shape::Integer)
        {
            return Err(err(
                DiagnosticCode::Range,
                "metadata/step",
                "positive real step requires noninteger numeric type",
            ));
        }
        if m.integer_step
            .is_some_and(|s| s == 0 || s > i64::MAX as u64)
            || (m.integer_step.is_some() && self.port.shape != Shape::Integer)
        {
            return Err(err(
                DiagnosticCode::Range,
                "metadata/integer_step",
                "positive integer step requires integer",
            ));
        }
        if m.precision.is_some_and(|p| p > 15) {
            return Err(err(
                DiagnosticCode::Range,
                "metadata/precision",
                "precision must be 0..15",
            ));
        }
        if let Some(u) = m.display_unit {
            u.check(self.port.unit)?;
        }
        if let Some(r) = &m.integer_range
            && (r.min > r.max || self.port.shape != Shape::Integer || self.range.is_some())
        {
            return Err(err(
                DiagnosticCode::Range,
                "metadata/integer_range",
                "exact integer bounds require integer and no floating range",
            ));
        }
        if self.port.shape == Shape::Integer && self.range.is_some() {
            return Err(err(
                DiagnosticCode::Type,
                "range",
                "v2 integers require exact integer_range",
            ));
        }
        let mut choices = BTreeSet::new();
        for k in &m.enum_choices {
            Value::Enum(k.clone()).validate()?;
            if !choices.insert(k) {
                return Err(err(
                    DiagnosticCode::Duplicate,
                    "metadata/enum_choices",
                    "duplicate enum choice",
                ));
            }
        }
        if (self.port.shape == Shape::Enum && choices.is_empty())
            || (self.port.shape != Shape::Enum && !choices.is_empty())
        {
            return Err(err(
                DiagnosticCode::Type,
                "metadata/enum_choices",
                "enum choices require enum parameter",
            ));
        }
        if m.optional_file && self.port.shape != Shape::File {
            return Err(err(
                DiagnosticCode::Type,
                "metadata/optional_file",
                "optional file requires file type",
            ));
        }
        Ok(())
    }
    pub fn validate_value(&self, value: &Value) -> Result<()> {
        let path = format!("parameters/{}", self.id.as_str());
        self.port.validate(value).map_err(|e| e.at(&path))?;
        self.validate_metadata().map_err(|e| e.at(&path))?;
        match value {
            Value::Enum(key) if !self.metadata.enum_choices.contains(key) => {
                return Err(err(DiagnosticCode::Range, &path, "undeclared enum choice"));
            }
            Value::File(None) if !self.metadata.optional_file => {
                return Err(err(
                    DiagnosticCode::Reference,
                    &path,
                    "file parameter requires an asset",
                ));
            }
            Value::Integer(x) => {
                if let Some(r) = &self.metadata.integer_range
                    && (*x < r.min || *x > r.max)
                {
                    return Err(err(
                        DiagnosticCode::Range,
                        &path,
                        "integer outside exact range",
                    ));
                }
            }
            _ => {}
        }
        if let Some(range) = &self.range {
            range.validate(value).map_err(|e| e.at(&path))?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constant {
    pub id: ValueId,
    pub value: Value,
    pub port: Port,
    pub source_refs: Vec<SourceRef>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub id: OperationId,
    pub operator: OperatorId,
    pub inputs: Vec<ValueId>,
    pub output: ValueId,
    pub output_port: Port,
    pub source_refs: Vec<SourceRef>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SourceRef {
    Parameter(ParameterId),
    Value(ValueId),
    Operation(OperationId),
    Object(ObjectId),
    Asset(AssetId),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneObject {
    pub id: ObjectId,
    pub label: String,
    #[serde(deserialize_with = "deserialize_unique_map")]
    pub bindings: BTreeMap<PropertyId, ValueId>,
    #[serde(default, deserialize_with = "deserialize_unique_map")]
    pub properties: BTreeMap<PropertyId, Port>,
    pub assets: Vec<AssetId>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub id: AssetId,
    pub uri: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Linear,
    Hold,
    EasyEase,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalarKey {
    pub time: TimeTicks,
    pub value: Finite,
    pub interpolation: Interpolation,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScalarTrack {
    pub parameter: ParameterId,
    pub keys: Vec<ScalarKey>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Constraint {
    NonZero {
        parameter: ParameterId,
    },
    Equal {
        left: ParameterId,
        right: ParameterId,
    },
    ScalarCompare {
        left: ParameterId,
        right: ParameterId,
        comparison: Comparison,
    },
    ScalarBounds {
        parameter: ParameterId,
        min: Option<ParameterId>,
        max: Option<ParameterId>,
    },
    NormBounds {
        parameter: ParameterId,
        min: Option<ParameterId>,
        max: Option<ParameterId>,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedDocument {
    document: Document,
    order: Vec<usize>,
    ports: BTreeMap<ValueId, Port>,
}
impl ValidatedDocument {
    pub fn allocated_bytes(&self) -> Result<usize> {
        let ports = self
            .ports
            .len()
            .checked_mul(std::mem::size_of::<(ValueId, Port)>() + 64)
            .ok_or_else(|| {
                err(
                    DiagnosticCode::Limit,
                    "validated/storage",
                    "port byte overflow",
                )
            })?;
        let order = self
            .order
            .capacity()
            .checked_mul(std::mem::size_of::<usize>())
            .ok_or_else(|| {
                err(
                    DiagnosticCode::Limit,
                    "validated/storage",
                    "order byte overflow",
                )
            })?;
        self.document
            .allocated_bytes()?
            .checked_add(ports)
            .and_then(|n| n.checked_add(order))
            .ok_or_else(|| {
                err(
                    DiagnosticCode::Limit,
                    "validated/storage",
                    "compiled storage overflow",
                )
            })
    }
    pub fn encode(&self, limit: usize) -> Result<Vec<u8>> {
        bounded_size(&self.document, limit)?;
        serde_json::to_vec(&self.document)
            .map_err(|e| err(DiagnosticCode::Schema, "encode", &e.to_string()))
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn operation_order(&self) -> &[usize] {
        &self.order
    }
    pub fn ports(&self) -> &BTreeMap<ValueId, Port> {
        &self.ports
    }
    pub fn parameter(&self, id: &ParameterId) -> Result<&Parameter> {
        self.document
            .parameters
            .iter()
            .find(|p| &p.id == id)
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::Reference,
                    format!("parameters/{}", id.as_str()),
                    "unknown parameter",
                )
            })
    }
    pub fn validate_effective(&self, values: &BTreeMap<ParameterId, Value>) -> Result<()> {
        for p in &self.document.parameters {
            let value = values.get(&p.id).ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::Reference,
                    format!("parameters/{}", p.id.as_str()),
                    "missing effective parameter",
                )
            })?;
            p.validate_value(value)?;
            if let Value::File(Some(asset)) = value
                && !self.document.assets.iter().any(|a| &a.id == asset)
            {
                return Err(err(
                    DiagnosticCode::Reference,
                    format!("parameters/{}", p.id.as_str()),
                    "effective file asset missing",
                ));
            }
        }
        if values.len() != self.document.parameters.len() {
            return Err(Diagnostic::new(
                DiagnosticCode::Reference,
                "parameters",
                "unexpected effective input",
            ));
        }
        for (i, c) in self.document.constraints.iter().enumerate() {
            let valid = match c {
                Constraint::NonZero { parameter } => values[parameter].as_scalar()? != 0.,
                Constraint::Equal { left, right } => values[left] == values[right],
                Constraint::ScalarCompare {
                    left,
                    right,
                    comparison,
                } => comparison.test(values[left].as_scalar()?, values[right].as_scalar()?),
                Constraint::ScalarBounds {
                    parameter,
                    min,
                    max,
                }
                | Constraint::NormBounds {
                    parameter,
                    min,
                    max,
                } => {
                    let x = if matches!(c, Constraint::NormBounds { .. }) {
                        let mut n = 0f64;
                        for v in values[parameter].components() {
                            n = n.hypot(v);
                        }
                        Finite::new(n)
                            .map_err(|e| e.at(format!("constraints/{i}")))?
                            .get()
                    } else {
                        values[parameter].as_scalar()?
                    };
                    min.as_ref()
                        .map(|id| values[id].as_scalar().map(|n| x >= n))
                        .transpose()?
                        .unwrap_or(true)
                        && max
                            .as_ref()
                            .map(|id| values[id].as_scalar().map(|n| x <= n))
                            .transpose()?
                            .unwrap_or(true)
                }
            };
            if !valid {
                return Err(Diagnostic::new(
                    DiagnosticCode::Range,
                    format!("constraints/{i}"),
                    "effective inputs violate constraint",
                )
                .affected(c.parameters().into_iter().map(|id| id.as_str().to_owned())));
            }
        }
        Ok(())
    }
}
fn err(code: DiagnosticCode, path: impl Into<String>, message: &str) -> Diagnostic {
    Diagnostic::new(code, path, message)
}
fn bound(n: usize, limit: usize, path: &str) -> Result<()> {
    if n > limit {
        Err(err(
            DiagnosticCode::Limit,
            path,
            "document exceeds resource limit",
        ))
    } else {
        Ok(())
    }
}
fn sum_count(counts: impl IntoIterator<Item = usize>, limit: usize, path: &str) -> Result<usize> {
    let mut n = 0usize;
    for c in counts {
        n = n
            .checked_add(c)
            .ok_or_else(|| err(DiagnosticCode::Limit, path, "count overflow"))?;
        bound(n, limit, path)?;
    }
    Ok(n)
}
impl Document {
    pub fn decode(bytes: &[u8], limits: DecodeLimits) -> Result<ValidatedDocument> {
        validate_json_budget(bytes, limits)?;
        let doc: Self = migrate_document(bytes)?;
        doc.validate(limits)
    }
    pub fn parse_current(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(|e| {
            err(
                DiagnosticCode::Schema,
                format!("json/line/{}/column/{}", e.line(), e.column()),
                &e.to_string(),
            )
        })
    }
    pub fn validate(self, limits: DecodeLimits) -> Result<ValidatedDocument> {
        self.allocated_bytes_bounded(limits.max_bytes)?;
        if self.schema_version != SCHEMA_VERSION {
            return Err(err(
                DiagnosticCode::Schema,
                "schema_version",
                "unsupported schema version",
            ));
        }
        if self.authority != Authority::RustFp64Sequential {
            return Err(err(
                DiagnosticCode::Capability,
                "authority",
                "requested evaluator is not installed",
            ));
        }
        // Count serialized storage without allocating a second document buffer.
        let mut counter = ByteCounter {
            count: 0,
            limit: limits.max_bytes,
        };
        serde_json::to_writer(&mut counter, &self).map_err(|_| {
            err(
                DiagnosticCode::Limit,
                "document",
                "typed document exceeds byte budget",
            )
        })?;
        bound(
            self.required_capabilities.len(),
            limits.max_nodes,
            "required_capabilities",
        )?;
        let mut caps = BTreeSet::new();
        for cap in &self.required_capabilities {
            if !CAPABILITIES.contains(&cap.as_str()) {
                return Err(err(
                    DiagnosticCode::Capability,
                    format!("required_capabilities/{cap}"),
                    "unsupported required capability",
                ));
            }
            if !caps.insert(cap) {
                return Err(err(
                    DiagnosticCode::Duplicate,
                    "required_capabilities",
                    "duplicate capability",
                ));
            }
        }
        sum_count(
            [
                self.parameters.len(),
                self.constants.len(),
                self.operations.len(),
                self.objects.len(),
                self.assets.len(),
                self.tracks.len(),
                self.constraints.len(),
            ],
            limits.max_nodes,
            "nodes",
        )?;
        sum_count(
            self.operations
                .iter()
                .flat_map(|o| [o.inputs.len(), o.source_refs.len()])
                .chain(self.parameters.iter().map(|p| p.source_refs.len()))
                .chain(self.constants.iter().map(|c| c.source_refs.len()))
                .chain(
                    self.objects
                        .iter()
                        .flat_map(|o| [o.bindings.len(), o.assets.len(), o.properties.len()]),
                ),
            limits.max_edges,
            "edges",
        )?;
        let input_elements = sum_count(
            self.parameters
                .iter()
                .map(|p| p.default.element_count())
                .chain(self.constants.iter().map(|c| c.value.element_count())),
            limits.max_elements,
            "values",
        )?;
        let mut output_elements = input_elements;
        sum_count(
            self.tracks.iter().map(|t| t.keys.len()),
            limits.max_keys,
            "tracks",
        )?;
        let mut ports = BTreeMap::new();
        let mut params = BTreeSet::new();
        for p in &self.parameters {
            if !params.insert(p.id.clone()) {
                return Err(err(
                    DiagnosticCode::Duplicate,
                    format!("parameters/{}", p.id.as_str()),
                    "duplicate parameter",
                ));
            }
            p.port
                .validate_shape(limits.max_elements)
                .map_err(|e| e.at(format!("parameters/{}", p.id.as_str())))?;
            p.validate_value(&p.default)?;
            if ports.insert(p.value_id.clone(), p.port.clone()).is_some() {
                return Err(err(
                    DiagnosticCode::Duplicate,
                    format!("values/{}", p.value_id.as_str()),
                    "duplicate value producer",
                ));
            }
        }
        for c in &self.constants {
            c.port.validate_shape(limits.max_elements)?;
            c.port
                .validate(&c.value)
                .map_err(|e| e.at(format!("constants/{}", c.id.as_str())))?;
            if ports.insert(c.id.clone(), c.port.clone()).is_some() {
                return Err(err(
                    DiagnosticCode::Duplicate,
                    format!("values/{}", c.id.as_str()),
                    "duplicate value producer",
                ));
            }
        }
        let mut ops = BTreeSet::new();
        let mut producers = BTreeMap::new();
        for (i, o) in self.operations.iter().enumerate() {
            if !ops.insert(o.id.clone()) {
                return Err(err(
                    DiagnosticCode::Duplicate,
                    format!("operations/{}", o.id.as_str()),
                    "duplicate operation",
                ));
            }
            o.output_port.validate_shape(limits.max_elements)?;
            match &o.output_port.shape {
                Shape::Vector { len } => {
                    bound(*len, limits.max_elements, "operation shape")?;
                    if *len == 0 {
                        return Err(err(DiagnosticCode::Type, "operation shape", "empty vector"));
                    }
                }
                Shape::Matrix { rows, cols } => {
                    let n = rows.checked_mul(*cols).ok_or_else(|| {
                        err(
                            DiagnosticCode::Limit,
                            "operation shape",
                            "matrix size overflow",
                        )
                    })?;
                    bound(n, limits.max_elements, "operation shape")?;
                    if n == 0 {
                        return Err(err(DiagnosticCode::Type, "operation shape", "empty matrix"));
                    }
                }
                _ => {}
            }
            let count = match &o.output_port.shape {
                Shape::Vector { len } => *len,
                Shape::Matrix { rows, cols } => rows.checked_mul(*cols).ok_or_else(|| {
                    err(
                        DiagnosticCode::Limit,
                        "operation shape",
                        "matrix size overflow",
                    )
                })?,
                Shape::Color { .. } => 4,
                _ => 1,
            };
            output_elements = output_elements.checked_add(count).ok_or_else(|| {
                err(
                    DiagnosticCode::Limit,
                    "values",
                    "value storage count overflow",
                )
            })?;
            bound(output_elements, limits.max_elements, "values")?;
            if ports
                .insert(o.output.clone(), o.output_port.clone())
                .is_some()
            {
                return Err(err(
                    DiagnosticCode::Duplicate,
                    format!("values/{}", o.output.as_str()),
                    "duplicate value producer",
                ));
            }
            producers.insert(o.output.clone(), i);
        }
        let mut objects = BTreeSet::new();
        for o in &self.objects {
            if !objects.insert(o.id.clone()) {
                return Err(err(
                    DiagnosticCode::Duplicate,
                    "objects",
                    "duplicate object",
                ));
            }
        }
        let mut assets = BTreeSet::new();
        for a in &self.assets {
            if !assets.insert(a.id.clone()) {
                return Err(err(DiagnosticCode::Duplicate, "assets", "duplicate asset"));
            }
            if a.uri.is_empty() || a.uri.len() > 4096 {
                return Err(err(
                    DiagnosticCode::Range,
                    format!("assets/{}", a.id.as_str()),
                    "asset URI must be 1..4096 bytes",
                ));
            }
        }
        let check_source = |r: &SourceRef, path: &str| {
            let valid = match r {
                SourceRef::Parameter(id) => params.contains(id),
                SourceRef::Value(id) => ports.contains_key(id),
                SourceRef::Operation(id) => ops.contains(id),
                SourceRef::Object(id) => objects.contains(id),
                SourceRef::Asset(id) => assets.contains(id),
            };
            if valid {
                Ok(())
            } else {
                Err(err(
                    DiagnosticCode::Reference,
                    path,
                    "source reference does not exist",
                ))
            }
        };
        for p in &self.parameters {
            for r in &p.source_refs {
                check_source(r, &format!("parameters/{}/source_refs", p.id.as_str()))?;
            }
        }
        for c in &self.constants {
            for r in &c.source_refs {
                check_source(r, &format!("constants/{}/source_refs", c.id.as_str()))?;
            }
        }
        let mut indegrees = vec![0usize; self.operations.len()];
        let mut children = vec![vec![]; self.operations.len()];
        for (i, o) in self.operations.iter().enumerate() {
            for r in &o.source_refs {
                check_source(r, &format!("operations/{}/source_refs", o.id.as_str()))?;
            }
            let mut deps = BTreeSet::new();
            for id in &o.inputs {
                if !ports.contains_key(id) {
                    return Err(err(
                        DiagnosticCode::Reference,
                        format!("operations/{}/inputs/{}", o.id.as_str(), id.as_str()),
                        "unknown operand",
                    ));
                }
                if let Some(&producer) = producers.get(id) {
                    deps.insert(producer);
                }
            }
            indegrees[i] = deps.len();
            for producer in deps {
                children[producer].push(i);
            }
        }
        let mut ready: BTreeSet<_> = indegrees
            .iter()
            .enumerate()
            .filter_map(|(i, &n)| (n == 0).then_some(i))
            .collect();
        let mut order = vec![];
        while let Some(i) = ready.pop_first() {
            order.push(i);
            for &child in &children[i] {
                indegrees[child] -= 1;
                if indegrees[child] == 0 {
                    ready.insert(child);
                }
            }
        }
        if order.len() != self.operations.len() {
            return Err(err(
                DiagnosticCode::Cycle,
                "operations",
                "operation graph contains a cycle",
            ));
        }
        for o in &self.objects {
            if o.label.len() > 4096 {
                return Err(err(
                    DiagnosticCode::Limit,
                    "objects/label",
                    "label too long",
                ));
            }
            for (property, id) in &o.bindings {
                if property.as_str().is_empty() || property.as_str().len() > 128 {
                    return Err(err(
                        DiagnosticCode::InvalidId,
                        "objects/bindings",
                        "invalid property identifier",
                    ));
                }
                if !ports.contains_key(id) {
                    return Err(err(
                        DiagnosticCode::Reference,
                        "objects/bindings",
                        "unknown value",
                    ));
                }
            }
            for (property, port) in &o.properties {
                port.validate_shape(limits.max_elements).map_err(|e| {
                    e.at(format!(
                        "objects/{}/properties/{}",
                        o.id.as_str(),
                        property.as_str()
                    ))
                })?;
            }
            for (property, source) in &o.bindings {
                let target = o.properties.get(property).ok_or_else(|| {
                    err(
                        DiagnosticCode::Reference,
                        format!("objects/{}/properties/{}", o.id.as_str(), property.as_str()),
                        "binding target missing",
                    )
                })?;
                let source_port = &ports[source];
                if target.shape != source_port.shape {
                    return Err(err(
                        DiagnosticCode::Type,
                        format!("objects/{}/bindings/{}", o.id.as_str(), property.as_str()),
                        "binding shape mismatch",
                    ));
                }
                if target.unit != source_port.unit {
                    return Err(err(
                        DiagnosticCode::Unit,
                        format!("objects/{}/bindings/{}", o.id.as_str(), property.as_str()),
                        "binding unit mismatch",
                    ));
                }
            }
            for a in &o.assets {
                if !assets.contains(a) {
                    return Err(err(
                        DiagnosticCode::Reference,
                        "objects/assets",
                        "unknown asset",
                    ));
                }
            }
        }
        let mut tracked = BTreeSet::new();
        for t in &self.tracks {
            let p = self
                .parameters
                .iter()
                .find(|p| p.id == t.parameter)
                .ok_or_else(|| {
                    err(
                        DiagnosticCode::Reference,
                        "tracks/parameter",
                        "unknown parameter",
                    )
                })?;
            if !tracked.insert(t.parameter.clone()) {
                return Err(err(
                    DiagnosticCode::Duplicate,
                    "tracks/parameter",
                    "multiple tracks require explicit blending (unsupported in schema v1)",
                ));
            }
            if p.port.shape != Shape::Scalar {
                return Err(err(
                    DiagnosticCode::Type,
                    "tracks/parameter",
                    "scalar track requires scalar parameter",
                ));
            }
            if t.keys.is_empty() {
                return Err(err(
                    DiagnosticCode::Time,
                    "tracks/keys",
                    "track requires at least one key",
                ));
            }
            for (i, k) in t.keys.iter().enumerate() {
                p.validate_value(&Value::Scalar(k.value))
                    .map_err(|e| e.at(format!("tracks/{}/keys/{i}", t.parameter.as_str())))?;
                if i > 0 && t.keys[i - 1].time >= k.time {
                    return Err(err(
                        DiagnosticCode::Time,
                        "tracks/keys",
                        "key times must be strictly increasing",
                    ));
                }
                if i > 0 {
                    k.time.checked_sub(t.keys[i - 1].time)?;
                }
            }
        }
        for (i, c) in self.constraints.iter().enumerate() {
            let path = format!("constraints/{i}");
            let ids = c.parameters();
            for id in &ids {
                if !params.contains(*id) {
                    return Err(err(
                        DiagnosticCode::Reference,
                        &path,
                        "unknown constrained parameter",
                    ));
                }
            }
            let first = &self
                .parameters
                .iter()
                .find(|p| &p.id == ids[0])
                .ok_or_else(|| err(DiagnosticCode::Reference, &path, "unknown parameter"))?
                .port;
            match c {
                Constraint::NonZero { .. } if first.shape != Shape::Scalar => {
                    return Err(err(DiagnosticCode::Type, &path, "nonzero requires scalar"));
                }
                Constraint::Equal { right, .. } => {
                    let other = &self
                        .parameters
                        .iter()
                        .find(|p| &p.id == right)
                        .ok_or_else(|| err(DiagnosticCode::Reference, &path, "unknown parameter"))?
                        .port;
                    if first != other {
                        return Err(err(
                            DiagnosticCode::Type,
                            &path,
                            "equality constraint requires identical ports",
                        ));
                    }
                }
                Constraint::ScalarCompare { .. }
                | Constraint::ScalarBounds { .. }
                | Constraint::NormBounds { .. } => {
                    if matches!(
                        c,
                        Constraint::ScalarBounds {
                            min: None,
                            max: None,
                            ..
                        } | Constraint::NormBounds {
                            min: None,
                            max: None,
                            ..
                        }
                    ) {
                        return Err(err(
                            DiagnosticCode::Range,
                            &path,
                            "bound requires at least one endpoint",
                        ));
                    }
                    if matches!(c, Constraint::NormBounds { .. }) {
                        if !matches!(first.shape, Shape::Vector { .. }) {
                            return Err(err(
                                DiagnosticCode::Type,
                                &path,
                                "norm bound requires vector",
                            ));
                        }
                    } else if first.shape != Shape::Scalar {
                        return Err(err(
                            DiagnosticCode::Type,
                            &path,
                            "dependent bounds require scalar",
                        ));
                    }
                    for id in ids.iter().skip(1) {
                        let other = &self
                            .parameters
                            .iter()
                            .find(|p| &p.id == *id)
                            .ok_or_else(|| err(DiagnosticCode::Reference, &path, "missing bound"))?
                            .port;
                        if other.shape != Shape::Scalar || other.unit != first.unit {
                            return Err(err(
                                DiagnosticCode::Unit,
                                &path,
                                "dependent bound requires compatible scalar units",
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
        for p in &self.parameters {
            if let Value::File(Some(id)) = &p.default
                && !assets.contains(id)
            {
                return Err(err(
                    DiagnosticCode::Reference,
                    format!("parameters/{}", p.id.as_str()),
                    "file asset missing",
                ));
            }
        }
        for c in &self.constants {
            if let Value::File(Some(id)) = &c.value
                && !assets.contains(id)
            {
                return Err(err(
                    DiagnosticCode::Reference,
                    format!("constants/{}", c.id.as_str()),
                    "file asset missing",
                ));
            }
        }
        Ok(ValidatedDocument {
            document: self,
            order,
            ports,
        })
    }
}

/// Reject byte/depth excess before deserialization can allocate recursive input.
pub fn validate_json_budget(bytes: &[u8], limits: DecodeLimits) -> Result<()> {
    bound(bytes.len(), limits.max_bytes, "document")?;
    // Bound nesting before serde allocates recursive input. Strings and escaped quotes are ignored.
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for &b in bytes {
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else if b == b'"' {
            quoted = true;
        } else if b == b'{' || b == b'[' {
            depth += 1;
            bound(depth, limits.max_depth, "document")?;
        } else if b == b'}' || b == b']' {
            depth = depth.saturating_sub(1);
        }
    }
    Ok(())
}

impl Document {
    /// Conservative retained heap/inline accounting. BTree nodes include per-entry slack.
    pub fn allocated_bytes(&self) -> Result<usize> {
        self.allocated_bytes_bounded(usize::MAX)
    }
    pub fn allocated_bytes_bounded(&self, limit: usize) -> Result<usize> {
        struct Counter(usize, usize);
        impl Counter {
            fn add(&mut self, n: usize) -> Result<()> {
                self.0 = self
                    .0
                    .checked_add(n)
                    .filter(|n| *n <= self.1)
                    .ok_or_else(|| {
                        err(
                            DiagnosticCode::Limit,
                            "storage",
                            "allocated byte budget exceeded",
                        )
                    })?;
                Ok(())
            }
            fn vec<T>(&mut self, v: &Vec<T>) -> Result<()> {
                self.add(
                    v.capacity()
                        .checked_mul(std::mem::size_of::<T>())
                        .ok_or_else(|| {
                            err(DiagnosticCode::Limit, "storage", "vector capacity overflow")
                        })?,
                )
            }
            fn map<K, V>(&mut self, v: &BTreeMap<K, V>) -> Result<()> {
                self.add(
                    v.len()
                        .checked_mul(std::mem::size_of::<(K, V)>().checked_add(64).ok_or_else(
                            || err(DiagnosticCode::Limit, "storage", "map node size overflow"),
                        )?)
                        .ok_or_else(|| {
                            err(DiagnosticCode::Limit, "storage", "map capacity overflow")
                        })?,
                )
            }
            fn sources(&mut self, v: &Vec<SourceRef>) -> Result<()> {
                self.vec(v)?;
                for r in v {
                    let n = match r {
                        SourceRef::Parameter(id) => id.as_str().len(),
                        SourceRef::Value(id) => id.as_str().len(),
                        SourceRef::Operation(id) => id.as_str().len(),
                        SourceRef::Object(id) => id.as_str().len(),
                        SourceRef::Asset(id) => id.as_str().len(),
                    };
                    self.add(n)?;
                }
                Ok(())
            }
        }
        let mut c = Counter(0, limit);
        c.add(std::mem::size_of::<Self>())?;
        c.add(self.id.as_str().len())?;
        c.vec(&self.required_capabilities)?;
        for x in &self.required_capabilities {
            c.add(x.capacity())?;
        }
        c.vec(&self.parameters)?;
        c.vec(&self.constants)?;
        c.vec(&self.operations)?;
        c.vec(&self.objects)?;
        c.vec(&self.assets)?;
        c.vec(&self.tracks)?;
        c.vec(&self.constraints)?;
        for p in &self.parameters {
            c.add(p.id.as_str().len())?;
            c.add(p.value_id.as_str().len())?;
            c.add(p.default.heap_bytes()?)?;
            c.sources(&p.source_refs)?;
            c.vec(&p.metadata.enum_choices)?;
            for key in &p.metadata.enum_choices {
                c.add(key.capacity())?;
            }
        }
        for k in &self.constants {
            c.add(k.id.as_str().len())?;
            c.add(k.value.heap_bytes()?)?;
            c.sources(&k.source_refs)?;
        }
        for o in &self.operations {
            c.add(o.id.as_str().len())?;
            c.add(o.operator.as_str().len())?;
            c.add(o.output.as_str().len())?;
            c.vec(&o.inputs)?;
            for id in &o.inputs {
                c.add(id.as_str().len())?;
            }
            c.sources(&o.source_refs)?;
        }
        for o in &self.objects {
            c.add(o.id.as_str().len())?;
            c.add(o.label.capacity())?;
            c.map(&o.properties)?;
            for id in o.properties.keys() {
                c.add(id.as_str().len())?;
            }
            c.map(&o.bindings)?;
            for (id, value) in &o.bindings {
                c.add(id.as_str().len())?;
                c.add(value.as_str().len())?;
            }
            c.vec(&o.assets)?;
            for id in &o.assets {
                c.add(id.as_str().len())?;
            }
        }
        for a in &self.assets {
            c.add(a.id.as_str().len())?;
            c.add(a.uri.capacity())?;
        }
        for t in &self.tracks {
            c.add(t.parameter.as_str().len())?;
            c.vec(&t.keys)?;
        }
        for constraint in &self.constraints {
            for id in constraint.parameters() {
                c.add(id.as_str().len())?;
            }
        }
        Ok(c.0)
    }
}
