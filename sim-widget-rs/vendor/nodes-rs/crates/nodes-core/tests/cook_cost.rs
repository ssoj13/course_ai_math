//! What the cook driver costs per node, apart from whatever the node does.
//!
//! A host that ticks a graph in a loop (a trading pipeline, a live viewport)
//! pays this on every node on every tick, so it is worth having a number for it
//! that someone can watch. The chain below is deliberately trivial —
//! `MapInsert` writes one key — so nearly all of the measured time IS the
//! driver: resolving inputs, converting them, staging the overrides, running
//! the closure, stamping status.
//!
//! The assertions are ceilings and ratios rather than wall-clock targets, so
//! they survive a slow machine but still fail if the per-node cost goes back
//! into the tens of microseconds, or if the payload starts being copied.

use nodes_core::prelude::*;
use nodes_core::{
    BoxMap, BoxValue, Graph, NodeId, NodeTypeRegistry, ValueTypes, cook, register_builtins,
};
use std::time::Instant;

const NODES: usize = 10;
const TICKS: usize = 400;
/// Timed rounds per measurement; the fastest one is reported (see `per_tick`).
const ROUNDS: usize = 5;

/// A node that forwards its input untouched — the shape a pipeline node has
/// when it folds a layer into a payload and passes the whole thing on.
///
/// Deliberately NOT one of the `Map` builtins: `MapInsert` has to copy the
/// incoming map (it must not mutate a value its source still holds), so a chain
/// of them measures that copy rather than the driver.
fn pass_through() -> Arc<dyn NodeType> {
    fn_node(
        NodeDef::new("Pass", "Debug")
            .with_inputs(vec![map("in")])
            .with_outputs(vec![map("out")]),
        |ctx| {
            let v = ctx.node.cook_value("in").unwrap_or(BoxValue::Null);
            ctx.node.set_out("out", v);
            Ok(())
        },
    )
}

/// Same ports and same wires as [`pass_through`], but the closure does
/// nothing. The difference between the two is what the port accessors cost
/// (`cook_value` + `set_out`), as opposed to what the driver costs around them.
fn silent() -> Arc<dyn NodeType> {
    fn_node(
        NodeDef::new("Silent", "Debug")
            .with_inputs(vec![map("in")])
            .with_outputs(vec![map("out")]),
        |_ctx| Ok(()),
    )
}

fn fixtures() -> (NodeTypeRegistry, ValueTypes) {
    let mut types = ValueTypes::new();
    let mut reg = NodeTypeRegistry::new();
    register_builtins(&mut reg, &mut types);
    reg.add(pass_through(), &mut types);
    reg.add(silent(), &mut types);
    (reg, types)
}

/// A flat bag of `leaves` scalars, fed into the head of the chain.
fn payload(leaves: usize) -> BoxValue {
    let mut root = BoxMap::new();
    for i in 0..leaves {
        root.insert(format!("k{i}"), BoxValue::F64(i as f64));
    }
    BoxValue::obj(root)
}

/// `NODES` nodes of `ty` in a line, the payload entering at the head.
fn chain_of(ty: &str, leaves: usize, reg: &NodeTypeRegistry, types: &ValueTypes) -> Graph {
    let mut graph = Graph::new();
    let mut prev: Option<NodeId> = None;
    for i in 0..NODES {
        let id = graph
            .create_node(ty, [i as f32 * 200.0, 0.0], reg, types)
            .expect("node type is registered");
        match prev {
            None => {
                let node = graph.nodes.get_mut(&id).expect("just created");
                node.set_input_value("in", payload(leaves));
            }
            Some(p) => {
                graph
                    .connect(p, "out", id, "in", reg, types)
                    .expect("chain connects");
            }
        }
        prev = Some(id);
    }
    graph
}

fn chain(leaves: usize, reg: &NodeTypeRegistry, types: &ValueTypes) -> Graph {
    chain_of("Pass", leaves, reg, types)
}

/// Seconds per tick, after a warm-up that lets the first pass allocate the attr
/// objects every later pass reuses.
///
/// Reports the BEST of [`ROUNDS`] rounds, not the average. This is a wall-clock
/// budget on a developer machine that is also running compilers, browsers and
/// whatever else: unrelated load can only ever make a round slower, so the
/// minimum is the closest estimate of the cost with nothing in the way. Averaging
/// measured the machine's mood instead — the same code came out anywhere between
/// 22 and 35 us/node, which straddles the threshold and failed roughly two runs
/// in three. Raising the threshold instead would have hidden real regressions.
fn per_tick(graph: &mut Graph, reg: &NodeTypeRegistry, types: &ValueTypes) -> f64 {
    for _ in 0..20 {
        cook(graph, reg, types);
    }
    let mut best = f64::INFINITY;
    for _ in 0..ROUNDS {
        let start = Instant::now();
        for _ in 0..TICKS {
            cook(graph, reg, types);
        }
        best = best.min(start.elapsed().as_secs_f64() / TICKS as f64);
    }
    best
}

#[test]
fn the_driver_costs_a_few_microseconds_per_node() {
    let (reg, types) = fixtures();
    let mut graph = chain(60, &reg, &types);
    let tick = per_tick(&mut graph, &reg, &types);
    let node = tick / NODES as f64;

    println!(
        "cook: {:.1} us/tick over {NODES} nodes = {:.2} us/node (60-leaf payload)",
        tick * 1e6,
        node * 1e6
    );
    assert!(
        node < 30e-6,
        "the driver costs {:.1} us per node",
        node * 1e6
    );
}

/// Where the per-node cost actually goes.
///
/// Printed rather than asserted (beyond a sanity check): the split is what
/// someone optimising the driver needs to see, and pinning a ratio between two
/// sub-microsecond quantities would only produce a flaky test.
#[test]
fn report_where_the_per_node_cost_goes() {
    let (reg, types) = fixtures();
    let full = per_tick(&mut chain_of("Pass", 60, &reg, &types), &reg, &types) / NODES as f64;
    let bare = per_tick(&mut chain_of("Silent", 60, &reg, &types), &reg, &types) / NODES as f64;

    println!(
        "per node: {:.2} us total, {:.2} us driver (wire resolution + bookkeeping), \
         {:.2} us port access (cook_value + set_out)",
        full * 1e6,
        bare * 1e6,
        (full - bare) * 1e6
    );
    assert!(
        bare <= full * 1.2,
        "a node that does nothing cost more than one that works"
    );
}

/// The payload must ride the chain without being copied at each hop. With
/// copy-on-write interiors that holds by construction; this pins it, because a
/// stray deep copy in the driver would be invisible except as lost speed.
#[test]
fn a_bigger_payload_does_not_cost_more_per_node() {
    let (reg, types) = fixtures();
    let small = per_tick(&mut chain(10, &reg, &types), &reg, &types);
    let big = per_tick(&mut chain(1000, &reg, &types), &reg, &types);

    println!(
        "cook: 10-leaf {:.1} us/tick, 1000-leaf {:.1} us/tick, ratio {:.2}",
        small * 1e6,
        big * 1e6,
        big / small
    );
    assert!(
        big < small * 4.0,
        "a 100x bigger payload made the tick {:.1}x slower — something is copying it",
        big / small
    );
}
