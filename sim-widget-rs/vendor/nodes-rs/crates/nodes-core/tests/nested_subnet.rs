//! Subnet evaluation and `remove_node` integrity tests.
//!
//! Updated for the bag-based subnet model (PR1):
//!   Subnet      : [ Any "in" ] => [ Map "out" ]
//!   SubnetInput : []           => [ Map "bag" ]   (seeded by prestage from parent inputs)
//!   SubnetOutput: [ Map "bag"] => []              (read by prestage → stashed as __subnet_out__)
//!
//! The prestage packs ALL parent input names into `{"in": value, ...}` and seeds
//! SubnetInput.bag with that Object before cooking children.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nodes_core::{
    BoxValue, Graph, NodeDef, NodeTypeRegistry, ValueTypes, cook, cook_dirty, default_registry,
    fn_node,
};

fn fixtures() -> (NodeTypeRegistry, ValueTypes) {
    default_registry()
}

/// Single-level subnet: parent Subnet has literal "in" = 10.0.
/// Prestage packs in_bag = {"in": 10.0} → SubnetInput.bag.
/// Inner: SI.bag → MapGet("in") → 10.0 → Add(b=5) → 15.0 → MapInsert("result") → SO.bag.
/// Expected: parent.out == Object{"result": 15.0}.
#[test]
fn nested_subnet_evaluates_child() {
    let (reg, types) = fixtures();
    let mut graph = Graph::new();

    let parent = graph
        .create_node("Subnet", [0.0, 0.0], &reg, &types)
        .unwrap();
    // Subnet "in" literal = 10.0; prestage will pack {"in": 10.0}.
    graph
        .nodes
        .get_mut(&parent)
        .unwrap()
        .set_input_value("in", BoxValue::F64(10.0));

    let si = graph
        .create_node("SubnetInput", [0.0, 0.0], &reg, &types)
        .unwrap();
    let mg = graph
        .create_node("MapGet", [100.0, 0.0], &reg, &types)
        .unwrap();
    let add = graph
        .create_node("Add", [200.0, 0.0], &reg, &types)
        .unwrap();
    let mi = graph
        .create_node("MapInsert", [300.0, 0.0], &reg, &types)
        .unwrap();
    let so = graph
        .create_node("SubnetOutput", [400.0, 0.0], &reg, &types)
        .unwrap();

    for child in [si, mg, add, mi, so] {
        graph.set_parent(child, Some(parent));
    }

    // MapGet reads "in" from the bag (= 10.0).
    graph
        .nodes
        .get_mut(&mg)
        .unwrap()
        .set_input_value("key", BoxValue::String("in".to_string()));
    // Add b = 5.0; a comes from wire.
    graph
        .nodes
        .get_mut(&add)
        .unwrap()
        .set_input_value("b", BoxValue::F64(5.0));
    // MapInsert writes "result".
    graph
        .nodes
        .get_mut(&mi)
        .unwrap()
        .set_input_value("key", BoxValue::String("result".to_string()));

    graph.connect(si, "bag", mg, "map", &reg, &types).unwrap();
    graph.connect(mg, "value", add, "a", &reg, &types).unwrap();
    graph
        .connect(add, "result", mi, "value", &reg, &types)
        .unwrap();
    graph.connect(mi, "out", so, "bag", &reg, &types).unwrap();

    cook(&mut graph, &reg, &types);

    let out = graph.nodes[&parent]
        .output_value("out")
        .expect("parent 'out' must be set");
    let map = out.as_object().expect("'out' must be an Object");
    let result = map
        .get("result")
        .and_then(|v| v.as_f64())
        .expect("'result' key must exist");
    assert!((result - 15.0).abs() < 1e-6, "expected 15.0, got {result}");
}

