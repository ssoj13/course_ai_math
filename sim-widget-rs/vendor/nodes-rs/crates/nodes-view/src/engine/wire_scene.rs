//! Wire geometry builder for 3D tube rendering.
//!
//! This module converts graph connections (stored in `Subnet` adjacency) into
//! `wgpu_tube::TubeInstance` records. Selection marquee remains an SDF shape in
//! the regular shape pass.

use nodes_core::{GraphRead, LayoutCache, NodeId, NodeTypeRegistry, PinSide};
use wgpu_tube::{CapKind, TubeInstance, pack_caps};

use crate::VisibleNodes;
use crate::camera::GraphPlane;

use super::shape2d::{PickLayer, PickTarget, Shape2d, ShapeInstance, ShapeStyle};

/// Half-width of wire stroke in graph UV units (scales with zoom like node bodies); the drawn
/// wire is twice this wide (here: 4.0 UV total).
pub const WIRE_HALF_WIDTH_UV: f32 = 2.0;

/// Non-pickable sentinel id for wire/marquee shapes. These shapes set
/// `pickable: false`, so the id is never matched against a real node; the value
/// is irrelevant and only fills the [`PickTarget`] payload.
fn sentinel_node() -> NodeId {
    NodeId::default()
}

/// The read-only graph a wire pass reads: the shared [`GraphRead`] trio (topology + the two type
/// tables that color each wire end) plus the pin geometry (`layout`).
///
/// Grouped because resolving a SINGLE connection needs all four — the edge from `graph.subnet`,
/// its pin endpoints from `layout`, and the port types from `graph.registry`/`graph.value_types`
/// — and no caller ever varies one without the others. `layout` stays HERE rather than in the
/// shared trio because only `nodes-view` needs pin geometry; folding it in would force the fit on
/// the editor and chrome crates, which do not.
#[derive(Clone, Copy)]
pub struct WireGraph<'a> {
    pub graph: GraphRead<'a>,
    pub layout: &'a LayoutCache,
}

/// Build tube instances for all visible subnet connections.
///
/// # Where this is used
///
/// Called by `GraphRenderer::render_scene` before pass encoding, then uploaded to
/// `wgpu_tube::TubeRenderer` (`set_camera`/`update_instances`) and drawn in the
/// main color pass.
///
/// # Geometry mapping
///
/// Pin positions in `LayoutCache` are graph-UV coordinates (`[f32; 2]`). Tubes
/// are rendered in world space, so each endpoint/control point is projected
/// through [`GraphPlane::graph_to_world`]. Control points follow the same
/// horizontal S-curve heuristic used by the old 2D wire path (`dx = 0.4 * |x|`)
/// to preserve connection shape.
pub fn build_wire_tubes(
    graph: WireGraph<'_>,
    visible: Option<&VisibleNodes>,
    plane: &GraphPlane,
    radius_world: f32,
    out: &mut Vec<TubeInstance>,
) {
    let WireGraph {
        graph:
            GraphRead {
                subnet,
                registry,
                value_types,
            },
        layout,
    } = graph;
    let radius = radius_world.max(0.0);

    for (&to_node, consumer) in &subnet.nodes {
        for (to_input_idx, input_name) in consumer.input_names().iter().enumerate() {
            let Some(edge) = subnet.index.in_edge(to_node, input_name) else {
                continue;
            };

            let from_node = edge.from_node;
            let Some(source) = subnet.nodes.get(&from_node) else {
                continue;
            };
            let Some(from_output_idx) = source
                .output_names()
                .iter()
                .position(|name| name == &edge.from_output)
            else {
                continue;
            };

            // PLACED (rotation-applied) pin centers via the SSOT accessor: `pin_positions` are in
            // each node's unrotated local frame, but the pin dots are drawn rotated about the node
            // center (shader). Reading local positions here would make wires miss the sockets of any
            // rotated node — `pin_world` applies the same `NodeXform` the body/pins use.
            let (Some(a_uv), Some(b_uv)) = (
                layout.pin_world(from_node, PinSide::Output, from_output_idx),
                layout.pin_world(to_node, PinSide::Input, to_input_idx),
            ) else {
                continue;
            };
            // Draw the wire when EITHER endpoint node is on screen: a connection between two
            // culled (offscreen) nodes cannot itself be visible. Cheap and robust; wires are one
            // instanced draw regardless of count, so no per-wire bbox math is warranted.
            if let Some(vis) = visible
                && !vis.is_visible(from_node)
                && !vis.is_visible(to_node)
            {
                continue;
            }

            let dx = (b_uv[0] - a_uv[0]).abs() * 0.4;
            let c1_uv = [a_uv[0] + dx, a_uv[1]];
            let c2_uv = [b_uv[0] - dx, b_uv[1]];

            let p0 = plane.graph_to_world(a_uv[0], a_uv[1]);
            let p1 = plane.graph_to_world(c1_uv[0], c1_uv[1]);
            let p2 = plane.graph_to_world(c2_uv[0], c2_uv[1]);
            let p3 = plane.graph_to_world(b_uv[0], b_uv[1]);

            // Per-type source->dest gradient over the tube length (u = 0..1): the output
            // pin's type colour at the start (u=0), the input pin's type colour at the end
            // (u=1). `wgpu_tube` lerps color0->color1 along u, so a Float->Vec3 wire visibly
            // shifts hue from source to destination. (The blended `wire_color_for_types` is
            // still used for the flat pin tint elsewhere; here we want the two endpoints.)
            let from_type = output_port_type(registry, source.type_name(), from_output_idx);
            let to_type = input_port_type(registry, consumer.type_name(), to_input_idx);
            let color0 = nodes_core::pin_visual_for_type(from_type, value_types).color;
            let color1 = nodes_core::pin_visual_for_type(to_type, value_types).color;

            out.push(TubeInstance {
                p0: [p0.x, p0.y, p0.z, radius],
                p1: [p1.x, p1.y, p1.z, 0.0],
                p2: [p2.x, p2.y, p2.z, 0.0],
                p3: [p3.x, p3.y, p3.z, radius],
                color0,
                color1,
                // Start always Round (clean exit from the output pin); the END cap hints the
                // connection's data kind (see `end_cap_for_type`).
                caps: pack_caps(CapKind::Round, end_cap_for_type(from_type)),
                _pad: [0; 3],
            });
        }
    }
}

