//! What can be drawn in a pane, and how each kind turns into primitives.
//!
//! Series are **borrowed** descriptors built fresh each frame: the caller owns
//! the data, the widget owns nothing but view state. Aggregation for anything
//! wider than a pixel goes through [`crate::data::Series`], so a series of six
//! million bars costs the same per frame as one of six hundred.
//!
//! # Gaps are `NaN`
//!
//! An indicator has no value until it has enough bars — a 26-period MACD is
//! blank for its first 26. That is a *gap*, not a zero, and drawing it as zero
//! invents a line that dives to the axis. Overlay values are plain `&[f64]`
//! aligned to bar indices, and `NaN` means "no value here": lines break, areas
//! stop, bars are skipped.

use crate::data::{Bar, Series as Data};
use crate::prim::{ChartPainter, ColorMap, GridView, RectPrim};
use crate::scale::{Range, Scale, norm};
use egui::{Color32, Pos2, Rect, Stroke};

/// The geometry a series draws with.
pub enum Plot<'a> {
    /// OHLC bodies and wicks, coloured by direction.
    Candles(&'a Data),
    /// Open/close ticks on a vertical range — the classic OHLC bar.
    Bars(&'a Data),
    /// A connected line, one value per bar index.
    Line(&'a [f64]),
    /// A line with the space beneath it filled.
    Area(&'a [f64]),
    /// A column per bar from zero — volume, a MACD histogram.
    Histogram(&'a [f64]),
    /// A dot per bar, unconnected.
    Scatter(&'a [f64]),
    /// A price-by-time cell field — order-book heat, footprint.
    Grid {
        view: GridView<'a>,
        map: ColorMap<'a>,
    },
}

/// One drawable series and how it looks.
pub struct SeriesSpec<'a> {
    pub plot: Plot<'a>,
    /// Shown in the pane's legend row.
    pub name: String,
    /// Used by line/area/scatter, and as the legend swatch.
    pub color: Color32,
    /// Rising colour for direction-coloured kinds (candles, histogram).
    pub up: Color32,
    /// Falling colour.
    pub down: Color32,
    pub width: f32,
    /// Pin the last value to the price axis as a coloured badge.
    pub badge: bool,
    /// Colour a histogram by the sign of its value rather than one colour.
    pub by_sign: bool,
    pub visible: bool,
}

/// Rising / falling defaults, close to the reference terminal's.
pub const UP: Color32 = Color32::from_rgb(38, 166, 109);
pub const DOWN: Color32 = Color32::from_rgb(221, 66, 68);

impl<'a> SeriesSpec<'a> {
    pub fn new(name: impl Into<String>, plot: Plot<'a>) -> Self {
        Self {
            plot,
            name: name.into(),
            color: Color32::from_rgb(120, 160, 230),
            up: UP,
            down: DOWN,
            width: 1.0,
            badge: false,
            by_sign: false,
            visible: true,
        }
    }

    pub fn candles(name: impl Into<String>, d: &'a Data) -> Self {
        Self {
            badge: true,
            ..Self::new(name, Plot::Candles(d))
        }
    }

    pub fn line(name: impl Into<String>, v: &'a [f64]) -> Self {
        Self::new(name, Plot::Line(v))
    }

    pub fn histogram(name: impl Into<String>, v: &'a [f64]) -> Self {
        Self::new(name, Plot::Histogram(v))
    }

    pub fn color(mut self, c: Color32) -> Self {
        self.color = c;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    pub fn badge(mut self, on: bool) -> Self {
        self.badge = on;
        self
    }

    pub fn by_sign(mut self, on: bool) -> Self {
        self.by_sign = on;
        self
    }

    /// Value at bar `i`, if the series has one there and it is not a gap.
    pub fn value_at(&self, i: usize) -> Option<f64> {
        let v = match &self.plot {
            Plot::Candles(d) | Plot::Bars(d) => d.bar(i).map(|b| b.c),
            Plot::Line(v) | Plot::Area(v) | Plot::Histogram(v) | Plot::Scatter(v) => {
                v.get(i).copied()
            }
            Plot::Grid { .. } => None,
        }?;
        v.is_finite().then_some(v)
    }

    /// Grow `r` to cover this series over `[from, to)` bars.
    ///
    /// Reads through the pyramid for OHLC, so the extent of six million bars
    /// costs the same as the extent of six hundred. Overlays are scanned
    /// directly — they are one value per bar and the visible slice is what is
    /// being asked about.
    pub fn extend_range(&self, r: &mut Range, d: &Data, from: usize, to: usize, cols: usize) {
        if !self.visible {
            return;
        }
        match &self.plot {
            Plot::Candles(s) | Plot::Bars(s) => {
                for (a, b) in columns(from, to, cols) {
                    if let Some(bar) = s.column(a, b) {
                        r.include(bar.h);
                        r.include(bar.l);
                    }
                }
            }
            Plot::Histogram(v) => {
                // A histogram is read against zero, so the baseline is part of
                // its extent even when every value is far from it.
                r.include(0.0);
                for x in v.get(from..to.min(v.len())).unwrap_or(&[]) {
                    r.include(*x);
                }
            }
            Plot::Line(v) | Plot::Area(v) | Plot::Scatter(v) => {
                for x in v.get(from..to.min(v.len())).unwrap_or(&[]) {
                    r.include(*x);
                }
            }
            Plot::Grid { .. } => {
                let _ = d;
            }
        }
    }

    /// Paint into `rect`, mapping bar index and value to screen.
    pub fn draw(&self, p: &mut dyn ChartPainter, cx: &PaneCtx<'_>) {
        if !self.visible {
            return;
        }
        match &self.plot {
            Plot::Candles(s) => self.draw_candles(p, cx, s, true),
            Plot::Bars(s) => self.draw_candles(p, cx, s, false),
            Plot::Line(v) => self.draw_line(p, cx, v, false),
            Plot::Area(v) => self.draw_line(p, cx, v, true),
            Plot::Histogram(v) => self.draw_histogram(p, cx, v),
            Plot::Scatter(v) => self.draw_scatter(p, cx, v),
            Plot::Grid { view, map } => p.grid(cx.rect, view, *map),
        }
    }

    fn draw_candles(&self, p: &mut dyn ChartPainter, cx: &PaneCtx<'_>, s: &Data, bodies: bool) {
        let mut up_bodies = Vec::new();
        let mut down_bodies = Vec::new();
        let mut up_wicks = Vec::new();
        let mut down_wicks = Vec::new();
        // Bodies narrow as bars get closer; below ~3px they are a solid block,
        // so the wick alone carries the shape.
        let w = (cx.col_w * 0.62).clamp(1.0, 24.0);
        for (k, (a, b)) in columns(cx.from, cx.to, cx.cols).enumerate() {
            let Some(bar) = s.column(a, b) else { continue };
            let x = cx.x_of(k);
            let (wicks, bodies_v) = if bar.is_up() {
                (&mut up_wicks, &mut up_bodies)
            } else {
                (&mut down_wicks, &mut down_bodies)
            };
            wicks.push([Pos2::new(x, cx.y_of(bar.h)), Pos2::new(x, cx.y_of(bar.l))]);
            if bodies {
                let (yo, yc) = (cx.y_of(bar.o), cx.y_of(bar.c));
                bodies_v.push(RectPrim::new(
                    Rect::from_min_max(
                        Pos2::new(x - w * 0.5, yo.min(yc)),
                        Pos2::new(x + w * 0.5, yo.max(yc)),
                    ),
                    if bar.is_up() { self.up } else { self.down },
                ));
            } else {
                // OHLC bar: open tick left, close tick right.
                let half = w * 0.5;
                wicks.push([
                    Pos2::new(x - half, cx.y_of(bar.o)),
                    Pos2::new(x, cx.y_of(bar.o)),
                ]);
                wicks.push([
                    Pos2::new(x, cx.y_of(bar.c)),
                    Pos2::new(x + half, cx.y_of(bar.c)),
                ]);
            }
        }
        p.segments(&up_wicks, Stroke::new(self.width, self.up));
        p.segments(&down_wicks, Stroke::new(self.width, self.down));
        p.rects(&up_bodies);
        p.rects(&down_bodies);
    }

    fn draw_line(&self, p: &mut dyn ChartPainter, cx: &PaneCtx<'_>, v: &[f64], fill: bool) {
        // Split at gaps rather than joining across them: a line that leaps a
        // blank stretch asserts data that is not there.
        let mut run: Vec<Pos2> = Vec::new();
        let flush = |p: &mut dyn ChartPainter, run: &mut Vec<Pos2>| {
            if run.len() >= 2 {
                if fill {
                    let mut poly = run.clone();
                    let base = cx.rect.bottom();
                    poly.push(Pos2::new(run[run.len() - 1].x, base));
                    poly.push(Pos2::new(run[0].x, base));
                    // Flat translucent fill; the line itself carries the shape.
                    let c = self.color.linear_multiply(0.25);
                    for w in poly.windows(2) {
                        let r = Rect::from_two_pos(w[0], Pos2::new(w[1].x, base));
                        p.rects(&[RectPrim::new(r, c)]);
                    }
                }
                p.polyline(run, Stroke::new(self.width.max(1.0), self.color));
            }
            run.clear();
        };
        for (k, (a, b)) in columns(cx.from, cx.to, cx.cols).enumerate() {
            match agg_mean(v, a, b) {
                Some(y) => run.push(Pos2::new(cx.x_of(k), cx.y_of(y))),
                None => flush(p, &mut run),
            }
        }
        flush(p, &mut run);
    }

    fn draw_histogram(&self, p: &mut dyn ChartPainter, cx: &PaneCtx<'_>, v: &[f64]) {
        let w = (cx.col_w * 0.7).clamp(1.0, 24.0);
        let zero = cx.y_of(0.0);
        let mut prims = Vec::new();
        for (k, (a, b)) in columns(cx.from, cx.to, cx.cols).enumerate() {
            let Some(y) = agg_sum(v, a, b) else { continue };
            let x = cx.x_of(k);
            let top = cx.y_of(y);
            let c = if self.by_sign {
                if y >= 0.0 { self.up } else { self.down }
            } else {
                self.color
            };
            prims.push(RectPrim::new(
                Rect::from_min_max(
                    Pos2::new(x - w * 0.5, top.min(zero)),
                    Pos2::new(x + w * 0.5, top.max(zero)),
                ),
                c,
            ));
        }
        p.rects(&prims);
    }

    fn draw_scatter(&self, p: &mut dyn ChartPainter, cx: &PaneCtx<'_>, v: &[f64]) {
        let r = (cx.col_w * 0.25).clamp(1.0, 3.0);
        let mut prims = Vec::new();
        for (k, (a, b)) in columns(cx.from, cx.to, cx.cols).enumerate() {
            let Some(y) = agg_mean(v, a, b) else { continue };
            let c = Pos2::new(cx.x_of(k), cx.y_of(y));
            prims.push(RectPrim::new(
                Rect::from_center_size(c, egui::vec2(r * 2.0, r * 2.0)),
                self.color,
            ));
        }
        p.rects(&prims);
    }
}

/// Everything a series needs to place itself in a pane.
pub struct PaneCtx<'a> {
    pub rect: Rect,
    pub scale: Scale,
    pub range: Range,
    /// First and last visible bar index.
    pub from: usize,
    pub to: usize,
    /// Pixel columns the visible range is split into.
    pub cols: usize,
    /// Width of one column in points.
    pub col_w: f32,
    pub _marker: std::marker::PhantomData<&'a ()>,
}

impl PaneCtx<'_> {
    /// Centre x of drawn column `k`.
    pub fn x_of(&self, k: usize) -> f32 {
        self.rect.left() + (k as f32 + 0.5) * self.col_w
    }

    /// Screen y of a value.
    pub fn y_of(&self, v: f64) -> f32 {
        let t = norm(self.scale, self.range, v) as f32;
        self.rect.bottom() - t * self.rect.height()
    }

    /// Bar index under a screen x, clamped to the visible range.
    pub fn bar_at(&self, x: f32) -> usize {
        let k = ((x - self.rect.left()) / self.col_w.max(0.001))
            .floor()
            .max(0.0) as usize;
        let span = self.to.saturating_sub(self.from);
        let per = (span as f64 / self.cols.max(1) as f64).max(1.0);
        (self.from + (k as f64 * per) as usize).min(self.to.saturating_sub(1))
    }
}

/// The `[from, to)` raw-bar span of each drawn column.
///
/// Column boundaries are computed in bar space rather than accumulated in
/// floats, so the last column ends exactly on `to` instead of a rounding error
/// short of it.
pub fn columns(from: usize, to: usize, cols: usize) -> impl Iterator<Item = (usize, usize)> {
    let span = to.saturating_sub(from);
    let cols = cols.max(1).min(span.max(1));
    (0..cols).map(move |k| {
        let a = from + span * k / cols;
        let b = from + span * (k + 1) / cols;
        (a, b.max(a + 1))
    })
}

/// Mean of the finite values in `[a, b)`, or `None` when all are gaps.
fn agg_mean(v: &[f64], a: usize, b: usize) -> Option<f64> {
    let s = v.get(a..b.min(v.len()))?;
    let mut sum = 0.0;
    let mut n = 0u32;
    for x in s {
        if x.is_finite() {
            sum += *x;
            n += 1;
        }
    }
    (n > 0).then(|| sum / n as f64)
}

/// Sum of the finite values in `[a, b)` — what aggregating volume means.
fn agg_sum(v: &[f64], a: usize, b: usize) -> Option<f64> {
    let s = v.get(a..b.min(v.len()))?;
    let mut sum = 0.0;
    let mut any = false;
    for x in s {
        if x.is_finite() {
            sum += *x;
            any = true;
        }
    }
    any.then_some(sum)
}

/// The OHLC readout for a hovered bar, as the reference terminal prints it.
pub fn ohlc_label(b: &Bar, prev_close: Option<f64>) -> String {
    let change = prev_close.map(|p| (b.c - p, if p != 0.0 { (b.c - p) / p * 100.0 } else { 0.0 }));
    match change {
        Some((d, pct)) => format!(
            "O {:.2}  H {:.2}  L {:.2}  C {:.2}  {:+.2} ({:+.2}%)  Vol {:.0}",
            b.o, b.h, b.l, b.c, d, pct, b.v
        ),
        None => format!(
            "O {:.2}  H {:.2}  L {:.2}  C {:.2}  Vol {:.0}",
            b.o, b.h, b.l, b.c, b.v
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Columns must tile the range exactly: no gap, no overlap, ending on `to`.
    #[test]
    fn columns_tile_the_range() {
        for (from, to, cols) in [(0, 1000, 300), (7, 993, 128), (0, 5, 100), (10, 11, 4)] {
            let v: Vec<_> = columns(from, to, cols).collect();
            assert!(!v.is_empty());
            assert_eq!(v[0].0, from, "first column must start at from");
            assert_eq!(v[v.len() - 1].1, to.max(from + 1), "last must end at to");
            for w in v.windows(2) {
                assert_eq!(w[0].1, w[1].0, "columns must not gap or overlap: {v:?}");
            }
        }
    }

    /// More columns than bars must not produce empty ones.
    #[test]
    fn columns_never_collapse_to_nothing() {
        for (a, b) in columns(0, 3, 50) {
            assert!(b > a, "empty column {a}..{b}");
        }
    }

    /// A gap is not a zero: aggregating over only-gaps yields nothing, so the
    /// line breaks instead of diving to the axis.
    #[test]
    fn gaps_are_not_zeros() {
        let v = [f64::NAN, f64::NAN, 4.0, 6.0];
        assert_eq!(agg_mean(&v, 0, 2), None);
        assert_eq!(agg_mean(&v, 2, 4), Some(5.0));
        assert_eq!(agg_mean(&v, 1, 3), Some(4.0), "mixed must ignore the gap");
        assert_eq!(agg_sum(&v, 0, 2), None);
        assert_eq!(agg_sum(&v, 2, 4), Some(10.0));
    }

    /// A histogram is read against zero, so zero is always in its extent.
    #[test]
    fn a_histogram_range_includes_its_baseline() {
        let v = [5.0, 6.0, 7.0];
        let s = SeriesSpec::histogram("h", &v);
        let d = crate::data::Series::regular(0, 1000);
        let mut r = Range::empty();
        s.extend_range(&mut r, &d, 0, 3, 3);
        assert_eq!(r.lo, 0.0, "baseline must be included");
        assert_eq!(r.hi, 7.0);
    }
}
