//! Pure painting + geometry helpers (no input handling).
//!
//! These mirror playa's `timeline_helpers` / draw passes but operate on the
//! generic model. Kept side-effect free (apart from `Painter` calls) so the
//! widget's interaction logic in `widget.rs` stays readable.

use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2, epaint::StrokeKind};
use egui_widgets_config::{Semantic, WidgetsConfig, mono, prop};

use crate::model::{Band, Clip, Fill, PropLane, Track};

/// Height of a [`Band`] strip along a bar's bottom edge, and how far above it its click zone
/// reaches (a 3-pixel strip alone is hard to hit).
pub const BAND_HEIGHT: f32 = 3.0;
const BAND_PICK: f32 = 4.0;

/// Where `band` sits on `clip`'s bar `geom`: frames map linearly across the full bar (no view
/// lookup), clipped to the bar. None when none of it is on the bar.
pub fn band_rect(clip: &Clip, geom: &ClipGeom, band: &Band) -> Option<Rect> {
    let full = geom.full;
    if !full.is_positive() || clip.duration <= 0 {
        return None;
    }
    let per_frame = full.width() / clip.duration as f32;
    let x0 = full.left() + (band.start - clip.start) as f32 * per_frame;
    let x1 = x0 + band.duration as f32 * per_frame;
    let rect = Rect::from_min_max(
        Pos2::new(x0.max(full.left()), full.bottom() - BAND_HEIGHT),
        Pos2::new(x1.min(full.right()), full.bottom()),
    );
    (rect.width() > 0.0).then_some(rect)
}

/// The id of the band of `clip` whose click zone holds `pos` (the strip and [`BAND_PICK`] above).
pub fn hit_band(clip: &Clip, geom: &ClipGeom, pos: Pos2) -> Option<crate::model::ClipId> {
    clip.bands.iter().find_map(|band| {
        let rect = band_rect(clip, geom, band)?;
        let zone = Rect::from_min_max(rect.min - Vec2::new(0.0, BAND_PICK), rect.max);
        zone.contains(pos).then_some(band.id).flatten()
    })
}
use crate::view::{TimelineConfig, TimelineView};

/// Precomputed clip geometry, shared between the draw pass and the interaction
/// pass to avoid recomputing rects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipGeom {
    /// Full source bar.
    pub full: Rect,
    /// Visible (played) bar, `None` when fully trimmed (`visible_start >= visible_end`).
    pub visible: Option<Rect>,
}

impl ClipGeom {
    /// Compute bar rects for a clip on a given row.
    /// No bar: a lane clip of a collapsed track. Nothing hits or paints it.
    pub const NONE: Self = Self {
        full: Rect::NOTHING,
        visible: None,
    };

    /// The bar of `clip` on its `track` (whose top is `track_top`): the track's own row, or for
    /// a lane clip ([`Clip::lane`]) that property lane while the track is expanded
    /// ([`Self::NONE`] otherwise). The one placement of every bar, ghost and trim preview.
    pub fn place(
        clip: &Clip,
        track: &Track,
        track_top: f32,
        area_min_x: f32,
        config: &TimelineConfig,
        view: &TimelineView,
    ) -> Self {
        match clip.lane {
            None => Self::calc(clip, track_top, config.row_height, area_min_x, config, view),
            Some(lane) if track.expanded && lane < track.lanes.len() => Self::calc(
                clip,
                config.lane_center_y(track_top, lane) - config.lane_height * 0.5,
                config.lane_height,
                area_min_x,
                config,
                view,
            ),
            Some(_) => Self::NONE,
        }
    }

    /// The bar of `clip` on a row of height `row_h` whose top is `row_y`.
    fn calc(
        clip: &Clip,
        row_y: f32,
        row_h: f32,
        area_min_x: f32,
        config: &TimelineConfig,
        view: &TimelineView,
    ) -> Self {
        let to_x = |frame: i64| view.frame_to_x(frame as f32, area_min_x, config);
        let top = row_y + 4.0;
        let bottom = row_y + row_h - 4.0;

        let full = Rect::from_min_max(
            Pos2::new(to_x(clip.start), top),
            Pos2::new(to_x(clip.full_end()), bottom),
        );

        let visible = if clip.visible_start() < clip.visible_end() {
            Some(Rect::from_min_max(
                Pos2::new(to_x(clip.visible_start()), top),
                Pos2::new(to_x(clip.visible_end()), bottom),
            ))
        } else {
            None
        };

        Self { full, visible }
    }
}

