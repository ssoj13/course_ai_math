//! Single source of truth for node chrome geometry (title, body, pins).
//!
//! Constants here (`TITLE_H`, `PORT_H`, `PIN_R`, `GRID_STEP`, …) are reused by
//! [`crate::layout`] (positions), `nodes-view` (SDF + grid), and tests.
//! Change pin placement or default sizes **here** — do not duplicate in renderers.
//!
//! ## Port row geometry (pin + label + inline widget)
//!
//! Before 2026-07-05 the port-name label (nodes-view's `text_labels.rs`) and the
//! inline editable widget (nodes-editor's `node_widgets.rs`, via this module's
//! now-removed `inline_input_anchor_uv`) each hand-derived their own x/y offset
//! from the pin, independently, in two different crates. They silently
//! disagreed: the widget's fixed offset started drawing UNDER any label longer
//! than ~2-3 characters. [`port_row_geometry`] is the single authoritative fix —
//! built on `flex-geom`'s pure `arrange_hbox`/`arrange_vbox` (the same primitives
//! `wgpu-widgets` uses for widget layout, extracted to a GPU-free crate so
//! `nodes-core` can use them too) — both the label renderer and the widget
//! renderer call this ONE function and can no longer drift apart.
//!
//! [`PortRowParams`] bundles the tunable knobs (label column width, gaps, widget
//! size) as named, defaultable fields instead of scattered magic constants, and
//! [`PinEdgeLayout`] (see below) makes horizontal vs. vertical node orientation a
//! real, resolvable parameter that [`port_row_geometry`] itself branches on —
//! rather than something only the pin-position math knew about.

/// Which vertical edge of a node carries a port (inputs left, outputs right).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PinSide {
    Input,
    Output,
}

/// Pin rails on the node border: horizontal (in left / out right) or vertical (in top / out bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PinEdgeLayout {
    Horizontal,
    Vertical,
}

/// Blender-style grid step (matches nodes-view minor grid).
pub const GRID_STEP: f32 = 20.0;

pub const FRAME_NODE_TYPE: &str = "Frame";
/// Sticky-note annotation node: a resizable board comment carrying free multi-line text
/// (`Node::note_text`). Like [`FRAME_NODE_TYPE`] it has no sockets and a user-resizable box, but it
/// draws its text and is solid (opaque, fully pickable) instead of a hollow backdrop.
pub const NOTE_NODE_TYPE: &str = "Note";

/// Default node width (height follows [`NODE_ASPECT`] = 16:9).
pub const DEFAULT_NODE_W: f32 = 320.0;
pub const MIN_NODE_W: f32 = 160.0;
pub const MAX_NODE_W: f32 = 640.0;

/// Width / height for normal nodes (16:9 body).
pub const NODE_ASPECT: f32 = 16.0 / 9.0;

pub const DEFAULT_FRAME_W: f32 = 400.0;
pub const DEFAULT_FRAME_H: f32 = 280.0;
/// Default sticky-note box (smaller than a Frame — a note wraps a few lines of text).
pub const DEFAULT_NOTE_W: f32 = 260.0;
pub const DEFAULT_NOTE_H: f32 = 140.0;
pub const MIN_NODE_H: f32 = 80.0;
pub const MAX_NODE_H: f32 = 1200.0;

pub const TITLE_H: f32 = 24.0;
/// Compact subtitle strip under the title bar (minimal view mode).
pub const MINIMAL_SUMMARY_H: f32 = 16.0;
pub const PORT_H: f32 = 22.0;
/// Height reserved for the Full-mode processing-info line (dirty/cooked status).
pub const PROCESSING_INFO_H: f32 = PORT_H;
pub const PAD: f32 = 8.0;

/// Pin radius in graph UV (renderer uses the same value).
pub const PIN_R: f32 = 7.0;

