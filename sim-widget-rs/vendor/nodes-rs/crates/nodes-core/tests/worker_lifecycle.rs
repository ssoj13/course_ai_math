//! The background-worker contract, exercised the way an out-of-crate node
//! library sees it: declare a worker, cook through the real driver, delete the
//! node.
//!
//! The property under test is **ownership**. A worker owned by the application
//! rather than by its node keeps running after the node is deleted — a socket
//! nobody can see and nobody can stop. Here the graph owns it, so removing the
//! node stops it and dropping the graph stops the rest.

use nodes_core::box_model::BoxValue;
use nodes_core::cook::{CookCtx, CookResult};
use nodes_core::node::{Node, NodeDef};
use nodes_core::port::PortDef;
use nodes_core::worker::{NodeWorker, WorkerSet};
use nodes_core::{
    CancelToken, CookOpts, Graph, NodeType, cook_with, default_registry, noop_progress,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Stands in for a poller or a socket: it only has to record that it was told
/// to stop.
struct Mailbox {
    stops: Arc<AtomicUsize>,
}

impl NodeWorker for Mailbox {
    fn stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
    }
}

/// A node type whose instances each own a `Mailbox`.
///
/// `starts` counts how many were created, so a test can tell "started once and
/// reused" from "restarted every cook".
struct Source {
    starts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

impl NodeType for Source {
    fn def(&self) -> NodeDef {
        NodeDef::new("Source", "Test").with_outputs(vec![PortDef::float("seen", 0.0)])
    }

    fn start_worker(&self, node: &Node, workers: &mut WorkerSet) {
        self.starts.fetch_add(1, Ordering::SeqCst);
        workers.install(
            node.id,
            Arc::new(Mailbox {
                stops: Arc::clone(&self.stops),
            }),
        );
    }

    fn cook(&self, ctx: &mut CookCtx<'_>) -> CookResult<()> {
        // Whether the background lane is live. A node with no worker takes its
        // synchronous path instead — which is what makes a replay under
        // `WorkerPolicy::None` deterministic.
        let seen = f64::from(u8::from(ctx.worker::<Mailbox>().is_some()));
        ctx.node.set_out("seen", BoxValue::F64(seen));
        Ok(())
    }
}

struct Counters {
    starts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

impl Counters {
    fn new() -> Self {
        Self {
            starts: Arc::new(AtomicUsize::new(0)),
            stops: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn source(&self) -> Arc<dyn NodeType> {
        Arc::new(Source {
            starts: Arc::clone(&self.starts),
            stops: Arc::clone(&self.stops),
        })
    }

    fn started(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }

    fn stopped(&self) -> usize {
        self.stops.load(Ordering::SeqCst)
    }
}

fn run(graph: &mut Graph, reg: &nodes_core::NodeTypeRegistry, types: &nodes_core::ValueTypes) {
    let cancel = CancelToken::default();
    let mut opts = CookOpts::new(&cancel, noop_progress());
    cook_with(graph, reg, types, &mut opts);
}

fn run_without_workers(
    graph: &mut Graph,
    reg: &nodes_core::NodeTypeRegistry,
    types: &nodes_core::ValueTypes,
) {
    let cancel = CancelToken::default();
    let mut opts = CookOpts::new(&cancel, noop_progress()).without_workers();
    cook_with(graph, reg, types, &mut opts);
}

#[test]
fn the_worker_starts_once_and_the_cook_can_reach_it() {
    let c = Counters::new();
    let (mut reg, mut types) = default_registry();
    reg.add(c.source(), &mut types);

    let mut graph = Graph::new();
    let id = graph
        .create_node("Source", [0.0, 0.0], &reg, &types)
        .expect("create");

    run(&mut graph, &reg, &types);
    assert_eq!(c.started(), 1, "started on the first cook");
    assert_eq!(
        graph.nodes[&id].output_value("seen"),
        Some(BoxValue::F64(1.0)),
        "the cook must see its own worker"
    );

    run(&mut graph, &reg, &types);
    assert_eq!(
        c.started(),
        1,
        "and reused on the next cook, not restarted every tick"
    );
    assert_eq!(c.stopped(), 0);
}

/// The whole point: deleting the node stops the thread.
#[test]
fn removing_the_node_stops_its_worker() {
    let c = Counters::new();
    let (mut reg, mut types) = default_registry();
    reg.add(c.source(), &mut types);

    let mut graph = Graph::new();
    let id = graph
        .create_node("Source", [0.0, 0.0], &reg, &types)
        .expect("create");
    run(&mut graph, &reg, &types);
    assert_eq!(c.stopped(), 0);

    assert!(graph.remove_node(id));
    assert_eq!(
        c.stopped(),
        1,
        "a deleted node must not leave a thread running"
    );
    assert!(graph.workers().is_empty());
}

/// And so does dropping the graph, for whatever was still running.
#[test]
fn dropping_the_graph_stops_what_is_left() {
    let c = Counters::new();
    let (mut reg, mut types) = default_registry();
    reg.add(c.source(), &mut types);

    {
        let mut graph = Graph::new();
        graph
            .create_node("Source", [0.0, 0.0], &reg, &types)
            .expect("create");
        run(&mut graph, &reg, &types);
        assert_eq!(c.stopped(), 0);
    }
    assert_eq!(c.stopped(), 1);
}

/// `without_workers` starts nothing, so a replay has no thread deciding how
/// much arrived before the tick read it.
///
/// This is the structural version of "remember to swap the threaded engine for
/// an inline one in every harness" — the setting is on the pass, so it covers
/// every async node in the graph at once, including ones added later.
#[test]
fn a_pass_without_workers_starts_none() {
    let c = Counters::new();
    let (mut reg, mut types) = default_registry();
    reg.add(c.source(), &mut types);

    let mut graph = Graph::new();
    let id = graph
        .create_node("Source", [0.0, 0.0], &reg, &types)
        .expect("create");

    run_without_workers(&mut graph, &reg, &types);
    assert_eq!(c.started(), 0, "no worker may be started");
    assert!(graph.workers().is_empty());
    assert_eq!(
        graph.nodes[&id].output_value("seen"),
        Some(BoxValue::F64(0.0)),
        "and the node must be able to tell, so it can take its own path"
    );
}
