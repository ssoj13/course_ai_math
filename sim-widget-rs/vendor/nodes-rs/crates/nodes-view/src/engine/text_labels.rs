//! Graph labels on the working plane (port names, collapsed-node summaries, and — for hosts
//! without a widget title bar — node titles), scaled with camera zoom.
//!
//! # Who draws node titles
//! The full winit editor draws titles through the `wgpu-widgets` title bar
//! (`nodes-editor::node_widgets`), which also carries the view-mode toggle and the
//! bypass/display flags. Lightweight hosts (the egui [`GraphViewportState`] embed used for
//! read-only DIAGRAMs) have no widget layer, so they ask for `draw_titles` and get the title
//! as a plain plane label from here.
//!
//! Titles MUST live in this pass rather than as a 2D overlay painted by the host. The viewport
//! is a real 3D scene (perspective fly camera, orbit, per-node rotation), so a node projects to
//! an arbitrary quadrilateral, not an axis-aligned screen rect. Text painted into a
//! screen-space rect derived from two projected corners is wrong the moment the camera leaves
//! the flat top-down lens: it skews, mis-scales (one `px_per_world` cannot serve nodes at
//! different depths), ignores occlusion, and flies apart entirely for nodes behind the eye
//! (no `w` clip). A [`PlaneLabel`] instead rides the SAME `surface_xfm` as the node body, so it
//! is correct in every camera pose by construction.

use glam::{Mat4, Vec2, Vec3};
use nodes_core::{
    LayoutCache, NodeTypeRegistry, NodeViewMode, NodeXform, PinSide, Subnet,
    chrome::{MINIMAL_SUMMARY_H, PAD, PortRowParams, TITLE_H, port_row_geometry},
    drawn_pin_indices, is_frame_node, is_note_node, node_regions, visible_pin_indices,
};
use wgpu_text::{PlaneLabel, quantize_plane_raster_scale};

use crate::VisibleNodes;
use crate::camera::flat::Camera;

const PORT_FONT: f32 = 11.0;
const PORT_LINE_H: f32 = 14.0;
/// Title font/line-height/color — the SAME values the editor's widget title bar uses
/// (`node_widgets::TITLE_FONT` / `TITLE_LINE_H` / its label style), so a graph looks identical
/// whether its titles come from the widget bar or from this pass.
const TITLE_FONT: f32 = 13.0;
const TITLE_LINE_H: f32 = 16.0;
const TITLE_COLOR: [f32; 4] = [0.92, 0.94, 0.98, 1.0];
/// Nudge plane labels toward the camera so coplanar text wins the depth test against
/// the node bodies the shape pass writes at the same plane `z` (see `node_widgets`'
/// `WIDGET_DEPTH_PUSH` for the matching widget-overlay convention).
const LABEL_DEPTH_PUSH: f32 = 0.25;

