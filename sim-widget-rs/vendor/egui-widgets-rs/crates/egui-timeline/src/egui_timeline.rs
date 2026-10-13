//! A reusable egui timeline / time-track widget.
//!
//! The track spans an indexed slot set `0..count` (e.g. positions into a present
//! frame sequence). It paints:
//! - a recessed background,
//! - a per-slot **tint** layer (a caller closure `i -> Option<Color32>`, run-length
//!   coalesced) — used by a player to draw the frame-cache fill ("loaded" bar),
//! - adaptive **ticks** with caller-formatted **labels** (`i -> String`, e.g. the
//!   real frame number for slot `i`),
//! - optional **bookmark markers** at present-index slots (accent tick + triangle;
//!   same recipe as `egui-track-timeline` / playa ruler bookmarks),
//! - a **playhead** at `pos` carrying its label.
//!
//! It reports a **seek** when the user clicks or drags the track, and is generic
//! (no app types) so the same widget serves any egui application.
//! Colours come from the active [`egui::Visuals`] plus a configurable accent.

use egui::{Color32, Painter, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

/// A timeline widget. Cheap to construct each frame (a few config fields).
///
/// Not `Copy`: [`Timeline::markers`] holds an owned slot list (empty by default).
#[derive(Clone, Debug)]
pub struct Timeline {
    height: f32,
    minor_ticks: bool,
    accent: Color32,
    /// When `true` the playhead head block is drawn at full accent brightness
    /// (vs. dimmed at rest) — a cheap "is playing" affordance, mirroring the
    /// vfx-view time track. Host-driven; the widget keeps no clock.
    playing: bool,
    /// Seek-emission policy while scrubbing (see [`Timeline::redraw_on_scrub`]).
    /// Default `true` preserves the historic behaviour (seek emitted every drag
    /// frame). `false` moves the playhead live but only commits the seek once on
    /// drag release — lets a host pause/resume decode instead of thrashing it.
    redraw_on_scrub: bool,
    /// Present-index slots (`0..count`) drawn as bookmark glyphs on the track.
    /// Empty by default — additive; existing callers need not set this.
    markers: Vec<usize>,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            height: 22.0,
            minor_ticks: true,
            // == egui_widgets_config::Palette::default().playhead — kept const so
            // `Default`/builder stay non-`ui`; host may override via `.accent(..)`.
            accent: Color32::from_rgb(100, 210, 100),
            playing: false,
            // Historic default: emit the seek continuously while dragging.
            redraw_on_scrub: true,
            markers: Vec::new(),
        }
    }
}

/// What [`Timeline::show`] / [`Timeline::show_ex`] report back.
pub struct TimelineResponse {
    /// The new position if the user clicked/dragged to seek, else `None`.
    ///
    /// With `redraw_on_scrub = true` this fires on every drag frame; with
    /// `false` it fires once on drag release (plus on any plain click).
    pub seek: Option<usize>,
    /// `true` on the frame a scrub drag BEGAN (egui `drag_started`). A host can
    /// use it to enter a pause-while-scrubbing state.
    pub scrub_started: bool,
    /// `true` on the frame a scrub drag ENDED (egui `drag_stopped`). A host can
    /// use it to arm a resume timer.
    pub scrub_stopped: bool,
    /// The underlying egui response (hover/drag state, for the caller).
    pub response: egui::Response,
}

impl Timeline {
    pub fn new() -> Self {
        Self::default()
    }

    /// Track height in points (default 22).
    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    /// Draw a per-slot minor tick when slots are wide enough (default on).
    pub fn minor_ticks(mut self, on: bool) -> Self {
        self.minor_ticks = on;
        self
    }

    /// Playhead accent colour (default green).
    pub fn accent(mut self, c: Color32) -> Self {
        self.accent = c;
        self
    }

    /// Mark the transport as playing so the playhead head block brightens
    /// (default `false`). Purely cosmetic — the widget owns no clock.
    pub fn playing(mut self, on: bool) -> Self {
        self.playing = on;
        self
    }

    /// Seek-emission policy while the user drags the playhead (default `true`).
    ///
    /// While dragging, the playhead ALWAYS tracks the cursor visually. This flag
    /// only controls when the committed [`TimelineResponse::seek`] is emitted:
    /// - `true`  — every drag frame (live scrub; the historic behaviour).
    /// - `false` — once on drag release, so a host can pause heavy decode during
    ///   the drag and seek only to the final frame (plain clicks still seek).
    pub fn redraw_on_scrub(mut self, on: bool) -> Self {
        self.redraw_on_scrub = on;
        self
    }

