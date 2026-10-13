//! Build [`ShapeInstance`] list from layout + subnet (same data for GPU + pick).

use std::collections::HashMap;

use nodes_core::{
    LayoutCache, Node, NodeId, NodeTypeRegistry, NodeViewKind, NodeViewMode, PinSide, PinVisual,
    Subnet, ValueTypes, category_header_bg_rgba, drawn_pin_indices, is_frame_node, is_note_node,
    node_regions,
};

use nodes_core::chrome::PIN_R;
use nodes_core::pin_visual_for_type;

use super::image_pipeline::ImagePreview;
use super::shape2d::{PickLayer, PickTarget, Shape2d, ShapeInstance, ShapeStyle, pin_shape_to_sdf};
use crate::VisibleNodes;

/// Per-node depth stride along the depth axis (clip Z, see `shape.wgsl`). Used at small scale;
/// compressed to fit [`DEPTH_SPAN_BUDGET`] in large graphs (see `node_stride` in build).
const NODE_DEPTH_STRIDE: f32 = 0.002;
/// Max total depth span (reversed-Z NDC) across ALL nodes. The per-node stride is compressed
/// to keep `node_count * stride` within this budget, so far nodes never get pushed past a clip
/// plane by the `clip.z += depth_bias * w` bias (shape.wgsl) and vanish — the bug seen at 1024
/// nodes. Small graphs keep the full [`NODE_DEPTH_STRIDE`]; only large graphs compress.
const DEPTH_SPAN_BUDGET: f32 = 0.4;
/// How many sub-slots the per-node stride is divided into for the WITHIN-node layers
/// (body/title/pin/outline). The intra-node layer stack uses `node_stride / DEPTH_LAYER_SLOTS`,
/// so a node's whole stack always fits inside its own slot — at ANY node count — and the
/// chrome of a far node can never z-interleave over a nearer overlapping node's body.
/// Max layer index (Pin=3) + the outline bump must stay below `DEPTH_LAYER_SLOTS`.
const DEPTH_LAYER_SLOTS: f32 = 8.0;

/// Depth bias for one layer of a node. `layer_stride` is derived per-build from the (possibly
/// compressed) `node_stride` so layers always nest inside one node slot.
#[inline]
fn shape_layer_depth(depth_base: f32, layer: PickLayer, layer_stride: f32) -> f32 {
    depth_base + (layer as u32 as f32) * layer_stride
}

const NODE_CORNER_R: f32 = 6.0;
/// Title strip is drawn flush on the node body (no bottom rounding seam).
const TITLE_BAR_RADIUS: f32 = 0.0;
const FRAME_BORDER_PICK: f32 = 10.0;
// Node/pin colors are no longer hard-coded here — they come from the shared `ui_theme::Theme`
// (SSOT) via `ShapeSceneParams::theme`: node outline = `node_outline`/`node_outline_hot`, node
// body = `node_body`/`node_body_frame`, pin stroke = `pin_stroke`/`pin_stroke_hot`.

pub struct ShapeSceneParams<'a> {
    pub layout: &'a LayoutCache,
    pub subnet: &'a Subnet,
    pub registry: &'a NodeTypeRegistry,
    pub value_types: &'a ValueTypes,
    pub selected: &'a [NodeId],
    pub hover: Option<&'a PickTarget>,
    /// Shared visible-set + per-node LOD ([`crate::compute_visible_nodes`]): culled nodes are
    /// skipped, and [`crate::NodeLod::BodyOnly`] nodes emit only their body/title (no
    /// pins/fields/chrome). `None` = draw every node at full detail (no culling).
    pub visible: Option<&'a VisibleNodes>,
    /// Shared color palette (SSOT) — every node/pin/outline color is read from here.
    pub theme: &'a ui_theme::Theme,
    /// OPTIONAL per-node body-fill override (read-only status coloring hook). When a node id is
    /// present its RGBA REPLACES the theme body fill for that node's card (title/pins/outline are
    /// untouched); absent ids and `None` fall back to the theme default, so the editor/demo paths
    /// that pass `None` are byte-unchanged. Supplied by diagram hosts (e.g. the colmap Pipeline
    /// tab tinting nodes by live pipeline status).
    pub node_tint: Option<&'a HashMap<NodeId, [f32; 4]>>,
}

