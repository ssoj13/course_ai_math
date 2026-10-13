//! Op paths and address resolution over the flat graph (see `docs/NODES.md`).

use nodes_core::register_builtins;
use nodes_core::{
    AddressTail, BoxValue, Graph, NodeTypeRegistry, ResolvedTarget, ValueTypes, op_path_of,
    parse_address, parse_op_path, resolve_address, resolve_op,
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
fn parse_parm_tail_on_last_segment() {
    let addr = parse_address("/geo1/box1.frequency").unwrap();
    assert_eq!(addr.path.segments, ["geo1", "box1"]);
    assert!(matches!(addr.tail, AddressTail::Param(_)));
}

#[test]
fn parse_bus_slot() {
    let addr = parse_address("@data").unwrap();
    assert!(matches!(addr.tail, AddressTail::BusSlot(ref s) if s == "data"));
}

#[test]
fn nested_resolve_and_op_path_roundtrip() {
    let (reg, types) = fixtures();
    let mut root = Graph::new();
    let geo = root
        .create_node("Subnet", [0.0, 0.0], &reg, &types)
        .unwrap();
    let geo_name = root.nodes.get(&geo).unwrap().name().to_string();
    let box_id = root
        .create_node("Float", [50.0, 50.0], &reg, &types)
        .unwrap();
    root.set_parent(box_id, Some(geo));
    let box_name = root.nodes.get(&box_id).unwrap().name().to_string();

    let path = parse_op_path(&format!("/{geo_name}/{box_name}")).unwrap();
    let loc = resolve_op(&root, &path).unwrap();
    assert_eq!(loc.node_id, box_id);

    let back = op_path_of(&root, box_id).unwrap();
    assert_eq!(back.segments, [geo_name, box_name]);
}

#[test]
fn strip_virtual_prefixes_obj_out_mat() {
    let (reg, types) = fixtures();
    let mut root = Graph::new();
    let id = root.create_node("Float", [0.0, 0.0], &reg, &types).unwrap();
    let name = root.nodes.get(&id).unwrap().name().to_string();

    for prefix in ["obj", "out", "mat"] {
        let path = parse_op_path(&format!("/{prefix}/{name}")).unwrap();
        let loc = resolve_op(&root, &path).unwrap();
        assert_eq!(loc.node_id, id);
    }
}

#[test]
fn resolve_bus_uses_focus_graph() {
    let (reg, _types) = fixtures();
    let mut root = Graph::new();
    root.bus_mut().set("data", BoxValue::F64(42.0));
    let addr = parse_address("@data").unwrap();
    let target = resolve_address(&root, &root, &reg, &addr).unwrap();
    match target {
        ResolvedTarget::BusSlot { slot, .. } => assert_eq!(slot, "data"),
        _ => panic!("expected bus slot"),
    }
}

#[test]
fn rename_changes_path_not_id() {
    let (reg, types) = fixtures();
    let mut root = Graph::new();
    let id = root.create_node("Float", [0.0, 0.0], &reg, &types).unwrap();
    let old_path = op_path_of(&root, id).unwrap();
    nodes_core::rename_node(&mut root, id, "my_float").unwrap();
    let new_path = op_path_of(&root, id).unwrap();
    assert_ne!(old_path.format(), new_path.format());
    assert_eq!(new_path.segments, ["my_float"]);
}
