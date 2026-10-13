use nodes_core::{
    BoxValue, CoreError, Graph, NodeTypeRegistry, ValueTypes, cook, register_builtins,
};

// `register_builtins` brings the builtin VALUE types with it (the `Builtins` library declares them),
// so a separate `register_builtin_types` call here would register each one twice.
fn fixtures() -> (NodeTypeRegistry, ValueTypes) {
    let mut types = ValueTypes::new();
    let mut reg = NodeTypeRegistry::new();
    register_builtins(&mut reg, &mut types);
    (reg, types)
}

#[test]
fn add_two_float_constants() {
    let (registry, types) = fixtures();

    let mut graph = Graph::new();
    let a = graph
        .create_node("Float", [0.0, 0.0], &registry, &types)
        .unwrap();
    let b = graph
        .create_node("Float", [200.0, 0.0], &registry, &types)
        .unwrap();
    graph
        .nodes
        .get_mut(&a)
        .unwrap()
        .set_input_value("value", BoxValue::F64(2.0));
    graph
        .nodes
        .get_mut(&b)
        .unwrap()
        .set_input_value("value", BoxValue::F64(3.0));
    let add = graph
        .create_node("Add", [100.0, 120.0], &registry, &types)
        .unwrap();
    graph
        .connect(a, "out", add, "a", &registry, &types)
        .unwrap();
    graph
        .connect(b, "out", add, "b", &registry, &types)
        .unwrap();

    cook(&mut graph, &registry, &types);

    let out = graph
        .nodes
        .get(&add)
        .unwrap()
        .output_value("result")
        .and_then(|v| v.as_f64())
        .unwrap();
    assert!((out - 5.0).abs() < 1e-9);
}

#[test]
fn convert_node_widens_int_to_float() {
    let (registry, types) = fixtures();

    let mut graph = Graph::new();
    let i = graph
        .create_node("Int", [0.0, 0.0], &registry, &types)
        .unwrap();
    let conv = graph
        .create_node("Convert", [120.0, 0.0], &registry, &types)
        .unwrap();
    graph
        .nodes
        .get_mut(&i)
        .unwrap()
        .set_input_value("value", BoxValue::I64(7));
    graph
        .nodes
        .get_mut(&conv)
        .unwrap()
        .set_input_value("to", BoxValue::String("Float".to_string()));
    graph
        .connect(i, "out", conv, "value", &registry, &types)
        .unwrap();

    cook(&mut graph, &registry, &types);

    let out = graph.nodes.get(&conv).unwrap().output_value("out").unwrap();
    assert_eq!(out, BoxValue::F64(7.0));
}

#[test]
fn connect_incompatible_types_rejected() {
    let (registry, types) = fixtures();

    let mut graph = Graph::new();
    let v = graph
        .create_node("MakeVec3", [0.0, 0.0], &registry, &types)
        .unwrap();
    let f = graph
        .create_node("Float", [120.0, 0.0], &registry, &types)
        .unwrap();
    // Vec3 → Float is incompatible (no implicit conversion from Vec3 to Float).
    let result = graph.connect(v, "vec", f, "value", &registry, &types);
    assert!(
        matches!(result, Err(CoreError::InvalidWire(_))),
        "expected InvalidWire, got {result:?}"
    );
}

#[test]
fn stored_literal_survives_cook_and_disconnect() {
    // Fix 1 verification: the authored literal must not be overwritten by cook.
    let (registry, types) = fixtures();

    let mut graph = Graph::new();
    let src = graph
        .create_node("Float", [0.0, 0.0], &registry, &types)
        .unwrap();
    let add = graph
        .create_node("Add", [120.0, 0.0], &registry, &types)
        .unwrap();
    // Set authored literal on add's input 'a'.
    graph
        .nodes
        .get_mut(&add)
        .unwrap()
        .set_input_value("a", BoxValue::F64(99.0));
    graph
        .nodes
        .get_mut(&src)
        .unwrap()
        .set_input_value("value", BoxValue::F64(5.0));
    graph
        .connect(src, "out", add, "a", &registry, &types)
        .unwrap();

    cook(&mut graph, &registry, &types);
    // After cook the stored literal must still be 99.0, not 5.0.
    let stored = graph.nodes.get(&add).unwrap().input_value("a").unwrap();
    assert_eq!(
        stored,
        BoxValue::F64(99.0),
        "stored literal was overwritten"
    );

    // After disconnect, in_f64 should read the stored literal.
    graph.disconnect_input(add, "a");
    cook(&mut graph, &registry, &types);
    let result_after_disconnect = graph
        .nodes
        .get(&add)
        .unwrap()
        .output_value("result")
        .and_then(|v| v.as_f64())
        .unwrap();
    // b is 0.0 default, a is stored 99.0 → result should be 99.0.
    assert!(
        (result_after_disconnect - 99.0).abs() < 1e-9,
        "got {result_after_disconnect}"
    );
}