/// Inline attribute control to the right of an input pin (Full view).
pub const PORT_INLINE_W: f32 = 72.0;
pub const PORT_INLINE_H: f32 = 18.0;
/// Extra width added to a `String` field declaring `UiHint::FilePath` for its browse button. The
/// `FilePathEdit` widget draws the button in this right-edge zone; SHARED by the widget surface size
/// (`nodes-editor::node_widgets`) AND the GPU pick rect ([`inline_field_rects`]) so the clickable
/// button region can never drift from the field's only hit-test rect.
pub const FILEPATH_FIELD_EXTRA_W: f32 = 24.0;
/// Gap between the label column and the inline widget.
pub const PORT_INLINE_GAP: f32 = 4.0;
/// Gap between a pin's edge and the start of its port-name label column.
pub const PIN_LABEL_GAP: f32 = 3.0;
/// Width reserved for a port-name label column next to a pin. Sized generously
/// for real port names in this codebase (`slider`, `combo`, `radio`, ... all
/// comfortably under this) — see [`PortRowParams::label_w`] to override per row.
pub const PORT_LABEL_W: f32 = 64.0;

/// Tunable knobs for one port row's label/widget layout, bundled as data instead
/// of scattered magic constants — a future node style can override any field.
/// `Default` matches the historical fixed constants exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PortRowParams {
    /// Width of the port-name label column.
    pub label_w: f32,
    /// Gap between the pin edge and the label column.
    pub pin_label_gap: f32,
    /// Gap between the label column and the inline widget.
    pub label_widget_gap: f32,
    /// Inline widget `[w, h]` (Int/Float/String kinds; Bool uses `TITLE_FLAG_SIZE`
    /// in `nodes-editor`, unaffected by this).
    pub widget_size: [f32; 2],
}

impl Default for PortRowParams {
    fn default() -> Self {
        Self {
            label_w: PORT_LABEL_W,
            pin_label_gap: PIN_LABEL_GAP,
            label_widget_gap: PORT_INLINE_GAP,
            widget_size: [PORT_INLINE_W, PORT_INLINE_H],
        }
    }
}

/// Re-exported so callers can NAME the rect type this module speaks in — both the
/// [`port_row_geometry`] node-bounds argument and the [`PortRowRects`] fields are
/// `flex_geom::Rect`, but `flex-geom` is an internal dependency of this crate.
pub use flex_geom::Rect;

/// Resolved rects for one port row: the label column and (input rows only, when
/// requested) the inline widget slot. `label.right() <= widget.left()` always
/// holds by construction (they're arranged in the same flex pass, never derived
/// independently) — see the module doc.
#[derive(Debug, Clone, Copy)]
pub struct PortRowRects {
    pub label: flex_geom::Rect,
    pub widget: Option<flex_geom::Rect>,
}

