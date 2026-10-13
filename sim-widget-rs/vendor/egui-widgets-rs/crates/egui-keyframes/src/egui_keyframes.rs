//! # egui-keyframes
//!
//! A reusable, **generic** keyframe / animation-curve panel for [`egui`] 0.34.
//!
//! It renders an After Effects-style attribute editor: a list of animatable
//! [`Property`] rows, each with an inline value editor (left) and a keyframe
//! track painted over a shared time ruler (right). A draggable playhead marks the
//! current [`KeyframePanel::time`]. Users can:
//!
//! - scrub the playhead (click / drag the ruler),
//! - edit a property's current value (drag value / slider / checkbox),
//! - add a keyframe (double-click an empty spot on a track, or the `+` button),
//! - move a keyframe horizontally in time (drag a diamond),
//! - delete a keyframe (right-click a diamond).
//!
//! ## Decoupling
//!
//! The widget owns no state about your data. Each frame you pass a flat
//! [`KeyframePanel`] snapshot and apply the returned [`KeyAction`] intents. Values
//! are plain `f32` and ids are `u64` — there is no dependency on any engine value
//! type, `Uuid`, or serialization format. See [`model`].
//!
//! ## Persistent view state
//!
//! [`KeyframesState`] holds the only thing the widget needs to remember between
//! frames: the name-column width (a draggable splitter). Persist it yourself; with
//! the default `serde` feature it derives `Serialize`/`Deserialize`.
//!
//! ## Example
//!
//! ```no_run
//! use egui_keyframes::{KeyframePanel, KeyframesState, Property, Key, KeyAction};
//!
//! # fn ui_example(ui: &mut egui::Ui, state: &mut KeyframesState) {
//! let mut panel = KeyframePanel {
//!     properties: vec![{
//!         let mut p = Property::float(1, "Opacity", 100.0);
//!         p.range = Some((0.0, 100.0));
//!         p.keys = vec![Key::new(0.0, 0.0), Key::new(1.0, 100.0)];
//!         p
//!     }],
//!     time: 0.5,
//!     start: 0.0,
//!     end: 2.0,
//! };
//!
//! for action in egui_keyframes::show(ui, &panel, state) {
//!     match action {
//!         KeyAction::Seek { time } => panel.time = time,
//!         // ... apply AddKey / MoveKey / DeleteKey / SetValue to your data ...
//!         _ => {}
//!     }
//! }
//! # }
//! ```

mod model;

pub use model::{Key, KeyAction, KeyframePanel, PropKind, Property};

use egui::{
    Align, Color32, CornerRadius, Layout, Pos2, Rect, Response, Sense, Stroke, TextStyle, Ui, Vec2,
};

/// Persistent UI state for the keyframe panel — just the splitter position.
///
/// The widget is otherwise stateless across frames (interaction state lives in
/// egui's own memory, keyed by widget ids). Persist this with your app config.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct KeyframesState {
    /// Width in points of the left "name + value" column. Drag the splitter to
    /// change; clamped to a sane range each frame.
    pub name_column_width: f32,
}

impl Default for KeyframesState {
    fn default() -> Self {
        Self {
            name_column_width: DEFAULT_NAME_COL,
        }
    }
}

/// Visual constants. Kept private — the look is intentionally opinionated and
/// faithful to the source AE-style panel rather than themeable, to keep the API
/// small. Colors derive from the active egui visuals where it matters.
const ROW_PAD: f32 = 2.0;
const KEY_RADIUS: f32 = 5.0;
const MIN_NAME_COL: f32 = 120.0;
/// Default name-column width (px) — the initial [`KeyframesState`] value AND the
/// splitter's double-click-to-reset target, kept as one constant.
const DEFAULT_NAME_COL: f32 = 220.0;
/// Horizontal padding inside the track so edge keyframes are not clipped.
const TRACK_INSET: f32 = KEY_RADIUS + 2.0;

