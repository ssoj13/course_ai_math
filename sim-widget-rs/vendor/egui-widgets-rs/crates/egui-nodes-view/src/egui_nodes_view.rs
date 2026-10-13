//! `egui-nodes-view` — a thin egui-widgets-rs surface over nodes-rs's GPU node graph.
//!
//! `nodes-egui` already ships the full egui widget: [`GraphViewportState::ui`]
//! does camera (MMB pan / RMB zoom / Alt orbit), marquee + click selection, node
//! drag / resize, wiring, delete, and the wgpu render → `egui::Image` present.
//! This crate re-exports that widget under the workspace's `egui-*` naming and
//! adds a ready-made [`SubnetHost`] so a caller can show a GPU node panel over a
//! plain [`nodes_core::Subnet`] without hand-writing the [`GraphViewportHost`]
//! trait (undo / redo / scope / history use the trait defaults).
//!
//! This is the ONLY node graph in cglibs. The egui-snarl-based `egui-node-graph`
//! that used to sit beside it as a CPU / immediate-mode alternative is deleted:
//! one node-graph engine, and it is ours. A wgpu device is therefore required.
//!
//! ## Usage
//! ```ignore
//! use egui_nodes_view::{SubnetHost, GraphViewportState, GraphViewportConfig, GraphViewportExtras};
//!
//! // once, when the wgpu RenderState becomes available (e.g. eframe CreationContext):
//! state.configure_wgpu_render_state(render_state.clone());
//!
//! // per frame, inside a panel:
//! state.ui(ui, &mut host, &GraphViewportConfig::EDITOR, GraphViewportExtras::default());
//! ```

/// Re-export the headless graph model crate so a host can build a [`nodes_core::Subnet`]
/// without adding a second dependency of its own.
pub use nodes_core;
/// Re-export the whole egui bridge (widget + config + host trait + edit enums)
/// so a consumer needs only this one crate, not `nodes-egui` directly.
pub use nodes_egui::{
    GraphGpuBridge, GraphViewportConfig, GraphViewportExtras, GraphViewportHost,
    GraphViewportState, ViewportEditKind, ViewportEditPhase, WireDropActions,
};

use nodes_core::{NodeTypeRegistry, Subnet, ValueTypes};

/// A ready-made [`GraphViewportHost`] over an owned [`Subnet`] and its registries.
///
/// Covers the common case — *"show a GPU graph panel over a plain nodes-core
/// subnet"* — so a caller does not implement the four required
/// [`GraphViewportHost`] methods by hand. Undo / redo, edit scope and history
/// hooks use the trait defaults (no-op / root scope); a host that needs those
/// implements [`GraphViewportHost`] on its own type instead of using this.
pub struct SubnetHost {
    /// The graph being displayed / edited.
    pub subnet: Subnet,
    /// Node-type definitions (ports + types) the viewport reads for layout + wiring.
    pub registry: NodeTypeRegistry,
    /// Value-type table used for pin colours and connection compatibility.
    pub value_types: ValueTypes,
}

impl SubnetHost {
    /// Wrap an existing subnet + registries as a viewport host.
    pub fn new(subnet: Subnet, registry: NodeTypeRegistry, value_types: ValueTypes) -> Self {
        Self {
            subnet,
            registry,
            value_types,
        }
    }
}

impl GraphViewportHost for SubnetHost {
    fn subnet(&self) -> &Subnet {
        &self.subnet
    }
    fn subnet_mut(&mut self) -> &mut Subnet {
        &mut self.subnet
    }
    fn registry(&self) -> &NodeTypeRegistry {
        &self.registry
    }
    fn value_types(&self) -> &ValueTypes {
        &self.value_types
    }
}