/// Which edit tool a hover position maps to over a clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Drag the head edge (trim-in).
    TrimStart,
    /// Drag the tail edge (trim-out).
    TrimEnd,
    /// Move the whole clip.
    Move,
    /// Slide content within the trim zones (between full and visible bars).
    Slide,
}

impl Tool {
    /// Cursor to display for this tool.
    pub fn cursor(self) -> egui::CursorIcon {
        match self {
            Tool::TrimStart | Tool::TrimEnd => egui::CursorIcon::ResizeHorizontal,
            Tool::Move => egui::CursorIcon::Grab,
            Tool::Slide => egui::CursorIcon::ResizeColumn,
        }
    }
}

/// Tool detection on the *visible* bar only (edges → trim, middle → move).
fn detect_on_visible(pos: Pos2, bar: Rect, threshold: f32) -> Option<Tool> {
    if !bar.expand(threshold).contains(pos) {
        return None;
    }
    let dist_left = (pos.x - bar.min.x).abs();
    let dist_right = (pos.x - bar.max.x).abs();
    if dist_left < threshold {
        Some(Tool::TrimStart)
    } else if dist_right < threshold {
        Some(Tool::TrimEnd)
    } else if bar.contains(pos) {
        Some(Tool::Move)
    } else {
        None
    }
}

/// Full geometry-aware tool detection: trim zones (between the full bar and the
/// visible bar) map to [`Tool::Slide`]; everything else falls back to the
/// visible-bar handles. Mirrors playa's `detect_layer_tool_with_geom`.
pub fn detect_tool(pos: Pos2, geom: &ClipGeom, threshold: f32) -> Option<Tool> {
    if !geom.full.expand(threshold).contains(pos) {
        return None;
    }

    let Some(visible) = geom.visible else {
        // Fully trimmed: only Move on the full bar.
        return geom.full.contains(pos).then_some(Tool::Move);
    };

    let in_y = pos.y >= geom.full.min.y && pos.y <= geom.full.max.y;
    let in_left_trim = in_y && pos.x >= geom.full.min.x && pos.x < visible.min.x - threshold;
    let in_right_trim = in_y && pos.x > visible.max.x + threshold && pos.x <= geom.full.max.x;

    if in_left_trim || in_right_trim {
        return Some(Tool::Slide);
    }

    detect_on_visible(pos, visible, threshold)
}

/// Stable HSV color derived from a label hash (when a clip has no explicit color).
pub fn hash_color(s: &str) -> Color32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    let hue = (hasher.finish() % 360) as f32;
    hsv_to_rgb(hue, 0.65, 0.55)
}