/// Render the keyframe panel and return the list of edit intents produced this
/// frame. See the crate docs for the interaction model.
///
/// `panel` is a read-only snapshot; `state` persists the splitter position.
pub fn show(ui: &mut Ui, panel: &KeyframePanel, state: &mut KeyframesState) -> Vec<KeyAction> {
    let mut actions = Vec::new();

    // Clamp the splitter to the available width so the track always has room.
    let avail = ui.available_width();
    let max_name_col = (avail - 160.0).max(MIN_NAME_COL);
    state.name_column_width = state.name_column_width.clamp(MIN_NAME_COL, max_name_col);

    let row_h = ui
        .text_style_height(&TextStyle::Body)
        .max(ui.spacing().interact_size.y);

    if panel.properties.is_empty() {
        ui.label("(no animatable properties)");
        return actions;
    }

    // The time window must be non-degenerate for the linear time<->x mapping.
    let span = (panel.end - panel.start).max(f64::EPSILON);

    // We lay out rows by hand (rather than a table) so the track column is a
    // single continuous painting surface shared by the ruler and the playhead.
    let top = ui.cursor().min;
    let total_w = ui.available_width();
    let split_x = top.x + state.name_column_width;
    // Track rect spans from the splitter to the right edge, insets applied when
    // mapping time so diamonds at the window edges stay fully visible.
    let track_left = split_x + 1.0;
    let track_right = top.x + total_w;

    // Map a panel time to an x pixel inside the track (with edge inset).
    let time_to_x = |t: f64| -> f32 {
        let frac = ((t - panel.start) / span).clamp(0.0, 1.0) as f32;
        let l = track_left + TRACK_INSET;
        let r = (track_right - TRACK_INSET).max(l);
        l + frac * (r - l)
    };
    // Inverse map: an x pixel back to a panel time (clamped to the window).
    let x_to_time = |x: f32| -> f64 {
        let l = track_left + TRACK_INSET;
        let r = (track_right - TRACK_INSET).max(l + 1.0);
        let frac = ((x - l) / (r - l)).clamp(0.0, 1.0) as f64;
        panel.start + frac * span
    };

    // --- Ruler header: scrub + playhead -----------------------------------
    let ruler_h = row_h;
    let ruler_rect = Rect::from_min_size(
        Pos2::new(track_left, top.y),
        Vec2::new(track_right - track_left, ruler_h),
    );
    let ruler_resp = ui.interact(
        ruler_rect,
        ui.make_persistent_id("kf_ruler"),
        Sense::click_and_drag(),
    );
    if let Some(t) = scrub_time(&ruler_resp, x_to_time) {
        actions.push(KeyAction::Seek { time: t });
    }
    paint_ruler(ui, ruler_rect);

    // Advance the layout cursor past the ruler row.
    ui.allocate_rect(ruler_rect, Sense::hover());
    ui.advance_cursor_after_rect(Rect::from_min_max(
        Pos2::new(top.x, top.y),
        Pos2::new(track_right, top.y + ruler_h),
    ));

    // --- Property rows -----------------------------------------------------
    let mut rows_top = top.y + ruler_h + ROW_PAD;
    for (pi, prop) in panel.properties.iter().enumerate() {
        let row_top = rows_top;
        let row_rect = Rect::from_min_max(
            Pos2::new(top.x, row_top),
            Pos2::new(track_right, row_top + row_h),
        );

        // Striped background for readability.
        if pi % 2 == 1 {
            ui.painter()
                .rect_filled(row_rect, CornerRadius::ZERO, ui.visuals().faint_bg_color);
        }

        // Left column: name + inline value editor, inside a child UI clipped to
        // the name column so widgets never overlap the track.
        let name_rect = Rect::from_min_max(
            Pos2::new(top.x, row_top),
            Pos2::new(split_x - 4.0, row_top + row_h),
        );
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(name_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        edit_property_row(&mut child, prop, &mut actions);

        // Right column: the keyframe track for this property.
        let track_rect = Rect::from_min_max(
            Pos2::new(track_left, row_top),
            Pos2::new(track_right, row_top + row_h),
        );
        track_row(ui, prop, track_rect, &time_to_x, &x_to_time, &mut actions);

        rows_top += row_h + ROW_PAD;
    }

    // Make egui aware of the full area we painted so following widgets flow below.
    let used = Rect::from_min_max(top, Pos2::new(track_right, rows_top));
    ui.advance_cursor_after_rect(used);

    // --- Splitter + playhead (drawn last, on top of all rows) -------------
    let split_top = top.y;
    let split_bottom = rows_top;
    draw_splitter(ui, state, split_x, split_top, split_bottom, max_name_col);

    // Playhead spans the whole panel height over the track.
    let px = time_to_x(panel.time);
    let playhead_color = ui.visuals().warn_fg_color;
    let metrics = egui_widgets_config::WidgetsConfig::get(ui).metrics;
    ui.painter().line_segment(
        [Pos2::new(px, split_top), Pos2::new(px, split_bottom)],
        Stroke::new(metrics.stroke_thin, playhead_color),
    );
    // Small triangle handle at the top of the playhead.
    ui.painter()
        .circle_filled(Pos2::new(px, split_top + 3.0), 3.0, playhead_color);

    actions
}

/// Translate a ruler click/drag into a seek time, if the interaction is active.
fn scrub_time(resp: &Response, x_to_time: impl Fn(f32) -> f64) -> Option<f64> {
    if resp.clicked() || resp.dragged() {
        let x = resp.interact_pointer_pos()?.x;
        Some(x_to_time(x))
    } else {
        None
    }
}

/// Paint the ruler background strip and a baseline.
fn paint_ruler(ui: &Ui, rect: Rect) {
    let metrics = egui_widgets_config::WidgetsConfig::get(ui).metrics;
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, ui.visuals().extreme_bg_color);
    let line_y = rect.max.y - 1.0;
    p.line_segment(
        [Pos2::new(rect.min.x, line_y), Pos2::new(rect.max.x, line_y)],
        Stroke::new(
            metrics.stroke_thin,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ),
    );
}