/// Port name labels in graph UV units with dynamic `raster_scale`.
#[allow(clippy::too_many_arguments)]
pub fn build_graph_plane_labels(
    layout: &LayoutCache,
    subnet: &Subnet,
    registry: &NodeTypeRegistry,
    camera: &Camera,
    viewport: Vec2,
    visible: Option<&VisibleNodes>,
    theme: &ui_theme::Theme,
    draw_titles: bool,
) -> Vec<PlaneLabel> {
    let mut out = Vec::new();
    // Rasterize glyphs at the on-plane pixels-per-world scale (perspective: evaluated at the
    // graph plane) so text stays crisp at the current dolly without re-rastering per glyph.
    let ppg = camera.px_per_world_at_target(viewport);
    let raster_scale = quantize_plane_raster_scale(ppg);
    // Lift labels off the working plane toward the camera. The plane-text pipeline draws with
    // `depth_compare: Always` + no depth write, so this is not needed to pass a depth test;
    // it is a small belt-and-suspenders nudge (toward the eye, so negate `forward()` which
    // points eye→plane) that keeps glyphs from ever coplanar-fighting the node bodies. Under
    // perspective the nudge is essentially parallax-free at the graph's dolly distance.
    let depth_offset = -camera.forward() * LABEL_DEPTH_PUSH;

    for (id, layout_node) in &layout.nodes {
        let Some(node) = subnet.nodes.get(id) else {
            continue;
        };
        if is_frame_node(node) {
            continue;
        }
        // The SAME shared visible-set the shapes/wires use; additionally skip labels for any LOD
        // tier below `Full` — text is the priciest per-node work (glyph shaping/upload) and is
        // unreadable once the node shrinks past `lod_no_text_px`.
        if let Some(vis) = visible
            && (!vis.is_visible(*id) || !vis.lod(*id).draws_text())
        {
            continue;
        }

        let node_def = registry.get(node.type_name());
        let node_left = layout_node.center[0] - layout_node.size[0] * 0.5;
        let view_mode = node.view_mode();

        // Node title — only for hosts with no widget title bar (see the module doc). Notes carry
        // their own body text and frames are bare backdrops: neither has a title strip.
        if draw_titles && !is_note_node(node) {
            let regions = node_regions(layout_node.center, layout_node.size, view_mode, false);
            // The node's instance name, falling back to its type when unset (a diagram loaded from
            // a schema-only registry never names instances). Mirrors the widget bar, which labels
            // the strip with `node.display_name()`.
            let dn = node.display_name();
            let title = if dn.is_empty() { node.type_name() } else { dn };
            if !title.is_empty() {
                // The band BETWEEN the chrome widgets (view-mode toggle on the left, bypass/display
                // flags on the right) — the same SSOT split the widget bar lays out against, so a
                // long title is clipped exactly where the editor clips it instead of running under
                // the flags.
                let [u_min, u_max] = regions.title_label_u_range();
                let anchor =
                    Vec2::new(u_min, regions.title.min[1] + (TITLE_H - TITLE_LINE_H) * 0.5);
                push_label(
                    &mut out,
                    camera,
                    depth_offset,
                    anchor,
                    u_max - u_min,
                    title,
                    TITLE_FONT,
                    TITLE_LINE_H,
                    TITLE_COLOR,
                    raster_scale,
                    layout_node.xform(),
                );
            }
        }

        // Node titles are drawn by the widget title bar (node_widgets); not duplicated here.
        if matches!(view_mode, NodeViewMode::Minimal | NodeViewMode::Mid)
            && visible_pin_indices(node, PinSide::Input).is_empty()
            && visible_pin_indices(node, PinSide::Output).is_empty()
        {
            let regions = node_regions(layout_node.center, layout_node.size, view_mode, false);
            // `Arc<str>` deref, not a `to_string()`: this runs per visible node, per frame.
            let summary = node_def
                .map(|d| &*d.display_name)
                .unwrap_or(node.type_name());
            let body = regions.body;
            let anchor = Vec2::new(
                body.min[0] + PAD,
                body.min[1] + (body.max[1] - body.min[1] - MINIMAL_SUMMARY_H) * 0.5,
            );
            let label_w = (body.max[0] - body.min[0] - PAD * 2.0).max(24.0);
            push_port_label(
                &mut out,
                camera,
                depth_offset,
                anchor,
                label_w,
                summary,
                theme.text,
                raster_scale,
                layout_node.xform(),
            );
            continue;
        }

        let node_top = layout_node.center[1] - layout_node.size[1] * 0.5;
        let orientation = node.resolved_orientation(layout_node.size[0], layout_node.size[1]);
        let row_params = PortRowParams::default();

        // Which sockets/labels to draw — the SSOT [`drawn_pin_indices`] already applies the
        // Minimal-mode collapse (first socket only), so the visible socket and its label stay in
        // lockstep with `shape_scene` at every view mode.
        let input_labels = drawn_pin_indices(node, PinSide::Input);
        let output_labels = drawn_pin_indices(node, PinSide::Output);

        for i in input_labels {
            let Some(pin) = layout.pin_positions.get(&(*id, PinSide::Input, i)) else {
                continue;
            };
            let name = node_def
                .map(|d| d.inputs.get(i).map(|p| p.name.as_str()).unwrap_or("in"))
                .unwrap_or("in");
            // `needs_widget` doesn't affect the returned label rect (it only decides
            // whether a widget slot trails the label in the same flex pass) — passed
            // `true` unconditionally since this call only reads `.label`.
            let rects = port_row_geometry(
                nodes_core::chrome::Rect::new(
                    node_left,
                    node_top,
                    layout_node.size[0],
                    layout_node.size[1],
                ),
                PinSide::Input,
                pin.world,
                orientation,
                &row_params,
                true,
            );
            let anchor = Vec2::new(
                rects.label.x,
                rects.label.y + (rects.label.h - PORT_LINE_H).max(0.0) * 0.5,
            );
            push_port_label(
                &mut out,
                camera,
                depth_offset,
                anchor,
                rects.label.w,
                name,
                theme.text,
                raster_scale,
                layout_node.xform(),
            );
        }

        for i in output_labels {
            let Some(pin) = layout.pin_positions.get(&(*id, PinSide::Output, i)) else {
                continue;
            };
            let name = node_def
                .map(|d| d.outputs.get(i).map(|p| p.name.as_str()).unwrap_or("out"))
                .unwrap_or("out");
            let rects = port_row_geometry(
                nodes_core::chrome::Rect::new(
                    node_left,
                    node_top,
                    layout_node.size[0],
                    layout_node.size[1],
                ),
                PinSide::Output,
                pin.world,
                orientation,
                &row_params,
                false,
            );
            let anchor = Vec2::new(
                rects.label.x,
                rects.label.y + (rects.label.h - PORT_LINE_H).max(0.0) * 0.5,
            );
            push_port_label(
                &mut out,
                camera,
                depth_offset,
                anchor,
                rects.label.w,
                name,
                theme.text,
                raster_scale,
                layout_node.xform(),
            );
        }

        // Full mode adds a processing-info line below the port rows (cumulative
        // view-mode design — see `NodeViewMode` doc): dirty/cooked status, using the
        // node's existing dirty flag rather than new tracking. Anchored against the
        // node's ACTUAL height (`layout_node.size[1]`, which already applies the
        // `full_view_height` floor in `compute_node_size`) so it always hugs the real
        // bottom edge — recomputing height from `pin_row_count` here independently
        // (the old approach) drifted from the true body height whenever a low-row
        // node's floor won over its content height (e.g. a 1-port Subnet).
        if view_mode == NodeViewMode::Full {
            let anchor = Vec2::new(
                node_left + PAD,
                node_top + layout_node.size[1] - PAD - PORT_LINE_H,
            );
            let status = if node.dirty() { "dirty" } else { "cooked" };
            push_port_label(
                &mut out,
                camera,
                depth_offset,
                anchor,
                80.0,
                status,
                theme.text_disabled,
                raster_scale,
                layout_node.xform(),
            );
        }
    }

    out
}

