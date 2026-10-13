//! **`NodeKind::Job` — the model, pinned.**
//!
//! The crate used one word, *cook*, for two categorically different things: cheap, lazy, equivalent
//! dataflow evaluation, and an imperative 40-minute side-effecting JOB whose output is a reference to
//! work that already happened on disk. Treating the second as the first is why merely OPENING a graph
//! re-ran a reconstruction. These tests hold the model in place:
//!
//! - the cook driver NEVER runs a Job, however dirty it is;
//! - `execute` does, and cooks the Job's cheap dataflow dependencies first;
//! - a Job's outputs are SAVED, so its graph re-loads clean and nothing re-runs;
//! - a dataflow node's outputs are still blanked and recomputed (unchanged behaviour);
//! - a node fed by a never-executed Job FAILS honestly instead of computing on nothing;
//! - a zero-output SINK stays dirty (it carries no evidence it ever ran).
//!
//! Plus `CancelToken`'s interop with the `Arc<AtomicBool>` that worker APIs actually take.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use nodes_core::{
    BoxValue, CancelToken, NodeDef, NodeLibrary, NodeStatus, NodeType, PortDef, Subnet, cook,
    cook_dirty, default_registry, execute, execute_all, fn_job, fn_node, is_stale,
    load_subnet_from_file, subnet_to_file,
};

/// Counts the runs of each node type's cook, so a test can assert "it did NOT run" as evidence
/// rather than description.
#[derive(Clone, Default)]
struct Runs {
    job: Arc<AtomicUsize>,
    dataflow: Arc<AtomicUsize>,
}

impl Runs {
    fn jobs(&self) -> usize {
        self.job.load(Ordering::Relaxed)
    }
}

/// A miniature of the real thing: `Reconstruct` is a JOB (40 minutes of GPU in the original; here it
/// just records that it ran and emits the path of the artifact it "produced"), and `ReadSparse` is an
/// ordinary dataflow node that consumes the job's output.
struct ReconLib(Runs);

impl NodeLibrary for ReconLib {
    fn name(&self) -> &str {
        "recon"
    }

    fn types(&self) -> Vec<Arc<dyn NodeType>> {
        let job_runs = Arc::clone(&self.0.job);
        let flow_runs = Arc::clone(&self.0.dataflow);
        vec![
            fn_job(
                NodeDef::new("Reconstruct", "Test")
                    .with_inputs(vec![PortDef::string("workspace")])
                    .with_outputs(vec![PortDef::string("sparse")]),
                move |ctx| {
                    job_runs.fetch_add(1, Ordering::Relaxed);
                    ctx.node
                        .set_out("sparse", BoxValue::String("sparse/0".into()));
                    Ok(())
                },
            ),
            fn_node(
                NodeDef::new("ReadSparse", "Test")
                    .with_inputs(vec![PortDef::string("sparse")])
                    .with_outputs(vec![PortDef::int("points", 0)]),
                move |ctx| {
                    flow_runs.fetch_add(1, Ordering::Relaxed);
                    let path = ctx.node.in_str("sparse");
                    ctx.node.set_out("points", BoxValue::I64(path.len() as i64));
                    Ok(())
                },
            ),
        ]
    }
}

/// A `Reconstruct` job wired into a `ReadSparse` dataflow consumer.
fn recon_graph(
    lib: &ReconLib,
) -> (
    Subnet,
    nodes_core::NodeTypeRegistry,
    nodes_core::ValueTypes,
    nodes_core::NodeId,
    nodes_core::NodeId,
) {
    let (mut registry, mut value_types) = default_registry();
    registry.add_library(lib, &mut value_types);
    let mut graph = Subnet::new();
    let job = graph
        .create_node("Reconstruct", [0.0, 0.0], &registry, &value_types)
        .expect("Reconstruct spawns");
    let reader = graph
        .create_node("ReadSparse", [300.0, 0.0], &registry, &value_types)
        .expect("ReadSparse spawns");
    graph
        .connect_idx(job, 0, reader, 0, &registry, &value_types)
        .expect("Reconstruct.sparse -> ReadSparse.sparse");
    (graph, registry, value_types, job, reader)
}