/// HSV → RGB (h in degrees, s/v in `0..=1`).
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color32 {
    let c = v * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Color32::from_rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// Smallest integer 1/2/5 × 10ⁿ step whose screen spacing meets `min_px`.
///
/// Ruler spacing is a lower bound: rounding it down is what made labels overlap at
/// low zoom. The shared upward ladder keeps the grid stable on round frame values.
fn ruler_step(min_px: f32, ppf: f32) -> i64 {
    egui_widgets_config::time_axis::nice_step_up(f64::from(min_px / ppf)) as i64
}

/// Greatest grid multiple at or before `frame`.
fn grid_floor(frame: i64, step: i64) -> i64 {
    let remainder = frame.rem_euclid(step);
    frame.checked_sub(remainder).unwrap_or(frame)
}

/// Draw the adaptive frame ruler (ticks + numeric labels + playhead tick) into
/// `rect`. Pure paint; scrubbing input is handled by the caller.
#[allow(clippy::too_many_arguments)]
pub fn draw_ruler(
    painter: &Painter,
    rect: Rect,
    playhead: i64,
    markers: &[i64],
    config: &TimelineConfig,
    view: &TimelineView,
    tok: &WidgetsConfig,
    sem: &Semantic,
) {
    painter.rect_filled(rect, 0.0, sem.surface);

    let ppf = view.ppf(config);
    if !ppf.is_finite() || ppf <= 0.0 {
        return;
    }

    let Some((first, last)) = view.visible_frames(rect, painter.clip_rect(), config) else {
        return;
    };
    let visible_start = first.floor() as i64;
    let visible_end = last.ceil() as i64;
    let label_font = mono(tok.fonts.xs);

    // The widest visible endpoint has the greatest digit count for this monospace
    // ruler. Measure both because a negative sign also consumes one glyph.
    let widest_label = [visible_start, visible_end]
        .into_iter()
        .map(|frame| {
            painter
                .layout_no_wrap(format!("{frame}"), label_font.clone(), sem.weak)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max);

    // Labels and minor ticks have separate grids. A 2-step minor grid and a
    // 5-step label grid are both valid nice values, but iterating only the former
    // would never visit odd label multiples such as 5.
    let label_step = ruler_step(widest_label + tok.metrics.space_lg, ppf);
    let tick_step = ruler_step(15.0, ppf).min(label_step);

    let mut frame = grid_floor(visible_start, tick_step);
    while frame <= visible_end {
        let x = view.frame_to_x(frame as f32, rect.min.x, config);
        if x >= rect.min.x && x <= rect.max.x {
            painter.line_segment(
                [Pos2::new(x, rect.max.y - 5.0), Pos2::new(x, rect.max.y)],
                (1.0, Color32::from_gray(100)),
            );
        }
        let Some(next) = frame.checked_add(tick_step) else {
            break;
        };
        frame = next;
    }

    let mut frame = grid_floor(visible_start, label_step);
    while frame <= visible_end {
        let x = view.frame_to_x(frame as f32, rect.min.x, config);
        if x >= rect.min.x && x <= rect.max.x {
            painter.line_segment(
                [Pos2::new(x, rect.max.y - 8.0), Pos2::new(x, rect.max.y)],
                (1.0, Color32::from_gray(100)),
            );
            painter.text(
                Pos2::new(x, rect.min.y + 2.0),
                egui::Align2::CENTER_TOP,
                format!("{frame}"),
                label_font.clone(),
                sem.weak,
            );
        }
        let Some(next) = frame.checked_add(label_step) else {
            break;
        };
        frame = next;
    }

    // Bookmark markers — subtle accent tick + small triangle at the ruler top.
    // Drawn before the playhead so the playhead reads on top where they overlap.
    let marker_color = sem.accent;
    for &m in markers {
        let mx = view.frame_to_x(m as f32, rect.min.x, config);
        if mx < rect.min.x || mx > rect.max.x {
            continue;
        }
        painter.line_segment(
            [Pos2::new(mx, rect.min.y), Pos2::new(mx, rect.max.y)],
            (1.0, marker_color),
        );
        let s = 5.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(mx - s / 2.0, rect.min.y),
                Pos2::new(mx + s / 2.0, rect.min.y),
                Pos2::new(mx, rect.min.y + s),
            ],
            marker_color,
            Stroke::NONE,
        ));
    }

    // Playhead tick on the ruler.
    let px = view.frame_to_x(playhead as f32, rect.min.x, config);
    if px >= rect.min.x && px <= rect.max.x {
        painter.line_segment(
            [Pos2::new(px, rect.min.y), Pos2::new(px, rect.max.y)],
            (2.0, sem.playhead),
        );
    }
}

