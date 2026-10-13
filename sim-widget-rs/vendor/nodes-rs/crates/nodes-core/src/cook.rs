//! Graph evaluation: topological cook with dirty propagation and staged ordering.
//!
//! - [`cook`] / [`cook_with`] — full graph (with or without host options)
//! - [`cook_dirty`] / [`cook_dirty_with`] — dirty nodes + downstream only (editor hot path)
//! - [`mark_dirty`] — flag after attr / connection edits
//!
//! ## The cook contract
//!
//! A node type's [`crate::NodeType::cook`] is `fn(&mut CookCtx) -> CookResult<()>`. Everything it needs
//! is on the [`CookCtx`]: its own node, a read-only [`GraphView`], the registry + the value-type
//! registry, the graph's [`DataBus`] (slots AND heavy `Arc` handles), a [`CancelToken`] and a
//! [`ProgressSink`]. A cook REPORTS failure by returning [`CookError`] — it never panics, never
//! logs-and-pretends, and never needs a driver special-case to reach the bus (which is exactly
//! what `BusRead`/`BusWrite` used to need: they were hardcoded in this driver, so a custom
//! bus-touching node type was impossible to write).
//!
//! ## Status
//!
//! The driver stamps every node it runs with a [`NodeStatus`] (`Cooking` → `Done`/`Error`),
//! stored transiently in the node's `tmp` section (never serialized). A cancelled cook leaves
//! the node `Idle` and still DIRTY, so the next pass re-runs it — cancellation is not a failure.
//! A panicking cook is caught and converted into `NodeStatus::Error("panicked: …")`.
//!
//! ## SSOT guarantee
//!
//! Connected input resolution is **transient**: resolved values are written into
//! `data.compute.<name>` ([`Node::set_compute`]/[`Node::insert_compute`], a section that exists
//! only for the duration of one node's cook pass and is never serialized). The stored
//! `inputs.<name>.value` is **never mutated** by the cook engine; it always holds the author's
//! literal. This means:
//! - Serialization always saves the authored literal, not a stale pulled value.
//! - After disconnect the original literal is immediately visible again without a re-save.
//!
//! ## Incompatible-type handling
//!
//! Conversion is owned by [`ValueTypes::convert`] (the type registry is the SSOT — see
//! `value_types.rs`). When it returns `None` for a connected input the engine logs a warning and
//! keeps the stored authored value (no silent forwarding of an incompatible value).

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use crate::box_model::BoxValue;
use crate::bus::DataBus;
use crate::graph::NodeId;
use crate::graph_container::Graph;
use crate::index::AdjacencyIndex;
use crate::node::Node;
use crate::node_type::NodeKind;
use crate::registry::NodeTypeRegistry;
use crate::value_types::ValueTypes;
use crate::worker::{WorkerPolicy, WorkerSet};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Result of a node cook. `Ok(())` = the node produced its outputs.
pub type CookResult<T = ()> = Result<T, CookError>;

/// Why a node cook did not complete.
#[derive(Debug, Clone)]
pub enum CookError {
    /// A numeric/domain failure (division by zero, singular matrix, …).
    Math(String),
    /// The host asked the cook to stop ([`CancelToken`]). **Not a failure**: the node is left
    /// `Idle` + dirty so the next pass re-runs it.
    Cancelled,
    /// Anything else, with a human-readable reason.
    Custom(String),
}

impl CookError {
    /// Wrap any `Display` error (an `io::Error`, an image-decode error, …) as a [`CookError`].
    /// Lets a cook write `something().map_err(CookError::msg)?` instead of formatting by hand.
    pub fn msg(e: impl fmt::Display) -> Self {
        Self::Custom(e.to_string())
    }
}

impl fmt::Display for CookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Math(msg) => write!(f, "math error: {msg}"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::Custom(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for CookError {}

// ---------------------------------------------------------------------------
// Cancellation + progress
// ---------------------------------------------------------------------------

/// Shared cancel flag handed to every cook of one pass. Clone it into a UI thread and call
/// [`CancelToken::cancel`] to ask a long-running cook (an MVS solve, a bake) to bail out; the
/// cook observes it via [`CookCtx::check_cancel`].
///
/// **The `Arc<AtomicBool>` inside IS the contract, not an implementation detail** ([`CancelToken::as_arc`]
/// / [`CancelToken::from_arc`]). We deliberately declined to expose it at first, on the grounds that it
/// leaks a representation detail for zero gain. A real consumer produced the gain within a day: its
/// compute stages (`extract_features`, `run_incremental_mapper`, `dense_stereo`, `train_splats`) each
/// take an `Option<Arc<AtomicBool>>` and poll it INSIDE their own loops. Without the raw flag a cook can
/// only check cancellation at its BOUNDARIES (`ctx.check_cancel()?` before and after a stage), so a
/// 40-minute splat train would ignore Ctrl-C until it finished. `Arc<AtomicBool>` is also the single most
/// common cancellation shape in the ecosystem, and a cancellation type that cannot interoperate with it
/// is not finished. These two methods are how cancellation crosses into worker code that predates this
/// crate.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// Request cancellation. Idempotent; visible to every clone of this token.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// The raw flag, to hand INTO a worker API that takes an `Arc<AtomicBool>` and polls it inside its
    /// own loop (see the type docs). It is the SAME `Arc` this token reads, not a copy: setting it from
    /// the worker side cancels this token, and [`CancelToken::cancel`] is seen by the worker.
    pub fn as_arc(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.0)
    }

    /// Adopt a flag the HOST already owns — typically the `Arc<AtomicBool>` a Ctrl-C handler was
    /// installed around before this crate was in the picture.
    ///
    /// Load-bearing: without it a host must keep its own flag AND a separate token, set both, and they
    /// WILL drift. With it there is exactly ONE flag.
    pub fn from_arc(flag: Arc<AtomicBool>) -> Self {
        Self(flag)
    }
}

/// Sink a long cook reports sub-progress to (`ctx.phase("matching", 3, 12)`). The host implements
/// it to drive a progress bar; `Send + Sync` because a cook may fan out to worker threads.
///
/// The report carries the NODE it came from, so a host can render **per-node** progress (the whole
/// point of [`NodeStatus::Cooking`]) rather than a single global bar. The driver fills the id in;
/// a cook only supplies the phase name and counts.
pub trait ProgressSink: Send + Sync {
    /// Report that node `node`'s phase `name` is `done` of `total` units complete (`total == 0` =
    /// indeterminate).
    fn phase(&self, node: NodeId, name: &str, done: u64, total: u64);
}

/// The default [`ProgressSink`]: drops every report. Used when the host supplies none.
pub struct NoopProgress;

impl ProgressSink for NoopProgress {
    fn phase(&self, _node: NodeId, _name: &str, _done: u64, _total: u64) {}
}

/// The process-wide no-op sink — the default for a pass whose host reports no progress.
///
/// This is the ONE way to name it. There used to also be a `pub static NOOP_PROGRESS: NoopProgress`,
/// but once the sink became `Arc`-carried nothing could use the `&'static dyn` form any more (every
/// consumer needs an `Arc`), so it was two names for one thing — exactly the drift this crate keeps
/// deleting. Allocated once, cloned thereafter.
pub fn noop_progress() -> Arc<dyn ProgressSink> {
    static SINK: OnceLock<Arc<dyn ProgressSink>> = OnceLock::new();
    Arc::clone(SINK.get_or_init(|| Arc::new(NoopProgress)))
}

/// Host-supplied services for one cook pass, keyed by CONCRETE TYPE ([`TypeId`]) rather than by a
/// string.
///
/// Fills the gap between the two lanes that already existed: `DataBus` slots (serializable
/// node→node data) and `DataBus` handles (transient node→node payloads under a string key, e.g.
/// `pointcloud/{id}` — a sound pattern, because there the key IS data). A HOST SERVICE is neither:
/// its key is an identity, and a cross-crate hard-coded string for it has no compile-time check and
/// no collision detection. Keying by type removes both problems.
///
/// Deliberately minimal — a map, an install, a get. No registry, no lifetimes, no ordering.
#[derive(Default, Clone)]
pub struct ServiceMap {
    by_type: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl ServiceMap {
    /// An empty map (no services installed).
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the service of type `T`, replacing any previous one of the SAME type (which is the
    /// only thing it could collide with — that is the point of keying by type).
    pub fn insert<T: Any + Send + Sync>(&mut self, service: Arc<T>) {
        self.by_type.insert(TypeId::of::<T>(), service);
    }

    /// The service of type `T`, or `None`. Reached from a cook as [`CookCtx::service`].
    pub fn get<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.by_type
            .get(&TypeId::of::<T>())
            .cloned()
            .and_then(|a| a.downcast::<T>().ok())
    }
}

/// The shared EMPTY service map — the default for a pass whose host installs no services (and for
/// [`CookCtx::for_test`]).
fn empty_services() -> &'static ServiceMap {
    static EMPTY: OnceLock<ServiceMap> = OnceLock::new();
    EMPTY.get_or_init(ServiceMap::new)
}

// ---------------------------------------------------------------------------
// Cook context
// ---------------------------------------------------------------------------

/// Read-only view of the rest of the graph handed to a cook: every OTHER node plus the derived
/// topology. The cooking node itself is detached from `nodes` for the duration of its cook (it
/// is `ctx.node`), which is what lets a cook hold `&mut Node` and `&Graph`-worth of context at
/// the same time.
#[derive(Clone, Copy)]
pub struct GraphView<'a> {
    pub nodes: &'a HashMap<NodeId, Node>,
    pub index: &'a AdjacencyIndex,
}

impl<'a> GraphView<'a> {
    /// Another node by id (the cooking node itself is absent — it is `ctx.node`).
    pub fn node(&self, id: NodeId) -> Option<&'a Node> {
        self.nodes.get(&id)
    }

    /// Direct children of `id` (subnet interior).
    pub fn children_of(&self, id: NodeId) -> &'a [NodeId] {
        self.index.children_of(id)
    }
}

impl GraphView<'static> {
    /// A view of NOTHING (no nodes, empty topology) — the neighbourless context a unit test gives
    /// a cook closure that only reads its own node (see [`CookCtx::for_test`]).
    pub fn empty() -> Self {
        static NODES: OnceLock<HashMap<NodeId, Node>> = OnceLock::new();
        static INDEX: OnceLock<AdjacencyIndex> = OnceLock::new();
        Self {
            nodes: NODES.get_or_init(HashMap::new),
            index: INDEX.get_or_init(AdjacencyIndex::default),
        }
    }
}

/// Everything a node cook may touch. Fields are public because a closure routinely needs two of
/// them at once (`ctx.node` + `ctx.bus`), which method accessors would forbid.
///
/// `#[non_exhaustive]` means it cannot be built with a struct literal from ANOTHER crate, so new
/// context can be added here without breaking node authors. Out-of-crate authors unit-testing
/// their own [`crate::NodeType`] build one with [`CookCtx::for_test`] instead of registering the
/// type and driving a whole `cook()`.
#[non_exhaustive]
pub struct CookCtx<'a> {
    /// The node being cooked (detached from the graph for the duration of the cook).
    pub node: &'a mut Node,
    /// Read-only view of every other node + the topology.
    pub graph: GraphView<'a>,
    /// Node-type registry (port types, nested defs).
    pub registry: &'a NodeTypeRegistry,
    /// Value-type registry — the conversion SSOT ([`ValueTypes::convert`]).
    pub types: &'a ValueTypes,
    /// The graph's data bus: serializable slots AND transient heavy `Arc` handles. A write is
    /// visible to every node cooked LATER in this pass (see [`CookStage`]).
    pub bus: &'a mut DataBus,
    /// The graph's workers, for [`CookCtx::worker`]. `None` in a test context
    /// and under [`WorkerPolicy::None`].
    pub(crate) workers: Option<&'a WorkerSet>,
    /// Cooperative cancellation for long cooks.
    pub cancel: &'a CancelToken,
    /// Sub-progress reporting for long cooks.
    ///
    /// An `Arc`, not a `&dyn`: a real cook fans out to stage functions/worker threads that must OWN
    /// a handle to the sink (`Option<Arc<dyn ProgressSink>>`), and you cannot make one from a
    /// borrow. Mirrors [`CancelToken`], which is already `Arc`-backed and `Clone` for exactly this
    /// reason. Clone it with [`CookCtx::progress_arc`].
    pub progress: Arc<dyn ProgressSink>,
    /// Host-supplied SERVICES, keyed by type (see [`CookCtx::service`]).
    pub services: &'a ServiceMap,
}

