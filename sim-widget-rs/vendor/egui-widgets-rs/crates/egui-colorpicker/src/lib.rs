//! Maya-style HDR color picker popup for egui.
//!
//! The crate exports two things:
//!
//! - [`color_button`] / [`color_button_with`] — the swatch button you
//!   drop into a row; clicking opens the picker popup.
//! - [`PickerConfig`] — host-side knobs: HDR slider cap, alpha
//!   toggle, display-space transform (default sRGB OETF, but any
//!   linear→display function works — pass your tonemap here), and
//!   an optional eyedropper callback.
//!
//! Picker contents (Maya-style):
//!
//! * Two preview rectangles at the top: **working** (raw linear
//!   sRGB) vs **display** (after `display_transform`).
//! * A 2D Hue × Saturation gradient + vertical Value slider that
//!   honours the HDR cap, so emissive materials can pull V > 1.0.
//! * Side panel with RGB (linear + sRGB columns), HSV, hex, alpha
//!   rows. Editing any field updates the others in real time.
//! * Color history strip (12 slots, deduped, persisted via
//!   `egui::Memory.data` so it survives panel layouts).
//! * Eyedropper button shown only when the host installed a
//!   callback — the picker invokes the callback; the host wires it
//!   to a viewport-sample tool.
//!
//! All channels are stored linearly internally and exposed via the
//! `[f32; 4]` mutable handle. HDR values (any channel > 1.0) flow
//! through unchanged; the sRGB column clamps to `[0, 1]` because
//! sRGB encoding is undefined above 1.0.

use egui::{Color32, Mesh, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};
use serde::{Deserialize, Serialize};

mod color;
pub use color::{
    hex_to_linear, hsv_to_rgb, linear_to_hex, linear_to_srgb, rgb_to_hsv, srgb_to_linear,
};

/// Maximum number of past colors kept by the history strip.
pub const HISTORY_SIZE: usize = 12;

/// Picker configuration. Build with `PickerConfig::default()` then
/// tweak fields, or use the builder methods.
pub struct PickerConfig {
    /// Upper bound for the HDR slider lane (R/G/B/V linear). Values
    /// can still go higher via hex / direct edit; the slider just
    /// won't drag past this.
    pub hdr_max: f32,
    /// Show the alpha row in the popup.
    pub alpha_enabled: bool,
    /// Map a linear RGB triple to its on-screen "display" version.
    /// Default = sRGB OETF (per-channel gamma). Replace with your
    /// project's tonemap to keep the display preview honest.
    pub display_transform: Box<dyn Fn([f32; 3]) -> [f32; 3]>,
    /// Optional eyedropper hook. When `Some`, the popup shows an
    /// "Eyedropper" button; clicking it invokes the callback. The
    /// callback is expected to start a viewport-sample interaction;
    /// the actual color update happens later, when the host writes
    /// back into the `&mut [f32; 4]` handle on the next frame.
    pub eyedropper: Option<Box<dyn FnMut()>>,
    /// `true` while the host is in eyedropper-sampling mode — the
    /// popup's idle-auto-close is suppressed so the user can move
    /// the mouse off into the viewport, click a sample, and come
    /// back without the popup vanishing.
    pub eyedropper_sampling: bool,
}

impl Default for PickerConfig {
    fn default() -> Self {
        Self {
            // LDR display-space picker: the V / R / G / B lanes span 0..1, so the
            // whole bar is the usable range. (Was 4.0 — which put LDR white at
            // ~25 % of the V bar and sent everything above it into HDR blow-out.)
            hdr_max: 1.0,
            alpha_enabled: true,
            display_transform: Box::new(|c| {
                [
                    linear_to_srgb(c[0]),
                    linear_to_srgb(c[1]),
                    linear_to_srgb(c[2]),
                ]
            }),
            eyedropper: None,
            eyedropper_sampling: false,
        }
    }
}

impl PickerConfig {
    pub fn with_hdr_max(mut self, max: f32) -> Self {
        self.hdr_max = max;
        self
    }
    pub fn with_alpha(mut self, enabled: bool) -> Self {
        self.alpha_enabled = enabled;
        self
    }
    pub fn with_display_transform(mut self, f: impl Fn([f32; 3]) -> [f32; 3] + 'static) -> Self {
        self.display_transform = Box::new(f);
        self
    }
    pub fn with_eyedropper(mut self, f: impl FnMut() + 'static) -> Self {
        self.eyedropper = Some(Box::new(f));
        self
    }
}

/// History entries — the bar of last-used colors at the bottom of
/// the popup. Persisted via `egui::Memory.data` so it survives
/// panel layouts.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct ColorHistory {
    pub entries: Vec<[f32; 4]>,
}

impl ColorHistory {
    fn push(&mut self, c: [f32; 4]) {
        let eq = |a: [f32; 4], b: [f32; 4]| {
            (a[0] - b[0]).abs() < 1e-4
                && (a[1] - b[1]).abs() < 1e-4
                && (a[2] - b[2]).abs() < 1e-4
                && (a[3] - b[3]).abs() < 1e-4
        };
        self.entries.retain(|e| !eq(*e, c));
        self.entries.insert(0, c);
        self.entries.truncate(HISTORY_SIZE);
    }
}