pub(crate) fn header_fill_rgba(
    registry: &NodeTypeRegistry,
    type_name: &str,
    frame: bool,
) -> [f32; 4] {
    if frame {
        return [0.14, 0.16, 0.20, 0.55];
    }
    // `NodeDef` names are `Arc<str>` (a runtime-built type has no `&'static str`); deref to a
    // borrowed `&str` — no allocation on this per-node path.
    let cat = registry
        .get(type_name)
        .map(|d| &*d.category)
        .unwrap_or("Base");
    category_header_bg_rgba(cat)
}

/// Emit invisible pick-only quads for a node's title chrome flags (view-mode 3-strip toggle /
/// bypass / display). Shared by EVERY view mode — INCLUDING Minimal — so a node can always be
/// cycled back out of Minimal. Previously these were emitted only in the non-minimal branch, so a
/// node toggled to Minimal lost its clickable 3-strip and got stuck (unresponsive chrome). The
/// flag rects come from `node_regions`, which defines them for all non-frame view modes; the
/// visuals are drawn by the editor's title widget layer — these only stamp object ids for picking.
#[allow(clippy::too_many_arguments)]
fn push_chrome_flags(
    out: &mut Vec<ShapeInstance>,
    id: NodeId,
    view_mode_toggle: Option<nodes_core::GraphRect>,
    bypass_flag: Option<nodes_core::GraphRect>,
    display_flag: Option<nodes_core::GraphRect>,
    depth_base: f32,
    layer_stride: f32,
) {
    let mut chrome = |pick: PickTarget, r: Option<nodes_core::GraphRect>| {
        if let Some(r) = r {
            out.push(ShapeInstance {
                shape: Shape2d::RoundedRect {
                    min: r.min,
                    max: r.max,
                    radius: 0.0,
                },
                style: ShapeStyle {
                    fill: [0.0, 0.0, 0.0, 0.0],
                    stroke: [0.0, 0.0, 0.0, 0.0],
                    stroke_width: 0.0,
                },
                layer: PickLayer::Field,
                pick,
                hollow_border: None,
                pickable: true,
                pick_id: 0,
                depth_bias: shape_layer_depth(depth_base, PickLayer::Field, layer_stride),
                rotation: 0.0,
                pivot: [0.0, 0.0],
            });
        }
    };
    chrome(PickTarget::ViewModeToggle(id), view_mode_toggle);
    chrome(PickTarget::BypassFlag(id), bypass_flag);
    chrome(PickTarget::DisplayFlag(id), display_flag);
}

/// Declared port type name for `side`/`index` from the registered
/// [`nodes_core::NodeDef`] (port order matches the node's `input_names`/`output_names`).
fn port_type<'a>(
    registry: &'a NodeTypeRegistry,
    type_name: &str,
    side: PinSide,
    index: usize,
) -> &'a str {
    registry
        .get(type_name)
        .and_then(|d| match side {
            PinSide::Input => d.inputs.get(index),
            PinSide::Output => d.outputs.get(index),
        })
        .map(|p| p.port_type.as_str())
        .unwrap_or("float")
}

fn node_hot(hover: Option<&PickTarget>, id: &NodeId) -> bool {
    matches!(
        hover,
        Some(PickTarget::NodeBody(n) | PickTarget::NodeTitle(n)) if n == id
    )
}