impl<'a> CookCtx<'a> {
    /// Build a context for **unit-testing a `CookFn` in isolation**: the cook sees only its own
    /// `node` and the `bus`, with an empty [`GraphView`], no cancellation and no progress sink.
    /// Use [`Self::with_graph`] / [`Self::with_progress`] to fill in what a particular test needs
    /// (cancellation comes from the `cancel` token the caller passes in).
    ///
    /// This exists because `#[non_exhaustive]` (rightly) blocks out-of-crate struct-literal
    /// construction — without it, a host that registers its own node types could only test their
    /// cook by building a whole `Graph` and running the driver.
    pub fn for_test(
        node: &'a mut Node,
        registry: &'a NodeTypeRegistry,
        types: &'a ValueTypes,
        bus: &'a mut DataBus,
        cancel: &'a CancelToken,
    ) -> Self {
        Self {
            node,
            graph: GraphView::empty(),
            registry,
            types,
            bus,
            // A test context runs no threads; `ctx.worker()` reports none, the
            // same as a pass under `WorkerPolicy::None`.
            workers: None,
            cancel,
            progress: noop_progress(),
            services: empty_services(),
        }
    }

    /// Give a [`Self::for_test`] context a real graph view (neighbour nodes + topology).
    pub fn with_graph(mut self, graph: GraphView<'a>) -> Self {
        self.graph = graph;
        self
    }

    /// Give a [`Self::for_test`] context a progress sink (to assert what a cook reports).
    pub fn with_progress(mut self, progress: Arc<dyn ProgressSink>) -> Self {
        self.progress = progress;
        self
    }

    /// Give a [`Self::for_test`] context a service map (see [`Self::service`]).
    pub fn with_services(mut self, services: &'a ServiceMap) -> Self {
        self.services = services;
        self
    }

    /// An OWNED handle to the progress sink — what a cook hands to the worker threads / stage
    /// functions it fans out to, since they outlive the `&mut CookCtx` borrow.
    pub fn progress_arc(&self) -> Arc<dyn ProgressSink> {
        Arc::clone(&self.progress)
    }

    /// A host-supplied SERVICE of type `T`, keyed by its [`TypeId`] — `None` if the host installed
    /// none.
    ///
    /// This is the typed channel for things the HOST provides to every cook: a solver handle, a
    /// thumbnail-writer callback, a DB pool. The alternative was [`Self::handle`] with a
    /// hard-coded string key shared across crates (`"thumb_writer"`), which has no compile-time
    /// check and no collision detection — two crates picking the same key silently hand each other
    /// the wrong object. `DataBus` handles remain the right tool for node→node payloads, where the
    /// key is data (`pointcloud/{id}`) rather than an identity.
    ///
    /// A bare `dyn Fn` cannot be a `T` here (`Any` requires `Sized`, which is Rust's rule, not
    /// ours): wrap the callback in a newtype in YOUR crate
    /// (`struct ThumbWriter(pub Arc<dyn Fn(&Path) + Send + Sync>);`) and install THAT. The newtype
    /// is what gives the service its identity, so this is a feature rather than a workaround.
    /// This node's own background worker, as its concrete type.
    ///
    /// `None` when the type declares none, when the pass runs under
    /// [`WorkerPolicy::None`], or in a test context — all of which mean the
    /// same thing to a cook: *nothing is running in the background, do it
    /// yourself or produce nothing this tick*. Handling that case is the node's
    /// job, and it is what makes a deterministic replay possible.
    pub fn worker<T: crate::worker::NodeWorker>(&self) -> Option<Arc<T>> {
        self.workers?.get::<T>(self.node.id)
    }

