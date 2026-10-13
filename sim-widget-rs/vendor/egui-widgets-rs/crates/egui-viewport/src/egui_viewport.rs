//! Generic 2-D image viewport widget for egui: pan / zoom / fit / home (100%),
//! pixel-coordinate + RGBA readout HUD, channel isolation (R/G/B/A/RGB), and
//! click/drag scrub reporting.
//!
//! The widget owns NO image data and NO GPU. The caller already has its image on
//! a GPU/egui texture and passes the [`egui::TextureId`] plus its pixel size;
//! [`show`] paints it with `painter.image()`, maps pointer input into image
//! pixels, and mutates only the caller-owned [`ViewportState`] (pan / zoom /
//! channel / pinned pixel). Every side effect the caller must act on is returned
//! via [`ViewportResponse`] — there is no hidden global state.
//!
//! ## Why channel isolation is a state enum, not a tint
//!
//! Isolating a channel for VFX/CG review is not a per-channel multiply: the R-only
//! view shows R replicated to grayscale, the A-only view shows the alpha matte as
//! grayscale, and RGB drops alpha. A `Color32` tint cannot express
//! "splat one channel to luminance" — it can only scale channels. So the widget
//! reports the *intent* ([`Channel`]) in the state and lets the caller apply it in
//! its own colour pass / shader, where the full semantics are available. This also
//! keeps the widget a pure input/paint mapper with no opinion about pixel maths.
//!
//! ## Navigation (preserved from the source viewports)
//!
//! - MMB drag — pan
//! - RMB drag — zoom about the viewport centre (drag up = in, down = out)
//! - Scroll — zoom toward cursor
//! - Ctrl + `+`/`=`/`-` — zoom in / out (also numpad `+` / `-`)
//! - LMB drag — scrub (reported as a `0..=1` position); on a still image an LMB
//!   click pins the pixel readout instead
//! - `F` — fit to view · `H` / `0` — 1:1 (100%)
//! - `R`/`G`/`B`/`A` — isolate channel (press again → RGB(A))
//!
//! Gathered from exr-rs `exr-view` and openvdb-rs `vdb-view` (identical sources)
//! and decoupled from their `ImageBackend`/`ColorPass`/`DisplayGpu`/
//! `ViewControls`/`Playback` + wgpu types.

use egui_widgets_config::{Semantic, WidgetsConfig, mono, semantic};
use serde::{Deserialize, Serialize};

/// Wheel zoom rate: `factor = 1 + scroll * ZOOM_SPEED`.
const ZOOM_SPEED: f32 = 0.002;
/// Per-pixel zoom rate for right-button drag (vertical).
const ZOOM_DRAG_SPEED: f32 = 0.005;
/// Multiplicative zoom step for one Ctrl + `+`/`-` key press.
const ZOOM_KEY_STEP: f32 = 1.25;
/// Hard zoom clamps (1% .. 6400%) shared by every zoom path.
const ZOOM_MIN: f32 = 0.01;
const ZOOM_MAX: f32 = 64.0;

// ─── channel isolation ─────────────────────────────────────────────────────────

/// Which channel(s) the viewport is currently isolating.
///
/// The widget only *reports* this (mutated by the R/G/B/A keys); the caller maps
/// it onto its own colour pass — see the module docs for why a tint is unsuitable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Full colour (RGB, alpha composited / ignored by the caller as it sees fit).
    #[default]
    Rgb,
    /// Red isolated (typically splatted to grayscale by the caller).
    R,
    /// Green isolated.
    G,
    /// Blue isolated.
    B,
    /// Alpha isolated (the matte as grayscale).
    A,
}

// ─── persistent state ──────────────────────────────────────────────────────────

