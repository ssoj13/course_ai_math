//! [`NodeType`] — a node type as a first-class OBJECT, and [`NodeLibrary`] — a pack of them.
//!
//! ## Why this exists
//!
//! A node type used to be a plain `NodeDef` struct carrying a boxed closure, built by a
//! `register_*(&mut registry)` free function inside this crate. An external crate could technically
//! push a `NodeDef` in, but the type had no identity: no place to hang its own state, no way to
//! declare the custom value types its ports need, and no way to ship a whole PACK of types as one
//! unit. This module makes a node type an implementable trait, so a compute crate (an SfM/MVS
//! pipeline, a shader library, …) can define its nodes in its own code and a host injects them with
//! one call:
//!
//! ```ignore
//! registry.add_library(&MyNodes, &mut value_types);   // MyNodes: NodeLibrary
//! registry.add(Arc::new(MySingleNode), &mut value_types);
//! ```
//!
//! ## Where the pieces live
//! - [`NodeType`] — the behaviour + shape of ONE type. Its [`NodeType::def`] is called ONCE at
//!   registration and cached in the registry ([`crate::registry::RegEntry`]).
//! - [`fn_node`] / [`fn_node_staged`] — adapt a plain closure into a `NodeType`. Every builtin
//!   registers this way (see `builtins/`), and so does any custom node too simple to need a struct.
//! - [`NodeLibrary`] — the unit an external crate exports and a host injects.

use std::sync::Arc;

use crate::cook::{CookCtx, CookResult, CookStage};
use crate::node::{Node, NodeDef};
use crate::value_types::ValueTypeInfo;
use crate::worker::WorkerSet;

/// **Is this node a pure function of its inputs, or a JOB that DOES something?**
///
/// The crate used one word — *cook* — for two categorically different things, and that conflation is
/// the root of a whole family of symptoms.
///
/// - [`NodeKind::Dataflow`] — lazy, automatic, pull-based, CHEAP. The output is a VALUE derived from
///   the inputs. Recomputing is free *and equivalent*: you get the same answer, so throwing the answer
///   away and recomputing it on load is not merely acceptable, it is cheaper than storing it. `Add`,
///   `Convert`, `Vec3`.
/// - [`NodeKind::Job`] — imperative, explicit, expensive, SIDE-EFFECTING. The "output" is a REFERENCE to
///   an effect that ALREADY HAPPENED on disk. Recomputing is not free and **not even equivalent** — it
///   re-does the effect. This is Houdini's ROP, Nuke's Write. A photogrammetry `Mapper` / `DenseStereo` /
///   `Splat` is 40 minutes of GPU work whose output port carries a path.
///
/// The crate's ROADMAP states the assumption *"cook is cheap, rendering dominates"*. That is TRUE for
/// dataflow and FALSE for jobs, and we had no way to say which was which — so the loader marked every
/// node dirty, the editor's per-frame cook picked them up, and merely OPENING a graph re-ran a
/// reconstruction. Naming this fact makes the assumption true BY CONSTRUCTION: automatic cook now only
/// ever touches dataflow, which really is cheap.
///
/// This is deliberately NOT a cost label ("expensive"). A label is a subjective judgement and behaviour
/// cannot be derived from one. `Job` is a fact about what the node IS: *"I am not a pure function of my
/// inputs, my result already happened, do not recompute me just to look at me."*
///
/// Everything follows from it:
/// - the cook driver NEVER runs a Job ([`crate::cook`] / [`crate::cook_dirty`]); only
///   [`crate::execute`] / [`crate::execute_all`] do;
/// - a Job's outputs are PERSISTED on save, a Dataflow node's are blanked (`crate::serialization`);
/// - so a saved Job graph re-loads CLEAN and nothing re-runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// A pure function of its inputs; cheap and safe to recompute. The default — every existing type
    /// and every consumer is unchanged.
    #[default]
    Dataflow,
    /// An imperative, side-effecting job whose output records an effect that already happened. Never
    /// run by the cook driver; run only when a human or a pipeline asks ([`crate::execute`]).
    Job,
}

