//! HBox / VBox measure and arrange (main axis + cross alignment + per-child margins).

use glam::Vec2;

use super::algo::{Rect, SizeHint};
use super::geometry::{Align, AxisSize, FlexItem, Padding};
use super::resolve::{align_cross, resolve_axis_size_hint};
use super::spacing::Margins;

#[derive(Debug, Clone, Copy)]
pub struct FlexChildSpec {
    pub item: FlexItem,
    pub hint: SizeHint,
}

pub fn measure_hbox(
    constraint: Vec2,
    padding: Padding,
    gap: f32,
    children: &[FlexChildSpec],
) -> SizeHint {
    measure_flex(constraint, padding, gap, 0, children)
}

pub fn arrange_hbox(
    parent: Rect,
    padding: Padding,
    gap: f32,
    cross_align: Align,
    children: &[FlexChildSpec],
) -> Vec<Rect> {
    arrange_flex(parent, padding, gap, 0, cross_align, children)
}

pub fn measure_vbox(
    constraint: Vec2,
    padding: Padding,
    gap: f32,
    children: &[FlexChildSpec],
) -> SizeHint {
    measure_flex(constraint, padding, gap, 1, children)
}

pub fn arrange_vbox(
    parent: Rect,
    padding: Padding,
    gap: f32,
    cross_align: Align,
    children: &[FlexChildSpec],
) -> Vec<Rect> {
    arrange_flex(parent, padding, gap, 1, cross_align, children)
}

fn measure_flex(
    constraint: Vec2,
    padding: Padding,
    gap: f32,
    main_axis: usize,
    children: &[FlexChildSpec],
) -> SizeHint {
    let inner_w = (constraint.x - padding.left - padding.right).max(0.0);
    let inner_h = (constraint.y - padding.top - padding.bottom).max(0.0);
    let mut main_sum = 0.0f32;
    let mut cross_max = 0.0f32;
    let mut flex_weight = 0.0f32;

    for (i, ch) in children.iter().enumerate() {
        if i > 0 {
            main_sum += gap;
        }
        let (mb, ma) = ch.item.margins.main_axis(main_axis);
        main_sum += mb;
        let main = main_size(ch, inner_w, inner_h, main_axis, 0.0, 0.0);
        let cross = cross_size(ch, inner_w, inner_h, main_axis, 0.0, 0.0);
        main_sum += main + ma;
        cross_max = cross_max.max(cross + ch.item.margins.cross_span(main_axis));
        if matches!(ch.item.main, AxisSize::Flex(w) if w > 0.0) {
            flex_weight += flex_weight_for(ch.item.main);
        }
    }

    let preferred = if main_axis == 0 {
        Vec2::new(
            padding.left + padding.right + main_sum,
            padding.top + padding.bottom + cross_max,
        )
    } else {
        Vec2::new(
            padding.left + padding.right + cross_max,
            padding.top + padding.bottom + main_sum,
        )
    };
    SizeHint {
        min: preferred,
        preferred,
        max: if flex_weight > 0.0 {
            None
        } else {
            Some(preferred)
        },
    }
}