fn history_id() -> egui::Id {
    egui::Id::new("egui_colorpicker_history_v1")
}

fn load_history(ctx: &egui::Context) -> ColorHistory {
    ctx.memory_mut(|m| {
        m.data
            .get_persisted::<ColorHistory>(history_id())
            .unwrap_or_default()
    })
}

fn save_history(ctx: &egui::Context, h: ColorHistory) {
    ctx.memory_mut(|m| m.data.insert_persisted(history_id(), h));
}

/// Right margin (points) that [`ColorRow::default`] (and so [`color_edit_rgba`] /
/// [`color_edit_rgba_ex`]) keeps free at the end of the row, so a trailing swatch clears the
/// splitter line a host draws at the cell edge. Hosts whose cell already ends at a gutter (the
/// attribute grid) use [`ColorRow::flush`] instead: the swatch then ends AT the edge, the same edge
/// as a slider's value box.
pub const EDITOR_MARGIN: f32 = 12.0;

/// Default number of decimals SHOWN in the channel fields (display only).
pub const COLOR_DECIMALS_DEFAULT: usize = 3;

/// Smallest swatch side (px): below it the colour chip stops being clickable.
const SWATCH_MIN: f32 = 14.0;

/// Drop-in swatch button using the default [`PickerConfig`]. Use
/// when the host doesn't need to override any picker behaviour.
pub fn color_button(ui: &mut Ui, color: &mut [f32; 4]) -> Response {
    color_button_with(ui, color, &mut PickerConfig::default())
}

/// Like [`color_button`] but for an `egui::Color32` (sRGBA u8) — converts to/from the
/// linear `[f32;4]` the picker stores. Lets `Color32`-typed callers (e.g. egui-gizmo's
/// `GizmoVisuals`) use the rich picker instead of egui's bare `color_edit_button_srgba`.
///
/// `egui::Rgba` is PREMULTIPLIED linear: `Rgba::from(Color32)` applies the sRGB→linear
/// OETF, and `Rgba::from_rgba_premultiplied(..).into()` applies linear→sRGB. For a
/// fully-opaque colour (`a == 1.0`) premultiplied == unmultiplied, so the round-trip
/// leaves an opaque colour unchanged — the variant the gizmo's solid handle colours need.
pub fn color_button_srgba(ui: &mut Ui, color: &mut Color32) -> Response {
    let mut lin = egui::Rgba::from(*color).to_array(); // sRGBA Color32 -> premultiplied linear [f32;4]
    let r = color_button(ui, &mut lin);
    if r.changed() {
        *color = egui::Rgba::from_rgba_premultiplied(lin[0], lin[1], lin[2], lin[3]).into();
    }
    r
}

/// The layout of a colour row ([`color_edit`]): the ONE implementation behind
/// [`color_edit_rgba`], [`color_edit_rgba_ex`], [`color_edit_rgb`] and [`color_edit_flush`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorRow {
    /// Numeric channel fields: 3 (RGB, alpha untouched) or 4 (RGBA).
    pub channels: usize,
    /// Fractional digits SHOWN in each field; the stored value is never rounded.
    pub decimals: usize,
    /// The swatch before the fields instead of after them.
    pub swatch_left: bool,
    /// Some(margin): the fields share the available width, the row ending `margin` before the
    /// cell edge. None: fields of their natural width, left-packed.
    pub fill_margin: Option<f32>,
}

impl Default for ColorRow {
    /// The classic Nuke-style row: four fields filling the cell up to [`EDITOR_MARGIN`].
    fn default() -> Self {
        Self {
            channels: 4,
            decimals: COLOR_DECIMALS_DEFAULT,
            swatch_left: false,
            fill_margin: Some(EDITOR_MARGIN),
        }
    }
}

impl ColorRow {
    /// A row of `channels` fields filling the cell with no margin: the swatch (or the first field,
    /// with `swatch_left`) ends flush with the cell edge.
    pub fn flush(channels: usize) -> Self {
        Self {
            channels,
            fill_margin: Some(0.0),
            ..Self::default()
        }
    }
}