/// Build shape instances in draw order and assign stable per-frame GPU ids.
///
/// `registry` is reset at the start of each call (`begin_frame`) and receives one
/// allocation per `pickable` instance. Non-pickable chrome/wires keep id `0` (`ID_NONE`).
pub fn build_shape_instances(
    p: &ShapeSceneParams<'_>,
    registry: &mut wgpu_pick::IdRegistry<PickTarget>,
    // Live2D nodes whose cooked primary output is an RGBA8 image push a textured-quad preview here
    // (drawn by the image pass) instead of the flat color swatch. Pick-only callers pass a scratch.
    previews: &mut Vec<ImagePreview>,
) -> Vec<ShapeInstance> {
    registry.begin_frame();
    let mut out = Vec::new();

    let mut node_ids: Vec<&NodeId> = p.layout.nodes.keys().collect();
    node_ids.sort_by(|a, b| {
        let fa = p.subnet.nodes.get(a).is_some_and(is_frame_node);
        let fb = p.subnet.nodes.get(b).is_some_and(is_frame_node);
        let sa = p.selected.contains(a);
        let sb = p.selected.contains(b);
        let ha = node_hot(p.hover, a);
        let hb = node_hot(p.hover, b);
        let ya = p.layout.nodes.get(a).map(|n| n.center[1]).unwrap_or(0.0);
        let yb = p.layout.nodes.get(b).map(|n| n.center[1]).unwrap_or(0.0);
        let xa = p.layout.nodes.get(a).map(|n| n.center[0]).unwrap_or(0.0);
        let xb = p.layout.nodes.get(b).map(|n| n.center[0]).unwrap_or(0.0);
        fa.cmp(&fb)
            .then_with(|| sa.cmp(&sb))
            .then_with(|| ha.cmp(&hb))
            .then_with(|| ya.total_cmp(&yb))
            .then_with(|| xa.total_cmp(&xb))
            .then_with(|| a.0.as_u128().cmp(&b.0.as_u128()))
    });

    // Compress the per-node depth step so the total depth span fits DEPTH_SPAN_BUDGET no matter
    // how many nodes there are (fixes far nodes being depth-clipped in big graphs).
    let node_count = node_ids.len().max(1);
    let node_stride = NODE_DEPTH_STRIDE.min(DEPTH_SPAN_BUDGET / node_count as f32);
    // Intra-node layer step, derived so the whole layer stack fits one node slot at any N.
    let layer_stride = node_stride / DEPTH_LAYER_SLOTS;
    // Small extra bump so the outline clears title/pins when orbiting (kept within the slot).
    let outline_bias = layer_stride * 0.25;
    for (draw_rank, id) in node_ids.into_iter().enumerate() {
        let depth_base = draw_rank as f32 * node_stride;
        let Some(layout_node) = p.layout.nodes.get(id) else {
            continue;
        };
        let Some(node) = p.subnet.nodes.get(id) else {
            continue;
        };
        // UNIFIED cull: skip nodes the shared visible-set marks offscreen (the SAME policy the tube
        // wires, plane text and the editor's widget layer use — so a node is never drawn half).
        if let Some(vis) = p.visible
            && !vis.is_visible(*id)
        {
            continue;
        }
        // Per-node LOD: `BodyOnly` (a few px tall) draws just the body/title/outline, dropping the
        // pins, inline-field pick rects and chrome flags that are invisible/unusable at that size.
        let detail = p.visible.is_none_or(|v| v.lod(*id).draws_detail());
        let frame = is_frame_node(node);
        let view_mode = node.view_mode();
        let minimal = view_mode == NodeViewMode::Minimal;
        let regions = node_regions(layout_node.center, layout_node.size, view_mode, frame);
        let full_min = regions.full.min;
        let full_max = regions.full.max;

        let is_sel = p.selected.contains(id);
        let highlighted = is_sel || node_hot(p.hover, id);
        // OPTIONAL status tint: when the host supplied a body-fill override for this node id it
        // REPLACES the theme body fill below (title/pins/outline stay theme-driven so selection and
        // category cues survive). `None` → theme default, keeping the untinted editor path identical.
        let tint = p.node_tint.and_then(|m| m.get(id).copied());

        if is_note_node(node) {
            // Sticky note: a SOLID opaque card (no title strip, no pins) whose multi-line text is
            // drawn AND edited by the editor's widget layer (a `MultilineEdit` placed over this
            // body). The body is fully pickable (`NodeBody`) so a note selects/drags like any node;
            // the editor focuses its text widget from that same body pick. Kept as a small,
            // self-contained block instead of threading a `note` flag through the minimal/full
            // branches below.
            out.push(ShapeInstance {
                shape: Shape2d::RoundedRect {
                    min: full_min,
                    max: full_max,
                    radius: NODE_CORNER_R,
                },
                style: ShapeStyle {
                    fill: p.theme.node_body,
                    stroke: [0.0, 0.0, 0.0, 0.0],
                    stroke_width: 0.0,
                },
                layer: PickLayer::Body,
                pick: PickTarget::NodeBody(*id),
                hollow_border: None,
                pickable: true,
                pick_id: 0,
                depth_bias: shape_layer_depth(depth_base, PickLayer::Body, layer_stride),
                rotation: 0.0,
                pivot: [0.0, 0.0],
            });
            let outline_stroke = if highlighted {
                p.theme.node_outline_hot
            } else {
                p.theme.node_outline
            };
            let outline_width = if is_sel {
                2.5
            } else if highlighted {
                2.0
            } else {
                1.5
            };
            out.push(ShapeInstance {
                shape: Shape2d::RoundedRect {
                    min: full_min,
                    max: full_max,
                    radius: NODE_CORNER_R,
                },
                style: ShapeStyle {
                    fill: [0.0, 0.0, 0.0, 0.0],
                    stroke: outline_stroke,
                    stroke_width: outline_width,
                },
                layer: PickLayer::Outline,
                pick: PickTarget::NodeBody(*id),
                hollow_border: None,
                pickable: false,
                pick_id: 0,
                depth_bias: shape_layer_depth(depth_base, PickLayer::Outline, layer_stride)
                    + outline_bias,
                rotation: 0.0,
                pivot: [0.0, 0.0],
            });
            continue;
        }

        if minimal {
            let title_fill = header_fill_rgba(p.registry, node.type_name(), frame);
            let title_min = regions.title.min;
            let title_max = regions.title.max;
            out.push(ShapeInstance {
                shape: Shape2d::RoundedRect {
                    min: full_min,
                    max: full_max,
                    radius: NODE_CORNER_R,
                },
                style: ShapeStyle {
                    fill: tint.unwrap_or(p.theme.node_body),
                    stroke: [0.0, 0.0, 0.0, 0.0],
                    stroke_width: 0.0,
                },
                layer: PickLayer::Body,
                pick: PickTarget::NodeBody(*id),
                hollow_border: None,
                pickable: true,
                pick_id: 0,
                depth_bias: shape_layer_depth(depth_base, PickLayer::Body, layer_stride),
                rotation: 0.0,
                pivot: [0.0, 0.0],
            });
            out.push(ShapeInstance {
                shape: Shape2d::RoundedRect {
                    min: title_min,
                    max: title_max,
                    radius: TITLE_BAR_RADIUS,
                },
                style: ShapeStyle {
                    fill: title_fill,
                    stroke: [0.0, 0.0, 0.0, 0.0],
                    stroke_width: 0.0,
                },
                layer: PickLayer::Title,
                pick: PickTarget::NodeTitle(*id),
                hollow_border: None,
                pickable: true,
                pick_id: 0,
                depth_bias: shape_layer_depth(depth_base, PickLayer::Title, layer_stride),
                rotation: 0.0,
                pivot: [0.0, 0.0],
            });
            let outline_stroke = if highlighted {
                p.theme.node_outline_hot
            } else {
                p.theme.node_outline
            };
            let outline_width = if is_sel {
                2.5
            } else if highlighted {
                2.0
            } else {
                1.5
            };
            out.push(ShapeInstance {
                shape: Shape2d::RoundedRect {
                    min: full_min,
                    max: full_max,
                    radius: NODE_CORNER_R,
                },
                style: ShapeStyle {
                    fill: [0.0, 0.0, 0.0, 0.0],
                    stroke: outline_stroke,
                    stroke_width: outline_width,
                },
                layer: PickLayer::Outline,
                pick: PickTarget::NodeBody(*id),
                hollow_border: None,
                pickable: false,
                pick_id: 0,
                depth_bias: shape_layer_depth(depth_base, PickLayer::Outline, layer_stride)
                    + outline_bias,
                rotation: 0.0,
                pivot: [0.0, 0.0],
            });
            if !frame && detail {
                // SSOT for "pins to DRAW": Minimal collapses to the first socket per side, so the
                // single visible socket matches the single label (see `displayed_pin_indices`).
                let vis_in = drawn_pin_indices(node, PinSide::Input);
                let vis_out = drawn_pin_indices(node, PinSide::Output);
                if let Some(&i) = vis_in.first()
                    && let Some(pin) = p.layout.pin_positions.get(&(*id, PinSide::Input, i))
                {
                    let visual = pin_visual_for_type(
                        port_type(p.registry, node.type_name(), PinSide::Input, i),
                        p.value_types,
                    );
                    push_pin(
                        &mut out,
                        *id,
                        PinSide::Input,
                        i,
                        pin.world,
                        visual,
                        p.hover,
                        depth_base,
                        layer_stride,
                        p.theme,
                    );
                }
                if let Some(&i) = vis_out.first()
                    && let Some(pin) = p.layout.pin_positions.get(&(*id, PinSide::Output, i))
                {
                    let visual = pin_visual_for_type(
                        port_type(p.registry, node.type_name(), PinSide::Output, i),
                        p.value_types,
                    );
                    push_pin(
                        &mut out,
                        *id,
                        PinSide::Output,
                        i,
                        pin.world,
                        visual,
                        p.hover,
                        depth_base,
                        layer_stride,
                        p.theme,
                    );
                }
            }
            // Chrome flags in Minimal too, so the 3-strip stays clickable and the node can be
            // cycled back out of Minimal (frame nodes have `None` rects → no-op). Dropped at
            // `BodyOnly` LOD (a few px tall — unclickable anyway; zoom in restores them).
            if detail {
                push_chrome_flags(
                    &mut out,
                    *id,
                    regions.view_mode_toggle,
                    regions.bypass_flag,
                    regions.display_flag,
                    depth_base,
                    layer_stride,
                );
            }
            continue;
        }

        let body_fill = tint.unwrap_or(if frame {
            p.theme.node_body_frame
        } else {
            p.theme.node_body
        });
        let title_fill = header_fill_rgba(p.registry, node.type_name(), frame);

        out.push(ShapeInstance {
            shape: Shape2d::RoundedRect {
                min: full_min,
                max: full_max,
                radius: NODE_CORNER_R,
            },
            style: ShapeStyle {
                fill: body_fill,
                stroke: [0.0, 0.0, 0.0, 0.0],
                stroke_width: 0.0,
            },
            layer: PickLayer::Body,
            pick: PickTarget::NodeBody(*id),
            hollow_border: if frame { Some(FRAME_BORDER_PICK) } else { None },
            pickable: true,
            pick_id: 0,
            depth_bias: shape_layer_depth(depth_base, PickLayer::Body, layer_stride),
            rotation: 0.0,
            pivot: [0.0, 0.0],
        });

        out.push(ShapeInstance {
            shape: Shape2d::RoundedRect {
                min: regions.title.min,
                max: regions.title.max,
                radius: TITLE_BAR_RADIUS,
            },
            style: ShapeStyle {
                fill: title_fill,
                stroke: [0.0, 0.0, 0.0, 0.0],
                stroke_width: 0.0,
            },
            layer: PickLayer::Title,
            pick: PickTarget::NodeTitle(*id),
            hollow_border: None,
            pickable: true,
            pick_id: 0,
            depth_bias: shape_layer_depth(depth_base, PickLayer::Title, layer_stride),
            rotation: 0.0,
            pivot: [0.0, 0.0],
        });

        // Live2D body preview: a filled swatch in the content region driven by the node's
        // cooked primary output (colour / scalar). Regenerated implicitly every frame from
        // `output_value` — cheap enough to skip caching (the `cooked_gen` freshness signal is
        // reserved for the future expensive Live3D thumbnail path). Frame nodes never preview.
        if !frame && node.view_kind() == NodeViewKind::Live2D {
            let pad = 4.0;
            let bmin = [regions.body.min[0] + pad, regions.body.min[1] + pad];
            let bmax = [regions.body.max[0] - pad, regions.body.max[1] - pad];
            let fits = bmax[0] > bmin[0] && bmax[1] > bmin[1];
            if let Some((iw, ih)) = nodes_core::primary_image_dims(node) {
                // Cooked output is a real RGBA8 image: collect a textured-quad preview (drawn by the
                // image pass, `image_pipeline`) and SKIP the flat swatch. The invisible `NodeBody`
                // pick quad drawn below still makes a click on the image select the node.
                if fits {
                    let xf = layout_node.xform();
                    previews.push(ImagePreview {
                        node_id: *id,
                        aabb_min: bmin,
                        aabb_max: bmax,
                        pivot: xf.center,
                        rotation: xf.rotation,
                        depth_bias: shape_layer_depth(depth_base, PickLayer::Title, layer_stride),
                        w: iw,
                        h: ih,
                        cooked_gen: node.cooked_gen(),
                    });
                }
            } else if let Some(color) = preview_color(node)
                && fits
            {
                out.push(ShapeInstance {
                    shape: Shape2d::RoundedRect {
                        min: bmin,
                        max: bmax,
                        radius: NODE_CORNER_R * 0.5,
                    },
                    style: ShapeStyle {
                        fill: color,
                        stroke: [0.0, 0.0, 0.0, 0.0],
                        stroke_width: 0.0,
                    },
                    // Above the body fill; a click on the swatch still selects the node.
                    layer: PickLayer::Title,
                    pick: PickTarget::NodeBody(*id),
                    hollow_border: None,
                    pickable: true,
                    pick_id: 0,
                    depth_bias: shape_layer_depth(depth_base, PickLayer::Title, layer_stride),
                    rotation: 0.0,
                    pivot: [0.0, 0.0],
                });
            }
        }

        let outline_stroke = if highlighted {
            p.theme.node_outline_hot
        } else {
            p.theme.node_outline
        };
        let outline_width = if is_sel {
            2.5
        } else if highlighted {
            2.0
        } else {
            1.5
        };
        out.push(ShapeInstance {
            shape: Shape2d::RoundedRect {
                min: full_min,
                max: full_max,
                radius: NODE_CORNER_R,
            },
            style: ShapeStyle {
                fill: [0.0, 0.0, 0.0, 0.0],
                stroke: outline_stroke,
                stroke_width: outline_width,
            },
            layer: PickLayer::Outline,
            pick: PickTarget::NodeBody(*id),
            hollow_border: None,
            pickable: false,
            pick_id: 0,
            depth_bias: shape_layer_depth(depth_base, PickLayer::Outline, layer_stride)
                + outline_bias,
            rotation: 0.0,
            pivot: [0.0, 0.0],
        });

        if frame {
            continue;
        }

        // LOD gate: `BodyOnly` nodes (a few px tall) skip pins, inline-field pick rects and title
        // chrome flags — only body/title/outline remain. `Full`/`NoText` keep the full detail.
        if detail {
            // GPU-pick hit rects for inline editable value fields. Invisible in color (the pick-only
            // flag in `instance_to_gpu` discards them in the color pass), they stamp
            // `NodeField{node,port}` into the id buffer so the editor classifies field clicks purely
            // via GPU pick -- no CPU/router hit-test. Geometry comes from the SAME
            // `inline_field_rects` the editor's widget layer uses, so the hit rect can never drift
            // from the value box. Read-only (wire-connected) fields are skipped so a click there
            // falls through to the node body (selection) instead of focusing a non-editable field.
            for f in nodes_core::chrome::inline_field_rects(p.subnet, *id, p.layout, p.registry) {
                if !f.editable {
                    continue;
                }
                let [fx, fy, fw, fh] = f.rect;
                out.push(ShapeInstance {
                    shape: Shape2d::RoundedRect {
                        min: [fx, fy],
                        max: [fx + fw, fy + fh],
                        radius: 0.0,
                    },
                    style: ShapeStyle {
                        fill: [0.0, 0.0, 0.0, 0.0],
                        stroke: [0.0, 0.0, 0.0, 0.0],
                        stroke_width: 0.0,
                    },
                    layer: PickLayer::Field,
                    pick: PickTarget::NodeField {
                        node: *id,
                        port: f.port,
                    },
                    hollow_border: None,
                    pickable: true,
                    pick_id: 0,
                    depth_bias: shape_layer_depth(depth_base, PickLayer::Field, layer_stride),
                    rotation: 0.0,
                    pivot: [0.0, 0.0],
                });
            }

            // GPU-pick hit rects for the title chrome flags (view-mode toggle, bypass, display). Like
            // the field rects above these are invisible pick-only quads (color-discarded) that stamp
            // their object id into the pick buffer; the flag VISUALS are still drawn by the editor's
            // title widget surface. Rects come from `node_regions` -- the SAME source the widget
            // layout uses -- so pick and visual coincide. This is what makes the chrome first-class in
            // the one GPU pick authority instead of a second CPU/router hit-test.
            push_chrome_flags(
                &mut out,
                *id,
                regions.view_mode_toggle,
                regions.bypass_flag,
                regions.display_flag,
                depth_base,
                layer_stride,
            );

            // DRAW only the pins whose per-port display level is visible at this view mode (SSOT:
            // `displayed_pin_indices`); `visible_pin_indices` still positions every pin for wiring.
            let vis_in = drawn_pin_indices(node, PinSide::Input);
            let vis_out = drawn_pin_indices(node, PinSide::Output);
            for &i in &vis_in {
                if let Some(pin) = p.layout.pin_positions.get(&(*id, PinSide::Input, i)) {
                    let visual = pin_visual_for_type(
                        port_type(p.registry, node.type_name(), PinSide::Input, i),
                        p.value_types,
                    );
                    push_pin(
                        &mut out,
                        *id,
                        PinSide::Input,
                        i,
                        pin.world,
                        visual,
                        p.hover,
                        depth_base,
                        layer_stride,
                        p.theme,
                    );
                }
            }
            for &i in &vis_out {
                if let Some(pin) = p.layout.pin_positions.get(&(*id, PinSide::Output, i)) {
                    let visual = pin_visual_for_type(
                        port_type(p.registry, node.type_name(), PinSide::Output, i),
                        p.value_types,
                    );
                    push_pin(
                        &mut out,
                        *id,
                        PinSide::Output,
                        i,
                        pin.world,
                        visual,
                        p.hover,
                        depth_base,
                        layer_stride,
                        p.theme,
                    );
                }
            }
        } // end `if detail`
    }

    for instance in &mut out {
        if instance.pickable {
            instance.pick_id = registry.alloc(instance.pick.clone());
        }
    }

    // Apply the SINGLE node placement transform ([`LayoutNode::xform`]) to EVERY node-attached
    // instance — body, title, field, outline AND pins. All shapes are authored in the node's
    // unrotated local frame (pins included, since the layout now stores local pin positions); the
    // shader rotates the drawn quad about the node center while evaluating the SDF in that local
    // frame, so a rounded-rect body becomes a correctly-oriented rounded-rect and pins orbit to
    // their rotated sockets — matching the wires (tube endpoints taken via `LayoutCache::pin_world`,
    // the SAME xform) and the widgets/labels (same xform as a surface matrix). GPU pick uses the
    // same VS → in sync.
    for instance in &mut out {
        let nid = match instance.pick {
            PickTarget::NodeBody(id)
            | PickTarget::NodeTitle(id)
            | PickTarget::ViewModeToggle(id)
            | PickTarget::BypassFlag(id)
            | PickTarget::DisplayFlag(id) => id,
            PickTarget::NodeField { node, .. } => node,
            PickTarget::Pin { node, .. } => node,
        };
        if let Some(ln) = p.layout.nodes.get(&nid) {
            let xf = ln.xform();
            if xf.is_identity() {
                continue;
            }
            instance.rotation = xf.rotation;
            instance.pivot = xf.center;
        }
    }

    out
}

