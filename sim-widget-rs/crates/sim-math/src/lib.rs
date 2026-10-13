#![forbid(unsafe_code)]
use serde::Serialize;
use sim_model::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    Add,
    Sub,
    Mul,
    Div,
    Sqrt,
    Sin,
    Cos,
    VectorAdd,
    VectorSub,
    Scale,
    Dot,
    Norm,
}
impl Builtin {
    fn arity(self) -> usize {
        match self {
            Self::Sqrt | Self::Sin | Self::Cos | Self::Norm => 1,
            _ => 2,
        }
    }
    fn output(self, ports: &[&Port]) -> Result<Port> {
        if ports.len() != self.arity() {
            return Err(error(DiagnosticCode::Type, "operator arity mismatch"));
        }
        let a = ports[0];
        let require_scalar = |p: &Port| {
            if p.shape == Shape::Scalar {
                Ok(())
            } else {
                Err(error(DiagnosticCode::Type, "operator requires scalar"))
            }
        };
        let require_vector = |p: &Port| {
            if matches!(p.shape, Shape::Vector { .. }) {
                Ok(())
            } else {
                Err(error(DiagnosticCode::Type, "operator requires vector"))
            }
        };
        let same = |b: &Port| {
            if a == b {
                Ok(())
            } else if a.shape != b.shape {
                Err(error(DiagnosticCode::Type, "operand shapes differ"))
            } else {
                Err(error(DiagnosticCode::Unit, "operand units differ"))
            }
        };
        let (shape, unit) = match self {
            Self::Add | Self::Sub => {
                require_scalar(a)?;
                same(ports[1])?;
                (Shape::Scalar, a.unit)
            }
            Self::Mul | Self::Div => {
                require_scalar(a)?;
                require_scalar(ports[1])?;
                (
                    Shape::Scalar,
                    if self == Self::Mul {
                        a.unit.multiply(ports[1].unit)?
                    } else {
                        a.unit.divide(ports[1].unit)?
                    },
                )
            }
            Self::Sqrt => {
                require_scalar(a)?;
                (Shape::Scalar, a.unit.sqrt()?)
            }
            Self::Sin | Self::Cos => {
                require_scalar(a)?;
                if a.unit != Unit::ANGLE {
                    return Err(error(
                        DiagnosticCode::Unit,
                        "trigonometric input must be radians",
                    ));
                }
                (Shape::Scalar, Unit::DIMENSIONLESS)
            }
            Self::VectorAdd | Self::VectorSub => {
                require_vector(a)?;
                same(ports[1])?;
                (a.shape.clone(), a.unit)
            }
            Self::Scale => {
                require_vector(a)?;
                require_scalar(ports[1])?;
                (a.shape.clone(), a.unit.multiply(ports[1].unit)?)
            }
            Self::Dot => {
                require_vector(a)?;
                require_vector(ports[1])?;
                if a.shape != ports[1].shape {
                    return Err(error(DiagnosticCode::Type, "dot operand dimensions differ"));
                }
                (Shape::Scalar, a.unit.multiply(ports[1].unit)?)
            }
            Self::Norm => {
                require_vector(a)?;
                (Shape::Scalar, a.unit)
            }
        };
        Ok(Port { shape, unit })
    }
    fn evaluate(self, values: &[Value]) -> Result<Value> {
        if values.len() != self.arity() {
            return Err(error(DiagnosticCode::Type, "operator arity mismatch"));
        }
        let a = &values[0];
        let vec = |v: &Value| match v {
            Value::Vector(xs) => Ok(xs.iter().map(|x| x.get()).collect::<Vec<_>>()),
            _ => Err(error(DiagnosticCode::Type, "expected vector")),
        };
        match self {
            Self::Add => Value::scalar(a.as_scalar()? + values[1].as_scalar()?),
            Self::Sub => Value::scalar(a.as_scalar()? - values[1].as_scalar()?),
            Self::Mul => Value::scalar(a.as_scalar()? * values[1].as_scalar()?),
            Self::Div => {
                let divisor = values[1].as_scalar()?;
                if divisor == 0. {
                    return Err(error(DiagnosticCode::Singular, "division by zero"));
                }
                Value::scalar(a.as_scalar()? / divisor)
            }
            Self::Sqrt => {
                let x = a.as_scalar()?;
                if x < 0. {
                    return Err(error(
                        DiagnosticCode::Singular,
                        "square root of a negative value",
                    ));
                }
                Value::scalar(libm::sqrt(x))
            }
            Self::Sin => Value::scalar(libm::sin(a.as_scalar()?)),
            Self::Cos => Value::scalar(libm::cos(a.as_scalar()?)),
            Self::VectorAdd | Self::VectorSub | Self::Dot => {
                let x = vec(a)?;
                let y = vec(&values[1])?;
                if x.len() != y.len() {
                    return Err(error(DiagnosticCode::Type, "vector dimensions differ"));
                }
                if self == Self::Dot {
                    Value::scalar(
                        x.iter()
                            .zip(&y)
                            .map(|(a, b)| a * b)
                            .reduce(|sum, p| sum + p)
                            .unwrap_or(0.),
                    )
                } else {
                    Value::vector(
                        x.iter()
                            .zip(&y)
                            .map(|(a, b)| {
                                if self == Self::VectorAdd {
                                    a + b
                                } else {
                                    a - b
                                }
                            })
                            .collect(),
                    )
                }
            }
            Self::Scale => {
                let x = vec(a)?;
                let scale = values[1].as_scalar()?;
                Value::vector(x.iter().map(|x| x * scale).collect())
            }
            Self::Norm => Value::scalar(vec(a)?.iter().fold(0., |acc, x| libm::hypot(acc, *x))),
        }
    }
}
fn error(code: DiagnosticCode, message: &str) -> Diagnostic {
    Diagnostic::new(code, "operator", message)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperatorDescriptor {
    pub id: OperatorId,
    pub builtin: Builtin,
    pub description: String,
}
/// A frozen descriptor table. Construct a new registry to change supported capabilities.
#[derive(Clone, Debug)]
pub struct OperatorRegistry {
    operators: Arc<BTreeMap<OperatorId, OperatorDescriptor>>,
}
impl OperatorRegistry {
    /// Frozen descriptor storage is shared by all compiled graphs and candidate sessions.
    pub fn allocated_bytes(&self) -> Result<usize> {
        let mut n = self
            .operators
            .len()
            .checked_mul(std::mem::size_of::<(OperatorId, OperatorDescriptor)>() + 64)
            .and_then(|n| {
                n.checked_add(
                    std::mem::size_of::<Self>()
                        + std::mem::size_of::<BTreeMap<OperatorId, OperatorDescriptor>>()
                        + 2 * std::mem::size_of::<usize>(),
                )
            })
            .ok_or_else(|| error(DiagnosticCode::Limit, "registry storage overflow"))?;
        for (key, d) in self.operators.iter() {
            n = n
                .checked_add(key.as_str().len())
                .and_then(|n| n.checked_add(d.id.as_str().len()))
                .and_then(|n| n.checked_add(d.description.capacity()))
                .ok_or_else(|| error(DiagnosticCode::Limit, "registry storage overflow"))?;
        }
        Ok(n)
    }
    pub fn from_descriptors(descriptors: Vec<OperatorDescriptor>) -> Result<Self> {
        if descriptors.len() > 4096 {
            return Err(error(
                DiagnosticCode::Limit,
                "operator registry entry limit",
            ));
        }
        let mut bytes = 0usize;
        for d in &descriptors {
            if d.description.capacity() > 4096 {
                return Err(error(DiagnosticCode::Limit, "operator description limit"));
            }
            bytes = bytes
                .checked_add(d.description.capacity())
                .and_then(|n| n.checked_add(d.id.as_str().len()))
                .ok_or_else(|| error(DiagnosticCode::Limit, "registry byte overflow"))?;
        }
        if bytes > 1048576 {
            return Err(error(DiagnosticCode::Limit, "registry byte limit"));
        }
        let mut operators = BTreeMap::new();
        for d in descriptors {
            if operators.insert(d.id.clone(), d).is_some() {
                return Err(error(DiagnosticCode::Duplicate, "duplicate operator ID"));
            }
        }
        Ok(Self {
            operators: Arc::new(operators),
        })
    }
    pub fn standard() -> Result<Self> {
        let entries = [
            ("scalar.add", Builtin::Add),
            ("scalar.sub", Builtin::Sub),
            ("scalar.mul", Builtin::Mul),
            ("scalar.div", Builtin::Div),
            ("scalar.sqrt", Builtin::Sqrt),
            ("scalar.sin", Builtin::Sin),
            ("scalar.cos", Builtin::Cos),
            ("vector.add", Builtin::VectorAdd),
            ("vector.sub", Builtin::VectorSub),
            ("vector.scale", Builtin::Scale),
            ("vector.dot", Builtin::Dot),
            ("vector.norm", Builtin::Norm),
        ];
        let descriptors = entries
            .into_iter()
            .map(|(id, builtin)| {
                Ok(OperatorDescriptor {
                    id: OperatorId::new(id)?,
                    builtin,
                    description: format!("{builtin:?}"),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Self::from_descriptors(descriptors)
    }
    pub fn descriptors(&self) -> impl Iterator<Item = &OperatorDescriptor> {
        self.operators.values()
    }
    pub fn descriptor(&self, id: &OperatorId) -> Result<&OperatorDescriptor> {
        self.operators.get(id).ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::Capability,
                format!("operators/{}", id.as_str()),
                "operator is not installed",
            )
        })
    }
    pub fn compile(&self, doc: ValidatedDocument) -> Result<CompiledGraph> {
        for &i in doc.operation_order() {
            let o = &doc.document().operations[i];
            let path = format!("operations/{}", o.id.as_str());
            let op = self.descriptor(&o.operator).map_err(|e| e.at(&path))?;
            let ports = o
                .inputs
                .iter()
                .map(|id| {
                    doc.ports()
                        .get(id)
                        .ok_or_else(|| error(DiagnosticCode::Reference, "unknown operand"))
                })
                .collect::<Result<Vec<_>>>()?;
            let computed = op.builtin.output(&ports).map_err(|e| e.at(&path))?;
            if computed.shape != o.output_port.shape {
                return Err(Diagnostic::new(
                    DiagnosticCode::Type,
                    &path,
                    "declared result shape disagrees with operator",
                ));
            }
            if computed.unit != o.output_port.unit {
                return Err(Diagnostic::new(
                    DiagnosticCode::Unit,
                    &path,
                    "declared result units disagree with operator",
                ));
            }
        }
        let mut reverse: BTreeMap<ValueId, Vec<usize>> = BTreeMap::new();
        for (i, o) in doc.document().operations.iter().enumerate() {
            for id in &o.inputs {
                reverse.entry(id.clone()).or_default().push(i);
            }
        }
        Ok(CompiledGraph {
            doc: Arc::new(doc),
            registry: self.clone(),
            reverse: Arc::new(reverse),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum InputOrigin {
    Default,
    Constant,
    Track,
    Override,
    Derived,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Operand {
    pub id: ValueId,
    pub value: Value,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Provenance {
    pub origin: InputOrigin,
    pub parameter: Option<ParameterId>,
    pub operation: Option<OperationId>,
    pub operator: Option<OperatorId>,
    pub operands: Vec<Operand>,
    pub result: Value,
    pub source_refs: Vec<SourceRef>,
    pub validated_constraints: Vec<usize>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Evaluation {
    truth: BTreeMap<ValueId, Value>,
    provenance: BTreeMap<ValueId, Provenance>,
    evaluated_operations: Vec<OperationId>,
    contract: Arc<ValidatedDocument>,
}
impl Evaluation {
    pub fn evaluated_operations(&self) -> &[OperationId] {
        &self.evaluated_operations
    }
    pub fn truth(&self) -> &BTreeMap<ValueId, Value> {
        &self.truth
    }
    pub fn provenance(&self) -> &BTreeMap<ValueId, Provenance> {
        &self.provenance
    }
}
#[derive(Clone, Debug)]
pub struct CompiledGraph {
    doc: Arc<ValidatedDocument>,
    registry: OperatorRegistry,
    reverse: Arc<BTreeMap<ValueId, Vec<usize>>>,
}
impl CompiledGraph {
    pub fn shares_registry(&self, registry: &OperatorRegistry) -> bool {
        Arc::ptr_eq(&self.registry.operators, &registry.operators)
    }
    /// Document and dependency-cache ownership, excluding the separately shared registry.
    pub fn cache_bytes(&self) -> Result<usize> {
        let mut n = self
            .doc
            .allocated_bytes()?
            .checked_add(std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>())
            .and_then(|n| {
                self.reverse
                    .len()
                    .checked_mul(std::mem::size_of::<(ValueId, Vec<usize>)>() + 64)
                    .and_then(|m| n.checked_add(m))
            })
            .ok_or_else(|| error(DiagnosticCode::Limit, "compiled storage overflow"))?;
        for (id, edges) in self.reverse.iter() {
            n = n
                .checked_add(id.as_str().len())
                .and_then(|n| {
                    edges
                        .capacity()
                        .checked_mul(std::mem::size_of::<usize>())
                        .and_then(|m| n.checked_add(m))
                })
                .ok_or_else(|| error(DiagnosticCode::Limit, "dependency storage overflow"))?;
        }
        Ok(n)
    }
    pub fn document_snapshot(&self) -> Arc<ValidatedDocument> {
        self.doc.clone()
    }
    pub fn document(&self) -> &ValidatedDocument {
        &self.doc
    }
    pub fn evaluate(
        &self,
        inputs: &BTreeMap<ParameterId, Value>,
        origins: &BTreeMap<ParameterId, InputOrigin>,
    ) -> Result<Evaluation> {
        self.evaluate_cached(inputs, origins, None)
    }
    pub fn evaluate_incremental(
        &self,
        inputs: &BTreeMap<ParameterId, Value>,
        origins: &BTreeMap<ParameterId, InputOrigin>,
        previous: &Evaluation,
    ) -> Result<Evaluation> {
        if !Arc::ptr_eq(&self.doc, &previous.contract) {
            return Err(error(
                DiagnosticCode::StaleRevision,
                "incremental cache belongs to a different document contract",
            ));
        }
        self.evaluate_cached(inputs, origins, Some(previous))
    }
    fn evaluate_cached(
        &self,
        inputs: &BTreeMap<ParameterId, Value>,
        origins: &BTreeMap<ParameterId, InputOrigin>,
        previous: Option<&Evaluation>,
    ) -> Result<Evaluation> {
        self.doc.validate_effective(inputs)?;
        if origins.len() != inputs.len() || inputs.keys().any(|id| !origins.contains_key(id)) {
            return Err(error(
                DiagnosticCode::Reference,
                "every effective parameter needs a provenance origin",
            ));
        }
        let mut truth = previous.map(|p| p.truth.clone()).unwrap_or_default();
        let mut provenance = previous.map(|p| p.provenance.clone()).unwrap_or_default();
        let mut roots = BTreeSet::new();
        let mut evaluated_operations = vec![];
        for p in &self.doc.document().parameters {
            let value = inputs[&p.id].clone();
            let origin = origins[&p.id];
            if !matches!(
                origin,
                InputOrigin::Default | InputOrigin::Track | InputOrigin::Override
            ) {
                return Err(error(DiagnosticCode::Type, "invalid parameter origin"));
            }
            let constraints = self
                .doc
                .document()
                .constraints
                .iter()
                .enumerate()
                .filter_map(|(i, c)| c.parameters().contains(&&p.id).then_some(i))
                .collect();
            if truth.get(&p.value_id) != Some(&value) {
                roots.insert(p.value_id.clone());
            }
            truth.insert(p.value_id.clone(), value.clone());
            provenance.insert(
                p.value_id.clone(),
                Provenance {
                    origin,
                    parameter: Some(p.id.clone()),
                    operation: None,
                    operator: None,
                    operands: vec![],
                    result: value,
                    source_refs: p.source_refs.clone(),
                    validated_constraints: constraints,
                },
            );
        }
        for c in &self.doc.document().constants {
            truth.insert(c.id.clone(), c.value.clone());
            provenance.insert(
                c.id.clone(),
                Provenance {
                    origin: InputOrigin::Constant,
                    parameter: None,
                    operation: None,
                    operator: None,
                    operands: vec![],
                    result: c.value.clone(),
                    source_refs: c.source_refs.clone(),
                    validated_constraints: vec![],
                },
            );
        }
        let mut dirty = BTreeSet::new();
        let mut pending = roots.into_iter().collect::<Vec<_>>();
        while let Some(id) = pending.pop() {
            if let Some(children) = self.reverse.get(&id) {
                for &i in children {
                    if dirty.insert(i) {
                        pending.push(self.doc.document().operations[i].output.clone());
                    }
                }
            }
        }
        for &i in self.doc.operation_order() {
            if previous.is_some() && !dirty.contains(&i) {
                continue;
            }
            let o = &self.doc.document().operations[i];
            evaluated_operations.push(o.id.clone());
            let operands = o
                .inputs
                .iter()
                .map(|id| {
                    truth
                        .get(id)
                        .cloned()
                        .map(|value| Operand {
                            id: id.clone(),
                            value,
                        })
                        .ok_or_else(|| {
                            error(DiagnosticCode::Reference, "missing evaluated operand")
                        })
                })
                .collect::<Result<Vec<_>>>()?;
            let values = operands.iter().map(|x| x.value.clone()).collect::<Vec<_>>();
            let result = self
                .registry
                .descriptor(&o.operator)?
                .builtin
                .evaluate(&values)
                .map_err(|e| e.at(format!("operations/{}", o.id.as_str())))?;
            o.output_port
                .validate(&result)
                .map_err(|e| e.at(format!("operations/{}", o.id.as_str())))?;
            truth.insert(o.output.clone(), result.clone());
            provenance.insert(
                o.output.clone(),
                Provenance {
                    origin: InputOrigin::Derived,
                    parameter: None,
                    operation: Some(o.id.clone()),
                    operator: Some(o.operator.clone()),
                    operands,
                    result,
                    source_refs: o.source_refs.clone(),
                    validated_constraints: vec![],
                },
            );
        }
        Ok(Evaluation {
            truth,
            provenance,
            evaluated_operations,
            contract: self.doc.clone(),
        })
    }
}
