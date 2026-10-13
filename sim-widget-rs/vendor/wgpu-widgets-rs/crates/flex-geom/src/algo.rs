//! Two-pass Qt-style layout: measure (bottom-up) → arrange (top-down).

use glam::Vec2;

/// Axis-aligned rect in surface-local pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn min(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    pub fn max(&self) -> Vec2 {
        Vec2::new(self.x + self.w, self.y + self.h)
    }

    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.x < self.x + self.w && p.y >= self.y && p.y < self.y + self.h
    }

    /// Returns `[x, y, w, h]` for GPU upload.
    pub fn as_array(&self) -> [f32; 4] {
        [self.x, self.y, self.w, self.h]
    }
}

/// Returned by [`Widget::measure`] to negotiate layout.
#[derive(Debug, Clone, Copy)]
pub struct SizeHint {
    pub min: Vec2,
    pub preferred: Vec2,
    /// `None` = no maximum constraint (widget expands to fill).
    pub max: Option<Vec2>,
}

impl SizeHint {
    pub fn fixed(w: f32, h: f32) -> Self {
        let v = Vec2::new(w, h);
        Self { min: v, preferred: v, max: Some(v) }
    }

    pub fn at_least(w: f32, h: f32) -> Self {
        let v = Vec2::new(w, h);
        Self { min: v, preferred: v, max: None }
    }
}
