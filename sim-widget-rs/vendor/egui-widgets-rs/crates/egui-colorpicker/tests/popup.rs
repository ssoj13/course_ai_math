//! The picker popup's idle close, through egui's accessibility tree (egui_kittest): it
//! stays open while the pointer rests on its swatch or inside it, and closes once the
//! pointer has been away for the idle time.

use egui::{Pos2, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

/// Frames of real time (60 Hz) well past the popup's 0.5 s idle close.
const TWO_SECONDS: usize = 120;

/// One swatch near the bottom of a short screen (the popup has no room below it and
/// lands over its surroundings, as in a viewport toolbar), its rect stored for the
/// test. Here egui reported the swatch not hovered while the popup was open, and the
/// picker closed itself under a resting pointer.
fn harness() -> Harness<'static, [f32; 4]> {
    Harness::builder()
        .with_size(Vec2::new(320.0, 420.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_ui_state(
            |ui, color: &mut [f32; 4]| {
                ui.add_space(380.0);
                let r = egui_colorpicker::color_button(ui, color);
                ui.ctx()
                    .data_mut(|d| d.insert_temp(egui::Id::new("swatch_rect"), r.rect));
            },
            [0.5, 0.25, 0.75, 1.0],
        )
}

fn open(h: &Harness<'_, [f32; 4]>) -> bool {
    h.query_by_label("LDR").is_some()
}

/// Click the swatch and leave the pointer on it.
fn click_swatch(h: &mut Harness<'static, [f32; 4]>) -> Pos2 {
    h.run();
    let swatch: egui::Rect = h
        .ctx
        .data(|d| d.get_temp(egui::Id::new("swatch_rect")))
        .expect("swatch drawn");
    let at = swatch.center();
    h.hover_at(at);
    h.run_steps(2);
    h.drag_at(at);
    h.drop_at(at);
    h.hover_at(at);
    h.run_steps(2);
    assert!(open(h), "a click on the swatch opens the picker");
    at
}

#[test]
fn stays_open_while_the_pointer_rests_on_the_swatch() {
    let mut h = harness();
    click_swatch(&mut h);
    h.run_steps(TWO_SECONDS);
    assert!(open(&h), "the pointer never left the swatch");
}

#[test]
fn stays_open_while_the_pointer_is_inside_and_closes_when_it_leaves() {
    let mut h = harness();
    click_swatch(&mut h);
    // Inside the popup, on its "range" caption (no tooltip).
    let inside = h.get_by_label("range").rect().center();
    h.hover_at(inside);
    h.run_steps(TWO_SECONDS);
    assert!(open(&h), "the pointer is inside the popup");
    // Away from both: closed within the idle time (plus a few frames).
    h.hover_at(Pos2::new(410.0, 510.0));
    h.run_steps(45);
    assert!(!open(&h), "0.75 s away closes it");
}
