use crate::TransactionId;
use serde::{Deserialize, Serialize};
use sim_model::*;
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DocumentEdit {
    PutParameter { parameter: Parameter },
    DeleteParameter { id: ParameterId },
    PutConstant { constant: Constant },
    DeleteConstant { id: ValueId },
    PutOperation { operation: Operation },
    DeleteOperation { id: OperationId },
    PutObject { object: SceneObject },
    DeleteObject { id: ObjectId },
    PutAsset { asset: Asset },
    DeleteAsset { id: AssetId },
    PutTrack { track: ScalarTrack },
    DeleteTrack { parameter: ParameterId },
    SetConstraints { constraints: Vec<Constraint> },
    SetPermissions { permissions: LearnerPermissions },
    Unlock { parameter: ParameterId },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverridePlaybackPolicy {
    Pause,
    Preserve,
    SetIntent(bool),
}
pub(crate) fn validate_patch_entries(entries: &[(ParameterId, Option<Value>)]) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for (id, _) in entries {
        if !seen.insert(id) {
            return Err(Diagnostic::new(
                DiagnosticCode::Duplicate,
                "override_patch/entries",
                "repeated parameter in atomic override patch",
            ));
        }
    }
    Ok(())
}
pub(crate) fn deserialize_patch_entries<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<(ParameterId, Option<Value>)>, D::Error> {
    let entries = Vec::<(ParameterId, Option<Value>)>::deserialize(d)?;
    validate_patch_entries(&entries).map_err(serde::de::Error::custom)?;
    Ok(entries)
}
pub(crate) fn patch_value_bytes(value: &Value) -> Result<usize> {
    match value {
        Value::Vector(xs) | Value::Matrix { values: xs, .. } => xs
            .len()
            .checked_mul(std::mem::size_of::<Finite>())
            .ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::Limit,
                    "override_patch/value",
                    "logical byte overflow",
                )
            }),
        Value::Enum(key) => Ok(key.len()),
        _ => value.heap_bytes(),
    }
}
pub(crate) fn patch_entries_bytes(entries: &Vec<(ParameterId, Option<Value>)>) -> Result<usize> {
    let mut n = array(entries)?;
    for (id, value) in entries {
        n = add(n, id.as_str().len())?;
        if let Some(value) = value {
            n = add(n, value.heap_bytes()?)?;
        }
    }
    Ok(n)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommandKind {
    SetParameter {
        parameter: ParameterId,
        value: Value,
    },
    SetParameters {
        expected_document_revision: u64,
        #[serde(deserialize_with = "deserialize_unique_map")]
        values: BTreeMap<ParameterId, Value>,
    },
    ResetParameters {
        expected_document_revision: u64,
        parameters: Vec<ParameterId>,
    },
    EditDocument {
        expected_document_revision: u64,
        edits: Vec<DocumentEdit>,
    },
    BeginTransaction,
    UpdateTransaction {
        id: TransactionId,
        #[serde(deserialize_with = "deserialize_unique_map")]
        values: BTreeMap<ParameterId, Value>,
    },
    UpdateDocumentTransaction {
        id: TransactionId,
        expected_document_revision: u64,
        edits: Vec<DocumentEdit>,
    },
    CommitTransaction {
        id: TransactionId,
    },
    CancelTransaction {
        id: TransactionId,
    },
    Undo,
    Redo,
    Seek {
        time: TimeTicks,
    },
    SetLiveOverride {
        parameter: ParameterId,
        value: Value,
    },
    SetLiveOverrides {
        #[serde(deserialize_with = "deserialize_unique_map")]
        values: BTreeMap<ParameterId, Value>,
    },
    ReturnToGuide {
        parameters: Vec<ParameterId>,
    },
    PatchLiveOverrides {
        #[serde(deserialize_with = "deserialize_patch_entries")]
        entries: Vec<(ParameterId, Option<Value>)>,
        playback: OverridePlaybackPolicy,
    },
    SetPlaying {
        playing: bool,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub session: SessionId,
    pub generation: GenerationId,
    pub document: DocumentId,
    pub epoch: u64,
    pub expected_document_revision: u64,
    pub expected_revision: u64,
    pub kind: CommandKind,
}

fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or_else(|| {
        Diagnostic::new(
            DiagnosticCode::Limit,
            "command/storage",
            "payload byte overflow",
        )
    })
}
fn array<T>(v: &Vec<T>) -> Result<usize> {
    v.capacity()
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::Limit,
                "command/storage",
                "array capacity overflow",
            )
        })
}
fn sources(v: &Vec<SourceRef>) -> Result<usize> {
    let mut n = array(v)?;
    for r in v {
        let bytes = match r {
            SourceRef::Parameter(id) => id.as_str().len(),
            SourceRef::Value(id) => id.as_str().len(),
            SourceRef::Operation(id) => id.as_str().len(),
            SourceRef::Object(id) => id.as_str().len(),
            SourceRef::Asset(id) => id.as_str().len(),
        };
        n = add(n, bytes)?;
    }
    Ok(n)
}
fn values(v: &BTreeMap<ParameterId, Value>) -> Result<usize> {
    let mut n = v
        .len()
        .checked_mul(std::mem::size_of::<(ParameterId, Value)>() + 64)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::Limit,
                "command/storage",
                "map storage overflow",
            )
        })?;
    for (id, value) in v {
        n = add(n, id.as_str().len())?;
        n = add(n, value.heap_bytes()?)?;
    }
    Ok(n)
}
impl DocumentEdit {
    pub(crate) fn heap_bytes(&self) -> Result<usize> {
        match self {
            Self::PutParameter { parameter: p } => {
                let mut n = add(p.default.heap_bytes()?, sources(&p.source_refs)?)?;
                n = add(n, array(&p.metadata.enum_choices)?)?;
                for k in &p.metadata.enum_choices {
                    n = add(n, k.capacity())?;
                }
                Ok(n)
            }
            Self::PutConstant { constant: c } => {
                add(c.value.heap_bytes()?, sources(&c.source_refs)?)
            }
            Self::PutOperation { operation: o } => add(array(&o.inputs)?, sources(&o.source_refs)?),
            Self::PutObject { object: o } => {
                let maps = o
                    .bindings
                    .len()
                    .checked_mul(std::mem::size_of::<(PropertyId, ValueId)>() + 64)
                    .and_then(|n| {
                        o.properties
                            .len()
                            .checked_mul(std::mem::size_of::<(PropertyId, Port)>() + 64)
                            .and_then(|m| n.checked_add(m))
                    })
                    .ok_or_else(|| {
                        Diagnostic::new(
                            DiagnosticCode::Limit,
                            "command/storage",
                            "object map overflow",
                        )
                    })?;
                add(add(o.label.capacity(), array(&o.assets)?)?, maps)
            }
            Self::PutAsset { asset: a } => Ok(a.uri.capacity()),
            Self::PutTrack { track: t } => array(&t.keys),
            Self::SetConstraints { constraints } => array(constraints),
            _ => Ok(0),
        }
    }
}
pub(crate) fn edits_heap_bytes(edits: &Vec<DocumentEdit>) -> Result<usize> {
    let mut n = array(edits)?;
    for edit in edits {
        n = add(n, edit.heap_bytes()?)?;
    }
    Ok(n)
}
impl Command {
    pub fn decode(bytes: &[u8], limits: DecodeLimits) -> Result<Self> {
        validate_json_budget(bytes, limits)?;
        let command: Self = serde_json::from_slice(bytes)
            .map_err(|e| Diagnostic::new(DiagnosticCode::Schema, "command_json", e.to_string()))?;
        if let CommandKind::PatchLiveOverrides { entries, .. } = &command.kind {
            if entries.len() > limits.max_nodes {
                return Err(Diagnostic::new(
                    DiagnosticCode::Limit,
                    "override_patch/entries",
                    "patch entry limit exceeded",
                ));
            }
            let mut elements = 0usize;
            for (_, value) in entries {
                if let Some(value) = value {
                    value.validate()?;
                    elements = elements.checked_add(value.element_count()).ok_or_else(|| {
                        Diagnostic::new(
                            DiagnosticCode::Limit,
                            "override_patch/entries",
                            "patch element overflow",
                        )
                    })?;
                }
            }
            if elements > limits.max_elements {
                return Err(Diagnostic::new(
                    DiagnosticCode::Limit,
                    "override_patch/entries",
                    "patch element limit exceeded",
                ));
            }
        }
        if command.allocated_bytes()? > limits.max_bytes {
            return Err(Diagnostic::new(
                DiagnosticCode::Limit,
                "command_json",
                "decoded command allocation exceeds byte limit",
            ));
        }
        Ok(command)
    }
    pub fn encode(&self, limit: usize) -> Result<Vec<u8>> {
        if self.allocated_bytes()? > limit {
            return Err(Diagnostic::new(
                DiagnosticCode::Limit,
                "command_json",
                "command allocation exceeds byte limit",
            ));
        }
        bounded_size(self, limit)?;
        serde_json::to_vec(self)
            .map_err(|e| Diagnostic::new(DiagnosticCode::Schema, "command_json", e.to_string()))
    }
    pub(crate) fn allocated_bytes(&self) -> Result<usize> {
        let heap = match &self.kind {
            CommandKind::PatchLiveOverrides { entries, .. } => patch_entries_bytes(entries)?,
            CommandKind::SetParameter { value, .. }
            | CommandKind::SetLiveOverride { value, .. } => value.heap_bytes()?,
            CommandKind::SetParameters { values: map, .. }
            | CommandKind::SetLiveOverrides { values: map }
            | CommandKind::UpdateTransaction { values: map, .. } => values(map)?,
            CommandKind::ResetParameters { parameters, .. }
            | CommandKind::ReturnToGuide { parameters } => array(parameters)?,
            CommandKind::EditDocument { edits, .. }
            | CommandKind::UpdateDocumentTransaction { edits, .. } => edits_heap_bytes(edits)?,
            _ => 0,
        };
        add(std::mem::size_of::<Self>(), heap)
    }
}
pub(crate) fn apply_edits(raw: &mut Document, edits: Vec<DocumentEdit>) -> Result<()> {
    fn remove<T>(items: &mut Vec<T>, matches: impl Fn(&T) -> bool) -> Result<()> {
        let i = items.iter().position(matches).ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::Reference,
                "edit/delete",
                "entity does not exist",
            )
        })?;
        items.remove(i);
        Ok(())
    }
    fn put<T>(items: &mut Vec<T>, item: T, matches: impl Fn(&T) -> bool) {
        if let Some(i) = items.iter().position(matches) {
            items[i] = item;
        } else {
            items.push(item);
        }
    }
    for edit in edits {
        match edit {
            DocumentEdit::Unlock { parameter } => {
                let p = raw
                    .parameters
                    .iter_mut()
                    .find(|p| p.id == parameter)
                    .ok_or_else(|| {
                        Diagnostic::new(
                            DiagnosticCode::Reference,
                            "edit/unlock",
                            "parameter missing",
                        )
                    })?;
                p.metadata.locked = false;
            }
            DocumentEdit::PutParameter { parameter } => {
                if let Some(old) = raw.parameters.iter().find(|p| p.id == parameter.id)
                    && (old.metadata.locked || old.readonly)
                {
                    return Err(Diagnostic::new(
                        DiagnosticCode::Permission,
                        format!("parameters/{}", old.id.as_str()),
                        "unlock locked parameter explicitly; readonly definitions cannot be replaced",
                    ));
                }
                let id = parameter.id.clone();
                put(&mut raw.parameters, parameter, |p| p.id == id);
            }
            DocumentEdit::DeleteParameter { id } => {
                if raw
                    .parameters
                    .iter()
                    .any(|p| p.id == id && (p.readonly || p.metadata.locked))
                {
                    return Err(Diagnostic::new(
                        DiagnosticCode::Permission,
                        "edit/delete",
                        "parameter is locked or readonly",
                    ));
                }
                remove(&mut raw.parameters, |p| p.id == id)?;
            }
            DocumentEdit::PutConstant { constant } => {
                let id = constant.id.clone();
                put(&mut raw.constants, constant, |c| c.id == id);
            }
            DocumentEdit::DeleteConstant { id } => remove(&mut raw.constants, |c| c.id == id)?,
            DocumentEdit::PutOperation { operation } => {
                let id = operation.id.clone();
                put(&mut raw.operations, operation, |o| o.id == id);
            }
            DocumentEdit::DeleteOperation { id } => remove(&mut raw.operations, |o| o.id == id)?,
            DocumentEdit::PutObject { object } => {
                let id = object.id.clone();
                put(&mut raw.objects, object, |o| o.id == id);
            }
            DocumentEdit::DeleteObject { id } => remove(&mut raw.objects, |o| o.id == id)?,
            DocumentEdit::PutAsset { asset } => {
                let id = asset.id.clone();
                put(&mut raw.assets, asset, |a| a.id == id);
            }
            DocumentEdit::DeleteAsset { id } => remove(&mut raw.assets, |a| a.id == id)?,
            DocumentEdit::PutTrack { track } => {
                if let Some(p) = raw.parameters.iter().find(|p| p.id == track.parameter) {
                    crate::writable(p)?;
                }
                let id = track.parameter.clone();
                put(&mut raw.tracks, track, |t| t.parameter == id);
            }
            DocumentEdit::DeleteTrack { parameter } => {
                if let Some(p) = raw.parameters.iter().find(|p| p.id == parameter) {
                    crate::writable(p)?;
                }
                remove(&mut raw.tracks, |t| t.parameter == parameter)?
            }
            DocumentEdit::SetConstraints { constraints } => raw.constraints = constraints,
            DocumentEdit::SetPermissions { permissions } => raw.learner_permissions = permissions,
        }
    }
    Ok(())
}