/// An editable colour row: numeric channel fields and the [`color_button`] swatch, laid out as
/// `row` says. Fields are unclamped HDR `DragValue`s (>= 0, may exceed 1.0). The `Response`
/// reports `changed()` if any field OR the swatch changed this frame. Colour-space agnostic: it
/// edits the values in whatever space the caller stores them. Sizes come from the style
/// (`interact_size.y` for the swatch, `item_spacing.x` between items).
pub fn color_edit(ui: &mut Ui, color: &mut [f32; 4], row: &ColorRow) -> Response {
    let n = row.channels.clamp(3, 4);
    ui.horizontal(|ui| {
        let side = ui.spacing().interact_size.y.max(SWATCH_MIN);
        let gap = ui.spacing().item_spacing.x;
        let mut swatch = *color;
        if n == 3 {
            swatch[3] = 1.0;
        }
        let mut resp: Option<Response> = None;
        let mut picked = false;
        let union = |resp: &mut Option<Response>, r: Response| {
            *resp = Some(match resp.take() {
                Some(acc) => acc.union(r),
                None => r,
            });
        };
        if let Some(margin) = row.fill_margin {
            // The swatch is the row's identity: it keeps its slot at the edge whatever the width,
            // and the fields take what is left, shrinking (their text clips) before it does.
            let total = (ui.available_width() - margin).max(side);
            let height = side.max(ui.spacing().interact_size.y);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(total, height), Sense::hover());
            let swatch_x = if row.swatch_left {
                rect.left()
            } else {
                rect.right() - side
            };
            let swatch_rect = Rect::from_min_size(
                egui::pos2(swatch_x, rect.center().y - side / 2.0),
                Vec2::splat(side),
            );
            let fields = if row.swatch_left {
                Rect::from_min_max(egui::pos2(swatch_rect.right() + gap, rect.top()), rect.max)
            } else {
                Rect::from_min_max(
                    rect.min,
                    egui::pos2(swatch_rect.left() - gap, rect.bottom()),
                )
            };
            let width = ((fields.width() - gap * (n - 1) as f32) / n as f32).max(0.0);
            for (i, c) in color[..n].iter_mut().enumerate() {
                let cell = Rect::from_min_size(
                    egui::pos2(fields.left() + i as f32 * (width + gap), fields.top()),
                    Vec2::new(width, height),
                );
                let field = ui
                    .scope_builder(egui::UiBuilder::new().max_rect(cell), |ui| {
                        ui.set_clip_rect(ui.clip_rect().intersect(cell));
                        // A field's own minimum width must not push the swatch out.
                        ui.spacing_mut().interact_size.x = 0.0;
                        channel_field_w(ui, c, width, row.decimals)
                    })
                    .inner;
                union(&mut resp, field);
            }
            let sw = ui
                .scope_builder(egui::UiBuilder::new().max_rect(swatch_rect), |ui| {
                    color_button(ui, &mut swatch)
                })
                .inner;
            picked = sw.changed();
            union(&mut resp, sw);
        } else {
            if row.swatch_left {
                let sw = color_button(ui, &mut swatch);
                picked = sw.changed();
                union(&mut resp, sw);
            }
            for c in &mut color[..n] {
                union(&mut resp, channel_field(ui, c, row.decimals));
            }
            if !row.swatch_left {
                let sw = color_button(ui, &mut swatch);
                picked = sw.changed();
                union(&mut resp, sw);
            }
        }
        // The popup edits the `swatch` copy: take its channels back (alpha stays the caller's).
        if picked {
            color[..n].copy_from_slice(&swatch[..n]);
        }
        resp.unwrap_or_else(|| ui.allocate_response(Vec2::ZERO, Sense::hover()))
    })
    .inner
}

/// Nuke-style RGBA colour editor: four numeric channel fields (`R G B A`) and the swatch popup on
/// one row ([`ColorRow::default`]). See [`color_edit`].
pub fn color_edit_rgba(ui: &mut Ui, color: &mut [f32; 4]) -> Response {
    color_edit_rgba_ex(ui, color, false, COLOR_DECIMALS_DEFAULT)
}

/// [`color_edit_rgba`] with the swatch before (`swatch_left`) or after the fields and `decimals`
/// shown per field. See [`color_edit`].
pub fn color_edit_rgba_ex(
    ui: &mut Ui,
    color: &mut [f32; 4],
    swatch_left: bool,
    decimals: usize,
) -> Response {
    color_edit(
        ui,
        color,
        &ColorRow {
            swatch_left,
            decimals,
            ..ColorRow::default()
        },
    )
}

/// A colour row that ends exactly at the cell edge ([`ColorRow::flush`]): `channels` (3 = RGB,
/// 4 = RGBA) fields share the width and the swatch is flush right, like a slider's value box.
pub fn color_edit_flush(
    ui: &mut Ui,
    color: &mut [f32; 4],
    channels: usize,
    swatch_left: bool,
    decimals: usize,
) -> Response {
    color_edit(
        ui,
        color,
        &ColorRow {
            swatch_left,
            decimals,
            ..ColorRow::flush(channels)
        },
    )
}

/// Nuke-style RGB colour editor: three numeric channel fields of their natural width + the swatch
/// (alpha pinned to `1.0`). See [`color_edit`].
pub fn color_edit_rgb(ui: &mut Ui, rgb: &mut [f32; 3]) -> Response {
    let mut rgba = [rgb[0], rgb[1], rgb[2], 1.0];
    let row = ColorRow {
        channels: 3,
        fill_margin: None,
        ..ColorRow::default()
    };
    let resp = color_edit(ui, &mut rgba, &row);
    if resp.changed() {
        *rgb = [rgba[0], rgba[1], rgba[2]];
    }
    resp
}

/// One HDR colour-channel numeric field (unclamped above, never negative).
/// `decimals` formats the DISPLAY only — the stored value keeps full precision.
fn channel_field(ui: &mut Ui, v: &mut f32, decimals: usize) -> Response {
    ui.add(
        egui::DragValue::new(v)
            .speed(0.005)
            .range(0.0..=f32::MAX)
            .max_decimals(decimals),
    )
}