/// Draw the draggable column splitter and apply drag deltas to `state`, via the
/// shared [`egui_widgets_config::splitter::interact`] mechanics (so its drag,
/// hover/active colour, cursor and double-click-to-reset match every other
/// splitter in the widget set). Double-click resets the name column to its default.
fn draw_splitter(
    ui: &mut Ui,
    state: &mut KeyframesState,
    x: f32,
    top: f32,
    bottom: f32,
    max_clamp: f32,
) {
    // Zero-width rect carrying just the splitter's y-span; `interact` builds the
    // hit zone from `x` ± half-width and paints the line at `x`.
    let span = Rect::from_min_max(Pos2::new(x, top), Pos2::new(x, bottom));
    let id = ui.make_persistent_id("kf_splitter");
    let metrics = egui_widgets_config::WidgetsConfig::get(ui).metrics;
    let hit = egui_widgets_config::splitter::interact(ui, span, x, id, 4.0, 0.0, &metrics);
    if let Some(dx) = hit.delta {
        state.name_column_width = (state.name_column_width + dx).clamp(MIN_NAME_COL, max_clamp);
    }
    if hit.double_clicked {
        state.name_column_width = DEFAULT_NAME_COL.clamp(MIN_NAME_COL, max_clamp);
    }
}

/// Render the left-column name label + inline value editor for one property,
/// pushing any [`KeyAction::SetValue`] edits.
fn edit_property_row(ui: &mut Ui, prop: &Property, actions: &mut Vec<KeyAction>) {
    ui.push_id(prop.id, |ui| {
        ui.label(format!("{}:", prop.name));
        // Right-align the editor so labels and editors line up across rows.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            value_editor(ui, prop, actions);
        });
    });
}

/// Inline value editor selected by [`PropKind`]. Emits one `SetValue` per changed
/// component. Works on copies (the model is read-only); the caller applies edits.
fn value_editor(ui: &mut Ui, prop: &Property, actions: &mut Vec<KeyAction>) {
    let speed = prop.eff_speed();
    match prop.kind {
        PropKind::Bool => {
            let mut b = prop.value[0] != 0.0;
            if ui.checkbox(&mut b, "").changed() {
                actions.push(KeyAction::SetValue {
                    prop: prop.id,
                    component: 0,
                    value: if b { 1.0 } else { 0.0 },
                });
            }
        }
        PropKind::Float => {
            let mut v = prop.value[0];
            let resp = if let Some((min, max)) = prop.range {
                ui.add(egui_slider::SlimSlider::new(&mut v, min..=max))
            } else {
                ui.add(egui::DragValue::new(&mut v).speed(speed as f64))
            };
            if resp.changed() {
                actions.push(KeyAction::SetValue {
                    prop: prop.id,
                    component: 0,
                    value: v,
                });
            }
        }
        kind => {
            // Multi-component: one drag value per axis, edited right-to-left so
            // axes read X, Y, Z(, W) left-to-right under the right_to_left layout.
            let n = kind.arity();
            let labels = kind.axis_labels();
            for c in (0..n).rev() {
                let mut v = prop.value[c];
                let resp = ui.add(egui::DragValue::new(&mut v).speed(speed as f64));
                if let Some(l) = labels.get(c) {
                    ui.label(*l);
                }
                if resp.changed() {
                    actions.push(KeyAction::SetValue {
                        prop: prop.id,
                        component: c,
                        value: v,
                    });
                }
            }
        }
    }
}

