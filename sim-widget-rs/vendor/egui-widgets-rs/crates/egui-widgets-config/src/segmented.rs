//! Segmented control: N mutually exclusive buttons joined into one strip (like an iOS / macOS
//! segmented control). It is the grid's editor for a short choice list hinted `segmented`, and a
//! standalone widget (tab strips, on / off / all pickers).
//!
//! Hand-painted so it reads as ONE control (shared rounded frame, hairline dividers, the selected
//! segment filled with the selection colour). Segments are EQUAL width and fill the available
//! width, so the strip ends at the same edge as the sliders above and below it. The strip is as
//! high as the style's `interact_size.y` (what [`crate::AttrMetrics::apply`] sets, so compact metrics
//! compact the strip, and the egui default 18 px standalone) and uses the style's font, so a host that compacts its
//! controls ([`crate::AttrMetrics::apply`]) compacts the strip too.
//!
//! Labels are never truncated: when the widest label does not fit its equal segment at the
//! minimum padding, the same control is drawn as a dropdown instead (same height, same value
//! semantics, the combo box's own AccessKit node).

use egui::text::LayoutJob;
use egui::{ComboBox, Rangef, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType};
use std::borrow::Cow;

/// Corner radius (px) of the strip; the selected segment's is one less so it sits inside.
const ROUND: f32 = 4.0;
/// Narrowest strip (px) when the available width is unbounded; a bounded one never overflows its
/// cell: it becomes a dropdown instead.
const MIN_WIDTH: f32 = 60.0;
/// Least horizontal padding (px) on each side of a label inside its segment; a strip whose
/// segments cannot give every label this much becomes a dropdown.
const PAD_X: f32 = 6.0;

/// A segmented control over `items` (label, value); exactly one is selected. Returns `true`
/// when the selection changed this frame.
pub fn segmented<T: PartialEq + Clone>(ui: &mut Ui, value: &mut T, items: &[(&str, T)]) -> bool {
    segmented_tips(ui, value, items, &[])
}

/// [`segmented`] with a per-segment hover tooltip: `tips[i]` is shown when the pointer hovers
/// segment `i` (empty or missing: none). Short labels can carry a full explanation on hover.
pub fn segmented_tips<T: PartialEq + Clone>(
    ui: &mut Ui,
    value: &mut T,
    items: &[(&str, T)],
    tips: &[&str],
) -> bool {
    let selected = items.iter().position(|(_, v)| v == value);
    let clicked = segmented_pick(
        ui,
        items.len(),
        selected,
        |i| Cow::Borrowed(items[i].0),
        |i| tips.get(i).copied().unwrap_or(""),
    );
    match clicked {
        Some(i) => {
            *value = items[i].1.clone();
            true
        }
        None => false,
    }
}

/// [`segmented`] over `n` segments selected by index, labelled by `label(i)` (borrowed where the
/// host owns the text, so a steady frame allocates nothing). Returns `true` on a change.
pub fn segmented_idx<'a>(
    ui: &mut Ui,
    value: &mut usize,
    n: usize,
    label: impl Fn(usize) -> Cow<'a, str>,
) -> bool {
    match segmented_pick(ui, n, Some(*value), label, |_| "") {
        Some(i) => {
            *value = i;
            true
        }
        None => false,
    }
}

/// The strip over `n` segments labelled by `label(i)`; the index clicked this frame (never the
/// one already `selected`). The primitive under [`segmented`] and the grid's string choices,
/// which would otherwise build a `Vec` of items per row per frame. Fills the available width;
/// when the widest label does not fit its segment it is drawn as a dropdown ([`dropdown`])
/// rather than truncated.
pub fn segmented_pick<'a>(
    ui: &mut Ui,
    n: usize,
    selected: Option<usize>,
    label: impl Fn(usize) -> Cow<'a, str>,
    tip: impl Fn(usize) -> &'a str,
) -> Option<usize> {
    if n == 0 {
        return None;
    }
    let h = ui.spacing().interact_size.y;
    let style = ui.style();
    let font = style
        .override_font_id
        .clone()
        .unwrap_or_else(|| egui::TextStyle::Button.resolve(style));
    let avail = ui.available_width();
    let need_w = need_width(ui, n, &font, &label);
    let total_w = if avail.is_finite() {
        avail
    } else {
        MIN_WIDTH.max(need_w)
    };
    if need_w > total_w {
        return dropdown(ui, n, selected, &label, &tip, total_w, h);
    }
    // Ids of the segments hang off the strip's own (auto) id, so two strips with the same labels
    // in one `Ui` do not share them.
    let (rect, base) = ui.allocate_exact_size(Vec2::new(total_w, h), Sense::hover());

    // Snapshot theme colours up front so the per-segment `ui.interact` (which needs `&mut ui`)
    // does not clash with the immutable `ui.visuals()` borrow.
    let (track, edge, sel_bg, sel_txt, txt, hover_bg) = {
        let v = ui.visuals();
        (
            v.extreme_bg_color,
            v.widgets.inactive.bg_stroke.color,
            v.selection.bg_fill,
            v.strong_text_color(),
            v.text_color(),
            v.widgets.hovered.bg_fill,
        )
    };
    let thin = crate::WidgetsConfig::get(ui).metrics.stroke_thin;
    ui.painter().rect_filled(rect, ROUND, track);

    let seg_w = total_w / n as f32;
    let enabled = ui.is_enabled();
    let mut clicked = None;
    for i in 0..n {
        let name = label(i);
        let x = rect.left() + seg_w * i as f32;
        let seg = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(seg_w, h));
        let id = base.id.with(i);
        let mut resp = ui
            .interact(seg, id, Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let is_sel = selected == Some(i);
        resp.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, enabled, is_sel, &*name));
        if !tip(i).is_empty() {
            resp = resp.on_hover_text(tip(i));
        }
        let painter = ui.painter();
        if is_sel {
            painter.rect_filled(seg.shrink(1.5), ROUND - 1.0, sel_bg);
        } else if resp.hovered() {
            painter.rect_filled(seg.shrink(1.5), ROUND - 1.0, hover_bg);
        }
        let color = if is_sel { sel_txt } else { txt };
        let job = LayoutJob::simple_singleline(name.to_string(), font.clone(), color);
        let galley = painter.layout_job(job);
        let pos = seg.center() - galley.size() / 2.0;
        painter.galley(pos, galley, color);
        // Hairline divider before every segment except the first.
        if i > 0 {
            painter.vline(
                x,
                Rangef::new(rect.top() + 3.0, rect.bottom() - 3.0),
                Stroke::new(thin, edge),
            );
        }
        if resp.clicked() && !is_sel {
            clicked = Some(i);
        }
    }
    // Outer frame, drawn last so it sits on top of the segment fills.
    ui.painter()
        .rect_stroke(rect, ROUND, Stroke::new(thin, edge), StrokeKind::Inside);
    clicked
}