/// Draw a single clip bar (full + visible + label + selection/hover outline).
pub fn draw_clip(
    painter: &Painter,
    clip: &Clip,
    geom: &ClipGeom,
    selected: bool,
    hovered: bool,
    tok: &WidgetsConfig,
    sem: &Semantic,
) {
    // Full bar — dimmed backing showing the untrimmed extent.
    let backing = if selected {
        let s = sem.selection;
        Color32::from_rgba_unmultiplied(s.r(), s.g(), s.b(), 130)
    } else {
        // Neutral semi-transparent dim for the untrimmed extent — reads as a
        // recessed backing on any theme; intentionally not a solid theme colour.
        Color32::from_rgba_unmultiplied(80, 80, 80, 100)
    };
    painter.rect_filled(geom.full, 4.0, backing);

    // Visible (played) bar — full color + label.
    if let Some(visible) = geom.visible {
        let base = clip.color.unwrap_or_else(|| hash_color(&clip.label));
        painter.rect_filled(visible, 4.0, base);

        // Pattern overlay (e.g. the "missing/offline source" hatch). Drawn over the
        // solid fill (so the bar keeps its base hue) but under the label; pinned to
        // the bar's own corner so it moves with the clip rather than crawling.
        if let Fill::Pattern(pattern) = &clip.fill {
            egui_pattern::fill_pattern(
                painter,
                visible,
                pattern,
                Color32::from_rgba_unmultiplied(0, 0, 0, 90),
                visible.min,
            );
        }

        let galley = painter.layout_no_wrap(clip.label.clone(), prop(tok.fonts.sm), Color32::WHITE);
        let mut label = Rect::NOTHING;
        if visible.width() > galley.size().x + 8.0 {
            label = Rect::from_center_size(visible.center(), galley.size());
            painter.galley(label.min, galley, Color32::WHITE);
        }
        // Bands: their strips, and a label beside a band's start where it clears the bar's.
        let bar = painter.with_clip_rect(painter.clip_rect().intersect(visible));
        for band in &clip.bands {
            let Some(strip) = band_rect(clip, geom, band) else {
                continue;
            };
            bar.rect_filled(strip, 0.0, band.color);
            if band.label.is_empty() {
                continue;
            }
            let text = painter.layout_no_wrap(band.label.clone(), prop(tok.fonts.sm), band.color);
            let at = Rect::from_min_size(
                Pos2::new(
                    strip.left() + 2.0,
                    (strip.top() - text.size().y).max(visible.top()),
                ),
                text.size(),
            );
            if at.right() <= visible.right() && !at.intersects(label) {
                bar.galley(at.min, text, band.color);
            }
        }
    }

    // Outline.
    let (stroke_color, stroke_w) = if selected {
        (sem.selection, tok.metrics.stroke_thick)
    } else if hovered {
        (sem.hover, tok.metrics.stroke_thick)
    } else {
        (sem.weak, tok.metrics.stroke_thin)
    };
    painter.rect_stroke(
        geom.full,
        4.0,
        Stroke::new(stroke_w, stroke_color),
        StrokeKind::Middle,
    );
}

/// Draw a ghost outline (drag preview) at the given rect.
pub fn draw_ghost(painter: &Painter, rect: Rect, color: Color32) {
    painter.rect_stroke(rect, 4.0, Stroke::new(2.0_f32, color), StrokeKind::Middle);
}

/// Draw alternating row backgrounds for the given `tracks` (per-track heights,
/// so expanded tracks get a taller band). Theme-aware: even rows
/// use the panel fill, odd rows a subtly lifted variant (softer than the old fixed
/// dark greys, and correct in light themes).
pub fn draw_row_backgrounds(
    painter: &Painter,
    area: Rect,
    tracks: &[Track],
    config: &TimelineConfig,
    sem: &Semantic,
) {
    let even = sem.panel;
    // Lift odd rows into a subtle stripe. gamma_multiply(>1) lightens, (<1) darkens;
    // pick the direction from the panel's own luminance so the stripe stays visible
    // in BOTH dark and light themes (a light panel would wash out if only lightened).
    let is_light = even.r() as u16 + even.g() as u16 + even.b() as u16 > 384;
    let odd = even.gamma_multiply(if is_light { 0.94 } else { 1.12 });
    // Cumulative tops so each stripe covers a track's FULL height (clip row plus,
    // when expanded, its lane bands) — matching the widget's variable-height rows.
    let mut y = area.min.y;
    for (row, track) in tracks.iter().enumerate() {
        let h = track.height(config.row_height, config.lane_height);
        let row_rect = Rect::from_min_size(Pos2::new(area.min.x, y), Vec2::new(area.width(), h));
        let bg = if row % 2 == 0 { even } else { odd };
        painter.rect_filled(row_rect, 0.0, bg);
        y += h;
    }
}

/// Paint a keyframe diamond (a 45°-rotated square) centered at `center`, with a
/// radius of `r` points. Shared by [`draw_lane`] and the move-key drag ghost so
/// there is exactly one diamond shape definition. (Copied in spirit from
/// `egui-keyframes::paint_diamond`; re-implemented here to avoid a dependency.)
///
/// When `selected` is set the diamond is painted with a bright highlight fill and
/// a white outline (AE-style selected key), overriding `fill`/`stroke`; the host
/// drives selection via [`crate::Keyframe::selected`].
pub fn draw_key_diamond(
    painter: &Painter,
    center: Pos2,
    r: f32,
    fill: Color32,
    stroke: Stroke,
    selected: bool,
) {
    let (fill, stroke) = if selected {
        (
            Color32::from_rgb(255, 226, 120),
            Stroke::new(1.5, Color32::WHITE),
        )
    } else {
        (fill, stroke)
    };
    let pts = vec![
        Pos2::new(center.x, center.y - r),
        Pos2::new(center.x + r, center.y),
        Pos2::new(center.x, center.y + r),
        Pos2::new(center.x - r, center.y),
    ];
    painter.add(egui::Shape::convex_polygon(pts, fill, stroke));
}

