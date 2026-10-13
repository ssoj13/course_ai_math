//! A reusable charting widget for egui: stacked panes over one shared time
//! axis, each with its own value scale.
//!
//! Carries no application types. Candles, lines, histograms and levels are
//! plain data the caller owns and the widget borrows for the frame.
//!
//! # Level of detail is the architecture, not an optimisation
//!
//! The target is that nothing dies when a caller hands over far more data than
//! anyone would sensibly display — a year of 5-second bars is 6.3 million of
//! them. Reducing that to ~2000 pixel columns every frame is 378M reads a
//! second at 60fps, which is not survivable.
//!
//! It works because OHLC aggregation is **associative**: `high` is a max,
//! `low` a min, `open` the first and `close` the last. A pyramid of levels is
//! therefore *exact* rather than approximate — a level-3 bar equals what a
//! direct fold of 512 raw bars would give. Drawing picks the level where a
//! pixel column covers 1..8 bars, so the work per frame is bounded by the
//! pane's width whatever the dataset size.
//!
//! That has to shape the API rather than hide behind it: the caller owns a
//! series object that *holds* its pyramid and updates it on append, and the
//! widget borrows it. Rebuilding a pyramid every frame would defeat the whole
//! arrangement.
//!
//! See `CLAUDE.md` beside this crate for the measured numbers, the reference
//! this is modelled on, and why there is one renderer behind a seam rather
//! than two backends.

pub mod data;
pub mod prim;
pub mod scale;
pub mod series;

pub use data::{Bar, FANOUT, Series, Time};
pub use prim::{ChartPainter, ColorMap, EguiPainter, GridView, RectPrim};
pub use scale::{Range, Scale, Tick};
pub use series::{DOWN, PaneCtx, Plot, SeriesSpec, UP, ohlc_label};

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};

/// Colours and metrics. Dark by default, because a chart is looked at for
/// hours and a bright field behind thin coloured lines is what makes one
/// unreadable.
#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: Color32,
    pub grid: Color32,
    pub axis_text: Color32,
    pub legend_text: Color32,
    pub crosshair: Color32,
    pub pane_split: Color32,
    /// Width of the value-axis gutter on the right.
    pub axis_w: f32,
    /// Height of the time axis along the bottom.
    pub time_h: f32,
    pub font: f32,
    /// Fraction of the pane left blank past the last bar, so the newest candle
    /// does not sit against the axis.
    pub headroom: f32,
    pub dash: f32,
    pub gap: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: Color32::from_rgb(14, 17, 23),
            grid: Color32::from_rgb(38, 43, 54),
            axis_text: Color32::from_rgb(140, 149, 166),
            legend_text: Color32::from_rgb(190, 198, 212),
            crosshair: Color32::from_rgb(120, 130, 150),
            pane_split: Color32::from_rgb(30, 35, 45),
            axis_w: 62.0,
            time_h: 20.0,
            font: 11.0,
            headroom: 0.06,
            dash: 3.0,
            gap: 4.0,
        }
    }
}

/// A horizontal reference level, optionally shading the zone beyond it.
///
/// RSI 70/30 and Stochastic 80/20 in the reference terminal.
pub struct Guide {
    pub value: f64,
    pub color: Color32,
    /// Fill between `value` and this, when set.
    pub band_to: Option<f64>,
    pub label: bool,
}

impl Guide {
    pub fn level(value: f64) -> Self {
        Self {
            value,
            color: Color32::from_rgb(150, 120, 60),
            band_to: None,
            label: true,
        }
    }

    pub fn band_to(mut self, other: f64) -> Self {
        self.band_to = Some(other);
        self
    }

    pub fn color(mut self, c: Color32) -> Self {
        self.color = c;
        self
    }
}

/// One stacked pane: its own value axis, its own series.
pub struct Pane<'a> {
    pub series: Vec<SeriesSpec<'a>>,
    pub guides: Vec<Guide>,
    /// Share of the vertical space, relative to the other panes.
    pub weight: f32,
    /// Overrides the state's per-pane scale when set.
    pub scale: Option<Scale>,
    /// Legend prefix, e.g. `"RSI(6,14,24)"`.
    pub title: String,
}

