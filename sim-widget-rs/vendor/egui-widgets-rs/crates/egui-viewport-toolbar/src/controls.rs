//! Compact viewer controls for a viewport toolbar, shared by every host (WarpBro, Playa).

use std::ops::RangeInclusive;

use egui::Ui;
use egui_widgets_config::icons;

/// Exposure in EV stops: a reset button and the EV field.
///
/// The button bypasses exposure the After Effects way: from a non-zero value it parks that
/// value in `hold` and sets 0 EV; at 0 EV it restores `hold`. `hold` is the caller's state
/// (persist it or not). The field answers the set's reset gesture
/// ([`egui_widgets_config::reset`]: Ctrl/Cmd-click or a right click) with 0 EV, without
/// touching `hold`. Returns `true` when `ev` changed.
pub fn exposure_control(ui: &mut Ui, ev: &mut f32, hold: &mut f32, range: RangeInclusive<f32>) -> bool {
    let mut changed = false;
    let bypassed = *ev == 0.0;
    let tip = if bypassed {
        format!("Restore {hold:+.2} EV")
    } else {
        "Reset exposure to 0 EV".to_owned()
    };
    if ui
        .add_enabled(!bypassed || *hold != 0.0, egui::Button::new(icons::RESET).small())
        .on_hover_text(tip)
        .clicked()
    {
        if bypassed {
            *ev = *hold;
        } else {
            *hold = *ev;
            *ev = 0.0;
        }
        changed = true;
    }
    let mut field = ui
        .add(
            egui::DragValue::new(ev)
                .speed(0.05)
                .range(range)
                .fixed_decimals(2)
                .suffix(" EV"),
        )
        .on_hover_text("Exposure in EV stops (2^EV linear gain)\nCtrl+click or right-click: 0 EV");
    egui_widgets_config::reset::apply(ui, &mut field, ev, 0.0);
    changed || field.changed()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Click the reset button (the first widget) and return the new (ev, hold).
    fn click_reset(mut ev: f32, mut hold: f32) -> (f32, f32) {
        let ctx = egui::Context::default();
        let mut button = egui::Rect::NOTHING;
        let frame = |events: Vec<egui::Event>, ev: &mut f32, hold: &mut f32, button: &mut egui::Rect| {
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                ui.horizontal(|ui| {
                    let before = ui.cursor().min;
                    exposure_control(ui, ev, hold, -10.0..=10.0);
                    *button = egui::Rect::from_min_size(before, egui::vec2(12.0, 12.0));
                });
            });
            output.textures_delta.clear();
        };
        frame(Vec::new(), &mut ev, &mut hold, &mut button);
        let pos = button.center();
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(vec![egui::Event::PointerMoved(pos), press(true)], &mut ev, &mut hold, &mut button);
        frame(vec![press(false)], &mut ev, &mut hold, &mut button);
        (ev, hold)
    }

    /// Click the EV field (the control's last widget) with `button` + `modifiers`; the final
    /// `ev` and what the control reported on the release frame.
    fn click_field(mut ev: f32, button: egui::PointerButton, modifiers: egui::Modifiers) -> (f32, bool) {
        let ctx = egui::Context::default();
        // One frame: (the field's rect, the control's `changed`).
        let frame = |mut events: Vec<egui::Event>, ev: &mut f32| {
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
            let mut seen = (egui::Rect::NOTHING, false);
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                let mut hold = 0.0;
                ui.horizontal(|ui| {
                    let changed = exposure_control(ui, ev, &mut hold, -10.0..=10.0);
                    // The field is the row's right end: probe just inside it.
                    let end = ui.min_rect().right_bottom();
                    let rect = egui::Rect::from_min_max(egui::pos2(end.x - 8.0, 0.0), egui::pos2(end.x - 4.0, end.y));
                    seen = (rect, changed);
                });
            });
            output.textures_delta.clear();
            seen
        };
        let pos = frame(Vec::new(), &mut ev).0.center();
        let press = |pressed| egui::Event::PointerButton { pos, button, pressed, modifiers };
        frame(vec![egui::Event::PointerMoved(pos), press(true)], &mut ev);
        let changed = frame(vec![press(false)], &mut ev).1;
        (ev, changed)
    }

    #[test]
    fn the_ev_field_resets_to_zero_on_ctrl_click_and_right_click() {
        assert_eq!(
            click_field(2.5, egui::PointerButton::Primary, egui::Modifiers::COMMAND),
            (0.0, true),
            "Ctrl/Cmd-click"
        );
        assert_eq!(
            click_field(2.5, egui::PointerButton::Secondary, egui::Modifiers::NONE),
            (0.0, true),
            "right click"
        );
        assert_eq!(
            click_field(2.5, egui::PointerButton::Primary, egui::Modifiers::NONE).0,
            2.5,
            "a plain click starts typing, it is no reset"
        );
    }

    #[test]
    fn reset_bypasses_to_zero_and_restores_the_held_value() {
        assert_eq!(click_reset(1.5, 0.0), (0.0, 1.5), "reset parks the value");
        assert_eq!(click_reset(0.0, 1.5), (1.5, 1.5), "a second click restores it");
        assert_eq!(click_reset(0.0, 0.0), (0.0, 0.0), "nothing held: the button is inert");
    }
}
