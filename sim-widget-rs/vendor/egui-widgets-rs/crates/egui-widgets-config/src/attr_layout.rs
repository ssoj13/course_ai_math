//! Geometry shared by property tables and hosts that already own their row projection.
use egui::{Align, Id, InnerResponse, Layout, Rect, Response, Sense, Ui, UiBuilder, Vec2};
use std::hash::Hash;

/// Fixed column geometry in points. Zero prefix width reserves no gutter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttrLayout {
    pub prefix_width: f32,
    pub label_width: f32,
    pub editor_width: f32,
    pub action_width: f32,
    pub row_height: f32,
    pub gap: f32,
}
impl Default for AttrLayout {
    fn default() -> Self {
        Self {
            prefix_width: 0.0,
            label_width: 130.0,
            editor_width: 180.0,
            action_width: 96.0,
            row_height: 24.0,
            gap: 4.0,
        }
    }
}
/// Every rectangle comes from one row origin; an absent prefix is a zero-width rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AttrRowRects {
    pub row: Rect,
    pub prefix: Rect,
    pub label: Rect,
    pub value: Rect,
    pub actions: Rect,
    pub trailing: Rect,
}
fn nonnegative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}
fn finite_rect(rect: Rect) -> Rect {
    let min = egui::pos2(
        if rect.min.x.is_finite() {
            rect.min.x
        } else {
            0.0
        },
        if rect.min.y.is_finite() {
            rect.min.y
        } else {
            0.0
        },
    );
    Rect::from_min_size(
        min,
        Vec2::new(nonnegative(rect.width()), nonnegative(rect.height())),
    )
}
impl AttrLayout {
    /// Fit fixed columns to a row, proportionally shrinking them on narrow panels.
    /// Slack remains after the action column, rather than stretching numeric fields.
    pub fn row_rects(&self, row: Rect) -> AttrRowRects {
        let row = finite_rect(row);
        let mut widths = [
            self.prefix_width,
            self.label_width,
            self.editor_width,
            self.action_width,
        ]
        .map(nonnegative);
        let active = widths.iter().filter(|w| **w > 0.0).count();
        let gaps = active.saturating_sub(1);
        let gap = nonnegative(self.gap).min(if gaps == 0 {
            0.0
        } else {
            row.width() / gaps as f32
        });
        let available = (row.width() - gap * gaps as f32).max(0.0);
        let total: f32 = widths.iter().sum();
        if total > available && total > 0.0 {
            for width in &mut widths {
                *width *= available / total;
            }
        }
        let mut x = row.left();
        let mut seen = false;
        let cells = widths.map(|width| {
            if width > 0.0 && seen {
                x += gap;
            }
            let rect = Rect::from_min_size(
                egui::pos2(x.min(row.right()), row.top()),
                Vec2::new(width.min((row.right() - x).max(0.0)), row.height()),
            );
            x = rect.right();
            seen |= width > 0.0;
            rect
        });
        AttrRowRects {
            row,
            prefix: cells[0],
            label: cells[1],
            value: cells[2],
            actions: cells[3],
            trailing: Rect::from_min_max(egui::pos2(x.min(row.right()), row.top()), row.max),
        }
    }
}
/// One-line numeric geometry. Axis identity belongs to the editor, not this layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ValueEditorLayout {
    pub numeric_width: f32,
    pub row_height: Option<f32>,
    pub gap: f32,
}
impl Default for ValueEditorLayout {
    fn default() -> Self {
        Self {
            numeric_width: 46.0,
            row_height: None,
            gap: 4.0,
        }
    }
}
impl ValueEditorLayout {
    /// Allocate equally sized, vertically centered numeric cells. Scalar rows reserve
    /// the same three-component width budget as XYZ, so their first fields align.
    #[deprecated(note = "left-packed cells no longer match the attribute grid: use `spread_cells`")]
    pub fn numeric_cells(
        &self,
        cell: Rect,
        count: usize,
    ) -> impl ExactSizeIterator<Item = Rect> + Clone {
        #[allow(deprecated)]
        self.numeric_cells_iter(cell, count)
    }
    /// Allocation-free variant for per-frame editor rendering.
    #[deprecated(note = "left-packed cells no longer match the attribute grid: use `spread_cells`")]
    pub fn numeric_cells_iter(
        &self,
        cell: Rect,
        count: usize,
    ) -> impl ExactSizeIterator<Item = Rect> + Clone {
        let cell = finite_rect(cell);
        let reserved = count.max(3);
        let gap =
            nonnegative(self.gap).min(cell.width() / reserved.saturating_sub(1).max(1) as f32);
        let width = nonnegative(self.numeric_width)
            .min(((cell.width() - gap * (reserved - 1) as f32) / reserved as f32).max(0.0));
        let height = self
            .row_height
            .map(nonnegative)
            .unwrap_or(cell.height())
            .min(cell.height());
        (0..count).map(move |i| {
            Rect::from_min_size(
                egui::pos2(
                    cell.left() + i as f32 * (width + gap),
                    cell.center().y - height * 0.5,
                ),
                Vec2::new(width, height),
            )
        })
    }
}
impl ValueEditorLayout {
    /// `count` equally wide cells spread over the WHOLE of `cell` (gaps of [`Self::gap`]), so the
    /// last one ends at the cell's right edge: the same edge as a slider's value box. Unlike
    /// [`Self::numeric_cells`] (left-packed at `numeric_width`, the shared geometry of timeline
    /// rows) the width follows the cell; at the grid's minimum editor width it IS `numeric_width`.
    /// THE function for numeric fields: the attribute grid and a host's timeline property rows
    /// both use it, so X / Y / Z columns line up between the two.
    pub fn spread_cells(
        &self,
        cell: Rect,
        count: usize,
    ) -> impl ExactSizeIterator<Item = Rect> + Clone {
        let cell = finite_rect(cell);
        let n = count.max(1) as f32;
        let gap = nonnegative(self.gap).min(cell.width() / (n - 1.0).max(1.0));
        let width = ((cell.width() - gap * (n - 1.0)) / n).max(0.0);
        let height = self
            .row_height
            .map(nonnegative)
            .unwrap_or(cell.height())
            .min(cell.height());
        (0..count).map(move |i| {
            Rect::from_min_size(
                egui::pos2(
                    cell.left() + i as f32 * (width + gap),
                    cell.center().y - height * 0.5,
                ),
                Vec2::new(width, height),
            )
        })
    }
}
/// Draw in an exact, clipped cell without changing the parent's row projection.
pub fn cell_ui<R>(
    ui: &mut Ui,
    rect: Rect,
    id: impl Hash + std::fmt::Debug,
    draw: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let rect = finite_rect(rect);
    let mut cell = ui.new_child(
        UiBuilder::new()
            .id_salt(Id::new(id))
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    cell.set_clip_rect(ui.clip_rect().intersect(rect));
    if cell.spacing().interact_size.y > rect.height() {
        cell.spacing_mut().interact_size.y = rect.height();
    }
    cell.set_min_size(rect.size());
    let inner = draw(&mut cell);
    let response = ui.interact(rect, cell.id().with("cell"), Sense::hover());
    InnerResponse { inner, response }
}
/// A generic glyph button whose response is exactly square and fits inside its cell.
/// Domain meaning, selection, tooltip and callbacks remain owned by the host.
pub fn square_icon(ui: &mut Ui, rect: Rect, icon: &str, enabled: bool) -> Response {
    square_icon_with_paint(ui, rect, icon, enabled, None, None)
}

/// Customize glyph paint without cloning or mutating the inherited Ui style.
/// Hit geometry and interaction are identical to [`square_icon`].
/// A square icon button in `rect` sized to the style's icon width, painted in the selection
/// colour while `active` (an on / keyed state). The one icon toggle of attribute rows: key and
/// animation buttons, channel carets, row toggles.
pub fn icon_toggle(ui: &mut Ui, rect: Rect, icon: &str, active: bool, enabled: bool) -> Response {
    let side = ui.spacing().icon_width.min(rect.width()).min(rect.height());
    let rect = Rect::from_center_size(rect.center(), Vec2::splat(side));
    let mut font = egui::TextStyle::Button.resolve(ui.style());
    font.size = side * 0.8;
    let color = active.then(|| ui.visuals().selection.stroke.color);
    square_icon_with_paint(ui, rect, icon, enabled, Some(font), color)
}

pub fn square_icon_with_paint(
    ui: &mut Ui,
    rect: Rect,
    icon: &str,
    enabled: bool,
    font: Option<egui::FontId>,
    color: Option<egui::Color32>,
) -> Response {
    square_icon_labeled(ui, rect, icon, icon, enabled, font, color)
}

/// An icon button of the attribute grid's look in `rect`: the `icon` glyph in a square of the
/// style's icon width, with `name` as its AccessKit label and hover tooltip (assistive
/// technology reads a word, not the glyph). The one named icon button: the grid's value
/// actions, the ramp's Reset.
pub fn icon_button(ui: &mut Ui, rect: Rect, icon: &str, name: &str, enabled: bool) -> Response {
    icon_button_tip(ui, rect, icon, name, name, enabled)
}

/// [`icon_button`] whose hover tooltip `tip` is longer than its AccessKit `name` ("Reset to
/// default" on a button named "Reset").
pub fn icon_button_tip(
    ui: &mut Ui,
    rect: Rect,
    icon: &str,
    name: &str,
    tip: &str,
    enabled: bool,
) -> Response {
    let side = ui.spacing().icon_width.min(rect.width()).min(rect.height());
    let square = Rect::from_center_size(rect.center(), Vec2::splat(side));
    let mut font = egui::TextStyle::Button.resolve(ui.style());
    font.size = side * 0.8;
    square_icon_labeled(ui, square, icon, name, enabled, Some(font), None).on_hover_text(tip)
}

/// [`square_icon_with_paint`] with an explicit AccessKit `label` (the plain one labels the glyph).
fn square_icon_labeled(
    ui: &mut Ui,
    rect: Rect,
    icon: &str,
    label: &str,
    enabled: bool,
    font: Option<egui::FontId>,
    color: Option<egui::Color32>,
) -> Response {
    let rect = finite_rect(rect);
    let side = rect.width().min(rect.height());
    let square = Rect::from_center_size(rect.center(), Vec2::splat(side));
    let mut cell = ui.new_child(
        UiBuilder::new()
            .id_salt((
                "square_icon",
                square.min.x.to_bits(),
                square.min.y.to_bits(),
            ))
            .max_rect(square),
    );
    cell.set_clip_rect(ui.clip_rect().intersect(square));
    if !enabled {
        cell.disable();
    }
    let response = cell.interact(square, Id::new((cell.id(), "button")), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    if cell.is_rect_visible(square) {
        let visuals = cell.style().interact(&response);
        cell.painter().rect(
            square,
            visuals.corner_radius,
            visuals.bg_fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        cell.painter().text(
            square.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            font.unwrap_or_else(|| egui::TextStyle::Button.resolve(cell.style())),
            color.unwrap_or_else(|| visuals.text_color()),
        );
    }
    response
}

#[cfg(test)]
#[allow(deprecated)] // the tests keep covering the packed cells until they are removed
mod tests {
    use super::*;
    /// A named icon button is announced by its name (once), never by its glyph.
    #[test]
    fn icon_button_is_named_for_assistive_technology() {
        use egui_kittest::{Harness, kittest::Queryable as _};
        let harness = Harness::builder().with_max_steps(8).build_ui(|ui| {
            let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::splat(14.0));
            icon_button(ui, rect, "*", "Reset", true);
        });
        assert!(harness.query_by_label("Reset").is_some());
        assert!(
            harness.query_by_label("*").is_none(),
            "the glyph is not the name"
        );
    }

    #[test]
    fn custom_icon_paint_preserves_style_and_clipped_hit_geometry() {
        use std::sync::Arc;
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let style = ui.style().clone();
            let cell = Rect::from_min_size(egui::pos2(10.0, 20.0), Vec2::new(12.0, 18.0));
            ui.set_clip_rect(cell);
            let response = square_icon_with_paint(
                ui,
                cell,
                "*",
                true,
                Some(egui::FontId::proportional(9.0)),
                Some(egui::Color32::YELLOW),
            );
            assert!(Arc::ptr_eq(&style, ui.style()));
            assert_eq!(response.rect.width(), 12.0);
            assert_eq!(response.rect.height(), 12.0);
            assert_eq!(response.rect.center(), cell.center());
            assert!(cell.contains_rect(response.interact_rect));
        });
        output.textures_delta.clear();
    }
    #[test]
    fn columns_and_numeric_cells_fit_narrow_and_invalid_requests() {
        for width in [0.0, 1.0, 47.0, 200.0, 640.0] {
            let row = Rect::from_min_size(egui::pos2(12.0, 20.0), Vec2::new(width, 24.0));
            let layout = AttrLayout {
                prefix_width: 42.0,
                ..Default::default()
            };
            let rects = layout.row_rects(row);
            for rect in [
                rects.prefix,
                rects.label,
                rects.value,
                rects.actions,
                rects.trailing,
            ] {
                assert!(rect.is_finite());
                assert!(rect.left() >= row.left() && rect.right() <= row.right() + 0.001);
                assert!(rect.width() >= 0.0);
            }
            for count in [1, 3, 4, 8] {
                for rect in ValueEditorLayout::default().numeric_cells(rects.value, count) {
                    assert!(rect.is_finite());
                    assert!(
                        rect.left() >= rects.value.left()
                            && rect.right() <= rects.value.right() + 0.001
                    );
                    assert_eq!(rect.center().y, row.center().y);
                }
            }
        }
        let bad = AttrLayout {
            label_width: f32::NAN,
            editor_width: f32::INFINITY,
            gap: -1.0,
            ..Default::default()
        };
        assert!(
            bad.row_rects(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(100.0, 24.0)
            ))
            .value
            .is_finite()
        );
    }
    #[test]
    fn scalar_and_xyz_share_fixed_first_cell() {
        let cell = Rect::from_min_size(egui::pos2(80.0, 20.0), Vec2::new(180.0, 24.0));
        let layout = ValueEditorLayout::default();
        assert_eq!(
            layout.numeric_cells(cell, 1).next().unwrap(),
            layout.numeric_cells(cell, 3).next().unwrap()
        );
        assert_eq!(layout.numeric_cells(cell, 1).next().unwrap().width(), 46.0);
    }
    #[test]
    fn cells_reuse_fitting_inherited_style_and_only_clamp_smaller_cells() {
        use std::sync::Arc;
        let ctx = egui::Context::default();
        let mut style = egui::Style::default();
        style.spacing.interact_size.y = 14.0;
        let parent_style = Arc::new(style);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.set_style(parent_style.clone());
            let parent_clip = Rect::from_min_size(egui::pos2(5.0, 5.0), Vec2::new(120.0, 120.0));
            ui.set_clip_rect(parent_clip);
            for height in [16.0, 14.0] {
                let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), Vec2::new(80.0, height));
                let response = cell_ui(ui, rect, height.to_bits(), |cell| {
                    assert!(Arc::ptr_eq(cell.style(), &parent_style));
                    assert_eq!(cell.spacing().interact_size.y, 14.0);
                    assert_eq!(cell.clip_rect(), parent_clip.intersect(rect));
                });
                assert_eq!(response.response.rect, rect);
            }
            assert!(Arc::ptr_eq(ui.style(), &parent_style));
            assert_eq!(ui.clip_rect(), parent_clip);
            let mut default_style = egui::Style::default();
            default_style.spacing.interact_size.y = 18.0;
            let default_style = Arc::new(default_style);
            ui.set_style(default_style.clone());
            let small = Rect::from_min_size(egui::pos2(10.0, 40.0), Vec2::new(80.0, 8.0));
            cell_ui(ui, small, "small_cell", |cell| {
                assert!(!Arc::ptr_eq(cell.style(), &default_style));
                assert_eq!(cell.spacing().interact_size.y, 8.0);
                assert_eq!(cell.clip_rect(), parent_clip.intersect(small));
            });
            assert!(Arc::ptr_eq(ui.style(), &default_style));
            assert_eq!(ui.spacing().interact_size.y, 18.0);
            assert_eq!(ui.clip_rect(), parent_clip);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn actual_icon_response_is_square_centered_and_clipped() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let row = Rect::from_min_size(egui::pos2(40.0, 40.0), Vec2::new(80.0, 24.0));
            let response = square_icon(ui, row, "*", true);
            assert_eq!(response.rect.width(), 24.0);
            assert_eq!(response.rect.height(), 24.0);
            assert_eq!(response.rect.center(), row.center());
            let clip = ui.clip_rect().intersect(row);
            let inherited_height = ui.spacing().interact_size.y;
            cell_ui(ui, row, "cell_test", |cell| {
                assert_eq!(cell.clip_rect(), clip);
                assert_eq!(
                    cell.spacing().interact_size.y,
                    inherited_height.min(row.height())
                );
            });
        });
        output.textures_delta.clear();
    }

    #[test]
    fn spread_cells_fill_the_cell_so_the_last_one_ends_at_its_right_edge() {
        let cell = Rect::from_min_size(egui::pos2(10.0, 4.0), Vec2::new(133.0, 20.0));
        let layout = ValueEditorLayout {
            gap: 2.0,
            row_height: Some(14.0),
            ..Default::default()
        };
        for count in [1, 3, 4] {
            let cells: Vec<Rect> = layout.spread_cells(cell, count).collect();
            assert_eq!(cells.len(), count);
            assert!(
                (cells[count - 1].right() - cell.right()).abs() < 1e-3,
                "{count}: {cells:?}"
            );
            assert_eq!(cells[0].left(), cell.left());
            assert!(
                cells
                    .iter()
                    .all(|c| (c.width() - cells[0].width()).abs() < 1e-3)
            );
            assert!(
                cells
                    .iter()
                    .all(|c| c.height() == 14.0 && c.center().y == cell.center().y)
            );
        }
        assert_eq!(layout.spread_cells(cell, 0).count(), 0);
    }
}