/// Compute a port row's label/widget rects from the pin's world position and the
/// node's bounds, via `flex_geom::arrange_hbox`/`arrange_vbox`. Outputs never get
/// an inline widget (they're computed, not user-editable) regardless of
/// `needs_widget` — only inputs do.
pub fn port_row_geometry(
    node: Rect,
    side: PinSide,
    pin_world: [f32; 2],
    orientation: PinEdgeLayout,
    params: &PortRowParams,
    needs_widget: bool,
) -> PortRowRects {
    use flex_geom::{
        Align, FlexChildSpec, FlexItem, Padding, Rect, SizeHint, arrange_hbox, arrange_vbox,
    };

    // The node's bounding box; the four scalars used to arrive as separate positional arguments,
    // which made a transposed `w`/`h` at a call site silently plausible.
    let (px, py, w, h) = (node.x, node.y, node.w, node.h);

    let needs_widget = needs_widget && side == PinSide::Input;
    let spacer = |main: f32, cross: f32| FlexChildSpec {
        item: FlexItem::fixed(main, cross),
        hint: SizeHint::fixed(main, cross),
    };
    let flex_spacer = || FlexChildSpec {
        item: FlexItem::flex_both(1.0),
        hint: SizeHint::at_least(0.0, 0.0),
    };

    match orientation {
        PinEdgeLayout::Horizontal => {
            let row_h = PORT_H;
            let row_y = pin_world[1] - row_h * 0.5;
            // Input rows read left-to-right starting just past the pin; output rows
            // are right-aligned, ending just before the pin (mirrored via a leading
            // flex spacer that absorbs the leftover space).
            let (parent, mirrored) = match side {
                PinSide::Input => {
                    let x0 = pin_world[0] + PIN_R + params.pin_label_gap;
                    let x1 = px + w - PAD;
                    (Rect::new(x0, row_y, (x1 - x0).max(0.0), row_h), false)
                }
                PinSide::Output => {
                    let x1 = pin_world[0] - PIN_R - params.pin_label_gap;
                    let x0 = px + PAD;
                    (Rect::new(x0, row_y, (x1 - x0).max(0.0), row_h), true)
                }
            };

            let mut children = Vec::with_capacity(3);
            if mirrored {
                children.push(flex_spacer());
                children.push(spacer(params.label_w, row_h));
            } else {
                children.push(spacer(params.label_w, row_h));
                if needs_widget {
                    children.push(spacer(params.label_widget_gap, row_h));
                    children.push(spacer(params.widget_size[0], params.widget_size[1]));
                }
            }

            let rects = arrange_hbox(parent, Padding::ZERO, 0.0, Align::Center, &children);
            PortRowRects {
                label: if mirrored { rects[1] } else { rects[0] },
                widget: if needs_widget && !mirrored {
                    Some(rects[2])
                } else {
                    None
                },
            }
        }
        PinEdgeLayout::Vertical => {
            // Same construction, transposed: main axis Y, column centered on the
            // pin's X. Inputs stack downward from the top edge; outputs stack
            // upward from the bottom edge.
            let col_w = params.label_w.max(if needs_widget {
                params.widget_size[0]
            } else {
                0.0
            });
            let col_x = pin_world[0] - col_w * 0.5;
            let label_h = PORT_H * 0.6;

            let (parent, mirrored) = match side {
                PinSide::Input => {
                    let y0 = pin_world[1] + PIN_R + params.pin_label_gap;
                    let y1 = py + h - PAD;
                    (Rect::new(col_x, y0, col_w, (y1 - y0).max(0.0)), false)
                }
                PinSide::Output => {
                    let y1 = pin_world[1] - PIN_R - params.pin_label_gap;
                    let y0 = py + PAD;
                    (Rect::new(col_x, y0, col_w, (y1 - y0).max(0.0)), true)
                }
            };

            let mut children = Vec::with_capacity(3);
            if mirrored {
                children.push(flex_spacer());
                children.push(spacer(col_w, label_h));
            } else {
                children.push(spacer(col_w, label_h));
                if needs_widget {
                    children.push(spacer(col_w, params.label_widget_gap));
                    children.push(spacer(
                        params.widget_size[0].min(col_w),
                        params.widget_size[1],
                    ));
                }
            }

            let rects = arrange_vbox(parent, Padding::ZERO, 0.0, Align::Center, &children);
            PortRowRects {
                label: if mirrored { rects[1] } else { rects[0] },
                widget: if needs_widget && !mirrored {
                    Some(rects[2])
                } else {
                    None
                },
            }
        }
    }
}

/// One inline value-field's placement on a node, in graph-UV space: the input port it edits,
/// its hit/draw rect `[x, y, w, h]`, and whether it is editable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InlineFieldRect {
    /// Input port index on the node.
    pub port: usize,
    /// Field rect in graph-UV: `[x, y, w, h]`.
    pub rect: [f32; 4],
    /// `false` when the input is wire-connected: the field shows a read-only echo of the
    /// upstream value and must NOT be an edit / GPU-pick target (clicking it selects the node).
    pub editable: bool,
}