/// **The core guarantee.** A Job is dirty (freshly created, never run) and the automatic cook driver
/// still refuses to run it — that is what makes the crate's "cook is cheap" assumption true by
/// construction, and what stops a GUI from re-running a reconstruction just because it drew a frame.
#[test]
fn the_cook_driver_never_runs_a_job_however_dirty() {
    let runs = Runs::default();
    let lib = ReconLib(runs.clone());
    let (mut graph, registry, value_types, job, _reader) = recon_graph(&lib);

    assert!(
        graph.nodes.get(&job).map(|n| n.dirty()).unwrap_or(false),
        "a fresh job really is dirty — so the zero below is the model, not an empty graph"
    );

    cook(&mut graph, &registry, &value_types);
    cook_dirty(&mut graph, &registry, &value_types);
    cook_dirty(&mut graph, &registry, &value_types);

    assert_eq!(runs.jobs(), 0, "no cook of any kind may execute a job");
    assert!(
        graph
            .nodes
            .get(&job)
            .map(|n| is_stale(n, &registry))
            .unwrap_or(false),
        "the job is left STALE (dirty), for a host to show and a human to execute"
    );
}

/// A dataflow node fed by a job that has never run must FAIL, not quietly compute on its stored
/// literal. Silently computing on nothing is how a graph shows a plausible number that means nothing.
#[test]
fn a_node_fed_by_an_unexecuted_job_fails_honestly() {
    let runs = Runs::default();
    let lib = ReconLib(runs.clone());
    let (mut graph, registry, value_types, _job, reader) = recon_graph(&lib);

    cook(&mut graph, &registry, &value_types);

    let status = graph
        .nodes
        .get(&reader)
        .map(|n| n.status())
        .expect("the reader node");
    match status {
        NodeStatus::Error(msg) => {
            assert!(
                msg.contains("has not been executed"),
                "the error must name the cause, got: {msg}"
            );
        }
        other => panic!("expected an honest error, got {other:?}"),
    }
}

/// `execute` runs the job (and cooks its cheap dataflow dependencies first). This is the ONLY way a
/// job ever runs.
#[test]
fn execute_runs_the_job_and_its_downstream_then_cooks() {
    let runs = Runs::default();
    let lib = ReconLib(runs.clone());
    let (mut graph, registry, value_types, job, reader) = recon_graph(&lib);

    execute(&mut graph, job, &registry, &value_types);
    assert_eq!(runs.jobs(), 1, "the explicit entry point DOES run the job");

    // The job's output is now real, so its consumer cooks normally.
    cook_dirty(&mut graph, &registry, &value_types);
    let points = graph
        .nodes
        .get(&reader)
        .and_then(|n| n.output_value("points"));
    assert_eq!(
        points,
        Some(BoxValue::I64("sparse/0".len() as i64)),
        "the consumer computed from the job's REAL output"
    );
}

/// **The chain that used to re-run a reconstruction, killed at the root.** A job that has run is
/// saved WITH its outputs; on load it comes back CLEAN, and cooking the loaded graph runs nothing.
#[test]
fn a_job_graph_round_trips_clean_and_does_not_re_execute() {
    let runs = Runs::default();
    let lib = ReconLib(runs.clone());
    let (mut graph, registry, value_types, job, _reader) = recon_graph(&lib);

    execute_all(&mut graph, &registry, &value_types);
    assert_eq!(runs.jobs(), 1);

    let file = subnet_to_file(&graph, &registry);
    let mut loaded = Subnet::new();
    load_subnet_from_file(&mut loaded, file, &registry, &value_types).expect("load");

    let node = loaded.nodes.get(&job).expect("the job survived the load");
    assert_eq!(
        node.output_value("sparse"),
        Some(BoxValue::String("sparse/0".into())),
        "a job's outputs are PERSISTED — they record an effect that already happened"
    );
    assert!(
        !node.dirty(),
        "every output came back with a value, so the job is clean: nothing to run"
    );
    assert!(!is_stale(node, &registry), "and therefore not stale");

    // The exact chain: open the graph, let the editor cook. Nothing may execute.
    cook(&mut loaded, &registry, &value_types);
    cook_dirty(&mut loaded, &registry, &value_types);
    assert_eq!(
        runs.jobs(),
        1,
        "opening the graph must not re-execute the job that produced it"
    );
}