impl<'a> Pane<'a> {
    pub fn new() -> Self {
        Self {
            series: Vec::new(),
            guides: Vec::new(),
            weight: 1.0,
            scale: None,
            title: String::new(),
        }
    }

    pub fn price() -> Self {
        Self {
            weight: 3.0,
            ..Self::new()
        }
    }

    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }

    pub fn weight(mut self, w: f32) -> Self {
        self.weight = w.max(0.1);
        self
    }

    pub fn series(mut self, s: SeriesSpec<'a>) -> Self {
        self.series.push(s);
        self
    }

    pub fn guide(mut self, g: Guide) -> Self {
        self.guides.push(g);
        self
    }
}

impl Default for Pane<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// View state the host owns across frames.
///
/// Kept here rather than in egui memory so a host can save it with a layout,
/// restore it, or drive it from elsewhere (a range button, a linked chart).
#[derive(Clone, Debug)]
pub struct ChartState {
    /// First visible bar.
    pub first: usize,
    /// Bars across the pane.
    pub visible: usize,
    pub selected: Option<usize>,
    /// Per pane index; grown on demand.
    pub scales: Vec<Scale>,
    /// Stay pinned to the newest bar as data is appended.
    pub follow: bool,
}

impl Default for ChartState {
    fn default() -> Self {
        Self {
            first: 0,
            visible: 240,
            selected: None,
            scales: Vec::new(),
            follow: true,
        }
    }
}

/// Smallest window the view may zoom to. Below a handful of bars there is
/// nothing left to read and the maths starts dividing by almost nothing.
const MIN_VISIBLE: usize = 8;

impl ChartState {
    pub fn scale_of(&self, pane: usize) -> Scale {
        self.scales.get(pane).copied().unwrap_or_default()
    }

    pub fn set_scale(&mut self, pane: usize, s: Scale) {
        if self.scales.len() <= pane {
            self.scales.resize(pane + 1, Scale::default());
        }
        self.scales[pane] = s;
    }

    /// Clamp the window to the data, keeping `follow` honoured.
    pub fn clamp(&mut self, len: usize) {
        self.visible = self.visible.clamp(MIN_VISIBLE, len.max(MIN_VISIBLE));
        let max_first = len.saturating_sub(self.visible);
        if self.follow {
            self.first = max_first;
        } else {
            self.first = self.first.min(max_first);
        }
    }

    /// Zoom by `factor` (<1 zooms in), holding the bar under `anchor` (`0..=1`
    /// across the pane) in place.
    ///
    /// Anchored so the wheel keeps what is under the cursor under the cursor,
    /// which is the difference between zooming and hunting.
    pub fn zoom(&mut self, factor: f32, anchor: f32, len: usize) {
        let at = self.first as f32 + anchor.clamp(0.0, 1.0) * self.visible as f32;
        let want = ((self.visible as f32) * factor).round() as i64;
        let new_vis = want.clamp(MIN_VISIBLE as i64, len.max(MIN_VISIBLE) as i64) as usize;
        let new_first = at - anchor.clamp(0.0, 1.0) * new_vis as f32;
        self.visible = new_vis;
        self.first = new_first.max(0.0) as usize;
        // Any deliberate zoom that does not end at the right edge means the
        // operator is looking at history, so stop dragging them back.
        self.follow = self.first + self.visible >= len;
        self.clamp(len);
    }

    /// Pan by whole bars, **positive going back in time**.
    ///
    /// The sign follows the gesture rather than the index: dragging the plot to
    /// the right pulls older bars into view, and that is a positive `bars`.
    /// Stated because it is genuinely confusable — it caught me while writing
    /// `follow_tracks_the_right_edge`.
    pub fn pan(&mut self, bars: i64, len: usize) {
        let f = self.first as i64 - bars;
        self.first = f.max(0) as usize;
        self.follow = self.first + self.visible >= len;
        self.clamp(len);
    }
}

