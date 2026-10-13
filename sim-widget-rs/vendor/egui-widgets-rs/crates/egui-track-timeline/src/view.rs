//! View configuration, persistent view state, and the in-widget drag state machine.
//!
//! [`TimelineConfig`] is static layout (row height, base zoom). [`TimelineView`]
//! is the per-instance mutable view state the widget owns and edits in place
//! (zoom, pan, selection-independent drag gesture). The host typically stores one
//! `TimelineView` per timeline panel and persists it via serde.

use serde::{Deserialize, Serialize};

use crate::model::{KeyPos, Track};

/// Static layout configuration. Cheap to clone; usually a constant per app.
#[derive(Clone, Copy, Debug)]
pub struct TimelineConfig {
    /// Height of one track row (the clip band), in points.
    pub row_height: f32,
    /// Height of one expanded property lane sub-row, in points. Used only for
    /// tracks with [`crate::Track::expanded`] set; the host outline must use the
    /// same value so its lanes line up with the widget's.
    pub lane_height: f32,
    /// Base pixels-per-frame at `zoom == 1.0`. Effective ppf is
    /// `pixels_per_frame * view.zoom`.
    pub pixels_per_frame: f32,
    /// Half-width (points) of the grab zone at each clip edge for trim handles
    /// (sourced from `TimelineMetrics::handle_grab`).
    pub edge_threshold: f32,
    /// Extra grab padding (points) added to a keyframe diamond's drawn radius;
    /// effective grab half-width = `drawn_radius + key_grab_pad`. Hit-slop so the
    /// tiny diamonds are comfortable to click. From `TimelineMetrics::key_grab_pad`.
    pub key_grab_pad: f32,
    /// Pick priority of a keyframe over a clip trim/slide handle in the thin
    /// fringe where their grab zones overlap (higher wins). AE-style the keyframe
    /// sits on top. From `TimelineMetrics::prio_key` / `prio_tool`.
    pub prio_key: i32,
    /// Companion to [`Self::prio_key`] — the clip trim/slide handle's priority.
    pub prio_tool: i32,
    /// Whether to draw the dimmed work-area overlay when the model supplies one.
    pub show_work_area: bool,
    /// Snap clip drags to nearby edges. `0.0` disables snapping entirely.
    ///
    /// The value is a distance in **POINTS, not frames**, and that is the whole reason it is not
    /// a `bool`: a frame-based threshold is unusable at both ends of the zoom range - a hair's
    /// width when zoomed out, and a magnet that swallows every drag when zoomed in. In points it
    /// feels the same at any zoom, which is what "snapping" means to a user.
    pub snap_threshold: f32,
    /// Keyframe gestures snap to WHOLE frames (default `true`): a key added with Ctrl+click lands
    /// on the frame nearest the cursor, and a key drag moves by a whole number of frames - so a
    /// key between frames (a [`crate::Keyframe::frame`] of `12.25`) keeps its fraction as it
    /// moves. `false` makes both exact: the key lands at the fractional time under the cursor
    /// and a drag moves by the exact distance. Keys are always DRAWN and hit-tested at their exact
    /// time either way ([`key_time`], [`key_delta`]).
    pub snap_keys_to_frames: bool,
}

impl Default for TimelineConfig {
    fn default() -> Self {
        Self {
            row_height: 22.0,
            lane_height: 16.0,
            pixels_per_frame: 2.0,
            edge_threshold: 8.0,
            key_grab_pad: 5.0,
            prio_key: 30,
            prio_tool: 25,
            show_work_area: true,
            snap_threshold: 0.0,
            snap_keys_to_frames: true,
        }
    }
}

impl TimelineConfig {
    /// The radius of a key diamond: the ONE value the lane painter, the drag preview and the key
    /// hit-test share (the hit zone is this plus [`Self::key_grab_pad`]).
    pub fn key_radius(&self) -> f32 {
        (self.lane_height * 0.28).min(5.0)
    }

    /// The y of the center of lane `lane` of a track whose top is `track_top` (lanes stack under
    /// the clip row).
    pub fn lane_center_y(&self, track_top: f32, lane: usize) -> f32 {
        track_top + self.row_height + lane as f32 * self.lane_height + self.lane_height / 2.0
    }
}

/// The key time a Ctrl+click at fractional frame `at` creates: the nearest whole frame when
/// `snap_to_frames` ([`TimelineConfig::snap_keys_to_frames`]), else `at` itself.
pub fn key_time(at: f64, snap_to_frames: bool) -> f64 {
    if snap_to_frames { at.round() } else { at }
}

