//! Edit intents the widget reports back to the caller.
//!
//! The widget never mutates the model's clips directly. Instead, each user
//! gesture that *would* change a clip is reported as a [`TimelineAction`], which
//! the caller applies to its own authoritative model. View-only changes
//! (zoom / pan) are applied to [`crate::TimelineView`] in place and are *not*
//! reported as actions.

use crate::model::{ClipId, KeyPos};

/// A single edit intent produced by a timeline gesture.
///
/// Frame deltas are signed: positive moves later in time / down a track.
/// The caller is responsible for clamping (e.g. preventing negative durations)
/// when applying these to its model — the widget proposes, the caller disposes.
///
/// Not `Copy`: the multi-key variants carry `Vec<KeyPos>`. Not `Eq`: key times are `f64`.
#[derive(Clone, Debug, PartialEq)]
pub enum TimelineAction {
    /// The playhead should move to `frame` (ruler scrub or empty-space click).
    Seek { frame: i64 },

    /// A work-area handle was dragged on the ruler.
    ///
    /// Carries the WHOLE range, not the edge that moved, so the caller applies one consistent
    /// state instead of reconstructing the other end from a model it may already have changed.
    ///
    /// **`end` is EXCLUSIVE**, matching [`crate::WorkArea`]. A caller whose own range is
    /// inclusive converts at this boundary - and should keep that `-1` next to the `+1` it
    /// already writes when building `WorkArea`, so the pair is readable in one place.
    ///
    /// The widget clamps only enough to keep the range non-empty (`end > start`); everything
    /// else - refusing the edit outright, snapping, extending past the content - is the
    /// caller's, as with every other action here.
    SetWorkArea { start: i64, end: i64 },

    /// `id` should become the selection. The widget reports the raw click;
    /// modifier-aware multi-select is left to the caller (it owns the model's
    /// selection list). `additive` is `true` when Ctrl/Cmd was held (toggle),
    /// `range` is `true` when Shift was held (range select).
    Select {
        id: ClipId,
        additive: bool,
        range: bool,
    },

    /// Selection should be cleared (click on empty space).
    ClearSelection,

    /// Clip `id` should move to a new start frame and/or track.
    ///
    /// `new_track` is the destination track index (clamped to the track count
    /// by the widget). When the drag was purely horizontal it equals the
    /// clip's current track.
    MoveClip {
        id: ClipId,
        new_start: i64,
        new_track: usize,
    },

    /// Trim the left (head) edge of clip `id` by `delta` frames.
    /// Positive `delta` trims more material off the head (visible start moves
    /// right); negative restores it.
    TrimStart { id: ClipId, delta: i64 },

    /// Trim the right (tail) edge of clip `id` by `delta` frames.
    /// Positive `delta` trims more material off the tail (visible end moves
    /// left); negative restores it.
    TrimEnd { id: ClipId, delta: i64 },

    /// Slide clip `id` by `delta` frames: the full bar moves while the visible
    /// (played) content stays fixed, by compensating `trim_in`/`trim_out`.
    /// The caller applies `start += delta`, `trim_in -= delta`,
    /// `trim_out += delta` (clamped to `>= 0`).
    Slide { id: ClipId, delta: i64 },

    /// Add a keyframe at time `frame` on lane `lane` of track `track` (Ctrl+click in a
    /// lane). `track`/`lane` index into [`crate::TimelineModel::tracks`] and the
    /// track's [`crate::Track::lanes`]. The caller chooses the new key's value.
    ///
    /// `frame` is the time under the cursor: the nearest whole frame while
    /// [`crate::TimelineConfig::snap_keys_to_frames`] is on (the default), else the exact
    /// fractional time.
    AddKey {
        track: usize,
        lane: usize,
        frame: f64,
    },

    /// Select `keys` (marquee commit or Shift/plain click on a diamond).
    /// `add == false` REPLACES the current key selection; `add == true` extends
    /// it. The host owns the authoritative selection set; the widget only reports
    /// the positional hits it found.
    SelectKeys { keys: Vec<KeyPos>, add: bool },

    /// Clear the keyframe selection (plain click on empty lane space, or an empty
    /// non-additive marquee).
    ClearKeySelection,

    /// Move every currently-selected keyframe by `delta` frames as one atomic
    /// block (diamond drag). `keys` are the selected diamonds at release time (each by
    /// its exact [`KeyPos::frame`]); the host applies `frame + delta` and re-derives
    /// selection from the result.
    ///
    /// While [`crate::TimelineConfig::snap_keys_to_frames`] is on (the default) `delta` is a
    /// whole number of frames, so a key between frames keeps its fraction; off, it is the
    /// exact fractional distance dragged.
    MoveKeys { keys: Vec<KeyPos>, delta: f64 },

    /// Delete `keys` atomically (Delete/Backspace on the selection, the RMB menu,
    /// or Ctrl+RMB on a single diamond).
    DeleteKeys { keys: Vec<KeyPos> },
}