/// Inline value-field rects for a node's inputs, in graph-UV space.
///
/// SINGLE SOURCE OF TRUTH shared by the editor's widget placement (`nodes-editor::node_widgets`)
/// and the GPU pick quads (`nodes-view::shape_scene`): the field's hit rect can never drift from
/// the value box the user sees, because both derive from this one function. Returns empty for
/// frame nodes and Minimal view mode (no inline fields there). Ports whose stored value has no
/// inline widget kind (Vec/Color/Map/...) are skipped.
pub fn inline_field_rects(
    graph: &crate::Graph,
    node_id: crate::NodeId,
    layout: &crate::LayoutCache,
    registry: &crate::NodeTypeRegistry,
) -> Vec<InlineFieldRect> {
    let Some(node) = graph.nodes.get(&node_id) else {
        return Vec::new();
    };
    if crate::layout::is_backdrop(node) || node.view_mode() == crate::NodeViewMode::Minimal {
        return Vec::new();
    }
    let Some(layout_node) = layout.nodes.get(&node_id) else {
        return Vec::new();
    };
    let px = layout_node.center[0] - layout_node.size[0] * 0.5;
    let py = layout_node.center[1] - layout_node.size[1] * 0.5;
    let w = layout_node.size[0];
    let h = layout_node.size[1];
    let orientation = node.resolved_orientation(w, h);
    let params = PortRowParams::default();

    let mut out = Vec::new();
    for (port, name) in node.input_names().iter().enumerate() {
        // Widget size by stored value kind: Bool is a small square checkbox; scalars/strings a box.
        let Some(value) = node.input_value(name) else {
            continue;
        };
        // A String port declaring `UiHint::FilePath` renders as a `FilePathEdit` (field + browse
        // button), so its widget/pick box is wider by [`FILEPATH_FIELD_EXTRA_W`]. Keeping that width
        // HERE is what makes the browse button clickable: this rect is the GPU pick quad, and the
        // editor sizes the widget surface to the SAME width — one SSOT, no drift (else the button
        // would sit outside the field's only hit-test rect and never fire).
        let is_file_path = matches!(value.kind(), crate::ValueKind::String)
            && registry
                .get(node.type_name())
                .and_then(|d| d.attr_meta(name))
                .and_then(|a| a.ui_hint)
                == Some(crate::UiHint::FilePath);
        let size = match value.kind() {
            crate::ValueKind::Bool => [crate::TITLE_FLAG_SIZE, crate::TITLE_FLAG_SIZE],
            crate::ValueKind::String if is_file_path => {
                [PORT_INLINE_W + FILEPATH_FIELD_EXTRA_W, PORT_INLINE_H]
            }
            crate::ValueKind::I64 | crate::ValueKind::F64 | crate::ValueKind::String => {
                [PORT_INLINE_W, PORT_INLINE_H]
            }
            _ => continue,
        };
        let Some(pin) = layout.pin_positions.get(&(node_id, PinSide::Input, port)) else {
            continue;
        };
        // Same authoritative row layout the label renderer uses; `widget` is the value-box slot.
        let rects = port_row_geometry(
            Rect::new(px, py, w, h),
            PinSide::Input,
            pin.world,
            orientation,
            &params,
            true,
        );
        let Some(widget) = rects.widget else {
            continue;
        };
        out.push(InlineFieldRect {
            port,
            rect: [widget.x, widget.y, size[0], size[1]],
            editable: !crate::pin_is_connected(graph, node_id, PinSide::Input, port),
        });
    }
    out
}

/// Resolve an explicit orientation preference, falling back to aspect-based
/// auto-detection ([`pin_edge_layout`]) when `pref` is `None` ("auto").
#[inline]
pub fn resolve_orientation(pref: Option<PinEdgeLayout>, w: f32, h: f32) -> PinEdgeLayout {
    pref.unwrap_or_else(|| pin_edge_layout(w, h))
}

/// Center-to-center spacing between pins in a grouped rail.
pub const PIN_STEP: f32 = 26.0;

/// Choose horizontal vs vertical pin rails from node aspect.
#[inline]
pub fn pin_edge_layout(w: f32, h: f32) -> PinEdgeLayout {
    if h > w {
        PinEdgeLayout::Vertical
    } else {
        PinEdgeLayout::Horizontal
    }
}

/// Place `count` pin centers in a centered group along an edge segment.
#[inline]
pub fn group_coord_on_edge(edge_start: f32, edge_len: f32, slot: usize, count: usize) -> f32 {
    if count <= 1 {
        return edge_start + edge_len * 0.5;
    }
    let used = PIN_STEP * (count.saturating_sub(1)) as f32;
    let used = used.min(edge_len.max(PIN_STEP));
    let group_start = edge_start + (edge_len - used) * 0.5;
    group_start + slot as f32 * PIN_STEP
}

/// Graph **u** of pin center on the node border (half inside, half outside) — horizontal layout.
#[inline]
pub fn pin_x(side: PinSide, px: f32, w: f32) -> f32 {
    match side {
        PinSide::Input => px,
        PinSide::Output => px + w,
    }
}

/// Graph **v** of pin center on top/bottom border — vertical layout.
#[inline]
pub fn pin_y_edge(side: PinSide, py: f32, h: f32) -> f32 {
    match side {
        PinSide::Input => py,
        PinSide::Output => py + h,
    }
}

