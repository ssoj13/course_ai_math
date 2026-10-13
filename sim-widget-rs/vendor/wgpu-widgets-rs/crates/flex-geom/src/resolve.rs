//! Resolve [`Geometry`](super::geometry::Geometry) and axis rules into pixel [`Rect`](super::algo::Rect).

use super::algo::{Rect, SizeHint};
use super::geometry::{Align, AxisPos, AxisSize, Geometry};

pub fn resolve_axis_pos(pos: AxisPos, parent_min: f32, parent_span: f32, child_span: f32) -> f32 {
    match pos {
        AxisPos::Abs(v) => parent_min + v,
        AxisPos::Rel(t) => parent_min + t * parent_span,
        AxisPos::Anchor { align, offset } => {
            let base = match align {
                Align::Start | Align::Stretch => parent_min,
                Align::Center => parent_min + (parent_span - child_span) * 0.5,
                Align::End => parent_min + parent_span - child_span,
            };
            base + offset
        }
    }
}

pub fn resolve_axis_size(
    size: AxisSize,
    hint: SizeHint,
    parent_span: f32,
    flex_free: f32,
    _flex_weight: f32,
    flex_total: f32,
) -> f32 {
    match size {
        AxisSize::Fixed(v) => v,
        AxisSize::Content => hint.preferred.x.min(parent_span).max(hint.min.x),
        AxisSize::Flex(weight) if flex_total > 0.0 && weight > 0.0 => {
            flex_free * (weight / flex_total)
        }
        AxisSize::Flex(_) => hint.preferred.x.min(parent_span),
    }
}

/// Resolve `w` using `hint.preferred.x` / `hint.min.x`; pass `hint` as-is for both axes
/// by using separate hints if needed (call twice with swapped preferred in flex).
pub fn resolve_axis_size_hint(
    size: AxisSize,
    hint: &SizeHint,
    axis: usize,
    parent_span: f32,
    flex_free: f32,
    _flex_weight: f32,
    flex_total: f32,
) -> f32 {
    let (pref, min) = if axis == 0 {
        (hint.preferred.x, hint.min.x)
    } else {
        (hint.preferred.y, hint.min.y)
    };
    match size {
        AxisSize::Fixed(v) => v,
        AxisSize::Content => pref.min(parent_span).max(min),
        AxisSize::Flex(weight) if flex_total > 0.0 && weight > 0.0 => {
            flex_free * (weight / flex_total)
        }
        // Outside a flex distribution (e.g. `Geometry::fill()` in a Stack) `Flex` means
        // "fill the parent", not "use preferred size" -- a zero-preferred child like a
        // background `Panel` would otherwise collapse to 0 and never paint.
        AxisSize::Flex(_) => parent_span.max(min),
    }
}

pub fn resolve_geometry(parent: Rect, geom: Geometry, hint: SizeHint) -> Rect {
    let w = resolve_axis_size_hint(geom.w, &hint, 0, parent.w, 0.0, 0.0, 0.0);
    let h = resolve_axis_size_hint(geom.h, &hint, 1, parent.h, 0.0, 0.0, 0.0);
    let x = resolve_axis_pos(geom.x, parent.x, parent.w, w);
    let y = resolve_axis_pos(geom.y, parent.y, parent.h, h);
    Rect::new(x, y, w.max(0.0), h.max(0.0))
}

pub fn align_cross(
    align: Align,
    parent: Rect,
    cross_axis: usize,
    child_span: f32,
) -> f32 {
    let (min, span) = if cross_axis == 1 {
        (parent.y, parent.h)
    } else {
        (parent.x, parent.w)
    };
    match align {
        Align::Start => min,
        Align::Center => min + (span - child_span) * 0.5,
        Align::End => min + span - child_span,
        Align::Stretch => min,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Geometry;

    #[test]
    fn abs_geometry_top_left() {
        let parent = Rect::new(10.0, 20.0, 100.0, 50.0);
        let geom = Geometry::abs(5.0, 6.0, 30.0, 18.0);
        let hint = SizeHint::fixed(30.0, 18.0);
        let r = resolve_geometry(parent, geom, hint);
        assert!((r.x - 15.0).abs() < 1e-4);
        assert!((r.y - 26.0).abs() < 1e-4);
        assert!((r.w - 30.0).abs() < 1e-4);
    }

    #[test]
    fn anchor_center() {
        let parent = Rect::new(0.0, 0.0, 100.0, 24.0);
        let geom = Geometry {
            x: AxisPos::Anchor {
                align: Align::Center,
                offset: 0.0,
            },
            y: AxisPos::Anchor {
                align: Align::Center,
                offset: 0.0,
            },
            w: AxisSize::Fixed(40.0),
            h: AxisSize::Fixed(18.0),
        };
        let r = resolve_geometry(parent, geom, SizeHint::fixed(40.0, 18.0));
        assert!((r.x - 30.0).abs() < 1e-4);
        assert!((r.y - 3.0).abs() < 1e-4);
    }
}