/// Translucent rubber-band selection rectangle, painted each frame while a
/// [`crate::DragState::Marquee`] gesture is active.
pub fn draw_marquee(painter: &Painter, rect: Rect) {
    painter.rect_filled(
        rect,
        0.0,
        Color32::from_rgba_unmultiplied(120, 180, 255, 40),
    );
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(150, 200, 255, 200)),
        StrokeKind::Middle,
    );
}

/// Paint one expanded property lane: a subtle background band, a faint baseline,
/// and a keyframe diamond at each key. `lane_rect` is the lane's row band (full
/// track width, `lane_height` tall). Diamonds are placed horizontally via the
/// shared time axis (`view.frame_to_x`) so they line up 1:1 with the clip bars
/// and playhead above; they are centered vertically in the band.
/// `frames`: the frames on screen ([`TimelineView::visible_frames`]); keys outside them (and a
/// diamond's radius) are not drawn.
pub fn draw_lane(
    painter: &Painter,
    lane_rect: Rect,
    lane: &PropLane,
    frames: (f32, f32),
    config: &TimelineConfig,
    view: &TimelineView,
    sem: &Semantic,
) {
    // Subtle inset band, slightly darker/lighter than the row stripe so the lane
    // reads as a nested sub-row in both themes.
    let base = sem.panel;
    let is_light = base.r() as u16 + base.g() as u16 + base.b() as u16 > 384;
    let band = base.gamma_multiply(if is_light { 0.90 } else { 1.22 });
    painter.rect_filled(lane_rect, 0.0, band);

    // Faint baseline through the lane's vertical center.
    let cy = lane_rect.center().y;
    painter.line_segment(
        [
            Pos2::new(lane_rect.min.x, cy),
            Pos2::new(lane_rect.max.x, cy),
        ],
        Stroke::new(1.0, base.gamma_multiply(if is_light { 0.7 } else { 1.5 })),
    );

    // Diamonds. Radius scales with lane height but is capped for tidy rows.
    let r = config.key_radius();
    let fill = lane.color.unwrap_or(sem.accent);
    let stroke = Stroke::new(1.0, sem.panel);
    let margin = r / view.ppf(config);
    let (first, last) = (f64::from(frames.0 - margin), f64::from(frames.1 + margin));
    for key in lane
        .keys
        .iter()
        .filter(|key| key.frame >= first && key.frame <= last)
    {
        let x = view.frame_to_x(key.frame as f32, lane_rect.min.x, config);
        draw_key_diamond(painter, Pos2::new(x, cy), r, fill, stroke, key.selected);
    }
}

/// Dim the regions outside `[start, end)` of a work area across the track area.
pub fn draw_work_area(
    painter: &Painter,
    area: Rect,
    start: i64,
    end: i64,
    config: &TimelineConfig,
    view: &TimelineView,
) {
    let shade = Color32::from_rgba_unmultiplied(0, 0, 0, 100);
    let start_x = view.frame_to_x(start as f32, area.min.x, config);
    let end_x = view.frame_to_x(end as f32, area.min.x, config);

    if start_x > area.min.x {
        painter.rect_filled(
            Rect::from_min_max(area.min, Pos2::new(start_x, area.max.y)),
            0.0,
            shade,
        );
    }
    if end_x < area.max.x {
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(end_x, area.min.y), area.max),
            0.0,
            shade,
        );
    }
}

/// Half-width of a work-area handle's grab zone, in points.
///
/// Wider than the triangle is drawn: the handles sit ON the ruler where the scrub gesture also
/// lives, so a too-tight zone means the drag silently becomes a Seek and the playhead jumps.
pub const WORK_HANDLE_GRAB: f32 = 7.0;