/// Evenly space `count` pins between `top` and `bottom` (legacy helper; prefer [`group_coord_on_edge`]).
#[inline]
pub fn distribute_pin_y(top: f32, bottom: f32, slot: usize, count: usize) -> f32 {
    group_coord_on_edge(top, (bottom - top).max(0.0), slot, count)
}

/// Shared vertical rail for body pins (inputs/outputs use the same `port_index`).
///
/// **Top-aligned**: each port owns a `PORT_H`-tall slot directly below the title bar and the
/// pin sits at its slot center, so row 0 is ~`PORT_H/2` (~11px) under the title and rows step
/// down by `PORT_H`. Sockets therefore START just below the title bar instead of being
/// centered in the body (the old behaviour). This matches the node height
/// (`TITLE_H + rows*PORT_H + PAD`) exactly, so pins never overflow, and it keeps input/output
/// of the same index horizontally aligned regardless of how many ports each side has.
#[inline]
pub fn pin_rail_y(py: f32, port_index: usize) -> f32 {
    py + TITLE_H + (port_index as f32 + 0.5) * PORT_H
}

/// World position of a body-side pin (Mid/Full).
///
/// `port_index` is the port number; `row_count` is shared across both sides
/// ([`crate::layout::pin_row_count`]) so input/output sockets line up horizontally.
#[inline]
pub fn body_pin_world(
    px: f32,
    py: f32,
    w: f32,
    h: f32,
    side: PinSide,
    port_index: usize,
    row_count: usize,
) -> [f32; 2] {
    match pin_edge_layout(w, h) {
        PinEdgeLayout::Horizontal => [pin_x(side, px, w), pin_rail_y(py, port_index)],
        PinEdgeLayout::Vertical => {
            let left = px + PAD;
            let right = px + w - PAD;
            let x = group_coord_on_edge(left, right - left, port_index, row_count);
            [x, pin_y_edge(side, py, h)]
        }
    }
}

/// Vertical center of a body-side pin (horizontal layout).
#[inline]
pub fn body_pin_y(py: f32, port_index: usize) -> f32 {
    pin_rail_y(py, port_index)
}

/// World position of pins on the title bar (Minimal) or full node height (vertical Minimal).
#[inline]
pub fn title_pin_world(
    px: f32,
    py: f32,
    w: f32,
    h: f32,
    side: PinSide,
    slot: usize,
    count: usize,
) -> [f32; 2] {
    match pin_edge_layout(w, h) {
        PinEdgeLayout::Horizontal => {
            let top = py + PAD;
            let bottom = py + TITLE_H - PAD;
            let y = group_coord_on_edge(top, bottom - top, slot, count);
            [pin_x(side, px, w), y]
        }
        PinEdgeLayout::Vertical => {
            let left = px + PAD;
            let right = px + w - PAD;
            let x = group_coord_on_edge(left, right - left, slot, count);
            [x, pin_y_edge(side, py, h)]
        }
    }
}

/// Vertical center of pins on the title bar (Minimal, horizontal layout).
#[inline]
pub fn title_pin_y(py: f32, count: usize, slot: usize) -> f32 {
    let top = py + PAD;
    let bottom = py + TITLE_H - PAD;
    group_coord_on_edge(top, bottom - top, slot, count)
}

/// Single merged port on the left/right edge (minimal view); Y matches [`pin_rail_y`] row 0.
#[inline]
pub fn minimal_merged_pin_world(px: f32, py: f32, w: f32, side: PinSide) -> [f32; 2] {
    [pin_x(side, px, w), pin_rail_y(py, 0)]
}

