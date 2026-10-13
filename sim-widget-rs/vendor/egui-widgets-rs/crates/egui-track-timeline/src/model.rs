//! Generic, app-agnostic timeline data model.
//!
//! The caller rebuilds a [`TimelineModel`] every frame from its own data and
//! hands it to [`crate::TrackTimeline::show`]. The widget only *reads* this model
//! — it never owns the clips. All edits are reported back as
//! [`crate::TimelineAction`]s for the caller to apply.
//!
//! Time is expressed in integer **frames** (i64) to keep the model decoupled
//! from any particular time/rational type. Clips carry an opaque [`u64`] `id`
//! (caller-chosen, e.g. a hash of a UUID) so the widget can refer to them in
//! actions without depending on the caller's key type.

use egui::Color32;
use egui_pattern::Pattern;

/// Opaque, caller-chosen clip identity.
///
/// `u64` is used (instead of a `uuid::Uuid`) to avoid pulling a uuid dependency
/// into this generic widget. Callers keyed by UUID can hash/truncate their key
/// into a `u64`, or maintain a small `u64 → real-key` side map.
pub type ClipId = u64;

/// Positional address of a single keyframe diamond, as the widget sees it.
///
/// `track`/`lane` index into [`TimelineModel::tracks`] and the track's
/// [`Track::lanes`]; `frame` is the diamond's model frame (the same value stored
/// in [`Keyframe::frame`]). This is **purely positional / view-space** — it
/// carries no domain identity (no UUID). Multi-key gestures (marquee-select,
/// block-move, delete) report a `Vec<KeyPos>`; the host translates each into its
/// own authoritative key identity. `Hash`/`Eq` let hosts collect them into sets.
///
/// `frame` is the key's EXACT time as the host gave it in [`Keyframe::frame`] - a whole frame or
/// a time between two - copied, never recomputed from a screen position, so the host can match it
/// against its own key times exactly. Equality and hashing compare its bits (`-0.0` and `0.0`
/// differ, as do NaNs; a host that stores `0.0` for a key at zero gets `0.0` back).
#[derive(Clone, Copy, Debug)]
pub struct KeyPos {
    /// Track index into [`TimelineModel::tracks`].
    pub track: usize,
    /// Lane index into the track's [`Track::lanes`].
    pub lane: usize,
    /// The keyframe's time in frames, exactly [`Keyframe::frame`].
    pub frame: f64,
}

impl PartialEq for KeyPos {
    fn eq(&self, other: &Self) -> bool {
        self.track == other.track
            && self.lane == other.lane
            && self.frame.to_bits() == other.frame.to_bits()
    }
}

impl Eq for KeyPos {}

impl std::hash::Hash for KeyPos {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.track.hash(state);
        self.lane.hash(state);
        self.frame.to_bits().hash(state);
    }
}

/// How a clip's visible bar is filled.
///
/// `Solid` is the default flat color fill. `Pattern` overlays a pattern on top of
/// the fill - e.g. `Pattern::hatch()` for a clip whose media is offline / proxy /
/// "missing source" (playa's file-source bars), the typical NLE convention. The
/// pattern is drawn as one textured quad however wide the clip is (`egui-pattern`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Fill {
    /// Flat solid color fill (default).
    #[default]
    Solid,
    /// Solid fill plus a pattern overlay.
    Pattern(Pattern),
}