    pub fn service<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.services.get::<T>()
    }

    /// `Err(CookError::Cancelled)` once the host has cancelled the pass — a long cook should
    /// `ctx.check_cancel()?` inside its loop.
    pub fn check_cancel(&self) -> CookResult<()> {
        if self.cancel.is_cancelled() {
            Err(CookError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Sugar over [`DataBus::get_handle`]: a typed heavy runtime object from the bus's transient
    /// handle lane (a reconstruction, a GPU buffer, a DB connection) — `None` when the slot is
    /// absent or holds a different concrete type.
    pub fn handle<T: Any + Send + Sync>(&self, key: &str) -> Option<Arc<T>> {
        self.bus.get_handle::<T>(key)
    }

    /// Report sub-progress of a long cook: `ctx.phase("matching", 3, 12)`.
    ///
    /// This does BOTH halves of the job, which is why it takes `&mut self`:
    /// 1. writes `NodeStatus::Cooking { progress, msg }` into the node's own transient `tmp.status`,
    ///    so the value is on the node when it is REINSERTED into the graph — i.e. it is what
    ///    [`Node::status`] reports afterwards, and what the cook itself can read back mid-run. It is
    ///    NOT a live channel to another thread: the driver DETACHES the node from `graph.nodes` for
    ///    the whole duration of its cook, so nothing outside can observe this write while it is
    ///    happening. (This doc used to claim exactly that, and it was false.)
    /// 2. forwards the report — WITH this node's id — to the host's [`ProgressSink`]. THAT, and the
    ///    `StatusObserver` the driver calls, are the real live channels: they are how a UI drives a
    ///    per-node progress bar while the node is still detached.
    ///
    /// `total == 0` means indeterminate: progress is reported as `0.0`.
    pub fn phase(&mut self, name: &str, done: u64, total: u64) {
        let progress = if total == 0 {
            0.0
        } else {
            (done as f32 / total as f32).clamp(0.0, 1.0)
        };
        self.node.set_status(NodeStatus::Cooking {
            progress,
            msg: name.to_string(),
        });
        self.progress.phase(self.node.id, name, done, total);
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Per-node cook state, stamped by the driver into the node's transient `tmp` section (never
/// serialized). Read it via [`Node::status`] to colour a node / show a spinner or an error badge.
#[derive(Clone, Debug)]
pub enum NodeStatus {
    /// Never cooked this session, or the last attempt was cancelled.
    Idle,
    /// Currently cooking; `progress` in `0..=1` and a free-form phase message. The driver sets
    /// the initial `0.0`/empty; a long cook updates both by calling [`CookCtx::phase`], which
    /// writes them here AND forwards them to the host's [`ProgressSink`].
    Cooking { progress: f32, msg: String },
    /// Last cook succeeded, taking `elapsed_s` seconds.
    Done { elapsed_s: f64 },
    /// Last cook returned [`CookError`] (or panicked).
    Error(String),
}

/// When a node type is evaluated within one cook pass.
///
/// Replaces the old hardcoded `type_name() == "BusRead"` ordering split, which a custom
/// bus-reading node type could never match. `Late` nodes run in a second pass AFTER every `Main`
/// node, so they observe every bus write made during the main pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CookStage {
    /// Normal topological evaluation.
    #[default]
    Main,
    /// Deferred: runs after the whole `Main` pass (bus readers).
    Late,
}

// ---------------------------------------------------------------------------
// Driver options
// ---------------------------------------------------------------------------

/// Per-node status callback: invoked once when a node starts cooking and once when it finishes.
/// Drive a stage-by-stage progress UI from this (it replaced the old `cook_observed`, which could
/// only report a node's NAME and only before the cook).
pub type StatusObserver<'a> = &'a mut dyn FnMut(&Node, &NodeStatus);

/// Host-supplied knobs for one cook pass.
pub struct CookOpts<'a> {
    /// Cooperative cancellation, shared by every node in the pass.
    pub cancel: &'a CancelToken,
    /// Sub-progress sink handed to every cook. `Arc` (not `&dyn`) so a cook can hand an OWNED
    /// handle to the workers it fans out to — see [`CookCtx::progress`].
    pub progress: Arc<dyn ProgressSink>,
    /// Per-node status callback (see [`StatusObserver`]).
    pub observer: Option<StatusObserver<'a>>,
    /// Host services, keyed by type and readable from any cook as `ctx.service::<T>()` (see
    /// [`ServiceMap`]).
    pub services: ServiceMap,
    /// Whether this pass may start background workers ([`WorkerPolicy`]).
    pub workers: WorkerPolicy,
}

impl<'a> CookOpts<'a> {
    /// Options with no observer and no services.
    pub fn new(cancel: &'a CancelToken, progress: Arc<dyn ProgressSink>) -> Self {
        Self {
            cancel,
            progress,
            observer: None,
            services: ServiceMap::new(),
            workers: WorkerPolicy::default(),
        }
    }

    /// Start no background workers this pass ([`WorkerPolicy::None`]).
    ///
    /// What a replay or a backtest wants: with no thread deciding how much has
    /// arrived by the time a node reads it, the same input gives the same
    /// output. One call here replaces substituting every async worker by hand
    /// in every harness — and forgetting one is exactly how a run produces
    /// plausible numbers that depend on timing.
    pub fn without_workers(mut self) -> Self {
        self.workers = WorkerPolicy::None;
        self
    }

    /// Install a host service of type `T`, readable from every cook of this pass as
    /// `ctx.service::<T>()`. Chainable:
    /// `CookOpts::new(&cancel, noop_progress()).with_service(Arc::new(MySolver::new()))`.
    pub fn with_service<T: Any + Send + Sync>(mut self, service: Arc<T>) -> Self {
        self.services.insert(service);
        self
    }

    /// Attach a per-node status observer.
    pub fn with_observer(mut self, observer: StatusObserver<'a>) -> Self {
        self.observer = Some(observer);
        self
    }
}

// ---------------------------------------------------------------------------
// Stock cook closures
// ---------------------------------------------------------------------------

/// Forward the first input's cook value to the first output (constants, passthrough).
///
/// A plain `fn`, so it can be handed straight to [`crate::fn_node`] as the cook of a type that
/// needs no behaviour of its own (`Null`, `Display`, the constants, the backdrop nodes).
pub fn passthrough_cook(ctx: &mut CookCtx) -> CookResult<()> {
    let in0 = ctx.node.input_names().into_iter().next();
    let out0 = ctx.node.output_names().into_iter().next();
    let value = in0.as_deref().and_then(|i| ctx.node.cook_value(i));
    if let (Some(o), Some(v)) = (out0, value) {
        ctx.node.set_out(&o, v);
    }
    Ok(())
}

/// Write a single float output by name.
pub fn set_float_output(node: &mut Node, name: &str, value: f64) {
    node.set_out(name, BoxValue::F64(value));
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Evaluate the entire graph in topological order (default options: no cancel, no progress,
/// no observer).
pub fn cook(graph: &mut Graph, registry: &NodeTypeRegistry, value_types: &ValueTypes) {
    let cancel = CancelToken::default();
    let mut opts = CookOpts::new(&cancel, noop_progress());
    cook_with(graph, registry, value_types, &mut opts);
}

/// Evaluate the entire graph in topological order with host [`CookOpts`] (cancellation, progress,
/// per-node status observation).
pub fn cook_with(
    graph: &mut Graph,
    registry: &NodeTypeRegistry,
    value_types: &ValueTypes,
    opts: &mut CookOpts,
) {
    graph.rebuild_index();
    graph.bump_cook_gen();
    let order = top_level_order(graph);
    run_staged(graph, &order, registry, value_types, opts, RunJobs::No);
}

/// Re-evaluate dirty nodes and their downstream dependents (default options).
pub fn cook_dirty(graph: &mut Graph, registry: &NodeTypeRegistry, value_types: &ValueTypes) {
    let cancel = CancelToken::default();
    let mut opts = CookOpts::new(&cancel, noop_progress());
    cook_dirty_with(graph, registry, value_types, &mut opts);
}

/// Re-evaluate dirty nodes and their downstream dependents with host [`CookOpts`].
pub fn cook_dirty_with(
    graph: &mut Graph,
    registry: &NodeTypeRegistry,
    value_types: &ValueTypes,
    opts: &mut CookOpts,
) {
    graph.rebuild_index();
    graph.bump_cook_gen();
    let dirty: Vec<NodeId> = graph
        .nodes
        .iter()
        .filter(|(_, n)| n.dirty())
        .map(|(id, _)| *id)
        .collect();
    if dirty.is_empty() {
        return;
    }
    let mut affected = HashSet::new();
    for id in dirty {
        collect_downstream(graph, id, &mut affected);
        // A dirty node inside a subnet invalidates the containing subnet's output: the host's
        // cooked "out" is produced by `run_subnet_prestage` from the children, but there is NO
        // data edge child→host, so `collect_downstream` alone never reaches the host. Ascend to
        // the top-level ancestor and pull it (and its own downstream) into `affected` so
        // re-cooking the ancestor re-runs its prestage over the whole interior. Without this,
        // editing an inner child leaves the host Subnet's "out" stale until a full `cook`.
        let anc = top_level_ancestor(graph, id);
        if anc != id {
            collect_downstream(graph, anc, &mut affected);
        }
    }
    let order: Vec<NodeId> = top_level_order(graph)
        .into_iter()
        .filter(|id| affected.contains(id))
        .collect();
    run_staged(graph, &order, registry, value_types, opts, RunJobs::No);
}

/// Is `node` a **stale job** — a [`NodeKind::Job`] that needs running (never executed, or its inputs
/// changed since it last ran)?
///
/// The driver will never run it (that is the point), so a host uses this to SHOW it: tint the node,
/// enable an Execute button. A stale job is not an error — it is a graph waiting for a human.
pub fn is_stale(node: &Node, registry: &NodeTypeRegistry) -> bool {
    registry.kind_of(node) == NodeKind::Job && node.dirty()
}

/// Every [`NodeKind::Job`] in `graph`, in cook (topological) order — what a pipeline driver iterates.
pub fn jobs_in_order(graph: &Graph, registry: &NodeTypeRegistry) -> Vec<NodeId> {
    graph
        .index
        .topo_order(&graph.nodes)
        .into_iter()
        .filter(|id| {
            graph
                .nodes
                .get(id)
                .map(|n| registry.kind_of(n) == NodeKind::Job)
                .unwrap_or(false)
        })
        .collect()
}

/// **Execute ONE job** (default options): cook its cheap dataflow dependencies, then run the job
/// itself — the explicit counterpart of [`cook`], and the ONLY way a [`NodeKind::Job`] ever runs.
pub fn execute(
    graph: &mut Graph,
    node: NodeId,
    registry: &NodeTypeRegistry,
    value_types: &ValueTypes,
) {
    let cancel = CancelToken::default();
    let mut opts = CookOpts::new(&cancel, noop_progress());
    execute_with(graph, node, registry, value_types, &mut opts);
}

/// [`execute`] with host [`CookOpts`] (cancellation, progress, per-node status observation) — which a
/// job, being the long-running thing in the graph, is exactly what they exist for.
///
/// The dataflow dependencies are cooked first (they are cheap, and the job needs their values), then
/// the job runs through the SAME `eval_one` machinery as every other node — same status stamping, same
/// cancellation, same panic isolation. The only difference is that jobs are permitted in this pass.
///
/// It runs the job **regardless of dirtiness**: the caller pointed at this node and said run it, which
/// is also how you deliberately re-do a job whose artifacts you want rebuilt. (`execute_all`, by
/// contrast, only runs what is pending — see there.)
pub fn execute_with(
    graph: &mut Graph,
    node: NodeId,
    registry: &NodeTypeRegistry,
    value_types: &ValueTypes,
    opts: &mut CookOpts,
) {
    graph.rebuild_index();
    graph.bump_cook_gen();

    // Cook only what this job DEPENDS ON — not the whole graph. Its dependencies are dataflow (cheap,
    // and another job among them is still skipped, so this can never cascade into an unasked-for
    // reconstruction), while its CONSUMERS must not be cooked yet: they would run before the job
    // produced anything and fail with "upstream job has not been executed", which is true but useless
    // noise a moment before it stops being true.
    let mut deps = HashSet::new();
    collect_upstream(graph, node, &mut deps);
    deps.remove(&node);
    let dep_order: Vec<NodeId> = top_level_order(graph)
        .into_iter()
        .filter(|id| deps.contains(id))
        .collect();
    run_staged(graph, &dep_order, registry, value_types, opts, RunJobs::No);
    if opts.cancel.is_cancelled() {
        return;
    }

    // The job itself — the one pass in which a job is permitted to run.
    eval_one(graph, node, registry, value_types, opts, RunJobs::Yes);

    // Its outputs just changed, so everything downstream is out of date. `eval_one` cleans the node it
    // ran but does not propagate, and the consumers may well be CLEAN from an earlier pass — without
    // this they would keep serving values computed from the job's previous (or absent) output.
    let mut downstream = HashSet::new();
    collect_downstream(graph, node, &mut downstream);
    downstream.remove(&node);
    for id in downstream {
        if let Some(n) = graph.nodes.get_mut(&id) {
            n.set_dirty(true);
        }
    }
}

/// Everything `id` transitively depends on (following input wires upstream), `id` included.
fn collect_upstream(graph: &Graph, id: NodeId, out: &mut HashSet<NodeId>) {
    if !out.insert(id) {
        return; // already visited — also what keeps a malformed cycle from looping forever
    }
    let Some(node) = graph.nodes.get(&id) else {
        return;
    };
    for input in node.input_names() {
        if let Some(edge) = graph.index.in_edge(id, &input) {
            collect_upstream(graph, edge.from_node, out);
        }
    }
}

/// **Execute every PENDING job** in topological order (default options) — what a pipeline driver does.
pub fn execute_all(graph: &mut Graph, registry: &NodeTypeRegistry, value_types: &ValueTypes) {
    let cancel = CancelToken::default();
    let mut opts = CookOpts::new(&cancel, noop_progress());
    execute_all_with(graph, registry, value_types, &mut opts);
}

/// [`execute_all`] with host [`CookOpts`].
///
/// Runs each job that is **stale** ([`is_stale`]: never executed, or out of date because its inputs
/// changed), in topological order, re-cooking the dataflow between them so a job downstream of a job
/// sees fresh inputs. A job that is already up to date is SKIPPED — its artifacts exist on disk and
/// re-doing 40 minutes of GPU work to reproduce them is precisely what this whole model prevents. To
/// force one anyway, point [`execute_with`] at it.
pub fn execute_all_with(
    graph: &mut Graph,
    registry: &NodeTypeRegistry,
    value_types: &ValueTypes,
    opts: &mut CookOpts,
) {
    for job in jobs_in_order(graph, registry) {
        if opts.cancel.is_cancelled() {
            return;
        }
        let stale = graph
            .nodes
            .get(&job)
            .map(|n| is_stale(n, registry))
            .unwrap_or(false);
        if !stale {
            continue;
        }
        // Re-cooks the dataflow feeding THIS job (including anything the previous job just produced),
        // then runs it.
        execute_with(graph, job, registry, value_types, opts);
    }
    // Final dataflow pass so whatever consumes the last job's outputs is up to date.
    cook_dirty_with(graph, registry, value_types, opts);
}

/// Mark `node_id` and everything downstream dirty.
///
/// If `node_id` lives inside a subnet, its top-level ancestor (and that ancestor's downstream) is
/// dirtied too, because the ancestor's cooked output depends on its children via the prestage
/// (see [`cook_dirty`] for the same reasoning).
pub fn mark_dirty(graph: &mut Graph, node_id: NodeId) {
    graph.rebuild_index();
    let mut affected = HashSet::new();
    collect_downstream(graph, node_id, &mut affected);
    let anc = top_level_ancestor(graph, node_id);
    if anc != node_id {
        collect_downstream(graph, anc, &mut affected);
    }
    for id in affected {
        if let Some(node) = graph.nodes.get_mut(&id) {
            node.set_dirty(true);
        }
    }
}

// ---------------------------------------------------------------------------
// Ordering
// ---------------------------------------------------------------------------

/// May this pass execute [`NodeKind::Job`] nodes? [`RunJobs::No`] for every automatic cook,
/// [`RunJobs::Yes`] only for the explicit [`execute`] / [`execute_all`] entry points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunJobs {
    No,
    Yes,
}

/// Run `order` in two stages ([`CookStage::Main`] then [`CookStage::Late`]), bailing out early if
/// the pass is cancelled.
fn run_staged(
    graph: &mut Graph,
    order: &[NodeId],
    registry: &NodeTypeRegistry,
    value_types: &ValueTypes,
    opts: &mut CookOpts,
    run_jobs: RunJobs,
) {
    let (main, late) = split_stages(graph, registry, order);
    for id in main.into_iter().chain(late) {
        if opts.cancel.is_cancelled() {
            break;
        }
        eval_one(graph, id, registry, value_types, opts, run_jobs);
    }
}

/// The error a node must fail with when one of its connected inputs comes from a [`NodeKind::Job`]
/// that has never been executed (its output carries no value). `None` when every input is fine.
///
/// Without this the node would cook happily: `resolve_inputs` treats an absent/`Null` source output as
/// "no data yet" and leaves the consumer's STORED LITERAL in place, so a `Mapper` that never ran would
/// feed a downstream node its default — a plausible number that means nothing. Fail honestly instead.
fn unexecuted_job_input(
    graph: &Graph,
    id: NodeId,
    registry: &NodeTypeRegistry,
) -> Option<CookError> {
    let node = graph.nodes.get(&id)?;
    for input in node.input_names() {
        let Some(edge) = graph.index.in_edge(id, &input) else {
            continue;
        };
        let Some(src) = graph.nodes.get(&edge.from_node) else {
            continue;
        };
        if registry.kind_of(src) != NodeKind::Job {
            continue;
        }
        let produced = !matches!(
            src.output_value(&edge.from_output),
            None | Some(BoxValue::Null)
        );
        if !produced {
            return Some(CookError::Custom(format!(
                "input '{input}' comes from job '{}' ({}), which has not been executed — run it \
                 (execute) before cooking what depends on it",
                src.name(),
                src.type_name()
            )));
        }
    }
    None
}

/// Stamp `err` on a node the driver refused to cook, exactly as a failing cook would be stamped
/// (`NodeStatus::Error`, observer notified, node left DIRTY so a later pass retries it once the cause
/// is fixed). Keeps "refused" and "failed" indistinguishable to a host — both are honest failures.
fn fail_node(graph: &mut Graph, id: NodeId, type_name: &str, err: CookError, opts: &mut CookOpts) {
    let Some(node) = graph.nodes.get_mut(&id) else {
        return;
    };
    let status = NodeStatus::Error(err.to_string());
    log::error!("cook: node '{}' ({type_name}): {}", node.name(), err);
    node.set_status(status.clone());
    if let Some(observer) = opts.observer.as_deref_mut()
        && let Some(node) = graph.nodes.get(&id)
    {
        observer(node, &status);
    }
}

/// Topological order of top-level nodes (those without a parent).
fn top_level_order(graph: &Graph) -> Vec<NodeId> {
    let full = graph.index.topo_order(&graph.nodes);
    full.into_iter()
        .filter(|id| {
            graph
                .nodes
                .get(id)
                .map(|n| n.parent().is_none())
                .unwrap_or(false)
        })
        .collect()
}

/// Walk the parent chain from `id` to its top-level ancestor (the first node whose `parent()` is
/// `None`). Returns `id` itself when it is already top-level or its parent is missing. Guards
/// against a malformed parent cycle via a visited set so it always terminates.
fn top_level_ancestor(graph: &Graph, id: NodeId) -> NodeId {
    let mut cur = id;
    let mut seen = HashSet::new();
    while let Some(parent) = graph.nodes.get(&cur).and_then(|n| n.parent()) {
        if !seen.insert(cur) {
            break; // parent cycle — bail out rather than loop forever
        }
        cur = parent;
    }
    cur
}

/// Partition `order` by each node type's declared [`CookStage`] (unregistered types → `Main`).
fn split_stages(
    graph: &Graph,
    registry: &NodeTypeRegistry,
    order: &[NodeId],
) -> (Vec<NodeId>, Vec<NodeId>) {
    let mut main = Vec::new();
    let mut late = Vec::new();
    for &id in order {
        let stage = graph
            .nodes
            .get(&id)
            .and_then(|n| registry.node_type(n.type_name()))
            .map(|ty| ty.stage())
            .unwrap_or_default();
        match stage {
            CookStage::Main => main.push(id),
            CookStage::Late => late.push(id),
        }
    }
    (main, late)
}

// ---------------------------------------------------------------------------
// Input resolution
// ---------------------------------------------------------------------------

/// Resolve connected inputs of `id` into a transient override map.
///
/// - Connected inputs are pulled from the source node's output value and converted to the
///   consumer's declared input type through [`ValueTypes::convert`] (the type SSOT — a HOST type
///   coerces by its own registered rule).
/// - On conversion failure a warning is logged and the entry is **not** added — the cook closure
///   sees the stored literal instead (spec §3.8 rejection rule).
/// - Unconnected inputs are not added; the closure reads the stored literal.
fn resolve_inputs(
    graph: &Graph,
    id: NodeId,
    registry: &NodeTypeRegistry,
    value_types: &ValueTypes,
) -> crate::box_model::BoxMap {
    // A `BoxMap` rather than a `HashMap`, because that is what it becomes: this
    // used to build a HashMap that `set_compute` immediately re-collected into
    // a BTreeMap, hashing and reallocating every key on every node on every
    // tick to produce the same entries in the same order.
    let mut overrides = crate::box_model::BoxMap::new();
    let Some(node) = graph.nodes.get(&id) else {
        return overrides;
    };
    let def = registry.get(node.type_name());
    for input in node.input_names() {
        let Some(edge) = graph.index.in_edge(id, &input) else {
            continue; // unconnected → keep stored value
        };
        let Some(src) = graph.nodes.get(&edge.from_node) else {
            continue;
        };
        let Some(src_val) = src.output_value(&edge.from_output) else {
            continue;
        };
        // A `Null` source output is ABSENT data ("not produced yet"), not a type mismatch — e.g. a
        // `LoadImage` with no path yet emits `Null` on its `Image` output. Treat it like the
        // no-value case above (keep the consumer's stored literal) instead of running it through
        // the type check, which would reject `Null`→`Image` and warn on EVERY cook pass.
        if matches!(src_val, BoxValue::Null) {
            continue;
        }
        // BOTH port types are resolved BEFORE the decision. The source type used to be fetched only
        // inside the failure arm, which is precisely why `convert` could not see it.
        //
        // A missing def or a missing port means VERSION SKEW (a graph saved by a newer lib; a type
        // the host never registered). That used to degrade to `"Any"`, which forwards ANYTHING into
        // the port — the silent-wrong-data outcome again. Refuse the value and say why.
        let Some(target_ty) = def.and_then(|d| d.input_type(&input)) else {
            log::warn!(
                "cook: node '{}' ({}) declares no input port '{input}', but a wire feeds it: its \
                 type is unknown, so the incoming value is REFUSED (stored literal kept). \
                 Version skew?",
                node.name(),
                node.type_name(),
            );
            continue;
        };
        let Some(src_ty) = registry
            .get(src.type_name())
            .and_then(|d| d.output_type(&edge.from_output))
        else {
            log::warn!(
                "cook: source node '{}' ({}) declares no output port '{}', so the type of the value \
                 it feeds into '{input}' is unknown: REFUSED (stored literal kept). Version skew?",
                src.name(),
                src.type_name(),
                edge.from_output,
            );
            continue;
        };

        // The connect-time gate, re-applied at cook. This is what closes the DESERIALIZED-wire hole:
        // a graph loaded from JSON never re-runs `Graph::connect`, so this is the only place an
        // incompatible saved wire is ever caught.
        if !value_types.compatible(src_ty, target_ty) {
            log::warn!(
                "cook: incompatible wire into '{input}': source={src_ty} consumer_wants={target_ty}; \
                 keeping stored literal"
            );
            continue;
        }
        match value_types.convert(&src_val, src_ty, target_ty) {
            Some(v) => {
                overrides.insert(input, v);
            }
            None => {
                // Reachable even when `compatible` said yes: an `Any` source is judged by its VALUE,
                // which only exists here (see `ValueTypes::compatible`). A Float riding an `Any`
                // wire into an identity-typed port lands exactly here — and is refused.
                log::warn!(
                    "cook: value on input '{input}' is not a valid '{target_ty}' (source={src_ty}, \
                     got {:?}); REFUSED, keeping stored literal",
                    src_val.kind(),
                );
            }
        }
    }
    overrides
}

/// Live incoming value of a single connected input `input` on node `id`: follow its wire to the
/// source node's output and convert to the consumer's declared input type (same rule
/// `resolve_inputs` applies during cook). Returns `None` when the input is unconnected, the
/// source/output is missing, or the source has not produced a value.
///
/// Unlike `resolve_inputs` this reads the wire straight from node data ([`Node::source_of`])
/// rather than the cook edge index, so it is safe to call for **display outside a cook pass**,
/// when `graph.index` may be stale. The stored `inputs.<name>.value` literal is never mutated.
///
/// On an incompatible wire it falls back to the STORED LITERAL — the same value the cook closure
/// would actually see (spec §3.8 rejection) — and never to the raw source value. Showing the
/// rejected value would contradict the rule this very function documents, and would display a
/// number the node does not compute with. An unknown port type is treated as incompatible for the
/// same reason `resolve_inputs` does (version skew must not read as "Any").
pub fn resolved_input_value(
    graph: &Graph,
    id: NodeId,
    input: &str,
    registry: &NodeTypeRegistry,
    value_types: &ValueTypes,
) -> Option<BoxValue> {
    let node = graph.nodes.get(&id)?;
    let (src_id, src_attr_uuid) = node.source_of(input)?;
    let src = graph.nodes.get(&src_id)?;
    let out_name = src.attr_name_by_id(&src_attr_uuid)?;
    let src_val = src.output_value(&out_name)?;

    let converted = registry
        .get(node.type_name())
        .and_then(|d| d.input_type(input))
        .zip(
            registry
                .get(src.type_name())
                .and_then(|d| d.output_type(&out_name)),
        )
        .and_then(|(target_ty, src_ty)| value_types.convert(&src_val, src_ty, target_ty));

    converted.or_else(|| node.input_value(input))
}

// ---------------------------------------------------------------------------
// The per-node driver
// ---------------------------------------------------------------------------

/// Cook exactly one node: resolve its inputs, detach it, run its (optional) subnet prestage and
/// its cook closure inside a [`CookCtx`], then stamp its status and reinsert it.
///
/// The node is REMOVED from `graph.nodes` for the duration of the cook — that is what gives the
/// closure a `&mut Node` alongside a read-only view of everything else. The bus is reached by
/// **disjoint field borrows** of `Graph` (`nodes`/`index` shared, `bus` unique), never by taking
/// it out of the graph: an absent bus is impossible by type, and a panic cannot lose it.
fn eval_one(
    graph: &mut Graph,
    id: NodeId,
    registry: &NodeTypeRegistry,
    value_types: &ValueTypes,
    opts: &mut CookOpts,
    run_jobs: RunJobs,
) {
    if opts.cancel.is_cancelled() {
        return;
    }
    // The type's BEHAVIOUR comes from the registry entry (an `Arc<dyn NodeType>`),
    // cloned out so the driver can hand `&mut Graph` to the context while calling
    // it. Taken here, while the node is still borrowed, so the type NAME does not
    // have to be copied into a `String` on every node on every tick just to
    // survive the borrow — only the error paths below need it as an owned string.
    let (node_type, type_name, kind, bypass) = match graph.nodes.get(&id) {
        Some(n) => {
            let name = n.type_name();
            let (Some(def), Some(ty)) = (registry.get(name), registry.node_type(name).cloned())
            else {
                return;
            };
            // The registered def already owns the name as an `Arc<str>` and the
            // kind the registry stamped on it, so this costs a refcount bump
            // where it used to copy the name into a fresh `String` and take a
            // third hash lookup for the kind — on every node on every tick.
            (ty, Arc::clone(&def.type_name), def.kind, n.bypass())
        }
        None => return,
    };
    if bypass {
        pass_through(graph, id, registry, value_types);
        return;
    }
    // THE MODEL GUARD (see `NodeKind`). An automatic cook NEVER runs a Job, however dirty it is: a
    // Job's cook DOES something (40 minutes of GPU, artifacts on disk) and re-doing it is neither
    // free nor even equivalent. The node simply stays dirty — i.e. STALE (`is_stale`) — so a host can
    // show it as out-of-date and a human presses Execute.
    //
    // This is the ONE place any node is evaluated (top-level nodes AND subnet children both arrive
    // here via `run_staged`), so the guard covers a Job buried inside a compound for free.
    if kind == NodeKind::Job && run_jobs == RunJobs::No {
        return;
    }
    // A dataflow node fed by a Job that has never run must FAIL, not quietly compute on the stored
    // literal that `resolve_inputs` would fall back to (a `Null` source is treated as "absent data").
    // Silently computing on nothing is how a graph shows a plausible number that means nothing.
    if let Some(err) = unexecuted_job_input(graph, id, registry) {
        fail_node(graph, id, &type_name, err, opts);
        return;
    }
    // Build transient cook-input overrides without mutating the stored attrs.
    let cook_inputs = resolve_inputs(graph, id, registry, value_types);

    // Detach the node so the closure can borrow the rest of the graph.
    let Some(mut node) = graph.nodes.remove(&id) else {
        return;
    };
    node.set_compute(cook_inputs);

    let cooking = NodeStatus::Cooking {
        progress: 0.0,
        msg: String::new(),
    };
    node.set_status(cooking.clone());
    if let Some(observer) = opts.observer.as_deref_mut() {
        observer(&node, &cooking);
    }

    let started = Instant::now();
    // Run the prestage + cook isolated: a panicking cook closure must never leak the detached
    // node out of the graph (it is always reinserted below) — and a panic is reported as an
    // Error status instead of being logged and pretended away.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Pre-stage: if this node has children, run the inner subgraph and stash its output bag
        // before the node's own cook closure sees it.
        run_subnet_prestage(graph, &mut node, registry, value_types, opts, run_jobs);
        // Disjoint field borrows: `nodes`/`index` shared into the view, `bus` unique into the ctx.
        // Start this node's background worker the first time it cooks. The
        // node owns it from here: `Graph::remove_node` stops it, and dropping
        // the graph stops whatever is left.
        if opts.workers == WorkerPolicy::Background && !graph.workers.has(id) {
            node_type.start_worker(&node, &mut graph.workers);
        }
        let Graph {
            nodes,
            index,
            bus,
            workers,
            ..
        } = &mut *graph;
        let mut ctx = CookCtx {
            node: &mut node,
            graph: GraphView {
                nodes: &*nodes,
                index: &*index,
            },
            registry,
            types: value_types,
            bus,
            workers: Some(&*workers),
            cancel: opts.cancel,
            progress: Arc::clone(&opts.progress),
            services: &opts.services,
        };
        node_type.cook(&mut ctx)
    }));

    let produced_output = matches!(outcome, Ok(Ok(())));
    let status = match outcome {
        Err(_) => NodeStatus::Error(format!("panicked: node '{}' ({type_name})", node.name())),
        Ok(Err(CookError::Cancelled)) => NodeStatus::Idle,
        Ok(Err(e)) => NodeStatus::Error(e.to_string()),
        Ok(Ok(())) => NodeStatus::Done {
            elapsed_s: started.elapsed().as_secs_f64(),
        },
    };
    if let NodeStatus::Error(msg) = &status {
        log::error!("cook: node '{}' ({type_name}): {msg}", node.name());
    }
    node.set_status(status.clone());
    if let Some(observer) = opts.observer.as_deref_mut() {
        observer(&node, &status);
    }

    // Clear transient overrides before reinserting.
    node.clear_compute();
    // ONLY a node that actually SUCCEEDED is stamped fresh + clean. A cancelled, errored or
    // panicked node produced no output: stamping `cooked_gen` would lie to the freshness signal
    // live previews consume (`Graph::cook_gen`), and clearing `dirty` would mean `cook_dirty`
    // never retries it — the node would stay stuck showing stale/default outputs until the user
    // happened to edit something.
    if produced_output {
        node.set_cooked_gen(graph.cook_gen());
        node.set_dirty(false);
    }
    graph.nodes.insert(id, node);
}

