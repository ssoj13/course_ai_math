//! Background workers owned by a node **instance**.
//!
//! ## The pattern this makes first-class
//!
//! A node that talks to the outside world — an RSS poller, a websocket feed, a
//! model doing inference, a venue being watched for fills — must not do that
//! work inside its cook. A four-second HTTP call inside a cook stalls the whole
//! pass, and with it every unrelated lane in the graph.
//!
//! The shape that works is always the same: a background thread fills a
//! mailbox, and the node's cook only **drains** it into the graph. The cook
//! stays cheap and the tick stays a fast, predictable fold.
//!
//! Hosts already write this by hand, repeatedly, and it costs them three
//! things:
//!
//! 1. **The worker outlives the node.** Owned by the application rather than by
//!    the node, a websocket keeps running after its node is deleted.
//! 2. **Adding one touches several places** — the app struct, the service
//!    wiring, and the node — so "add an async source" stops being cheap.
//! 3. **Determinism is a convention.** A replay/backtest has to *remember* to
//!    swap the threaded worker for a synchronous one, and forgetting produces
//!    plausible numbers that depend on timing.
//!
//! ## What this module abstracts, and what it deliberately does not
//!
//! **Only the lifetime.** The framework starts a worker when its node first
//! cooks, hands the node back its own handle, and stops it when the node goes
//! away. That is all.
//!
//! Messages are **not** abstracted. Two real workers already have different
//! shapes — an interval poller that only produces, and a request/response queue
//! — and a common `post`/`drain` in terms of some erased value type would cost
//! every typed worker a round trip through it. A worker keeps its own typed API
//! (`fn drain(&self) -> Vec<MyThing>`); [`NodeWorker`] adds one method so the
//! framework can stop it.
//!
//! ## Determinism
//!
//! [`WorkerPolicy::None`] tells the driver to start no workers at all. A node
//! then finds no handle and takes whatever synchronous path it already has for
//! that case. One switch on the driver replaces "remember to swap the engine"
//! in every replay harness.
//!
//! # What is NOT a `NodeWorker`
//!
//! [`WorkerSet`] keys by [`NodeId`], so a worker's lifetime is exactly its
//! node's. That is the right default and it is what stops one node from reaching
//! into another's thread. It also means two shapes do not belong here:
//!
//! - a worker with a SECOND consumer — e.g. an inference engine serving both a
//!   node and a chat panel that has no node at all;
//! - a worker whose lifetime is not a node's — e.g. an exchange order-watch that
//!   follows the venue connection rather than any one node.
//!
//! Those are HOST SERVICES. Own them beside the graph, not in it, and hand the
//! node whatever it needs at cook time. Do NOT widen this map to a name-keyed
//! one to make them fit: the `NodeId` key is precisely what prevents unrelated
//! nodes from sharing a thread by accident.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use crate::graph::NodeId;

/// A background worker belonging to one node instance.
///
/// Implement it on whatever handle already owns the thread and the mailbox. The
/// trait asks for one thing — how to stop — because stopping is the only part
/// the framework has to know about; everything the *node* needs stays on the
/// concrete type and is reached through [`WorkerSet::get`].
///
/// `Send + Sync` because a cook pass may be driven from any thread, and the
/// handle is shared between that thread and the worker's own.
pub trait NodeWorker: Send + Sync + 'static {
    /// Stop the background thread(s) and release anything held.
    ///
    /// Called when the node is removed, when the graph is dropped, or when a
    /// host restarts the worker. Must be **idempotent** — a host may stop a
    /// worker it has already stopped — and must not block for long: it runs on
    /// the thread doing the removal, which is usually a UI thread.
    fn stop(&self);

    /// Is the worker still doing its job? `Err(reason)` once it is not.
    ///
    /// The framework CANNOT work this out by itself. A slot stays installed for as long as its
    /// node exists, so [`WorkerSet::has`] keeps answering `true` after the thread behind it has
    /// died — a poller whose socket dropped is never restarted and its node draws exactly like a
    /// healthy one. The worker is the only thing that knows, so it is the one asked.
    ///
    /// Defaults to healthy so existing workers keep compiling; implement it on any worker with a
    /// thread that can die (a socket poller, a subprocess, a reconnecting client). Hosts read it
    /// through [`WorkerSet::failed`].
    ///
    /// Must not block — it is polled from the UI thread. Report a cached flag, not a live probe.
    fn health(&self) -> Result<(), String> {
        Ok(())
    }
}

