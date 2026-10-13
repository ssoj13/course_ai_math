//! The drawing seam: chart geometry on one side, a renderer on the other.
//!
//! The chart never touches a painter directly. It emits batches of primitives
//! and something else turns them into pixels — [`EguiPainter`] today, a wgpu
//! backend later without the chart knowing.
//!
//! # Why a seam and not two backends now
//!
//! With the level-of-detail pyramid ([`crate::data`]) the number of primitives
//! per frame is bounded by the pane's pixel width, not by the dataset size:
//! ~2000 columns of two primitives each, whatever a caller hands over. That is
//! nothing for egui's tessellator, so a second backend would buy nothing yet.
//!
//! What *will* need one is [`ChartPainter::grid`] — a price-by-time cell field
//! (order-book heat, footprint). On the CPU that is tens of thousands of
//! rectangles a frame; on the GPU it is one textured quad with a colour map in
//! the shader, and mips come free. So the grid primitive is in the seam from
//! the start even though the CPU implementation expands it the slow way: when
//! the GPU backend arrives, the chart above it does not change.
//!
//! # Types
//!
//! `Pos2` / `Rect` / `Color32` / `Stroke` are re-exports of `emath` / `ecolor`,
//! which are separate crates from `egui`. A future move of the geometry layers
//! into a renderer-agnostic core is therefore an import change, not a rewrite.

use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke};

/// A filled rectangle — a candle body, a histogram bar, a shaded band.
#[derive(Clone, Copy, Debug)]
pub struct RectPrim {
    pub rect: Rect,
    pub fill: Color32,
}

impl RectPrim {
    pub fn new(rect: Rect, fill: Color32) -> Self {
        Self { rect, fill }
    }
}

/// A read-only view of a price-by-time cell field.
///
/// Rows are price buckets (low to high), columns are time. Borrowed rather
/// than owned: the caller keeps the field and the widget reads it for a frame.
#[derive(Clone, Copy, Debug)]
pub struct GridView<'a> {
    /// Row-major, `rows * cols` values.
    pub cells: &'a [f32],
    pub rows: usize,
    pub cols: usize,
    /// Value mapped to the top of the colour map; cells at or above it clamp.
    pub max: f32,
}

impl<'a> GridView<'a> {
    pub fn new(cells: &'a [f32], rows: usize, cols: usize, max: f32) -> Self {
        Self {
            cells,
            rows,
            cols,
            max,
        }
    }

    /// `0..=1` intensity of a cell, or `None` when out of range.
    pub fn at(&self, row: usize, col: usize) -> Option<f32> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        let v = *self.cells.get(row * self.cols + col)?;
        Some(if self.max > 0.0 {
            (v / self.max).clamp(0.0, 1.0)
        } else {
            0.0
        })
    }
}

/// Maps a `0..=1` intensity to a colour.
///
/// Deliberately a function rather than a palette type: the workspace already
/// has `egui-ramp`'s `Gradient::sample(t) -> Color32` **with an editor**, so a
/// caller passes that and their heat ramp is editable with a widget that
/// already exists. Growing a colour-map type here would duplicate it.
pub type ColorMap<'a> = &'a dyn Fn(f32) -> Color32;

/// What a chart draws through.
///
/// Batched on purpose: the candle bodies of a whole pane arrive in one call,
/// so a GPU backend can fill one buffer instead of taking 2000 calls.
pub trait ChartPainter {
    /// Restrict subsequent drawing to `rect`. Panes clip to their own area so
    /// a series cannot bleed into the pane above.
    fn clip(&mut self, rect: Rect);

    fn rects(&mut self, prims: &[RectPrim]);

    /// Independent segments — candle wicks, tick marks.
    fn segments(&mut self, segs: &[[Pos2; 2]], stroke: Stroke);

    /// One connected path — a moving average, an RSI line.
    fn polyline(&mut self, pts: &[Pos2], stroke: Stroke);

    /// A dashed horizontal rule: grid lines and level guides.
    fn dashed_hline(&mut self, y: f32, x0: f32, x1: f32, stroke: Stroke, dash: f32, gap: f32);

    /// A dashed vertical rule.
    fn dashed_vline(&mut self, x: f32, y0: f32, y1: f32, stroke: Stroke, dash: f32, gap: f32);

    /// Draw `text`, and report the box it occupied so a caller can lay out
    /// around it (badges size themselves to their label).
    fn text(&mut self, at: Pos2, align: Align2, text: &str, font: FontId, color: Color32) -> Rect;