/// What the chart reports back after a frame.
#[derive(Clone, Debug, Default)]
pub struct ChartResponse {
    /// Bar under the pointer.
    pub hovered: Option<usize>,
    /// Bar clicked with the primary button this frame.
    pub clicked: Option<usize>,
    pub response: Option<egui::Response>,
}

/// The widget.
pub struct Chart<'a> {
    panes: Vec<Pane<'a>>,
    data: &'a Series,
    theme: Theme,
}

impl<'a> Chart<'a> {
    /// `data` supplies the time axis and the bar count every pane is indexed
    /// against — panes may draw other series, but they share this clock.
    ///
    /// No id: the widget keeps nothing in egui memory. Everything that has to
    /// survive a frame is in the caller's [`ChartState`], which is what lets a
    /// host save it with a layout or drive two charts from one view.
    pub fn new(data: &'a Series) -> Self {
        Self {
            panes: Vec::new(),
            data,
            theme: Theme::default(),
        }
    }

    pub fn theme(mut self, t: Theme) -> Self {
        self.theme = t;
        self
    }

    pub fn pane(mut self, p: Pane<'a>) -> Self {
        self.panes.push(p);
        self
    }

    pub fn show(self, ui: &mut Ui, st: &mut ChartState, size: Vec2) -> ChartResponse {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let th = &self.theme;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, th.bg);

        let len = self.data.len();
        if len == 0 || self.panes.is_empty() {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "no data",
                FontId::proportional(13.0),
                th.axis_text,
            );
            return ChartResponse {
                response: Some(resp),
                ..Default::default()
            };
        }
        st.clamp(len);

        // Plot area: everything but the right gutter and the time axis.
        let plot = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.right() - th.axis_w, rect.bottom() - th.time_h),
        );
        let usable_w = plot.width() * (1.0 - th.headroom);
        let from = st.first;
        let to = (st.first + st.visible).min(len);
        let cols = ((usable_w / 3.0).floor() as usize).clamp(1, to.saturating_sub(from).max(1));
        let col_w = usable_w / cols as f32;

        let out = self.interact(ui, st, &resp, plot, col_w, cols, from, to, len);

        // Time divisions, computed once: the labels along the bottom and the
        // vertical rules through every pane are the same divisions, so a rule
        // always has a label under it.
        let t_ticks = {
            let (t0, t1) = (
                self.data.time().at(from),
                self.data.time().at(to.saturating_sub(1)),
            );
            let target = (plot.width() / 90.0).round().max(2.0) as usize;
            scale::time_ticks(t0, t1, target)
        };
        let t_xs: Vec<f32> = t_ticks
            .iter()
            .map(|t| {
                let bar = bar_of_time(self.data, t.value as i64, from, to);
                plot.left() + (column_of(bar, from, to, cols) as f32 + 0.5) * col_w
            })
            .collect();

        // Pane rects, split by weight.
        let total: f32 = self.panes.iter().map(|p| p.weight).sum::<f32>().max(0.001);
        let mut y = plot.top();
        let mut p = EguiPainter::new(&painter);
        for (i, pane) in self.panes.iter().enumerate() {
            let h = plot.height() * pane.weight / total;
            let pr = Rect::from_min_max(Pos2::new(plot.left(), y), Pos2::new(plot.right(), y + h));
            y += h;
            self.draw_pane(
                &mut p,
                st,
                pane,
                i,
                pr,
                from,
                to,
                cols,
                col_w,
                out.hovered,
                &t_xs,
            );
            if i + 1 < self.panes.len() {
                painter.hline(rect.x_range(), y, Stroke::new(1.0, th.pane_split));
            }
        }
        self.draw_time_axis(&mut p, plot, &t_ticks, &t_xs);
        out
    }
}

