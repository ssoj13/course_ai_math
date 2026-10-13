//! `egui-track-timeline` — a generic, multi-track NLE-style timeline widget for egui.
//!
//! Stacked tracks of clips are drawn as bars over a horizontal time axis with a
//! draggable playhead, an adaptive frame grid, zoom + horizontal pan, and the
//! four classic edit gestures: **move** (reposition in time / across tracks),
//! **trim-in** / **trim-out** (drag an edge), and **slide** (move the source
//! content while keeping the played bounds fixed).
//!
//! # Decoupling
//!
//! The widget owns **no application types**. The caller builds a flat
//! [`TimelineModel`] each frame (tracks of [`Clip`]s keyed by an opaque
//! [`ClipId`] = `u64`) and passes it to [`TrackTimeline::show`]. View state
//! ([`TimelineView`]: zoom / pan / active drag) is *owned and mutated* by the
//! widget. Model edits are **not** applied directly — they are returned as
//! [`TimelineAction`]s for the caller to apply to its own authoritative model.
//!
//! This mirrors the immediate-mode "the widget proposes, the caller disposes"
//! pattern and keeps the timeline reusable across apps.
//!
//! # Key times
//!
//! Clips live on whole frames (`i64`); keyframes on a property lane live at any TIME
//! ([`Keyframe::frame`], an `f64` in frames): a key between frames is drawn, hit-tested and
//! reported ([`KeyPos::frame`]) at its exact position. Key gestures snap to whole frames by
//! default ([`TimelineConfig::snap_keys_to_frames`]): Ctrl+click adds a key on the nearest frame
//! and a drag moves keys by whole frames, so a key between frames keeps its fraction.
//!
//! # Usage sketch
//!
//! ```no_run
//! use egui_track_timeline::{
//!     Clip, TimelineAction, TimelineConfig, TimelineModel, TimelineView, Track, TrackTimeline,
//! };
//!
//! struct App {
//!     view: TimelineView,
//!     timeline: TrackTimeline,
//!     // ... caller's own clip storage ...
//! }
//!
//! fn ui(app: &mut App, ui: &mut egui::Ui) {
//!     // Build the flat model from the caller's data each frame.
//!     let model = TimelineModel::new(
//!         24.0,
//!         vec![Track::new(
//!             "V1",
//!             vec![Clip::new(/*id*/ 1, /*start*/ 0, /*duration*/ 100, "shot_01")],
//!         )],
//!     );
//!
//!     let resp = app.timeline.show(ui, &mut app.view, &model);
//!     for action in resp.actions {
//!         match action {
//!             TimelineAction::Seek { frame } => { /* set playhead */ }
//!             TimelineAction::SetWorkArea { start, end } => { /* end is EXCLUSIVE */ }
//!             TimelineAction::MoveClip { id, new_start, new_track } => { /* apply */ }
//!             TimelineAction::TrimStart { id, delta } => { /* apply */ }
//!             TimelineAction::TrimEnd { id, delta } => { /* apply */ }
//!             TimelineAction::Slide { id, delta } => { /* apply */ }
//!             TimelineAction::Select { id, additive, range } => { /* apply */ }
//!             TimelineAction::ClearSelection => { /* clear */ }
//!             TimelineAction::AddKey { track, lane, frame } => { /* add keyframe */ }
//!             TimelineAction::SelectKeys { keys, add } => { /* select keys */ }
//!             TimelineAction::ClearKeySelection => { /* clear key selection */ }
//!             TimelineAction::MoveKeys { keys, delta } => { /* block-move keys */ }
//!             TimelineAction::DeleteKeys { keys } => { /* delete keys */ }
//!         }
//!     }
//!     let _ = TimelineConfig::default();
//! }
//! ```

mod actions;
mod model;
mod paint;
mod view;
mod widget;

pub use actions::TimelineAction;
/// The clip fill patterns (`Fill::Pattern`), so a host needs no second dependency.
pub use egui_pattern::{Dir, Pattern, Prim};
pub use model::{
    Band, Clip, ClipId, Fill, KeyPos, Keyframe, PropLane, TimelineModel, Track, WorkArea,
};
pub use paint::Tool;
pub use view::{DragState, TimelineConfig, TimelineView, ZOOM_MAX, ZOOM_MIN, key_delta, key_time};
pub use widget::{TimelineResponse, TrackTimeline};
