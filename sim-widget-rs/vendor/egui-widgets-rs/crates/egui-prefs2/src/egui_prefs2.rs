//! A two-pane Settings/Preferences panel for egui — the vfx-view "Settings" window look, as a
//! host-agnostic reusable widget.
//!
//! Layout (copied from vfx-view `panels/preferences.rs`):
//! - LEFT: a fixed 150px category sidebar. Each row is `icon  label`; the selected row gets an
//!   accent bar on its left edge and a filled background ([`sidebar_item`]).
//! - RIGHT: a scrollable content column with a big-icon category header ([`category_header`]),
//!   the category's body (sections built with [`section_grid`]), and a right-aligned
//!   "Reset to Defaults" [`footer`].
//!
//! Fonts/spacing are copied verbatim (body 11.0, heading 13.5, sidebar 11.0; split gap 3px;
//! grid min-col 130 / spacing [12,6]) so adopting apps match vfx-view pixel-for-pixel.
//!
//! # Usage (v0.1, closure-driven)
//! The host owns its settings and supplies (a) the category list and (b) a body renderer that
//! draws the selected category's widgets:
//! ```ignore
//! egui_prefs2::draw(ui, &mut state, &cats, |ui, idx| match idx {
//!     0 => { /* General widgets */ }
//!     1 => { /* Viewport widgets */ }
//!     _ => {}
//! }, Some(|| { *settings = Default::default(); }));
//! ```
//!
//! [`PrefsPanelState`] is serde-serializable, so the selected category + fonts persist across
//! runs (the host saves/loads it however it persists the rest of its config).
//!
//! ## Data-driven layer (v0.2)
//! The [`schema`] module builds on these primitives: a serde [`PrefsSchema`] (categories →
//! sections → typed [`FieldSpec`]s) is rendered by [`draw_schema`] against a `box-rs` value store
//! addressed by path — declare preferences as data instead of code, and get the filled
//! [`RustBox`] out for persistence. [`Widget::Custom`] is the escape hatch for bespoke controls.

use egui::{Response, Ui};

/// v0.2 data-driven layer: a serde [`schema::PrefsSchema`] (categories -> sections -> typed fields)
/// rendered against a `box-rs` value store. Built on the v0.1 primitives below.
mod schema;
pub use schema::{
    CustomRenderers, FieldSpec, PrefsSchema, SchemaCategory, Section, Widget, draw_schema,
};
// Re-export the box-rs value types the data-driven API traffics in, so a host can build schema
// defaults and read filled values (`egui_prefs2::{RustBox, BoxValue}`) without a direct box-rs dep.
pub use box_rs::{BoxValue, RustBox};

/// Font sizes for the panel (points). Defaults copied from vfx-view.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PrefsFonts {
    /// Category sidebar row font (vfx-view `sidebar_size`).
    pub sidebar: f32,
    /// Right-pane category header font (vfx-view `heading_size`).
    pub heading: f32,
    /// Section body text (informational — hosts also set egui's body TextStyle to match).
    pub body: f32,
}

impl Default for PrefsFonts {
    fn default() -> Self {
        Self {
            sidebar: 11.0,
            heading: 13.5,
            body: 11.0,
        }
    }
}

/// Layout measurements for the panel (points).
///
/// The counterpart to [`PrefsFonts`]: the sizes a host may reasonably disagree with, in one
/// place instead of scattered through the drawing code as literals. A host that sets none of
/// them gets the defaults, and one that has its own idea of density sets `row_gap` — or
/// simply sets `item_spacing.y` on the ui and leaves `row_gap` at `None`, which the panel
/// then inherits like every other widget in the app.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PrefsMetrics {
    /// Width of the category sidebar. Dragged by the splitter beside it.
    pub sidebar_width: f32,
    /// Width of the label column. Dragged by the splitter between the names and their
    /// controls.
    pub label_column: f32,
    /// Gap between the label column and the widget column.
    ///
    /// Not a knob worth exposing to a user — it is the breathing room either side of the
    /// splitter, and a large value simply looks broken. Hosts that persist [`PrefsMetrics`]
    /// should leave it alone and let the splitter move `label_column` instead.
    pub column_gap: f32,
    /// Gap between rows. `None` follows the host's `item_spacing.y`, which is the reason a
    /// host usually needs to set nothing here at all.
    pub row_gap: Option<f32>,
    /// Height reserved for the "Reset to Defaults" footer.
    pub footer_height: f32,
}