/// The dataflow half is untouched: outputs blanked on save, node dirty on load, recomputed. Cheap
/// recompute stays cheap recompute — the model only changes what a JOB does.
#[test]
fn a_dataflow_node_is_still_blanked_and_recomputed() {
    let (registry, value_types) = default_registry();
    let mut graph = Subnet::new();
    let id = graph
        .create_node("Float", [0.0, 0.0], &registry, &value_types)
        .expect("builtin Float");
    cook(&mut graph, &registry, &value_types);

    let file = subnet_to_file(&graph, &registry);
    let mut loaded = Subnet::new();
    load_subnet_from_file(&mut loaded, file, &registry, &value_types).expect("load");

    let node = loaded.nodes.get(&id).expect("node present");
    let out = node.output_names();
    let first = out.first().expect("Float declares an output");
    assert!(
        matches!(node.output_value(first), None | Some(BoxValue::Null)),
        "a dataflow output is blanked on save"
    );
    assert!(node.dirty(), "and therefore dirty on load");

    cook_dirty(&mut loaded, &registry, &value_types);
    assert!(
        !loaded.nodes.get(&id).map(|n| n.dirty()).unwrap_or(true),
        "and recomputed by an ordinary cook"
    );
}

/// A zero-output SINK (`BusWrite`, `SaveImage`) runs purely for its side effect, so "no output is
/// missing" is VACUOUSLY true for it. Read naively, the dirty rule would retire every sink in a loaded
/// graph — BusWrite would never re-publish and every BusRead would read an empty bus, silently killing
/// the whole Data Bus feature across a save/load. A sink must come back DIRTY.
#[test]
fn a_zero_output_sink_is_dirty_on_load() {
    let (registry, value_types) = default_registry();
    let mut graph = Subnet::new();
    let id = graph
        .create_node("BusWrite", [0.0, 0.0], &registry, &value_types)
        .expect("builtin BusWrite");
    cook(&mut graph, &registry, &value_types);

    let file = subnet_to_file(&graph, &registry);
    let mut loaded = Subnet::new();
    load_subnet_from_file(&mut loaded, file, &registry, &value_types).expect("load");

    let node = loaded.nodes.get(&id).expect("node present");
    assert!(
        node.output_names().is_empty(),
        "BusWrite is a zero-output sink"
    );
    assert!(
        node.dirty(),
        "a sink cannot be assumed done — it must cook again to re-publish its side effect"
    );
}

/// `as_arc` hands out the SAME flag, not a copy: a worker API that knows nothing about this crate can
/// poll it inside its own loop, and setting it cancels the token.
#[test]
fn as_arc_shares_the_flag_with_the_token() {
    let token = CancelToken::default();
    let flag: Arc<AtomicBool> = token.as_arc();

    assert!(!token.is_cancelled());
    flag.store(true, Ordering::Relaxed);
    assert!(
        token.is_cancelled(),
        "as_arc must expose the token's own flag, not a detached copy"
    );
}

/// `from_arc` adopts a flag the HOST already owns (a Ctrl-C handler's), so there is ONE flag rather
/// than a host flag and a token that drift apart.
#[test]
fn from_arc_adopts_the_hosts_flag_in_both_directions() {
    let host_flag: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let token = CancelToken::from_arc(Arc::clone(&host_flag));

    assert!(!token.is_cancelled());
    host_flag.store(true, Ordering::Relaxed);
    assert!(
        token.is_cancelled(),
        "the host's Ctrl-C flag cancels the token"
    );

    let token2 = CancelToken::from_arc(Arc::new(AtomicBool::new(false)));
    let worker_flag = token2.as_arc();
    token2.cancel();
    assert!(
        worker_flag.load(Ordering::Relaxed),
        "and cancelling the token is seen by whoever holds the flag"
    );
}