/// Build one temporary tube for in-progress wire drag preview.
///
/// `a_world_uv`/`b_world_uv` are graph-plane UV coordinates (the naming follows
/// existing `WirePreview` fields). The same S-curve heuristic is applied so the
/// preview matches persisted connection geometry.
pub fn wire_preview_tube(
    plane: &GraphPlane,
    a_world_uv: [f32; 2],
    b_world_uv: [f32; 2],
    color: [f32; 4],
    radius_world: f32,
) -> TubeInstance {
    let dx = (b_world_uv[0] - a_world_uv[0]).abs() * 0.4;
    let c1_uv = [a_world_uv[0] + dx, a_world_uv[1]];
    let c2_uv = [b_world_uv[0] - dx, b_world_uv[1]];

    let p0 = plane.graph_to_world(a_world_uv[0], a_world_uv[1]);
    let p1 = plane.graph_to_world(c1_uv[0], c1_uv[1]);
    let p2 = plane.graph_to_world(c2_uv[0], c2_uv[1]);
    let p3 = plane.graph_to_world(b_world_uv[0], b_world_uv[1]);

    TubeInstance {
        p0: [p0.x, p0.y, p0.z, radius_world.max(0.0)],
        p1: [p1.x, p1.y, p1.z, 0.0],
        p2: [p2.x, p2.y, p2.z, 0.0],
        p3: [p3.x, p3.y, p3.z, radius_world.max(0.0)],
        color0: color,
        color1: color,
        caps: pack_caps(CapKind::Round, CapKind::Arrow),
        _pad: [0; 3],
    }
}

/// End-cap style hinting a connection's data kind. The tube's start cap stays `Round`
/// (a clean exit from the output pin); the end cap varies so different data families read
/// differently at a glance without needing colour alone:
///
/// - `Any` (untyped pass-through) -> `Flat`: no directional emphasis.
/// - `Map` / object bags -> `Cube`: a blocky terminator for structured payloads.
/// - everything else (numeric/vector/colour/bool/...) -> `Arrow`: the common dataflow arrow.
///
/// This is a purely visual mapping local to `nodes-view` (`CapKind` is a `wgpu-tube` type and
/// must not leak into `nodes-core`). Keyed on the SOURCE output type — "what flows".
fn end_cap_for_type(from_type: &str) -> CapKind {
    match from_type {
        "Any" => CapKind::Flat,
        "Map" => CapKind::Cube,
        _ => CapKind::Arrow,
    }
}

/// Declared output port type by index, from the registered [`nodes_core::NodeDef`].
fn output_port_type<'a>(registry: &'a NodeTypeRegistry, type_name: &str, index: usize) -> &'a str {
    registry
        .get(type_name)
        .and_then(|d| d.outputs.get(index))
        .map(|p| p.port_type.as_str())
        .unwrap_or("float")
}

/// Declared input port type by index, from the registered [`nodes_core::NodeDef`].
fn input_port_type<'a>(registry: &'a NodeTypeRegistry, type_name: &str, index: usize) -> &'a str {
    registry
        .get(type_name)
        .and_then(|d| d.inputs.get(index))
        .map(|p| p.port_type.as_str())
        .unwrap_or("float")
}

/// Selection marquee in graph UV (semi-transparent fill + stroke).
pub fn append_marquee_shape(
    out: &mut Vec<ShapeInstance>,
    start: [f32; 2],
    end: [f32; 2],
    theme: &ui_theme::Theme,
) {
    let min = [start[0].min(end[0]), start[1].min(end[1])];
    let max = [start[0].max(end[0]), start[1].max(end[1])];
    if max[0] - min[0] < 0.5 && max[1] - min[1] < 0.5 {
        return;
    }
    out.push(ShapeInstance {
        shape: Shape2d::RoundedRect {
            min,
            max,
            radius: 0.0,
        },
        // Selection rubber-band shares the wire-preview accent family (themed SSOT).
        style: ShapeStyle {
            fill: theme.wire_preview_fill,
            stroke: theme.wire_preview_stroke,
            stroke_width: 1.5,
        },
        layer: PickLayer::Outline,
        pick: PickTarget::NodeBody(sentinel_node()),
        hollow_border: None,
        pickable: false,
        pick_id: 0,
        depth_bias: 0.0,
        rotation: 0.0,
        pivot: [0.0, 0.0],
    });
}