/// A single clip/segment placed on a track.
///
/// # Geometry
/// A clip has a *full bar* (its entire source material) and a *visible bar*
/// (the portion actually played, after trimming). The full bar spans
/// `start .. start + duration`. The visible bar is the full bar minus
/// `trim_in` frames on the left and `trim_out` frames on the right:
///
/// ```text
///  start                                   start + duration
///   |───────────────── full bar ───────────────────|
///   |  trim_in |──── visible (played) ────| trim_out |
/// ```
///
/// `duration`, `trim_in` and `trim_out` are all in timeline frames. When both
/// trims are zero the visible bar equals the full bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// Opaque caller identity (used to address this clip in [`crate::TimelineAction`]).
    pub id: ClipId,
    /// First frame of the full bar on the timeline (may be negative).
    pub start: i64,
    /// Total length of the full source bar, in frames (`>= 1`).
    pub duration: i64,
    /// Frames trimmed off the left (head) of the full bar.
    pub trim_in: i64,
    /// Frames trimmed off the right (tail) of the full bar.
    pub trim_out: i64,
    /// Display label drawn on the bar.
    pub label: String,
    /// Optional explicit bar color. When `None`, the widget derives a stable
    /// color from a hash of [`Clip::label`].
    pub color: Option<Color32>,
    /// Visible-bar fill style (solid by default; hatch flags offline/missing source).
    pub fill: Fill,
    /// The property lane of its track the bar sits on (shown while the track is expanded, as a
    /// time-remapped layer's take); `None`: the track's own row. Either way it is one of the
    /// track's clips: selection, moves and actions address it by [`Clip::id`].
    pub lane: Option<usize>,
    /// Marked spans on the bar (a take it plays, cached frames): strips along its bottom edge.
    pub bands: Vec<Band>,
}

/// A marked span on a clip's bar, in timeline frames: a strip of `color` along the bar's bottom
/// edge, `label` (an icon and a name) beside its start when there is room. Clicking it reports
/// `id` ([`crate::TimelineAction::Select`]) instead of the clip's.
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    pub start: i64,
    pub duration: i64,
    pub color: Color32,
    pub label: String,
    pub id: Option<ClipId>,
}

impl Clip {
    /// Add a marked span ([`Band`]; builder-style).
    pub fn with_band(mut self, band: Band) -> Self {
        self.bands.push(band);
        self
    }

    /// Put the bar on property lane `lane` of its track ([`Clip::lane`]; builder-style).
    pub fn on_lane(mut self, lane: usize) -> Self {
        self.lane = Some(lane);
        self
    }

    /// Construct a clip with no trims and no explicit color.
    pub fn new(id: ClipId, start: i64, duration: i64, label: impl Into<String>) -> Self {
        Self {
            id,
            start,
            duration,
            trim_in: 0,
            trim_out: 0,
            label: label.into(),
            color: None,
            fill: Fill::Solid,
            lane: None,
            bands: Vec::new(),
        }
    }

    /// Set an explicit bar color (builder-style).
    pub fn with_color(mut self, color: Color32) -> Self {
        self.color = Some(color);
        self
    }

    /// Set the visible-bar fill style (builder-style).
    pub fn with_fill(mut self, fill: Fill) -> Self {
        self.fill = fill;
        self
    }

    /// Set trim-in / trim-out (builder-style).
    pub fn with_trims(mut self, trim_in: i64, trim_out: i64) -> Self {
        self.trim_in = trim_in;
        self.trim_out = trim_out;
        self
    }

    /// Exclusive end frame of the full bar (`start + duration`).
    #[inline]
    pub fn full_end(&self) -> i64 {
        self.start + self.duration
    }

    /// First played (visible) frame: `start + trim_in`.
    #[inline]
    pub fn visible_start(&self) -> i64 {
        self.start + self.trim_in
    }

    /// Exclusive end of the played (visible) region: `start + duration - trim_out`.
    #[inline]
    pub fn visible_end(&self) -> i64 {
        self.full_end() - self.trim_out
    }
}

/// A single keyframe on a property lane: a value at a time `frame`, in frames - a whole frame or
/// a time between two (a key a plug-in or an interpolation placed at `12.25`).
///
/// The widget only needs `frame` to place the diamond horizontally at its exact position on the
/// ruler (via [`crate::TimelineView::frame_to_x`]); `value` is carried through so the host
/// can drive an animated parameter from the same data it feeds the timeline.
/// `value` is `f64` (not `f32`) to match the host's animation storage and avoid
/// precision loss when round-tripping keyframe values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframe {
    /// Time at which the key sits, in frames (maps to an x via the shared time axis).
    pub frame: f64,
    /// The keyed value at `frame` (opaque to the widget; drawn by neither).
    pub value: f64,
    /// Selection flag — a **host-set view flag** (default `false`). The widget
    /// only *reads* it: selected diamonds are painted highlighted and are the
    /// keys a block-move / delete acts on. Selection ownership lives in the host
    /// (keyed by its own domain identity); the widget never mutates this.
    pub selected: bool,
}