/// The delta a key drag of `frames` (fractional frames under the pointer) commits: a whole number
/// of frames when `snap_to_frames`, so every dragged key keeps its place within its frame, else
/// the exact distance.
pub fn key_delta(frames: f64, snap_to_frames: bool) -> f64 {
    if snap_to_frames {
        frames.round()
    } else {
        frames
    }
}

/// Every frame a drag may snap to: each clip's start and end, the playhead, and both work-area
/// bounds.
///
/// **All of them, or none.** A snap that catches clip edges but not the playhead looks like it
/// works and then fails exactly when it matters, which is worse than no snapping at all - the
/// user stops trusting it but keeps expecting it.
/// `skip` is the `(track, clip)` being dragged, and passing it is NOT optional in practice:
/// during a drag the model still holds that clip's ORIGINAL position, so leaving its own edges
/// in the target list makes it snap straight back to where the drag started - the gesture would
/// feel stuck rather than magnetic.
pub fn snap_targets(model: &crate::TimelineModel, skip: Option<(usize, usize)>) -> Vec<i64> {
    let mut out = Vec::new();
    for (ti, track) in model.tracks.iter().enumerate() {
        for (ci, clip) in track.clips.iter().enumerate() {
            if skip == Some((ti, ci)) {
                continue;
            }
            out.push(clip.start);
            out.push(clip.start + clip.duration);
        }
    }
    out.push(model.playhead);
    out.extend_from_slice(&model.markers);
    if let Some(wa) = model.work_area {
        out.push(wa.start);
        out.push(wa.end);
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Pull `proposed` to the nearest target within `max_dist` frames, or leave it alone.
///
/// `max_dist < 0` (a disabled threshold) snaps nothing. Ties go to the SMALLER target so a drag
/// between two equidistant edges lands somewhere deterministic rather than flickering between
/// them as the pointer jitters.
pub fn snap_frame(proposed: i64, targets: &[i64], max_dist: i64) -> i64 {
    if max_dist < 0 {
        return proposed;
    }
    targets
        .iter()
        .copied()
        .filter(|t| (t - proposed).abs() <= max_dist)
        .min_by_key(|t| ((t - proposed).abs(), *t))
        .unwrap_or(proposed)
}

/// Snap a MOVING clip by whichever of its two edges lands closest to a target.
///
/// A clip that only snapped by its head would refuse to butt up against the clip on its right,
/// which is the commonest reason to want snapping at all. Ties prefer the head, so a clip
/// equidistant by both edges moves the way the user is already thinking about it.
pub fn snap_move(start: i64, duration: i64, targets: &[i64], max_dist: i64) -> i64 {
    let by_head = snap_frame(start, targets, max_dist);
    let by_tail = snap_frame(start + duration, targets, max_dist) - duration;
    let (dh, dt) = ((by_head - start).abs(), (by_tail - start).abs());
    match (dh, dt) {
        (0, 0) => start,
        (0, _) => by_tail,
        (_, 0) => by_head,
        _ if dh <= dt => by_head,
        _ => by_tail,
    }
}

/// Y (screen-space) of track `index`'s top edge: the prefix-sum of every earlier
/// track's [`Track::height`], added to the track area's top `area_top`.
///
/// This replaces the old uniform `area_top + index * row_height` so that tracks
/// with expanded property lanes (taller rows) stack correctly. `index` may equal
/// `tracks.len()` to get the content's bottom edge. The host outline calls this
/// with the same metrics to mirror the widget's row tops 1:1.
pub fn track_top(tracks: &[Track], index: usize, area_top: f32, row_h: f32, lane_h: f32) -> f32 {
    let mut y = area_top;
    for t in tracks.iter().take(index) {
        y += t.height(row_h, lane_h);
    }
    y
}

/// Inverse of [`track_top`]: the index of the track whose vertical band contains
/// `y`, walking the same cumulative heights. Returns `None` when `y` is above the
/// first track or below the last. This is the single y→track lookup used by the
/// move-drag hit math (replacing a division by `row_height`, which silently
/// picked the wrong track once rows could differ in height).
pub fn y_to_track(
    tracks: &[Track],
    y: f32,
    area_top: f32,
    row_h: f32,
    lane_h: f32,
) -> Option<usize> {
    if y < area_top {
        return None;
    }
    let mut top = area_top;
    for (i, t) in tracks.iter().enumerate() {
        let h = t.height(row_h, lane_h);
        if y < top + h {
            return Some(i);
        }
        top += h;
    }
    None
}

/// Min/max zoom multiplier — re-exported from the shared time-axis so every
/// timeline widget in the set clamps to one range.
pub use egui_widgets_config::time_axis::{ZOOM_MAX, ZOOM_MIN};

/// Persistent, widget-owned view state (mutated in place by the widget).
///
/// `drag` is transient (skipped by serde) — it only lives for the duration of a
/// mouse gesture.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineView {
    /// Zoom multiplier (`1.0` = default). Clamped to `[ZOOM_MIN, ZOOM_MAX]`.
    pub zoom: f32,
    /// Horizontal scroll offset, in frames. Frame `pan_offset` sits at the left
    /// edge of the track area.
    pub pan_offset: f32,
    /// Active drag gesture, if any. Not serialized.
    #[serde(skip)]
    pub drag: Option<DragState>,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_offset: 0.0,
            drag: None,
        }
    }
}