/// Which work-area handle, if any, is under `x`.
///
/// `Some(false)` = the start handle, `Some(true)` = the end handle. When the two are within a
/// grab zone of each other the END wins - a collapsed range must stay draggable OPEN, and the
/// start handle cannot do that.
///
/// Pure so the hit rule can be gated without a pointer: this decides whether a ruler drag edits
/// the work area or scrubs the playhead, and getting it wrong is a jumping playhead, not a
/// visual nit.
pub fn work_handle_at(x: f32, start_x: f32, end_x: f32) -> Option<bool> {
    let end_hit = (x - end_x).abs() <= WORK_HANDLE_GRAB;
    let start_hit = (x - start_x).abs() <= WORK_HANDLE_GRAB;
    match (start_hit, end_hit) {
        (_, true) => Some(true),
        (true, false) => Some(false),
        (false, false) => None,
    }
}

/// Draw the two work-area handles on the ruler: a right-pointing wedge at the start, a
/// left-pointing one at the (exclusive) end, both hugging the ruler's bottom edge.
pub fn draw_work_handles(painter: &Painter, ruler: Rect, start_x: f32, end_x: f32, sem: &Semantic) {
    let h = 9.0_f32.min(ruler.height());
    let y1 = ruler.max.y;
    let y0 = y1 - h;
    let color = sem.playhead.gamma_multiply(0.85);

    // Wedges point INWARD, so the pair reads as brackets around the range rather than as two
    // arrows pointing the same way.
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(start_x, y0),
            Pos2::new(start_x + h, y0),
            Pos2::new(start_x, y1),
        ],
        color,
        Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(end_x, y0),
            Pos2::new(end_x - h, y0),
            Pos2::new(end_x, y1),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Minimum drawn width of the playhead band, in points.
///
/// Zoomed out, one frame is a fraction of a point; the band then degrades to a constant-width bar
/// anchored at the frame's START rather than vanishing. Zoomed in it is exactly one frame.
const PLAYHEAD_MIN_W: f32 = 2.0;

/// Alpha of the band fill, drawn under the leading edge's full-strength line.
const PLAYHEAD_BAND_ALPHA: u8 = 64;

/// Width of the playhead band: one frame, or [`PLAYHEAD_MIN_W`] when a frame is thinner.
///
/// Pure so it can be gated without a painter harness — the failure it guards against is the band
/// collapsing to nothing at low zoom, which is invisible in code review and obvious on screen.
pub fn playhead_band_width(x: f32, next_x: f32) -> f32 {
    (next_x - x).max(PLAYHEAD_MIN_W)
}

/// Draw the playhead + top triangle from the ruler top down through `bottom_y`.
///
/// **One FRAME wide, not a hairline.** A line on a frame boundary cannot say WHICH frame is
/// current - it reads as "between these two", and at high zoom that ambiguity is the whole
/// question being asked. The band spans `playhead ..= playhead + 1`: the extent of the frame
/// actually on screen.
///
/// The **leading edge** - where the frame begins - stays full strength and carries the exact
/// position, so the band is the extent and the edge is the reading. The grab triangle sits on
/// that edge rather than the band's centre, so the mark does not shift as zoom changes the
/// band's width.
pub fn draw_playhead(
    painter: &Painter,
    ruler: Rect,
    bottom_y: f32,
    playhead: i64,
    view: &TimelineView,
    config: &TimelineConfig,
    sem: &Semantic,
) {
    let x = view.frame_to_x(playhead as f32, ruler.min.x, config);
    let next = view.frame_to_x((playhead + 1) as f32, ruler.min.x, config);
    let color = sem.playhead;

    let w = playhead_band_width(x, next);
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(x, ruler.min.y), Pos2::new(x + w, bottom_y)),
        0.0,
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), PLAYHEAD_BAND_ALPHA),
    );

    painter.line_segment(
        [Pos2::new(x, ruler.min.y), Pos2::new(x, bottom_y)],
        (1.0, color),
    );
    let s = 8.0;
    let top = ruler.min.y;
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(x, top),
            Pos2::new(x - s / 2.0, top - s),
            Pos2::new(x + s / 2.0, top - s),
        ],
        color,
        Stroke::NONE,
    ));
}

#[cfg(test)]
mod tests {
    use crate::model::{Clip, PropLane, Track};

