//! UI tests of `backdrop_picker` through egui's accessibility tree (egui_kittest):
//! the menu opens ABOVE a bottom-edge button and flips below when there is no room,
//! each item selects its kind, and only a click outside both the menu and the colour
//! picker it opened closes it.

use egui::{Align, Layout, Pos2, Vec2};
use egui_display::{Backdrop, BackdropKind};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use egui_viewport_toolbar::backdrop_picker;

/// A 320x420 screen whose only widget is the picker, editing the harness state, at
/// the bottom-left corner (a bottom toolbar) or the top-left one.
fn harness(at_bottom: bool) -> Harness<'static, Backdrop> {
    Harness::builder()
        .with_size(Vec2::new(320.0, 420.0))
        // Real frame times: the colour picker closes itself 0.5 s after the pointer
        // leaves it, which the default quarter-second step would turn into two frames.
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(
            move |ui, state: &mut Backdrop| {
                let layout = if at_bottom {
                    Layout::bottom_up(Align::Min)
                } else {
                    Layout::top_down(Align::Min)
                };
                ui.with_layout(layout, |ui| {
                    if let Some(changed) = backdrop_picker(ui, state) {
                        *state = changed;
                    }
                });
            },
            Backdrop::default(),
        )
}

/// The accessible label of a menu item.
fn item(kind: BackdropKind) -> String {
    format!("{}  {}", kind.icon(), kind.label())
}

/// The menu is open (its 50% Gray item is in the tree).
fn menu_open(h: &Harness<'_, Backdrop>) -> bool {
    h.query_by_label(&item(BackdropKind::Gray50)).is_some()
}

/// A full click (press + release) at `pos`, then settle with the pointer still there:
/// `drop_at` removes the cursor, and a pointer gone from the colour picker starts its
/// idle-close timer.
fn click_at(h: &mut Harness<'_, Backdrop>, pos: Pos2) {
    h.hover_at(pos);
    h.run();
    h.drag_at(pos);
    h.drop_at(pos);
    h.hover_at(pos);
    h.run();
}

fn open_menu(h: &mut Harness<'_, Backdrop>) {
    h.get_by_label_contains("Backdrop: ").click();
    h.run();
    assert!(menu_open(h), "clicking the button opens the menu");
}

#[test]
fn drop_up_opens_above_the_button_and_selects_each_item() {
    let mut h = harness(true);
    h.run();
    for kind in BackdropKind::ALL {
        if !menu_open(&h) {
            open_menu(&mut h);
        }
        let button = h.get_by_label_contains("Backdrop: ").rect();
        let label = item(kind);
        let node = h.get_by_label(&label);
        assert!(
            node.rect().bottom() <= button.top(),
            "{kind:?} item {:?} must sit above the button {button:?}",
            node.rect()
        );
        node.click();
        h.run();
        assert_eq!(h.state().kind, kind, "clicking {kind:?} selects it");
        assert_eq!(
            h.get_by_label_contains("Backdrop: ").rect(),
            button,
            "the button stays put"
        );
        // Presets close the menu; the kinds with parameters keep it open for them.
        let keeps_open = matches!(kind, BackdropKind::Checker | BackdropKind::Color);
        assert_eq!(
            menu_open(&h),
            keeps_open,
            "{kind:?} leaves the menu open: {keeps_open}"
        );
    }
    // The button names the current kind (the last one picked).
    assert!(h.query_by_label("Backdrop: Custom").is_some());
}

#[test]
fn menu_flips_below_when_there_is_no_room_above() {
    let mut h = harness(false);
    h.run();
    open_menu(&mut h);
    let button = h.get_by_label_contains("Backdrop: ").rect();
    for kind in BackdropKind::ALL {
        let below = h.get_by_label(&item(kind)).rect();
        assert!(
            below.top() >= button.bottom(),
            "{kind:?} item {below:?} must sit below a top-edge button {button:?}"
        );
    }
}

#[test]
fn only_a_click_outside_the_menu_and_its_colour_picker_closes_it() {
    let mut h = harness(true);
    h.run();
    open_menu(&mut h);
    h.get_by_label(&item(BackdropKind::Color)).click();
    h.run();
    assert_eq!(h.state().kind, BackdropKind::Color);

    // The Custom row's swatch sits right after its "Colour" label.
    let label = h.get_by_label("Colour").rect();
    let side = egui::Style::default().spacing.interact_size.y;
    let gap = egui::Style::default().spacing.item_spacing.x;
    let swatch = Pos2::new(label.right() + gap + side * 0.5, label.center().y);
    // Open the picker and move into it, the way a user does.
    h.hover_at(swatch);
    h.run();
    h.drag_at(swatch);
    h.drop_at(swatch);
    h.hover_at(swatch);
    h.step();
    h.step();
    // The picker's "range" caption: inside the picker, and (unlike its buttons) without
    // a tooltip whose delay would keep the harness repainting.
    let picker = h
        .query_by_label("range")
        .map(|n| n.rect())
        .expect("the swatch opens the colour picker");
    h.hover_at(picker.center());
    h.run();
    assert!(
        h.query_by_label("LDR").is_some(),
        "the picker stays open under the pointer"
    );
    assert!(menu_open(&h), "opening the picker keeps the menu");

    // A click inside the picker is not outside the menu.
    click_at(&mut h, picker.center());
    assert!(
        menu_open(&h),
        "a click in the colour picker must not close the menu"
    );

    // A click on empty screen is.
    click_at(&mut h, Pos2::new(300.0, 10.0));
    assert!(!menu_open(&h), "a click outside closes the menu");
    assert_eq!(
        h.state().kind,
        BackdropKind::Color,
        "closing changes nothing"
    );
}

#[test]
fn checker_controls_select_the_checker() {
    let mut h = harness(true);
    h.state_mut().kind = BackdropKind::Black;
    h.run();
    open_menu(&mut h);
    h.get_by_label("Zoom with image").click();
    h.run();
    let state = *h.state();
    assert_eq!(
        state.kind,
        BackdropKind::Checker,
        "tuning the checker shows it"
    );
    assert_eq!(state.checker.scale, egui_display::CheckerScale::Image);
}