/// Carry a **bypassed** node's inputs straight to its outputs.
///
/// Bypass used to be "do not run", full stop — which left the outputs holding whatever the last
/// real cook produced. For a node that TRANSFORMS a value (the common case: one `map` in, one
/// `map` out, as every cryptobot stage is) that is not "switch this step off", it is "freeze this
/// step's last answer and keep publishing it". Downstream then rebuilds from a snapshot of the
/// past, every tick, and looks like it is running while never advancing — the same failure the
/// driver already logs loudly for an ERRORED node.
///
/// So a bypassed node now behaves the way Houdini and Nuke behave: the data flows through it
/// untouched.
///
/// **Pairing rule: by TYPE, in order.** Each output takes the first not-yet-used input of the same
/// port type. With one input and one output — nearly every node — that is simply "in becomes out".
/// An output with no matching input is left alone: there is nothing honest to put there, and
/// inventing a default would be a fabricated value rather than a missing one.
///
/// The values come from [`resolve_inputs`], i.e. what actually arrived ON THE WIRES this tick, not
/// the literals stored on the node — otherwise bypass would republish stale attributes, which is
/// the very bug this replaces.
fn pass_through(
    graph: &mut Graph,
    id: NodeId,
    registry: &NodeTypeRegistry,
    value_types: &ValueTypes,
) {
    let Some(def) = graph
        .nodes
        .get(&id)
        .and_then(|n| registry.get(n.type_name()))
    else {
        return;
    };
    // Pair first, while the registry entry is borrowed; then write.
    let mut taken = vec![false; def.inputs.len()];
    let mut pairs: Vec<(String, String)> = Vec::new();
    for out in &def.outputs {
        let matching = def
            .inputs
            .iter()
            .enumerate()
            .position(|(i, inp)| !taken[i] && inp.port_type == out.port_type);
        if let Some(i) = matching {
            taken[i] = true;
            pairs.push((out.name.clone(), def.inputs[i].name.clone()));
        }
    }
    if pairs.is_empty() {
        return;
    }

    let resolved = resolve_inputs(graph, id, registry, value_types);
    let Some(node) = graph.nodes.get_mut(&id) else {
        return;
    };
    for (out_name, in_name) in pairs {
        // The wire value if something is connected, else the node's own stored input.
        let value = resolved
            .get(in_name.as_str())
            .cloned()
            .or_else(|| node.input_value(&in_name));
        if let Some(value) = value {
            node.set_out(&out_name, value);
        }
    }
    // Reported as a finished cook, because for everything downstream it IS one: the outputs are
    // current. Leaving it dirty would have `cook_dirty` reconsider it on every pass forever.
    node.set_status(NodeStatus::Done { elapsed_s: 0.0 });
    node.set_dirty(false);
}

