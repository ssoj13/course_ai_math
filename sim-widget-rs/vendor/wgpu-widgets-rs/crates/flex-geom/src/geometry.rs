//! Placement specs: absolute, relative, and anchor+offset per axis.

use glam::Vec2;

use super::algo::{Rect, SizeHint};
use super::spacing::Margins;

/// Alignment along one axis (start = min edge in surface-local coords, Y-down).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    /// Fill the parent span on this axis (used as cross-axis in flex).
    Stretch,
}

/// Position along one axis inside the parent content rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AxisPos {
    /// Pixels from the parent content **min** edge.
    Abs(f32),
    /// Fraction `0..1` along the parent span (position of the child's min edge).
    Rel(f32),
    /// Align the child to start/center/end of the parent, then apply a pixel offset.
    Anchor { align: Align, offset: f32 },
}

impl Default for AxisPos {
    fn default() -> Self {
        Self::Abs(0.0)
    }
}

/// Size along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AxisSize {
    Fixed(f32),
    /// Use the child's measured preferred size (clamped to parent).
    #[default]
    Content,
    /// Share remaining space along the flex main axis (`weight` must be > 0).
    Flex(f32),
}

/// Full placement of a child inside a parent (Stack / manual layout).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Geometry {
    pub x: AxisPos,
    pub y: AxisPos,
    pub w: AxisSize,
    pub h: AxisSize,
}

impl Geometry {
    pub fn abs(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            x: AxisPos::Abs(x),
            y: AxisPos::Abs(y),
            w: AxisSize::Fixed(w),
            h: AxisSize::Fixed(h),
        }
    }

    pub fn fill() -> Self {
        Self {
            x: AxisPos::Abs(0.0),
            y: AxisPos::Abs(0.0),
            w: AxisSize::Flex(1.0),
            h: AxisSize::Flex(1.0),
        }
    }
}

/// Insets applied inside a parent rect before laying out children.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Padding {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Padding {
    pub const ZERO: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    pub fn all(v: f32) -> Self {
        Self {
            left: v,
            top: v,
            right: v,
            bottom: v,
        }
    }

    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            top: vertical,
            right: horizontal,
            bottom: vertical,
        }
    }

    pub fn horizontal(v: f32) -> Self {
        Self::symmetric(v, 0.0)
    }

    pub fn vertical(v: f32) -> Self {
        Self::symmetric(0.0, v)
    }

    /// Builder: replace the left inset, keep the rest.
    pub fn with_left(mut self, v: f32) -> Self {
        self.left = v;
        self
    }

    pub fn with_top(mut self, v: f32) -> Self {
        self.top = v;
        self
    }

    pub fn with_right(mut self, v: f32) -> Self {
        self.right = v;
        self
    }

    pub fn with_bottom(mut self, v: f32) -> Self {
        self.bottom = v;
        self
    }

    /// Total inset along the X axis (`left + right`).
    pub fn h_span(&self) -> f32 {
        self.left + self.right
    }

    /// Total inset along the Y axis (`top + bottom`).
    pub fn v_span(&self) -> f32 {
        self.top + self.bottom
    }

    pub fn shrink(self, parent: Rect) -> Rect {
        Rect::new(
            parent.x + self.left,
            parent.y + self.top,
            (parent.w - self.left - self.right).max(0.0),
            (parent.h - self.top - self.bottom).max(0.0),
        )
    }
}

/// One child in a flex row/column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlexItem {
    pub main: AxisSize,
    pub cross: AxisSize,
    /// Empty space around the child slot (Qt item margins).
    pub margins: Margins,
}

impl FlexItem {
    pub fn fixed(main: f32, cross: f32) -> Self {
        Self {
            main: AxisSize::Fixed(main),
            cross: AxisSize::Fixed(cross),
            margins: Margins::ZERO,
        }
    }

    pub fn flex_main(weight: f32, cross: f32) -> Self {
        Self {
            main: AxisSize::Flex(weight),
            cross: AxisSize::Fixed(cross),
            margins: Margins::ZERO,
        }
    }

    pub fn flex_both(weight: f32) -> Self {
        Self {
            main: AxisSize::Flex(weight),
            cross: AxisSize::Flex(1.0),
            margins: Margins::ZERO,
        }
    }

    pub fn with_margins(mut self, margins: Margins) -> Self {
        self.margins = margins;
        self
    }

    /// Extra main-axis space before this child (HBox: left, VBox: top).
    pub fn with_main_start(mut self, margin: f32, main_axis: usize) -> Self {
        if main_axis == 0 {
            self.margins.left = margin;
        } else {
            self.margins.top = margin;
        }
        self
    }

    /// Back-compat alias for [`Self::with_main_start`] on horizontal boxes.
    pub fn with_margin_before(self, margin: f32) -> Self {
        self.with_main_start(margin, 0)
    }

    /// Main axis from child [`SizeHint::preferred`]; fixed cross span.
    pub fn content_cross(cross: f32) -> Self {
        Self {
            main: AxisSize::Content,
            cross: AxisSize::Fixed(cross),
            margins: Margins::ZERO,
        }
    }
}

/// Clamp `hint` into `max` when present.
pub fn clamp_hint(hint: SizeHint, max: Vec2) -> SizeHint {
    SizeHint {
        min: hint.min.min(max),
        preferred: hint.preferred.min(max),
        max: hint.max.map(|m| m.min(max)),
    }
}
