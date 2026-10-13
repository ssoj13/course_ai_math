//! The viewer-backdrop control: a compact swatch button that opens a drop-up menu.
//!
//! The model ([`Backdrop`]) and both renderers live in `egui-display`; this is only
//! the toolbar's way to pick one. It is a stateless control like every other here:
//! it borrows the host's current value for one frame and returns the changed value,
//! and the host persists it.

use egui::{
    CornerRadius, Order, PopupCloseBehavior, RectAlign, Response, Sense, StrokeKind, Ui,
    WidgetInfo, WidgetType, vec2,
};
use egui_display::{Backdrop, BackdropKind, Checker, CheckerScale};

/// Where the menu goes: ABOVE the button first, because the toolbar sits at the
/// bottom of a viewport; below when there is no room above (a toolbar on the top
/// edge, or a short window); then the same pair right-aligned.
const ALIGNS: [RectAlign; 3] = [
    RectAlign::BOTTOM_START,
    RectAlign::TOP_END,
    RectAlign::BOTTOM_END,
];

/// Draw the backdrop button for `current`. Clicking it opens the menu: Checker (with
/// its cell size and scale), Black, 50% Gray and Custom (an inline swatch opening
/// the `egui-colorpicker` picker, in display space).
///
/// Returns the new backdrop on the frame the user changed it, else `None`; persist
/// it then. Choosing Black or 50% Gray closes the menu; Checker and Custom keep it
/// open for their parameters. A click outside the menu (and outside the colour
/// picker it opened) or Esc closes it.
pub fn backdrop_picker(ui: &mut Ui, current: &Backdrop) -> Option<Backdrop> {
    let side = ui.spacing().interact_size.y;
    let (rect, button) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let label = format!("Backdrop: {}", current.label());
    button.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), &label));
    paint_swatch(ui, rect, current, &button);
    let button = button.on_hover_text(format!("{label}\nClick to change"));

    let mut next = *current;
    let menu_id = button.id.with("backdrop_menu");
    let open_id = menu_id.with("open");
    let mut open = MenuOpen::load(ui.ctx(), open_id);
    if button.clicked() {
        open = !open;
    }
    let popup = egui::Popup::from_response(&button)
        .id(menu_id)
        .kind(egui::PopupKind::Menu)
        .open_bool(&mut open)
        .align(RectAlign::TOP_START)
        .align_alternatives(&ALIGNS)
        .close_behavior(PopupCloseBehavior::IgnoreClicks)
        .show(|ui| menu(ui, &mut next));
    if let Some(menu) = popup {
        // `IgnoreClicks` plus this, not `CloseOnClickOutside`: the colour picker's
        // popup is a separate area, so egui would count a click inside it as outside
        // this menu and close both. Any click on a layer below the popups is outside.
        // Read the click first: `layer_id_at` takes the context lock `input` holds.
        let click = ui.input(|i| i.pointer.any_click().then(|| i.pointer.interact_pos()));
        let outside = click.flatten().is_some_and(|p| {
            ui.ctx()
                .layer_id_at(p)
                .is_none_or(|layer| layer.order < Order::Foreground)
        });
        // The click that opened the menu is on the button, below the popups: not a close.
        if menu.inner || (outside && !button.clicked()) {
            open = false;
        }
    }
    MenuOpen::store(ui.ctx(), open_id, open);
    (next != *current).then_some(next)
}

/// Whether the menu is open, kept in egui's temp data rather than egui's popup memory:
/// that memory holds ONE open popup per viewport, so the colour picker opening its own
/// popup from inside this menu would close the menu. Remembers the pass it was last
/// drawn on, so a control that went unshown (a collapsed toolbar) comes back closed.
#[derive(Clone, Copy)]
struct MenuOpen {
    open: bool,
    pass: u64,
}

impl MenuOpen {
    fn load(ctx: &egui::Context, id: egui::Id) -> bool {
        let now = ctx.cumulative_pass_nr();
        ctx.data(|d| d.get_temp::<Self>(id))
            .is_some_and(|m| m.open && m.pass + 1 >= now)
    }

    fn store(ctx: &egui::Context, id: egui::Id, open: bool) {
        let pass = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(id, Self { open, pass }));
    }
}

/// The menu body. Returns whether the choice made should close the menu.
fn menu(ui: &mut Ui, b: &mut Backdrop) -> bool {
    ui.set_min_width(180.0);
    let mut close = false;
    for kind in BackdropKind::ALL {
        let item = format!("{}  {}", kind.icon(), kind.label());
        if ui.selectable_label(b.kind == kind, item).clicked() {
            b.kind = kind;
            close = matches!(kind, BackdropKind::Black | BackdropKind::Gray50);
        }
        match kind {
            BackdropKind::Checker => checker_controls(ui, b),
            BackdropKind::Color if b.kind == BackdropKind::Color => {
                ui.horizontal(|ui| {
                    ui.add_space(ui.spacing().indent);
                    ui.label("Colour");
                    let mut cfg = egui_colorpicker::PickerConfig::default().with_alpha(false);
                    egui_colorpicker::color_button_with(ui, &mut b.color, &mut cfg);
                });
            }
            _ => {}
        }
    }
    close
}

/// Cell size and scale, under the Checker item. Editing them selects the checker:
/// tuning a checker that is not shown would change nothing visible.
fn checker_controls(ui: &mut Ui, b: &mut Backdrop) {
    let mut checker = b.checker;
    ui.horizontal(|ui| {
        ui.add_space(ui.spacing().indent);
        ui.add(
            egui::Slider::new(&mut checker.size_px, Checker::SIZE)
                .step_by(1.0)
                .suffix(" px")
                .text("Cell"),
        );
    });
    ui.horizontal(|ui| {
        ui.add_space(ui.spacing().indent);
        let mut zooms = checker.scale == CheckerScale::Image;
        ui.checkbox(&mut zooms, "Zoom with image")
            .on_hover_text("Off: cells are screen pixels at any zoom. On: image pixels.");
        checker.scale = if zooms {
            CheckerScale::Image
        } else {
            CheckerScale::Screen
        };
    });
    if checker != b.checker {
        b.checker = checker;
        b.kind = BackdropKind::Checker;
    }
}

/// The button face: a mini preview of the backdrop (2x2 checker cells or the solid
/// colour) in a frame that follows the theme's hover state.
fn paint_swatch(ui: &Ui, rect: egui::Rect, backdrop: &Backdrop, response: &Response) {
    let visuals = ui.style().interact(response);
    let painter = ui.painter();
    let r = rect.shrink(3.0);
    let radius = 2u8;
    let [a, b] = backdrop.swatch();
    painter.rect_filled(r, radius, a);
    if a != b {
        // The B cells: top-right and bottom-left quarters, rounded only on their
        // outer corner so they sit inside the frame's rounding.
        let h = r.size() / 2.0;
        let top_right = egui::Rect::from_min_size(r.min + vec2(h.x, 0.0), h);
        let bottom_left = egui::Rect::from_min_size(r.min + vec2(0.0, h.y), h);
        let corner = |ne: u8, sw: u8| CornerRadius {
            nw: 0,
            ne,
            sw,
            se: 0,
        };
        painter.rect_filled(top_right, corner(radius, 0), b);
        painter.rect_filled(bottom_left, corner(0, radius), b);
    }
    // `interact` already reflects hover, press and keyboard focus.
    painter.rect_stroke(r, radius, visuals.fg_stroke, StrokeKind::Outside);
}