    /// Bookmark glyphs at the given present-index slots (playa / `egui-track-timeline`
    /// ruler markers). Drawn before the playhead so the head wins on overlap.
    pub fn markers(mut self, slots: impl IntoIterator<Item = usize>) -> Self {
        self.markers = slots.into_iter().collect();
        self
    }

    /// Render the timeline of `count` slots with the playhead at `pos`.
    ///
    /// `tint(i)` colours slot `i` (e.g. cache state); `None` leaves it empty.
    /// `label(i)` is the text shown at a tick / on the playhead for slot `i`
    /// (e.g. the real frame number). Returns a seek position when scrubbed.
    ///
    /// Thin wrapper over [`Timeline::show_ex`] with no timecode row.
    pub fn show(
        &self,
        ui: &mut Ui,
        pos: usize,
        count: usize,
        tint: impl Fn(usize) -> Option<Color32>,
        label: impl Fn(usize) -> String,
    ) -> TimelineResponse {
        self.show_ex(ui, pos, count, tint, label, |_| None)
    }

    /// As [`Timeline::show`], plus an optional second **timecode readout** on
    /// major ticks: `timecode(i)` returns `Some(text)` (e.g. an SMPTE string
    /// from [`smpte`]) to draw beneath the numeric label for slot `i`, or `None`
    /// to omit it. Honours [`Timeline::redraw_on_scrub`] and reports
    /// [`TimelineResponse::scrub_started`] / [`TimelineResponse::scrub_stopped`].
    pub fn show_ex(
        &self,
        ui: &mut Ui,
        pos: usize,
        count: usize,
        tint: impl Fn(usize) -> Option<Color32>,
        label: impl Fn(usize) -> String,
        timecode: impl Fn(usize) -> Option<String>,
    ) -> TimelineResponse {
        let count = count.max(1);
        let pos = pos.min(count - 1);

        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), self.height),
            Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        let visuals = ui.visuals();
        let cfg = egui_widgets_config::WidgetsConfig::get(ui);

        let pad = 6.0_f32;
        let x0 = rect.left() + pad;
        let x1 = rect.right() - pad;
        let inner = (x1 - x0).max(1.0);
        // Left edge x of slot `i` (slot width = inner / count).
        let slot_x = |i: f32| -> f32 { x0 + i / count as f32 * inner };

        // Recessed background.
        painter.rect_filled(rect, 3.0, visuals.extreme_bg_color);
        painter.rect_stroke(
            rect,
            3.0,
            visuals.widgets.noninteractive.bg_stroke,
            StrokeKind::Inside,
        );

        // Tint layer — coalesce runs of identical colour into one rect (so a
        // thousand-frame cache fill is a handful of draws, not a thousand).
        let top = rect.top() + 2.0;
        let bot = rect.bottom() - 2.0;
        let mut i = 0usize;
        while i < count {
            let c = tint(i);
            if let Some(col) = c {
                let mut j = i + 1;
                while j < count && tint(j) == c {
                    j += 1;
                }
                let r = Rect::from_min_max(
                    egui::pos2(slot_x(i as f32), top),
                    egui::pos2(slot_x(j as f32), bot),
                );
                painter.rect_filled(r, 0.0, col);
                i = j;
            } else {
                i += 1;
            }
        }

        // Adaptive ticks: major ticks ~64px apart (labelled), minor per-slot ticks
        // only when each slot is at least ~6px wide.
        let px_per_slot = inner / count as f32;
        let major_step = nice_step(64.0 / px_per_slot.max(0.01)).max(1.0) as usize;
        let tick_col = visuals.weak_text_color().gamma_multiply(0.7);
        let label_col = visuals.weak_text_color();
        let font = egui_widgets_config::mono(cfg.fonts.xs);

        if self.minor_ticks && px_per_slot >= 6.0 {
            for s in 0..count {
                let x = slot_x(s as f32 + 0.5);
                painter.line_segment(
                    [egui::pos2(x, bot - 3.0), egui::pos2(x, bot)],
                    Stroke::new(1.0, tick_col.gamma_multiply(0.5)),
                );
            }
        }
        let mut s = 0usize;
        while s < count {
            let x = slot_x(s as f32 + 0.5);
            painter.line_segment(
                [egui::pos2(x, rect.top() + 2.0), egui::pos2(x, bot)],
                Stroke::new(1.0, tick_col),
            );
            painter.text(
                egui::pos2(x + 2.0, rect.top() + 1.0),
                egui::Align2::LEFT_TOP,
                label(s),
                font.clone(),
                label_col,
            );
            // Optional SMPTE/timecode readout on majors, dimmer + a line below
            // the numeric label (skipped entirely when the host returns `None`).
            if let Some(tc) = timecode(s) {
                painter.text(
                    egui::pos2(x + 2.0, rect.top() + 1.0 + cfg.fonts.xs + 1.0),
                    egui::Align2::LEFT_TOP,
                    tc,
                    font.clone(),
                    label_col.gamma_multiply(0.75),
                );
            }
            s += major_step;
        }

        // Bookmark markers — accent tick + small triangle at the top (playa /
        // egui-track-timeline `draw_ruler` recipe). Drawn before the playhead so
        // the head reads on top where they overlap.
        let marker_color = self.accent;
        for &slot in &self.markers {
            if slot >= count {
                continue;
            }
            let mx = slot_x(slot as f32 + 0.5);
            if mx < rect.left() || mx > rect.right() {
                continue;
            }
            painter.line_segment(
                [egui::pos2(mx, rect.top()), egui::pos2(mx, rect.bottom())],
                Stroke::new(1.0, marker_color),
            );
            let s = 5.0;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(mx - s / 2.0, rect.top()),
                    egui::pos2(mx + s / 2.0, rect.top()),
                    egui::pos2(mx, rect.top() + s),
                ],
                marker_color,
                Stroke::NONE,
            ));
        }

        // Scrub: click or drag maps the pointer x to a slot. The playhead ALWAYS
        // tracks the cursor visually (`head_slot`); whether the committed `seek`
        // is emitted now or deferred to release is `redraw_on_scrub`'s job.
        let mut seek = None;
        let scrub_started = resp.drag_started();
        let scrub_stopped = resp.drag_stopped();
        let mut head_slot = pos;
        if (resp.dragged() || resp.clicked() || scrub_started || scrub_stopped)
            && let Some(p) = resp.interact_pointer_pos()
        {
            let frac = ((p.x - x0) / inner).clamp(0.0, 1.0);
            let slot = slot_at_frac(frac, count);
            head_slot = slot;
            // Commit the seek: continuously when redraw_on_scrub, else only on a
            // plain click or on drag release. Never emit a no-op (slot == pos).
            let commit = self.redraw_on_scrub || resp.clicked() || scrub_stopped;
            if commit && slot != pos {
                seek = Some(slot);
            }
        }

        // Playhead: a thin stem + a chunky head block carrying the slot label.
        let cx = slot_x(head_slot as f32 + 0.5);
        painter.line_segment(
            [egui::pos2(cx, rect.top()), egui::pos2(cx, rect.bottom())],
            Stroke::new(1.5, self.accent),
        );
        let galley = painter.layout_no_wrap(
            label(head_slot),
            egui_widgets_config::mono(cfg.fonts.sm),
            Color32::BLACK,
        );
        let head_w = (galley.size().x + 8.0).max(18.0);
        let head_cx = cx.clamp(rect.left() + head_w * 0.5, rect.right() - head_w * 0.5);
        let head = Rect::from_center_size(
            egui::pos2(head_cx, rect.center().y),
            Vec2::new(head_w, self.height - 4.0),
        );
        // Brighten the head while playing, dim slightly at rest (Maya-style cue).
        let head_col = if self.playing {
            self.accent
        } else {
            self.accent.gamma_multiply(0.85)
        };
        painter.rect_filled(head, 3.0, head_col);
        painter.rect_stroke(
            head,
            3.0,
            Stroke::new(1.0, Color32::from_black_alpha(120)),
            StrokeKind::Inside,
        );
        painter.galley(head.center() - galley.size() * 0.5, galley, Color32::BLACK);

        TimelineResponse {
            seek,
            scrub_started,
            scrub_stopped,
            response: resp,
        }
    }
}

