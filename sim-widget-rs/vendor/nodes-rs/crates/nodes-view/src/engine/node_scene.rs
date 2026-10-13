//! Build the Mesh3D backend's per-node draw items from the SAME layout + theme the SDF backend
//! uses. Each node becomes `(NodeMeshParams, NodeInstance)`: the params (real width/height/bands)
//! pick a cached procedural mesh in [`wgpu_node`]; the instance places it with a RIGID transform
//! (rotation onto the graph plane + translation, NO scale) so corners stay true circles.

use glam::{Mat4, Quat};
use nodes_core::{LayoutCache, NodeTypeRegistry, Subnet, is_frame_node, node_regions};

use crate::VisibleNodes;
use crate::camera::GraphPlane;

use super::shape_scene::header_fill_rgba;

/// Build `(params, instance)` for every node. `style` carries the shared shape/radius/depth; the
/// per-node width/height/title-band come from the layout so the 3D card matches the SDF node.
pub fn build_node_batches(
    layout: &LayoutCache,
    subnet: &Subnet,
    registry: &NodeTypeRegistry,
    plane: &GraphPlane,
    theme: &ui_theme::Theme,
    style: &wgpu_node::NodeStyle,
    visible: Option<&VisibleNodes>,
) -> Vec<(wgpu_node::NodeMeshParams, wgpu_node::NodeInstance)> {
    let mut out = Vec::with_capacity(layout.nodes.len());

    // Node local axes → world (unit, so the transform is rigid): +x = screen-right (−axis_u),
    // +y = screen-up (+axis_v, title on top), +z = toward camera (plane normal).
    let right0 = -plane.axis_u;
    let up0 = plane.axis_v;
    let front = plane.normal;
    // In-plane rotation axis for `LayoutNode.rotation` — the SAME axis (`axis_u × axis_v`) the SDF
    // body / pins / widgets / labels use, so a rotated Mesh3D card turns identically to them.
    let rot_axis = plane.axis_u.cross(plane.axis_v).normalize_or_zero();
    // Extruded: push the card back by half its thickness so the front face lands on the plane.
    let sink = match style.shape {
        wgpu_node::NodeShape::Extruded => style.depth * 0.5,
        wgpu_node::NodeShape::Flat => 0.0,
    };

    for (id, layout_node) in layout.nodes.iter() {
        // Same shared visible-set as the SDF/wire/text layers. Mesh3D bodies have no LOD tiers
        // (a card is either drawn or culled); LOD only trims per-port chrome/text/widgets.
        if let Some(vis) = visible
            && !vis.is_visible(*id)
        {
            continue;
        }
        let Some(node) = subnet.nodes.get(id) else {
            continue;
        };
        let [w, h] = layout_node.size;
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let frame = is_frame_node(node);

        // Title band height in node units, taken from the SAME region geometry the SDF path uses.
        let regions = node_regions(
            layout_node.center,
            layout_node.size,
            node.view_mode(),
            frame,
        );
        let title_h = (regions.title.max[1] - regions.title.min[1]).max(0.0);

        let params = style.params(w, h, title_h, 0.0);

        // Apply the node's single placement rotation ([`LayoutNode::rotation`]) to the card basis so
        // the 3D body rotates about its own center in the plane, matching every other layer.
        let (right, up) = if layout_node.rotation != 0.0 {
            let q = Quat::from_axis_angle(rot_axis, layout_node.rotation);
            (q * right0, q * up0)
        } else {
            (right0, up0)
        };

        let center =
            plane.graph_to_world(layout_node.center[0], layout_node.center[1]) - front * sink;
        let model = Mat4::from_cols(
            right.extend(0.0),
            up.extend(0.0),
            front.extend(0.0),
            center.extend(1.0),
        );

        let title_accent = header_fill_rgba(registry, node.type_name(), frame);
        let (body_theme, _t, footer) = wgpu_node::node_materials(theme, title_accent);
        let body = if frame {
            theme.node_body_frame
        } else {
            body_theme
        };

        out.push((
            params,
            wgpu_node::NodeInstance {
                model: model.to_cols_array_2d(),
                body,
                title: title_accent,
                footer,
                pick_id: 0,
                _pad: [0; 3],
            },
        ));
    }
    out
}

/// Which node-body renderer the graph viewport uses. SDF is the mature 2D-on-plane path; Mesh3D
/// draws real extruded cards via [`wgpu_node`] (shared theme + one cached procedural mesh per size).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GraphBackend {
    /// Signed-distance-field quads on the graph plane (default; full chrome + interaction).
    #[default]
    Sdf,
    /// Real 3D extruded node cards from [`wgpu_node`].
    Mesh3D,
}