impl Chart<'_> {
    /// Wheel zoom, drag pan, click select, right-click menu.
    #[allow(clippy::too_many_arguments)]
    fn interact(
        &self,
        ui: &Ui,
        st: &mut ChartState,
        resp: &egui::Response,
        plot: Rect,
        col_w: f32,
        cols: usize,
        from: usize,
        to: usize,
        len: usize,
    ) -> ChartResponse {
        let mut out = ChartResponse {
            response: Some(resp.clone()),
            ..Default::default()
        };
        let bar_at = |x: f32| -> usize {
            let k = ((x - plot.left()) / col_w.max(0.001)).floor().max(0.0) as usize;
            let span = to.saturating_sub(from);
            let per = (span as f64 / cols.max(1) as f64).max(1.0);
            (from + (k as f64 * per) as usize).min(to.saturating_sub(1))
        };

        if let Some(pos) = resp.hover_pos()
            && plot.contains(pos)
        {
            out.hovered = Some(bar_at(pos.x));
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.1 {
                let anchor = (pos.x - plot.left()) / plot.width().max(1.0);
                // Multiplicative, so a notch in and a notch out return to where
                // you were instead of drifting.
                st.zoom(if scroll > 0.0 { 0.85 } else { 1.0 / 0.85 }, anchor, len);
            }
        }

        // Middle drag pans; primary drag pans too, because that is what a
        // pointer expects on a plot and the click case is handled separately.
        let dragging = resp.dragged_by(egui::PointerButton::Middle)
            || resp.dragged_by(egui::PointerButton::Primary);
        if dragging {
            let dx = resp.drag_delta().x;
            if dx.abs() > 0.0 {
                let per = (to.saturating_sub(from) as f32 / cols.max(1) as f32).max(1.0);
                let bars = (dx / col_w.max(0.001) * per).round() as i64;
                if bars != 0 {
                    st.pan(bars, len);
                }
            }
        }
        if resp.clicked()
            && let Some(pos) = resp.interact_pointer_pos()
            && plot.contains(pos)
        {
            let b = bar_at(pos.x);
            st.selected = Some(b);
            out.clicked = Some(b);
        }

        let n_panes = self.panes.len();
        resp.clone().context_menu(|ui| {
            ui.label("Scale");
            for pane in 0..n_panes {
                let name = if self.panes[pane].title.is_empty() {
                    format!("pane {pane}")
                } else {
                    self.panes[pane].title.clone()
                };
                ui.horizontal(|ui| {
                    ui.label(name);
                    let mut s = st.scale_of(pane);
                    for (label, v) in [
                        ("lin", Scale::Linear),
                        ("log", Scale::Log),
                        ("%", Scale::Percent),
                    ] {
                        if ui.selectable_label(s == v, label).clicked() {
                            s = v;
                        }
                    }
                    st.set_scale(pane, s);
                });
            }
            ui.separator();
            if ui.button("Fit all").clicked() {
                st.first = 0;
                st.visible = len;
                st.follow = true;
                ui.close();
            }
            if ui.button("Jump to latest").clicked() {
                st.follow = true;
                st.clamp(len);
                ui.close();
            }
            let mut follow = st.follow;
            if ui.checkbox(&mut follow, "Follow new bars").changed() {
                st.follow = follow;
            }
        });
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_pane(
        &self,
        p: &mut EguiPainter<'_>,
        st: &ChartState,
        pane: &Pane<'_>,
        idx: usize,
        rect: Rect,
        from: usize,
        to: usize,
        cols: usize,
        col_w: f32,
        hovered: Option<usize>,
        time_xs: &[f32],
    ) {
        let th = &self.theme;
        let scale = pane.scale.unwrap_or_else(|| st.scale_of(idx));

        // Extent of everything visible, plus any guide, so a level line cannot
        // sit off-pane where it says nothing.
        let mut range = Range::empty();
        for s in &pane.series {
            s.extend_range(&mut range, self.data, from, to, cols);
        }
        for g in &pane.guides {
            range.include(g.value);
            if let Some(b) = g.band_to {
                range.include(b);
            }
        }
        let range = range.padded(0.06);

        let cx = PaneCtx {
            rect,
            scale,
            range,
            from,
            to,
            cols,
            col_w,
            _marker: std::marker::PhantomData,
        };
        let font = FontId::proportional(th.font);
        let axis_x = rect.right() + 6.0;

        p.clip(rect);

        // Grid: one line per value tick, spaced by the pane's own height so a
        // short pane gets fewer lines rather than a block of text.
        let target = (rect.height() / 46.0).round().max(2.0) as usize;
        let ticks = scale::value_ticks(scale, range, target);
        for t in &ticks {
            let y = cx.y_of(t.value);
            p.dashed_hline(
                y,
                rect.left(),
                rect.right(),
                Stroke::new(1.0, th.grid),
                th.dash,
                th.gap,
            );
        }
        // Vertical rules on the time divisions, through every pane, so a
        // feature can be read across panes at the same instant — which is the
        // reason panes are stacked in the first place.
        for x in time_xs {
            p.dashed_vline(
                *x,
                rect.top(),
                rect.bottom(),
                Stroke::new(1.0, th.grid),
                th.dash,
                th.gap,
            );
        }

        // Guides under the series: a band is context, not content.
        for g in &pane.guides {
            if let Some(b) = g.band_to {
                let (y0, y1) = (cx.y_of(g.value), cx.y_of(b));
                p.rects(&[RectPrim::new(
                    Rect::from_min_max(
                        Pos2::new(rect.left(), y0.min(y1)),
                        Pos2::new(rect.right(), y0.max(y1)),
                    ),
                    g.color.linear_multiply(0.10),
                )]);
            }
        }
        for g in &pane.guides {
            let y = cx.y_of(g.value);
            p.dashed_hline(
                y,
                rect.left(),
                rect.right(),
                Stroke::new(1.0, g.color),
                th.dash * 2.0,
                th.gap,
            );
        }

        for s in &pane.series {
            s.draw(p, &cx);
        }

        // Crosshair, and the selection kept from a click.
        if let Some(h) = hovered.or(st.selected) {
            let k = column_of(h, from, to, cols);
            let x = cx.x_of(k);
            p.dashed_vline(
                x,
                rect.top(),
                rect.bottom(),
                Stroke::new(1.0, th.crosshair),
                th.dash,
                th.gap,
            );
        }

        // Value axis in the gutter, outside the clip.
        p.clip(Rect::from_min_max(
            Pos2::new(rect.right(), rect.top()),
            Pos2::new(rect.right() + th.axis_w + 8.0, rect.bottom()),
        ));
        for t in &ticks {
            p.text(
                Pos2::new(axis_x, cx.y_of(t.value)),
                Align2::LEFT_CENTER,
                &t.label,
                font.clone(),
                th.axis_text,
            );
        }
        for g in &pane.guides {
            if g.label {
                self.badge(
                    p,
                    axis_x,
                    cx.y_of(g.value),
                    &format!("{:.2}", g.value),
                    g.color,
                );
            }
        }
        // Last-value badges, in each series' own colour. The single most
        // "finished terminal" detail in the reference, and the cheapest.
        for s in &pane.series {
            if !s.badge || !s.visible {
                continue;
            }
            let last = to.saturating_sub(1);
            if let Some(v) = s.value_at(last) {
                let step = ticks.first().map_or(0.01, |t| {
                    ticks.get(1).map_or(0.01, |t2| (t2.value - t.value).abs())
                });
                self.badge(
                    p,
                    axis_x,
                    cx.y_of(v),
                    &scale::format_value(v, step),
                    s.color,
                );
            }
        }

        // Legend row, inside the pane, in series colours.
        p.clip(rect);
        let mut x = rect.left() + 6.0;
        let y = rect.top() + 3.0;
        // The title takes its own line when the pane also prints OHLC rows:
        // both start at the pane's left edge, so sharing a line put them on top
        // of each other.
        let has_ohlc = pane
            .series
            .iter()
            .any(|s| s.visible && matches!(s.plot, Plot::Candles(_) | Plot::Bars(_)));
        let mut y = y;
        if !pane.title.is_empty() {
            let r = p.text(
                Pos2::new(x, y),
                Align2::LEFT_TOP,
                &pane.title,
                font.clone(),
                th.legend_text,
            );
            if has_ohlc {
                y = r.bottom() + 1.0;
            } else {
                x = r.right() + 8.0;
            }
        }
        let read_at = hovered.or(st.selected).unwrap_or(to.saturating_sub(1));
        // A candle series reads out as a full OHLC row — that IS the price
        // pane's top line in the reference, and it is coloured by direction so
        // the day's sign is legible without reading the number. Several candle
        // series stack onto their own rows (a compared instrument), which is
        // why this advances `y` rather than sharing one line.
        let mut row = y;
        for s in &pane.series {
            if !s.visible || !matches!(s.plot, Plot::Candles(_) | Plot::Bars(_)) {
                continue;
            }
            let (Plot::Candles(d) | Plot::Bars(d)) = &s.plot else {
                continue;
            };
            let Some(bar) = d.bar(read_at.min(d.len().saturating_sub(1))) else {
                continue;
            };
            let prev = read_at.checked_sub(1).and_then(|i| d.bar(i)).map(|b| b.c);
            let col = match prev {
                Some(pc) if bar.c < pc => s.down,
                Some(_) => s.up,
                None => th.legend_text,
            };
            let text = format!("{}  {}", s.name, series::ohlc_label(&bar, prev));
            let r = p.text(
                Pos2::new(rect.left() + 6.0, row),
                Align2::LEFT_TOP,
                &text,
                font.clone(),
                col,
            );
            row = r.bottom() + 1.0;
        }
        if row > y {
            // Overlays go under the OHLC rows rather than beside them.
            x = rect.left() + 6.0;
        }
        for s in &pane.series {
            if !s.visible || matches!(s.plot, Plot::Candles(_) | Plot::Bars(_)) {
                continue;
            }
            let label = match s.value_at(read_at) {
                Some(v) => format!("{} {:.4}", s.name, v),
                None => s.name.clone(),
            };
            let r = p.text(
                Pos2::new(x, row.max(y)),
                Align2::LEFT_TOP,
                &label,
                font.clone(),
                s.color,
            );
            x = r.right() + 10.0;
        }
    }

    /// A filled label pinned to the value axis.
    fn badge(&self, p: &mut EguiPainter<'_>, x: f32, y: f32, text: &str, color: Color32) {
        let font = FontId::proportional(self.theme.font);
        // Measured by drawing it transparent first: the box has to fit the
        // label, and the label's width is font-dependent.
        let r = p.text(
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            text,
            font.clone(),
            Color32::TRANSPARENT,
        );
        p.rects(&[RectPrim::new(r.expand2(egui::vec2(3.0, 1.5)), color)]);
        p.text(
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            text,
            font,
            Color32::from_rgb(16, 18, 24),
        );
    }

    fn draw_time_axis(&self, p: &mut EguiPainter<'_>, plot: Rect, ticks: &[Tick], xs: &[f32]) {
        let th = &self.theme;
        let axis = Rect::from_min_max(
            Pos2::new(plot.left(), plot.bottom()),
            Pos2::new(plot.right() + th.axis_w, plot.bottom() + th.time_h),
        );
        p.clip(axis);
        let font = FontId::proportional(th.font);
        // Positions come from the caller, which already mapped each timestamp
        // back to the column its bar occupies — the axis is indexed by bar, so
        // a division sits where its bar is, not where a continuous clock would
        // put it. That is what stops a weekend opening a gap.
        for (t, x) in ticks.iter().zip(xs) {
            p.text(
                Pos2::new(*x, axis.center().y),
                Align2::CENTER_CENTER,
                &t.label,
                font.clone(),
                th.axis_text,
            );
        }
    }
}