/// One installed worker: the trait object the framework stops through, plus the
/// same `Arc` kept as `dyn Any` so the owning node can get its concrete type
/// back. Both point at one allocation.
struct WorkerSlot {
    worker: Arc<dyn NodeWorker>,
    any: Arc<dyn Any + Send + Sync>,
    type_name: &'static str,
}

/// The workers of one graph, keyed by the node that owns them.
///
/// Transient by construction: never serialized, empty after a load, and
/// rebuilt by the driver on the next cook. Dropping it stops everything it
/// holds, so a graph going out of scope cannot leave threads behind.
#[derive(Default)]
pub struct WorkerSet {
    map: HashMap<NodeId, WorkerSlot>,
}

impl WorkerSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Install `worker` as `node`'s, stopping whatever it had before.
    ///
    /// Replacing rather than refusing is deliberate: restarting a worker after
    /// a parameter change is a normal thing for a host to do, and the old
    /// thread must not be orphaned when it happens.
    pub fn install<T: NodeWorker>(&mut self, node: NodeId, worker: Arc<T>) {
        self.stop(node);
        self.map.insert(
            node,
            WorkerSlot {
                worker: worker.clone(),
                any: worker,
                type_name: std::any::type_name::<T>(),
            },
        );
    }

    /// Every worker that reports itself broken, as `(node, reason)`.
    ///
    /// The seam that makes a dead worker visible: a host polls this and tints the offending nodes
    /// the same way a failed cook does, instead of the failure staying invisible in the one place
    /// a user actually looks. Empty for a healthy graph, and for any worker that does not override
    /// [`NodeWorker::health`].
    ///
    /// Whether to RESTART a broken worker is the driver's call, not the node's — otherwise every
    /// node author writes the same retry loop.
    pub fn failed(&self) -> Vec<(NodeId, String)> {
        self.map
            .iter()
            .filter_map(|(&id, slot)| slot.worker.health().err().map(|why| (id, why)))
            .collect()
    }

    /// `node`'s worker as its concrete type.
    ///
    /// `None` when the node has none *or* when `T` is not what was installed —
    /// a wrong-type read fails loud rather than returning something plausible,
    /// matching [`crate::DataBus::get_handle`].
    pub fn get<T: NodeWorker>(&self, node: NodeId) -> Option<Arc<T>> {
        self.map.get(&node)?.any.clone().downcast::<T>().ok()
    }

    /// Whether `node` has a worker installed.
    pub fn has(&self, node: NodeId) -> bool {
        self.map.contains_key(&node)
    }

    /// The concrete type name installed for `node` (diagnostics).
    pub fn type_name_of(&self, node: NodeId) -> Option<&'static str> {
        self.map.get(&node).map(|s| s.type_name)
    }

    /// Stop and forget `node`'s worker; whether there was one.
    pub fn stop(&mut self, node: NodeId) -> bool {
        match self.map.remove(&node) {
            Some(slot) => {
                slot.worker.stop();
                true
            }
            None => false,
        }
    }

    /// Stop and forget every worker.
    pub fn stop_all(&mut self) {
        for (_, slot) in self.map.drain() {
            slot.worker.stop();
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Nodes that currently own a worker.
    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.map.keys().copied()
    }
}

impl Drop for WorkerSet {
    fn drop(&mut self) {
        self.stop_all();
    }
}

impl std::fmt::Debug for WorkerSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerSet")
            .field("len", &self.len())
            .finish()
    }
}