/// A node TYPE (not an instance). One implementor = one entry in the
/// [`crate::registry::NodeTypeRegistry`].
///
/// Implement this in your own crate to inject a custom node into any nodes-rs host. The type may
/// hold its own state (a handle to a solver, a config struct) — it is stored as
/// `Arc<dyn NodeType>` and every `cook` gets `&self`, so that state must be immutable / interior-
/// mutable and `Send + Sync` (a cook pass may be driven from any thread).
pub trait NodeType: Send + Sync + 'static {
    /// The type's shape: name, display name, category, ports + their defaults, inspector schema.
    ///
    /// Called **ONCE** at registration time and CACHED by the registry, which then serves every
    /// `registry.get(name)` from that cache. It MUST therefore be pure — no I/O, no randomness, no
    /// reads of mutable global state — or the registry's copy and the "live" one silently diverge.
    ///
    /// ## Port defaults of a CUSTOM type: leaving them `Null` is correct
    ///
    /// A `def()` has no registry in hand, so it cannot know what a value of your own type looks
    /// like: `PortDef::typed("cloud", "PointCloud")` (equivalently
    /// `PortDef::new("cloud", "PointCloud", BoxValue::Null)`) is CORRECT-by-construction. At
    /// registration the registry re-seeds every `Null` port default from the PROTOTYPE of that
    /// port's value type ([`crate::ValueTypeInfo::default`], which your own [`Self::value_types`]
    /// declares and which is registered a moment earlier). Hand-spelling the prototype here is
    /// therefore optional.
    ///
    /// An explicit NON-`Null` default is honoured as-is and never overwritten — that is how a port
    /// gets a value other than its type's prototype (`PortDef::float("gain", 1.0)`).
    ///
    /// This matters because a `Null` prototype is what used to make the type checks mis-judge (see
    /// [`crate::ValueTypes::convert`]): it is now the registry's job to prevent that, not the node
    /// author's.
    fn def(&self) -> NodeDef;

    /// Custom value types this node type's ports need (see [`ValueTypeInfo`]). Registered into the
    /// host's [`crate::ValueTypes`] when the type is added, so a port typed `"Recon"` is a real,
    /// coercible, pin-colourable type in that host without the host knowing it exists.
    ///
    /// Default: none (the type only uses builtin port types).
    fn value_types(&self) -> Vec<ValueTypeInfo> {
        Vec::new()
    }

    /// When this type cooks relative to the rest of the pass. Default [`CookStage::Main`];
    /// [`CookStage::Late`] runs after every `Main` node in the same scope (bus readers).
    fn stage(&self) -> CookStage {
        CookStage::Main
    }

    /// Is this a dataflow function or a JOB? See [`NodeKind`] — this is the single most consequential
    /// thing a node type declares, and getting it wrong is what makes a GUI re-run a reconstruction.
    ///
    /// Default [`NodeKind::Dataflow`], so an existing type keeps its exact behaviour. Return
    /// [`NodeKind::Job`] when your `cook` DOES something (writes files, drives a GPU for minutes,
    /// launches a process) rather than COMPUTING something from its inputs.
    fn kind(&self) -> NodeKind {
        NodeKind::Dataflow
    }

    /// Compute one instance of this type. Everything a cook may touch is on `ctx`: its node, a
    /// read-only graph view, the bus, both registries, cancellation and progress. Report failure
    /// by returning [`crate::CookError`] — never panic (the driver catches panics, but a panic
    /// costs the node its error message).
    fn cook(&self, ctx: &mut CookCtx<'_>) -> CookResult<()>;

    /// Start the background worker this INSTANCE owns, if it has one.
    ///
    /// Called by the driver the first time the instance cooks, and again after
    /// anything stopped it. Install with [`WorkerSet::install`] keyed on
    /// `node.id()`; the cook then reaches it back with `ctx.worker::<T>()`, and
    /// the framework stops it when the node is removed or the graph dropped.
    ///
    /// ## When a node type wants this
    ///
    /// When it talks to something outside the graph: an RSS poller, a socket, a
    /// model, a venue being watched. Doing that work *inside* `cook` stalls the
    /// whole pass on one slow endpoint. The shape that works is a thread
    /// filling a mailbox and a cook that only drains it — which keeps the cook
    /// cheap and the pass predictable.
    ///
    /// Configure the worker from `node` (its parameters are already resolved),
    /// and read them again on a later cook to decide whether to reinstall:
    /// installing replaces and stops the previous one, so restarting after a
    /// parameter change cannot orphan a thread.
    ///
    /// Default: none, so every existing type is unchanged.
    fn start_worker(&self, _node: &Node, _workers: &mut WorkerSet) {}
}

