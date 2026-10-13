//! Convenient imports for authoring nodes — the builtins in `builtins/*` AND an out-of-crate node
//! library.
//!
//! What a DATAFLOW node pack must be able to name is here: the traits it implements ([`NodeType`],
//! [`NodeLibrary`]), the signature pieces its `cook` must spell ([`CookCtx`], [`CookResult`],
//! [`CookError`]), the shape it declares ([`NodeDef`], [`PortDef`], [`AttrDef`], [`UiHint`],
//! [`CookStage`]), the custom port types it brings ([`ValueTypeInfo`], [`ValueTypes`],
//! [`PinVisual`]), and the [`NodeTypeRegistry`] a host adds it to. Plus the [`crate::node!`]
//! macro's helpers: `float`, `int`, [`set_float_output`], and the `BoxValue` constructors
//! `fv`/`iv`/`bv`/`sv`/`vec3v`/`cv`.
//!
//! A pack that ships a **JOB** ([`crate::NodeKind::Job`] — an imperative, side-effecting node) also
//! needs [`crate::fn_job`] / [`crate::NodeKind`], which are deliberately NOT re-exported here: the
//! prelude glob is the DATAFLOW path (the overwhelmingly common one), and declaring a job should be
//! an explicit `use nodes_core::{fn_job, NodeKind};` rather than something a wildcard hands you by
//! accident. See `docs/custom-nodes.md` §1b.

pub use std::sync::Arc;

pub use crate::box_model::BoxValue;
pub use crate::bus::DataBus;
pub use crate::cook::{
    CancelToken, CookCtx, CookError, CookResult, CookStage, passthrough_cook, set_float_output,
};
pub use crate::graph::NodeId;
// `Node` (the instance a cook mutates) and `PinShape` (naming a custom type's pin) were missing:
// the worked example had to reach past the prelude for both. `NodeId`/`DataBus`/`CancelToken` are
// what `CookCtx::for_test` needs, so a node author can unit-test a cook without hunting imports.
pub use crate::node::{AttrDef, Node, NodeDef, UiHint};
pub use crate::node_type::{NodeLibrary, NodeType, fn_node, fn_node_staged};
pub use crate::port::{PinShape, PinVisual, PortDef};
pub use crate::registry::NodeTypeRegistry;
pub use crate::value_types::{ANY_TYPE, ValueTypeInfo, ValueTypes};

/// `BoxValue` constructors (replace the old `DynValue` boxed helpers).
pub fn fv(v: f64) -> BoxValue {
    BoxValue::F64(v)
}
pub fn iv(v: i64) -> BoxValue {
    BoxValue::I64(v)
}
pub fn bv(v: bool) -> BoxValue {
    BoxValue::Bool(v)
}
pub fn sv(v: impl Into<String>) -> BoxValue {
    BoxValue::String(v.into())
}
pub fn vec3v(v: [f64; 3]) -> BoxValue {
    BoxValue::Vec3(v)
}
pub fn cv(v: [f64; 4]) -> BoxValue {
    BoxValue::Color(v)
}
pub fn nullv() -> BoxValue {
    BoxValue::Null
}

// Port-def helpers.
pub fn float(name: &str, default: f64) -> PortDef {
    PortDef::float(name, default)
}
pub fn int(name: &str, default: i64) -> PortDef {
    PortDef::int(name, default)
}
pub fn bool(name: &str, default: bool) -> PortDef {
    PortDef::bool(name, default)
}
pub fn string(name: &str) -> PortDef {
    PortDef::string(name)
}
pub fn vec3(name: &str) -> PortDef {
    PortDef::vec3(name)
}
pub fn color(name: &str) -> PortDef {
    PortDef::color(name)
}
pub fn any(name: &str) -> PortDef {
    PortDef::any(name)
}

/// Map port — type `"Map"`, default `BoxValue::object()` (empty BTreeMap bag).
pub fn map(name: &str) -> PortDef {
    PortDef::typed(name, "Map")
}