/// Whether a cook pass may start background workers.
///
/// The point of the `None` variant is **reproducibility**: a replay or a
/// backtest wants no thread deciding how much arrived before the tick read it.
/// Set once on the driver, it applies to every async node in the graph — where
/// the alternative is each harness remembering to substitute each worker by
/// hand, and producing timing-dependent numbers on the day it forgets one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkerPolicy {
    /// Start each node's worker on its first cook. The normal, live setting.
    #[default]
    Background,
    /// Start nothing. A node finds no handle and uses whatever synchronous
    /// path it has for that case.
    None,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct Counter {
        stops: Arc<AtomicUsize>,
    }

    impl NodeWorker for Counter {
        fn stop(&self) {
            self.stops.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Other;
    impl NodeWorker for Other {
        fn stop(&self) {}
    }

    /// A worker whose thread can die, and which says so — the shape `health` exists for.
    struct Fragile {
        dead: AtomicBool,
    }
    impl NodeWorker for Fragile {
        fn stop(&self) {}
        fn health(&self) -> Result<(), String> {
            if self.dead.load(Ordering::SeqCst) {
                Err("socket closed".into())
            } else {
                Ok(())
            }
        }
    }

    fn counter() -> (Arc<Counter>, Arc<AtomicUsize>) {
        let stops = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(Counter {
                stops: Arc::clone(&stops),
            }),
            stops,
        )
    }

    /// A dead worker is reported instead of passing for healthy. Without this the slot stays
    /// installed, `has` keeps answering `true`, and the node draws exactly like a working one.
    #[test]
    fn a_broken_worker_is_reported() {
        let mut set = WorkerSet::new();
        let quiet = NodeId::new();
        let breaks = NodeId::new();
        let (w, _) = counter();
        set.install(quiet, w); // never overrides `health` → healthy by default
        let fragile = Arc::new(Fragile {
            dead: AtomicBool::new(false),
        });
        set.install(breaks, Arc::clone(&fragile));

        assert!(set.failed().is_empty(), "nothing is broken yet");

        fragile.dead.store(true, Ordering::SeqCst);
        let failed = set.failed();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].0, breaks);
        assert_eq!(failed[0].1, "socket closed");
    }

    #[test]
    fn a_worker_comes_back_as_its_own_type() {
        let mut set = WorkerSet::new();
        let id = NodeId::new();
        let (w, _) = counter();
        set.install(id, w);

        assert!(set.get::<Counter>(id).is_some());
        assert!(
            set.get::<Other>(id).is_none(),
            "a wrong-type read must fail rather than hand back something plausible"
        );
        assert!(set.get::<Counter>(NodeId::new()).is_none());
    }

    /// Removing the node stops the thread. This is the whole point: a websocket
    /// owned by the application keeps running after its node is deleted.
    #[test]
    fn stopping_a_node_stops_its_worker() {
        let mut set = WorkerSet::new();
        let id = NodeId::new();
        let (w, stops) = counter();
        set.install(id, w);

        assert!(set.stop(id));
        assert_eq!(stops.load(Ordering::SeqCst), 1);
        assert!(!set.has(id));
        assert!(!set.stop(id), "stopping twice reports nothing was there");
        assert_eq!(
            stops.load(Ordering::SeqCst),
            1,
            "and does not stop it a second time"
        );
    }

    /// Re-installing must not orphan the thread that was already running.
    #[test]
    fn replacing_a_worker_stops_the_old_one() {
        let mut set = WorkerSet::new();
        let id = NodeId::new();
        let (first, stops) = counter();
        set.install(id, first);
        let (second, _) = counter();
        set.install(id, second);

        assert_eq!(stops.load(Ordering::SeqCst), 1, "the first was stopped");
        assert_eq!(set.len(), 1, "and replaced, not added beside");
    }

    /// A dropped graph must not leave threads behind.
    #[test]
    fn dropping_the_set_stops_everything() {
        let (w, stops) = counter();
        {
            let mut set = WorkerSet::new();
            set.install(NodeId::new(), w);
            assert_eq!(stops.load(Ordering::SeqCst), 0);
        }
        assert_eq!(stops.load(Ordering::SeqCst), 1);
    }
}