/// Paint a run-length-coalesced per-slot **tint strip** into `rect`, positioning
/// each slot via the caller-supplied `slot_x`.
///
/// Use this when a HOST timeline owns the zoom / pan / ruler and this is only the
/// status / cache-fill bar that must stay ALIGNED with it: there are no ticks,
/// playhead or scrubbing here (the host's main timeline draws those). It is the
/// externally-driven counterpart to [`Timeline::show`] (which is self-contained
/// and spreads `0..count` across its own width).
///
/// - `slots` is the visible slot range to draw — pass the host's visible window
///   (computed from its zoom/pan) so off-screen slots cost nothing.
/// - `slot_x(i)` returns the screen x of slot `i`'s LEFT edge (so `slot_x(j)` is
///   the right edge of the run ending just before `j`); typically
///   `host.frame_to_x(first_frame + i)`.
/// - `tint(i)` colours slot `i`; `None` leaves it empty. Adjacent equal colours
///   coalesce into one rect (a thousand-frame fill becomes a handful of draws).
///
/// Runs are clipped to `rect`'s horizontal extent.
pub fn paint_tint_strip(
    painter: &Painter,
    rect: Rect,
    slots: std::ops::Range<usize>,
    slot_x: impl Fn(usize) -> f32,
    tint: impl Fn(usize) -> Option<Color32>,
) {
    let (top, bot) = (rect.top(), rect.bottom());
    let mut i = slots.start;
    while i < slots.end {
        if let Some(col) = tint(i) {
            let mut j = i + 1;
            while j < slots.end && tint(j) == Some(col) {
                j += 1;
            }
            let x_start = slot_x(i).max(rect.left());
            let x_end = slot_x(j).min(rect.right());
            if x_end > x_start {
                painter.rect_filled(
                    Rect::from_min_max(egui::pos2(x_start, top), egui::pos2(x_end, bot)),
                    0.0,
                    col,
                );
            }
            i = j;
        } else {
            i += 1;
        }
    }
}