    #[test]
    fn a_band_spans_its_frames_along_the_bars_bottom_and_picks_its_id() {
        let config = TimelineConfig::default();
        let view = TimelineView::default();
        let clip = Clip::new(1, 0, 100, "node").with_band(crate::model::Band {
            start: 25,
            duration: 50,
            color: egui::Color32::RED,
            label: String::new(),
            id: Some(9),
        });
        let track = Track::new("node", vec![clip.clone()]);
        let geom = super::ClipGeom::place(&clip, &track, 0.0, 0.0, &config, &view);
        let strip = super::band_rect(&clip, &geom, &clip.bands[0]).unwrap();
        let quarter = geom.full.left() + geom.full.width() * 0.25;
        assert!(
            (strip.left() - quarter).abs() < 1e-3
                && (strip.width() * 2.0 - geom.full.width()).abs() < 1e-3
        );
        assert_eq!(strip.bottom(), geom.full.bottom());
        assert_eq!(super::hit_band(&clip, &geom, strip.center()), Some(9));
        assert_eq!(
            super::hit_band(
                &clip,
                &geom,
                geom.full.left_bottom() + egui::vec2(1.0, -1.0)
            ),
            None
        );
    }

    #[test]
    fn a_lane_clip_sits_on_its_lane_and_only_while_its_track_is_expanded() {
        let config = TimelineConfig::default();
        let view = TimelineView::default();
        let lane = Clip::new(7, 10, 20, "take").on_lane(1);
        let mut track = Track::new("node", vec![Clip::new(1, 0, 100, "node"), lane.clone()])
            .with_lanes(vec![PropLane::new("a", vec![]), PropLane::new("b", vec![])]);
        assert_eq!(
            super::ClipGeom::place(&lane, &track, 0.0, 0.0, &config, &view),
            super::ClipGeom::NONE
        );
        track = track.expanded(true);
        let geom = super::ClipGeom::place(&lane, &track, 0.0, 0.0, &config, &view);
        let y = config.lane_center_y(0.0, 1);
        assert!(
            geom.full.top() < y && y < geom.full.bottom(),
            "{geom:?} around {y}"
        );
        let own = super::ClipGeom::place(&track.clips[0], &track, 0.0, 0.0, &config, &view);
        assert!(
            own.full.bottom() <= config.row_height,
            "the track's own clip keeps its row"
        );
    }
    use super::*;