/// [`channel_field`] sized to an exact width, so a row of channels can be made to
/// fill (and align with) the editor cell. `decimals` formats the display only.
fn channel_field_w(ui: &mut Ui, v: &mut f32, w: f32, decimals: usize) -> Response {
    ui.add_sized(
        Vec2::new(w, ui.spacing().interact_size.y),
        egui::DragValue::new(v)
            .speed(0.005)
            .range(0.0..=f32::MAX)
            .max_decimals(decimals),
    )
}

/// Swatch button with a customised config (HDR cap, display
/// transform, eyedropper hook, etc.). The `&mut PickerConfig` is
/// taken by mutable reference so the eyedropper callback can be
/// invoked from inside the popup without owning the entire config.
pub fn color_button_with(ui: &mut Ui, color: &mut [f32; 4], cfg: &mut PickerConfig) -> Response {
    // Nuke-style tiny chip: a round colour pill that sits next to
    // the R/G/B/A DragValues without stealing column width. Clicks
    // open the full picker popup.
    let size = ui.spacing().interact_size.y.max(SWATCH_MIN);
    let swatch_size = Vec2::new(size, size);
    let (rect, mut response) = ui.allocate_exact_size(swatch_size, Sense::click());

    paint_swatch_icon(ui, rect, color, cfg, response.hovered());

    // Snapshot the colour before the popup runs. The popup mutates `color` in
    // place (HS gradient / V slider / RGB-HSV-hex fields), but the swatch's own
    // `Sense::click` response never reports `changed()` on its own — so the host
    // (which re-packs only when `.changed()`) would silently drop every edit and
    // the picker would appear frozen. We compare after the popup and mark the
    // returned response changed ourselves, which is the signal hosts rely on.
    let color_before = *color;

    // `Popup::menu(&response)` wires the anchor click → toggle
    // gesture. We then override close-behaviour to
    // `IgnoreClicks`: a click landing anywhere — on the HS
    // gradient, on a DragValue, on the hex field, even outside
    // the popup — never closes the picker. The only close path
    // is the 1-second-no-hover timer below, plus an explicit
    // re-click on the swatch chip (which toggles via the
    // gesture and bypasses close-behaviour).
    let popup_id = response.id.with("colorpicker_popup");

    // --- Click-through barrier.
    //
    // `Popup::menu` mounts the picker in an `Order::Foreground` `Area` that
    // already senses clicks over its own rect. That blocks *background-layer*
    // widgets — but only if egui resolves the popup as the top layer at the
    // pointer, which it can fail to do when the swatch is nested inside another
    // floating surface (an `egui_dock` tab body, a `Window`) whose widgets then
    // catch clicks that land in the popup. To make the popup pointer-opaque we
    // drop a transparent full-rect interaction sink in `Order::Middle` —
    // strictly *below* the `Foreground` popup (so the gradient / sliders / hex
    // field keep every click) yet *above* the background dock content (so
    // nothing beneath the popup can be hit through its gaps). It is positioned
    // at the popup's rect from the previous frame; the one-frame lag is
    // invisible because the popup geometry is static while open. The rect is
    // cleared from memory when the popup closes so the sink vanishes with it.
    let backstop_key = popup_id.with("backstop_rect");
    if let Some(prev_rect) = egui_widgets_config::state::get::<Rect>(ui.ctx(), backstop_key) {
        egui::Area::new(popup_id.with("backstop"))
            .order(egui::Order::Middle)
            .fixed_pos(prev_rect.min)
            .show(ui.ctx(), |ui| {
                ui.allocate_rect(prev_rect, Sense::click_and_drag());
            });
    }

    let popup_response = egui::Popup::menu(&response)
        .id(popup_id)
        .gap(4.0)
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .show(|ui| picker_popup_contents(ui, color, cfg));

    // Track the popup rect for next frame's barrier, or clear it once closed.
    match &popup_response {
        Some(inner) => {
            let r = inner.response.rect;
            egui_widgets_config::state::set(ui.ctx(), backstop_key, r);
        }
        None => {
            egui_widgets_config::state::clear::<Rect>(ui.ctx(), backstop_key);
        }
    }

    // Three exit paths for the open popup:
    //   1. Mouse outside the popup AND outside the swatch chip
    //      for `IDLE_CLOSE_SECS` straight — primary close path.
    //   2. `Esc` pressed at any time the popup is open.
    //   3. Re-click on the swatch chip — handled by the
    //      `Popup::menu` gesture, no manual code needed here.
    //
    // While the idle timer is counting down, a 2 px progress
    // strip painted across the popup's bottom edge gives the
    // user a visual countdown — full width = just left the
    // popup, empty = closing now. Re-entering refills it.
    if let Some(inner) = popup_response {
        const IDLE_CLOSE_SECS: f64 = 0.5;
        let timer_key = popup_id.with("idle_close_timer");
        let now = ui.ctx().input(|i| i.time);
        // The pointer over the swatch or anywhere over the popup, by position: egui's
        // `hovered()` / `contains_pointer()` go false whenever another area (the popup
        // itself, its click-through backstop) is on top at the pointer, which closed the
        // picker under a pointer resting on its own swatch.
        let hovered = ui
            .ctx()
            .pointer_latest_pos()
            .is_some_and(|p| response.rect.contains(p) || inner.response.rect.contains(p));
        let sampling = cfg.eyedropper_sampling;
        let last_seen = if hovered || sampling {
            egui_widgets_config::state::set(ui.ctx(), timer_key, now);
            now
        } else {
            egui_widgets_config::state::get::<f64>(ui.ctx(), timer_key).unwrap_or(now)
        };

        // Close on Esc — convenient keyboard exit independent of
        // mouse position. `consume_key` marks the event consumed
        // so the app-level handler doesn't also see it.
        if ui
            .ctx()
            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            egui::Popup::close_id(ui.ctx(), popup_id);
        }

        // Close on idle timeout.
        let elapsed = now - last_seen;
        if !(hovered || sampling) && elapsed > IDLE_CLOSE_SECS {
            egui::Popup::close_id(ui.ctx(), popup_id);
        }

        // Visual countdown bar at the bottom of the popup.
        let progress = if hovered || sampling {
            1.0_f32
        } else {
            (((IDLE_CLOSE_SECS - elapsed) / IDLE_CLOSE_SECS).clamp(0.0, 1.0)) as f32
        };
        let popup_rect = inner.response.rect;
        let bar_h = 2.0;
        let bar_rect = Rect::from_min_size(
            popup_rect.left_bottom() - Vec2::new(0.0, bar_h),
            Vec2::new(popup_rect.width() * progress, bar_h),
        );
        let layer = egui::LayerId::new(egui::Order::Foreground, popup_id.with("idle_bar"));
        let painter = ui.ctx().layer_painter(layer);
        painter.rect_filled(bar_rect, 0.0, Color32::from_gray(180));

        if !(hovered || sampling) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(33));
        }
    }
    // Propagate any in-popup edit to the host via the standard `changed()` flag.
    if *color != color_before {
        response.mark_changed();
    }
    response
}