impl Default for PrefsMetrics {
    fn default() -> Self {
        Self {
            sidebar_width: 150.0,
            label_column: 130.0,
            column_gap: 12.0,
            row_gap: None,
            footer_height: 48.0,
        }
    }
}

impl PrefsMetrics {
    /// The row gap in force for `ui`: the override, or the host's own spacing.
    fn row_gap(&self, ui: &Ui) -> f32 {
        self.row_gap.unwrap_or(ui.spacing().item_spacing.y)
    }

    /// Keeps the dragged widths usable: a column narrow enough to hide every label, or wide
    /// enough to push the controls off the panel, is not a layout the user meant to ask for.
    fn clamped(mut self, available: f32) -> Self {
        let ceiling = (available * 0.6).max(MIN_PANE);
        self.sidebar_width = self.sidebar_width.clamp(MIN_PANE, ceiling);
        self.label_column = self.label_column.clamp(MIN_PANE, ceiling);
        self.column_gap = self.column_gap.clamp(0.0, 24.0);
        self
    }
}

/// Narrowest a dragged pane may become, and the floor for the ceiling that caps them.
const MIN_PANE: f32 = 48.0;

fn metrics_id() -> egui::Id {
    egui::Id::new("egui_prefs2_metrics")
}

/// The metrics [`draw`] is currently running with.
///
/// Published through the context rather than threaded through every helper's signature:
/// [`section_grid`] and friends are public and already called by other crates, and a widget
/// reading its own style out of the ui is the egui-shaped way to do this.
fn metrics(ui: &Ui) -> PrefsMetrics {
    ui.ctx()
        .data(|d| d.get_temp::<PrefsMetrics>(metrics_id()))
        .unwrap_or_default()
}

/// Persistent panel state: the open category index, the fonts and the layout metrics.
/// Serialize it alongside the host's config so the panel reopens where the user left it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PrefsPanelState {
    /// Index of the selected category (into the `categories` slice passed to [`draw`]).
    pub selected: usize,
    /// Font sizes.
    pub fonts: PrefsFonts,
    /// Layout measurements.
    #[serde(default)]
    pub metrics: PrefsMetrics,
}

/// A sidebar category entry: a small `icon` glyph and a `label`.
#[derive(Clone, Copy, Debug)]
pub struct Category<'a> {
    pub icon: &'a str,
    pub label: &'a str,
}

impl<'a> Category<'a> {
    pub fn new(icon: &'a str, label: &'a str) -> Self {
        Self { icon, label }
    }
}

