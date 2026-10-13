//! Node bounds in graph UV space (`view_pos` top-left, +u right, +v down).
//!
//! [`node_regions`] splits title chrome (view mode, bypass, display flags) for pick layers
//! and `nodes-view::shape_scene` (SDF instance bounds). Built from [`crate::layout::LayoutNode`].

use crate::view_mode::NodeViewMode;

use crate::layout::TITLE_H;

/// Square title-bar widgets: view-mode (left), display + bypass in a row (right).
/// Matches [`wgpu_widgets::Checkbox`] measure size (18×18).
pub const TITLE_FLAG_SIZE: f32 = 18.0;
/// Inset for title-bar chrome from the node edges (shared with editor [`HBox`] padding).
pub const TITLE_FLAG_PAD: f32 = 6.0;
pub const TITLE_FLAG_GAP: f32 = 2.0;
/// Gap between left view-mode square and node title text.
pub const TITLE_LABEL_GAP: f32 = 4.0;

/// Legacy alias (view mode is now a square on the left).
pub const VIEW_MODE_TOGGLE_W: f32 = TITLE_FLAG_SIZE;

/// Axis-aligned rectangle in graph `(u, v)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphRect {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl GraphRect {
    #[inline]
    pub fn from_center_size(center: [f32; 2], size: [f32; 2]) -> Self {
        let hw = size[0] * 0.5;
        let hh = size[1] * 0.5;
        Self {
            min: [center[0] - hw, center[1] - hh],
            max: [center[0] + hw, center[1] + hh],
        }
    }

    #[inline]
    pub fn width(self) -> f32 {
        self.max[0] - self.min[0]
    }

    #[inline]
    pub fn contains_uv(&self, uv: [f32; 2]) -> bool {
        uv[0] >= self.min[0] && uv[0] <= self.max[0] && uv[1] >= self.min[1] && uv[1] <= self.max[1]
    }

    /// Title strip along the top edge (`min[1]` .. `min[1] + TITLE_H`).
    #[inline]
    pub fn title_bar(self) -> Self {
        Self {
            min: self.min,
            max: [self.max[0], self.min[1] + TITLE_H],
        }
    }

    /// Body below the title bar.
    #[inline]
    pub fn body_below_title(self) -> Self {
        Self {
            min: [self.min[0], self.min[1] + TITLE_H],
            max: self.max,
        }
    }

    /// Min/Mid/Full square at the **left** of the title bar (before the label).
    #[inline]
    pub fn view_mode_toggle(self) -> Self {
        flag_square_left(self)
    }

    /// Display and bypass squares on the **right** (`bypass` left of `display`).
    #[inline]
    pub fn title_houdini_flags(self) -> (GraphRect, GraphRect) {
        flag_row_right(self)
    }

    /// Horizontal `[u_min, u_max]` band for the node title label between chrome widgets.
    #[inline]
    pub fn title_label_u_range(
        self,
        view_mode: Option<GraphRect>,
        display: Option<GraphRect>,
        bypass: Option<GraphRect>,
    ) -> [f32; 2] {
        let mut u_min = self.min[0];
        let mut u_max = self.max[0];
        if let Some(vm) = view_mode {
            u_min = vm.max[0] + TITLE_LABEL_GAP;
        }
        if let Some(d) = display {
            u_max = u_max.min(d.min[0] - TITLE_LABEL_GAP);
        }
        if let Some(b) = bypass {
            u_max = u_max.min(b.min[0] - TITLE_LABEL_GAP);
        }
        if u_max < u_min {
            u_max = u_min;
        }
        [u_min, u_max]
    }
}

#[inline]
fn flag_square_left(title: GraphRect) -> GraphRect {
    let y0 = title.min[1] + (TITLE_H - TITLE_FLAG_SIZE) * 0.5;
    GraphRect {
        min: [title.min[0] + TITLE_FLAG_PAD, y0],
        max: [
            title.min[0] + TITLE_FLAG_PAD + TITLE_FLAG_SIZE,
            y0 + TITLE_FLAG_SIZE,
        ],
    }
}