/// Compact circular swatch used inside an attribute row.
fn paint_swatch_icon(ui: &Ui, rect: Rect, color: &[f32; 4], cfg: &PickerConfig, hovered: bool) {
    let d = (cfg.display_transform)([color[0], color[1], color[2]]);
    let c32 = display_to_color32(d, color[3]);
    let painter = ui.painter();
    let radius = (rect.width().min(rect.height()) * 0.5) - 0.5;
    let center = rect.center();
    let cell = (radius * 0.6).max(2.0);
    // Inscribe the alpha checker INSIDE the circle (side = 2r/√2) so its corners
    // sit on the circle instead of poking past it — those poking corners (the
    // checker's light-gray base) were the "white corners" seen around the swatch.
    let chk_rect =
        Rect::from_center_size(center, Vec2::splat(radius * 2.0 / std::f32::consts::SQRT_2));
    paint_checker(painter, chk_rect, cell, 0.0);
    painter.circle_filled(center, radius, c32);
    let rim = if hovered {
        Color32::from_gray(200)
    } else {
        Color32::from_gray(80)
    };
    painter.circle_stroke(center, radius, Stroke::new(1.0_f32, rim));
}

fn display_to_color32(d: [f32; 3], a: f32) -> Color32 {
    let q = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(q(d[0]), q(d[1]), q(d[2]), q(a))
}

/// Paint the alpha checkerboard filling `rect`, `cell`-sized squares. `rounding` MUST match the
/// rounding of the colour rect drawn on top: the light base is rounded to it so its bright corners
/// don't spill past the colour's rounded corners (the "white corners" glitch), and dark cells are
/// clipped to the rounded footprint so nothing pokes out either.
fn paint_checker(painter: &egui::Painter, rect: Rect, cell: f32, rounding: f32) {
    let dark = Color32::from_gray(60);
    let light = Color32::from_gray(180);
    painter.rect_filled(rect, rounding, light);
    // Keep dark cells clear of the rounded corners (radius `rounding`) so they don't poke into the
    // notches the colour rect leaves uncovered. `rounding == 0` (square swatch) leaves this a no-op.
    let inner = rect.shrink(rounding);
    let cols = ((rect.width() / cell).ceil() as i32).max(1);
    let rows = ((rect.height() / cell).ceil() as i32).max(1);
    for r in 0..rows {
        for c in 0..cols {
            if (r + c) % 2 == 0 {
                continue;
            }
            let p = Pos2::new(rect.min.x + c as f32 * cell, rect.min.y + r as f32 * cell);
            let cr = Rect::from_min_size(p, Vec2::splat(cell)).intersect(inner);
            if cr.is_positive() {
                painter.rect_filled(cr, 0.0, dark);
            }
        }
    }
}