/// Which drawn column a bar index falls in.
fn column_of(bar: usize, from: usize, to: usize, cols: usize) -> usize {
    let span = to.saturating_sub(from).max(1);
    let k = (bar.saturating_sub(from)) * cols / span;
    k.min(cols.saturating_sub(1))
}

/// The visible bar nearest `t`, by binary search over the clock.
fn bar_of_time(d: &Series, t: i64, from: usize, to: usize) -> usize {
    let (mut lo, mut hi) = (from, to.saturating_sub(1));
    while lo < hi {
        let mid = (lo + hi) / 2;
        if d.time().at(mid) < t {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

#[cfg(test)]
mod view_tests {
    use super::*;

    fn st(first: usize, visible: usize) -> ChartState {
        ChartState {
            first,
            visible,
            follow: false,
            ..Default::default()
        }
    }

    /// Zoom must hold the bar under the cursor in place, or zooming turns into
    /// hunting for what you were looking at.
    #[test]
    fn zoom_holds_the_anchor() {
        let len = 10_000;
        for anchor in [0.0f32, 0.25, 0.5, 0.9, 1.0] {
            let mut s = st(4_000, 1_000);
            let before = s.first as f32 + anchor * s.visible as f32;
            s.zoom(0.5, anchor, len);
            let after = s.first as f32 + anchor * s.visible as f32;
            assert!(
                (before - after).abs() <= 2.0,
                "anchor {anchor}: bar {before} moved to {after}"
            );
        }
    }

    /// In then out returns to where you were, rather than drifting.
    #[test]
    fn zoom_is_reversible() {
        let len = 10_000;
        let mut s = st(4_000, 1_000);
        let (f0, v0) = (s.first, s.visible);
        s.zoom(0.85, 0.5, len);
        s.zoom(1.0 / 0.85, 0.5, len);
        assert!(
            (s.visible as i64 - v0 as i64).abs() <= 2,
            "{} vs {v0}",
            s.visible
        );
        assert!(
            (s.first as i64 - f0 as i64).abs() <= 2,
            "{} vs {f0}",
            s.first
        );
    }

    /// The window may never leave the data, however hard it is pushed.
    #[test]
    fn the_window_stays_inside_the_data() {
        let len = 500;
        let mut s = st(0, 100);
        s.pan(-100_000, len);
        assert!(s.first + s.visible <= len, "{s:?}");
        s.pan(100_000, len);
        assert_eq!(s.first, 0);
        // Zooming out past the data clamps to all of it.
        s.zoom(1000.0, 0.5, len);
        assert_eq!(s.visible, len);
        assert_eq!(s.first, 0);
        // And zooming in stops at a readable minimum instead of collapsing.
        s.zoom(0.0001, 0.5, len);
        assert_eq!(s.visible, MIN_VISIBLE);
    }

    /// A shorter series than the default window must still be valid.
    #[test]
    fn a_series_shorter_than_the_window_is_clamped() {
        let mut s = ChartState::default();
        s.clamp(3);
        assert_eq!(s.first, 0);
        assert!(s.visible >= MIN_VISIBLE);
    }

    /// Panning away from the right edge stops following; returning resumes it.
    #[test]
    fn follow_tracks_the_right_edge() {
        let len = 1_000;
        let mut s = ChartState {
            first: 0,
            visible: 100,
            follow: true,
            ..Default::default()
        };
        s.clamp(len);
        assert_eq!(s.first, len - 100, "following pins to the newest bar");
        // Positive pans back in time, matching a drag to the right.
        s.pan(200, len);
        assert!(!s.follow, "panning into history must stop following");
        assert_eq!(s.first, len - 100 - 200);
        s.pan(-10_000, len);
        assert!(s.follow, "panning back to the edge resumes it");
        assert_eq!(s.first, len - 100);
    }

    /// Every visible bar maps into a real column, and the ends map to the ends.
    #[test]
    fn bars_map_into_columns() {
        let (from, to, cols) = (100usize, 900usize, 260usize);
        assert_eq!(column_of(from, from, to, cols), 0);
        assert_eq!(column_of(to - 1, from, to, cols), cols - 1);
        for bar in (from..to).step_by(7) {
            let k = column_of(bar, from, to, cols);
            assert!(k < cols, "bar {bar} -> column {k} of {cols}");
        }
        // Degenerate spans must not divide by zero.
        assert_eq!(column_of(5, 5, 5, 10), 0);
    }
}