/// Persistent viewport state the caller owns across frames and the widget mutates
/// in place. Serde-derived so it can live in a saved layout / preferences blob.
///
/// `pan` is the offset (in screen points) of the image centre from the viewport
/// centre, so it scales cleanly around a centre pivot. `zoom` is the screen-pixels
/// per image-pixel factor. `fit_zoom` is the cached "fit to view" zoom, recomputed
/// lazily whenever it is non-positive (i.e. invalidated) and the image size and
/// viewport rect are both known.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ViewportState {
    /// Image-centre offset from viewport centre, in screen points.
    pub pan: egui::Vec2,
    /// Screen-pixels per image-pixel.
    pub zoom: f32,
    /// Cached fit-to-view zoom; `<= 0.0` means "recompute on next frame".
    pub fit_zoom: f32,
    /// Currently isolated channel (caller applies it to its colour pass).
    pub channel: Channel,
    /// Pinned pixel (LMB click on a still image), kept for the crosshair + readout.
    pub picked_pixel: Option<(u32, u32)>,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            // `0.0` forces a fit-zoom recompute on the first frame with a valid rect.
            fit_zoom: 0.0,
            channel: Channel::Rgb,
            picked_pixel: None,
        }
    }
}

impl ViewportState {
    /// Fit the image to the viewport (the `F` key): drop pan and snap zoom to the
    /// cached fit-zoom. Invalidates `fit_zoom` if it was never computed so the next
    /// frame recomputes it from the current rect.
    pub fn fit(&mut self) {
        self.pan = egui::Vec2::ZERO;
        if self.fit_zoom > 0.0 {
            self.zoom = self.fit_zoom;
        } else {
            // Force recompute next frame (rect/size not known here).
            self.fit_zoom = 0.0;
        }
    }

    /// Home / 1:1 (the `H` or `0` key): centre the image at 100% zoom.
    pub fn home(&mut self) {
        self.pan = egui::Vec2::ZERO;
        self.zoom = 1.0;
    }

    /// Invalidate the cached fit-zoom (call when the image dimensions change), so
    /// the next [`show`] recomputes it for the new size.
    pub fn invalidate_fit(&mut self) {
        self.fit_zoom = 0.0;
    }
}

// ─── per-call inputs ───────────────────────────────────────────────────────────

/// The image the caller wants drawn this frame: an egui texture and its pixel
/// size. A zero-sized image (or a `size` with a zero axis) draws only the
/// background + checker (and still services a scrub gesture across the viewport).
#[derive(Clone, Copy, Debug)]
pub struct ViewportImage {
    /// The already-uploaded egui texture to display.
    pub texture: egui::TextureId,
    /// Image dimensions in pixels `[w, h]`.
    pub size: [u32; 2],
}

impl ViewportImage {
    /// Convenience constructor.
    pub fn new(texture: egui::TextureId, size: [u32; 2]) -> Self {
        Self { texture, size }
    }

    fn w(&self) -> u32 {
        self.size[0]
    }
    fn h(&self) -> u32 {
        self.size[1]
    }
    fn is_empty(&self) -> bool {
        self.size[0] == 0 || self.size[1] == 0
    }
}

/// Optional scrub configuration. When `Some`, the primary button (LMB) scrubs the
/// sequence instead of pinning a pixel, and the widget reports the `0..=1`
/// position in [`ViewportResponse::scrub_to`].
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrubConfig {
    /// Inclusive frame-number range `(first, last)`. When set, a Flame-style scrub
    /// cursor with the target frame number is drawn while scrubbing.
    pub range: Option<(i32, i32)>,
    /// Optional on-image playhead position `0..=1`; drawn as a thin vertical line
    /// at `playhead_alpha` opacity (skipped when `playhead_alpha <= 0`).
    pub playhead: Option<f32>,
    /// Playhead opacity `0..=1` (0 hides it).
    pub playhead_alpha: f32,
}

// ─── response ──────────────────────────────────────────────────────────────────

/// What the viewport interaction produced this frame. The caller owns all side
/// effects: apply `state.channel` to its colour pass, and seek to `scrub_to`.
pub struct ViewportResponse {
    /// The underlying egui [`egui::Response`] for the viewport rect.
    pub response: egui::Response,
    /// The image pixel currently under the cursor (hover), if any and in bounds.
    pub hovered_pixel: Option<(u32, u32)>,
    /// When scrubbing was enabled and the primary button scrubbed, the `0..=1`
    /// position to seek to (the caller maps it to a frame).
    pub scrub_to: Option<f32>,
}