/// A [`NodeType`] whose behaviour is a plain closure — the adapter behind [`fn_node`].
///
/// Public so its (rare) explicit type can be named; you normally build it via [`fn_node`] /
/// [`fn_node_staged`] and immediately erase it to `Arc<dyn NodeType>`.
pub struct FnNode<F> {
    def: NodeDef,
    stage: CookStage,
    kind: NodeKind,
    value_types: Vec<ValueTypeInfo>,
    cook: F,
}

impl<F> NodeType for FnNode<F>
where
    F: Fn(&mut CookCtx<'_>) -> CookResult<()> + Send + Sync + 'static,
{
    fn def(&self) -> NodeDef {
        self.def.clone()
    }

    fn value_types(&self) -> Vec<ValueTypeInfo> {
        self.value_types.clone()
    }

    fn stage(&self) -> CookStage {
        self.stage
    }

    fn kind(&self) -> NodeKind {
        self.kind
    }

    fn cook(&self, ctx: &mut CookCtx<'_>) -> CookResult<()> {
        (self.cook)(ctx)
    }
}

/// Adapt a plain closure (or `fn`) into a [`NodeType`]: `CookStage::Main`, no custom value types.
///
/// This is how every builtin registers (see the `node!` macro), and the shortest way to inject a
/// custom node that needs no state of its own.
pub fn fn_node<F>(def: NodeDef, cook: F) -> Arc<dyn NodeType>
where
    F: Fn(&mut CookCtx<'_>) -> CookResult<()> + Send + Sync + 'static,
{
    fn_node_staged(def, CookStage::Main, Vec::new(), cook)
}

/// Adapt a plain closure into a **[`NodeKind::Job`]** type: the closure DOES something (drives a GPU
/// for minutes, writes artifacts, launches a process) rather than computing a value from its inputs.
///
/// The cook driver will never run it — only [`crate::execute`] / [`crate::execute_all`] will — and its
/// outputs are persisted on save, so the graph re-loads clean. See [`NodeKind`] for why that is the
/// whole point. Everything else matches [`fn_node`] (`CookStage::Main`, no custom value types).
pub fn fn_job<F>(def: NodeDef, cook: F) -> Arc<dyn NodeType>
where
    F: Fn(&mut CookCtx<'_>) -> CookResult<()> + Send + Sync + 'static,
{
    Arc::new(FnNode {
        def,
        stage: CookStage::Main,
        kind: NodeKind::Job,
        value_types: Vec::new(),
        cook,
    })
}

/// [`fn_node`] with an explicit [`CookStage`] and custom [`ValueTypeInfo`]s — for a closure-based
/// type that must run late (a bus reader) and/or brings its own port types.
///
/// Always [`NodeKind::Dataflow`]; a closure-authored job uses [`fn_job`].
pub fn fn_node_staged<F>(
    def: NodeDef,
    stage: CookStage,
    value_types: Vec<ValueTypeInfo>,
    cook: F,
) -> Arc<dyn NodeType>
where
    F: Fn(&mut CookCtx<'_>) -> CookResult<()> + Send + Sync + 'static,
{
    Arc::new(FnNode {
        def,
        stage,
        kind: NodeKind::Dataflow,
        value_types,
        cook,
    })
}

/// A named pack of node types (+ the value types they share). This is the unit an external crate
/// exports and a host injects with one call:
/// [`crate::NodeTypeRegistry::add_library`].
///
/// The builtins are themselves a library ([`crate::builtins::Builtins`]) — there is no privileged
/// registration path an out-of-crate library cannot use.
///
/// `Send + Sync` like [`NodeType`]: a host must be able to STORE its libraries (to rebuild a
/// registry, to hand them to a worker thread) and not just borrow one for the duration of an
/// `add_library` call. A library is a stateless descriptor, so this costs implementors nothing.
pub trait NodeLibrary: Send + Sync {
    /// Library name, for diagnostics and (later) namespacing — e.g. `"builtins"`, `"colmap"`.
    fn name(&self) -> &str;

    /// The types this library contributes.
    fn types(&self) -> Vec<Arc<dyn NodeType>>;

    /// Value types shared by the whole library, registered in ADDITION to each type's own
    /// [`NodeType::value_types`] (which is where a type-specific one belongs). Default: none.
    fn value_types(&self) -> Vec<ValueTypeInfo> {
        Vec::new()
    }
}