/// Draw the two-pane Settings panel into `ui`.
///
/// - `categories`: sidebar entries in display order.
/// - `body`: fills the right content pane for the selected category `idx` (build sections with
///   [`section_grid`]).
/// - `reset`: optional "Reset to Defaults" handler; when `None` the footer is omitted.
///
/// The selected index is clamped into range and kept in `state`.
pub fn draw(
    ui: &mut Ui,
    state: &mut PrefsPanelState,
    categories: &[Category<'_>],
    mut body: impl FnMut(&mut Ui, usize),
    reset: Option<impl FnMut()>,
) {
    if categories.is_empty() {
        return;
    }
    if state.selected >= categories.len() {
        state.selected = 0;
    }
    let has_reset = reset.is_some();
    let m = state.metrics.clamped(ui.available_width());
    state.metrics = m;
    ui.ctx().data_mut(|d| d.insert_temp(metrics_id(), m));
    let mut sidebar_drag = 0.0;

    ui.horizontal_top(|ui| {
        // Tighten the gap on both sides of the vertical split so the sidebar sits close to the
        // content column (a narrower split bar) — matches vfx-view.
        ui.spacing_mut().item_spacing.x = 3.0;

        // ── Sidebar (fixed, no scroll) ────────────────────────────────────────
        ui.vertical(|ui| {
            ui.set_width(m.sidebar_width);
            ui.add_space(m.row_gap(ui));
            for (i, c) in categories.iter().enumerate() {
                if sidebar_item(
                    ui,
                    state.selected == i,
                    c.icon,
                    c.label,
                    state.fonts.sidebar,
                )
                .clicked()
                {
                    state.selected = i;
                }
            }
        });

        // The divider is the control. A width like this is a thing the user points at and
        // drags; offering it as a number in the panel it lays out was the wrong instrument,
        // and one large enough to look broken is one drag away either direction.
        let bar = ui.available_rect_before_wrap();
        let hit = egui_widgets_config::splitter::interact(
            ui,
            bar,
            bar.left() + SPLITTER_W / 2.0,
            ui.id().with("egui_prefs2_sidebar_splitter"),
            SPLITTER_W / 2.0,
            0.0,
            &egui_widgets_config::WidgetsConfig::get(ui).metrics,
        );
        if let Some(delta) = hit.delta {
            sidebar_drag = delta;
        }
        if hit.double_clicked {
            sidebar_drag = f32::NAN;
        }
        ui.add_space(SPLITTER_W);

        // ── Content column ────────────────────────────────────────────────────
        ui.vertical(|ui| {
            // Synchronize the whole content pane to the panel's body font so section
            // headers, grid labels, comboboxes and sliders all render at one size and
            // match the sidebar. vfx-view gets this for free because its app-wide Body
            // TextStyle is already small; as a standalone crate we must set it here or
            // the content inherits the host's (larger) global Body and dwarfs the sidebar.
            sync_body_font(ui, state.fonts.body);
            // Reserve exactly the footer's height so `auto_shrink=false` doesn't grow the
            // container each frame (the footer competes with the ScrollArea for space otherwise).
            let reserve = if has_reset { m.footer_height } else { 0.0 };
            let scroll_max_h = (ui.available_height() - reserve).max(120.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(scroll_max_h)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.add_space(m.row_gap(ui));
                    let c = categories[state.selected];
                    category_header(ui, c.icon, c.label, state.fonts.heading);
                    body(ui, state.selected);
                });
            if let Some(reset) = reset {
                footer(ui, reset);
            }
        });
    });

    if sidebar_drag.is_nan() {
        state.metrics.sidebar_width = PrefsMetrics::default().sidebar_width;
    } else if sidebar_drag != 0.0 {
        state.metrics.sidebar_width += sidebar_drag;
    }
    // A drag inside the grid is reported through the context, because the rows are drawn by
    // free functions that never see the state.
    if let Some(width) = taken_label_width(ui) {
        state.metrics.label_column = width;
    }
}

/// Width of a splitter's lane. Wide enough to grab, narrow enough not to read as a gutter.
const SPLITTER_W: f32 = 6.0;

fn label_width_id() -> egui::Id {
    egui::Id::new("egui_prefs2_label_width")
}

/// Consumes the label-column width a [`section_grid`] splitter was dragged to this frame.
fn taken_label_width(ui: &Ui) -> Option<f32> {
    ui.ctx()
        .data_mut(|d| d.remove_temp::<f32>(label_width_id()))
}

/// Force the `Body` and `Button` text styles of `ui` (inherited by all its children) to `size`
/// points, so a section's labels, sliders, comboboxes and checkboxes render at the panel's body
/// size regardless of the host's global styling. Keeps the panel internally consistent.
fn sync_body_font(ui: &mut Ui, size: f32) {
    let styles = &mut ui.style_mut().text_styles;
    for ts in [egui::TextStyle::Body, egui::TextStyle::Button] {
        if let Some(f) = styles.get_mut(&ts) {
            f.size = size;
        }
    }
}

/// A left-panel sidebar row: `icon  label` with a hover/selected background fill and, when
/// selected, a 3px accent bar on the left edge. Returns the row [`Response`] (check `.clicked()`).
pub fn sidebar_item(
    ui: &mut Ui,
    selected: bool,
    icon: &str,
    label: &str,
    font_size: f32,
) -> Response {
    // Row height tracks the font size tightly (small padding, low floor) so shrinking the font
    // packs the list denser vertically instead of staying spread out.
    let row_h = (font_size + 5.0).max(14.0);
    let desired = egui::vec2(ui.available_width(), row_h);
    let (rect, resp) = ui.allocate_exact_size(desired, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&resp, selected);
        if resp.hovered() || selected {
            ui.painter()
                .rect_filled(rect.expand(1.0), 3.0, visuals.bg_fill);
        }
        if selected {
            let accent = ui.visuals().selection.bg_fill;
            let bar = egui::Rect::from_min_size(rect.left_top(), egui::vec2(3.0, rect.height()));
            ui.painter().rect_filled(bar, 0.0, accent);
        }
        let galley = ui.painter().layout_no_wrap(
            format!("{icon}  {label}"),
            egui::FontId::proportional(font_size),
            visuals.text_color(),
        );
        let text_pos = egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y * 0.5);
        ui.painter().galley(text_pos, galley, visuals.text_color());
    }
    resp
}

