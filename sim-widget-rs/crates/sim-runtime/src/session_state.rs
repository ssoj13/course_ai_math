use crate::{Command, CommandKind, Session, fail};
use serde::{Deserialize, Serialize};
use sim_model::*;
use std::collections::BTreeMap;
pub const SESSION_VERSION: u32 = 2;
fn serialize_session_version<S: serde::Serializer>(
    version: &u32,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    if !matches!(*version, 1 | SESSION_VERSION) {
        return Err(serde::ser::Error::custom("unsupported session version"));
    }
    s.serialize_u32(SESSION_VERSION)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCheckpoint {
    #[serde(serialize_with = "serialize_session_version")]
    pub version: u32,
    pub session: SessionId,
    pub epoch: u64,
    pub document: DocumentId,
    pub document_revision: u64,
    pub sequence: u64,
    pub revision: u64,
    pub time: TimeTicks,
    #[serde(deserialize_with = "deserialize_unique_map")]
    pub overrides: BTreeMap<ParameterId, Value>,
    pub playback_intent: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventKind {
    Seek {
        time: TimeTicks,
    },
    LiveOverride {
        parameter: ParameterId,
        value: Value,
    },
    LiveOverrides {
        #[serde(deserialize_with = "deserialize_unique_map")]
        values: BTreeMap<ParameterId, Value>,
    },
    ResetDefaults {
        #[serde(deserialize_with = "deserialize_unique_map")]
        values: BTreeMap<ParameterId, Value>,
    },
    ReturnToGuide {
        parameters: Vec<ParameterId>,
    },
    PatchLiveOverrides {
        #[serde(deserialize_with = "crate::commands::deserialize_patch_entries")]
        entries: Vec<(ParameterId, Option<Value>)>,
        playback: crate::OverridePlaybackPolicy,
    },
    Playback {
        playing: bool,
    },
    AuthorRevision {
        document_revision: u64,
        document: Box<Document>,
    },
    TransactionBegin {
        id: crate::TransactionId,
    },
    TransactionUpdate {
        id: crate::TransactionId,
        #[serde(deserialize_with = "deserialize_unique_map")]
        values: BTreeMap<ParameterId, Value>,
    },
    TransactionDocumentUpdate {
        id: crate::TransactionId,
        edits: Vec<crate::DocumentEdit>,
    },
    TransactionCancel {
        id: crate::TransactionId,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEvent {
    #[serde(serialize_with = "serialize_session_version")]
    pub version: u32,
    pub session: SessionId,
    pub epoch: u64,
    pub document: DocumentId,
    pub document_revision: u64,
    pub sequence: u64,
    pub revision: u64,
    pub time: TimeTicks,
    pub kind: EventKind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionEventWire {
    version: u32,
    session: SessionId,
    epoch: u64,
    document: DocumentId,
    document_revision: u64,
    sequence: u64,
    revision: u64,
    time: TimeTicks,
    kind: EventKind,
}
impl<'de> Deserialize<'de> for SessionEvent {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let wire = SessionEventWire::deserialize(d)?;
        if wire.version == 1 && matches!(wire.kind, EventKind::PatchLiveOverrides { .. }) {
            return Err(serde::de::Error::custom(
                "atomic override patch is not a V1 event",
            ));
        }
        Ok(Self {
            version: wire.version,
            session: wire.session,
            epoch: wire.epoch,
            document: wire.document,
            document_revision: wire.document_revision,
            sequence: wire.sequence,
            revision: wire.revision,
            time: wire.time,
            kind: wire.kind,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEnvelope {
    #[serde(serialize_with = "serialize_session_version")]
    pub version: u32,
    pub baseline: SessionCheckpoint,
    pub retained_start: u64,
    pub retained_end: u64,
    pub events: Vec<SessionEvent>,
}
impl SessionCheckpoint {
    pub(crate) fn into_current(mut self) -> Result<Self> {
        if !matches!(self.version, 1 | SESSION_VERSION) {
            return Err(fail(
                DiagnosticCode::Replay,
                "unsupported checkpoint version",
            ));
        }
        self.version = SESSION_VERSION;
        Ok(self)
    }
    pub(crate) fn validate_for(
        &self,
        doc: &ValidatedDocument,
        config: crate::SessionConfig,
        identity: &SessionId,
        epoch: u64,
    ) -> Result<()> {
        if self.version != SESSION_VERSION
            || self.session != *identity
            || self.epoch != epoch
            || self.document != doc.document().id
            || self.document_revision != doc.document().revision
        {
            return Err(fail(
                DiagnosticCode::Replay,
                "checkpoint version/identity/document revision/epoch mismatch",
            ));
        }
        self.storage_bytes(config.max_command_bytes)?;
        if config.role == crate::SessionRole::Learner {
            let permissions = doc.document().learner_permissions;
            if (self.time != TimeTicks::ZERO && !permissions.seek)
                || (!self.overrides.is_empty() && !permissions.overrides)
                || (self.playback_intent && !permissions.playback)
            {
                return Err(fail(
                    DiagnosticCode::Permission,
                    "learner checkpoint permission denied",
                ));
            }
        }
        for (id, value) in &self.overrides {
            let p = doc.parameter(id)?;
            crate::writable(p)?;
            if config.role == crate::SessionRole::Learner && !p.metadata.learner_exposed {
                return Err(fail(
                    DiagnosticCode::Permission,
                    "checkpoint parameter is not exposed to learner",
                ));
            }
            p.validate_value(value)?;
        }
        Ok(())
    }
    pub(crate) fn storage_bytes(&self, limit: usize) -> Result<usize> {
        let bytes = self.allocated_bytes()?.max(bounded_size(self, limit)?);
        if bytes > limit {
            return Err(fail(
                DiagnosticCode::Limit,
                "snapshot retained allocation exceeds budget",
            ));
        }
        Ok(bytes)
    }
    pub(crate) fn apply(&mut self, event: &SessionEvent) -> Result<()> {
        if event.version != SESSION_VERSION
            || event.session != self.session
            || event.epoch != self.epoch
            || event.document != self.document
            || event.document_revision != self.document_revision
            || self.sequence.checked_add(1) != Some(event.sequence)
            || self.revision.checked_add(1) != Some(event.revision)
        {
            return Err(Diagnostic::new(
                DiagnosticCode::Replay,
                "events",
                "foreign identity/version or event sequence/revision gap",
            ));
        }
        match &event.kind {
            EventKind::Seek { time } => {
                if event.time != *time {
                    return Err(Diagnostic::new(
                        DiagnosticCode::Replay,
                        "events/time",
                        "seek timestamp mismatch",
                    ));
                }
                self.time = *time;
            }
            EventKind::LiveOverride { parameter, value } => {
                self.overrides.insert(parameter.clone(), value.clone());
                self.playback_intent = false;
            }
            EventKind::LiveOverrides { values } | EventKind::ResetDefaults { values } => {
                self.overrides.extend(values.clone());
                self.playback_intent = false;
            }
            EventKind::ReturnToGuide { parameters } => {
                for p in parameters {
                    self.overrides.remove(p);
                }
            }
            EventKind::PatchLiveOverrides { entries, playback } => {
                crate::commands::validate_patch_entries(entries)?;
                for (id, value) in entries {
                    match value {
                        Some(value) => {
                            self.overrides.insert(id.clone(), value.clone());
                        }
                        None => {
                            self.overrides.remove(id);
                        }
                    }
                }
                match playback {
                    crate::OverridePlaybackPolicy::Pause => self.playback_intent = false,
                    crate::OverridePlaybackPolicy::Preserve => {}
                    crate::OverridePlaybackPolicy::SetIntent(intent) => {
                        self.playback_intent = *intent
                    }
                }
            }
            EventKind::Playback { playing } => self.playback_intent = *playing,
            _ => {
                return Err(Diagnostic::new(
                    DiagnosticCode::Replay,
                    "events/kind",
                    "author/draft events require a checkpoint baseline",
                ));
            }
        }
        if event.time != self.time {
            return Err(Diagnostic::new(
                DiagnosticCode::Replay,
                "events/time",
                "event time mismatch",
            ));
        }
        self.sequence = event.sequence;
        self.revision = event.revision;
        Ok(())
    }
}

impl Session {
    pub fn export_snapshot(&self) -> Result<SessionCheckpoint> {
        self.idle()?;
        let c = self.checkpoint();
        c.storage_bytes(self.config.max_command_bytes)?;
        Ok(c)
    }
    pub fn export_events(&self) -> Result<SessionEnvelope> {
        self.idle()?;
        let envelope = SessionEnvelope {
            version: SESSION_VERSION,
            baseline: self.baseline.clone(),
            retained_start: self
                .replay_events
                .front()
                .map(|e| e.sequence)
                .unwrap_or(self.baseline.sequence),
            retained_end: self
                .replay_events
                .back()
                .map(|e| e.sequence)
                .unwrap_or(self.baseline.sequence),
            events: self.replay_events.iter().cloned().collect(),
        };
        envelope.validate_storage(self.config)?;
        Ok(envelope)
    }
    fn pristine(&self) -> Result<()> {
        if self.imported
            || self.revision() != 0
            || self.sequence != 0
            || !self.history.is_empty()
            || !self.replay_events.is_empty()
            || !self.events.is_empty()
            || self.transaction.is_some()
            || !self.overrides.is_empty()
            || self.playing
        {
            return Err(fail(
                DiagnosticCode::Replay,
                "restore/replay requires a pristine fresh runtime instance",
            ));
        }
        Ok(())
    }
    fn check_checkpoint(&self, c: &SessionCheckpoint) -> Result<()> {
        c.validate_for(self.document(), self.config, &self.identity, self.epoch)
    }
    pub fn restore_snapshot(&mut self, expected: u64, c: SessionCheckpoint) -> Result<()> {
        let c = c.into_current()?;
        if expected != self.revision() {
            return Err(fail(
                DiagnosticCode::StaleRevision,
                "stale restore session revision",
            ));
        }
        self.idle()?;
        self.check_checkpoint(&c)?;
        self.pristine()?;
        let mut candidate = self.candidate();
        let graph = candidate.graph.clone();
        let e = graph.evaluate(c.time, &c.overrides, None)?;
        candidate.epoch = c.epoch;
        candidate.sequence = c.sequence;
        candidate.overrides = c.overrides.clone();
        candidate.playing = false;
        candidate.playback_intent = c.playback_intent;
        candidate.publish(c.revision, c.time, &graph, e, false)?;
        candidate.checkpoint_baseline();
        candidate.imported = true;
        candidate.storage_budget()?;
        *self = candidate;
        Ok(())
    }
    /// Loads a document and retained event suffix together in a fresh, paused runtime.
    pub fn from_events(
        doc: ValidatedDocument,
        registry: sim_math::OperatorRegistry,
        config: crate::SessionConfig,
        identity: SessionId,
        epoch: u64,
        envelope: SessionEnvelope,
    ) -> Result<Self> {
        envelope.validate_storage(config)?;
        let envelope = envelope.into_current()?;
        let mut session = Self::from_checkpoint(
            doc,
            registry,
            config,
            identity,
            epoch,
            envelope.baseline.clone(),
        )?;
        session.validate_envelope(&envelope)?;
        session.apply_event_suffix(envelope)?;
        Ok(session)
    }
    fn validate_envelope(&self, envelope: &SessionEnvelope) -> Result<()> {
        envelope.validate_storage(self.config)?;
        self.check_checkpoint(&envelope.baseline)?;
        let start = envelope
            .events
            .first()
            .map(|e| e.sequence)
            .unwrap_or(envelope.baseline.sequence);
        let end = envelope
            .events
            .last()
            .map(|e| e.sequence)
            .unwrap_or(envelope.baseline.sequence);
        if envelope.retained_start != start || envelope.retained_end != end {
            return Err(fail(
                DiagnosticCode::Replay,
                "retained event range mismatch",
            ));
        }
        Ok(())
    }
    pub fn replay(&mut self, expected: u64, envelope: SessionEnvelope) -> Result<()> {
        envelope.validate_storage(self.config)?;
        let envelope = envelope.into_current()?;
        if expected != self.revision() {
            return Err(fail(
                DiagnosticCode::StaleRevision,
                "stale replay session revision",
            ));
        }
        self.idle()?;
        self.pristine()?;
        self.validate_envelope(&envelope)?;
        let mut candidate = self.candidate();
        candidate.restore_snapshot(candidate.revision(), envelope.baseline.clone())?;
        candidate.apply_event_suffix(envelope)?;
        *self = candidate;
        Ok(())
    }
    fn apply_event_suffix(&mut self, envelope: SessionEnvelope) -> Result<()> {
        let mut state = envelope.baseline.clone();
        for event in &envelope.events {
            state.apply(event)?;
            // Check every intermediate command, including learner permissions and final constraints.
            let kind = match &event.kind {
                EventKind::Seek { time } => CommandKind::Seek { time: *time },
                EventKind::LiveOverride { parameter, value } => CommandKind::SetLiveOverride {
                    parameter: parameter.clone(),
                    value: value.clone(),
                },
                EventKind::LiveOverrides { values } => CommandKind::SetLiveOverrides {
                    values: values.clone(),
                },
                EventKind::ResetDefaults { values } => {
                    for (id, value) in values {
                        if self.document().parameter(id)?.default != *value {
                            return Err(fail(
                                DiagnosticCode::Replay,
                                "reset defaults event payload differs from authored default",
                            ));
                        }
                    }
                    CommandKind::ResetParameters {
                        expected_document_revision: event.document_revision,
                        parameters: values.keys().cloned().collect(),
                    }
                }
                EventKind::ReturnToGuide { parameters } => CommandKind::ReturnToGuide {
                    parameters: parameters.clone(),
                },
                EventKind::PatchLiveOverrides { entries, playback } => {
                    CommandKind::PatchLiveOverrides {
                        entries: entries.clone(),
                        playback: *playback,
                    }
                }
                EventKind::Playback { playing } => CommandKind::SetPlaying { playing: *playing },
                _ => return Err(fail(DiagnosticCode::Replay, "unsupported author event")),
            };
            self.dispatch(Command {
                session: self.identity.clone(),
                generation: self.generation.clone(),
                document: self.document().document().id.clone(),
                epoch: self.epoch,
                expected_document_revision: self.document().document().revision,
                expected_revision: self.revision(),
                kind,
            })?;
            if self.sequence != event.sequence
                || self.revision() != event.revision
                || self.checkpoint() != state
            {
                return Err(fail(DiagnosticCode::Replay, "replay state mismatch"));
            }
        }
        self.playing = false;
        self.playback_intent = state.playback_intent;
        self.baseline = envelope.baseline;
        self.replay_events = envelope.events.into();
        self.storage_budget()?;
        Ok(())
    }
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8], limits: DecodeLimits) -> Result<T> {
    validate_json_budget(bytes, limits)?;
    serde_json::from_slice(bytes).map_err(|e| {
        Diagnostic::new(
            DiagnosticCode::Schema,
            format!("session_json/line/{}/column/{}", e.line(), e.column()),
            e.to_string(),
        )
    })
}
impl SessionCheckpoint {
    pub fn decode(bytes: &[u8], limits: DecodeLimits) -> Result<Self> {
        let state: Self = decode::<Self>(bytes, limits)?.into_current()?;
        state.validate_budget(limits)?;
        Ok(state)
    }
    fn validate_budget(&self, limits: DecodeLimits) -> Result<()> {
        if self.version != SESSION_VERSION {
            return Err(Diagnostic::new(
                DiagnosticCode::Replay,
                "snapshot/version",
                "unsupported session version",
            ));
        }
        if self.allocated_bytes()? > limits.max_bytes {
            return Err(fail(
                DiagnosticCode::Limit,
                "decoded checkpoint allocation exceeds byte limit",
            ));
        }
        if self.overrides.len() > limits.max_nodes {
            return Err(Diagnostic::new(
                DiagnosticCode::Limit,
                "snapshot/overrides",
                "override count exceeds limit",
            ));
        }
        let count = self.overrides.values().try_fold(0usize, |sum, v| {
            v.validate()?;
            sum.checked_add(v.element_count()).ok_or_else(|| {
                Diagnostic::new(
                    DiagnosticCode::Limit,
                    "snapshot/overrides",
                    "element overflow",
                )
            })
        })?;
        if count > limits.max_elements {
            return Err(Diagnostic::new(
                DiagnosticCode::Limit,
                "snapshot/overrides",
                "element budget exceeded",
            ));
        }
        Ok(())
    }
    pub fn encode(&self, limit: usize) -> Result<Vec<u8>> {
        if !matches!(self.version, 1 | SESSION_VERSION) {
            return Err(fail(
                DiagnosticCode::Replay,
                "unsupported checkpoint version",
            ));
        }
        self.storage_bytes(limit)?;
        serde_json::to_vec(self)
            .map_err(|e| Diagnostic::new(DiagnosticCode::Schema, "snapshot/encode", e.to_string()))
    }
}
impl SessionEnvelope {
    fn validate_wire_versions(&self) -> Result<()> {
        if !matches!(self.version, 1 | SESSION_VERSION)
            || self.baseline.version != self.version
            || self.events.iter().any(|e| e.version != self.version)
        {
            return Err(fail(
                DiagnosticCode::Replay,
                "unsupported or mixed session wire versions",
            ));
        }
        if self.version == 1
            && self
                .events
                .iter()
                .any(|e| matches!(e.kind, EventKind::PatchLiveOverrides { .. }))
        {
            return Err(fail(
                DiagnosticCode::Replay,
                "atomic override patch is not a V1 event",
            ));
        }
        Ok(())
    }
    fn into_current(mut self) -> Result<Self> {
        self.validate_wire_versions()?;
        self.version = SESSION_VERSION;
        self.baseline.version = SESSION_VERSION;
        for event in &mut self.events {
            event.version = SESSION_VERSION;
        }
        Ok(self)
    }
    fn validate_storage(&self, config: crate::SessionConfig) -> Result<()> {
        if !matches!(self.version, 1 | SESSION_VERSION) {
            return Err(fail(DiagnosticCode::Replay, "future session envelope"));
        }
        let total = config
            .max_event_bytes
            .checked_add(config.max_command_bytes)
            .ok_or_else(|| fail(DiagnosticCode::Limit, "envelope budget overflow"))?;
        bounded_size(self, total)?;
        if self.allocated_bytes()? > total {
            return Err(fail(
                DiagnosticCode::Limit,
                "event envelope retained allocation exceeds budget",
            ));
        }
        if self.events.len() > config.event_capacity {
            return Err(fail(
                DiagnosticCode::Limit,
                "replay event count exceeds capacity",
            ));
        }
        Ok(())
    }
    pub fn decode(bytes: &[u8], limits: DecodeLimits) -> Result<Self> {
        let envelope: Self = decode::<Self>(bytes, limits)?.into_current()?;
        if envelope.version != SESSION_VERSION {
            return Err(Diagnostic::new(
                DiagnosticCode::Replay,
                "envelope/version",
                "unsupported session version",
            ));
        }
        envelope.baseline.validate_budget(limits)?;
        let mut elements = envelope
            .baseline
            .overrides
            .values()
            .try_fold(0usize, |n, v| {
                n.checked_add(v.element_count())
                    .ok_or_else(|| fail(DiagnosticCode::Limit, "envelope element overflow"))
            })?;
        for event in &envelope.events {
            if let EventKind::PatchLiveOverrides { entries, .. } = &event.kind {
                if entries.len() > limits.max_nodes {
                    return Err(fail(
                        DiagnosticCode::Limit,
                        "patch event entry limit exceeded",
                    ));
                }
                for (_, value) in entries {
                    if let Some(value) = value {
                        value.validate()?;
                        elements =
                            elements.checked_add(value.element_count()).ok_or_else(|| {
                                fail(DiagnosticCode::Limit, "patch event element overflow")
                            })?;
                    }
                }
            }
        }
        if elements > limits.max_elements {
            return Err(fail(
                DiagnosticCode::Limit,
                "envelope element limit exceeded",
            ));
        }
        if envelope.allocated_bytes()? > limits.max_bytes {
            return Err(fail(
                DiagnosticCode::Limit,
                "decoded envelope allocation exceeds byte limit",
            ));
        }
        if envelope.events.len() > limits.max_nodes {
            return Err(Diagnostic::new(
                DiagnosticCode::Limit,
                "envelope/events",
                "event count exceeds limit",
            ));
        }
        Ok(envelope)
    }
    pub fn encode(&self, limit: usize) -> Result<Vec<u8>> {
        self.validate_wire_versions()?;
        if self.allocated_bytes()? > limit {
            return Err(fail(
                DiagnosticCode::Limit,
                "event envelope allocation exceeds byte limit",
            ));
        }
        bounded_size(self, limit)?;
        serde_json::to_vec(self)
            .map_err(|e| Diagnostic::new(DiagnosticCode::Schema, "events/encode", e.to_string()))
    }
}

fn add_bytes(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or_else(|| {
        Diagnostic::new(
            DiagnosticCode::Limit,
            "session/storage",
            "allocated byte count overflow",
        )
    })
}
fn map_bytes(values: &BTreeMap<ParameterId, Value>) -> Result<usize> {
    let mut bytes = values
        .len()
        .checked_mul(std::mem::size_of::<(ParameterId, Value)>() + 64)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticCode::Limit,
                "session/storage",
                "map storage overflow",
            )
        })?;
    for (id, value) in values {
        bytes = add_bytes(bytes, id.as_str().len())?;
        bytes = add_bytes(bytes, value.heap_bytes()?)?;
    }
    Ok(bytes)
}
impl SessionCheckpoint {
    pub fn allocated_bytes(&self) -> Result<usize> {
        add_bytes(std::mem::size_of::<Self>(), map_bytes(&self.overrides)?).and_then(|n| {
            add_bytes(
                n,
                self.session.as_str().len() + self.document.as_str().len(),
            )
        })
    }
}
impl SessionEvent {
    pub fn allocated_bytes(&self) -> Result<usize> {
        let heap = match &self.kind {
            EventKind::PatchLiveOverrides { entries, .. } => {
                crate::commands::patch_entries_bytes(entries)?
            }
            EventKind::LiveOverride { value, .. } => value.heap_bytes()?,
            EventKind::LiveOverrides { values }
            | EventKind::ResetDefaults { values }
            | EventKind::TransactionUpdate { values, .. } => map_bytes(values)?,
            EventKind::ReturnToGuide { parameters } => parameters
                .capacity()
                .checked_mul(std::mem::size_of::<ParameterId>())
                .ok_or_else(|| {
                    Diagnostic::new(
                        DiagnosticCode::Limit,
                        "events/storage",
                        "ID capacity overflow",
                    )
                })?,
            EventKind::AuthorRevision { document, .. } => document.allocated_bytes()?,
            EventKind::TransactionDocumentUpdate { edits, .. } => {
                crate::commands::edits_heap_bytes(edits)?
            }
            _ => 0,
        };
        add_bytes(std::mem::size_of::<Self>(), heap).and_then(|n| {
            add_bytes(
                n,
                self.session.as_str().len() + self.document.as_str().len(),
            )
        })
    }
}
impl SessionEnvelope {
    pub fn allocated_bytes(&self) -> Result<usize> {
        let mut n = add_bytes(
            self.baseline.allocated_bytes()?,
            self.events
                .capacity()
                .checked_mul(std::mem::size_of::<SessionEvent>())
                .ok_or_else(|| {
                    Diagnostic::new(
                        DiagnosticCode::Limit,
                        "events/storage",
                        "event capacity overflow",
                    )
                })?,
        )?;
        for event in &self.events {
            n = add_bytes(n, event.allocated_bytes()?)?;
        }
        Ok(n)
    }
}

impl EventKind {
    pub(crate) fn compact_storage(mut self) -> Self {
        match &mut self {
            Self::PatchLiveOverrides { entries, .. } => {
                for (_, value) in entries.iter_mut() {
                    if let Some(v) = value {
                        *v = v.compact_storage();
                    }
                }
                *entries = std::mem::take(entries).into_boxed_slice().into_vec();
            }
            Self::LiveOverride { value, .. } => *value = value.compact_storage(),
            Self::LiveOverrides { values } | Self::ResetDefaults { values } => {
                for value in values.values_mut() {
                    *value = value.compact_storage();
                }
            }
            _ => {}
        }
        self
    }
}