impl TimelineView {
    /// Snapshot of the shared [`egui_widgets_config::time_axis::TimeView`] built
    /// from this widget view's persisted zoom/pan. The ONE implementation of the
    /// frame↔x mapping lives there; these methods just forward to it (keeping the
    /// widget's `&TimelineConfig`-based signatures for its callers).
    #[inline]
    fn axis(&self) -> egui_widgets_config::time_axis::TimeView {
        egui_widgets_config::time_axis::TimeView::new(self.zoom, self.pan_offset)
    }

    /// Effective pixels-per-frame for the current zoom.
    #[inline]
    pub fn ppf(&self, config: &TimelineConfig) -> f32 {
        self.axis().ppf(config.pixels_per_frame)
    }

    /// Map a frame to a screen x, given the track area's left edge.
    #[inline]
    pub fn frame_to_x(&self, frame: f32, area_min_x: f32, config: &TimelineConfig) -> f32 {
        self.axis()
            .frame_to_x(frame, area_min_x, config.pixels_per_frame)
    }

    /// Map a screen x back to a (fractional) frame.
    #[inline]
    pub fn x_to_frame(&self, x: f32, area_min_x: f32, config: &TimelineConfig) -> f32 {
        self.axis()
            .x_to_frame(x, area_min_x, config.pixels_per_frame)
    }

    /// The frames on screen: `[first, last]` (fractional) of the part of `rect` (an area whose
    /// left edge is frame-mapped, the ruler or the track area) inside `clip` (the painter's clip
    /// rect); `None` when none of it is.
    ///
    /// **The one visible-range query every paint loop culls with.** The widget's areas are as
    /// wide as the whole zoomed timeline inside the host's scroll area; a loop over `rect` instead
    /// drew every tick, label, clip and key of the WHOLE timeline - 14 000 lines and 7 000 labels
    /// a frame on a 7000-frame comp zoomed in, 99 % off screen (Playa `timeline_panel_cost`).
    pub fn visible_frames(
        &self,
        rect: egui::Rect,
        clip: egui::Rect,
        config: &TimelineConfig,
    ) -> Option<(f32, f32)> {
        let (x0, x1) = (rect.min.x.max(clip.min.x), rect.max.x.min(clip.max.x));
        (x0 < x1).then(|| {
            (
                self.x_to_frame(x0, rect.min.x, config),
                self.x_to_frame(x1, rect.min.x, config),
            )
        })
    }

    /// Convert a horizontal pixel delta to a rounded frame delta.
    #[inline]
    pub fn px_to_frames(&self, delta_px: f32, config: &TimelineConfig) -> i64 {
        self.axis().px_to_frames(delta_px, config.pixels_per_frame)
    }

    /// Apply a multiplicative zoom factor, clamped to the legal range.
    pub fn apply_zoom(&mut self, factor: f32) {
        let mut axis = self.axis();
        axis.apply_zoom(factor);
        self.zoom = axis.zoom;
    }
}