/// Compact node height: title bar + summary strip + padding.
#[inline]
pub fn minimal_node_height() -> f32 {
    TITLE_H + MINIMAL_SUMMARY_H + PAD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_pins_top_aligned_under_title() {
        // Row 0 sits ~PORT_H/2 below the title bar (sockets start just under the title,
        // not centered in the body), and rows step down by exactly PORT_H.
        let py = 100.0;
        let y0 = pin_rail_y(py, 0);
        assert!(
            (y0 - (py + TITLE_H + PORT_H * 0.5)).abs() < 0.001,
            "row 0 should be PORT_H/2 under the title, got {y0}"
        );
        let y1 = pin_rail_y(py, 1);
        assert!((y1 - y0 - PORT_H).abs() < 0.001, "rows step by PORT_H");
    }

    #[test]
    fn input_output_same_index_align() {
        // Horizontal node (w > h): input[i] and output[i] share a y, so wires stay straight
        // even when the two sides have different port counts.
        let (px, py, w, h) = (0.0, 0.0, 200.0, 120.0);
        let i_in = body_pin_world(px, py, w, h, PinSide::Input, 1, 3);
        let i_out = body_pin_world(px, py, w, h, PinSide::Output, 1, 2);
        assert!(
            (i_in[1] - i_out[1]).abs() < 0.001,
            "same index aligns across sides"
        );
    }

    /// The bug this module exists to prevent: the inline widget must never start
    /// before the label column ends, for every row of a realistic node, regardless
    /// of port count. Before the shared `port_row_geometry` pass, the widget's
    /// fixed pin-relative offset (~19px) started well before a label like "slider"
    /// (needing ~52px) finished rendering.
    #[test]
    fn label_and_widget_never_overlap_horizontal() {
        let params = PortRowParams::default();
        let (px, py, w, h) = (0.0, 0.0, DEFAULT_NODE_W, 180.0);
        for row_count in 1..=12usize {
            for port_idx in 0..row_count {
                let pin = body_pin_world(px, py, w, h, PinSide::Input, port_idx, row_count);
                let rects = port_row_geometry(
                    Rect::new(px, py, w, h),
                    PinSide::Input,
                    pin,
                    PinEdgeLayout::Horizontal,
                    &params,
                    true,
                );
                let widget = rects
                    .widget
                    .expect("input row with needs_widget=true must have a widget rect");
                assert!(
                    rects.label.x + rects.label.w <= widget.x + 1e-4,
                    "row {port_idx}/{row_count}: label ends at {}, widget starts at {} — overlap",
                    rects.label.x + rects.label.w,
                    widget.x
                );
            }
        }
    }

    #[test]
    fn output_row_has_no_widget() {
        let params = PortRowParams::default();
        let (px, py, w, h) = (0.0, 0.0, DEFAULT_NODE_W, 180.0);
        let pin = body_pin_world(px, py, w, h, PinSide::Output, 0, 1);
        let rects = port_row_geometry(
            Rect::new(px, py, w, h),
            PinSide::Output,
            pin,
            PinEdgeLayout::Horizontal,
            &params,
            true, // even if requested, outputs never get one
        );
        assert!(
            rects.widget.is_none(),
            "output ports are computed, never user-editable"
        );
        assert!(
            rects.label.x + rects.label.w <= pin[0] - PIN_R + 1e-4,
            "output label must end at/before the pin"
        );
    }

    #[test]
    fn vertical_orientation_label_and_widget_never_overlap() {
        let params = PortRowParams::default();
        // Tall node (h > w) so pin_edge_layout would already pick Vertical on its
        // own; exercised explicitly via `orientation` regardless.
        let (px, py, w, h) = (0.0, 0.0, 160.0, 400.0);
        for row_count in 1..=4usize {
            for port_idx in 0..row_count {
                let pin = body_pin_world(px, py, w, h, PinSide::Input, port_idx, row_count);
                let rects = port_row_geometry(
                    Rect::new(px, py, w, h),
                    PinSide::Input,
                    pin,
                    PinEdgeLayout::Vertical,
                    &params,
                    true,
                );
                let widget = rects
                    .widget
                    .expect("vertical input row must have a widget rect");
                assert!(
                    rects.label.y + rects.label.h <= widget.y + 1e-4,
                    "row {port_idx}/{row_count}: vertical label/widget overlap"
                );
            }
        }
    }

    #[test]
    fn resolve_orientation_falls_back_to_aspect_when_auto() {
        assert_eq!(
            resolve_orientation(None, 320.0, 180.0),
            PinEdgeLayout::Horizontal
        );
        assert_eq!(
            resolve_orientation(None, 160.0, 400.0),
            PinEdgeLayout::Vertical
        );
        // Explicit preference overrides aspect either way.
        assert_eq!(
            resolve_orientation(Some(PinEdgeLayout::Vertical), 320.0, 180.0),
            PinEdgeLayout::Vertical
        );
    }
}