fn picker_popup_contents(ui: &mut Ui, color: &mut [f32; 4], cfg: &mut PickerConfig) {
    const POPUP_W: f32 = 360.0;
    const PREVIEW_W: f32 = (POPUP_W - 12.0) * 0.5;
    /// Upper channel bound when the picker is in Full (unrestricted/HDR) mode.
    const HDR_FULL_MAX: f32 = 16.0;
    ui.set_max_width(POPUP_W);
    ui.set_min_width(POPUP_W);

    // Two HDR modes, persisted GLOBALLY (egui memory) so every picker shares the
    // choice:
    //   • Clipped — channels capped at 1.0 (classic LDR colour).
    //   • Full    — unrestricted up to `HDR_FULL_MAX`, so emissive / bloom colours
    //              can be authored > 1.0 (kept where the attribute stores float).
    // This OVERRIDES `cfg.hdr_max` for the popup, so the swatch caller need not
    // know about the mode; the sRGB column stays clamped [0,1] regardless.
    let hdr_id = egui::Id::new("egui_colorpicker_hdr_full");
    let mut hdr_full = ui
        .ctx()
        .data_mut(|d| d.get_persisted::<bool>(hdr_id))
        .unwrap_or(false);
    // Explicit, always-visible LDR | HDR mode toggle (two mutually-exclusive
    // segments, joined — same idiom as the app's `segmented` control, rebuilt
    // inline because this crate must not depend on `egui-widgets`, which already
    // depends on us). LDR clamps every channel to 1.0 (classic display colour);
    // HDR lifts the cap to `HDR_FULL_MAX` so emissive / bloom colours can be
    // authored > 1.0. The choice is persisted GLOBALLY so every picker shares it,
    // and OVERRIDES `cfg.hdr_max` for the popup (the sRGB column stays clamped
    // [0,1] regardless, since sRGB encoding is undefined above 1.0).
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("range")
                .small()
                .color(ui.visuals().weak_text_color()),
        );
        ui.spacing_mut().item_spacing.x = 1.0;
        if ui
            .selectable_label(!hdr_full, "LDR")
            .on_hover_text("Clipped: every channel capped at 1.0 (classic display colour).")
            .clicked()
        {
            hdr_full = false;
        }
        if ui
            .selectable_label(hdr_full, "HDR")
            .on_hover_text(
                "Full: author HDR colours > 1.0 (emissive / bloom), up to 16× — kept \
                 where the attribute stores float RGB.",
            )
            .clicked()
        {
            hdr_full = true;
        }
    });
    // Persist every frame (cheap) so the mode survives popup close / restart.
    ui.ctx().data_mut(|d| d.insert_persisted(hdr_id, hdr_full));
    cfg.hdr_max = if hdr_full { HDR_FULL_MAX } else { 1.0 };
    ui.add_space(4.0);

    // --- Section 1: working vs display previews.
    let preview_h = 36.0;
    ui.horizontal(|ui| {
        let half = PREVIEW_W;
        let working_disp = [
            linear_to_srgb(color[0]),
            linear_to_srgb(color[1]),
            linear_to_srgb(color[2]),
        ];
        let (r1, _) = ui.allocate_exact_size(Vec2::new(half, preview_h), Sense::hover());
        paint_checker(ui.painter(), r1, 4.0, 4.0);
        ui.painter()
            .rect_filled(r1, 4.0, display_to_color32(working_disp, color[3]));
        ui.painter().text(
            r1.left_top() + Vec2::new(6.0, 4.0),
            egui::Align2::LEFT_TOP,
            "working",
            egui::TextStyle::Small.resolve(ui.style()),
            Color32::WHITE,
        );

        let display = (cfg.display_transform)([color[0], color[1], color[2]]);
        let (r2, _) = ui.allocate_exact_size(Vec2::new(half, preview_h), Sense::hover());
        paint_checker(ui.painter(), r2, 4.0, 4.0);
        ui.painter()
            .rect_filled(r2, 4.0, display_to_color32(display, color[3]));
        ui.painter().text(
            r2.left_top() + Vec2::new(6.0, 4.0),
            egui::Align2::LEFT_TOP,
            "display",
            egui::TextStyle::Small.resolve(ui.style()),
            Color32::WHITE,
        );
    });

    ui.add_space(6.0);

    let (mut h_deg, mut s, mut v) = rgb_to_hsv(color[0], color[1], color[2]);

    // --- Section 2: HS gradient + V slider + RGB/HSV editors.
    ui.horizontal(|ui| {
        let hs_size = 180.0;
        if hs_picker(ui, hs_size, &mut h_deg, &mut s) {
            let (r, g, b) = hsv_to_rgb(h_deg, s, v);
            color[0] = r;
            color[1] = g;
            color[2] = b;
        }
        ui.add_space(6.0);
        if v_slider(ui, Vec2::new(22.0, hs_size), h_deg, s, &mut v, cfg.hdr_max) {
            let (r, g, b) = hsv_to_rgb(h_deg, s, v);
            color[0] = r;
            color[1] = g;
            color[2] = b;
        }
        ui.add_space(8.0);

        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("RGB")
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
            egui::Grid::new("egui_colorpicker_rgb_grid")
                .num_columns(3)
                .spacing([4.0, 2.0])
                .show(ui, |ui| {
                    for (label, ch) in [("R", 0usize), ("G", 1), ("B", 2)] {
                        ui.label(label);
                        ui.add(
                            egui::DragValue::new(&mut color[ch])
                                .range(0.0..=cfg.hdr_max)
                                .speed(0.005)
                                .max_decimals(3),
                        );
                        let mut srgb = linear_to_srgb(color[ch]).clamp(0.0, 1.0);
                        if ui
                            .add(
                                egui::DragValue::new(&mut srgb)
                                    .range(0.0..=1.0)
                                    .speed(0.005)
                                    .max_decimals(3),
                            )
                            .changed()
                        {
                            color[ch] = srgb_to_linear(srgb.clamp(0.0, 1.0));
                        }
                        ui.end_row();
                    }
                });

            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("HSV")
                    .small()
                    .color(ui.visuals().weak_text_color()),
            );
            let (orig_h, orig_s, orig_v) = (h_deg, s, v);
            egui::Grid::new("egui_colorpicker_hsv_grid")
                .num_columns(2)
                .spacing([4.0, 2.0])
                .show(ui, |ui| {
                    ui.label("H");
                    ui.add(
                        egui::DragValue::new(&mut h_deg)
                            .range(0.0..=360.0)
                            .speed(0.5)
                            .suffix("°")
                            .max_decimals(1),
                    );
                    ui.end_row();
                    ui.label("S");
                    ui.add(
                        egui::DragValue::new(&mut s)
                            .range(0.0..=1.0)
                            .speed(0.005)
                            .max_decimals(3),
                    );
                    ui.end_row();
                    ui.label("V");
                    ui.add(
                        egui::DragValue::new(&mut v)
                            .range(0.0..=cfg.hdr_max)
                            .speed(0.005)
                            .max_decimals(3),
                    );
                    ui.end_row();
                });
            if (h_deg - orig_h).abs() > 1e-4
                || (s - orig_s).abs() > 1e-4
                || (v - orig_v).abs() > 1e-4
            {
                let (r, g, b) = hsv_to_rgb(h_deg, s, v);
                color[0] = r;
                color[1] = g;
                color[2] = b;
            }
        });
    });

    ui.add_space(4.0);

    // --- Section 3: Hex + Alpha.
    ui.horizontal(|ui| {
        ui.label("Hex");
        let mut hex = linear_to_hex(color[0], color[1], color[2]);
        let resp = ui.add(egui::TextEdit::singleline(&mut hex).desired_width(80.0));
        if resp.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some((r, g, b)) = hex_to_linear(&hex)
        {
            color[0] = r;
            color[1] = g;
            color[2] = b;
        }

        if cfg.alpha_enabled {
            ui.add_space(12.0);
            ui.label("A");
            ui.add(
                egui::DragValue::new(&mut color[3])
                    .range(0.0..=1.0)
                    .speed(0.005)
                    .max_decimals(3),
            );
        }
    });

    ui.add_space(6.0);

    // --- Section 4: Color history strip.
    let mut history = load_history(ui.ctx());
    let mut history_dirty = false;
    ui.horizontal(|ui| {
        if ui
            .small_button("save")
            .on_hover_text("Add current color to history")
            .clicked()
        {
            history.push(*color);
            history_dirty = true;
        }
        let cell = Vec2::new(20.0, 18.0);
        let mut to_remove: Option<usize> = None;
        for (i, entry) in history.entries.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(cell, Sense::click());
            let d = (cfg.display_transform)([entry[0], entry[1], entry[2]]);
            paint_checker(ui.painter(), r, 3.0, 2.0);
            ui.painter()
                .rect_filled(r, 2.0, display_to_color32(d, entry[3]));
            ui.painter().rect_stroke(
                r,
                2.0,
                Stroke::new(1.0_f32, Color32::from_gray(40)),
                egui::StrokeKind::Inside,
            );
            if resp.clicked() {
                *color = *entry;
            }
            if resp.double_clicked() {
                to_remove = Some(i);
            }
            resp.on_hover_text("click = apply · double-click = remove");
        }
        if let Some(i) = to_remove {
            history.entries.remove(i);
            history_dirty = true;
        }
    });
    if history_dirty {
        save_history(ui.ctx(), history);
    }

    // --- Section 5: Eyedropper button (host-installed callback).
    if let Some(cb) = cfg.eyedropper.as_mut() {
        ui.add_space(4.0);
        if ui
            .button("Eyedropper")
            .on_hover_text("Sample a color from the viewport. Host app drives the actual sampling.")
            .clicked()
        {
            cb();
        }
    }
}

