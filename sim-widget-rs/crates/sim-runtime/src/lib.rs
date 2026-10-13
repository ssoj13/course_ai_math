#![forbid(unsafe_code)]
mod commands;
mod history;
mod presentation;
mod session_state;
mod track;
mod view_state;
pub use commands::*;
use history::HistoryEntry;
pub use presentation::*;
use serde::{Deserialize, Serialize};
pub use session_state::*;
pub use sim_math::InputOrigin;
use sim_math::{CompiledGraph, Evaluation, OperatorRegistry, Provenance};
use sim_model::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
pub use track::ScalarTrackAdapter;
pub use view_state::*;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeSet {
    pub changed_values: BTreeSet<ValueId>,
    pub removed_values: BTreeSet<ValueId>,
    pub provenance_only: BTreeSet<ValueId>,
    pub changed_properties: BTreeSet<PropertyAddress>,
    pub removed_properties: BTreeSet<PropertyAddress>,
    pub changed_objects: BTreeSet<ObjectId>,
    pub removed_objects: BTreeSet<ObjectId>,
    pub structural: bool,
}
impl ChangeSet {
    fn allocated_bytes(&self) -> Result<usize> {
        let mut bytes = std::mem::size_of::<Self>();
        let mut add = |n: usize| -> Result<()> {
            bytes = bytes
                .checked_add(n)
                .ok_or_else(|| fail(DiagnosticCode::Limit, "change set storage overflow"))?;
            Ok(())
        };
        for set in [
            &self.changed_values,
            &self.removed_values,
            &self.provenance_only,
        ] {
            for id in set {
                add(std::mem::size_of::<ValueId>() + 64 + id.as_str().len())?;
            }
        }
        for set in [&self.changed_objects, &self.removed_objects] {
            for id in set {
                add(std::mem::size_of::<ObjectId>() + 64 + id.as_str().len())?;
            }
        }
        for set in [&self.changed_properties, &self.removed_properties] {
            for id in set {
                add(std::mem::size_of::<PropertyAddress>()
                    + 64
                    + id.object.as_str().len()
                    + id.property.as_str().len())?;
            }
        }
        Ok(bytes)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedProperty {
    pub source: ValueId,
    pub port: Port,
    pub value: Value,
}
#[derive(Clone, Debug, PartialEq)]
pub struct FrameSnapshot {
    revision: u64,
    document_revision: u64,
    time: TimeTicks,
    document: DocumentId,
    session: SessionId,
    epoch: u64,
    authority: Authority,
    generation: GenerationId,
    evaluation: Arc<Evaluation>,
    properties: Arc<BTreeMap<PropertyAddress, ResolvedProperty>>,
    changes: Arc<ChangeSet>,
    contract: Arc<ValidatedDocument>,
}
impl FrameSnapshot {
    pub fn objects(&self) -> &[SceneObject] {
        &self.contract.document().objects
    }
    pub fn generation(&self) -> &GenerationId {
        &self.generation
    }
    pub fn command(&self, kind: CommandKind) -> Command {
        Command {
            session: self.session.clone(),
            generation: self.generation.clone(),
            document: self.document.clone(),
            epoch: self.epoch,
            expected_document_revision: self.document_revision,
            expected_revision: self.revision,
            kind,
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn document_revision(&self) -> u64 {
        self.document_revision
    }
    pub fn document_id(&self) -> &DocumentId {
        &self.document
    }
    pub fn session_id(&self) -> &SessionId {
        &self.session
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn authority(&self) -> &Authority {
        &self.authority
    }
    pub fn time(&self) -> TimeTicks {
        self.time
    }
    pub fn truth(&self) -> &BTreeMap<ValueId, Value> {
        self.evaluation.truth()
    }
    pub fn provenance(&self) -> &BTreeMap<ValueId, Provenance> {
        self.evaluation.provenance()
    }
    pub fn properties(&self) -> &BTreeMap<PropertyAddress, ResolvedProperty> {
        &self.properties
    }
    pub fn changes(&self) -> &ChangeSet {
        &self.changes
    }
    pub fn evaluated_operations(&self) -> &[OperationId] {
        self.evaluation.evaluated_operations()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionRole {
    Author,
    Learner,
}
#[derive(Clone, Copy, Debug)]
pub struct SessionConfig {
    pub history_capacity: usize,
    pub event_capacity: usize,
    pub decode_limits: DecodeLimits,
    pub role: SessionRole,
    pub max_history_bytes: usize,
    pub max_event_bytes: usize,
    pub max_command_bytes: usize,
    pub max_batch_edits: usize,
    pub max_candidate_bytes: usize,
    pub max_frame_bytes: usize,
}
impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            history_capacity: 128,
            event_capacity: 1024,
            decode_limits: DecodeLimits::default(),
            role: SessionRole::Author,
            max_history_bytes: 16 * 1048576,
            max_event_bytes: 4 * 1048576,
            max_command_bytes: 1048576,
            max_batch_edits: 4096,
            max_candidate_bytes: 64 * 1048576,
            max_frame_bytes: 16 * 1048576,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TransactionId(u64);
impl TransactionId {
    pub fn sequence(self) -> u64 {
        self.0
    }
}
#[derive(Clone, Debug)]
struct RuntimeGraph {
    compiled: CompiledGraph,
    tracks: Arc<Vec<ScalarTrackAdapter>>,
}
impl RuntimeGraph {
    fn shares_contract(&self, other: &Self) -> bool {
        Arc::ptr_eq(
            &self.compiled.document_snapshot(),
            &other.compiled.document_snapshot(),
        )
    }
    fn cache_bytes(&self) -> Result<usize> {
        let mut n = self
            .compiled
            .cache_bytes()?
            .checked_add(std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>())
            .and_then(|n| {
                self.tracks
                    .capacity()
                    .checked_mul(std::mem::size_of::<ScalarTrackAdapter>())
                    .and_then(|m| n.checked_add(m))
            })
            .ok_or_else(|| fail(DiagnosticCode::Limit, "runtime cache storage overflow"))?;
        for track in self.tracks.iter() {
            n = n
                .checked_add(track.cache_bytes()?)
                .ok_or_else(|| fail(DiagnosticCode::Limit, "adapter cache storage overflow"))?;
        }
        Ok(n)
    }
    fn compile(registry: &OperatorRegistry, doc: ValidatedDocument) -> Result<Self> {
        let tracks = doc
            .document()
            .tracks
            .iter()
            .cloned()
            .map(ScalarTrackAdapter::new)
            .collect::<Result<_>>()?;
        Ok(Self {
            compiled: registry.compile(doc)?,
            tracks: Arc::new(tracks),
        })
    }
    fn document(&self) -> &ValidatedDocument {
        self.compiled.document()
    }
    fn evaluate(
        &self,
        time: TimeTicks,
        overrides: &BTreeMap<ParameterId, Value>,
        previous: Option<&Evaluation>,
    ) -> Result<Evaluation> {
        let doc = self.document();
        let mut inputs: BTreeMap<_, _> = doc
            .document()
            .parameters
            .iter()
            .map(|p| (p.id.clone(), p.default.clone()))
            .collect();
        let mut origins: BTreeMap<_, _> = inputs
            .keys()
            .map(|id| (id.clone(), InputOrigin::Default))
            .collect();
        for track in self.tracks.iter() {
            inputs.insert(track.parameter().clone(), track.sample(time)?);
            origins.insert(track.parameter().clone(), InputOrigin::Track);
        }
        for (id, value) in overrides {
            let p = doc.parameter(id)?;
            writable(p)?;
            inputs.insert(id.clone(), value.clone());
            origins.insert(id.clone(), InputOrigin::Override);
        }
        match previous {
            Some(prev) => self.compiled.evaluate_incremental(&inputs, &origins, prev),
            None => self.compiled.evaluate(&inputs, &origins),
        }
    }
}
#[derive(Clone, Debug)]
struct Transaction {
    id: TransactionId,
    draft: RuntimeGraph,
}
#[derive(Clone, Debug)]
pub struct DispatchResult {
    pub revision: u64,
    pub document_revision: u64,
    pub transaction: Option<TransactionId>,
    pub changes: ChangeSet,
    pub snapshot: FrameSnapshot,
}

#[derive(Serialize)]
struct PatchCheckpointView<'a> {
    version: u32,
    session: &'a SessionId,
    epoch: u64,
    document: &'a DocumentId,
    document_revision: u64,
    sequence: u64,
    revision: u64,
    time: TimeTicks,
    overrides: &'a BTreeMap<&'a ParameterId, &'a Value>,
    playback_intent: bool,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PatchEventKindView<'a> {
    PatchLiveOverrides {
        entries: &'a [(ParameterId, Option<Value>)],
        playback: OverridePlaybackPolicy,
    },
}
#[derive(Serialize)]
struct PatchEventView<'a> {
    version: u32,
    session: &'a SessionId,
    epoch: u64,
    document: &'a DocumentId,
    document_revision: u64,
    sequence: u64,
    revision: u64,
    time: TimeTicks,
    kind: PatchEventKindView<'a>,
}
/// Immutable publication is the only mathematical read path. Commands commit candidate state atomically.
#[derive(Debug)]
pub struct Session {
    graph: RuntimeGraph,
    registry: OperatorRegistry,
    config: SessionConfig,
    frame: FrameSnapshot,
    overrides: BTreeMap<ParameterId, Value>,
    playing: bool,
    history: VecDeque<HistoryEntry>,
    cursor: usize,
    replay_events: VecDeque<SessionEvent>,
    events: VecDeque<SessionEvent>,
    transaction: Option<Transaction>,
    identity: SessionId,
    epoch: u64,
    sequence: u64,
    baseline: SessionCheckpoint,
    playback_intent: bool,
    generation: GenerationId,
    imported: bool,
}
fn fail(code: DiagnosticCode, message: &str) -> Diagnostic {
    Diagnostic::new(code, "session", message)
}
fn increment(n: u64) -> Result<u64> {
    n.checked_add(1)
        .ok_or_else(|| fail(DiagnosticCode::Overflow, "revision/sequence overflow"))
}
fn writable(p: &Parameter) -> Result<()> {
    if p.readonly || p.metadata.locked {
        Err(Diagnostic::new(
            DiagnosticCode::Permission,
            format!("parameters/{}", p.id.as_str()),
            "parameter is readonly or locked",
        ))
    } else {
        Ok(())
    }
}
fn resolve(
    graph: &RuntimeGraph,
    evaluation: &Evaluation,
) -> Result<BTreeMap<PropertyAddress, ResolvedProperty>> {
    let mut properties = BTreeMap::new();
    for object in &graph.document().document().objects {
        for (id, source) in &object.bindings {
            let value = evaluation
                .truth()
                .get(source)
                .ok_or_else(|| fail(DiagnosticCode::Reference, "resolved binding source absent"))?;
            let port = object.properties.get(id).ok_or_else(|| {
                fail(
                    DiagnosticCode::Reference,
                    "resolved binding property absent",
                )
            })?;
            properties.insert(
                PropertyAddress {
                    object: object.id.clone(),
                    property: id.clone(),
                },
                ResolvedProperty {
                    source: source.clone(),
                    port: port.clone(),
                    value: value.clone(),
                },
            );
        }
    }
    Ok(properties)
}
impl Session {
    pub fn new(
        doc: ValidatedDocument,
        registry: OperatorRegistry,
        config: SessionConfig,
    ) -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| fail(DiagnosticCode::Overflow, "session identity overflow"))?;
        Self::new_with_identity(
            doc,
            registry,
            config,
            SessionId::new(format!("session-{id}"))?,
        )
    }
    pub fn new_with_identity(
        doc: ValidatedDocument,
        registry: OperatorRegistry,
        config: SessionConfig,
        identity: SessionId,
    ) -> Result<Self> {
        Self::new_with_epoch(doc, registry, config, identity, 0)
    }
    pub fn new_with_epoch(
        doc: ValidatedDocument,
        registry: OperatorRegistry,
        config: SessionConfig,
        identity: SessionId,
        epoch: u64,
    ) -> Result<Self> {
        Self::initialize(doc, registry, config, identity, epoch, None)
    }
    /// Validates document + saved effective inputs together before the first truth publication.
    pub fn from_checkpoint(
        doc: ValidatedDocument,
        registry: OperatorRegistry,
        config: SessionConfig,
        identity: SessionId,
        epoch: u64,
        checkpoint: SessionCheckpoint,
    ) -> Result<Self> {
        Self::initialize(doc, registry, config, identity, epoch, Some(checkpoint))
    }
    fn initialize(
        doc: ValidatedDocument,
        registry: OperatorRegistry,
        config: SessionConfig,
        identity: SessionId,
        epoch: u64,
        checkpoint: Option<SessionCheckpoint>,
    ) -> Result<Self> {
        static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
        let next = NEXT_GENERATION
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| fail(DiagnosticCode::Overflow, "runtime generation overflow"))?;
        let generation = GenerationId::new(format!("generation-{next}"))?;
        if config.history_capacity > 65536
            || config.event_capacity > 1048576
            || config.max_batch_edits > 65536
        {
            return Err(fail(
                DiagnosticCode::Limit,
                "session capacity exceeds limit",
            ));
        }
        Self::construction_budget(&doc, &registry, config)?;
        let doc = doc.document().clone().validate(config.decode_limits)?;
        Self::graph_budget(&doc, config)?;

        let imported = checkpoint.is_some();
        let checkpoint = checkpoint.unwrap_or_else(|| SessionCheckpoint {
            version: SESSION_VERSION,
            session: identity.clone(),
            epoch,
            document: doc.document().id.clone(),
            document_revision: doc.document().revision,
            sequence: 0,
            revision: 0,
            time: TimeTicks::ZERO,
            overrides: BTreeMap::new(),
            playback_intent: false,
        });
        let checkpoint = checkpoint.into_current()?;
        checkpoint.validate_for(&doc, config, &identity, epoch)?;
        let overrides = checkpoint
            .overrides
            .iter()
            .map(|(id, value)| (id.clone(), value.compact_storage()))
            .collect::<BTreeMap<_, _>>();
        let graph = RuntimeGraph::compile(&registry, doc)?;
        let cached = graph
            .cache_bytes()?
            .checked_add(registry.allocated_bytes()?)
            .ok_or_else(|| fail(DiagnosticCode::Limit, "initial cache budget overflow"))?;
        if cached > config.max_candidate_bytes {
            return Err(fail(
                DiagnosticCode::Limit,
                "compiled initial caches exceed candidate budget",
            ));
        }
        let evaluation = graph.evaluate(checkpoint.time, &overrides, None)?;
        bounded_size(
            &(evaluation.truth(), evaluation.provenance()),
            config.max_frame_bytes,
        )?;
        let properties = resolve(&graph, &evaluation)?;
        Self::frame_bytes(&evaluation, &properties, config.max_frame_bytes)?;
        let raw = graph.document().document();
        let frame = FrameSnapshot {
            revision: checkpoint.revision,
            document_revision: raw.revision,
            time: checkpoint.time,
            document: raw.id.clone(),
            session: identity.clone(),
            epoch,
            authority: raw.authority.clone(),
            generation: generation.clone(),
            evaluation: Arc::new(evaluation),
            properties: Arc::new(properties),
            changes: Arc::new(ChangeSet::default()),
            contract: graph.compiled.document_snapshot(),
        };
        let baseline = SessionCheckpoint {
            overrides: overrides.clone(),
            ..checkpoint.clone()
        };
        let s = Self {
            graph,
            registry,
            config,
            frame,
            overrides,
            playing: false,
            history: VecDeque::new(),
            cursor: 0,
            replay_events: VecDeque::new(),
            events: VecDeque::new(),
            transaction: None,
            identity,
            epoch,
            sequence: checkpoint.sequence,
            baseline,
            playback_intent: checkpoint.playback_intent,
            generation,
            imported,
        };
        s.storage_budget()?;
        Ok(s)
    }
    fn construction_budget(
        doc: &ValidatedDocument,
        registry: &OperatorRegistry,
        config: SessionConfig,
    ) -> Result<()> {
        doc.document()
            .allocated_bytes_bounded(config.decode_limits.max_bytes)?;
        let bytes = doc
            .allocated_bytes()?
            .checked_add(registry.allocated_bytes()?)
            .ok_or_else(|| fail(DiagnosticCode::Limit, "construction storage overflow"))?;
        if bytes > config.max_candidate_bytes {
            return Err(fail(
                DiagnosticCode::Limit,
                "immutable document/registry exceeds candidate budget",
            ));
        }
        Ok(())
    }
    fn graph_budget(doc: &ValidatedDocument, config: SessionConfig) -> Result<()> {
        let raw = doc.document();
        let input = raw
            .parameters
            .iter()
            .map(|p| p.default.element_count())
            .chain(raw.constants.iter().map(|c| c.value.element_count()));
        let outputs = raw.operations.iter().map(|o| match o.output_port.shape {
            Shape::Vector { len } => len,
            Shape::Matrix { rows, cols } => rows.saturating_mul(cols),
            Shape::Color { .. } => 4,
            _ => 1,
        });
        let n = input
            .chain(outputs)
            .try_fold(0usize, |a, b| a.checked_add(b))
            .ok_or_else(|| fail(DiagnosticCode::Limit, "candidate element overflow"))?;
        // Reserve three scalar buffers plus per-node publication/dependency overhead before evaluation.
        let bytes = n
            .checked_mul(24)
            .and_then(|n| {
                raw.operations
                    .len()
                    .checked_mul(512)
                    .and_then(|m| n.checked_add(m))
            })
            .ok_or_else(|| fail(DiagnosticCode::Limit, "candidate bytes overflow"))?;
        if bytes > config.max_frame_bytes || bytes > config.max_candidate_bytes {
            return Err(fail(
                DiagnosticCode::Limit,
                "candidate/frame allocation budget exceeded",
            ));
        }
        Ok(())
    }
    pub fn document(&self) -> &ValidatedDocument {
        self.graph.document()
    }
    pub fn snapshot(&self) -> FrameSnapshot {
        self.frame.clone()
    }
    pub fn revision(&self) -> u64 {
        self.frame.revision
    }
    pub fn playback_intent(&self) -> bool {
        self.playback_intent
    }
    pub fn is_playing(&self) -> bool {
        self.playing
    }
    pub fn history_len(&self) -> usize {
        self.history.len()
    }
    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }
    pub fn can_redo(&self) -> bool {
        self.cursor < self.history.len()
    }
    /// Bounded audit observations; export_events provides only the replayable current-epoch suffix.
    pub fn events(&self) -> &VecDeque<SessionEvent> {
        &self.events
    }
    pub fn live_overrides(&self) -> &BTreeMap<ParameterId, Value> {
        &self.overrides
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn checkpoint(&self) -> SessionCheckpoint {
        SessionCheckpoint {
            version: SESSION_VERSION,
            session: self.identity.clone(),
            epoch: self.epoch,
            document: self.document().document().id.clone(),
            document_revision: self.document().document().revision,
            sequence: self.sequence,
            revision: self.revision(),
            time: self.frame.time,
            overrides: self.overrides.clone(),
            playback_intent: self.playback_intent,
        }
    }
    pub fn full_evaluation(&self) -> Result<Evaluation> {
        let g = self
            .transaction
            .as_ref()
            .map(|t| &t.draft)
            .unwrap_or(&self.graph);
        g.evaluate(self.frame.time, &self.overrides, None)
    }

    fn event_bytes(events: &VecDeque<SessionEvent>, limit: usize) -> Result<usize> {
        let mut bytes = events
            .capacity()
            .checked_mul(std::mem::size_of::<SessionEvent>())
            .ok_or_else(|| fail(DiagnosticCode::Limit, "event buffer capacity overflow"))?;
        for e in events {
            bytes = bytes
                .checked_add(e.allocated_bytes()?)
                .filter(|n| *n <= limit)
                .ok_or_else(|| {
                    fail(
                        DiagnosticCode::Limit,
                        "event allocation byte budget exceeded",
                    )
                })?;
        }
        if bytes > limit {
            return Err(fail(
                DiagnosticCode::Limit,
                "event buffer exceeds allocation budget",
            ));
        }
        Ok(bytes.max(bounded_size(events, limit)?))
    }
    fn history_bytes(&self) -> Result<usize> {
        let mut bytes = self
            .history
            .capacity()
            .checked_mul(std::mem::size_of::<HistoryEntry>())
            .ok_or_else(|| fail(DiagnosticCode::Limit, "history buffer overflow"))?;
        for e in &self.history {
            bytes = bytes
                .checked_add(e.bytes(self.config.max_history_bytes)?)
                .ok_or_else(|| fail(DiagnosticCode::Limit, "history byte overflow"))?;
        }
        Ok(bytes)
    }

    fn frame_bytes(
        evaluation: &Evaluation,
        properties: &BTreeMap<PropertyAddress, ResolvedProperty>,
        limit: usize,
    ) -> Result<usize> {
        fn plus(a: usize, b: usize, limit: usize) -> Result<usize> {
            a.checked_add(b).filter(|n| *n <= limit).ok_or_else(|| {
                fail(
                    DiagnosticCode::Limit,
                    "frame retained allocation exceeds budget",
                )
            })
        }
        let mut bytes = evaluation
            .truth()
            .len()
            .checked_mul(std::mem::size_of::<(ValueId, Value)>() + 64)
            .ok_or_else(|| fail(DiagnosticCode::Limit, "truth map size overflow"))?;
        bytes = plus(
            bytes,
            evaluation
                .provenance()
                .len()
                .checked_mul(std::mem::size_of::<(ValueId, Provenance)>() + 64)
                .ok_or_else(|| fail(DiagnosticCode::Limit, "provenance map size overflow"))?,
            limit,
        )?;
        for (id, value) in evaluation.truth() {
            bytes = plus(bytes, id.as_str().len(), limit)?;
            bytes = plus(bytes, value.heap_bytes()?, limit)?;
        }
        for (id, p) in evaluation.provenance() {
            for text in [
                p.parameter.as_ref().map(ParameterId::as_str),
                p.operation.as_ref().map(OperationId::as_str),
                p.operator.as_ref().map(OperatorId::as_str),
            ]
            .into_iter()
            .flatten()
            {
                bytes = plus(bytes, text.len(), limit)?;
            }
            for source in &p.source_refs {
                let text = match source {
                    SourceRef::Parameter(id) => id.as_str(),
                    SourceRef::Value(id) => id.as_str(),
                    SourceRef::Operation(id) => id.as_str(),
                    SourceRef::Object(id) => id.as_str(),
                    SourceRef::Asset(id) => id.as_str(),
                };
                bytes = plus(bytes, text.len(), limit)?;
            }
            bytes = plus(bytes, id.as_str().len(), limit)?;
            bytes = plus(bytes, p.result.heap_bytes()?, limit)?;
            bytes = plus(
                bytes,
                p.operands
                    .capacity()
                    .checked_mul(std::mem::size_of::<sim_math::Operand>())
                    .ok_or_else(|| fail(DiagnosticCode::Limit, "operand capacity overflow"))?,
                limit,
            )?;
            bytes = plus(
                bytes,
                p.source_refs
                    .capacity()
                    .checked_mul(std::mem::size_of::<SourceRef>())
                    .ok_or_else(|| fail(DiagnosticCode::Limit, "source capacity overflow"))?,
                limit,
            )?;
            bytes = plus(
                bytes,
                p.validated_constraints
                    .capacity()
                    .checked_mul(std::mem::size_of::<usize>())
                    .ok_or_else(|| fail(DiagnosticCode::Limit, "constraint capacity overflow"))?,
                limit,
            )?;
            for operand in &p.operands {
                bytes = plus(bytes, operand.id.as_str().len(), limit)?;
                bytes = plus(bytes, operand.value.heap_bytes()?, limit)?;
            }
        }
        bytes = plus(
            bytes,
            properties
                .len()
                .checked_mul(std::mem::size_of::<(PropertyAddress, ResolvedProperty)>() + 64)
                .ok_or_else(|| fail(DiagnosticCode::Limit, "property map capacity overflow"))?,
            limit,
        )?;
        for (id, p) in properties {
            bytes = plus(
                bytes,
                id.object.as_str().len() + id.property.as_str().len() + p.source.as_str().len(),
                limit,
            )?;
            bytes = plus(bytes, p.value.heap_bytes()?, limit)?;
        }
        for id in evaluation.evaluated_operations() {
            bytes = plus(
                bytes,
                2 * std::mem::size_of::<OperationId>() + id.as_str().len(),
                limit,
            )?;
        }
        Ok(bytes)
    }
    fn storage_budget(&self) -> Result<()> {
        self.storage_bytes().map(|_| ())
    }
    fn storage_bytes(&self) -> Result<usize> {
        if !self.graph.compiled.shares_registry(&self.registry) {
            return Err(fail(
                DiagnosticCode::Capability,
                "compiled registry ownership differs from session authority",
            ));
        }
        let mut immutable = self
            .registry
            .allocated_bytes()?
            .checked_add(self.graph.cache_bytes()?)
            .ok_or_else(|| fail(DiagnosticCode::Limit, "immutable cache storage overflow"))?;
        if let Some(tx) = &self.transaction {
            if !tx.draft.compiled.shares_registry(&self.registry) {
                return Err(fail(
                    DiagnosticCode::Capability,
                    "draft registry authority differs",
                ));
            }
            if !self.graph.shares_contract(&tx.draft) {
                immutable = immutable
                    .checked_add(tx.draft.cache_bytes()?)
                    .ok_or_else(|| fail(DiagnosticCode::Limit, "draft cache storage overflow"))?;
            }
        }
        let mut bytes = self.history_bytes()?;
        let event = Self::event_bytes(&self.replay_events, self.config.max_event_bytes)?
            .checked_add(Self::event_bytes(
                &self.events,
                self.config.max_event_bytes,
            )?)
            .ok_or_else(|| fail(DiagnosticCode::Limit, "event storage overflow"))?;
        let checkpoint = self
            .baseline
            .storage_bytes(self.config.max_command_bytes)?
            .checked_add(
                self.checkpoint()
                    .storage_bytes(self.config.max_command_bytes)?,
            )
            .ok_or_else(|| fail(DiagnosticCode::Limit, "checkpoint storage overflow"))?;
        let mut frame = bounded_size(
            &(self.frame.truth(), self.frame.provenance()),
            self.config.max_frame_bytes,
        )?;
        for property in self.frame.properties().iter() {
            frame = frame
                .checked_add(bounded_size(
                    &property,
                    self.config.max_frame_bytes.saturating_sub(frame),
                )?)
                .ok_or_else(|| fail(DiagnosticCode::Limit, "property frame overflow"))?;
        }
        frame = frame.max(Self::frame_bytes(
            &self.frame.evaluation,
            self.frame.properties(),
            self.config.max_frame_bytes,
        )?);
        frame = frame
            .checked_add(self.frame.changes().allocated_bytes()?)
            .filter(|n| *n <= self.config.max_frame_bytes)
            .ok_or_else(|| {
                fail(
                    DiagnosticCode::Limit,
                    "retained frame and changes exceed allocation budget",
                )
            })?;
        bytes = bytes
            .checked_add(event)
            .and_then(|n| n.checked_add(checkpoint))
            .and_then(|n| n.checked_add(frame))
            .and_then(|n| n.checked_mul(2))
            .ok_or_else(|| fail(DiagnosticCode::Limit, "candidate storage overflow"))?;
        bytes = bytes
            .checked_add(immutable)
            .ok_or_else(|| fail(DiagnosticCode::Limit, "candidate cache storage overflow"))?;
        if bytes > self.config.max_candidate_bytes {
            return Err(fail(
                DiagnosticCode::Limit,
                "candidate state exceeds byte budget",
            ));
        }
        Ok(bytes)
    }

    /// Admission covers the still-live source, candidate, compilation scratch, and new retention
    /// payloads. Deletions are intentionally not credited until the atomic publication succeeds.
    fn document_transition_budget(
        &self,
        kind: &CommandKind,
        retained: usize,
        command_bytes: usize,
    ) -> Result<()> {
        let (source, author_commit, new_history) = match kind {
            CommandKind::SetParameter { .. }
            | CommandKind::SetParameters { .. }
            | CommandKind::EditDocument { .. } => (&self.graph, true, true),
            CommandKind::UpdateTransaction { id, .. }
            | CommandKind::UpdateDocumentTransaction { id, .. } => {
                (&self.tx(*id)?.draft, false, false)
            }
            CommandKind::CommitTransaction { id } => (&self.tx(*id)?.draft, true, true),
            CommandKind::Undo | CommandKind::Redo => {
                let index = if matches!(kind, CommandKind::Undo) {
                    self.cursor
                        .checked_sub(1)
                        .ok_or_else(|| fail(DiagnosticCode::History, "nothing to undo"))?
                } else {
                    self.cursor
                };
                let entry = self
                    .history
                    .get(index)
                    .ok_or_else(|| fail(DiagnosticCode::History, "nothing to redo"))?;
                let target = if matches!(kind, CommandKind::Undo) {
                    &entry.before
                } else {
                    &entry.after
                };
                let prospective = target
                    .allocated_bytes()?
                    .checked_add(Self::evaluation_reserve(target)?)
                    .and_then(|n| n.checked_add(self.graph.cache_bytes().ok()?))
                    .ok_or_else(|| {
                        fail(DiagnosticCode::Limit, "history transition size overflow")
                    })?;
                return self.check_transition_reserve(
                    retained,
                    prospective,
                    command_bytes,
                    true,
                    false,
                );
            }
            _ => return Ok(()),
        };
        // Existing cache storage covers old nodes, indices, tracks and reverse edges. Incoming
        // payloads cover strings/data and grown container buffers; extra nodes/edges reserve the
        // validated and compiled indices before creating them.
        let edits = match kind {
            CommandKind::EditDocument { edits, .. }
            | CommandKind::UpdateDocumentTransaction { edits, .. } => edits.as_slice(),
            _ => &[],
        };
        let mut index_bytes = 0usize;
        let mut working = Self::evaluation_reserve(source.document())?;
        for edit in edits {
            index_bytes = index_bytes
                .checked_add(1024)
                .ok_or_else(|| fail(DiagnosticCode::Limit, "transition index overflow"))?;
            match edit {
                DocumentEdit::PutOperation { operation } => {
                    index_bytes = index_bytes
                        .checked_add(operation.inputs.len().checked_mul(256).ok_or_else(|| {
                            fail(DiagnosticCode::Limit, "transition edge overflow")
                        })?)
                        .ok_or_else(|| fail(DiagnosticCode::Limit, "transition index overflow"))?;
                    working = working
                        .checked_add(
                            Self::output_elements(&operation.output_port.shape)?
                                .checked_mul(24)
                                .and_then(|n| n.checked_add(512))
                                .ok_or_else(|| {
                                    fail(DiagnosticCode::Limit, "transition output overflow")
                                })?,
                        )
                        .ok_or_else(|| {
                            fail(DiagnosticCode::Limit, "transition working storage overflow")
                        })?;
                }
                DocumentEdit::PutParameter { parameter } => {
                    working = working
                        .checked_add(
                            parameter
                                .default
                                .element_count()
                                .checked_mul(24)
                                .ok_or_else(|| {
                                    fail(DiagnosticCode::Limit, "transition parameter overflow")
                                })?,
                        )
                        .ok_or_else(|| {
                            fail(DiagnosticCode::Limit, "transition working overflow")
                        })?;
                }
                DocumentEdit::PutConstant { constant } => {
                    working = working
                        .checked_add(constant.value.element_count().checked_mul(24).ok_or_else(
                            || fail(DiagnosticCode::Limit, "transition constant overflow"),
                        )?)
                        .ok_or_else(|| {
                            fail(DiagnosticCode::Limit, "transition working overflow")
                        })?;
                }
                _ => {}
            }
        }
        let prospective = source
            .cache_bytes()?
            .checked_add(
                command_bytes
                    .checked_mul(2)
                    .ok_or_else(|| fail(DiagnosticCode::Limit, "transition command overflow"))?,
            )
            .and_then(|n| n.checked_add(index_bytes))
            .and_then(|n| n.checked_add(working))
            .ok_or_else(|| fail(DiagnosticCode::Limit, "prospective contract overflow"))?;
        self.check_transition_reserve(
            retained,
            prospective,
            command_bytes,
            author_commit,
            new_history,
        )
    }
    fn output_elements(shape: &Shape) -> Result<usize> {
        match shape {
            Shape::Vector { len } => Ok(*len),
            Shape::Matrix { rows, cols } => rows
                .checked_mul(*cols)
                .ok_or_else(|| fail(DiagnosticCode::Limit, "transition shape overflow")),
            Shape::Color { .. } => Ok(4),
            _ => Ok(1),
        }
    }
    fn evaluation_reserve(doc: &ValidatedDocument) -> Result<usize> {
        let raw = doc.document();
        let mut elements = 0usize;
        for count in raw
            .parameters
            .iter()
            .map(|p| p.default.element_count())
            .chain(raw.constants.iter().map(|c| c.value.element_count()))
        {
            elements = elements
                .checked_add(count)
                .ok_or_else(|| fail(DiagnosticCode::Limit, "transition elements overflow"))?;
        }
        for op in &raw.operations {
            elements = elements
                .checked_add(Self::output_elements(&op.output_port.shape)?)
                .ok_or_else(|| fail(DiagnosticCode::Limit, "transition elements overflow"))?;
        }
        elements
            .checked_mul(24)
            .and_then(|n| {
                raw.operations
                    .len()
                    .checked_mul(512)
                    .and_then(|m| n.checked_add(m))
            })
            .ok_or_else(|| {
                fail(
                    DiagnosticCode::Limit,
                    "transition evaluator reserve overflow",
                )
            })
    }
    fn check_transition_reserve(
        &self,
        retained: usize,
        prospective: usize,
        command_bytes: usize,
        author_commit: bool,
        new_history: bool,
    ) -> Result<()> {
        // Two prospective buffers reserve the new compiled contract and overlapping
        // validation/container-growth scratch. Undo arguments own two further documents;
        // enabled author audit owns one. Existing retention/candidate copies are in retained.
        let copies = 2
            + usize::from(new_history && self.config.history_capacity > 0) * 2
            + usize::from(author_commit && self.config.event_capacity > 0);
        let reserve = prospective
            .checked_mul(copies)
            .and_then(|n| n.checked_add(command_bytes))
            .and_then(|n| n.checked_add(retained))
            .ok_or_else(|| fail(DiagnosticCode::Limit, "author transition storage overflow"))?;
        if reserve > self.config.max_candidate_bytes {
            return Err(fail(
                DiagnosticCode::Limit,
                &format!(
                    "author transition requires {reserve} bytes including old/new/temporary storage; limit is {}",
                    self.config.max_candidate_bytes
                ),
            ));
        }
        Ok(())
    }
    fn author(&self) -> Result<()> {
        if self.config.role == SessionRole::Author {
            Ok(())
        } else {
            Err(fail(DiagnosticCode::Permission, "author role required"))
        }
    }
    fn learner(&self, allowed: bool) -> Result<()> {
        if self.config.role == SessionRole::Learner && !allowed {
            Err(fail(
                DiagnosticCode::Permission,
                "learner permission denied",
            ))
        } else {
            Ok(())
        }
    }
    fn parameter_write(
        &self,
        doc: &ValidatedDocument,
        id: &ParameterId,
        learner: bool,
    ) -> Result<()> {
        let p = doc.parameter(id)?;
        writable(p)?;
        if learner && self.config.role == SessionRole::Learner && !p.metadata.learner_exposed {
            return Err(fail(
                DiagnosticCode::Permission,
                "parameter is not exposed to learner",
            ));
        }
        Ok(())
    }
    fn idle(&self) -> Result<()> {
        if self.transaction.is_some() {
            Err(fail(
                DiagnosticCode::Transaction,
                "finish active transaction before other commands",
            ))
        } else {
            Ok(())
        }
    }
    fn tx(&self, id: TransactionId) -> Result<&Transaction> {
        self.transaction
            .as_ref()
            .filter(|t| t.id == id)
            .ok_or_else(|| {
                fail(
                    DiagnosticCode::Transaction,
                    "transaction ID absent or stale",
                )
            })
    }
    fn document_revision(&self, expected: u64) -> Result<()> {
        if self.document().document().revision != expected {
            Err(fail(
                DiagnosticCode::StaleRevision,
                "stale document revision",
            ))
        } else {
            Ok(())
        }
    }
    fn patch(
        &self,
        doc: &ValidatedDocument,
        values: BTreeMap<ParameterId, Value>,
    ) -> Result<Document> {
        let mut raw = doc.document().clone();
        for (id, value) in values {
            self.parameter_write(doc, &id, false)?;
            let p = raw
                .parameters
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or_else(|| fail(DiagnosticCode::Reference, "parameter missing"))?;
            p.validate_value(&value)?;
            p.default = value;
        }
        Ok(raw)
    }
    fn compile(&self, raw: Document) -> Result<RuntimeGraph> {
        if raw.id != self.document().document().id
            || raw.authority != self.document().document().authority
        {
            return Err(fail(
                DiagnosticCode::Permission,
                "document identity and evaluator authority are immutable",
            ));
        }
        let doc = raw.validate(self.config.decode_limits)?;
        Self::construction_budget(&doc, &self.registry, self.config)?;
        Self::graph_budget(&doc, self.config)?;
        RuntimeGraph::compile(&self.registry, doc)
    }
    fn append_history(
        &mut self,
        before: ValidatedDocument,
        after: ValidatedDocument,
    ) -> Result<()> {
        let e = HistoryEntry { before, after };
        if self.config.history_capacity > 0 {
            e.bytes(self.config.max_history_bytes)?
                .checked_add(std::mem::size_of::<HistoryEntry>())
                .filter(|n| *n <= self.config.max_history_bytes)
                .ok_or_else(|| {
                    fail(
                        DiagnosticCode::Limit,
                        "newest complete history group cannot fit byte budget",
                    )
                })?;
        }
        self.history.truncate(self.cursor);
        if self.config.history_capacity > 0 {
            self.history.push_back(e);
            self.history.shrink_to_fit();

            while self.history.len() > self.config.history_capacity
                || self.history_bytes()? > self.config.max_history_bytes
            {
                self.history.pop_front();
                self.history.shrink_to_fit();
            }
        }
        self.cursor = self.history.len();
        Ok(())
    }
    fn publish(
        &mut self,
        rev: u64,
        time: TimeTicks,
        graph: &RuntimeGraph,
        evaluation: Evaluation,
        structural: bool,
    ) -> Result<()> {
        bounded_size(
            &(evaluation.truth(), evaluation.provenance()),
            self.config.max_frame_bytes,
        )?;
        let properties = resolve(graph, &evaluation)?;
        Self::frame_bytes(&evaluation, &properties, self.config.max_frame_bytes)?;
        let frame_bytes = Self::frame_bytes(&evaluation, &properties, self.config.max_frame_bytes)?;
        let mut changes = ChangeSet {
            structural,
            ..ChangeSet::default()
        };
        for (id, value) in evaluation.truth() {
            if self.frame.truth().get(id) != Some(value) {
                changes.changed_values.insert(id.clone());
            } else if self.frame.provenance().get(id) != evaluation.provenance().get(id) {
                changes.provenance_only.insert(id.clone());
            }
        }
        changes.removed_values = self
            .frame
            .truth()
            .keys()
            .filter(|id| !evaluation.truth().contains_key(*id))
            .cloned()
            .collect();
        for (id, value) in &properties {
            if self.frame.properties().get(id) != Some(value) {
                changes.changed_properties.insert(id.clone());
            }
        }
        changes.removed_properties = self
            .frame
            .properties()
            .keys()
            .filter(|id| !properties.contains_key(*id))
            .cloned()
            .collect();
        let before = self.frame.objects();
        let after = &graph.document().document().objects;
        for o in after {
            if before.iter().find(|old| old.id == o.id) != Some(o) {
                changes.changed_objects.insert(o.id.clone());
            }
        }
        changes.removed_objects = before
            .iter()
            .filter(|o| !after.iter().any(|n| n.id == o.id))
            .map(|o| o.id.clone())
            .collect();
        frame_bytes
            .checked_add(changes.allocated_bytes()?)
            .filter(|n| *n <= self.config.max_frame_bytes)
            .ok_or_else(|| {
                fail(
                    DiagnosticCode::Limit,
                    "frame and changes exceed allocation budget",
                )
            })?;
        self.frame = FrameSnapshot {
            revision: rev,
            document_revision: graph.document().document().revision,
            time,
            document: graph.document().document().id.clone(),
            session: self.identity.clone(),
            epoch: self.epoch,
            authority: graph.document().document().authority.clone(),
            generation: self.generation.clone(),
            evaluation: Arc::new(evaluation),
            properties: Arc::new(properties),
            changes: Arc::new(changes),
            contract: graph.compiled.document_snapshot(),
        };
        Ok(())
    }
    fn checkpoint_baseline(&mut self) {
        self.replay_events.clear();
        self.baseline = self.checkpoint();
    }
    fn record_author_revision(&mut self) -> Result<()> {
        if self.config.event_capacity == 0 {
            self.sequence = increment(self.sequence)?;
            self.epoch = increment(self.epoch)?;
            self.frame.epoch = self.epoch;
            self.checkpoint_baseline();
            Ok(())
        } else {
            self.record(EventKind::AuthorRevision {
                document_revision: self.document().document().revision,
                document: Box::new(self.document().document().clone()),
            })
        }
    }
    fn retain_audit(&mut self, kind: EventKind) -> Result<()> {
        let audit = SessionEvent {
            version: SESSION_VERSION,
            session: self.identity.clone(),
            epoch: self.epoch,
            document: self.document().document().id.clone(),
            document_revision: self.document().document().revision,
            sequence: self.sequence,
            revision: self.revision(),
            time: self.frame.time,
            kind,
        };
        bounded_size(&audit, self.config.max_event_bytes)?;
        if audit.allocated_bytes()? > self.config.max_event_bytes {
            return Err(fail(
                DiagnosticCode::Limit,
                "single event allocation exceeds byte budget",
            ));
        }
        if self.config.event_capacity > 0 {
            self.events.push_back(audit);
            while self.events.len() > self.config.event_capacity
                || Self::event_bytes(&self.events, self.config.max_event_bytes).is_err()
            {
                self.events.pop_front();
                self.events.shrink_to_fit();
            }
        }
        Ok(())
    }
    fn record(&mut self, kind: EventKind) -> Result<()> {
        self.sequence = increment(self.sequence)?;
        if matches!(kind, EventKind::AuthorRevision { .. }) {
            self.epoch = increment(self.epoch)?;
            self.frame.epoch = self.epoch;
        }
        if matches!(
            kind,
            EventKind::AuthorRevision { .. }
                | EventKind::TransactionBegin { .. }
                | EventKind::TransactionUpdate { .. }
                | EventKind::TransactionDocumentUpdate { .. }
                | EventKind::TransactionCancel { .. }
        ) {
            if self.config.event_capacity > 0 {
                self.retain_audit(kind)?;
            }
            self.checkpoint_baseline();
            return Ok(());
        }
        self.retain_audit(kind.clone())?;
        let event = SessionEvent {
            version: SESSION_VERSION,
            session: self.identity.clone(),
            epoch: self.epoch,
            document: self.document().document().id.clone(),
            document_revision: self.document().document().revision,
            sequence: self.sequence,
            revision: self.revision(),
            time: self.frame.time,
            kind,
        };
        bounded_size(&event, self.config.max_event_bytes)?;
        self.replay_events.push_back(event);
        while self.replay_events.len() > self.config.event_capacity
            || Self::event_bytes(&self.replay_events, self.config.max_event_bytes).is_err()
        {
            let event = self
                .replay_events
                .pop_front()
                .ok_or_else(|| fail(DiagnosticCode::Limit, "event cannot fit budget"))?;
            self.replay_events.shrink_to_fit();
            self.baseline.apply(&event)?;
        }
        Ok(())
    }
    fn author_commit(&mut self, mut raw: Document, rev: u64) -> Result<()> {
        raw.revision = self.document().document().revision;
        let changed = self.document().document() != &raw;
        raw.revision = if changed {
            increment(self.document().document().revision)?
        } else {
            self.document().document().revision
        };
        let graph = self.compile(raw)?;
        let evaluation = graph.evaluate(self.frame.time, &self.overrides, None)?;
        if changed && self.config.history_capacity > 0 {
            self.append_history(self.document().clone(), graph.document().clone())?;
        }
        self.publish(rev, self.frame.time, &graph, evaluation, changed)?;
        self.graph = graph;
        if changed {
            self.record_author_revision()?;
        } else {
            self.sequence = increment(self.sequence)?;
            self.checkpoint_baseline();
        }
        Ok(())
    }
    fn apply(&mut self, rev: u64, kind: CommandKind) -> Result<Option<TransactionId>> {
        match kind {
            CommandKind::SetParameter { parameter, value } => {
                self.author()?;
                self.idle()?;
                let raw = self.patch(self.document(), BTreeMap::from([(parameter, value)]))?;
                self.author_commit(raw, rev)?;
            }
            CommandKind::SetParameters {
                expected_document_revision,
                values,
            } => {
                self.author()?;
                self.idle()?;
                self.document_revision(expected_document_revision)?;
                let raw = self.patch(self.document(), values)?;
                self.author_commit(raw, rev)?;
            }
            CommandKind::ResetParameters {
                expected_document_revision,
                parameters,
            } => {
                self.idle()?;
                self.document_revision(expected_document_revision)?;
                let mut values = BTreeMap::new();
                for id in parameters {
                    values.insert(id.clone(), self.document().parameter(&id)?.default.clone());
                }
                self.apply_overrides(rev, values.clone(), EventKind::ResetDefaults { values })?;
            }
            CommandKind::EditDocument {
                expected_document_revision,
                edits,
            } => {
                self.author()?;
                self.idle()?;
                self.document_revision(expected_document_revision)?;
                let mut raw = self.document().document().clone();
                commands::apply_edits(&mut raw, edits)?;
                self.author_commit(raw, rev)?;
            }
            CommandKind::BeginTransaction => {
                self.author()?;
                self.idle()?;
                let id = TransactionId(rev);
                self.transaction = Some(Transaction {
                    id,
                    draft: self.graph.clone(),
                });
                self.frame.revision = rev;
                self.frame.changes = Arc::new(ChangeSet::default());
                self.record(EventKind::TransactionBegin { id })?;
                return Ok(Some(id));
            }
            CommandKind::UpdateTransaction { id, values } => {
                self.author()?;
                let tx = self.tx(id)?;
                let raw = self.patch(tx.draft.document(), values.clone())?;
                let draft = self.compile(raw)?;
                let e = draft.evaluate(self.frame.time, &self.overrides, None)?;
                self.publish(rev, self.frame.time, &draft, e, true)?;
                self.transaction
                    .as_mut()
                    .ok_or_else(|| fail(DiagnosticCode::Transaction, "transaction absent"))?
                    .draft = draft;
                self.record(EventKind::TransactionUpdate { id, values })?;
                return Ok(Some(id));
            }
            CommandKind::UpdateDocumentTransaction {
                id,
                expected_document_revision,
                edits,
            } => {
                self.author()?;
                self.document_revision(expected_document_revision)?;
                let tx = self.tx(id)?;
                let mut raw = tx.draft.document().document().clone();
                commands::apply_edits(&mut raw, edits.clone())?;
                let draft = self.compile(raw)?;
                let e = draft.evaluate(self.frame.time, &self.overrides, None)?;
                self.publish(rev, self.frame.time, &draft, e, true)?;
                self.transaction
                    .as_mut()
                    .ok_or_else(|| fail(DiagnosticCode::Transaction, "transaction absent"))?
                    .draft = draft;
                self.record(EventKind::TransactionDocumentUpdate { id, edits })?;
                return Ok(Some(id));
            }
            CommandKind::CommitTransaction { id } => {
                self.author()?;
                let raw = self.tx(id)?.draft.document().document().clone();
                self.author_commit(raw, rev)?;
                self.transaction = None;
            }
            CommandKind::CancelTransaction { id } => {
                self.author()?;
                self.tx(id)?;
                let graph = self.graph.clone();
                let e = graph.evaluate(self.frame.time, &self.overrides, None)?;
                self.publish(rev, self.frame.time, &graph, e, true)?;
                self.transaction = None;
                self.record(EventKind::TransactionCancel { id })?;
            }
            CommandKind::Undo | CommandKind::Redo => {
                self.author()?;
                self.idle()?;
                let undo = matches!(kind, CommandKind::Undo);
                let index = if undo {
                    self.cursor
                        .checked_sub(1)
                        .ok_or_else(|| fail(DiagnosticCode::History, "nothing to undo"))?
                } else {
                    self.cursor
                };
                let entry = self
                    .history
                    .get(index)
                    .ok_or_else(|| fail(DiagnosticCode::History, "nothing to redo"))?;
                let mut raw = if undo {
                    entry.before.document().clone()
                } else {
                    entry.after.document().clone()
                };
                raw.revision = increment(self.document().document().revision)?;
                let graph = self.compile(raw)?;
                let e = graph.evaluate(self.frame.time, &self.overrides, None)?;
                self.publish(rev, self.frame.time, &graph, e, true)?;
                self.graph = graph;
                self.cursor = if undo { index } else { index + 1 };
                self.record_author_revision()?;
            }
            CommandKind::Seek { time } => {
                self.idle()?;
                self.learner(self.document().document().learner_permissions.seek)?;
                let graph = self.graph.clone();
                let e = graph.evaluate(time, &self.overrides, Some(&self.frame.evaluation))?;
                self.publish(rev, time, &graph, e, false)?;
                self.record(EventKind::Seek { time })?;
            }
            CommandKind::SetLiveOverride { parameter, value } => {
                self.apply_overrides(
                    rev,
                    BTreeMap::from([(parameter.clone(), value.clone())]),
                    EventKind::LiveOverride { parameter, value },
                )?;
            }
            CommandKind::PatchLiveOverrides { entries, playback } => {
                self.apply_live_patch(rev, entries, playback)?
            }
            CommandKind::SetLiveOverrides { values } => {
                self.apply_overrides(rev, values.clone(), EventKind::LiveOverrides { values })?;
            }
            CommandKind::ReturnToGuide { parameters } => {
                self.idle()?;
                self.learner(
                    self.document()
                        .document()
                        .learner_permissions
                        .return_to_guide,
                )?;
                let mut overrides = self.overrides.clone();
                for id in &parameters {
                    self.document().parameter(id)?;
                    overrides.remove(id);
                }
                let graph = self.graph.clone();
                let e =
                    graph.evaluate(self.frame.time, &overrides, Some(&self.frame.evaluation))?;
                self.publish(rev, self.frame.time, &graph, e, false)?;
                self.overrides = overrides;
                self.record(EventKind::ReturnToGuide { parameters })?;
            }
            CommandKind::SetPlaying { playing } => {
                self.idle()?;
                self.learner(self.document().document().learner_permissions.playback)?;
                self.playing = playing;
                self.playback_intent = playing;
                self.frame.revision = rev;
                self.frame.changes = Arc::new(ChangeSet::default());
                self.record(EventKind::Playback { playing })?;
            }
        }
        Ok(None)
    }
    fn patch_playback(&self, policy: OverridePlaybackPolicy) -> Result<(bool, bool)> {
        let target = match policy {
            OverridePlaybackPolicy::Pause => (false, false),
            OverridePlaybackPolicy::Preserve => (self.playing, self.playback_intent),
            OverridePlaybackPolicy::SetIntent(intent) => (intent, intent),
        };
        if matches!(policy, OverridePlaybackPolicy::SetIntent(_))
            || target != (self.playing, self.playback_intent)
        {
            self.learner(self.document().document().learner_permissions.playback)?;
        }
        Ok(target)
    }
    fn patch_preflight(
        &self,
        entries: &Vec<(ParameterId, Option<Value>)>,
        policy: OverridePlaybackPolicy,
        retained: usize,
        command_bytes: usize,
    ) -> Result<bool> {
        self.idle()?;
        let scratch = self
            .overrides
            .len()
            .checked_add(entries.len())
            .and_then(|n| n.checked_mul(std::mem::size_of::<(&ParameterId, &Value)>() + 64))
            .ok_or_else(|| {
                fail(
                    DiagnosticCode::Limit,
                    "patch reference-map reserve overflow",
                )
            })?;
        let queue_buffers =
            [&self.events, &self.replay_events]
                .into_iter()
                .try_fold(0usize, |n, events| {
                    let capacity = events
                        .capacity()
                        .checked_mul(2)
                        .map(|n| n.max(4))
                        .ok_or_else(|| {
                            fail(
                                DiagnosticCode::Limit,
                                "patch event growth capacity overflow",
                            )
                        })?;
                    capacity
                        .checked_mul(std::mem::size_of::<SessionEvent>())
                        .and_then(|m| n.checked_add(m))
                        .ok_or_else(|| {
                            fail(DiagnosticCode::Limit, "patch event growth reserve overflow")
                        })
                })?;
        retained
            .checked_add(
                command_bytes
                    .checked_mul(2)
                    .ok_or_else(|| fail(DiagnosticCode::Limit, "patch payload reserve overflow"))?,
            )
            .and_then(|n| n.checked_add(scratch))
            .and_then(|n| n.checked_add(queue_buffers))
            .and_then(|n| n.checked_add(std::mem::size_of::<SessionEvent>() + 256))
            .filter(|n| *n <= self.config.max_candidate_bytes)
            .ok_or_else(|| {
                fail(
                    DiagnosticCode::Limit,
                    "patch old/candidate/event-growth storage exceeds byte budget",
                )
            })?;
        commands::validate_patch_entries(entries)?;
        let target = self.patch_playback(policy)?;
        let permissions = self.document().document().learner_permissions;
        let mut unchanged = target == (self.playing, self.playback_intent);
        for (id, value) in entries {
            self.parameter_write(self.document(), id, true)?;
            match value {
                Some(value) => {
                    self.learner(permissions.overrides)?;
                    self.document().parameter(id)?.validate_value(value)?;
                    unchanged &= self.overrides.get(id) == Some(value);
                }
                None => {
                    self.learner(permissions.return_to_guide)?;
                    unchanged &= !self.overrides.contains_key(id);
                }
            }
        }
        if unchanged {
            return Ok(true);
        }
        // References describe the exact combined checkpoint without cloning its Values.
        let mut projected = self.overrides.iter().collect::<BTreeMap<_, _>>();
        for (id, value) in entries {
            match value {
                Some(value) => {
                    projected.insert(id, value);
                }
                None => {
                    projected.remove(id);
                }
            }
        }
        let mut allocation = std::mem::size_of::<SessionCheckpoint>()
            + self.identity.as_str().len()
            + self.document().document().id.as_str().len();
        for (id, value) in &projected {
            allocation = allocation
                .checked_add(std::mem::size_of::<(ParameterId, Value)>() + 64 + id.as_str().len())
                .and_then(|n| {
                    commands::patch_value_bytes(value)
                        .ok()
                        .and_then(|m| n.checked_add(m))
                })
                .ok_or_else(|| {
                    fail(
                        DiagnosticCode::Limit,
                        "projected checkpoint allocation overflow",
                    )
                })?;
        }
        if allocation > self.config.max_command_bytes {
            return Err(fail(
                DiagnosticCode::Limit,
                "projected patch checkpoint exceeds byte budget",
            ));
        }
        let view = PatchCheckpointView {
            version: SESSION_VERSION,
            session: &self.identity,
            epoch: self.epoch,
            document: &self.document().document().id,
            document_revision: self.document().document().revision,
            sequence: increment(self.sequence)?,
            revision: increment(self.revision())?,
            time: self.frame.time,
            overrides: &projected,
            playback_intent: target.1,
        };
        bounded_size(&view, self.config.max_command_bytes)?;
        // Admit the compacted event using references before copying any payload.
        let event_allocation = entries.iter().try_fold(
            std::mem::size_of::<SessionEvent>()
                .checked_add(self.identity.as_str().len())
                .and_then(|n| n.checked_add(self.document().document().id.as_str().len()))
                .and_then(|n| {
                    entries
                        .len()
                        .checked_mul(std::mem::size_of::<(ParameterId, Option<Value>)>())
                        .and_then(|m| n.checked_add(m))
                })
                .ok_or_else(|| fail(DiagnosticCode::Limit, "patch event allocation overflow"))?,
            |n, (id, value)| {
                n.checked_add(id.as_str().len())
                    .and_then(|n| {
                        value
                            .as_ref()
                            .map(commands::patch_value_bytes)
                            .transpose()
                            .ok()
                            .and_then(|m| n.checked_add(m.unwrap_or(0)))
                    })
                    .ok_or_else(|| fail(DiagnosticCode::Limit, "patch event allocation overflow"))
            },
        )?;
        if event_allocation > self.config.max_event_bytes {
            return Err(fail(
                DiagnosticCode::Limit,
                "patch event allocation exceeds byte budget",
            ));
        }
        let event = PatchEventView {
            version: SESSION_VERSION,
            session: &self.identity,
            epoch: self.epoch,
            document: &self.document().document().id,
            document_revision: self.document().document().revision,
            sequence: increment(self.sequence)?,
            revision: increment(self.revision())?,
            time: self.frame.time,
            kind: PatchEventKindView::PatchLiveOverrides {
                entries,
                playback: policy,
            },
        };
        bounded_size(&event, self.config.max_event_bytes)?;
        Ok(false)
    }
    fn apply_live_patch(
        &mut self,
        rev: u64,
        entries: Vec<(ParameterId, Option<Value>)>,
        playback: OverridePlaybackPolicy,
    ) -> Result<()> {
        let target = self.patch_playback(playback)?;
        let event = EventKind::PatchLiveOverrides { entries, playback }.compact_storage();
        if let EventKind::PatchLiveOverrides { entries, .. } = &event {
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
        }
        let graph = self.graph.clone();
        let e = graph.evaluate(
            self.frame.time,
            &self.overrides,
            Some(&self.frame.evaluation),
        )?;
        self.publish(rev, self.frame.time, &graph, e, false)?;
        self.playing = target.0;
        self.playback_intent = target.1;
        self.record(event)
    }
    fn apply_overrides(
        &mut self,
        rev: u64,
        values: BTreeMap<ParameterId, Value>,
        event: EventKind,
    ) -> Result<()> {
        self.idle()?;
        self.learner(self.document().document().learner_permissions.overrides)?;
        let mut overrides = self.overrides.clone();
        for (id, value) in values {
            self.parameter_write(self.document(), &id, true)?;
            self.document().parameter(&id)?.validate_value(&value)?;
            overrides.insert(id, value.compact_storage());
        }
        let graph = self.graph.clone();
        let e = graph.evaluate(self.frame.time, &overrides, Some(&self.frame.evaluation))?;
        self.publish(rev, self.frame.time, &graph, e, false)?;
        self.overrides = overrides;
        self.playing = false;
        self.playback_intent = false;
        self.record(event.compact_storage())
    }
    pub fn prepare_command(&self, kind: CommandKind) -> Command {
        self.frame.command(kind)
    }
    pub fn dispatch(&mut self, command: Command) -> Result<DispatchResult> {
        if command.session != self.identity
            || command.generation != self.generation
            || command.document != self.document().document().id
            || command.epoch != self.epoch
        {
            return Err(fail(
                DiagnosticCode::StaleRevision,
                "command belongs to a foreign document/session/epoch/runtime generation",
            ));
        }
        self.document_revision(command.expected_document_revision)?;
        if command.expected_revision != self.revision() {
            return Err(fail(
                DiagnosticCode::StaleRevision,
                "stale session revision",
            ));
        }
        bounded_size(&command, self.config.max_command_bytes)?;
        if command.allocated_bytes()? > self.config.max_command_bytes {
            return Err(fail(
                DiagnosticCode::Limit,
                "typed command retained allocation exceeds budget",
            ));
        }
        let count = match &command.kind {
            CommandKind::EditDocument { edits, .. }
            | CommandKind::UpdateDocumentTransaction { edits, .. } => edits.len(),
            CommandKind::SetParameters { values, .. }
            | CommandKind::UpdateTransaction { values, .. }
            | CommandKind::SetLiveOverrides { values } => values.len(),
            CommandKind::PatchLiveOverrides { entries, .. } => entries.len(),
            CommandKind::ResetParameters { parameters, .. }
            | CommandKind::ReturnToGuide { parameters } => parameters.len(),
            _ => 1,
        };
        if count > self.config.max_batch_edits {
            return Err(fail(
                DiagnosticCode::Limit,
                "command batch count exceeds limit",
            ));
        }
        let retained = self.storage_bytes()?;
        let command_bytes = command
            .allocated_bytes()?
            .max(bounded_size(&command, self.config.max_command_bytes)?);
        self.document_transition_budget(&command.kind, retained, command_bytes)?;
        if let CommandKind::PatchLiveOverrides { entries, playback } = &command.kind
            && self.patch_preflight(entries, *playback, retained, command_bytes)?
        {
            return Ok(DispatchResult {
                revision: self.revision(),
                document_revision: self.document().document().revision,
                transaction: None,
                changes: ChangeSet::default(),
                snapshot: self.snapshot(),
            });
        }
        let rev = increment(command.expected_revision)?;
        if let CommandKind::ResetParameters {
            expected_document_revision,
            parameters,
        } = &command.kind
        {
            self.idle()?;
            self.document_revision(*expected_document_revision)?;
            self.learner(self.document().document().learner_permissions.overrides)?;
            let mut unchanged = !self.playing && !self.playback_intent;
            for id in parameters {
                self.parameter_write(self.document(), id, true)?;
                unchanged &=
                    self.overrides.get(id) == Some(&self.document().parameter(id)?.default);
            }
            if unchanged {
                return Ok(DispatchResult {
                    revision: self.revision(),
                    document_revision: self.document().document().revision,
                    transaction: None,
                    changes: ChangeSet::default(),
                    snapshot: self.snapshot(),
                });
            }
        }
        let mut candidate = self.candidate();
        let transaction = candidate.apply(rev, command.kind)?;
        candidate.storage_budget()?;
        let result = DispatchResult {
            revision: candidate.revision(),
            document_revision: candidate.document().document().revision,
            transaction,
            changes: candidate.frame.changes().clone(),
            snapshot: candidate.snapshot(),
        };
        *self = candidate;
        Ok(result)
    }
    pub fn set_parameter(
        &mut self,
        expected: u64,
        parameter: ParameterId,
        value: Value,
    ) -> Result<()> {
        self.dispatch(Command {
            session: self.identity.clone(),
            generation: self.generation.clone(),
            document: self.document().document().id.clone(),
            epoch: self.epoch,
            expected_document_revision: self.document().document().revision,
            expected_revision: expected,
            kind: CommandKind::SetParameter { parameter, value },
        })
        .map(|_| ())
    }
    pub fn begin_transaction(&mut self, expected: u64) -> Result<TransactionId> {
        self.dispatch(Command {
            session: self.identity.clone(),
            generation: self.generation.clone(),
            document: self.document().document().id.clone(),
            epoch: self.epoch,
            expected_document_revision: self.document().document().revision,
            expected_revision: expected,
            kind: CommandKind::BeginTransaction,
        })?
        .transaction
        .ok_or_else(|| fail(DiagnosticCode::Transaction, "transaction missing"))
    }
    pub fn update_transaction(
        &mut self,
        expected: u64,
        id: TransactionId,
        values: BTreeMap<ParameterId, Value>,
    ) -> Result<()> {
        self.dispatch(Command {
            session: self.identity.clone(),
            generation: self.generation.clone(),
            document: self.document().document().id.clone(),
            epoch: self.epoch,
            expected_document_revision: self.document().document().revision,
            expected_revision: expected,
            kind: CommandKind::UpdateTransaction { id, values },
        })
        .map(|_| ())
    }
    pub fn commit_transaction(&mut self, expected: u64, id: TransactionId) -> Result<()> {
        self.dispatch(Command {
            session: self.identity.clone(),
            generation: self.generation.clone(),
            document: self.document().document().id.clone(),
            epoch: self.epoch,
            expected_document_revision: self.document().document().revision,
            expected_revision: expected,
            kind: CommandKind::CommitTransaction { id },
        })
        .map(|_| ())
    }
    pub fn cancel_transaction(&mut self, expected: u64, id: TransactionId) -> Result<()> {
        self.dispatch(Command {
            session: self.identity.clone(),
            generation: self.generation.clone(),
            document: self.document().document().id.clone(),
            epoch: self.epoch,
            expected_document_revision: self.document().document().revision,
            expected_revision: expected,
            kind: CommandKind::CancelTransaction { id },
        })
        .map(|_| ())
    }
    pub fn undo(&mut self, expected: u64) -> Result<()> {
        self.dispatch(Command {
            session: self.identity.clone(),
            generation: self.generation.clone(),
            document: self.document().document().id.clone(),
            epoch: self.epoch,
            expected_document_revision: self.document().document().revision,
            expected_revision: expected,
            kind: CommandKind::Undo,
        })
        .map(|_| ())
    }
    pub fn redo(&mut self, expected: u64) -> Result<()> {
        self.dispatch(Command {
            session: self.identity.clone(),
            generation: self.generation.clone(),
            document: self.document().document().id.clone(),
            epoch: self.epoch,
            expected_document_revision: self.document().document().revision,
            expected_revision: expected,
            kind: CommandKind::Redo,
        })
        .map(|_| ())
    }
    pub fn seek(&mut self, expected: u64, time: TimeTicks) -> Result<()> {
        self.dispatch(Command {
            session: self.identity.clone(),
            generation: self.generation.clone(),
            document: self.document().document().id.clone(),
            epoch: self.epoch,
            expected_document_revision: self.document().document().revision,
            expected_revision: expected,
            kind: CommandKind::Seek { time },
        })
        .map(|_| ())
    }
    pub fn set_live_override(
        &mut self,
        expected: u64,
        parameter: ParameterId,
        value: Value,
    ) -> Result<()> {
        self.dispatch(Command {
            session: self.identity.clone(),
            generation: self.generation.clone(),
            document: self.document().document().id.clone(),
            epoch: self.epoch,
            expected_document_revision: self.document().document().revision,
            expected_revision: expected,
            kind: CommandKind::SetLiveOverride { parameter, value },
        })
        .map(|_| ())
    }
    pub fn return_to_guide(&mut self, expected: u64, parameters: &[ParameterId]) -> Result<()> {
        self.dispatch(Command {
            session: self.identity.clone(),
            generation: self.generation.clone(),
            document: self.document().document().id.clone(),
            epoch: self.epoch,
            expected_document_revision: self.document().document().revision,
            expected_revision: expected,
            kind: CommandKind::ReturnToGuide {
                parameters: parameters.to_vec(),
            },
        })
        .map(|_| ())
    }
    pub fn set_playing(&mut self, expected: u64, playing: bool) -> Result<()> {
        self.dispatch(Command {
            session: self.identity.clone(),
            generation: self.generation.clone(),
            document: self.document().document().id.clone(),
            epoch: self.epoch,
            expected_document_revision: self.document().document().revision,
            expected_revision: expected,
            kind: CommandKind::SetPlaying { playing },
        })
        .map(|_| ())
    }
    fn candidate(&self) -> Self {
        Self {
            graph: self.graph.clone(),
            registry: self.registry.clone(),
            config: self.config,
            frame: self.frame.clone(),
            overrides: self.overrides.clone(),
            playing: self.playing,
            history: self.history.clone(),
            cursor: self.cursor,
            replay_events: self.replay_events.clone(),
            events: self.events.clone(),
            transaction: self.transaction.clone(),
            identity: self.identity.clone(),
            epoch: self.epoch,
            sequence: self.sequence,
            baseline: self.baseline.clone(),
            playback_intent: self.playback_intent,
            generation: self.generation.clone(),
            imported: self.imported,
        }
    }
}