/// Right-pane header: a large accent-colored `icon` + bold `label` at `heading_size`, then a
/// separator. The icon is drawn slightly larger than the label for hierarchy.
pub fn category_header(ui: &mut Ui, icon: &str, label: &str, heading_size: f32) {
    let icon_sz = heading_size * (15.0 / 13.5);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(icon)
                .size(icon_sz)
                .color(ui.visuals().selection.bg_fill),
        );
        ui.add_space(metrics(ui).column_gap / 2.0);
        ui.label(egui::RichText::new(label).size(heading_size).strong());
    });
    ui.separator();
    ui.add_space(metrics(ui).row_gap(ui));
}

/// Section header — a small, dim label that introduces a group of rows. Scales with the egui
/// Body text style so it follows the host's body-size setting. Copied from vfx-view.
pub fn section_header(ui: &mut Ui, title: &str) {
    // Enough to separate one group from the last row of the previous one, scaled to the
    // panel's own rhythm rather than fixed at a size that reads as a gulf in a dense panel.
    let gap = metrics(ui).row_gap(ui);
    ui.add_space(gap * 2.0 + 2.0);
    let sz = ui
        .style()
        .text_styles
        .get(&egui::TextStyle::Body)
        .map(|f| f.size)
        .unwrap_or(11.0);
    let text = egui::RichText::new(title)
        .size(sz)
        .color(ui.visuals().widgets.noninteractive.text_color());
    ui.label(text);
    ui.add_space(gap);
}

/// A two-column grid for a settings section. Add rows inside the closure with
/// `ui.label("Name"); <widget>; ui.end_row();`.
///
/// Row spacing follows the host's `item_spacing.y` rather than a literal of its own: an app
/// that has decided how dense its rows are should not find one panel disagreeing, and a host
/// that has no opinion still inherits egui's default through the same field. The column gap
/// stays fixed — it is a two-column layout's own measurement, not the app's line rhythm.
// egui 0.35: `Grid::new` takes `impl AsIdSalt`, which now additionally requires `Debug`.
pub fn section_grid<R>(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug + Copy,
    add_rows: impl FnOnce(&mut Ui) -> R,
) -> R {
    let m = metrics(ui);
    let left = ui.min_rect().left();
    // Derived from the caller's grid id, not from the parent `Ui`: several sections are drawn
    // into the same `Ui`, so a fixed salt would give every one of their splitters the same
    // id and egui would report the clash (and hand them each other's drag).
    let splitter_id = ui.id().with(("egui_prefs2_label_splitter", &id));
    let out = egui::Grid::new(id)
        .num_columns(2)
        // The label column is *exactly* this wide, not merely at least: a minimum lets the
        // longest label in the section decide where every control starts, and a splitter the
        // user drags has to land where they dropped it.
        .min_col_width(m.label_column)
        .max_col_width(m.label_column)
        .spacing([m.column_gap, m.row_gap(ui)])
        .show(ui, add_rows);

    // One handle for the whole section, on the boundary the columns share.
    let rect = out.response.rect;
    let x = left + m.label_column + m.column_gap / 2.0;
    let hit = egui_widgets_config::splitter::interact(
        ui,
        rect,
        x,
        splitter_id,
        SPLITTER_W / 2.0,
        0.0,
        &egui_widgets_config::WidgetsConfig::get(ui).metrics,
    );
    let dragged = if hit.double_clicked {
        Some(PrefsMetrics::default().label_column)
    } else {
        hit.delta.map(|d| m.label_column + d)
    };
    if let Some(width) = dragged {
        ui.ctx()
            .data_mut(|d| d.insert_temp(label_width_id(), width));
    }
    out.inner
}

/// Right-pane footer: a separator + a right-aligned "Reset to Defaults" button that calls `reset`.
pub fn footer(ui: &mut Ui, mut reset: impl FnMut()) {
    let gap = metrics(ui).row_gap(ui);
    // A custom body may grow its parent's layout beyond the pane. Anchor the footer to
    // the visible pane, so its right edge never follows an oversized child.
    let rect = ui.available_rect_before_wrap().intersect(ui.clip_rect());
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.add_space(gap * 4.0);
        ui.separator();
        ui.add_space(gap);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new("Reset to Defaults").truncate())
                .on_hover_text("Reset to Defaults")
                .clicked()
            {
                reset();
            }
        });
    });
}