/// The in-widget drag state machine. One variant is active at a time while a
/// gesture is in flight. Mirrors playa's `GlobalDragState` timeline variants,
/// retargeted from layer indices/UUIDs to `(track, clip)` indices into the
/// generic [`crate::TimelineModel`].
#[derive(Clone, Copy, Debug)]
pub enum DragState {
    /// Horizontal pan (middle-mouse). Records the grab point and the pan offset
    /// at grab time so the delta can be applied absolutely each frame.
    Pan {
        start_x: f32,
        initial_pan_offset: f32,
    },
    /// Dragging one of the two work-area handles on the ruler.
    ///
    /// The handle is read from the live cursor each frame rather than accumulated from a delta,
    /// so the range cannot drift away from the pointer over a long drag. `moving_end` says which
    /// triangle was grabbed; the opposite edge is held from `other`, captured at grab time so a
    /// model that changes mid-drag cannot drag the other end along with it.
    WorkArea { moving_end: bool, other: i64 },
    /// Moving a clip in time and/or across tracks. The destination track is
    /// resolved from the live cursor Y via [`y_to_track`] each frame (not from a
    /// stored start-Y), so it stays correct across variable-height rows.
    Move {
        track: usize,
        clip: usize,
        initial_start: i64,
        start_x: f32,
    },
    /// Trimming the left (head) edge.
    TrimStart {
        track: usize,
        clip: usize,
        start_x: f32,
    },
    /// Trimming the right (tail) edge.
    TrimEnd {
        track: usize,
        clip: usize,
        start_x: f32,
    },
    /// Sliding content within fixed visible bounds.
    Slide {
        track: usize,
        clip: usize,
        start_x: f32,
    },
    /// Block-moving every currently-selected keyframe diamond horizontally in
    /// time. `anchor` is the KeyPos of the diamond the drag began on (used
    /// for the live target guide); `start_x` is the grab X, so the frame delta is
    /// [`key_delta`]`((cur.x - start_x) / ppf)` (whole frames by default). On release the
    /// widget emits a single [`crate::TimelineAction::MoveKeys`] over all `selected` keys.
    MoveKeys { anchor: KeyPos, start_x: f32 },
    /// Rubber-band (marquee) selection started on empty lane space. `start` is the
    /// grab point; the live rect is `start`→cursor. `add` mirrors the Shift
    /// modifier: `false` replaces the selection, `true` extends it.
    Marquee { start: egui::Pos2, add: bool },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frames on screen are the clip's x range mapped through the view: at the default 2
    /// points a frame, points 100..400 of an area starting at 0 are frames 50..200; pan shifts them;
    /// an area wholly off screen has none.
    #[test]
    fn visible_frames_map_the_clip_through_the_view() {
        let config = TimelineConfig::default();
        let mut view = TimelineView::default();
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(10_000.0, 50.0));
        let clip = egui::Rect::from_min_max(egui::pos2(100.0, 0.0), egui::pos2(400.0, 50.0));
        let ppf = view.ppf(&config);
        assert_eq!(
            view.visible_frames(area, clip, &config),
            Some((100.0 / ppf, 400.0 / ppf))
        );
        view.pan_offset = 1000.0;
        assert_eq!(
            view.visible_frames(area, clip, &config),
            Some((1000.0 + 100.0 / ppf, 1000.0 + 400.0 / ppf))
        );
        // Clamped to the area: a clip wider than the area stops at its edges.
        let small = egui::Rect::from_min_max(egui::pos2(200.0, 0.0), egui::pos2(300.0, 50.0));
        assert_eq!(
            view.visible_frames(small, clip, &config),
            Some((1000.0, 1000.0 + 100.0 / ppf))
        );
        let away = egui::Rect::from_min_max(egui::pos2(500.0, 0.0), egui::pos2(900.0, 50.0));
        assert_eq!(view.visible_frames(away, clip, &config), None);
    }
    use crate::model::{PropLane, Track};

    #[test]
    fn snapping_is_off_at_a_negative_radius_and_ties_go_to_the_smaller_target() {
        let targets = [10_i64, 20, 30];

        // -1 is how a disabled threshold reaches here. It must snap NOTHING, including an exact
        // hit - otherwise "off" still quietly moves the drag.
        assert_eq!(snap_frame(10, &targets, -1), 10);
        assert_eq!(snap_frame(11, &targets, -1), 11);

        assert_eq!(snap_frame(12, &targets, 3), 10);
        assert_eq!(snap_frame(12, &targets, 1), 12, "out of range: left alone");

        // Equidistant between 10 and 20. Ties must be DETERMINISTIC or the clip flickers between
        // two edges while the pointer sits still.
        assert_eq!(snap_frame(15, &targets, 5), 10);
    }

    /// Key gestures snap to whole frames by default - an added key lands on a frame, a drag moves
    /// by whole frames so a key between frames keeps its fraction - and are exact when off.
    #[test]
    fn key_gestures_snap_to_whole_frames_by_default_and_are_exact_when_off() {
        assert!(TimelineConfig::default().snap_keys_to_frames);
        assert_eq!(key_time(12.4, true), 12.0);
        assert_eq!(key_time(12.6, true), 13.0);
        assert_eq!(key_time(12.4, false), 12.4);
        assert_eq!(key_delta(2.7, true), 3.0);
        assert_eq!(
            12.25 + key_delta(2.7, true),
            15.25,
            "the fraction survives the move"
        );
        assert_eq!(key_delta(2.7, false), 2.7);
        assert_eq!(key_delta(-0.4, true), 0.0, "under half a frame: no move");
    }

    #[test]
    fn a_moving_clip_snaps_by_whichever_edge_is_closer() {
        let targets = [100_i64];

        // Tail lands on the target: the clip butts up against it from the left. A head-only
        // snap would refuse this, which is the commonest reason to want snapping at all.
        assert_eq!(snap_move(88, 10, &targets, 3), 90);

        // Head lands on it: butting up from the right.
        assert_eq!(snap_move(102, 10, &targets, 3), 100);

        // Neither edge is near: untouched.
        assert_eq!(snap_move(40, 10, &targets, 3), 40);

        // Head 2 away, tail 3 away -> the head wins, the smaller move.
        assert_eq!(snap_move(98, 5, &targets, 4), 100);
    }

    #[test]
    fn the_dragged_clip_is_not_its_own_snap_target() {
        let mut model = crate::TimelineModel::new(24.0, vec![]);
        model.playhead = 7;
        model.tracks = vec![Track::new(
            "t",
            vec![
                crate::model::Clip::new(1, 50, 10, "a"),
                crate::model::Clip::new(2, 200, 10, "b"),
            ],
        )];

        // Without the skip, a drag would find the clip's OWN 50/60 in the list and be pulled
        // straight back to where it started - the gesture would feel stuck, not magnetic.
        let t = snap_targets(&model, Some((0, 0)));
        assert!(
            !t.contains(&50),
            "the dragged clip's head must not be a target"
        );
        assert!(!t.contains(&60), "nor its tail");
        assert!(t.contains(&200), "other clips still are");
        assert!(t.contains(&7), "and so is the playhead");

        model.markers = vec![42];
        let with_mark = snap_targets(&model, Some((0, 0)));
        assert!(with_mark.contains(&42), "bookmarks are snap targets");

        let all = snap_targets(&model, None);
        assert!(all.contains(&50), "with nothing dragged, every edge counts");
    }

    /// Build a track with `lanes` empty lanes and a given expanded state.
    fn track(expanded: bool, lanes: usize) -> Track {
        let ls = (0..lanes)
            .map(|i| PropLane::new(format!("p{i}"), vec![]))
            .collect();
        Track::new("t", vec![]).with_lanes(ls).expanded(expanded)
    }

    #[test]
    fn track_top_is_prefix_sum_of_mixed_heights() {
        // t0 collapsed(22), t1 expanded 2 lanes(22 + 2*16 = 54), t2 collapsed(22).
        let tracks = vec![track(false, 0), track(true, 2), track(false, 0)];
        let (row_h, lane_h, top) = (22.0, 16.0, 100.0);
        assert_eq!(track_top(&tracks, 0, top, row_h, lane_h), 100.0);
        assert_eq!(track_top(&tracks, 1, top, row_h, lane_h), 122.0);
        assert_eq!(track_top(&tracks, 2, top, row_h, lane_h), 122.0 + 54.0);
        // index == len() yields the content's bottom edge.
        assert_eq!(
            track_top(&tracks, 3, top, row_h, lane_h),
            122.0 + 54.0 + 22.0
        );
    }

    #[test]
    fn y_to_track_round_trips_each_track() {
        let tracks = vec![track(false, 0), track(true, 2), track(false, 0)];
        let (row_h, lane_h, top) = (22.0, 16.0, 100.0);
        for k in 0..tracks.len() {
            let ktop = track_top(&tracks, k, top, row_h, lane_h);
            let h = tracks[k].height(row_h, lane_h);
            // A y at the top edge and at mid-band both map back to track k.
            assert_eq!(y_to_track(&tracks, ktop, top, row_h, lane_h), Some(k));
            assert_eq!(
                y_to_track(&tracks, ktop + h / 2.0, top, row_h, lane_h),
                Some(k)
            );
        }
    }

    #[test]
    fn y_to_track_out_of_bounds_is_none() {
        let tracks = vec![track(false, 0), track(false, 0)];
        let (row_h, lane_h, top) = (22.0, 16.0, 100.0);
        assert_eq!(y_to_track(&tracks, top - 1.0, top, row_h, lane_h), None);
        let bottom = track_top(&tracks, tracks.len(), top, row_h, lane_h);
        assert_eq!(y_to_track(&tracks, bottom, top, row_h, lane_h), None);
    }
}
