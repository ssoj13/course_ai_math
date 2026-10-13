//! Capability flags for [`super::GraphViewportState::ui`] (editor vs diagram host).

/// What the shared graph viewport may do in a given host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GraphViewportConfig {
    pub allow_wiring: bool,
    pub allow_delete: bool,
    pub allow_auto_layout: bool,
    pub allow_view_mode_toggle: bool,
    pub allow_resize: bool,
    pub allow_node_drag: bool,
    pub allow_inline_widgets: bool,
    /// Reserved: cook is driven by hosts via [`GraphViewportHost::on_frame_end`] (e.g. `DiagramSession::commit_frame(.., cook)`) instead of reading this flag in the viewport.
    pub cook_after_render: bool,
    pub show_orbit_hud: bool,
    /// Call [`super::host::GraphViewportHost::on_frame_end`] after interaction.
    pub sync_after_frame: bool,
    /// Full subnet→graph sync in `on_frame_end` (topology + geometry).
    pub diagram_editable: bool,
    /// Paint each node's title (its box `display_name`, falling back to the type name) as an
    /// egui overlay in the title-bar region. The shared GPU renderer draws port names but NEVER
    /// node titles — in the full winit editor those come from the `wgpu-widgets` title-bar layer
    /// (`nodes-editor::NodeWidgetLayer`), which this lightweight egui viewport does not have. So a
    /// pure egui embed (colmap-ui Pipeline tab, mermaid diagrams) would otherwise show titleless
    /// cards. This is the SAME egui-painter compensation the sticky-note text overlay already uses.
    /// Default `false` (editor hosts own titles via their widget layer); `true` for diagram hosts.
    pub draw_node_titles: bool,
}

impl GraphViewportConfig {
    /// Full node editor (wiring, resize, inline widgets, cook).
    pub const EDITOR: Self = Self {
        allow_wiring: true,
        allow_delete: true,
        allow_auto_layout: true,
        allow_view_mode_toggle: true,
        allow_resize: true,
        allow_node_drag: true,
        allow_inline_widgets: false,
        cook_after_render: true,
        show_orbit_hud: true,
        sync_after_frame: false,
        diagram_editable: false,
        // Editor hosts draw titles via their own `wgpu-widgets` title-bar layer.
        draw_node_titles: false,
    };

    /// Read-only diagram host: pan/zoom/select/drag; geometry sync only.
    pub const DIAGRAM: Self = Self {
        allow_wiring: false,
        allow_delete: false,
        allow_auto_layout: false,
        allow_view_mode_toggle: false,
        allow_resize: false,
        allow_node_drag: true,
        allow_inline_widgets: false,
        cook_after_render: false,
        show_orbit_hud: false,
        sync_after_frame: true,
        diagram_editable: false,
        // No widget layer in an egui embed → paint titles here so cards aren't blank.
        draw_node_titles: true,
    };

    /// Editable diagram: drag, delete, wire, auto-layout, cook, full graph sync + export.
    pub const DIAGRAM_EDIT: Self = Self {
        allow_wiring: true,
        allow_delete: true,
        allow_auto_layout: true,
        allow_view_mode_toggle: false,
        allow_resize: false,
        allow_node_drag: true,
        allow_inline_widgets: false,
        cook_after_render: true,
        show_orbit_hud: false,
        sync_after_frame: true,
        diagram_editable: true,
        // Editable diagram is still an egui embed with no widget layer → paint titles here.
        draw_node_titles: true,
    };
}