/// Build a label's plane surface transform, ROTATED with the node via its [`NodeXform`] — the SAME
/// single transform the SDF body/pins and inline widgets use, so a label can never desync from its
/// (rotated) card. `anchor` is authored in the node's UNROTATED local frame; a non-identity xform
/// post-rotates the whole label surface about the node center in the plane (a `+rot` graph-UV
/// rotation = a `+rot` world rotation about `axis_u × axis_v`, matching `shape.wgsl` and
/// `node_widgets::surface_xfm`). Identity xform is the common, no-op path.
fn label_surface_xfm(camera: &Camera, anchor: Vec2, depth_offset: Vec3, xform: NodeXform) -> Mat4 {
    let base = camera
        .plane
        .surface_xfm_from_graph_uv_offset(anchor, depth_offset);
    let pivot = camera
        .plane
        .graph_to_world(xform.center[0], xform.center[1]);
    let n = camera
        .plane
        .axis_u
        .cross(camera.plane.axis_v)
        .normalize_or_zero();
    crate::rotate_surface_about(base, pivot, n, xform.rotation)
}

/// Push one plane label anchored at graph `anchor` (node-local), lifted off the plane by
/// `depth_offset` and rotated with the node via `xform`.
#[allow(clippy::too_many_arguments)]
fn push_label(
    out: &mut Vec<PlaneLabel>,
    camera: &Camera,
    depth_offset: Vec3,
    anchor: Vec2,
    width: f32,
    text: &str,
    font_size: f32,
    line_height: f32,
    color: [f32; 4],
    raster_scale: f32,
    xform: NodeXform,
) {
    out.push(PlaneLabel {
        surface_xfm: label_surface_xfm(camera, anchor, depth_offset, xform),
        local_rect: [0.0, 0.0, width.max(8.0), line_height],
        // EVERY label here is one line - the box is exactly `line_height` tall. Wrapping it put the
        // second line outside its own box, and a narrow title strip broke mid-word: a node called
        // `Comp` rendered as "Com" over "p". The width still matters, as the clip.
        wrap: false,
        text: text.to_string(),
        font_size,
        line_height,
        color,
        raster_scale,
        // CUT AT THE BOX, now that the text no longer wraps to fit it. The width is the band
        // between the chrome widgets, computed so a long title stops where the editor's own title
        // bar stops it - previously that width only caused wrapping, and nothing clipped.
        clip_rect: [0.0, 0.0, width.max(8.0), line_height],
    });
}