/// The strip's dropdown form: a combo box `width` wide and `h` high listing every label, the
/// selected one as its text. Returns the index picked this frame (never the `selected` one).
fn dropdown<'a>(
    ui: &mut Ui,
    n: usize,
    selected: Option<usize>,
    label: &impl Fn(usize) -> Cow<'a, str>,
    tip: &impl Fn(usize) -> &'a str,
    width: f32,
    h: f32,
) -> Option<usize> {
    let mut picked = None;
    // The id hangs off the strip's auto id like the segments' do.
    let id = ui.next_auto_id();
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = h;
        ComboBox::from_id_salt(id)
            .width(width)
            // A cell too narrow even for the dropdown still must not overflow.
            .truncate()
            .selected_text(selected.map_or(Cow::Borrowed(""), label))
            .show_ui(ui, |ui| {
                for i in 0..n {
                    let mut resp = ui.selectable_label(selected == Some(i), &*label(i));
                    if !tip(i).is_empty() {
                        resp = resp.on_hover_text(tip(i));
                    }
                    if resp.clicked() && selected != Some(i) {
                        picked = Some(i);
                    }
                }
            });
    });
    picked
}

/// Width the strip needs for every label at the minimum padding (equal segments). Cached in egui
/// memory against a hash of the labels and the font, so a steady frame measures nothing.
fn need_width<'a>(
    ui: &Ui,
    n: usize,
    font: &egui::FontId,
    label: &impl Fn(usize) -> Cow<'a, str>,
) -> f32 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    font.hash(&mut hasher);
    for i in 0..n {
        label(i).hash(&mut hasher);
    }
    let key = hasher.finish();
    let id = ui.next_auto_id().with("seg_need_w");
    if let Some((k, w)) = ui.data(|d| d.get_temp::<(u64, f32)>(id))
        && k == key
    {
        return w;
    }
    let widest = (0..n)
        .map(|i| {
            ui.painter()
                .layout_no_wrap(label(i).into_owned(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
        .fold(0.0, f32::max);
    let w = (widest + 2.0 * PAD_X) * n as f32;
    ui.data_mut(|d| d.insert_temp(id, (key, w)));
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT as _, Queryable as _};

    fn harness(width: f32) -> Harness<'static, String> {
        Harness::builder()
            .with_size(Vec2::new(width, 60.0))
            .with_max_steps(8)
            .build_ui_state(
                |ui, v: &mut String| {
                    let items = [
                        ("off".to_owned(), "off".to_owned()),
                        ("selected".to_owned(), "selected".to_owned()),
                        ("all".to_owned(), "all".to_owned()),
                    ];
                    let items: Vec<(&str, String)> =
                        items.iter().map(|(l, v)| (l.as_str(), v.clone())).collect();
                    segmented(ui, v, &items);
                },
                "selected".to_owned(),
            )
    }

    /// Wide: the strip (radio segments). Narrow: a combo showing the full label, never "selec…".
    #[test]
    fn narrow_width_becomes_a_dropdown_with_the_full_label() {
        let wide = harness(300.0);
        assert!(wide.query_by_label("off").is_some(), "wide: segments");
        assert_eq!(
            wide.get_by_label("selected").accesskit_node().role(),
            egui::accesskit::Role::RadioButton
        );

        let mut narrow = harness(120.0);
        narrow.run();
        assert!(
            narrow.query_by_label("off").is_none(),
            "narrow: no segments"
        );
        let combo = narrow.get_by_role(egui::accesskit::Role::ComboBox);
        assert_eq!(combo.value().as_deref(), Some("selected"), "full label");
    }
}