impl Keyframe {
    /// Construct a keyframe at time `frame` holding `value` (unselected).
    pub fn new(frame: f64, value: f64) -> Self {
        Self {
            frame,
            value,
            selected: false,
        }
    }

    /// Set the selection view flag (builder-style). Host-only; the widget reads it.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

/// One After-Effects-style property lane shown *under* a track while the track
/// is [`Track::expanded`]. Each lane is one animatable property; its keyframes
/// are drawn as diamonds aligned 1:1 with the clip bars above (same time axis).
///
/// The lane's `name` is drawn by the **host outline**, not this widget — the
/// widget only paints the lane's background band and its keyframe diamonds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropLane {
    /// Property name (drawn by the host's outline column, not the widget).
    pub name: String,
    /// Keyframes on this lane, in arbitrary order (widget maps via `frame_to_x`).
    pub keys: Vec<Keyframe>,
    /// Optional explicit diamond color; when `None` the widget uses a theme accent.
    pub color: Option<Color32>,
}

impl PropLane {
    /// Construct a named lane from a keyframe list (no explicit color).
    pub fn new(name: impl Into<String>, keys: Vec<Keyframe>) -> Self {
        Self {
            name: name.into(),
            keys,
            color: None,
        }
    }

    /// Set an explicit diamond color (builder-style).
    pub fn with_color(mut self, color: Color32) -> Self {
        self.color = Some(color);
        self
    }
}

/// One horizontal track (row) holding clips. Tracks stack top-to-bottom in
/// [`TimelineModel::tracks`] order (index `0` = topmost row).
///
/// A track may carry a set of [`PropLane`]s; when [`Track::expanded`] is `true`
/// the widget draws those lanes as extra sub-rows below the clip row, growing
/// the track's [`Track::height`]. When collapsed (the default) a track behaves
/// exactly as before — a single `row_height`-tall row — so existing callers are
/// unaffected.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    /// Optional track name (drawn by the host's own outline panel, if any —
    /// this widget only renders the bars, not a name column).
    pub name: String,
    /// Clips on this track, in arbitrary order.
    pub clips: Vec<Clip>,
    /// Twirl state: when `true` the track's [`PropLane`]s are shown as sub-rows.
    /// Toggled by the host (which owns the twirl triangle in its outline).
    pub expanded: bool,
    /// Property lanes shown only while [`Track::expanded`]. Empty by default.
    pub lanes: Vec<PropLane>,
}

impl Track {
    /// Construct a named track from a clip list. The track starts collapsed with
    /// no lanes, preserving the pre-lanes behavior for existing callers.
    pub fn new(name: impl Into<String>, clips: Vec<Clip>) -> Self {
        Self {
            name: name.into(),
            clips,
            expanded: false,
            lanes: Vec::new(),
        }
    }

    /// Attach property lanes (builder-style). Does not change `expanded`.
    pub fn with_lanes(mut self, lanes: Vec<PropLane>) -> Self {
        self.lanes = lanes;
        self
    }

    /// Set the twirl (expanded) state (builder-style).
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// Total drawn height of this track in points: the clip `row_h` plus, when
    /// [`Track::expanded`], one `lane_h`-tall band per lane. Collapsed tracks are
    /// always exactly `row_h` tall. This is the SINGLE source of truth for track
    /// height — the widget and the host outline both call it so their rows line
    /// up 1:1.
    #[inline]
    pub fn height(&self, row_h: f32, lane_h: f32) -> f32 {
        row_h
            + if self.expanded {
                self.lanes.len() as f32 * lane_h
            } else {
                0.0
            }
    }
}

/// Optional work-area (play range) overlay: frames outside `[start, end)` are
/// dimmed, mirroring the playa timeline's work-area shading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkArea {
    /// First in-range frame (inclusive).
    pub start: i64,
    /// Exclusive end frame.
    pub end: i64,
}