/// Pre-stage for any node that has children (i.e. acts as a subnet container).
///
/// Runs BEFORE the node's own cook closure so the cook can read the inner subgraph result via
/// `cook_value("__subnet_out__")`.
///
/// Flow:
/// 1. If no children → return immediately (no-op; node cooks normally).
/// 2. Pack the node's resolved inputs into a `BoxValue::Object` bag.
/// 3. Find the child `SubnetInput`; set its "bag" output to that packed bag and mark it clean (so
///    it is not re-cooked by the topo pass).
/// 4. Topo-cook all other children via [`eval_one`] (recurses for nested subnets).
/// 5. Find the child `SubnetOutput`; follow its "bag" input's source edge to the actual cooked
///    value, defaulting to an empty Object if unconnected / absent.
/// 6. Stash the resulting bag in `node.data.compute.__subnet_out__` so the node's own cook closure
///    can read it via `cook_value("__subnet_out__")`.
fn run_subnet_prestage(
    graph: &mut Graph,
    node: &mut Node,
    registry: &NodeTypeRegistry,
    value_types: &ValueTypes,
    opts: &mut CookOpts,
    run_jobs: RunJobs,
) {
    let subnet_id = node.id;
    let children = graph.children_of(subnet_id);

    // No children → this node is not acting as a subnet; skip entirely.
    if children.is_empty() {
        return;
    }

    // Build an Object bag from the node's resolved inputs (data.compute already populated by the
    // driver before calling us).
    let in_bag = {
        let mut entries = std::collections::BTreeMap::new();
        for name in node.input_names() {
            let val = node.cook_value(&name).unwrap_or(BoxValue::Null);
            entries.insert(name, val);
        }
        BoxValue::obj(entries)
    };

    // Locate the boundary nodes. A well-formed subnet has EXACTLY one SubnetInput and one
    // SubnetOutput. We sort by uuid so the chosen node is deterministic (children come from a
    // HashMap bucket), and warn — rather than silently paper over — a malformed topology so
    // authoring mistakes surface instead of producing a plausible-but-wrong bag (project rule: no
    // silent fallback as a crutch).
    let collect_kind = |kind: &str| -> Vec<NodeId> {
        let mut v: Vec<NodeId> = children
            .iter()
            .copied()
            .filter(|c| {
                graph
                    .nodes
                    .get(c)
                    .map(|n| n.type_name() == kind)
                    .unwrap_or(false)
            })
            .collect();
        v.sort_by_key(|id| id.uuid());
        v
    };
    let inputs = collect_kind("SubnetInput");
    let outputs = collect_kind("SubnetOutput");
    if inputs.is_empty() {
        log::warn!(
            "subnet '{}': has children but no SubnetInput — inner nodes will not \
             receive the input bag",
            node.name()
        );
    } else if inputs.len() > 1 {
        log::warn!(
            "subnet '{}': {} SubnetInput nodes — only the first (by id) is seeded",
            node.name(),
            inputs.len()
        );
    }
    if outputs.is_empty() {
        log::warn!(
            "subnet '{}': has children but no SubnetOutput — output bag will be empty",
            node.name()
        );
    } else if outputs.len() > 1 {
        log::warn!(
            "subnet '{}': {} SubnetOutput nodes — only the first (by id) is read",
            node.name(),
            outputs.len()
        );
    }
    let subnet_input_id: Option<NodeId> = inputs.first().copied();

    // Seed SubnetInput's "bag" output; mark it clean so the topo pass skips it.
    if let Some(input_id) = subnet_input_id
        && let Some(input_node) = graph.nodes.get_mut(&input_id)
    {
        input_node.set_out("bag", in_bag);
        input_node.set_dirty(false);
    }

    // Topo-cook all children except SubnetInput (already seeded above), in the SAME two stages the
    // top level uses: every `Main` child first, then every `Late` one. Without this split the
    // `CookStage::Late` contract ("runs after every Main node, so it sees every bus write of this
    // pass") would silently be false inside a subnet — a `BusRead` parented into a compound would
    // read the PREVIOUS pass's slot value (or Null on the first cook).
    // Jobs inside a compound follow the SAME rule as jobs at the top level: an automatic cook skips
    // them; an explicit `execute` of the compound runs them (that is what the caller asked for).
    let children_to_cook = topo_sort_children(graph, &children, subnet_input_id);
    run_staged(
        graph,
        &children_to_cook,
        registry,
        value_types,
        opts,
        run_jobs,
    );

    // Read SubnetOutput's "bag" input by following its source edge to the cooked producer's output
    // value. Fall back to empty Object if unconnected / absent.
    let out_bag: BoxValue = outputs
        .first()
        .copied()
        .and_then(|output_id| {
            let output_node = graph.nodes.get(&output_id)?;
            if let Some((src_node, src_attr_uuid)) = output_node.source_of("bag") {
                // Follow source edge → read the cooked output value directly.
                let src_out_name = graph
                    .nodes
                    .get(&src_node)
                    .and_then(|n| n.attr_name_by_id(&src_attr_uuid))?;
                graph
                    .nodes
                    .get(&src_node)
                    .and_then(|n| n.output_value(&src_out_name))
            } else {
                // Unconnected SubnetOutput "bag": use its stored literal.
                output_node.input_value("bag")
            }
        })
        .filter(|v| v.is_object())
        .unwrap_or_else(BoxValue::object);

    // Stash for the node's own cook to read via cook_value("__subnet_out__").
    node.insert_compute("__subnet_out__", out_bag);
}

/// Topological sort of `children` (Kahn), using only edges internal to the child set.
/// Excludes `subnet_input_id` from the result (already seeded; should not be re-cooked).
/// Reads sources directly from node data — safe when the parent Subnet node is temporarily absent
/// from `graph.nodes`.
fn topo_sort_children(
    graph: &Graph,
    children: &[NodeId],
    subnet_input_id: Option<NodeId>,
) -> Vec<NodeId> {
    let child_set: HashSet<NodeId> = children.iter().copied().collect();

    // in_degree[n] = number of inputs of n that are sourced from another child.
    let mut in_degree: HashMap<NodeId, usize> = children.iter().map(|id| (*id, 0)).collect();
    // forward_edges[src] = list of consumers (child_id) that have src as a source.
    let mut forward_edges: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

    for &id in children {
        let Some(node) = graph.nodes.get(&id) else {
            continue;
        };
        for input in node.input_names() {
            let Some((src, _)) = node.source_of(&input) else {
                continue;
            };
            if child_set.contains(&src) {
                *in_degree.entry(id).or_default() += 1;
                forward_edges.entry(src).or_default().push(id);
            }
        }
    }

    // SubnetInput is pre-seeded (not cooked by us), so decrement in-degrees of its consumers as if
    // it had already been processed — this unblocks nodes like MapGet that source from SubnetInput.
    if let Some(si_id) = subnet_input_id
        && let Some(consumers) = forward_edges.get(&si_id)
    {
        for &cid in consumers {
            if let Some(d) = in_degree.get_mut(&cid) {
                *d = d.saturating_sub(1);
            }
        }
    }

    // Start queue with degree-0 nodes, excluding SubnetInput.
    let mut queue: Vec<NodeId> = in_degree
        .iter()
        .filter(|(id, d)| **d == 0 && Some(**id) != subnet_input_id)
        .map(|(id, _)| *id)
        .collect();
    queue.sort_by_key(|id| id.uuid());

    let mut result = Vec::with_capacity(children.len());
    let mut head = 0;
    while head < queue.len() {
        let id = queue[head];
        head += 1;
        result.push(id);
        let mut newly_ready: Vec<NodeId> = Vec::new();
        if let Some(consumers) = forward_edges.get(&id) {
            for &cid in consumers {
                if Some(cid) == subnet_input_id {
                    continue;
                }
                if let Some(d) = in_degree.get_mut(&cid) {
                    *d = d.saturating_sub(1);
                    if *d == 0 {
                        newly_ready.push(cid);
                    }
                }
            }
        }
        newly_ready.sort_by_key(|id| id.uuid());
        queue.extend(newly_ready);
    }
    // Append any cycle orphans.
    let seen: HashSet<NodeId> = result.iter().copied().collect();
    let mut leftovers: Vec<NodeId> = children
        .iter()
        .copied()
        .filter(|id| !seen.contains(id) && Some(*id) != subnet_input_id)
        .collect();
    leftovers.sort_by_key(|id| id.uuid());
    result.extend(leftovers);
    result
}