/// Map a node's cooked primary output to a Live2D body-preview swatch colour.
///
/// Colour outputs show their RGBA; scalar (float) outputs show a grayscale value clamped to
/// `0..1`; anything else (or a node with no outputs / not-yet-cooked) yields `None` and the
/// node draws as a plain body. Reads the FIRST output (`output_names().first()`) — the node's
/// primary result. Values come straight from the cooked graph, so the swatch tracks edits.
fn preview_color(node: &Node) -> Option<[f32; 4]> {
    let name = node.output_names().first()?.clone();
    let v = node.output_value(&name)?;
    // Order matters: Color (4-comp) before Vec3 (3-comp) before the scalar/bool fallbacks, so a
    // value is classified by its richest interpretation. Components are clamped to 0..1 for
    // display (a preview swatch, not a value readout — out-of-range channels just saturate).
    if let Some(c) = v.as_color() {
        return Some([
            (c[0] as f32).clamp(0.0, 1.0),
            (c[1] as f32).clamp(0.0, 1.0),
            (c[2] as f32).clamp(0.0, 1.0),
            (c[3] as f32).clamp(0.0, 1.0),
        ]);
    }
    if let Some(v3) = v.as_vec3() {
        return Some([
            (v3[0] as f32).clamp(0.0, 1.0),
            (v3[1] as f32).clamp(0.0, 1.0),
            (v3[2] as f32).clamp(0.0, 1.0),
            1.0,
        ]);
    }
    if let Some(b) = v.as_bool() {
        // On = accent green, off = inert gray — a legible boolean indicator.
        return Some(if b {
            [0.30, 0.72, 0.38, 1.0]
        } else {
            [0.22, 0.24, 0.28, 1.0]
        });
    }
    if let Some(f) = v.as_f64() {
        let g = (f as f32).clamp(0.0, 1.0);
        return Some([g, g, g, 1.0]);
    }
    None
}