    /// Every shape `paint` emits under a painter clipped to `clip`, with the default tokens.
    fn shapes(
        clip: Rect,
        paint: impl FnOnce(&Painter, &WidgetsConfig, &Semantic),
    ) -> Vec<egui::Shape> {
        let ctx = egui::Context::default();
        // A screen wide enough to hold the clip: a painter's clip rect is always within it.
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(100_000.0, 1_000.0),
            )),
            ..Default::default()
        });
        let painter = ctx
            .layer_painter(egui::LayerId::background())
            .with_clip_rect(clip);
        let tok = WidgetsConfig::default();
        let sem = Semantic::resolve(&egui::Visuals::dark(), &tok.palette);
        paint(&painter, &tok, &sem);
        let mut out = ctx.end_pass();
        // No GPU here: the font atlas upload is dropped explicitly (epaint asserts otherwise).
        out.textures_delta.clear();
        out.shapes
            .into_iter()
            .map(|clipped| clipped.shape)
            .collect()
    }

    /// An area 60 000 points wide (a long comp zoomed in, inside the host's scroll area) with 300
    /// points on screen.
    fn wide() -> (Rect, Rect) {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(60_000.0, 20.0));
        let clip = Rect::from_min_max(Pos2::new(30_000.0, 0.0), Pos2::new(30_300.0, 400.0));
        (rect, clip)
    }

    /// The ruler draws the ticks and labels of the frames ON SCREEN only: a handful for 300 points,
    /// not one per step of the whole 60 000-point area (the unculled ruler made thousands).
    #[test]
    fn the_ruler_draws_only_the_frames_on_screen() {
        let (rect, clip) = wide();
        let config = TimelineConfig::default();
        let view = TimelineView::default();
        let drawn = shapes(clip, |painter, tok, sem| {
            draw_ruler(painter, rect, 0, &[], &config, &view, tok, sem)
        });
        let ticks = drawn
            .iter()
            .filter(|shape| matches!(shape, egui::Shape::LineSegment { .. }))
            .count();
        let labels = drawn
            .iter()
            .filter(|shape| matches!(shape, egui::Shape::Text(_)))
            .count();
        // Minor ticks are at least 15 points apart: at most 300 / 15 + 1 on screen, plus the
        // labelled ones and the playhead line.
        assert!(
            ticks > 0 && ticks <= 2 * (300 / 15 + 2),
            "{ticks} ticks for 300 points"
        );
        assert!(
            labels > 0 && labels <= 300 / 15 + 2,
            "{labels} labels for 300 points"
        );
        for shape in &drawn {
            if let egui::Shape::Text(text) = shape {
                let x = text.pos.x;
                assert!(
                    (clip.min.x - 100.0..=clip.max.x + 100.0).contains(&x),
                    "a label at x {x}, off screen"
                );
            }
        }
    }

    /// A lane of 30 000 keys (one per frame at 2 points a frame) draws the ~150 on screen.
    #[test]
    fn a_lane_draws_only_the_keys_on_screen() {
        let (rect, clip) = wide();
        let config = TimelineConfig::default();
        let view = TimelineView::default();
        let keys = (0..30_000)
            .map(|frame| crate::model::Keyframe::new(f64::from(frame), 0.0))
            .collect();
        let lane = PropLane::new("x", keys);
        let lane_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), config.lane_height));
        let frames = view
            .visible_frames(lane_rect, clip, &config)
            .expect("on screen");
        let drawn = shapes(clip, |painter, _, sem| {
            draw_lane(painter, lane_rect, &lane, frames, &config, &view, sem)
        });
        let diamonds = drawn
            .iter()
            .filter(|shape| matches!(shape, egui::Shape::Path(_)))
            .count();
        let on_screen = 300 / 2;
        assert!(
            diamonds >= on_screen && diamonds <= on_screen + 8,
            "{diamonds} diamonds for {on_screen} frames on screen"
        );
    }

    #[test]
    fn ruler_step_never_rounds_below_the_required_screen_spacing() {
        let ppf = 0.135;
        let min_px = 40.0;
        let step = ruler_step(min_px, ppf);

        assert_eq!(step, 500);
        assert!(step as f32 * ppf >= min_px);
    }

    #[test]
    fn a_work_handle_claims_the_ruler_only_within_its_grab_zone() {
        // Clear of both: the drag must fall through to the playhead scrub.
        assert_eq!(work_handle_at(500.0, 100.0, 300.0), None);

        assert_eq!(work_handle_at(100.0, 100.0, 300.0), Some(false));
        assert_eq!(work_handle_at(300.0, 100.0, 300.0), Some(true));

        // The zone is inclusive at its edge and closed just past it - a handle that is one point
        // narrower than it is drawn reads as "the triangle does not grab".
        assert_eq!(
            work_handle_at(100.0 + WORK_HANDLE_GRAB, 100.0, 300.0),
            Some(false)
        );
        assert_eq!(
            work_handle_at(100.0 + WORK_HANDLE_GRAB + 0.1, 100.0, 300.0),
            None
        );

        // COLLAPSED RANGE: both handles sit on the same x. The END must win, because dragging
        // the end is what re-opens the range - grabbing the start there can only push it further
        // into itself, and the work area would be unrecoverable without a keyboard.
        assert_eq!(work_handle_at(200.0, 200.0, 200.0), Some(true));
    }

    #[test]
    fn the_playhead_band_is_one_frame_wide_and_never_collapses() {
        // Zoomed IN: a frame is 20 pt, so the band is the frame.
        assert_eq!(playhead_band_width(100.0, 120.0), 20.0);

        // Zoomed OUT: a frame is a fraction of a point. The band must still be VISIBLE - this is
        // the reddening case for dropping the `.max()`, which would draw a zero-width rect and
        // leave only the hairline the stage exists to replace.
        assert_eq!(playhead_band_width(100.0, 100.05), PLAYHEAD_MIN_W);
        assert_eq!(playhead_band_width(100.0, 100.0), PLAYHEAD_MIN_W);

        // Exactly at the threshold, the frame's own width wins - no discontinuity.
        assert_eq!(
            playhead_band_width(100.0, 100.0 + PLAYHEAD_MIN_W),
            PLAYHEAD_MIN_W
        );

        assert!(
            playhead_band_width(0.0, -50.0) > 0.0,
            "a reversed or degenerate mapping must never produce a negative width"
        );
    }
}