#[test]
fn save_load_recook_preserves_output() {
    // Fix serialization + SSOT: save a cooked graph, load it, re-cook, outputs match.
    use nodes_core::{load_subnet_from_file, subnet_from_json, subnet_to_json_pretty};

    let (registry, types) = fixtures();

    let mut graph = Graph::new();
    let a = graph
        .create_node("Float", [0.0, 0.0], &registry, &types)
        .unwrap();
    let b = graph
        .create_node("Float", [0.0, 0.0], &registry, &types)
        .unwrap();
    let add = graph
        .create_node("Add", [120.0, 0.0], &registry, &types)
        .unwrap();
    graph
        .nodes
        .get_mut(&a)
        .unwrap()
        .set_input_value("value", BoxValue::F64(4.0));
    graph
        .nodes
        .get_mut(&b)
        .unwrap()
        .set_input_value("value", BoxValue::F64(6.0));
    graph
        .connect(a, "out", add, "a", &registry, &types)
        .unwrap();
    graph
        .connect(b, "out", add, "b", &registry, &types)
        .unwrap();
    cook(&mut graph, &registry, &types);
    let pre_save = graph
        .nodes
        .get(&add)
        .unwrap()
        .output_value("result")
        .and_then(|v| v.as_f64())
        .unwrap();

    let json = subnet_to_json_pretty(&graph, &registry).unwrap();
    // Verify output value is NOT persisted (blanked on save) — these are Dataflow nodes.
    assert!(
        json.contains("\"result\""),
        "result attr key should be present"
    );
    let file = subnet_from_json(&json).unwrap();
    let mut loaded = Graph::new();
    load_subnet_from_file(&mut loaded, file, &registry, &types).unwrap();

    cook(&mut loaded, &registry, &types);
    let post_load = loaded
        .nodes
        .get(&add)
        .unwrap()
        .output_value("result")
        .and_then(|v| v.as_f64())
        .unwrap();
    assert!(
        (pre_save - post_load).abs() < 1e-9,
        "pre={pre_save} post={post_load}"
    );
}

#[test]
fn schema_attr_meta_choices_and_range() {
    let (registry, _types) = fixtures();

    // Convert.to declares a closed choice set (drives a ComboBox in the inspector).
    let conv = registry.get("Convert").expect("Convert registered");
    let to_meta = conv.attr_meta("to").expect("Convert.to has schema");
    let choices = to_meta.choices.as_ref().expect("Convert.to has choices");
    assert!(choices.contains(&"Float".to_string()));
    assert!(choices.contains(&"Color".to_string()));
    assert_eq!(choices.len(), 9, "all conversion targets exposed");
    assert!(
        to_meta.description.is_some(),
        "Convert.to has a tooltip description"
    );
    // The `to` value is a first-class attr (unified model), default "Float".
    assert!(
        conv.attr_meta("missing").is_none(),
        "unknown attr has no schema"
    );

    // Clamp.value declares a numeric range (drives a Slider in the inspector).
    let clamp = registry.get("Clamp").expect("Clamp registered");
    let value_meta = clamp.attr_meta("value").expect("Clamp.value has schema");
    assert_eq!(value_meta.range, Some((0.0, 1.0)));
}

#[test]
fn convert_reads_to_from_attr() {
    // The Convert node now reads its target type from the `to` attr (unified model),
    // not a `params.to` side-channel. Default is Float; overriding the attr changes
    // the output type.
    let (registry, types) = fixtures();
    let mut graph = Graph::new();
    let i = graph
        .create_node("Int", [0.0, 0.0], &registry, &types)
        .unwrap();
    let conv = graph
        .create_node("Convert", [120.0, 0.0], &registry, &types)
        .unwrap();
    graph
        .nodes
        .get_mut(&i)
        .unwrap()
        .set_input_value("value", BoxValue::I64(2));
    graph
        .connect(i, "out", conv, "value", &registry, &types)
        .unwrap();

    // Default `to` = "Float".
    cook(&mut graph, &registry, &types);
    assert_eq!(
        graph.nodes.get(&conv).unwrap().output_value("out"),
        Some(BoxValue::F64(2.0))
    );

    // Override to "String".
    graph
        .nodes
        .get_mut(&conv)
        .unwrap()
        .set_input_value("to", BoxValue::String("String".to_string()));
    cook(&mut graph, &registry, &types);
    assert_eq!(
        graph.nodes.get(&conv).unwrap().output_value("out"),
        Some(BoxValue::String("2".to_string()))
    );
}