// ─── main entry point ──────────────────────────────────────────────────────────

/// Draw the 2-D image viewport into `ui`.
///
/// Handles navigation + channel keys, paints the caller's texture, draws the
/// checker background, the pixel readout HUD (when `read_pixel` is supplied), the
/// pinned-pixel crosshair, the zoom label and the scrub cursor / playhead. Mutates
/// only `state`; returns everything the caller must act on in [`ViewportResponse`].
///
/// `read_pixel` is an optional callback returning the linear `[r, g, b, a]` at an
/// image pixel for the readout HUD; pass `|_, _| None` to disable the value
/// readout (the coordinate readout still works through `hovered_pixel`).
pub fn show(
    ui: &mut egui::Ui,
    state: &mut ViewportState,
    image: ViewportImage,
    scrub: ScrubConfig,
    read_pixel: impl Fn(u32, u32) -> Option<[f32; 4]>,
) -> ViewportResponse {
    let available = ui.available_size();
    if available.x <= 0.0 || available.y <= 0.0 {
        // Degenerate rect: allocate a zero response so the caller still gets one.
        let (_, response) = ui.allocate_exact_size(egui::Vec2::ZERO, egui::Sense::hover());
        return ViewportResponse {
            response,
            hovered_pixel: None,
            scrub_to: None,
        };
    }

    let (rect, response) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());
    let scrub_enabled = scrub.range.is_some() || scrub.playhead.is_some();

    // Compute fit-zoom once we have a valid rect and a sized image.
    if state.fit_zoom <= 0.0 && !image.is_empty() {
        let fw = rect.width() / image.w() as f32;
        let fh = rect.height() / image.h() as f32;
        state.fit_zoom = fw.min(fh);
        state.zoom = state.fit_zoom;
    }

    // ── Navigation ──────────────────────────────────────────────────────────
    // Middle-button drag pans.
    if response.dragged_by(egui::PointerButton::Middle) {
        state.pan += response.drag_delta();
    }
    // Right-button drag zooms about the VIEWPORT CENTRE (drag up = in, down =
    // out). We zoom about the centre rather than the cursor on purpose: with a
    // cursor pivot the zoom point slid around as the mouse moved during the drag,
    // which felt unstable — a fixed centre pivot was requested. (The wheel still
    // zooms toward the cursor, where a moving pivot is natural.)
    //
    // Driven from the RAW pointer state (`secondary` button down + per-frame
    // `pointer.delta()`) rather than `dragged_by(Secondary)` alone: egui arbitrates
    // the secondary button between a click, a context-menu trigger and a drag
    // (`is_decidedly_dragging` gates on `could_any_button_be_click`), so the
    // claimed-drag signal is suppressed for small/slow right-drags — which is
    // exactly why the RMB zoom felt dead. We still OR-in `dragged_by` so a fast
    // drag that strays off the viewport rect keeps zooming.
    let (rmb_down, raw_dy) = ui.input(|i| {
        (
            i.pointer.button_down(egui::PointerButton::Secondary),
            i.pointer.delta().y,
        )
    });
    if rmb_down
        && raw_dy != 0.0
        && (response.contains_pointer() || response.dragged_by(egui::PointerButton::Secondary))
    {
        zoom_toward_center(state, 1.0 - raw_dy * ZOOM_DRAG_SPEED);
    }
    // Wheel zooms toward the cursor.
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            zoom_toward_cursor(state, rect, ui, 1.0 + scroll * ZOOM_SPEED);
        }
    }

    // ── Keyboard ────────────────────────────────────────────────────────────
    ui.input(|i| {
        if i.key_pressed(egui::Key::F) {
            state.fit();
        }
        if i.key_pressed(egui::Key::H) || i.key_pressed(egui::Key::Num0) {
            state.home();
        }
        // Ctrl + +/=/- zoom (covers the main-row `-` `=` `_` `+` and the numpad
        // `+` `-`, which egui normalises onto Plus/Minus/Equals). Zoom toward the
        // viewport centre by a fixed step per press.
        if i.modifiers.ctrl {
            if i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals) {
                zoom_toward_center(state, ZOOM_KEY_STEP);
            }
            if i.key_pressed(egui::Key::Minus) {
                zoom_toward_center(state, 1.0 / ZOOM_KEY_STEP);
            }
        }
        toggle_channel(state, i, egui::Key::R, Channel::R);
        toggle_channel(state, i, egui::Key::G, Channel::G);
        toggle_channel(state, i, egui::Key::B, Channel::B);
        toggle_channel(state, i, egui::Key::A, Channel::A);
    });

    // ── Background + checker ────────────────────────────────────────────────
    // Design tokens for this frame: fonts/metrics (`tok`) + theme-resolved
    // semantic colours (`sem`), threaded into the no-`ui` paint helpers below.
    let tok = WidgetsConfig::get(ui);
    let sem = semantic(ui);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(28));
    draw_checker(&painter, rect);

    if image.is_empty() {
        // No image yet: a scrub still works, mapped across the viewport (there are
        // no image bounds to map to until a frame lands).
        let mut scrub_to = None;
        if scrub_enabled
            && is_primary_scrub(&response)
            && let Some(p) = response.interact_pointer_pos()
        {
            scrub_to = Some(((p.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0));
        }
        draw_zoom_label(&painter, rect, state.zoom, &tok, &sem);
        return ViewportResponse {
            response,
            hovered_pixel: None,
            scrub_to,
        };
    }

    // Paint the caller's texture at the current pan/zoom.
    let scaled = egui::vec2(image.w() as f32 * state.zoom, image.h() as f32 * state.zoom);
    let img_rect = egui::Rect::from_center_size(rect.center() + state.pan, scaled);
    painter.image(
        image.texture,
        img_rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    // Scrub (LMB): map the pointer X across the IMAGE bounds — not the whole
    // viewport — to a 0..=1 position the caller turns into a frame.
    let mut scrub_to = None;
    if scrub_enabled
        && is_primary_scrub(&response)
        && let Some(p) = response.interact_pointer_pos()
    {
        scrub_to = Some(((p.x - img_rect.left()) / img_rect.width().max(1.0)).clamp(0.0, 1.0));
    }

    // LMB click → pin pixel readout — ONLY when NOT scrubbing (with scrubbing the
    // left button is the scrub, handled above).
    if !scrub_enabled
        && response.clicked_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
    {
        let (px, py) = screen_to_pixel(pos, img_rect, state.zoom);
        state.picked_pixel = (px < image.w() && py < image.h()).then_some((px, py));
    }

    // The image pixel under the cursor (in bounds), reported back to the caller.
    let hovered_pixel = response.hover_pos().and_then(|hover| {
        let (px, py) = screen_to_pixel(hover, img_rect, state.zoom);
        (px < image.w() && py < image.h()).then_some((px, py))
    });

    // Readout HUD: hover takes priority, the pinned pick is shown otherwise.
    let readout: Option<(u32, u32, bool)> = match hovered_pixel {
        Some((px, py)) => Some((px, py, false)),
        None => state.picked_pixel.map(|(x, y)| (x, y, true)),
    };
    if let Some((px, py, pinned)) = readout {
        let tag = if pinned { "[pin] " } else { "" };
        // With a value (from `read_pixel`) show the full RGBA readout; without one
        // still show the coordinate so the HUD is useful with no callback.
        let label = match read_pixel(px, py) {
            Some([r, g, b, a]) => {
                format!("{tag}({px},{py})  R:{r:.4}  G:{g:.4}  B:{b:.4}  A:{a:.4}")
            }
            None => format!("{tag}({px},{py})"),
        };
        painter.text(
            rect.min + egui::vec2(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            label,
            mono(tok.fonts.sm),
            egui::Color32::WHITE,
        );
    }

    // Crosshair at the pinned pixel.
    if let Some((px, py)) = state.picked_pixel {
        let center = img_rect.min
            + egui::vec2(
                (px as f32 + 0.5) * state.zoom,
                (py as f32 + 0.5) * state.zoom,
            );
        let arm = (state.zoom * 0.5).clamp(3.0, 8.0);
        let col = egui::Color32::from_rgba_unmultiplied(255, 220, 60, 200);
        painter.line_segment(
            [center - egui::vec2(arm, 0.0), center + egui::vec2(arm, 0.0)],
            egui::Stroke::new(tok.metrics.stroke_thin, col),
        );
        painter.line_segment(
            [center - egui::vec2(0.0, arm), center + egui::vec2(0.0, arm)],
            egui::Stroke::new(tok.metrics.stroke_thin, col),
        );
    }

    draw_zoom_label(&painter, rect, state.zoom, &tok, &sem);

    // Optional on-image playhead: a thin vertical line at the current frame's
    // position across the IMAGE bounds (not the viewport). The colour is white and
    // the opacity comes from `playhead_alpha`.
    if let Some(pos) = scrub.playhead
        && scrub.playhead_alpha > 0.0
    {
        let a = (scrub.playhead_alpha.clamp(0.0, 1.0) * 255.0) as u8;
        let x = img_rect.left() + pos.clamp(0.0, 1.0) * img_rect.width();
        painter.line_segment(
            [
                egui::pos2(x, img_rect.top()),
                egui::pos2(x, img_rect.bottom()),
            ],
            egui::Stroke::new(tok.metrics.stroke_thin, egui::Color32::from_white_alpha(a)),
        );
    }

    // Flame-style scrub cursor: while LMB-scrubbing, a 0.5-opacity white vertical
    // line at the (clamped) pointer X with the target frame number beside it. The X
    // is clamped to the image bounds, so the line never runs past first/last frame.
    if let Some((first, last)) = scrub.range
        && is_primary_scrub(&response)
        && let Some(p) = response.interact_pointer_pos()
    {
        draw_scrub_cursor(&painter, rect, img_rect, p, first, last, &tok);
    }

    ViewportResponse {
        response,
        hovered_pixel,
        scrub_to,
    }
}

// ─── helpers ───────────────────────────────────────────────────────────────────

/// Zoom by `factor` about the viewport centre (keyboard / RMB zoom — no cursor
/// pivot). `pan` is already relative to the centre, so it just scales.
fn zoom_toward_center(state: &mut ViewportState, factor: f32) {
    let old = state.zoom;
    state.zoom = (state.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
    let scale = state.zoom / old;
    state.pan *= scale;
}

/// Zoom by `factor` toward the cursor (wheel zoom): the pixel under the cursor
/// stays put while everything scales around it.
fn zoom_toward_cursor(state: &mut ViewportState, rect: egui::Rect, ui: &egui::Ui, factor: f32) {
    let old_zoom = state.zoom;
    state.zoom = (state.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
    let scale = state.zoom / old_zoom;
    let pivot = ui
        .input(|i| i.pointer.hover_pos())
        .unwrap_or_else(|| rect.center());
    let delta = pivot - rect.center();
    state.pan = delta + (state.pan - delta) * scale;
}

/// True when the primary button is scrubbing (dragged or tapped) — the gesture
/// that seeks a sequence in [`show`].
fn is_primary_scrub(response: &egui::Response) -> bool {
    response.dragged_by(egui::PointerButton::Primary)
        || response.clicked_by(egui::PointerButton::Primary)
}

/// Map a screen position to an image pixel, clamped at the top-left origin.
fn screen_to_pixel(pos: egui::Pos2, img_rect: egui::Rect, zoom: f32) -> (u32, u32) {
    let rel = pos - img_rect.min;
    let px = (rel.x / zoom).floor() as i32;
    let py = (rel.y / zoom).floor() as i32;
    (px.max(0) as u32, py.max(0) as u32)
}

/// Toggle a channel-isolate mode on/off with a key (pressing again → RGB).
fn toggle_channel(
    state: &mut ViewportState,
    input: &egui::InputState,
    key: egui::Key,
    mode: Channel,
) {
    if input.key_pressed(key) {
        state.channel = if state.channel == mode {
            Channel::Rgb
        } else {
            mode
        };
    }
}

/// Bottom-right zoom percentage label.
fn draw_zoom_label(
    painter: &egui::Painter,
    rect: egui::Rect,
    zoom: f32,
    tok: &WidgetsConfig,
    sem: &Semantic,
) {
    painter.text(
        rect.max - egui::vec2(8.0, 8.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("{:.0}%", zoom * 100.0),
        mono(tok.fonts.sm),
        sem.weak,
    );
}

/// Draw the Flame-style scrub cursor while LMB-scrubbing a sequence.
///
/// A half-opacity (0.5) white vertical line at the pointer X — clamped to the
/// image bounds (`img_rect`) so it never passes first/last frame — spanning the
/// full viewport height (`rect`). The target frame number is drawn in a small dark
/// backdrop just to the RIGHT of the line at the pointer Y (flipped to the left
/// near the right edge so it stays on screen). `first`/`last` are the sequence's
/// frame-number range; the displayed frame is `first + round(frac * (last-first))`.
fn draw_scrub_cursor(
    painter: &egui::Painter,
    rect: egui::Rect,
    img_rect: egui::Rect,
    pointer: egui::Pos2,
    first: i32,
    last: i32,
    tok: &WidgetsConfig,
) {
    // Position along the scrub range (image X-extent), clamped to [first, last].
    let frac = ((pointer.x - img_rect.left()) / img_rect.width().max(1.0)).clamp(0.0, 1.0);
    let x = img_rect.left() + frac * img_rect.width();

    // Vertical line: white at 0.5 opacity, full viewport height.
    painter.line_segment(
        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
        egui::Stroke::new(
            tok.metrics.stroke_thin,
            egui::Color32::from_white_alpha(128),
        ),
    );

    // Target frame number, laid out so we can size + place a legible backdrop.
    let span = (last - first).max(0) as f32;
    let frame = first + (frac * span).round() as i32;
    let galley =
        painter.layout_no_wrap(format!("{frame}"), mono(tok.fonts.md), egui::Color32::WHITE);
    let pad = egui::vec2(4.0, 2.0);
    let size = galley.size() + pad * 2.0;
    // Vertically centre on the pointer, kept inside the viewport.
    let y = (pointer.y - size.y * 0.5).clamp(rect.top(), (rect.bottom() - size.y).max(rect.top()));
    // Default to the RIGHT of the line; flip left if it would clip the edge.
    let right_x = x + 6.0;
    let bg_x = if right_x + size.x <= rect.right() {
        right_x
    } else {
        x - 6.0 - size.x
    };
    let bg = egui::Rect::from_min_size(egui::pos2(bg_x, y), size);
    painter.rect_filled(
        bg,
        tok.metrics.radius_sm,
        egui::Color32::from_black_alpha(128),
    );
    painter.galley(bg.min + pad, galley, egui::Color32::WHITE);
}

/// Draw the transparent-background checker behind the image.
fn draw_checker(painter: &egui::Painter, rect: egui::Rect) {
    const TILE: f32 = 8.0;
    let dark = egui::Color32::from_gray(38);
    let light = egui::Color32::from_gray(52);
    let cols = ((rect.width() / TILE).ceil() as i32).max(1);
    let rows = ((rect.height() / TILE).ceil() as i32).max(1);
    for row in 0..rows {
        for col in 0..cols {
            let color = if (row + col) % 2 == 0 { dark } else { light };
            let min = rect.min + egui::vec2(col as f32 * TILE, row as f32 * TILE);
            let tile = egui::Rect::from_min_size(min, egui::vec2(TILE, TILE)).intersect(rect);
            painter.rect_filled(tile, 0.0, color);
        }
    }
}
