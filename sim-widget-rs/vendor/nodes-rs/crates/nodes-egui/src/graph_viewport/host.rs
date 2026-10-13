//! Host traits for the shared graph viewport.
//!
//! Implement [`GraphViewportHost`] in the application crate (`nodes-editor`, diagram viewer)
//! to supply subnet data and optional undo/cook/inline-widget hooks.

use std::collections::{HashMap, HashSet};

use nodes_core::{LayoutCache, NodeId, NodeTypeRegistry, Subnet, ValueTypes};
use nodes_view::Camera;

/// Discrete edit classification for history routing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportEditKind {
    NodeDrag,
    ResizeWidth,
    ResizeBox,
    Delete,
    WireConnect,
    WireDisconnect,
    AutoLayout,
}

/// Lifecycle phase for a viewport edit gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportEditPhase {
    Begin,
    End,
    Cancel,
}

/// Subnet + registries required to render and interact with the graph.
///
/// The viewport calls `subnet` / `subnet_mut` every frame for layout and drawing.
pub trait GraphViewportHost {
    fn subnet(&self) -> &Subnet;
    fn subnet_mut(&mut self) -> &mut Subnet;
    fn registry(&self) -> &NodeTypeRegistry;
    fn value_types(&self) -> &ValueTypes;

    /// Current edit scope: `None` = root level; `Some(id)` = inside compound node `id`.
    ///
    /// The viewport passes this to `compute_static_layout_scoped` so only the nodes
    /// parented to the active scope are rendered and interactable.
    fn scope(&self) -> Option<NodeId> {
        None
    }

    /// Auto-layout shortcut (L). Default: core subnet rank layout.
    fn auto_layout(&mut self) {
        nodes_core::layout::auto_layout_subnet(self.subnet_mut());
    }

    /// Called before [`Self::auto_layout`] when the viewport handles **L**.
    fn prepare_auto_layout(&mut self) {}

    /// Called at end of viewport frame when [`super::GraphViewportConfig::sync_after_frame`].
    fn on_frame_end(&mut self, _viewport: &super::GraphViewportState, _editable: bool) {}

    /// Optional inline port widgets (editor). Default: no-op.
    fn draw_inline_widgets(
        &mut self,
        ui: &mut egui::Ui,
        viewport: egui::Rect,
        layout: &LayoutCache,
        camera: &Camera,
    ) {
        let _ = (ui, viewport, layout, camera);
    }

    /// History hooks (diagram hosts override; editor default no-op).
    fn on_viewport_edit(
        &mut self,
        _kind: ViewportEditKind,
        _phase: ViewportEditPhase,
        _camera: &Camera,
        _selection: &HashSet<NodeId>,
    ) {
    }

    fn undo_network(&mut self, _viewport: &mut super::GraphViewportState) -> bool {
        false
    }

    fn redo_network(&mut self, _viewport: &mut super::GraphViewportState) -> bool {
        false
    }

    fn undo_ui(&mut self, _viewport: &mut super::GraphViewportState) -> bool {
        false
    }

    fn redo_ui(&mut self, _viewport: &mut super::GraphViewportState) -> bool {
        false
    }
}

/// A ready-made [`GraphViewportHost`] over a plain `Subnet` plus its registries.
///
/// Embedders that just want "a graph panel over a `Subnet`" can use this instead of
/// hand-rolling the trait (which every host — `nodes-editor`, the diagram viewer,
/// `egui-nodes-view` — otherwise re-implements). Construct with:
/// - [`SubnetHost::with_builtins`] — the builtin node types only;
/// - [`SubnetHost::with_libraries`] — builtins **plus your own** [`nodes_core::NodeLibrary`]s (the
///   easy injection path: your custom node types + their custom value types, in one call);
/// - [`SubnetHost::new`] — bring your own fully-built registries.
///
/// The undo / cook / inline-widget hooks keep the trait defaults (no-ops); a richer host still
/// implements [`GraphViewportHost`] directly.
pub struct SubnetHost {
    /// The graph shown and edited by the viewport.
    pub subnet: Subnet,
    /// Node type definitions (drives spawn menu, port names, cook).
    pub registry: NodeTypeRegistry,
    /// Value-type table (drives pin colours / compatibility).
    pub value_types: ValueTypes,
}

impl SubnetHost {
    /// Wrap an existing subnet with caller-provided registries.
    pub fn new(subnet: Subnet, registry: NodeTypeRegistry, value_types: ValueTypes) -> Self {
        Self {
            subnet,
            registry,
            value_types,
        }
    }

    /// Empty subnet with all builtins registered (see [`nodes_core::default_registry`]).
    pub fn with_builtins() -> Self {
        Self::with_libraries(&[])
    }

    /// Empty subnet with the builtins **plus** every node type in `libs`.
    ///
    /// The egui-viewport counterpart of `nodes_editor::EditorWidget::with_libraries`: an embedder
    /// that owns a compute crate implements [`nodes_core::NodeLibrary`] over its node types and
    /// passes it here. Each library also contributes its custom VALUE types (pin colours, coercion
    /// rules), so a port typed `"Recon"` renders and connects correctly with no further wiring.
    /// Builtins are added FIRST, so a library can knowingly override one.
    pub fn with_libraries(libs: &[&dyn nodes_core::NodeLibrary]) -> Self {
        let (mut registry, mut value_types) = nodes_core::default_registry();
        for lib in libs {
            registry.add_library(*lib, &mut value_types);
        }
        Self {
            subnet: Subnet::new(),
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

/// Called when the user drops a wire in empty space (editor tab menu).
pub trait WireDropActions {
    fn open_link_search(
        &mut self,
        pos: egui::Pos2,
        from_node: NodeId,
        from_output: usize,
        output_type: String,
    );
}

/// Optional hooks passed into [`super::GraphViewportState::ui`].
#[derive(Default)]
pub struct GraphViewportExtras<'a> {
    pub wire_drop: Option<&'a mut dyn WireDropActions>,
    /// OPTIONAL per-node body-fill override, forwarded to the renderer
    /// ([`nodes_view::RenderParams::node_tint`]). `None` (the [`Default`]) leaves every node
    /// theme-colored, so the editor / demo hosts that use `GraphViewportExtras::default()` are
    /// unaffected. Diagram hosts pass a map to tint nodes read-only (e.g. by live pipeline status).
    pub node_tint: Option<&'a HashMap<NodeId, [f32; 4]>>,
}
