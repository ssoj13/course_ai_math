use nodes_core::{BoxValue, Graph, NodeTypeRegistry, ValueTypes, cook, register_builtins};

// `register_builtins` brings the builtin VALUE types with it (the `Builtins` library declares them),
// so a separate `register_builtin_types` call here would register each one twice.
fn fixtures() -> (NodeTypeRegistry, ValueTypes) {
    let mut types = ValueTypes::new();
    let mut reg = NodeTypeRegistry::new();
    register_builtins(&mut reg, &mut types);
    (reg, types)
}

#[test]
fn bus_write_read_roundtrip() {
    let (reg, types) = fixtures();
    let mut graph = Graph::new();
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

    assert!(graph.bus().contains("data"));
    let read_out = graph
        .nodes
        .get(&read)
        .unwrap()
        .output_value("value")
        .and_then(|v| v.as_f64())
        .unwrap();
    assert!((read_out - 42.0).abs() < 1e-6);
}
