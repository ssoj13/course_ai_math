//! Per-widget and container spacing (Qt `QMargins` / layout spacing).

use super::algo::Rect;

/// Insets around a flex child (surface-local: left/top/right/bottom).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Margins {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Margins {
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

    pub fn only_left(v: f32) -> Self {
        Self {
            left: v,
            ..Self::ZERO
        }
    }

    pub fn only_right(v: f32) -> Self {
        Self {
            right: v,
            ..Self::ZERO
        }
    }

    pub fn only_top(v: f32) -> Self {
        Self {
            top: v,
            ..Self::ZERO
        }
    }

    pub fn only_bottom(v: f32) -> Self {
        Self {
            bottom: v,
            ..Self::ZERO
        }
    }

    /// Builder: replace the left margin, keep the rest.
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

    /// Shrink `outer` to the content rect inside these margins.
    pub fn shrink(self, outer: Rect) -> Rect {
        Rect::new(
            outer.x + self.left,
            outer.y + self.top,
            (outer.w - self.left - self.right).max(0.0),
            (outer.h - self.top - self.bottom).max(0.0),
        )
    }

    /// Main-axis before/after for flex (`main_axis`: 0 = HBox, 1 = VBox).
    #[inline]
    pub fn main_axis(self, main_axis: usize) -> (f32, f32) {
        if main_axis == 0 {
            (self.left, self.right)
        } else {
            (self.top, self.bottom)
        }
    }

    /// Cross-axis before/after for flex.
    #[inline]
    pub fn cross_axis(self, main_axis: usize) -> (f32, f32) {
        if main_axis == 0 {
            (self.top, self.bottom)
        } else {
            (self.left, self.right)
        }
    }

    pub fn main_span(self, main_axis: usize) -> f32 {
        let (a, b) = self.main_axis(main_axis);
        a + b
    }

    pub fn cross_span(self, main_axis: usize) -> f32 {
        let (a, b) = self.cross_axis(main_axis);
        a + b
    }

    /// `axis = 0` -> horizontal span (`left + right`); `axis = 1` -> vertical (`top + bottom`).
    #[inline]
    pub fn h_span_xy(self, axis: usize) -> f32 {
        if axis == 0 {
            self.left + self.right
        } else {
            self.top + self.bottom
        }
    }
}