/// Push a port-name / summary label using the shared port font.
#[allow(clippy::too_many_arguments)]
fn push_port_label(
    out: &mut Vec<PlaneLabel>,
    camera: &Camera,
    depth_offset: Vec3,
    anchor: Vec2,
    width: f32,
    text: &str,
    color: [f32; 4],
    raster_scale: f32,
    xform: NodeXform,
) {
    push_label(
        out,
        camera,
        depth_offset,
        anchor,
        width,
        text,
        PORT_FONT,
        PORT_LINE_H,
        color,
        raster_scale,
        xform,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use nodes_core::{Subnet, compute_static_layout};

    /// Build a one-node graph and return its plane labels for the given `draw_titles`.
    fn labels_for(draw_titles: bool) -> (Vec<PlaneLabel>, String) {
        let (registry, value_types) = nodes_core::default_registry();

        let mut subnet = Subnet::new();
        let id = subnet
            .create_node("Add", [0.0, 0.0], &registry, &value_types)
            .expect("Add");
        subnet
            .nodes
            .get_mut(&id)
            .unwrap()
            .set_view_mode(NodeViewMode::Full);
        let title = subnet.nodes.get(&id).unwrap().display_name().to_string();

        let layout = compute_static_layout(&subnet);
        let labels = build_graph_plane_labels(
            &layout,
            &subnet,
            &registry,
            &Camera::default(),
            Vec2::new(1920.0, 1080.0),
            None,
            &ui_theme::Theme::default(),
            draw_titles,
        );
        (labels, title)
    }

    /// Editor hosts (`draw_titles = false`): the widget title bar owns the title, so this pass
    /// emits port names but no title label — otherwise every name would be drawn twice.
    #[test]
    fn full_node_emits_ports_not_title() {
        let (labels, title) = labels_for(false);
        assert!(
            !labels.iter().any(|l| l.text == title),
            "title {title:?} must come from the widget bar, not a plane label; got {:?}",
            labels.iter().map(|l| l.text.as_str()).collect::<Vec<_>>()
        );
        assert!(
            labels.iter().any(|l| l.text == "result"),
            "expected output port-name label 'result', got {:?}",
            labels.iter().map(|l| l.text.as_str()).collect::<Vec<_>>()
        );
    }

    /// Widget-less hosts (`draw_titles = true`, the egui DIAGRAM embed): the title comes from this
    /// pass, as a plane label — so it rides the node's surface transform and stays correct under
    /// perspective/orbit instead of being painted into a screen-space rect by the host.
    #[test]
    fn diagram_host_emits_title_label() {
        let (labels, title) = labels_for(true);
        assert!(!title.is_empty(), "node should carry a display name");
        assert!(
            labels.iter().any(|l| l.text == title),
            "expected title label {title:?}, got {:?}",
            labels.iter().map(|l| l.text.as_str()).collect::<Vec<_>>()
        );
        assert!(
            labels.iter().any(|l| l.text == "result"),
            "port labels must still be emitted alongside the title"
        );
    }
}
