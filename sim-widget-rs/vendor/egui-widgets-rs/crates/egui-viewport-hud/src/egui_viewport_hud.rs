//! Generic viewport **HUD** — semi-transparent corner stat panels for egui.
//!
//! Paints caller-supplied [`HudBlock`]s anchored to the four corners of a
//! viewport `Rect` (usdview style: gold monospace on a translucent black
//! plate). Multiple blocks at the same corner stack *away* from it (top corners
//! grow downward, bottom corners grow upward). The widget owns NO data — the
//! host formats its stats into `Vec<String>` lines and passes them in; this
//! crate only lays out and paints, returning the painted [`HudExtents`] so the
//! host can keep other overlays (e.g. a hover read-out) clear of the HUD.
//!
//! Gathered from vfx-rs `vfx-view/src/panels/hud.rs`; the USD stage traversal /
//! FPS / GPU-timing gathering stays in the app.

use egui::{Color32, FontId, Pos2, Rect, Ui};

/// Which corner of the viewport a [`HudBlock`] anchors to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl HudCorner {
    #[inline]
    fn is_left(self) -> bool {
        matches!(self, HudCorner::TopLeft | HudCorner::BottomLeft)
    }
    #[inline]
    fn is_top(self) -> bool {
        matches!(self, HudCorner::TopLeft | HudCorner::TopRight)
    }
}

/// Visual style of the HUD plates. [`Default`] matches usdview (gold monospace
/// on a translucent black plate).
#[derive(Debug, Clone)]
pub struct HudStyle {
    /// Text colour.
    pub text_color: Color32,
    /// Translucent plate behind each line.
    pub bg_color: Color32,
    /// Inset from the viewport edge (px).
    pub pad: f32,
    /// Monospace font size (px).
    pub font_size: f32,
    /// Extra vertical gap between lines (px).
    pub line_gap: f32,
}

impl Default for HudStyle {
    fn default() -> Self {
        Self {
            text_color: Color32::from_rgb(218, 165, 32), // usdview gold
            bg_color: Color32::from_rgba_premultiplied(0, 0, 0, 160),
            pad: 6.0,
            font_size: 12.0,
            line_gap: 2.0,
        }
    }
}

/// One stacked group of lines anchored to a corner. Lines read top→down within
/// the block; blocks pile up away from their corner in the order given.
#[derive(Debug, Clone)]
pub struct HudBlock {
    pub corner: HudCorner,
    pub lines: Vec<String>,
}

impl HudBlock {
    pub fn new(corner: HudCorner, lines: Vec<String>) -> Self {
        Self { corner, lines }
    }
}

/// The Y extents actually painted on each side, so the host can place extra
/// overlays clear of the HUD. Top values are the *bottom* of the top blocks;
/// bottom values are the *top* of the bottom blocks. Empty side = the rect edge.
#[derive(Debug, Clone, Copy)]
pub struct HudExtents {
    pub top_left_bottom: f32,
    pub top_right_bottom: f32,
    pub bottom_left_top: f32,
    pub bottom_right_top: f32,
}

/// Draw all HUD blocks onto `rect`. Returns the painted [`HudExtents`].
pub fn draw_hud(ui: &mut Ui, rect: Rect, blocks: &[HudBlock], style: &HudStyle) -> HudExtents {
    let painter = ui.painter();
    let font = FontId::monospace(style.font_size);
    let line_h = style.font_size + style.line_gap;

    let mut ext = HudExtents {
        top_left_bottom: rect.top(),
        top_right_bottom: rect.top(),
        bottom_left_top: rect.bottom(),
        bottom_right_top: rect.bottom(),
    };

    // Per-corner cursors: top corners grow downward, bottom corners grow upward.
    let mut tl_y = rect.top() + style.pad;
    let mut tr_y = rect.top() + style.pad;
    let mut bl_y = rect.bottom() - style.pad;
    let mut br_y = rect.bottom() - style.pad;

    for block in blocks {
        if block.lines.is_empty() {
            continue;
        }
        let block_h = line_h * block.lines.len() as f32;
        let left = block.corner.is_left();

        if block.corner.is_top() {
            let cursor = if left { &mut tl_y } else { &mut tr_y };
            let top = *cursor;
            for (i, line) in block.lines.iter().enumerate() {
                draw_hud_line(
                    painter,
                    line,
                    &font,
                    left,
                    rect,
                    style,
                    top + line_h * i as f32,
                );
            }
            *cursor = top + block_h;
            if left {
                ext.top_left_bottom = tl_y;
            } else {
                ext.top_right_bottom = tr_y;
            }
        } else {
            let cursor = if left { &mut bl_y } else { &mut br_y };
            let top = *cursor - block_h;
            for (i, line) in block.lines.iter().enumerate() {
                draw_hud_line(
                    painter,
                    line,
                    &font,
                    left,
                    rect,
                    style,
                    top + line_h * i as f32,
                );
            }
            *cursor = top;
            if left {
                ext.bottom_left_top = bl_y;
            } else {
                ext.bottom_right_top = br_y;
            }
        }
    }

    ext
}

/// Paint one HUD line (left- or right-aligned) with a translucent plate behind
/// it. Mirrors the original `draw_hud_text` from vfx-view.
fn draw_hud_line(
    painter: &egui::Painter,
    text: &str,
    font: &FontId,
    left_aligned: bool,
    rect: Rect,
    style: &HudStyle,
    y: f32,
) {
    let galley = painter.layout_no_wrap(text.to_string(), font.clone(), style.text_color);
    let x = if left_aligned {
        rect.left() + style.pad
    } else {
        rect.right() - galley.size().x - style.pad
    };
    let pos = Pos2::new(x, y);
    let bg_rect = Rect::from_min_size(
        pos - egui::vec2(2.0, 1.0),
        galley.size() + egui::vec2(4.0, 2.0),
    );
    painter.rect_filled(bg_rect, 2.0, style.bg_color);
    painter.galley(pos, galley, style.text_color);
}
