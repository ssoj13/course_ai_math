//! Full-width tinted title bar + optional collapsing section for egui.
//!
//! Hermetic chrome: the caller supplies label, open/closed state, icons, and a
//! colour-coding tint (`Color32` + mix amount 0..=1). No app types.
//!
//! Two layers:
//! - [`TitleBar`] — paints the strip and reports clicks; body visibility is the caller's.
//! - [`CollapsingSection`] — [`TitleBar`] plus show/hide of a body closure.

use egui::{Color32, Id, Response, Sense, Ui, UiBuilder, Vec2};
use egui_widgets_config::{WidgetsConfig, icons, prop};

/// Response from [`TitleBar::show`].
#[derive(Clone)]
pub struct TitleBarResponse {
    /// Underlying egui response for the full-width bar (click / hover).
    pub response: Response,
    /// Open state after this frame's click (toggled if the bar was clicked).
    pub open: bool,
}

impl TitleBarResponse {
    /// Convenience: the bar was clicked this frame.
    pub fn clicked(&self) -> bool {
        self.response.clicked()
    }
}

/// Full-width section title bar.
///
/// ```ignore
/// let mut open = true;
/// let r = TitleBar::new("Layer")
///     .open(open)
///     .id_salt("ae_section_layer")
///     .tint(Color32::from_rgb(64, 128, 200), 0.25)
///     .show(ui);
/// open = r.open;
/// ```
pub struct TitleBar<'a> {
    label: &'a str,
    open: bool,
    id_salt: Id,
    tint: Color32,
    tint_amount: f32,
    icon_open: &'a str,
    icon_closed: &'a str,
}

impl<'a> TitleBar<'a> {
    /// New bar with default caret icons; no tint until [`Self::tint`] is set.
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            open: true,
            id_salt: Id::new(label),
            tint: Color32::TRANSPARENT,
            tint_amount: 0.0,
            icon_open: icons::CARET_DOWN,
            icon_closed: icons::CARET_RIGHT,
        }
    }

    /// Whether the section is currently open (selects the open vs closed icon).
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Stable id salt so multiple bars with the same label don't collide.
    pub fn id_salt(mut self, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.id_salt = Id::new(salt);
        self
    }

    /// Colour-coding tint mixed into the bar background.
    ///
    /// `amount` is clamped to `0..=1`. `0` = panel fill only; `1` = solid `color`.
    pub fn tint(mut self, color: Color32, amount: f32) -> Self {
        self.tint = color;
        self.tint_amount = amount.clamp(0.0, 1.0);
        self
    }

    /// Override the open / closed icons (phosphor glyph strings by default).
    pub fn icons(mut self, icon_open: &'a str, icon_closed: &'a str) -> Self {
        self.icon_open = icon_open;
        self.icon_closed = icon_closed;
        self
    }

    /// Allocate the full-width bar, paint it, return click + toggled open state.
    pub fn show(self, ui: &mut Ui) -> TitleBarResponse {
        ui.push_id(self.id_salt, |ui| {
            let cfg = WidgetsConfig::get(ui);
            let height = cfg.metrics.row_h.max(ui.spacing().interact_size.y);
            let width = ui.available_width();
            let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

            let base = ui.visuals().widgets.inactive.bg_fill;
            let fill = if self.tint_amount <= 0.0 || self.tint.a() == 0 {
                base
            } else {
                base.lerp_to_gamma(self.tint, self.tint_amount)
            };
            let fill = if response.hovered() {
                fill.lerp_to_gamma(ui.visuals().widgets.hovered.bg_fill, 0.35)
            } else {
                fill
            };

            ui.painter().rect_filled(rect, cfg.metrics.radius_sm, fill);

            let icon = if self.open {
                self.icon_open
            } else {
                self.icon_closed
            };
            let fg = ui.visuals().widgets.inactive.fg_stroke.color;
            let font = prop(cfg.fonts.sm);
            let pad = cfg.metrics.space_sm;
            let icon_pos = egui::pos2(rect.min.x + pad, rect.center().y);
            let icon_galley = ui.painter().layout_no_wrap(icon.to_owned(), font, fg);
            let icon_size = icon_galley.size();
            ui.painter().galley(
                egui::pos2(icon_pos.x, icon_pos.y - icon_size.y * 0.5),
                icon_galley,
                fg,
            );

            let label_x = icon_pos.x + icon_size.x + pad;
            let label_font = prop(cfg.fonts.sm);
            let label_galley = ui
                .painter()
                .layout_no_wrap(self.label.to_owned(), label_font, fg);
            let label_size = label_galley.size();
            ui.painter().galley(
                egui::pos2(label_x, rect.center().y - label_size.y * 0.5),
                label_galley,
                fg,
            );

            let open = if response.clicked() {
                !self.open
            } else {
                self.open
            };

            TitleBarResponse { response, open }
        })
        .inner
    }
}