/// 2D Hue × Saturation gradient + cursor. Returns true on edit.
fn hs_picker(ui: &mut Ui, size: f32, h: &mut f32, s: &mut f32) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    let painter = ui.painter();

    let n = 32usize;
    let mut mesh = Mesh::default();
    for j in 0..=n {
        for i in 0..=n {
            let u = i as f32 / n as f32;
            let vt = j as f32 / n as f32;
            let h_d = u * 360.0;
            let s_v = 1.0 - vt;
            let (r, g, b) = hsv_to_rgb(h_d, s_v, 1.0);
            let c = display_to_color32(
                [linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b)],
                1.0,
            );
            let p = Pos2::new(
                rect.min.x + u * rect.width(),
                rect.min.y + vt * rect.height(),
            );
            mesh.colored_vertex(p, c);
        }
    }
    let stride = (n + 1) as u32;
    for j in 0..n as u32 {
        for i in 0..n as u32 {
            let a = j * stride + i;
            let b = a + 1;
            let c = a + stride;
            let d = c + 1;
            mesh.add_triangle(a, b, c);
            mesh.add_triangle(b, d, c);
        }
    }
    painter.add(Shape::mesh(mesh));

    let cur = Pos2::new(
        rect.min.x + (*h / 360.0).clamp(0.0, 1.0) * rect.width(),
        rect.min.y + (1.0 - *s).clamp(0.0, 1.0) * rect.height(),
    );
    painter.circle_stroke(cur, 6.0, Stroke::new(2.0_f32, Color32::WHITE));
    painter.circle_stroke(cur, 6.0, Stroke::new(1.0_f32, Color32::BLACK));

    let mut changed = false;
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let u = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
        let v = ((pos.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
        *h = u * 360.0;
        *s = 1.0 - v;
        changed = true;
    }
    changed
}