fn collect_downstream(graph: &Graph, start: NodeId, out: &mut HashSet<NodeId>) {
    let mut stack = vec![start];
    while let Some(id) = stack.pop() {
        if !out.insert(id) {
            continue;
        }
        for (to, _input) in graph.index.out_edges(id) {
            stack.push(*to);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::box_model::BoxValue;
    use crate::graph_container::Graph;
    use crate::node::NodeDef;
    use crate::node_type::{NodeLibrary, NodeType, fn_node, fn_node_staged};
    use crate::port::{PinVisual, PortDef};
    use crate::prelude::any;
    use crate::value_types::{ValueTypeInfo, ValueTypes};

    fn setup() -> (Graph, NodeTypeRegistry, ValueTypes) {
        let (reg, types) = crate::default_registry();
        (Graph::new(), reg, types)
    }

    /// Full subnet pipeline:
    ///   Subnet S (input "in" = F64(1.0))
    ///   Children: SubnetInput SI, MapGet MG (key="in"), Add A (b=1.0),
    ///             MapInsert MI (key="r"), SubnetOutput SO
    ///   Wires: SI.bag → MG.map ; MG.value → A.a ; A.result → MI.value ;
    ///          (MI.map left default = empty Object) ; MI.out → SO.bag
    ///   Expected: S output "out" == Object { "r": F64(2.0) }
    #[test]
    fn subnet_bag_pipeline() {
        let (mut graph, reg, types) = setup();

        // Create Subnet at top level.
        let s_id = graph
            .create_node("Subnet", [0.0, 0.0], &reg, &types)
            .unwrap();

        // Set Subnet input "in" literal to 1.0.
        graph
            .nodes
            .get_mut(&s_id)
            .unwrap()
            .set_input_value("in", BoxValue::F64(1.0));

        // Create children (parented to S).
        let si_id = graph
            .create_node("SubnetInput", [0.0, 0.0], &reg, &types)
            .unwrap();
        let mg_id = graph
            .create_node("MapGet", [0.0, 0.0], &reg, &types)
            .unwrap();
        let a_id = graph.create_node("Add", [0.0, 0.0], &reg, &types).unwrap();
        let mi_id = graph
            .create_node("MapInsert", [0.0, 0.0], &reg, &types)
            .unwrap();
        let so_id = graph
            .create_node("SubnetOutput", [0.0, 0.0], &reg, &types)
            .unwrap();

        graph.set_parent(si_id, Some(s_id));
        graph.set_parent(mg_id, Some(s_id));
        graph.set_parent(a_id, Some(s_id));
        graph.set_parent(mi_id, Some(s_id));
        graph.set_parent(so_id, Some(s_id));

        // MapGet key = "in" (reads the "in" field from the bag).
        graph
            .nodes
            .get_mut(&mg_id)
            .unwrap()
            .set_input_value("key", BoxValue::String("in".to_string()));

        // MapInsert key = "r" (writes output under key "r").
        graph
            .nodes
            .get_mut(&mi_id)
            .unwrap()
            .set_input_value("key", BoxValue::String("r".to_string()));

        // Add node: set literal b = 1.0 (adds 1 to whatever MG yields).
        graph
            .nodes
            .get_mut(&a_id)
            .unwrap()
            .set_input_value("b", BoxValue::F64(1.0));

        // Wire: SI.bag → MG.map
        graph
            .connect(si_id, "bag", mg_id, "map", &reg, &types)
            .unwrap();
        // Wire: MG.value → A.a
        graph
            .connect(mg_id, "value", a_id, "a", &reg, &types)
            .unwrap();
        // Wire: A.result → MI.value  (MI.map left unconnected = empty Object default)
        graph
            .connect(a_id, "result", mi_id, "value", &reg, &types)
            .unwrap();
        // Wire: MI.out → SO.bag
        graph
            .connect(mi_id, "out", so_id, "bag", &reg, &types)
            .unwrap();

        // Cook the whole graph.
        cook(&mut graph, &reg, &types);

        // S output "out" must be Object { "r": F64(2.0) }.
        let out = graph.nodes[&s_id]
            .output_value("out")
            .expect("Subnet 'out' must be set");
        let mut expected = BTreeMap::new();
        expected.insert("r".to_string(), BoxValue::F64(2.0));
        assert_eq!(out, BoxValue::obj(expected), "subnet bag pipeline mismatch");
    }

    /// An empty Subnet (no children) must emit an empty Object on "out".
    #[test]
    fn empty_subnet_emits_empty_object() {
        let (mut graph, reg, types) = setup();
        let s_id = graph
            .create_node("Subnet", [0.0, 0.0], &reg, &types)
            .unwrap();
        cook(&mut graph, &reg, &types);
        let out = graph.nodes[&s_id]
            .output_value("out")
            .expect("Subnet 'out' must be set");
        assert!(out.is_object(), "empty subnet output must be an Object");
        assert_eq!(
            out.as_object().map(|m| m.len()),
            Some(0),
            "empty subnet output must be empty"
        );
    }

    /// A normal non-subnet node (Add, no children) still cooks correctly after the prestage
    /// change — prestage is a no-op when children is empty.
    #[test]
    fn non_subnet_node_cooks_normally() {
        let (mut graph, reg, types) = setup();
        let add_id = graph.create_node("Add", [0.0, 0.0], &reg, &types).unwrap();
        graph
            .nodes
            .get_mut(&add_id)
            .unwrap()
            .set_input_value("a", BoxValue::F64(3.0));
        graph
            .nodes
            .get_mut(&add_id)
            .unwrap()
            .set_input_value("b", BoxValue::F64(4.0));
        cook(&mut graph, &reg, &types);
        let result = graph.nodes[&add_id]
            .output_value("result")
            .expect("Add 'result' must be set");
        assert_eq!(result, BoxValue::F64(7.0), "Add node result mismatch");
    }

    /// THE BLOCKER, end to end through a real cook: an `Any`-typed output wired into an identity
    /// custom port must NOT hand that node a foreign value.
    ///
    /// `Null` is a genuine `Any -> Any` passthrough, so it republishes whatever it is given — here a
    /// `Float`. Before `convert` took the source type, the `PointCloud` consumer received
    /// `F64(3.0)` as its `cloud` input and cooked with it. Now the value is refused and the node
    /// falls back to its stored literal, exactly as any other incompatible wire does.
    #[test]
    fn any_wire_cannot_smuggle_a_foreign_value_into_an_identity_port() {
        use crate::port::{PinVisual, PortDef};
        use crate::value_types::ValueTypeInfo;

        let (_g, mut reg, mut types) = setup();
        // A consumer with a `PointCloud` input that reports what it actually received.
        reg.add(
            fn_node(
                NodeDef::new("TakeCloud", "Debug")
                    .with_inputs(vec![PortDef::typed("cloud", "PointCloud")])
                    .with_outputs(vec![PortDef::float("got_float", -1.0)]),
                |ctx| {
                    let got = ctx.node.cook_value("cloud").and_then(|v| v.as_f64());
                    ctx.node
                        .set_out("got_float", BoxValue::F64(got.unwrap_or(-1.0)));
                    Ok(())
                },
            ),
            &mut types,
        );
        types.register(
            ValueTypeInfo::new("PointCloud", PinVisual::ANY).with_default(BoxValue::buffer(
                crate::box_model::DType::F32,
                vec![3],
                vec![0u8; 12],
            )),
        );

        let mut graph = Graph::new();
        // `Null` is an Any -> Any passthrough: feed it a Float and it publishes a Float on an `Any`
        // output — the dynamically-typed source the blocker relied on.
        let src = graph.create_node("Null", [0.0, 0.0], &reg, &types).unwrap();
        graph
            .nodes
            .get_mut(&src)
            .unwrap()
            .set_input_value("in", BoxValue::F64(3.0));
        let dst = graph
            .create_node("TakeCloud", [200.0, 0.0], &reg, &types)
            .unwrap();
        graph
            .connect(src, "out", dst, "cloud", &reg, &types)
            .expect("Any -> PointCloud connects (the value is unknown until cook)");

        cook(&mut graph, &reg, &types);

        assert_eq!(
            graph.nodes[&dst].output_value("got_float"),
            Some(BoxValue::F64(-1.0)),
            "the PointCloud port must NOT have received the Float riding the Any wire"
        );
    }

    /// A host SERVICE reaches every cook of the pass, keyed by its type rather than by a
    /// cross-crate string (see [`ServiceMap`] / [`CookCtx::service`]).
    #[test]
    fn host_service_reaches_a_cook_by_type() {
        // A service is a newtype in the HOST's crate — which is what gives it its identity.
        struct Solver(u64);

        let (_g, mut reg, mut types) = setup();
        reg.add(
            fn_node(
                NodeDef::new("UseSolver", "Debug").with_outputs(vec![PortDef::float("seen", -1.0)]),
                |ctx| {
                    let seen = ctx.service::<Solver>().map(|s| s.0 as f64).unwrap_or(-1.0);
                    ctx.node.set_out("seen", BoxValue::F64(seen));
                    Ok(())
                },
            ),
            &mut types,
        );

        let mut graph = Graph::new();
        let n = graph
            .create_node("UseSolver", [0.0, 0.0], &reg, &types)
            .unwrap();

        // Without the service installed the cook sees nothing — `None`, not a wrong object.
        cook(&mut graph, &reg, &types);
        assert_eq!(
            graph.nodes[&n].output_value("seen"),
            Some(BoxValue::F64(-1.0))
        );

        let cancel = CancelToken::default();
        let mut opts = CookOpts::new(&cancel, noop_progress()).with_service(Arc::new(Solver(42)));
        mark_dirty(&mut graph, n);
        cook_with(&mut graph, &reg, &types, &mut opts);
        assert_eq!(
            graph.nodes[&n].output_value("seen"),
            Some(BoxValue::F64(42.0)),
            "the cook must reach the host service by TYPE"
        );
    }

    #[test]
    fn cook_generation_bumps_and_stamps_cooked_nodes() {
        let (mut graph, reg, types) = setup();
        let add_id = graph.create_node("Add", [0.0, 0.0], &reg, &types).unwrap();
        assert_eq!(graph.cook_gen(), 0, "fresh graph starts at gen 0");
        assert_eq!(
            graph.nodes[&add_id].cooked_gen(),
            0,
            "never-cooked node reads gen 0"
        );

        cook(&mut graph, &reg, &types);
        let g1 = graph.cook_gen();
        assert_eq!(g1, 1, "first full cook bumps the generation to 1");
        assert_eq!(
            graph.nodes[&add_id].cooked_gen(),
            g1,
            "a cooked node is stamped with the current generation"
        );

        // A no-op cook_dirty (nothing dirty) still bumps the pass counter but re-cooks nothing,
        // so the node keeps its previous stamp — consumers see "not refreshed".
        cook_dirty(&mut graph, &reg, &types);
        assert_eq!(graph.cook_gen(), 2, "each pass advances the generation");
        assert_eq!(
            graph.nodes[&add_id].cooked_gen(),
            g1,
            "an unaffected node is NOT re-stamped by cook_dirty"
        );

        // Editing the node re-cooks it and re-stamps it with the newest generation.
        mark_dirty(&mut graph, add_id);
        cook_dirty(&mut graph, &reg, &types);
        assert_eq!(
            graph.nodes[&add_id].cooked_gen(),
            graph.cook_gen(),
            "a re-cooked node is stamped with the newest generation"
        );
    }

    /// A cook that succeeds records `Done`; one that returns `Err` records `Error` and keeps the
    /// message (the node is NOT silently left looking healthy).
    #[test]
    fn failing_cook_leaves_error_status() {
        let (mut graph, reg, types) = setup();
        let div = graph.create_node("Div", [0.0, 0.0], &reg, &types).unwrap();
        graph
            .nodes
            .get_mut(&div)
            .unwrap()
            .set_input_value("b", BoxValue::F64(0.0));
        cook(&mut graph, &reg, &types);
        match graph.nodes[&div].status() {
            NodeStatus::Error(msg) => assert!(
                msg.contains("division by zero"),
                "error status must carry the cook's reason, got {msg:?}"
            ),
            other => panic!("expected Error status, got {other:?}"),
        }

        // A healthy node reports Done.
        let add = graph.create_node("Add", [0.0, 0.0], &reg, &types).unwrap();
        cook(&mut graph, &reg, &types);
        assert!(
            matches!(graph.nodes[&add].status(), NodeStatus::Done { .. }),
            "a successful cook must report Done"
        );
    }

    /// A cook that observes a cancelled token returns `CookError::Cancelled`: the node stays
    /// `Idle` (NOT an error), stays dirty, and the pass stops without cooking the rest.
    #[test]
    fn cancelled_cook_is_idle_and_stops_the_pass() {
        let (mut graph, reg, types) = setup();
        let a = graph.create_node("Add", [0.0, 0.0], &reg, &types).unwrap();
        let b = graph
            .create_node("Add", [200.0, 0.0], &reg, &types)
            .unwrap();
        // b consumes a, so a cooks first and b would follow.
        graph.connect(a, "result", b, "a", &reg, &types).unwrap();

        // Cancel before the pass even starts: the first node bails out on `check_cancel`.
        let cancel = CancelToken::default();
        cancel.cancel();
        let mut opts = CookOpts::new(&cancel, noop_progress());
        cook_with(&mut graph, &reg, &types, &mut opts);

        assert!(matches!(graph.nodes[&a].status(), NodeStatus::Idle));
        assert!(matches!(graph.nodes[&b].status(), NodeStatus::Idle));
        assert!(graph.nodes[&a].dirty(), "a cancelled node stays dirty");
        assert_eq!(
            graph.nodes[&a].cooked_gen(),
            0,
            "a cancelled node is never stamped"
        );

        // Cancelling from INSIDE a cook (mid-pass) is the same: Idle, dirty, pass stops.
        let mut reg2 = reg.clone();
        let mut types2 = types.clone();
        let stop = CancelToken::default();
        {
            let stop = stop.clone();
            reg2.add(
                fn_node(
                    NodeDef::new("CancelMe", "Debug")
                        .with_display("Cancel Me")
                        .with_outputs(vec![PortDef::float("out", 0.0)]),
                    move |ctx| {
                        stop.cancel();
                        ctx.check_cancel()
                    },
                ),
                &mut types2,
            );
        }
        let mut g2 = Graph::new();
        let c = g2
            .create_node("CancelMe", [0.0, 0.0], &reg2, &types)
            .unwrap();
        let mut opts = CookOpts::new(&stop, noop_progress());
        cook_with(&mut g2, &reg2, &types, &mut opts);
        assert!(matches!(g2.nodes[&c].status(), NodeStatus::Idle));
        assert!(g2.nodes[&c].dirty(), "a cancelled node stays dirty");
    }

    /// A bypassed node CARRIES ITS INPUT THROUGH; it does not freeze its last result.
    ///
    /// The distinction is the whole point. Bypass used to mean "do not run", so the outputs kept
    /// whatever the last real cook produced and everything downstream rebuilt from a snapshot of
    /// the past on every tick — running, but never advancing. The test pins the difference by
    /// making the two answers different numbers: cooked is `a + b`, passed through is just `a`.
    #[test]
    fn a_bypassed_node_passes_its_input_through() {
        let (mut graph, reg, types) = setup();
        let src = graph
            .create_node("Float", [0.0, 0.0], &reg, &types)
            .unwrap();
        let add = graph
            .create_node("Add", [100.0, 0.0], &reg, &types)
            .unwrap();
        graph.connect_idx(src, 0, add, 0, &reg, &types).unwrap();
        graph.rebuild_index();

        let src_in = graph.nodes[&src].input_names()[0].clone();
        let add_out = graph.nodes[&add].output_names()[0].clone();
        let set = |g: &mut Graph, id, name: &str, v: f64| {
            if let Some(n) = g.nodes.get_mut(&id) {
                n.set_input_value(name, BoxValue::F64(v));
                n.set_dirty(true);
            }
        };

        // `b = 1` so a real cook and a pass-through cannot produce the same number.
        set(&mut graph, add, "b", 1.0);
        set(&mut graph, src, &src_in, 5.0);
        let cancel = CancelToken::default();
        let mut opts = CookOpts::new(&cancel, noop_progress());
        cook_with(&mut graph, &reg, &types, &mut opts);
        assert_eq!(
            graph.nodes[&add].output_value(&add_out),
            Some(BoxValue::F64(6.0)),
            "cooked normally: a + b"
        );

        // Bypass it and change the input: the NEW input must appear on the output.
        if let Some(n) = graph.nodes.get_mut(&add) {
            n.set_bypass(true);
        }
        set(&mut graph, src, &src_in, 9.0);
        cook_with(&mut graph, &reg, &types, &mut opts);
        assert_eq!(
            graph.nodes[&add].output_value(&add_out),
            Some(BoxValue::F64(9.0)),
            "bypassed: the input passes through, and it is this tick's input"
        );
    }

    /// `BusWrite` → `BusRead` still round-trips a slot now that both are ORDINARY cook closures
    /// (using `ctx.bus`) and the ordering is driven by `CookStage::Late` rather than a hardcoded
    /// `type_name() == "BusRead"` check in the driver.
    #[test]
    fn bus_write_read_roundtrip_via_ordinary_cooks() {
        let (mut graph, reg, types) = setup();
        let src = graph
            .create_node("Float", [0.0, 0.0], &reg, &types)
            .unwrap();
        let write = graph
            .create_node("BusWrite", [120.0, 0.0], &reg, &types)
            .unwrap();
        let read = graph
            .create_node("BusRead", [240.0, 0.0], &reg, &types)
            .unwrap();
        graph
            .nodes
            .get_mut(&src)
            .unwrap()
            .set_input_value("value", BoxValue::F64(42.0));
        graph
            .connect(src, "out", write, "value", &reg, &types)
            .unwrap();

        cook(&mut graph, &reg, &types);

        assert!(
            graph.bus().contains("data"),
            "BusWrite must have written its slot"
        );
        assert_eq!(
            graph.nodes[&read].output_value("value"),
            Some(BoxValue::F64(42.0)),
            "the deferred BusRead must observe the main pass's write"
        );
        assert_eq!(
            reg.node_type("BusRead").map(|ty| ty.stage()),
            Some(CookStage::Late),
            "BusRead must declare itself Late (this is what defers it)"
        );
    }

    /// The §0 bug: a HOST-registered custom value type must FLOW across a wire on cook.
    /// Before the conversion kernel moved into `ValueTypes`, `convert(v, "Recon")` hit the
    /// hardcoded `_ => None` arm — the wire connected, then the value was dropped on every cook
    /// and the consumer silently kept its own literal.
    #[test]
    fn custom_value_type_flows_through_a_wire() {
        let (_g, mut reg, mut types) = setup();
        // A host type with identity coercion (an opaque payload).
        types.register(ValueTypeInfo::new("Recon", PinVisual::ANY));

        reg.add(
            fn_node(
                NodeDef::new("MakeRecon", "Debug")
                    .with_display("Make Recon")
                    .with_outputs(vec![PortDef::typed("recon", "Recon")]),
                |ctx| {
                    ctx.node
                        .set_out("recon", BoxValue::String("payload".into()));
                    Ok(())
                },
            ),
            &mut types,
        );
        reg.add(
            fn_node(
                NodeDef::new("TakeRecon", "Debug")
                    .with_display("Take Recon")
                    .with_inputs(vec![PortDef::typed("recon", "Recon")])
                    .with_outputs(vec![PortDef::typed("echo", "Recon")]),
                |ctx| {
                    let v = ctx.node.cook_value("recon").unwrap_or(BoxValue::Null);
                    ctx.node.set_out("echo", v);
                    Ok(())
                },
            ),
            &mut types,
        );

        let mut graph = Graph::new();
        let make = graph
            .create_node("MakeRecon", [0.0, 0.0], &reg, &types)
            .unwrap();
        let take = graph
            .create_node("TakeRecon", [200.0, 0.0], &reg, &types)
            .unwrap();
        graph
            .connect(make, "recon", take, "recon", &reg, &types)
            .unwrap();

        cook(&mut graph, &reg, &types);

        assert_eq!(
            graph.nodes[&take].output_value("echo"),
            Some(BoxValue::String("payload".into())),
            "a custom-typed value must survive the wire (this is the bug being fixed)"
        );
    }

    /// `CookCtx::for_test` lets a node author exercise a `CookFn` directly — no graph, no driver.
    /// This is the escape hatch `#[non_exhaustive]` would otherwise deny an out-of-crate host.
    #[test]
    fn cook_fn_can_be_unit_tested_through_for_test() {
        let (mut graph, reg, types) = setup();
        let id = graph.create_node("Add", [0.0, 0.0], &reg, &types).unwrap();
        let mut node = graph.nodes.remove(&id).expect("just created");
        node.set_input_value("a", BoxValue::F64(2.0));
        node.set_input_value("b", BoxValue::F64(5.0));

        let mut bus = DataBus::new();
        let cancel = CancelToken::default();
        let ty = reg.node_type("Add").expect("Add registered").clone();
        let mut ctx = CookCtx::for_test(&mut node, &reg, &types, &mut bus, &cancel);
        ty.cook(&mut ctx).expect("Add cooks");

        assert_eq!(node.output_value("result"), Some(BoxValue::F64(7.0)));
    }

    /// An ERRORED node must NOT be stamped fresh + clean: it produced nothing, so `cooked_gen`
    /// would lie to the live-preview freshness signal, and a clean flag would stop `cook_dirty`
    /// from ever retrying it (the node would sit there stale until the user edited something).
    #[test]
    fn errored_node_stays_dirty_and_is_not_stamped_fresh() {
        let (mut graph, reg, types) = setup();
        let div = graph.create_node("Div", [0.0, 0.0], &reg, &types).unwrap();
        graph
            .nodes
            .get_mut(&div)
            .unwrap()
            .set_input_value("b", BoxValue::F64(0.0));

        cook(&mut graph, &reg, &types);

        assert!(matches!(graph.nodes[&div].status(), NodeStatus::Error(_)));
        assert!(
            graph.nodes[&div].dirty(),
            "a failed node stays dirty so the next pass retries it"
        );
        assert_eq!(
            graph.nodes[&div].cooked_gen(),
            0,
            "a failed node must never advertise fresh output"
        );

        // Fixing the input makes the retry succeed — and only THEN is it stamped.
        graph
            .nodes
            .get_mut(&div)
            .unwrap()
            .set_input_value("b", BoxValue::F64(2.0));
        cook_dirty(&mut graph, &reg, &types);
        assert!(matches!(
            graph.nodes[&div].status(),
            NodeStatus::Done { .. }
        ));
        assert!(!graph.nodes[&div].dirty());
        assert_eq!(graph.nodes[&div].cooked_gen(), graph.cook_gen());
    }

    /// `CookCtx::phase` is REAL live state, not a field nobody writes: it records
    /// `NodeStatus::Cooking { progress, msg }` on the node AND reports to the host's sink with the
    /// node's id (so a UI can draw a per-node progress bar).
    #[test]
    fn cook_progress_lands_in_node_status_and_reaches_the_sink() {
        use std::sync::Mutex;

        #[derive(Default)]
        struct Recorder(Mutex<Vec<(NodeId, String, u64, u64)>>);
        impl ProgressSink for Recorder {
            fn phase(&self, node: NodeId, name: &str, done: u64, total: u64) {
                if let Ok(mut v) = self.0.lock() {
                    v.push((node, name.to_string(), done, total));
                }
            }
        }

        let (_g, mut reg, mut types) = setup();
        reg.add(
            fn_node(
                NodeDef::new("Slow", "Debug").with_outputs(vec![PortDef::float("seen", -1.0)]),
                |ctx| {
                    ctx.phase("solve", 3, 4);
                    // The node can read its own live status straight back out of `tmp.status`.
                    let seen = match ctx.node.status() {
                        NodeStatus::Cooking { progress, msg } if msg == "solve" => progress as f64,
                        other => panic!("expected Cooking{{0.75,'solve'}}, got {other:?}"),
                    };
                    ctx.node.set_out("seen", BoxValue::F64(seen));
                    Ok(())
                },
            ),
            &mut types,
        );

        let mut graph = Graph::new();
        let n = graph.create_node("Slow", [0.0, 0.0], &reg, &types).unwrap();
        let cancel = CancelToken::default();
        let sink = Arc::new(Recorder::default());
        let mut opts = CookOpts::new(&cancel, Arc::clone(&sink) as Arc<dyn ProgressSink>);
        cook_with(&mut graph, &reg, &types, &mut opts);

        assert_eq!(
            graph.nodes[&n].output_value("seen"),
            Some(BoxValue::F64(0.75)),
            "the cook must read its own progress back from tmp.status"
        );
        let reports = sink.0.lock().expect("sink not poisoned");
        assert_eq!(
            reports.as_slice(),
            [(n, "solve".to_string(), 3, 4)],
            "the host sink must receive the report tagged with the reporting node"
        );
    }

    /// `CookStage::Late` must hold in EVERY scope, including inside a subnet: a `BusRead` parented
    /// into a compound has to observe the `BusWrite` that ran in the same pass, not the previous
    /// one. (`BusWrite` is fed by a Float, so it has an in-edge and topo-sorts AFTER the
    /// edge-less `BusRead` — before the stage split, the read deterministically ran first and saw
    /// nothing.)
    #[test]
    fn late_stage_is_honoured_inside_a_subnet() {
        let (mut graph, reg, types) = setup();
        let sub = crate::builtins::create_subnet(&mut graph, [0.0, 0.0], &reg, &types).unwrap();

        let f = graph
            .create_node("Float", [0.0, 200.0], &reg, &types)
            .unwrap();
        let w = graph
            .create_node("BusWrite", [200.0, 200.0], &reg, &types)
            .unwrap();
        let r = graph
            .create_node("BusRead", [400.0, 200.0], &reg, &types)
            .unwrap();
        graph
            .nodes
            .get_mut(&f)
            .unwrap()
            .set_input_value("value", BoxValue::F64(7.0));
        graph.connect(f, "out", w, "value", &reg, &types).unwrap();
        for child in [f, w, r] {
            graph.set_parent(child, Some(sub));
        }

        cook(&mut graph, &reg, &types);

        assert_eq!(
            graph.nodes[&r].output_value("value"),
            Some(BoxValue::F64(7.0)),
            "a Late BusRead inside a subnet must see the same pass's BusWrite"
        );
    }

    /// A `BusWrite` with NO value to publish must leave the slot alone. Writing `Null` would
    /// destroy whatever another node had already put there.
    #[test]
    fn bus_write_without_a_value_does_not_clobber_the_slot() {
        let (mut graph, reg, types) = setup();
        let w = graph
            .create_node("BusWrite", [0.0, 0.0], &reg, &types)
            .unwrap();
        // Simulate a hand-built / older-JSON node whose `value` attr is simply absent.
        graph
            .nodes
            .get_mut(&w)
            .unwrap()
            .data
            .remove_path("inputs.value")
            .expect("remove the value attr");
        graph.bus_mut().set("data", BoxValue::F64(1.0));

        cook(&mut graph, &reg, &types);

        assert_eq!(
            graph.bus().get("data"),
            Some(&BoxValue::F64(1.0)),
            "a valueless BusWrite must not overwrite an existing slot"
        );
    }

    /// A cook can reach the bus's transient HANDLE lane through `ctx` — the thing a custom node
    /// could not do at all while `BusRead`/`BusWrite` were driver special-cases.
    #[test]
    fn custom_node_can_use_the_bus_handle_lane() {
        let (_g, mut reg, mut types) = setup();
        reg.add(
            fn_node(
                NodeDef::new("StashHandle", "Debug")
                    .with_display("Stash Handle")
                    .with_outputs(vec![PortDef::int("len", 0)]),
                |ctx| {
                    ctx.bus.set_handle("heavy", Arc::new(vec![1u32, 2, 3]));
                    let got = ctx
                        .handle::<Vec<u32>>("heavy")
                        .ok_or_else(|| CookError::Custom("handle vanished".into()))?;
                    ctx.node.set_out("len", BoxValue::I64(got.len() as i64));
                    Ok(())
                },
            ),
            &mut types,
        );
        let mut graph = Graph::new();
        let n = graph
            .create_node("StashHandle", [0.0, 0.0], &reg, &types)
            .unwrap();
        cook(&mut graph, &reg, &types);
        assert_eq!(graph.nodes[&n].output_value("len"), Some(BoxValue::I64(3)));
        assert!(
            graph.bus().contains_handle("heavy"),
            "the bus survives the cook"
        );
    }

    // -----------------------------------------------------------------------
    // Increment 2: a node type is a first-class object an external crate can implement.
    // -----------------------------------------------------------------------

    /// A struct-based `NodeType` (the shape an EXTERNAL crate writes): registering it with
    /// `registry.add` cooks it AND lands its declared `value_types()` in the host's `ValueTypes`
    /// automatically — the host never has to know the type brings a `"Recon"` port type with it.
    #[test]
    fn custom_node_type_registers_its_own_value_types_and_cooks() {
        struct MyNode;
        impl NodeType for MyNode {
            fn def(&self) -> NodeDef {
                NodeDef::new("MyNode", "Custom")
                    .with_display("My Node")
                    .with_outputs(vec![PortDef::typed("recon", "Recon")])
            }
            fn value_types(&self) -> Vec<ValueTypeInfo> {
                vec![
                    ValueTypeInfo::new("Recon", PinVisual::ANY)
                        .with_default(BoxValue::String(String::new())),
                ]
            }
            fn cook(&self, ctx: &mut CookCtx<'_>) -> CookResult<()> {
                ctx.node
                    .set_out("recon", BoxValue::String("payload".into()));
                Ok(())
            }
        }

        let (mut reg, mut types) = crate::default_registry();
        assert!(
            types.get("Recon").is_none(),
            "the host knows nothing of Recon yet"
        );

        reg.add(Arc::new(MyNode), &mut types);

        // The type's own value type came along for the ride.
        assert!(
            types.get("Recon").is_some(),
            "add() must register the type's value types"
        );
        assert_eq!(reg.get("MyNode").map(|d| &*d.display_name), Some("My Node"));

        let mut graph = Graph::new();
        let id = graph
            .create_node("MyNode", [0.0, 0.0], &reg, &types)
            .unwrap();
        cook(&mut graph, &reg, &types);
        assert_eq!(
            graph.nodes[&id].output_value("recon"),
            Some(BoxValue::String("payload".into()))
        );
    }

    /// A whole LIBRARY (the unit an external crate exports) plugs in with one `add_library` call:
    /// every type it carries is present and cooks.
    #[test]
    fn node_library_adds_all_its_types() {
        struct MyLib;
        impl NodeLibrary for MyLib {
            fn name(&self) -> &str {
                "mylib"
            }
            fn types(&self) -> Vec<Arc<dyn NodeType>> {
                vec![
                    fn_node(
                        NodeDef::new("One", "MyLib").with_outputs(vec![PortDef::float("out", 0.0)]),
                        |ctx| {
                            set_float_output(ctx.node, "out", 1.0);
                            Ok(())
                        },
                    ),
                    fn_node(
                        NodeDef::new("Two", "MyLib").with_outputs(vec![PortDef::float("out", 0.0)]),
                        |ctx| {
                            set_float_output(ctx.node, "out", 2.0);
                            Ok(())
                        },
                    ),
                ]
            }
        }

        let (mut reg, mut types) = crate::default_registry();
        reg.add_library(&MyLib, &mut types);
        assert!(reg.get("One").is_some() && reg.get("Two").is_some());

        let mut graph = Graph::new();
        let one = graph.create_node("One", [0.0, 0.0], &reg, &types).unwrap();
        let two = graph
            .create_node("Two", [200.0, 0.0], &reg, &types)
            .unwrap();
        cook(&mut graph, &reg, &types);
        assert_eq!(
            graph.nodes[&one].output_value("out"),
            Some(BoxValue::F64(1.0))
        );
        assert_eq!(
            graph.nodes[&two].output_value("out"),
            Some(BoxValue::F64(2.0))
        );
    }

    /// The caching contract: `NodeType::def()` is called EXACTLY ONCE per registration, and every
    /// later `registry.get(..)` is served from that cache. (This is why `def()` must be pure — if
    /// the registry called it again, a def that read mutable state would diverge from the cached
    /// copy every consumer sees.)
    #[test]
    fn def_is_called_exactly_once_per_registration() {
        use std::sync::atomic::AtomicUsize;

        struct Counted(Arc<AtomicUsize>);
        impl NodeType for Counted {
            fn def(&self) -> NodeDef {
                self.0.fetch_add(1, Ordering::SeqCst);
                NodeDef::new("Counted", "Custom")
            }
            fn cook(&self, _ctx: &mut CookCtx<'_>) -> CookResult<()> {
                Ok(())
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let (mut reg, mut types) = crate::default_registry();
        reg.add(Arc::new(Counted(calls.clone())), &mut types);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "def() runs once, at registration"
        );

        // Reading the def back (as the node menu / inspector / renderers do) hits the cache.
        for _ in 0..5 {
            assert!(reg.get("Counted").is_some());
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "get() must never re-run def()"
        );
    }

    /// A custom `Late`-staged type is deferred exactly like the builtin `BusRead`: it observes a
    /// bus write made by a `Main` node in the same pass. Staging is a property of the TYPE, not of
    /// a name the driver recognises.
    #[test]
    fn custom_late_staged_type_runs_after_main() {
        let (mut reg, mut types) = crate::default_registry();
        reg.add(
            fn_node_staged(
                NodeDef::new("MyBusRead", "Custom").with_outputs(vec![any("seen")]),
                CookStage::Late,
                Vec::new(),
                |ctx| {
                    let v = ctx.bus.get("data").cloned().unwrap_or(BoxValue::Null);
                    ctx.node.set_out("seen", v);
                    Ok(())
                },
            ),
            &mut types,
        );

        let mut graph = Graph::new();
        // The writer has an in-edge (from the Float), so topo order puts the edge-less reader
        // FIRST — only the stage split can make the reader see the write.
        let f = graph
            .create_node("Float", [0.0, 0.0], &reg, &types)
            .unwrap();
        let w = graph
            .create_node("BusWrite", [200.0, 0.0], &reg, &types)
            .unwrap();
        let r = graph
            .create_node("MyBusRead", [400.0, 0.0], &reg, &types)
            .unwrap();
        graph
            .nodes
            .get_mut(&f)
            .unwrap()
            .set_input_value("value", BoxValue::F64(9.0));
        graph.connect(f, "out", w, "value", &reg, &types).unwrap();

        cook(&mut graph, &reg, &types);

        assert_eq!(
            graph.nodes[&r].output_value("seen"),
            Some(BoxValue::F64(9.0)),
            "a custom Late type must observe the same pass's Main-stage bus write"
        );
    }
}