    /// Paint a cell field into `rect`, row 0 at the **bottom** (low price).
    ///
    /// The one primitive that will justify a GPU backend; see the module docs.
    fn grid(&mut self, rect: Rect, view: &GridView<'_>, map: ColorMap<'_>);
}

/// [`ChartPainter`] over an `egui::Painter`.
pub struct EguiPainter<'a> {
    painter: &'a egui::Painter,
    clip: Option<egui::Painter>,
}

impl<'a> EguiPainter<'a> {
    pub fn new(painter: &'a egui::Painter) -> Self {
        Self {
            painter,
            clip: None,
        }
    }

    fn p(&self) -> &egui::Painter {
        self.clip.as_ref().unwrap_or(self.painter)
    }
}

impl ChartPainter for EguiPainter<'_> {
    fn clip(&mut self, rect: Rect) {
        self.clip = Some(self.painter.with_clip_rect(rect));
    }

    fn rects(&mut self, prims: &[RectPrim]) {
        let p = self.p();
        for r in prims {
            // A body of zero height is a doji: it must still be a visible line
            // rather than nothing at all.
            let rect = if r.rect.height() < 1.0 {
                Rect::from_min_max(r.rect.min, Pos2::new(r.rect.max.x, r.rect.min.y + 1.0))
            } else {
                r.rect
            };
            p.rect_filled(rect, 0.0, r.fill);
        }
    }

    fn segments(&mut self, segs: &[[Pos2; 2]], stroke: Stroke) {
        let p = self.p();
        for s in segs {
            p.line_segment(*s, stroke);
        }
    }

    fn polyline(&mut self, pts: &[Pos2], stroke: Stroke) {
        if pts.len() >= 2 {
            self.p().add(egui::Shape::line(pts.to_vec(), stroke));
        }
    }

    fn dashed_hline(&mut self, y: f32, x0: f32, x1: f32, stroke: Stroke, dash: f32, gap: f32) {
        let pts = [Pos2::new(x0, y), Pos2::new(x1, y)];
        self.p()
            .extend(egui::Shape::dashed_line(&pts, stroke, dash, gap));
    }

    fn dashed_vline(&mut self, x: f32, y0: f32, y1: f32, stroke: Stroke, dash: f32, gap: f32) {
        let pts = [Pos2::new(x, y0), Pos2::new(x, y1)];
        self.p()
            .extend(egui::Shape::dashed_line(&pts, stroke, dash, gap));
    }

    fn text(&mut self, at: Pos2, align: Align2, text: &str, font: FontId, color: Color32) -> Rect {
        self.p().text(at, align, text, font, color)
    }

    fn grid(&mut self, rect: Rect, view: &GridView<'_>, map: ColorMap<'_>) {
        if view.rows == 0 || view.cols == 0 {
            return;
        }
        // The slow expansion the GPU backend exists to replace. Cells smaller
        // than a pixel are still drawn a pixel wide: dropping them would lose
        // exactly the thin high-intensity bands a heat map is read for.
        let cw = (rect.width() / view.cols as f32).max(1.0);
        let ch = (rect.height() / view.rows as f32).max(1.0);
        let mut prims = Vec::with_capacity(view.rows * view.cols);
        for row in 0..view.rows {
            for col in 0..view.cols {
                let Some(t) = view.at(row, col) else { continue };
                if t <= 0.0 {
                    continue;
                }
                let x = rect.left() + col as f32 * rect.width() / view.cols as f32;
                // Row 0 is the LOW price, and screen y grows downward.
                let y = rect.bottom() - (row + 1) as f32 * rect.height() / view.rows as f32;
                prims.push(RectPrim::new(
                    Rect::from_min_size(Pos2::new(x, y), egui::vec2(cw, ch)),
                    map(t),
                ));
            }
        }
        self.rects(&prims);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_intensity_is_normalised_and_clamped() {
        let cells = [0.0, 5.0, 10.0, 20.0];
        let g = GridView::new(&cells, 2, 2, 10.0);
        assert_eq!(g.at(0, 0), Some(0.0));
        assert_eq!(g.at(0, 1), Some(0.5));
        assert_eq!(g.at(1, 0), Some(1.0));
        assert_eq!(g.at(1, 1), Some(1.0), "above max must clamp, not overflow");
        assert_eq!(g.at(2, 0), None);
        assert_eq!(g.at(0, 2), None);
    }

    /// A field whose max is zero must not divide by it.
    #[test]
    fn an_empty_grid_is_not_a_division_by_zero() {
        let cells = [0.0; 4];
        let g = GridView::new(&cells, 2, 2, 0.0);
        assert_eq!(g.at(1, 1), Some(0.0));
    }
}
