//! Headless node graph core (no egui, no wgpu).
//!
//! ## Role
//!
//! SSOT for graph **data**, **evaluation** (`cook`), and **layout in graph UV**.
//! Renderers (`nodes-view`) and editors only consume [`Graph`], [`LayoutCache`], and
//! registries — they do not duplicate geometry rules.
//!
//! ## Data model
//!
//! Every node is `Node { id, data }` where `data` is a [`RustBox`] holding **all**
//! node data (`type`, `name`, unified `attrs`, ordered `inputs`/`outputs` ref lists,
//! `parent`/`children`, `view`). The graph is a flat `HashMap<Uuid, Node>` plus a
//! transient [`index::AdjacencyIndex`]; connections live in each consumer's
//! `attrs.<input>.source`. See `docs/superpowers/specs/2026-05-28-unified-node-data-design.md`.
//!
//! ## Module map
//!
//! | Module | Responsibility |
//! |--------|----------------|
//! | [`graph_container`] | [`Graph`]: nodes + bus + index |
//! | [`mod@node`] / [`registry`] | Type defs and instances |
//! | [`index`] | Transient adjacency / topo order |
//! | [`mod@cook`] | Topological evaluation (`CookCtx`, staging, status, cancel) |
//! | [`node_type`] | `NodeType` / `NodeLibrary`: the extension point for custom nodes |
//! | [`layout`] + [`chrome`] | Sizes, pin positions, auto-layout |
//! | [`port`] / [`value_types`] | Typed sockets + the type SSOT (visual + coercion) |
//! | [`serialization`] | JSON graphs, paste, duplicate |
//! | [`address`] / [`expr`] | Op paths, `ch("…")` helpers |
//! | [`builtins`] | Stock node types |

pub mod address;
pub mod box_model;
pub mod builtins;
pub mod bus;
pub mod category;
pub mod chrome;
pub mod cook;
pub mod error;
pub mod executor;
pub mod expr;
pub mod force_sim;
pub mod graph;
pub mod graph_container;
pub mod graph_ctx;
pub mod graph_rect;
pub mod index;
pub mod layout;
pub mod name;
pub mod node;
pub mod node_type;
pub mod port;
pub mod prelude;
pub mod registry;
pub mod schema;
pub mod script;
pub mod serialization;
pub mod subnet;
pub mod value_types;
/// Builtin prototype-value seed table — crate-private; the type default authority is
/// [`ValueTypes::make_default`] (see `values.rs`).
mod values;
pub mod view_mode;
pub mod worker;

/// Build a [`NodeType`] (as `Arc<dyn NodeType>`) from a name, category, ports and a cook.
///
/// This is the terse form every schema-less builtin uses; the result goes into a
/// [`NodeLibrary::types`] list. The cook body is an EXPRESSION evaluating to
/// `CookResult<()>` — a failure is REPORTED to the driver (which records
/// [`cook::NodeStatus::Error`] on the node), never swallowed.
///
/// ```ignore
/// let add = node!("Add", "Math",
///     [float("a", 0.0), float("b", 0.0)] => [float("result", 0.0)],
///     |ctx| {
///         let a = ctx.node.in_f64("a");
///         let b = ctx.node.in_f64("b");
///         ctx.node.set_out("result", nodes_core::BoxValue::F64(a + b));
///         Ok(())
///     }
/// );
/// ```
///
/// The second form takes any ready cook function/closure (e.g. [`cook::passthrough_cook`]).
/// Both produce a [`cook::CookStage::Main`] type with no inspector schema and no custom value
/// types — a type needing any of those builds its [`node::NodeDef`] explicitly and calls
/// [`fn_node`] / [`fn_node_staged`] (or implements [`NodeType`] directly).
#[macro_export]
macro_rules! node {
    ($name:literal, $cat:literal,
     [$($input:expr),* $(,)?] => [$($output:expr),* $(,)?],
     |$ctx:ident| $body:expr) => {
        $crate::node_type::fn_node(
            $crate::node::NodeDef::new($name, $cat)
                .with_inputs(vec![$($input),*])
                .with_outputs(vec![$($output),*]),
            |$ctx: &mut $crate::cook::CookCtx| -> $crate::cook::CookResult<()> { $body },
        )
    };

    ($name:literal, $cat:literal,
     [$($input:expr),* $(,)?] => [$($output:expr),* $(,)?],
     $cook_fn:expr) => {
        $crate::node_type::fn_node(
            $crate::node::NodeDef::new($name, $cat)
                .with_inputs(vec![$($input),*])
                .with_outputs(vec![$($output),*]),
            $cook_fn,
        )
    };
}