/// Outer slot rects per child (includes margins; widgets receive inner rect via tree layout).
fn arrange_flex(
    parent: Rect,
    padding: Padding,
    gap: f32,
    main_axis: usize,
    cross_align: Align,
    children: &[FlexChildSpec],
) -> Vec<Rect> {
    let content = padding.shrink(parent);
    let n = children.len();
    if n == 0 {
        return vec![];
    }

    let mut fixed_main = 0.0f32;
    let mut flex_total = 0.0f32;
    for (i, ch) in children.iter().enumerate() {
        if i > 0 {
            fixed_main += gap;
        }
        let (mb, ma) = ch.item.margins.main_axis(main_axis);
        fixed_main += mb;
        if let AxisSize::Flex(w) = ch.item.main
            && w > 0.0
        {
            flex_total += w;
            fixed_main += ma;
            continue;
        }
        fixed_main += main_size(ch, content.w, content.h, main_axis, 0.0, 0.0) + ma;
    }

    let free_main = if main_axis == 0 {
        (content.w - fixed_main).max(0.0)
    } else {
        (content.h - fixed_main).max(0.0)
    };

    let mut main_pos = if main_axis == 0 {
        content.x
    } else {
        content.y
    };
    let mut out = Vec::with_capacity(n);

    for (i, ch) in children.iter().enumerate() {
        if i > 0 {
            main_pos += gap;
        }
        let (mb, ma) = ch.item.margins.main_axis(main_axis);
        main_pos += mb;

        let main = main_size(ch, content.w, content.h, main_axis, free_main, flex_total);
        let cross = cross_size(ch, content.w, content.h, main_axis, free_main, flex_total);
        let cross_span = match ch.item.cross {
            AxisSize::Flex(_) => {
                if main_axis == 0 {
                    content.h
                } else {
                    content.w
                }
            }
            _ => cross,
        };
        let outer_cross = cross_span + ch.item.margins.cross_span(main_axis);

        let (x, y, w, h) = if main_axis == 0 {
            let y_outer = match cross_align {
                Align::Stretch => content.y,
                a => align_cross(a, content, 1, outer_cross),
            };
            let h_outer = match cross_align {
                Align::Stretch => content.h,
                _ => outer_cross,
            };
            (main_pos, y_outer, main, h_outer)
        } else {
            let x_outer = match cross_align {
                Align::Stretch => content.x,
                a => align_cross(a, content, 0, outer_cross),
            };
            let w_outer = match cross_align {
                Align::Stretch => content.w,
                _ => outer_cross,
            };
            (x_outer, main_pos, w_outer, main)
        };

        out.push(Rect::new(x, y, w.max(0.0), h.max(0.0)));
        main_pos += main + ma;
    }
    out
}

/// Widget rect inside an outer slot (main-axis margins are applied in cursor advance).
pub fn flex_child_content_rect(outer: Rect, margins: Margins, main_axis: usize) -> Rect {
    let (cb, ca) = margins.cross_axis(main_axis);
    Rect::new(
        outer.x,
        outer.y + cb,
        outer.w.max(0.0),
        (outer.h - cb - ca).max(0.0),
    )
}

fn flex_weight_for(main: AxisSize) -> f32 {
    match main {
        AxisSize::Flex(w) => w,
        _ => 0.0,
    }
}

fn main_size(
    ch: &FlexChildSpec,
    inner_w: f32,
    inner_h: f32,
    main_axis: usize,
    flex_free: f32,
    flex_total: f32,
) -> f32 {
    let parent_span = if main_axis == 0 { inner_w } else { inner_h };
    resolve_axis_size_hint(
        ch.item.main,
        &ch.hint,
        main_axis,
        parent_span,
        flex_free,
        flex_weight_for(ch.item.main),
        flex_total,
    )
}

fn cross_size(
    ch: &FlexChildSpec,
    inner_w: f32,
    inner_h: f32,
    main_axis: usize,
    flex_free: f32,
    flex_total: f32,
) -> f32 {
    let cross_axis = 1 - main_axis;
    let parent_span = if cross_axis == 0 { inner_w } else { inner_h };
    resolve_axis_size_hint(
        ch.item.cross,
        &ch.hint,
        cross_axis,
        parent_span,
        flex_free,
        flex_weight_for(ch.item.cross),
        flex_total,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::FlexItem;

    #[test]
    fn margins_expand_main_sum() {
        let item = FlexItem::fixed(10.0, 10.0).with_margins(Margins::symmetric(4.0, 0.0));
        let spec = FlexChildSpec {
            item,
            hint: SizeHint::fixed(10.0, 10.0),
        };
        let hint = measure_hbox(Vec2::new(200.0, 24.0), Padding::ZERO, 0.0, &[spec]);
        assert!((hint.preferred.x - 18.0).abs() < 1e-4);
    }
}
