//! Per-instance input ports: [`Node::add_input`].
//!
//! A [`NodeDef`]'s ports belong to the TYPE, so every instance of a type has the same set. That is
//! right for most nodes and wrong for one shape: a node whose fan-in is decided per instance — a
//! compositor's layer stack, a merge with N branches — where the port set IS the document.
//! Without a per-instance API such a host must either declare a fixed maximum and draw dead pins
//! forever, or address its inputs BY POSITION, which is the fragility a named port set removes.

use nodes_core::register_builtins;
use nodes_core::{BoxValue, Graph, NodeTypeRegistry, ValueTypes};

fn fixtures() -> (NodeTypeRegistry, ValueTypes) {
    let mut types = ValueTypes::new();
    let mut reg = NodeTypeRegistry::new();
    register_builtins(&mut reg, &mut types);
    (reg, types)
}

/// A port added to one instance appears on THAT instance, in order, and nowhere else.
#[test]
fn an_added_port_is_per_instance_and_lands_last() {
    let (reg, types) = fixtures();
    let mut g = Graph::new();
    let a = g
        .create_node("Add", [0.0, 0.0], &reg, &types)
        .expect("create a");
    let b = g
        .create_node("Add", [0.0, 0.0], &reg, &types)
        .expect("create b");

    let before = g.nodes[&a].input_names();

    assert!(
        g.nodes
            .get_mut(&a)
            .expect("a")
            .add_input("in.3", BoxValue::Null),
        "a fresh name must be accepted"
    );

    let after = g.nodes[&a].input_names();
    assert_eq!(
        after.len(),
        before.len() + 1,
        "exactly one port was added: {after:?}"
    );
    assert_eq!(
        after.last().map(String::as_str),
        Some("in.3"),
        "a new port lands at the END of the declared order, never in the middle"
    );
    assert_eq!(
        &after[..before.len()],
        &before[..],
        "the ports the type declared keep their order and their names"
    );

    assert_eq!(
        g.nodes[&b].input_names(),
        before,
        "PER-INSTANCE: the sibling of the same type must be untouched. If this fails the port \
         went onto the shared NodeDef and every instance grew a pin."
    );
}

/// Adding the same name twice is refused, and refused WITHOUT damage.
///
/// The dangerous version of this is a silent overwrite: the port's attr would be replaced, taking
/// the connected wire's `source` with it, while the wire is still drawn. Ports are a SET.
#[test]
fn a_duplicate_port_is_refused_and_changes_nothing() {
    let (reg, types) = fixtures();
    let mut g = Graph::new();
    let n = g
        .create_node("Add", [0.0, 0.0], &reg, &types)
        .expect("create");

    let node = g.nodes.get_mut(&n).expect("node");
    assert!(node.add_input("extra", BoxValue::Null));
    let after_first = node.input_names();

    assert!(
        !node.add_input("extra", BoxValue::Null),
        "a duplicate must be REFUSED, not silently overwritten"
    );
    assert_eq!(
        node.input_names(),
        after_first,
        "a refused add must leave the order untouched - no duplicate entry, no reordering"
    );
}
