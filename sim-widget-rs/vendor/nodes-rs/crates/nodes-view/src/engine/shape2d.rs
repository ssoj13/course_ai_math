//! Shape types shared by GPU render and CPU pick.

use nodes_core::{NodeId, PinShape, PinSide};

/// Visual primitive evaluated in the shape shader / `shape_sdf`.
#[derive(Debug, Clone)]
pub enum Shape2d {
    RoundedRect {
        min: [f32; 2],
        max: [f32; 2],
        radius: f32,
    },
    Circle {
        center: [f32; 2],
        radius: f32,
    },
    RegularPolygon {
        center: [f32; 2],
        radius: f32,
        sides: u8,
        rotation: f32,
    },
    /// Capsule along `a`→`b` with radius `half_width` in graph UV (SDF, same pass as nodes).
    Segment {
        a: [f32; 2],
        b: [f32; 2],
        half_width: f32,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct ShapeStyle {
    pub fill: [f32; 4],
    pub stroke: [f32; 4],
    pub stroke_width: f32,
}

impl Default for ShapeStyle {
    fn default() -> Self {
        Self {
            fill: [0.15, 0.16, 0.18, 1.0],
            stroke: [0.35, 0.40, 0.50, 1.0],
            stroke_width: 1.5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PickLayer {
    Body = 0,
    Title = 1,
    /// Selection/hover stroke above title; below pins so sockets stay visible.
    Outline = 2,
    /// Inline editable field (value box) sitting on the node body — above title/body so its
    /// object id wins over them in the pick buffer, below pins so socket clicks are never stolen.
    Field = 3,
    Pin = 4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickTarget {
    NodeBody(NodeId),
    NodeTitle(NodeId),
    ViewModeToggle(NodeId),
    BypassFlag(NodeId),
    DisplayFlag(NodeId),
    /// An inline editable input field (value box) on a node, addressed by input port index.
    /// The single GPU-pick classification for text fields: the editor dispatches keyboard/pointer
    /// events to the matching widget by `(node, port)`, so fields need no separate CPU hit-test.
    NodeField {
        node: NodeId,
        port: usize,
    },
    Pin {
        node: NodeId,
        side: PinSide,
        index: usize,
    },
}

#[derive(Debug, Clone)]
pub struct ShapeInstance {
    pub shape: Shape2d,
    pub style: ShapeStyle,
    pub layer: PickLayer,
    pub pick: PickTarget,
    /// When set, body pick hits the border ring only (frame nodes).
    pub hollow_border: Option<f32>,
    /// If false, drawn but omitted from CPU pick (e.g. shared node outline).
    pub pickable: bool,
    /// Object id for GPU picking (`wgpu_pick`). `0` = `ID_NONE` (background/non-pickable).
    /// Assigned from `shape_scene::build_shape_instances` in shape draw order.
    pub pick_id: u32,
    /// Draw-rank offset for stable z-order. Larger = nearer the camera (drawn in front).
    /// Applied in `shape.wgsl` as a reversed-Z NDC bias (`clip.z += depth_bias * clip.w`).
    pub depth_bias: f32,
    /// In-plane rotation (radians) applied to this instance's DRAWN geometry about [`Self::pivot`].
    /// The SDF is still evaluated in the shape's LOCAL (unrotated) frame, so a rounded-rect body
    /// becomes an oriented rounded-rect without changing the SDF. `0` = axis-aligned (common case).
    /// Driven by the live force sim's node-rotation option (`Node::view_rotation`).
    pub rotation: f32,
    /// Pivot (graph-UV) the [`Self::rotation`] turns about — the owning node's center, so every
    /// chrome piece (body/title/field/outline) of one node rotates as a rigid group. Pins are NOT
    /// rotated here (the layout already places them at rotated positions). Ignored when rotation 0.
    pub pivot: [f32; 2],
}

pub fn pin_shape_to_sdf(shape: PinShape, center: [f32; 2], radius: f32) -> Shape2d {
    match shape {
        PinShape::Circle => Shape2d::Circle { center, radius },
        PinShape::Square => Shape2d::RegularPolygon {
            center,
            radius,
            sides: 4,
            rotation: 0.0,
        },
        PinShape::Triangle => Shape2d::RegularPolygon {
            center,
            radius,
            sides: 3,
            rotation: 0.0,
        },
        PinShape::Diamond => Shape2d::RegularPolygon {
            center,
            radius,
            sides: 4,
            rotation: std::f32::consts::FRAC_PI_4,
        },
    }
}
