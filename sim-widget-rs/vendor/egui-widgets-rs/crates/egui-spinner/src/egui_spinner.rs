//! A tiny indeterminate "busy" throbber for egui — a rotating arc.
//!
//! This is the indeterminate counterpart to the determinate `egui-progressbar`
//! sibling: use it when you cannot report a fraction (a network call, a spawn,
//! an unknown-length job) but still want to signal "working…".
//!
//! ## Why this exists when egui already has `egui::Spinner`
//! egui 0.34 ships a perfectly good built-in `egui::Spinner`. This crate is a
//! deliberately thin, self-painted variant kept for two reasons:
//! - **Consistent defaults across this widget set.** It sizes off
//!   `interact_size.y` and colours off `weak_text_color()` so a spinner dropped
//!   next to the other widgets here matches without per-call tweaking.
//! - **A free function API** (`spinner(ui)` / `spinner_sized(ui, d, c)`) that
//!   mirrors the terse, immediate-mode style of the rest of the library rather
//!   than the builder form of `egui::Spinner`.
//!
//! If you only need the stock look, `ui.add(egui::Spinner::new())` is fine — this
//! crate adds nothing semantically, only themed convenience. No app types.

/// Paint an indeterminate spinner at the default size.
///
/// Default diameter is `ui.spacing().interact_size.y` (one widget row tall) and
/// the colour is `ui.visuals().weak_text_color()` so it reads as a subtle
/// "busy" hint. Returns the allocated [`egui::Response`] (sized exactly to the
/// spinner) so it composes in rows like any other widget.
pub fn spinner(ui: &mut egui::Ui) -> egui::Response {
    let diameter = ui.spacing().interact_size.y;
    spinner_sized(ui, diameter, None)
}

/// Paint an indeterminate spinner at an explicit `diameter`, optional `color`.
///
/// `color = None` falls back to `ui.visuals().weak_text_color()`. The widget
/// reserves an exact `diameter × diameter` rect via [`egui::Ui::allocate_exact_size`]
/// and paints into it, so it never flexes to the available width.
///
/// The animation is driven from the egui time source (`ui.input(|i| i.time)`),
/// and we call `ui.ctx().request_repaint()` each frame so the arc keeps turning
/// even when nothing else in the UI is animating.
pub fn spinner_sized(
    ui: &mut egui::Ui,
    diameter: f32,
    color: Option<egui::Color32>,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(diameter, diameter), egui::Sense::hover());

    if !ui.is_rect_visible(rect) {
        return response;
    }

    // Keep spinning: without this the arc only advances when some other event
    // repaints the UI, making the spinner stutter or freeze.
    ui.ctx().request_repaint();

    let color = color.unwrap_or_else(|| ui.visuals().weak_text_color());
    let time = ui.input(|i| i.time);

    let center = rect.center();
    // Leave a little breathing room and keep the stroke inside the rect.
    let radius = (diameter * 0.5) - 1.0;
    let stroke_w = (diameter * 0.12).max(1.0);

    // One full turn every ~1.4 s. The arc spans ~3/4 of the circle so there is
    // an obvious "head" chasing a "tail" — the classic throbber read.
    let head = (time * std::f64::consts::TAU / 1.4) as f32;
    let arc_span = std::f32::consts::TAU * 0.75;

    // Approximate the arc with a short polyline; 32 segments is smooth at the
    // sizes a throbber is used at without burdening the tessellator.
    const SEGMENTS: usize = 32;
    let mut points = Vec::with_capacity(SEGMENTS + 1);
    for i in 0..=SEGMENTS {
        let t = i as f32 / SEGMENTS as f32;
        let angle = head + t * arc_span;
        points.push(center + radius * egui::vec2(angle.cos(), angle.sin()));
    }

    ui.painter().add(egui::Shape::line(
        points,
        egui::Stroke::new(stroke_w, color),
    ));

    response
}