/// Vertical V slider. Returns true on edit.
fn v_slider(ui: &mut Ui, size: Vec2, h_deg: f32, s: f32, v: &mut f32, hdr_max: f32) -> bool {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter();

    let stops = 8usize;
    let mut mesh = Mesh::default();
    for i in 0..=stops {
        let t = i as f32 / stops as f32;
        let v_here = (1.0 - t) * hdr_max;
        let (r, g, b) = hsv_to_rgb(h_deg, s, v_here);
        let c = display_to_color32(
            [linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b)],
            1.0,
        );
        let y = rect.min.y + t * rect.height();
        mesh.colored_vertex(Pos2::new(rect.min.x, y), c);
        mesh.colored_vertex(Pos2::new(rect.max.x, y), c);
    }
    for i in 0..stops as u32 {
        let a = i * 2;
        let b = a + 1;
        let c = a + 2;
        let d = a + 3;
        mesh.add_triangle(a, b, c);
        mesh.add_triangle(b, d, c);
    }
    painter.add(Shape::mesh(mesh));

    let t = 1.0 - (v.clamp(0.0, hdr_max) / hdr_max.max(1e-6));
    let y = rect.min.y + t * rect.height();
    painter.line_segment(
        [
            Pos2::new(rect.min.x - 2.0, y),
            Pos2::new(rect.max.x + 2.0, y),
        ],
        Stroke::new(2.0_f32, Color32::WHITE),
    );
    painter.line_segment(
        [
            Pos2::new(rect.min.x - 2.0, y),
            Pos2::new(rect.max.x + 2.0, y),
        ],
        Stroke::new(1.0_f32, Color32::BLACK),
    );

    let mut changed = false;
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let t = ((pos.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
        *v = (1.0 - t) * hdr_max;
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_editor_ends_at_the_cell_edge_for_rgb_and_rgba() {
        let ctx = egui::Context::default();
        for channels in [3, 4] {
            let mut right = (0.0, 0.0);
            let mut out = ctx.run_ui(Default::default(), |ui| {
                ui.set_max_width(210.0);
                let mut c = [0.2, 0.4, 0.6, 0.5];
                let r = color_edit_flush(ui, &mut c, channels, false, COLOR_DECIMALS_DEFAULT);
                let left = color_edit_flush(ui, &mut c, channels, true, 1);
                assert!(
                    (left.rect.right() - ui.max_rect().right()).abs() <= 1.0,
                    "swatch left"
                );
                right = (r.rect.right(), ui.max_rect().right());
            });
            out.textures_delta.clear();
            assert!(
                (right.0 - right.1).abs() <= 1.0,
                "{channels} channels: {right:?}"
            );
        }
    }

    #[test]
    fn narrow_colour_row_keeps_its_swatch_at_the_edge() {
        let ctx = egui::Context::default();
        for width in [200.0, 90.0, 40.0] {
            let mut rects = (Rect::NOTHING, Rect::NOTHING);
            let mut out = ctx.run_ui(Default::default(), |ui| {
                ui.set_max_width(width);
                let mut c = [0.2, 0.4, 0.6, 1.0];
                let r = color_edit_flush(ui, &mut c, 3, false, COLOR_DECIMALS_DEFAULT);
                rects = (r.rect, ui.max_rect());
            });
            out.textures_delta.clear();
            let side = SWATCH_MIN;
            assert!(
                rects.0.right() <= rects.1.right() + 1.0,
                "{width}: {rects:?}"
            );
            // The union reaches the right edge only through the swatch: it is there.
            assert!(rects.0.width() >= side, "{width}: {rects:?}");
            assert!(
                (rects.0.right() - rects.1.right()).abs() <= 1.0,
                "{width}: {rects:?}"
            );
        }
    }
}