/// Depth-2 nested subnet.
///
/// subnet_a (top-level):
///   "in" literal = 7.0 → prestage packs {"in": 7.0} → SI_A.bag.
///   Inner: SI_A.bag → MG_A("in") → 7.0 → subnet_b.in.
///
/// subnet_b (child of subnet_a):
///   "in" = 7.0 (from wire) → prestage packs {"in": 7.0} → SI_B.bag.
///   Inner: SI_B.bag → MG_B("in") → Add(a=7, b=7) → 14.0 → MI_B("v") → {"v":14.0} → SO_B.
///   subnet_b.out = {"v": 14.0}.
///
/// Back in subnet_a:
///   subnet_b.out → MI_A("result") → {"result": {"v":14.0}} → SO_A.
///   subnet_a.out = {"result": {"v": 14.0}}.
///
/// Also verifies grandchild TestCounter is cooked EXACTLY ONCE (double-cook regression).
#[test]
fn depth2_nested_subnet_no_double_cook() {
    let (mut reg, mut types) = fixtures();

    // A port-less node that counts how many times it is cooked.
    let counter = Arc::new(AtomicUsize::new(0));
    {
        let counter_in_cook = counter.clone();
        reg.add(
            fn_node(NodeDef::new("TestCounter", "Test"), move |_ctx| {
                counter_in_cook.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
            &mut types,
        );
    }

    let mut graph = Graph::new();

    // subnet_a: "in" = 7.0.
    let sa = graph
        .create_node("Subnet", [0.0, 0.0], &reg, &types)
        .unwrap();
    graph
        .nodes
        .get_mut(&sa)
        .unwrap()
        .set_input_value("in", BoxValue::F64(7.0));

    // subnet_a children: SI_A, MG_A, subnet_b, MI_A, SO_A.
    let sa_si = graph
        .create_node("SubnetInput", [0.0, 0.0], &reg, &types)
        .unwrap();
    let sa_mg = graph
        .create_node("MapGet", [100.0, 0.0], &reg, &types)
        .unwrap();
    let sb = graph
        .create_node("Subnet", [200.0, 0.0], &reg, &types)
        .unwrap();
    let sa_mi = graph
        .create_node("MapInsert", [350.0, 0.0], &reg, &types)
        .unwrap();
    let sa_so = graph
        .create_node("SubnetOutput", [450.0, 0.0], &reg, &types)
        .unwrap();

    for c in [sa_si, sa_mg, sb, sa_mi, sa_so] {
        graph.set_parent(c, Some(sa));
    }
    // MG_A extracts key "in" from in_bag → 7.0.
    graph
        .nodes
        .get_mut(&sa_mg)
        .unwrap()
        .set_input_value("key", BoxValue::String("in".to_string()));
    // MI_A inserts key "result" with subnet_b.out (a Map) into an empty map.
    graph
        .nodes
        .get_mut(&sa_mi)
        .unwrap()
        .set_input_value("key", BoxValue::String("result".to_string()));

    // subnet_a wiring: SI_A.bag → MG_A.map; MG_A.value → sb.in; sb.out → MI_A.map (value via MI); MI_A.out → SO_A.bag.
    // MI_A.value = subnet_b.out (the result Map); MI_A.map = default (empty Object).
    graph
        .connect(sa_si, "bag", sa_mg, "map", &reg, &types)
        .unwrap();
    graph
        .connect(sa_mg, "value", sb, "in", &reg, &types)
        .unwrap();
    graph
        .connect(sb, "out", sa_mi, "value", &reg, &types)
        .unwrap();
    graph
        .connect(sa_mi, "out", sa_so, "bag", &reg, &types)
        .unwrap();

    // subnet_b children: SI_B, MG_B, Add, counter, MI_B, SO_B.
    let sb_si = graph
        .create_node("SubnetInput", [0.0, 0.0], &reg, &types)
        .unwrap();
    let sb_mg = graph
        .create_node("MapGet", [100.0, 0.0], &reg, &types)
        .unwrap();
    let sb_add = graph
        .create_node("Add", [200.0, 0.0], &reg, &types)
        .unwrap();
    let sb_mi = graph
        .create_node("MapInsert", [300.0, 0.0], &reg, &types)
        .unwrap();
    let sb_so = graph
        .create_node("SubnetOutput", [400.0, 0.0], &reg, &types)
        .unwrap();
    let ctr = graph
        .create_node("TestCounter", [0.0, 100.0], &reg, &types)
        .unwrap();

    for c in [sb_si, sb_mg, sb_add, sb_mi, sb_so, ctr] {
        graph.set_parent(c, Some(sb));
    }
    // MG_B extracts key "in" from in_bag (7.0 was packed under "in" by subnet_b's prestage).
    graph
        .nodes
        .get_mut(&sb_mg)
        .unwrap()
        .set_input_value("key", BoxValue::String("in".to_string()));
    // MI_B writes key "v".
    graph
        .nodes
        .get_mut(&sb_mi)
        .unwrap()
        .set_input_value("key", BoxValue::String("v".to_string()));

    // subnet_b wiring: SI_B.bag → MG_B.map; MG_B.value → Add.a AND Add.b (doubles it);
    // Add.result → MI_B.value; MI_B.out → SO_B.bag.
    graph
        .connect(sb_si, "bag", sb_mg, "map", &reg, &types)
        .unwrap();
    graph
        .connect(sb_mg, "value", sb_add, "a", &reg, &types)
        .unwrap();
    graph
        .connect(sb_mg, "value", sb_add, "b", &reg, &types)
        .unwrap();
    graph
        .connect(sb_add, "result", sb_mi, "value", &reg, &types)
        .unwrap();
    graph
        .connect(sb_mi, "out", sb_so, "bag", &reg, &types)
        .unwrap();

    cook(&mut graph, &reg, &types);

    // subnet_a.out = Object{"result": Object{"v": 14.0}}.
    let sa_out = graph.nodes[&sa]
        .output_value("out")
        .expect("sa 'out' must be set");
    let sa_map = sa_out.as_object().expect("sa 'out' must be Object");
    let result_val = sa_map.get("result").expect("'result' key must exist");
    let result_map = result_val.as_object().expect("'result' must be an Object");
    let v = result_map
        .get("v")
        .and_then(|x| x.as_f64())
        .unwrap_or(f64::NAN);
    assert!(
        (v - 14.0).abs() < 1e-6,
        "expected 14.0 (7 doubled), got {v}\nsa_out={sa_out:?}"
    );

    // Grandchild TestCounter must be cooked EXACTLY once.
    let count = counter.load(Ordering::SeqCst);
    assert_eq!(
        count, 1,
        "grandchild cooked {count} times (double-cook regression)"
    );
}

/// C1 regression: editing an INNER child and re-cooking via `cook_dirty` (the editor
/// hot path) must refresh the host Subnet's output. Before the parent-ascent fix in
/// `cook_dirty`, a dirty inner child never re-cooked the host → stale "out".
#[test]
fn cook_dirty_inner_edit_refreshes_host() {
    let (reg, types) = fixtures();
    let mut graph = Graph::new();

    let parent = graph
        .create_node("Subnet", [0.0, 0.0], &reg, &types)
        .unwrap();
    graph
        .nodes
        .get_mut(&parent)
        .unwrap()
        .set_input_value("in", BoxValue::F64(10.0));

    let si = graph
        .create_node("SubnetInput", [0.0, 0.0], &reg, &types)
        .unwrap();
    let mg = graph
        .create_node("MapGet", [0.0, 0.0], &reg, &types)
        .unwrap();
    let add = graph.create_node("Add", [0.0, 0.0], &reg, &types).unwrap();
    let mi = graph
        .create_node("MapInsert", [0.0, 0.0], &reg, &types)
        .unwrap();
    let so = graph
        .create_node("SubnetOutput", [0.0, 0.0], &reg, &types)
        .unwrap();
    for c in [si, mg, add, mi, so] {
        graph.set_parent(c, Some(parent));
    }

    graph
        .nodes
        .get_mut(&mg)
        .unwrap()
        .set_input_value("key", BoxValue::String("in".to_string()));
    graph
        .nodes
        .get_mut(&mi)
        .unwrap()
        .set_input_value("key", BoxValue::String("result".to_string()));
    graph
        .nodes
        .get_mut(&add)
        .unwrap()
        .set_input_value("b", BoxValue::F64(5.0));

    graph.connect(si, "bag", mg, "map", &reg, &types).unwrap();
    graph.connect(mg, "value", add, "a", &reg, &types).unwrap();
    graph
        .connect(add, "result", mi, "value", &reg, &types)
        .unwrap();
    graph.connect(mi, "out", so, "bag", &reg, &types).unwrap();

    // Initial full cook: out = {result: 15.0}.
    cook(&mut graph, &reg, &types);
    let r0 = graph.nodes[&parent]
        .output_value("out")
        .unwrap()
        .as_object()
        .unwrap()
        .get("result")
        .and_then(|v| v.as_f64())
        .unwrap();
    assert!(
        (r0 - 15.0).abs() < 1e-6,
        "initial cook expected 15.0, got {r0}"
    );

    // Edit ONLY the inner Add child (b: 5 → 25), mark it dirty, then cook_dirty (no
    // full cook). The host must refresh to {result: 35.0} via the parent-ascent fix.
    graph
        .nodes
        .get_mut(&add)
        .unwrap()
        .set_input_value("b", BoxValue::F64(25.0));
    graph.nodes.get_mut(&add).unwrap().set_dirty(true);
    cook_dirty(&mut graph, &reg, &types);

    let r1 = graph.nodes[&parent]
        .output_value("out")
        .unwrap()
        .as_object()
        .unwrap()
        .get("result")
        .and_then(|v| v.as_f64())
        .unwrap();
    assert!(
        (r1 - 35.0).abs() < 1e-6,
        "after inner edit + cook_dirty expected 35.0, got {r1} (stale host = C1 bug)"
    );
}

/// Malformed subnet: children present but NO SubnetOutput → host "out" is an empty
/// Object (a warning is logged; we assert the soft, well-defined result here).
#[test]
fn subnet_missing_output_emits_empty() {
    let (reg, types) = fixtures();
    let mut graph = Graph::new();
    let parent = graph
        .create_node("Subnet", [0.0, 0.0], &reg, &types)
        .unwrap();
    graph
        .nodes
        .get_mut(&parent)
        .unwrap()
        .set_input_value("in", BoxValue::F64(1.0));
    let si = graph
        .create_node("SubnetInput", [0.0, 0.0], &reg, &types)
        .unwrap();
    let mi = graph
        .create_node("MapInsert", [0.0, 0.0], &reg, &types)
        .unwrap();
    graph.set_parent(si, Some(parent));
    graph.set_parent(mi, Some(parent));

    cook(&mut graph, &reg, &types);
    let out = graph.nodes[&parent]
        .output_value("out")
        .expect("'out' must be set");
    assert!(
        out.is_object() && out.as_object().unwrap().is_empty(),
        "missing SubnetOutput must yield an empty Object, got {out:?}"
    );
}

/// SubnetOutput present but its "bag" input unconnected → host "out" falls back to
/// the stored default (empty Object) via the literal branch.
#[test]
fn unconnected_subnet_output_is_empty() {
    let (reg, types) = fixtures();
    let mut graph = Graph::new();
    let parent = graph
        .create_node("Subnet", [0.0, 0.0], &reg, &types)
        .unwrap();
    let si = graph
        .create_node("SubnetInput", [0.0, 0.0], &reg, &types)
        .unwrap();
    let so = graph
        .create_node("SubnetOutput", [0.0, 0.0], &reg, &types)
        .unwrap();
    graph.set_parent(si, Some(parent));
    graph.set_parent(so, Some(parent));

    cook(&mut graph, &reg, &types);
    let out = graph.nodes[&parent]
        .output_value("out")
        .expect("'out' must be set");
    assert!(
        out.is_object() && out.as_object().unwrap().is_empty(),
        "unconnected SubnetOutput must yield an empty Object, got {out:?}"
    );
}

/// Type gate: a "Map" output cannot wire into a scalar (Float) input, and a scalar
/// output cannot wire into a "Map" input. `connect` must reject both directions.
#[test]
fn wire_gate_rejects_map_scalar_mismatch() {
    let (reg, types) = fixtures();
    let mut graph = Graph::new();
    let mi = graph
        .create_node("MapInsert", [0.0, 0.0], &reg, &types)
        .unwrap(); // out: Map
    let add = graph.create_node("Add", [0.0, 0.0], &reg, &types).unwrap(); // a: Float
    let flt = graph
        .create_node("Float", [0.0, 0.0], &reg, &types)
        .unwrap(); // out: Float
    let mg = graph
        .create_node("MapGet", [0.0, 0.0], &reg, &types)
        .unwrap(); // map: Map

    assert!(
        graph.connect(mi, "out", add, "a", &reg, &types).is_err(),
        "Map output into Float input must be rejected"
    );
    assert!(
        graph.connect(flt, "out", mg, "map", &reg, &types).is_err(),
        "Float output into Map input must be rejected"
    );
}

/// After `remove_node`, no dangling source, parent, or children refs remain.
#[test]
fn remove_node_no_dangling_refs() {
    let (reg, types) = fixtures();
    let mut graph = Graph::new();

    let src = graph
        .create_node("Float", [0.0, 0.0], &reg, &types)
        .unwrap();
    let add = graph
        .create_node("Add", [120.0, 0.0], &reg, &types)
        .unwrap();
    let child = graph
        .create_node("Float", [60.0, 0.0], &reg, &types)
        .unwrap();

    graph.connect(src, "out", add, "a", &reg, &types).unwrap();
    graph.set_parent(child, Some(src));

    assert!(graph.nodes.get(&add).unwrap().source_of("a").is_some());
    assert_eq!(graph.nodes.get(&child).unwrap().parent(), Some(src));
    assert!(graph.nodes.get(&src).unwrap().children().contains(&child));

    assert!(graph.remove_node(src));

    assert!(!graph.nodes.contains_key(&src));
    assert!(
        graph.nodes.get(&add).unwrap().source_of("a").is_none(),
        "dangling source ref not cleared"
    );
    assert_eq!(
        graph.nodes.get(&child).unwrap().parent(),
        None,
        "child still points to removed parent"
    );
    assert!(graph.index.in_edge(add, "a").is_none());
}

/// `remove_many` batch-deletes with the same integrity as `remove_node`: dangling source
/// refs on survivors are cleared and surviving children are reparented to root.
#[test]
fn remove_many_batch_integrity() {
    let (reg, types) = fixtures();
    let mut graph = Graph::new();

    let a = graph
        .create_node("Float", [0.0, 0.0], &reg, &types)
        .unwrap();
    let b = graph
        .create_node("Float", [60.0, 0.0], &reg, &types)
        .unwrap();
    let add = graph
        .create_node("Add", [120.0, 0.0], &reg, &types)
        .unwrap();
    let child = graph
        .create_node("Float", [180.0, 0.0], &reg, &types)
        .unwrap();

    graph.connect(a, "out", add, "a", &reg, &types).unwrap();
    graph.connect(b, "out", add, "b", &reg, &types).unwrap();
    graph.set_parent(child, Some(a));

    // Delete both sources in one batch: `add` survives with both inputs cleared, and the
    // surviving child of `a` is reparented to root.
    graph.remove_many(&[a, b]);

    assert!(!graph.nodes.contains_key(&a));
    assert!(!graph.nodes.contains_key(&b));
    assert!(graph.nodes.contains_key(&add));
    assert!(
        graph.nodes.get(&add).unwrap().source_of("a").is_none(),
        "source from removed `a` not cleared"
    );
    assert!(
        graph.nodes.get(&add).unwrap().source_of("b").is_none(),
        "source from removed `b` not cleared"
    );
    assert_eq!(
        graph.nodes.get(&child).unwrap().parent(),
        None,
        "surviving child not reparented to root"
    );
    assert!(graph.index.in_edge(add, "a").is_none());
    assert!(graph.index.in_edge(add, "b").is_none());
}

/// `create_subnet` seeds a Subnet with a wired SubnetInput→SubnetOutput pair, both parented
/// inside it — a pass-through compound with no dangling I/O (no cook warning).
#[test]
fn create_subnet_seeds_wired_io_children() {
    let (reg, types) = fixtures();
    let mut graph = Graph::new();

    let subnet = nodes_core::create_subnet(&mut graph, [0.0, 0.0], &reg, &types).unwrap();
    assert_eq!(graph.nodes.get(&subnet).unwrap().type_name(), "Subnet");

    let children = graph.children_of(subnet);
    assert_eq!(
        children.len(),
        2,
        "subnet must be seeded with exactly two helpers"
    );

    let mut kinds: Vec<&str> = children
        .iter()
        .map(|c| graph.nodes.get(c).unwrap().type_name())
        .collect();
    kinds.sort_unstable();
    assert_eq!(kinds, vec!["SubnetInput", "SubnetOutput"]);

    // Both children parented to the subnet.
    for c in &children {
        assert_eq!(graph.nodes.get(c).unwrap().parent(), Some(subnet));
    }

    // SubnetOutput.bag is wired from SubnetInput (the auto pass-through).
    let (in_id, out_id) = {
        let si = children
            .iter()
            .copied()
            .find(|c| graph.nodes.get(c).unwrap().type_name() == "SubnetInput")
            .unwrap();
        let so = children
            .iter()
            .copied()
            .find(|c| graph.nodes.get(c).unwrap().type_name() == "SubnetOutput")
            .unwrap();
        (si, so)
    };
    let src = graph.nodes.get(&out_id).unwrap().source_of("bag");
    assert_eq!(
        src.map(|(n, _)| n),
        Some(in_id),
        "SubnetOutput.bag must be fed by SubnetInput"
    );
}