pub use address::{
    AddressTail, EditFocus, OpAddress, OpPath, PortRef, ResolvedLocation, ResolvedTarget,
    VIRTUAL_CONTEXT_MAT, VIRTUAL_CONTEXT_OBJ, VIRTUAL_CONTEXT_OUT, op_path_of, parse_address,
    parse_op_path, resolve_address, resolve_op, resolve_relative,
};
pub use box_model::RustBox;
pub use box_model::{
    ArrayDiffPolicy, ArrayMergePolicy, ArraySubtractPolicy, BoxConfig, BoxError, BoxList, BoxMap,
    BoxMut, BoxRef, BoxValue, Change, ChangeKind, DType, DiffOptions, DiffReport, MergeOptions,
    PathKey, Segment, SubtractOptions, ValueKind,
};
pub use builtins::{Builtins, BuiltinsSubset, create_subnet, register_builtins};
pub use bus::DataBus;
pub use category::{category_accent_rgba, category_header_bg_rgba};
pub use cook::{
    CancelToken, CookCtx, CookError, CookOpts, CookResult, CookStage, GraphView, NodeStatus,
    NoopProgress, ProgressSink, ServiceMap, StatusObserver, cook, cook_dirty, cook_dirty_with,
    cook_with, execute, execute_all, execute_all_with, execute_with, is_stale, jobs_in_order,
    mark_dirty, noop_progress, passthrough_cook, resolved_input_value, set_float_output,
};
pub use error::{CoreError, CoreResult};
pub use executor::Executor;
pub use expr::{extract_ch_path, parse_ch_address, resolve_ch, resolve_ch_relative};
pub use force_sim::{ForceSim, ForceSimConfig, SeparationMode};
pub use graph::{NodeId, SubnetId};
pub use graph_container::Graph;
pub use graph_ctx::{GraphEdit, GraphRead};
pub use graph_rect::{
    GraphRect, NodeRegions, TITLE_FLAG_GAP, TITLE_FLAG_PAD, TITLE_FLAG_SIZE, TITLE_LABEL_GAP,
    VIEW_MODE_TOGGLE_W, graph_rect_node_local, node_regions,
};
pub use index::{AdjacencyIndex, InEdge};
pub use layout::{
    DEFAULT_FRAME_H, DEFAULT_FRAME_W, DEFAULT_NODE_W, DEFAULT_NOTE_H, DEFAULT_NOTE_W,
    FRAME_NODE_TYPE, GRID_STEP, LayoutCache, LayoutNode, MAX_NODE_H, MAX_NODE_W, MIN_NODE_H,
    MIN_NODE_W, NODE_ASPECT, NOTE_NODE_TYPE, NodeXform, PAD, PIN_LABEL_GAP, PORT_INLINE_H,
    PORT_INLINE_W, PORT_LABEL_W, PinEdgeLayout, PinPos, PinSide, PortRowParams, PortRowRects,
    TITLE_H, apply_resized_view_size, auto_layout_subnet, build_layout_graph, compute_node_size,
    compute_static_layout, compute_static_layout_scoped, cycle_view_mode_and_resize, is_backdrop,
    is_frame_node, is_note_node, layout_uv_bounds, layout_uv_bounds_selected, node_height,
    node_height_for_width, node_height_rows, pin_row_count, port_row_geometry,
    preserve_node_center_after_resize, primary_image_dims, resolve_orientation,
    set_view_mode_and_resize, snap_to_grid,
};
pub use name::{
    PATH_HOSTILE, infer_node_name, normalize_subnet_names, rename_node, sanitize_name_base,
    suggest_node_name, uniquify_name, validate_node_name, validate_path_name,
};
pub use node::{AttrDef, Node, NodeDef, UiHint};
pub use node_type::{FnNode, NodeKind, NodeLibrary, NodeType, fn_job, fn_node, fn_node_staged};
pub use port::{PinShape, PinVisual, PortDef, pin_visual_for_type, wire_color_for_types};
pub use registry::{NodeTypeRegistry, RegEntry, validate_def, validate_library};
pub use schema::{NodeSchema, PortSchema};
pub use script::{ScriptError, from_rhai, to_rhai};
pub use serialization::{
    PasteResult, SUBNET_FORMAT_VERSION, SerializedNode, SerializedViewMode, SubnetFile,
    SubnetMergeOptions, duplicate_selection, extract_fragment, fragment_min_position,
    load_subnet_from_file, load_subnet_json, merge_subnet_file, remap_node_ids, save_subnet_json,
    subnet_from_json, subnet_to_file, subnet_to_json_pretty,
};
pub use subnet::Subnet;
pub use value_types::{
    ANY_TYPE, CoerceFn, ValueTypeInfo, ValueTypes, builtin_value_types, register_builtin_types,
};
pub use worker::{NodeWorker, WorkerPolicy, WorkerSet};
// NOTE: there is deliberately no public `default_value(ty)` free function anymore — a type's
// default lives on the registered type. Use `ValueTypes::make_default(ty)` / `ValueTypes::port_def`.
pub use view_mode::{
    NodeViewKind, NodeViewMode, displayed_pin_indices, drawn_pin_indices, pin_is_connected,
    visible_pin_indices,
};

/// Build a fully-populated `(NodeTypeRegistry, ValueTypes)` with the [`Builtins`] library added.
///
/// The one-call convenience every embedder otherwise copies: a fresh registry + value types with
/// `add_library(&Builtins, &mut value_types)`, which also registers the builtin value types (the
/// library declares them). Returned as `(registry, value_types)` to match the argument order most
/// APIs in this crate take (registry first, value-types second).
///
/// A host that wants MORE than the builtins keeps going from here:
/// `registry.add_library(&MyNodes, &mut value_types)`.
///
/// # Example
/// ```
/// let (registry, value_types) = nodes_core::default_registry();
/// // Builtins are registered: a builtin value type resolves to a pin visual.
/// let _ = nodes_core::pin_visual_for_type("Float", &value_types);
/// assert!(registry.get("Add").is_some());
/// ```
pub fn default_registry() -> (NodeTypeRegistry, ValueTypes) {
    let mut value_types = ValueTypes::new();
    let mut registry = NodeTypeRegistry::new();
    register_builtins(&mut registry, &mut value_types);
    (registry, value_types)
}