/// Paint and handle interaction for one property's keyframe track.
///
/// Interactions:
/// - double-click empty track space => `AddKey` at that time (value = current).
/// - drag a diamond horizontally => `MoveKey` (time changes; value preserved).
/// - right-click a diamond => `DeleteKey`.
fn track_row(
    ui: &mut Ui,
    prop: &Property,
    rect: Rect,
    time_to_x: &impl Fn(f64) -> f32,
    x_to_time: &impl Fn(f32) -> f64,
    actions: &mut Vec<KeyAction>,
) {
    let mid_y = rect.center().y;
    let metrics = egui_widgets_config::WidgetsConfig::get(ui).metrics;
    let p = ui.painter();

    // A faint baseline so empty tracks are still legible.
    p.line_segment(
        [Pos2::new(rect.min.x, mid_y), Pos2::new(rect.max.x, mid_y)],
        Stroke::new(
            metrics.stroke_thin,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ),
    );

    // Background interaction surface for "add keyframe" via double-click.
    let bg_resp = ui.interact(
        rect,
        ui.make_persistent_id(("kf_track", prop.id)),
        Sense::click(),
    );
    if bg_resp.double_clicked()
        && let Some(pos) = bg_resp.interact_pointer_pos()
    {
        actions.push(KeyAction::AddKey {
            prop: prop.id,
            time: x_to_time(pos.x),
            value: prop.value[0],
        });
    }

    // Diamonds: one interactive handle per keyframe.
    let diamond_color = ui.visuals().selection.bg_fill;
    let stroke_color = ui.visuals().strong_text_color();
    for (ki, key) in prop.keys.iter().enumerate() {
        let cx = time_to_x(key.time);
        let center = Pos2::new(cx, mid_y);
        let hit = Rect::from_center_size(center, Vec2::splat(KEY_RADIUS * 2.0 + 2.0));
        let resp = ui.interact(
            hit,
            ui.make_persistent_id(("kf_key", prop.id, ki)),
            Sense::click_and_drag(),
        );

        // Drag => move in time (value preserved; vertical drag ignored — the
        // generic track has no value axis).
        if resp.dragged()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            actions.push(KeyAction::MoveKey {
                prop: prop.id,
                index: ki,
                time: x_to_time(pos.x),
                value: key.value,
            });
        }
        // Right-click => delete.
        if resp.secondary_clicked() {
            actions.push(KeyAction::DeleteKey {
                prop: prop.id,
                index: ki,
            });
        }

        let hot = resp.hovered() || resp.dragged();
        paint_diamond(
            ui,
            center,
            if hot { KEY_RADIUS + 1.0 } else { KEY_RADIUS },
            diamond_color,
            Stroke::new(metrics.stroke_thin, stroke_color),
        );
    }
}

/// Paint a keyframe diamond (a 45°-rotated square) with fill + stroke.
fn paint_diamond(ui: &Ui, center: Pos2, r: f32, fill: Color32, stroke: Stroke) {
    let pts = [
        Pos2::new(center.x, center.y - r),
        Pos2::new(center.x + r, center.y),
        Pos2::new(center.x, center.y + r),
        Pos2::new(center.x - r, center.y),
    ];
    let p = ui.painter();
    p.add(egui::Shape::convex_polygon(pts.to_vec(), fill, stroke));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Showing the panel never reports an edit: a ranged property whose stored value lies
    /// outside its range is drawn as it is, with no `SetValue` to write it back clamped.
    #[test]
    fn showing_an_out_of_range_value_emits_no_edit() {
        let mut prop = Property::float(1, "Opacity", 250.0);
        prop.range = Some((0.0, 100.0));
        let panel = KeyframePanel {
            properties: vec![prop],
            time: 0.5,
            start: 0.0,
            end: 2.0,
        };
        let mut state = KeyframesState::default();
        let ctx = egui::Context::default();
        for _ in 0..3 {
            let mut actions = Vec::new();
            let mut out = ctx.run_ui(Default::default(), |ui| {
                actions = show(ui, &panel, &mut state);
            });
            out.textures_delta.clear();
            assert!(
                !actions.iter().any(|a| matches!(a, KeyAction::SetValue { .. })),
                "showing wrote the value: {actions:?}"
            );
        }
        assert_eq!(panel.properties[0].value[0], 250.0);
    }
}