/// Right row: bypass square, then display square flush to the right edge.
#[inline]
fn flag_row_right(title: GraphRect) -> (GraphRect, GraphRect) {
    let y0 = title.min[1] + (TITLE_H - TITLE_FLAG_SIZE) * 0.5;
    let display_x1 = title.max[0] - TITLE_FLAG_PAD;
    let display_x0 = display_x1 - TITLE_FLAG_SIZE;
    let bypass_x1 = display_x0 - TITLE_FLAG_GAP;
    let bypass_x0 = bypass_x1 - TITLE_FLAG_SIZE;
    let display = GraphRect {
        min: [display_x0, y0],
        max: [display_x1, y0 + TITLE_FLAG_SIZE],
    };
    let bypass = GraphRect {
        min: [bypass_x0, y0],
        max: [bypass_x1, y0 + TITLE_FLAG_SIZE],
    };
    (display, bypass)
}

/// Pick/draw regions for one node in graph UV space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeRegions {
    pub full: GraphRect,
    pub title: GraphRect,
    pub body: GraphRect,
    pub view_mode_toggle: Option<GraphRect>,
    pub display_flag: Option<GraphRect>,
    pub bypass_flag: Option<GraphRect>,
}

/// Surface-local `[x, y, w, h]` for a graph UV rect relative to a node top-left origin.
#[inline]
pub fn graph_rect_node_local(r: GraphRect, node_origin: [f32; 2]) -> [f32; 4] {
    [
        r.min[0] - node_origin[0],
        r.min[1] - node_origin[1],
        r.width(),
        r.max[1] - r.min[1],
    ]
}

impl NodeRegions {
    /// `[u_min, u_max]` for drawing the node title between chrome widgets.
    #[inline]
    pub fn title_label_u_range(self) -> [f32; 2] {
        self.title
            .title_label_u_range(self.view_mode_toggle, self.display_flag, self.bypass_flag)
    }
}

/// Layout regions from center/size and view mode (matches renderer + pick).
pub fn node_regions(
    center: [f32; 2],
    size: [f32; 2],
    view_mode: NodeViewMode,
    is_frame: bool,
) -> NodeRegions {
    let full = GraphRect::from_center_size(center, size);
    if is_frame {
        return NodeRegions {
            full,
            title: full,
            body: full,
            view_mode_toggle: None,
            display_flag: None,
            bypass_flag: None,
        };
    }
    let title = full.title_bar();
    let view_mode_toggle = Some(title.view_mode_toggle());
    let (display_flag, bypass_flag) = {
        let (d, b) = title.title_houdini_flags();
        (Some(d), Some(b))
    };
    if view_mode == NodeViewMode::Minimal {
        let body = GraphRect {
            min: [full.min[0], full.min[1] + TITLE_H],
            max: full.max,
        };
        return NodeRegions {
            full,
            title,
            body,
            view_mode_toggle,
            display_flag,
            bypass_flag,
        };
    }
    NodeRegions {
        full,
        title,
        body: full.body_below_title(),
        view_mode_toggle,
        display_flag,
        bypass_flag,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view_mode::NodeViewMode;

    #[test]
    fn title_flags_are_horizontal_bypass_left_of_display() {
        let r = node_regions([100.0, 50.0], [200.0, 120.0], NodeViewMode::Full, false);
        let display = r.display_flag.unwrap();
        let bypass = r.bypass_flag.unwrap();
        assert!(bypass.max[0] <= display.min[0]);
        assert!((display.max[0] - (r.title.max[0] - TITLE_FLAG_PAD)).abs() < 0.01);
        let vm = r.view_mode_toggle.unwrap();
        assert!(vm.max[0] < r.title_label_u_range()[0]);
    }
}