/// The full, flat input the caller builds each frame.
///
/// The widget reads this to paint; it never stores it. View state (zoom / pan /
/// drag) lives separately in [`crate::TimelineView`], which the widget *does*
/// mutate in place.
#[derive(Clone, Debug, Default)]
pub struct TimelineModel {
    /// Stacked tracks, top to bottom.
    pub tracks: Vec<Track>,
    /// Current playhead position, in frames.
    pub playhead: i64,
    /// Frames per second — used only to format ruler tooltips / labels; the
    /// widget works purely in frame space otherwise.
    pub fps: f32,
    /// Currently selected clip ids (the widget paints these highlighted).
    pub selection: Vec<ClipId>,
    /// Optional dimmed work-area overlay.
    pub work_area: Option<WorkArea>,
    /// Ruler bookmark frames; each is drawn as a subtle marker glyph on the ruler.
    pub markers: Vec<i64>,
}

impl TimelineModel {
    /// Convenience constructor for a model with a given fps and track list.
    pub fn new(fps: f32, tracks: Vec<Track>) -> Self {
        Self {
            tracks,
            playhead: 0,
            fps,
            selection: Vec::new(),
            work_area: None,
            markers: Vec::new(),
        }
    }

    /// Smallest `start` and largest `full_end()` across all clips, used by
    /// "fit to view". Returns `None` when there are no clips.
    pub fn frame_bounds(&self) -> Option<(i64, i64)> {
        let mut bounds: Option<(i64, i64)> = None;
        for track in &self.tracks {
            for clip in &track.clips {
                let (lo, hi) = bounds.get_or_insert((clip.start, clip.full_end()));
                *lo = (*lo).min(clip.start);
                *hi = (*hi).max(clip.full_end());
            }
        }
        bounds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_defaults_to_solid_fill() {
        let c = Clip::new(1, 0, 10, "a");
        assert_eq!(c.fill, Fill::Solid);
    }

    #[test]
    fn with_fill_sets_a_pattern() {
        let c = Clip::new(1, 0, 10, "a").with_fill(Fill::Pattern(Pattern::hatch()));
        assert_eq!(c.fill, Fill::Pattern(Pattern::hatch()));
    }

    #[test]
    fn fill_default_is_solid() {
        assert_eq!(Fill::default(), Fill::Solid);
    }

    #[test]
    fn model_starts_with_no_markers() {
        let m = TimelineModel::new(24.0, vec![]);
        assert!(m.markers.is_empty());
    }

    #[test]
    fn track_new_is_collapsed_with_no_lanes() {
        let t = Track::new("V1", vec![]);
        assert!(!t.expanded);
        assert!(t.lanes.is_empty());
    }

    #[test]
    fn track_height_ignores_lanes_when_collapsed() {
        // Two lanes but collapsed => height is exactly one row.
        let t = Track::new("V1", vec![]).with_lanes(vec![
            PropLane::new("opacity", vec![]),
            PropLane::new("scale", vec![]),
        ]);
        assert_eq!(t.height(22.0, 16.0), 22.0);
    }

    #[test]
    fn track_height_adds_lane_bands_when_expanded() {
        // row_h + 2 * lane_h once expanded.
        let t = Track::new("V1", vec![])
            .with_lanes(vec![
                PropLane::new("opacity", vec![]),
                PropLane::new("scale", vec![]),
            ])
            .expanded(true);
        assert_eq!(t.height(22.0, 16.0), 22.0 + 2.0 * 16.0);
    }

    #[test]
    fn keyframe_new_is_unselected() {
        let k = Keyframe::new(5.0, 1.0);
        assert!(!k.selected);
    }

    #[test]
    fn keyframe_selected_builder_sets_flag() {
        let k = Keyframe::new(5.0, 1.0).selected(true);
        assert!(k.selected);
    }

    #[test]
    fn keypos_is_hashable_and_eq() {
        use std::collections::HashSet;
        let a = KeyPos {
            track: 1,
            lane: 0,
            frame: 10.25,
        };
        let b = KeyPos {
            track: 1,
            lane: 0,
            frame: 10.25,
        };
        let c = KeyPos {
            track: 1,
            lane: 0,
            frame: 10.5,
        };
        assert_eq!(a, b);
        assert_ne!(a, c, "two keys within one frame are two keys");
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
        assert!(!set.contains(&c));
    }

    #[test]
    fn expanded_track_with_no_lanes_is_one_row() {
        // Expanded but no lanes => still just the clip row.
        let t = Track::new("V1", vec![]).expanded(true);
        assert_eq!(t.height(22.0, 16.0), 22.0);
    }
}
