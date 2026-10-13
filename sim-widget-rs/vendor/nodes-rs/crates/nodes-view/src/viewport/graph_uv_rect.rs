//! Axis-aligned rectangles in graph UV — building block for [`super::visible_graph_uv_rect`].

use glam::Vec2;

/// Axis-aligned bounds in graph `(u, v)` space.
///
/// Graph UV matches the node editor layout: **+u** right, **+v** down (see [`crate::GraphPlane`]).
/// `min` / `max` are inclusive corners of the visible region on that plane.
///
/// # Why not `(Vec2, Vec2)` everywhere?
///
/// This type carries helpers (`pad`, `union`, `is_degenerate`) so grid, minimap, and future
/// culling share the same semantics without copy-pasting margin and empty checks.
///
/// # Examples
///
/// ```
/// use glam::Vec2;
/// use nodes_view::GraphUvRect;
///
/// let on_screen = GraphUvRect {
///     min: Vec2::new(-100.0, -50.0),
///     max: Vec2::new(400.0, 300.0),
/// };
/// let for_grid = on_screen.pad(20.0);
/// assert!(for_grid.min.x < on_screen.min.x);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphUvRect {
    /// Minimum `(u, v)` corner (top-left of the bounds in graph space).
    pub min: Vec2,
    /// Maximum `(u, v)` corner (bottom-right of the bounds in graph space).
    pub max: Vec2,
}

impl GraphUvRect {
    /// Sentinel `min` for [`Self::empty`].
    pub const EMPTY_MIN: f32 = f32::MAX;
    /// Sentinel `max` for [`Self::empty`].
    pub const EMPTY_MAX: f32 = f32::MIN;

    /// Empty rect (no [`Self::expand`] yet); [`Self::is_empty`] is true.
    pub fn empty() -> Self {
        Self {
            min: Vec2::splat(Self::EMPTY_MIN),
            max: Vec2::splat(Self::EMPTY_MAX),
        }
    }

    /// Bounds from center and full size (width = `size.x`, height = `size.y`).
    pub fn from_center_size(center: Vec2, size: Vec2) -> Self {
        let half = size * 0.5;
        Self {
            min: center - half,
            max: center + half,
        }
    }

    /// True if never expanded or invalid (`min > max` on any axis).
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }

    /// True when either axis span is below `min_span` (too small for grid or fit).
    pub fn is_degenerate(&self, min_span: f32) -> bool {
        if self.is_empty() {
            return true;
        }
        let s = self.max - self.min;
        s.x < min_span || s.y < min_span
    }

    /// Center `(u, v)` of the rectangle.
    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    /// Width and height in graph units (`max - min`, or zero if empty).
    pub fn size(&self) -> Vec2 {
        if self.is_empty() {
            Vec2::ZERO
        } else {
            self.max - self.min
        }
    }

    /// Grow the bounds to include `uv` (ignores non-finite points).
    pub fn expand(&mut self, uv: Vec2) {
        if !uv.x.is_finite() || !uv.y.is_finite() {
            return;
        }
        if self.is_empty() {
            self.min = uv;
            self.max = uv;
            return;
        }
        self.min = self.min.min(uv);
        self.max = self.max.max(uv);
    }

    /// Outset all sides by `margin` graph units (used by the grid for one extra minor step).
    pub fn pad(self, margin: f32) -> Self {
        if self.is_empty() {
            return self;
        }
        let m = Vec2::splat(margin.max(0.0));
        Self {
            min: self.min - m,
            max: self.max + m,
        }
    }

    /// True if the axis-aligned box `[min, max]` overlaps this rect (graph UV).
    pub fn intersects_aabb(self, min: Vec2, max: Vec2) -> bool {
        if self.is_empty() {
            return true;
        }
        !(max.x < self.min.x || min.x > self.max.x || max.y < self.min.y || min.y > self.max.y)
    }

    /// Smallest axis-aligned rect containing both inputs.
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersects_aabb_partial_overlap() {
        let vis = GraphUvRect {
            min: Vec2::new(0.0, 0.0),
            max: Vec2::new(100.0, 100.0),
        };
        assert!(vis.intersects_aabb(Vec2::new(50.0, 50.0), Vec2::new(150.0, 150.0)));
        assert!(!vis.intersects_aabb(Vec2::new(200.0, 200.0), Vec2::new(300.0, 300.0)));
    }

    #[test]
    fn intersects_aabb_empty_visible_includes_all() {
        let vis = GraphUvRect::empty();
        assert!(vis.intersects_aabb(Vec2::ZERO, Vec2::ONE));
    }
}