/// Response from [`CollapsingSection::show`].
pub struct CollapsingSectionResponse<R> {
    /// Title-bar response (click / hover / toggled open).
    pub header: TitleBarResponse,
    /// Body return value when the section was open; `None` when collapsed.
    pub body_returned: Option<R>,
}

/// Title bar + show/hide body — drop-in for a colour-coded `CollapsingHeader`.
///
/// Open state is owned by the caller (pass [`CollapsingSection::open`]); a click
/// on the bar toggles and the new value is in [`CollapsingSectionResponse::header`].
pub struct CollapsingSection<'a> {
    bar: TitleBar<'a>,
}

impl<'a> CollapsingSection<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            bar: TitleBar::new(label),
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.bar = self.bar.open(open);
        self
    }

    pub fn id_salt(mut self, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.bar = self.bar.id_salt(salt);
        self
    }

    pub fn tint(mut self, color: Color32, amount: f32) -> Self {
        self.bar = self.bar.tint(color, amount);
        self
    }

    pub fn icons(mut self, icon_open: &'a str, icon_closed: &'a str) -> Self {
        self.bar = self.bar.icons(icon_open, icon_closed);
        self
    }

    /// Paint the title bar; if open, run `add_body` and return its value.
    pub fn show<R>(
        self,
        ui: &mut Ui,
        add_body: impl FnOnce(&mut Ui) -> R,
    ) -> CollapsingSectionResponse<R> {
        let open_before = self.bar.open;
        // Capture before `bar.show` consumes `self.bar`. Body MUST share this salt with
        // the header: without it, two open sections (e.g. AE Item + Layer) both nest
        // under a bare `titlebar_body` id and paint egui C270 duplicate splitters.
        let section_salt = self.bar.id_salt;
        let header = self.bar.show(ui);
        let body_returned = if open_before {
            // Indent body slightly so it reads as belonging under the bar.
            let body = ui
                .push_id(section_salt, |ui| {
                    ui.scope_builder(UiBuilder::new().id_salt("titlebar_body"), |ui| {
                        ui.add_space(WidgetsConfig::get(ui).metrics.space_xs);
                        add_body(ui)
                    })
                    .inner
                })
                .inner;
            Some(body)
        } else {
            None
        };
        CollapsingSectionResponse {
            header,
            body_returned,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tint_amount_is_clamped() {
        let bar = TitleBar::new("x").tint(Color32::RED, 2.5);
        assert!((bar.tint_amount - 1.0).abs() < f32::EPSILON);
        let bar = TitleBar::new("x").tint(Color32::RED, -0.5);
        assert!((bar.tint_amount - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn defaults_use_caret_icons() {
        let bar = TitleBar::new("Layer");
        assert_eq!(bar.icon_open, icons::CARET_DOWN);
        assert_eq!(bar.icon_closed, icons::CARET_RIGHT);
        assert!(bar.open);
    }
}