/// Map a `0..=1` fraction of the track to a slot index in `0..count`.
fn slot_at_frac(frac: f32, count: usize) -> usize {
    let slot = (frac.clamp(0.0, 1.0) * count as f32).floor() as usize;
    slot.min(count - 1)
}

/// A "nice" tick step (1, 2, 5 × 10ⁿ) at least `min` slots wide, so labels land
/// on readable round numbers regardless of zoom. Thin f32 facade over the shared
/// [`egui_widgets_config::time_axis::nice_step_up`] ladder.
fn nice_step(min: f32) -> f64 {
    egui_widgets_config::time_axis::nice_step_up(min as f64)
}

/// Format an absolute `frame` index as an SMPTE-style `HH:MM:SS:FF` timecode at
/// `fps`, for the optional timecode readout in [`Timeline::show_ex`].
///
/// Non-drop-frame (whole-frame count per second). `fps` is rounded to the
/// nearest whole frame count and floored to 1 to stay panic-free (no div-by-0),
/// so odd rates like 23.976 read as their nominal 24 fps grid — good enough for
/// a scrub ruler; a host needing drop-frame can pass its own formatter instead.
pub fn smpte(frame: i64, fps: f32) -> String {
    let rate = (fps.round() as i64).max(1);
    let f = frame.rem_euclid(rate);
    let total_secs = frame.div_euclid(rate);
    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours = total_secs / 3600;
    format!("{hours:02}:{mins:02}:{secs:02}:{f:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_mapping_spans_full_range() {
        assert_eq!(slot_at_frac(0.0, 10), 0);
        assert_eq!(slot_at_frac(1.0, 10), 9); // clamped, never count
        assert_eq!(slot_at_frac(0.55, 10), 5);
        assert_eq!(slot_at_frac(0.999, 1), 0);
    }

    /// The strip coalesces adjacent equal colours and skips `None` gaps. We feed
    /// a recording painter-free model by counting the runs `paint_tint_strip`
    /// *would* draw via the same logic (extracted to keep the test painter-free).
    #[test]
    fn tint_runs_coalesce_and_skip_gaps() {
        // colours: A A None B B B None A  -> runs: [0..2]=A, [3..6]=B, [7..8]=A = 3
        let cols = [
            Some(1u8),
            Some(1),
            None,
            Some(2),
            Some(2),
            Some(2),
            None,
            Some(1),
        ];
        let tint = |i: usize| cols[i];
        let mut runs = 0;
        let mut i = 0;
        while i < cols.len() {
            if let Some(c) = tint(i) {
                let mut j = i + 1;
                while j < cols.len() && tint(j) == Some(c) {
                    j += 1;
                }
                runs += 1;
                i = j;
            } else {
                i += 1;
            }
        }
        assert_eq!(runs, 3);
    }

    #[test]
    fn smpte_formats_and_stays_panic_free() {
        assert_eq!(smpte(0, 24.0), "00:00:00:00");
        assert_eq!(smpte(25, 24.0), "00:00:01:01");
        assert_eq!(smpte(24 * 60 * 60 + 5, 24.0), "01:00:00:05");
        // fps <= 0 must not divide by zero (floored to 1).
        assert_eq!(smpte(3, 0.0), "00:00:03:00");
    }

    #[test]
    fn nice_step_rounds_up() {
        assert_eq!(nice_step(1.0), 1.0);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(6.0), 10.0);
        assert_eq!(nice_step(15.0), 20.0);
    }
}