const PIN_STROKE_W: f32 = 1.25;

#[allow(clippy::too_many_arguments)]
fn push_pin(
    out: &mut Vec<ShapeInstance>,
    node: NodeId,
    side: PinSide,
    index: usize,
    center: [f32; 2],
    visual: PinVisual,
    hover: Option<&PickTarget>,
    depth_base: f32,
    layer_stride: f32,
    theme: &ui_theme::Theme,
) {
    let is_hover = matches!(
        hover,
        Some(PickTarget::Pin {
            node: n,
            side: s,
            index: i
        }) if *n == node && *s == side && *i == index
    );
    let (stroke_color, stroke_w) = visual.stroke.unwrap_or((theme.pin_stroke, PIN_STROKE_W));
    let mut style = ShapeStyle {
        fill: visual.color,
        stroke: stroke_color,
        stroke_width: stroke_w,
    };
    if is_hover {
        let hot = theme.pin_stroke_hot;
        style.stroke = hot;
        style.stroke_width = 1.125;
        style.fill = [
            (visual.color[0] + hot[0]) * 0.5,
            (visual.color[1] + hot[1]) * 0.5,
            (visual.color[2] + hot[2]) * 0.5,
            visual.color[3],
        ];
    }
    out.push(ShapeInstance {
        shape: pin_shape_to_sdf(visual.shape, center, PIN_R),
        style,
        layer: PickLayer::Pin,
        pick: PickTarget::Pin { node, side, index },
        hollow_border: None,
        pickable: true,
        pick_id: 0,
        depth_bias: shape_layer_depth(depth_base, PickLayer::Pin, layer_stride),
        rotation: 0.0,
        pivot: [0.0, 0.0],
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A node's whole layer stack (body→title→pin→outline + outline bump) must nest strictly
    /// inside ONE node depth slot at ANY node count — otherwise a far node's chrome
    /// z-interleaves over a nearer overlapping node's body. Also the total span must stay
    /// within the clip-Z budget so nothing gets depth-clipped.
    #[test]
    fn intra_node_layers_fit_one_slot_at_any_n() {
        let max_layer = (PickLayer::Pin as u32).max(PickLayer::Outline as u32) as f32;
        for n in [1usize, 100, 667, 1024, 4096, 100_000] {
            let node_stride = NODE_DEPTH_STRIDE.min(DEPTH_SPAN_BUDGET / n as f32);
            let layer_stride = node_stride / DEPTH_LAYER_SLOTS;
            let outline_bias = layer_stride * 0.25;
            let max_offset = max_layer * layer_stride + outline_bias;
            assert!(
                max_offset < node_stride,
                "n={n}: layer stack {max_offset} not < node slot {node_stride}"
            );
            let total = (n as f32 - 1.0) * node_stride + max_offset;
            assert!(
                total <= DEPTH_SPAN_BUDGET + 1e-3,
                "n={n}: total depth {total} > budget"
            );
        }
    }
}
