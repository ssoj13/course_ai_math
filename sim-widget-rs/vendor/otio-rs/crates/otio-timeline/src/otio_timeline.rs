//! otio-timeline — a starter egui NLE timeline widget over [`otio_edit::Editor`].
//!
//! This is the "face" of the timeline: it draws tracks + clips + the playhead
//! (DaVinci/After-Effects style) and turns mouse gestures into `otio_edit::ops`
//! — Move (reposition, leaves a gap), Slide (roll neighbours), Trim (edge in/out),
//! and a Razor tool, plus Delete (lift a clip / close a selected gap) and
//! Shift-Delete (ripple a clip) —
//! all with undo/redo. Trimming a shared edit point (a clip on both sides) rolls
//! both edges. Drags snap to neighbouring clip edges / the playhead (hold Alt to
//! bypass). The
//! "brain" (what an edit does, undo, layout) is entirely in otio-edit; this
//! crate holds only UI state and rendering.
//!
//! ## Usage
//! ```ignore
//! // once: let mut state = TimelineState::default();
//! // each frame, inside an egui Ui:
//! otio_timeline::timeline_ui(ui, &mut editor, &mut state);
//! ```
//! `editor` is your `otio_edit::Editor`; `state` is the widget's own UI state
//! (zoom/scroll/selection/tool). See `examples/demo.rs` for a runnable window.
//!
//! The whole crate is gated on the `widget` feature (egui is a heavy, optional
//! dep): with the feature off the crate is empty, so the otio-rs workspace can
//! include it without pulling egui into a default build.
#![cfg(feature = "widget")]

use std::collections::HashSet;

use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

use otio_core::{Document, Field, canonical_schema};
// Re-exported so hosts writing a `TimelineOpts::thumbnail` provider (and any other
// NodeId-keyed callback) can name the id type without depending on otio-core directly.
pub use otio_core::NodeId;
// Phosphor glyph constants for the drawn edit cursors (hand / ↔ / scissors) and the
// demo toolbar. The host registers the font via `egui_widgets_config::add_icon_font`; we only
// reference the &str consts here (no font work needed to name a glyph).
use egui_widgets_config::icons as ph;
use otio_edit::{Editor, LayoutCache, ops, snap};
use otio_schema::{build, codec};
use otio_time::{RationalTime, TimeRange};

/// The active editing tool. Two tools, DaVinci-Resolve style — one context-sensitive
/// `Edit` tool plus a `Blade`, instead of a separate radio per gesture:
/// - `Edit` — the grab ZONE picks the verb: drag a clip BODY to reposition it
///   (`move_by`; hold **Ctrl** to `slide` between neighbours instead), drag a clip
///   EDGE to trim its in/out (`trim_in`/`trim_out`), or ROLL a shared cut when the
///   neighbour on that edge is a clip. A plain click selects; an empty-space drag
///   rubber-band selects.
/// - `Blade` — click a clip to split it at the cursor (`razor`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tool {
    /// Context-sensitive edit: body→move (Ctrl→slide), edge→trim/roll, click→select.
    #[default]
    Edit,
    /// Click splits the clip under the cursor at that time (razor).
    Blade,
}

/// An in-progress mouse drag (started on a clip, applied on release).
#[derive(Clone, Copy)]
enum Drag {
    /// Reposition the clip, leaving a same-duration gap where it was (lift +
    /// overwrite — `ops::move_by`). Neighbours are NOT rolled. Same-kind destination
    /// lanes emit a cross-track move command.
    Move { clip: NodeId },
    /// Slide the clip between its neighbours (`ops::slide`): its media window is
    /// fixed and the neighbours absorb the delta (a non-rippling roll).
    Slide { clip: NodeId },
    /// Trim the clip's in-point (left edge).
    TrimIn { clip: NodeId },
    /// Trim the clip's out-point (right edge).
    TrimOut { clip: NodeId },
    /// Roll the shared edit point between `left` and its next neighbour (both
    /// clips): `left`'s out and the next clip's in move together. `edge_is_end`
    /// marks that the grabbed edge was the hit clip's out-edge, so the release
    /// snaps that clip's END rather than its start.
    Roll { left: NodeId, edge_is_end: bool },
}

/// The widget's UI state — owned by your app, persisted across frames.
pub struct TimelineState {
    /// Horizontal zoom: pixels per second of timeline time.
    pub px_per_sec: f32,
    /// Horizontal scroll offset, in pixels.
    pub scroll_x: f32,
    /// Per-track lane height, in pixels. Public so a host can drive a "track size"
    /// control; clamped to [`TRACK_H_MIN`]..=[`TRACK_H_MAX`] each frame.
    pub track_h: f32,
    /// Playhead position, in seconds.
    pub playhead: f64,
    /// The frame rate edits snap to (display/edit rate).
    pub rate: f64,
    /// The currently selected node ids (clips/gaps).
    pub selection: HashSet<NodeId>,
    /// The active tool.
    pub tool: Tool,
    /// Last edit error message (e.g. "edit exceeds media available range").
    pub last_error: Option<String>,
    /// Per-track layout cache (memoizes `range_of_all_children`).
    layout: LayoutCache,
    /// Active drag, if any.
    drag: Option<Drag>,
    /// Pointer time (seconds) where the active drag began.
    grab_sec: f64,
    /// Drag-start capture for snapping: the dragged clip's track, or `None` if the
    /// capture failed (then the release skips snapping).
    drag_track: Option<NodeId>,
    /// The dragged clip's start / end (seconds) at drag start — the moving edge the
    /// release snaps to neighbouring edges / the playhead.
    drag_start_sec: f64,
    drag_end_sec: f64,
    /// Transport playing state. Command hosts own clock advancement; legacy wrappers
    /// advance the playhead while this is true.
    pub playing: bool,
    /// In/out marks (seconds) — the record-side range of a three-point edit. `I`/`O` set
    /// them at the playhead; the widget renders them as a band. A host clears them / the
    /// marked range via [`clear_marks`] / [`clear_marked_range`].
    pub mark_in: Option<f64>,
    pub mark_out: Option<f64>,
    /// Active marquee (rubber-band) selection: the drag-start screen position of an
    /// empty-space drag; `None` when not marqueeing.
    marquee: Option<Pos2>,
    /// True while dragging in the ruler to scrub the playhead (vs a track-area marquee drag).
    scrubbing: bool,
    /// True while a `Ctrl`+drag on the ruler/empty track is scrubbing the B-ROLL time point. Kept
    /// separate from `scrubbing` because b-roll is a compare reference in TIME, not the transport: it
    /// never pauses/resumes playback (no `ScrubStart`/`ScrubStop`), and like `scrubbing` it survives a
    /// projection rebuild (it moves in time, independent of the clip layout).
    broll_scrubbing: bool,
    /// Was playback running when the current ruler-scrub began (to resume on release).
    scrub_was_playing: bool,
    /// Identity band of the Editor/projection currently driving this state. Replacing an
    /// Editor can reuse the same deterministic NodeIds, so transient interaction state must
    /// not leak into the replacement document.
    projection_epoch: Option<u64>,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self {
            px_per_sec: 40.0,
            scroll_x: 0.0,
            track_h: TRACK_H_DEFAULT,
            playhead: 0.0,
            rate: 24.0,
            selection: HashSet::new(),
            tool: Tool::Edit,
            last_error: None,
            layout: LayoutCache::new(),
            drag: None,
            grab_sec: 0.0,
            drag_track: None,
            drag_start_sec: 0.0,
            drag_end_sec: 0.0,
            playing: false,
            mark_in: None,
            mark_out: None,
            marquee: None,
            scrubbing: false,
            broll_scrubbing: false,
            scrub_was_playing: false,
            projection_epoch: None,
        }
    }
}

// --- geometry constants -----------------------------------------------------

const HEADER_W: f32 = 88.0; // left track-header column width
const RULER_H: f32 = 22.0; // top time-ruler height
/// Default per-track lane height (px). Min/max bound the public [`TimelineState::track_h`]
/// so a host slider clamps to the SAME range the widget enforces internally.
pub const TRACK_H_DEFAULT: f32 = 56.0;
/// Minimum track-lane height (px). See [`TRACK_H_DEFAULT`].
pub const TRACK_H_MIN: f32 = 24.0;
/// Maximum track-lane height (px). See [`TRACK_H_DEFAULT`].
pub const TRACK_H_MAX: f32 = 240.0;
const TRACK_GAP: f32 = 2.0; // gap between lanes
const EDGE_GRAB: f32 = 9.0; // px from a clip edge that grabs a trim handle / shows the ↔ cursor
const HIT_PAD: f32 = 4.0; // px of slop beyond a clip's edges so a near-miss grabs it, not a marquee
const SNAP_PX: f32 = 8.0; // px hysteresis for snapping a dragged edge to a target

/// Minimum zoom (px/sec). Public so hosts driving the zoom UI (e.g. a slider over
/// the public [`TimelineState::px_per_sec`] field) use the SAME bound the widget
/// clamps to internally — the two writers can't disagree on the valid range.
pub const PX_PER_SEC_MIN: f32 = 2.0;
/// Maximum zoom (px/sec). See [`PX_PER_SEC_MIN`].
pub const PX_PER_SEC_MAX: f32 = 2000.0;
// Fallback frame rate when the public `rate` field holds a non-positive/NaN value.
const DEFAULT_RATE: f64 = 24.0;

/// Optional, per-frame extension points for [`timeline_ui_ex`]. Kept separate so the
/// zero-config [`timeline_ui`] stays a non-breaking one-liner; hosts that want extras
/// (currently per-clip thumbnails) pass a populated `TimelineOpts`.
#[derive(Default)]
pub struct TimelineOpts<'a> {
    /// Per-clip thumbnail provider: maps a clip's [`NodeId`] to an egui texture painted
    /// as the clip body's background. Called once per visible clip per frame; the host
    /// owns the texture's lifetime (e.g. a decoded poster frame uploaded to egui).
    pub thumbnail: Option<&'a dyn Fn(NodeId) -> Option<egui::TextureId>>,
}

/// Where a move command lands. Same-track moves use a delta; cross-track moves carry
/// the destination track and absolute timeline-local start explicitly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MoveDestination {
    /// Reposition on the current track by `delta`.
    SameTrack { delta: RationalTime },
    /// Move to another track at the absolute `start` time.
    Track { track: NodeId, start: RationalTime },
}

/// A command emitted by the immutable timeline interaction API.
///
/// Edit commands are applied by [`apply_timeline_response`]. Transport commands only
/// update [`TimelineState`]; importantly, this enum has no "tick" command and the
/// command-oriented API never owns or advances a playback clock.
#[derive(Clone, Debug, PartialEq)]
pub enum TimelineCommand {
    Move {
        clip: NodeId,
        destination: MoveDestination,
    },
    TrimIn {
        clip: NodeId,
        delta: RationalTime,
    },
    TrimOut {
        clip: NodeId,
        delta: RationalTime,
    },
    Slide {
        clip: NodeId,
        delta: RationalTime,
    },
    Roll {
        left: NodeId,
        delta: RationalTime,
    },
    Blade {
        track: NodeId,
        time: RationalTime,
    },
    Lift {
        clip: NodeId,
    },
    RippleDelete {
        clip: NodeId,
    },
    TogglePlay,
    SetPlayhead {
        seconds: f64,
    },
    ScrubStart,
    ScrubStop,
    /// Name a B-ROLL point in TIME (timeline seconds) — a HOST action, NOT a document edit (so
    /// [`is_edit_command`] returns false and no revision match is required). The host compares the
    /// ACTIVE clip against ITSELF at this time (b-roll: the same footage at a different moment), so the
    /// gesture carries a time, not a clip. Emitted by a `Ctrl`+left-CLICK anywhere on the timeline, a
    /// `Ctrl`+left-DRAG on the ruler/empty track (which scrubs this point live), or the "Roll B from
    /// this time" context-menu item (at the playhead). The widget neither reads nor mutates B — it
    /// only NAMES the time.
    SetBRoll {
        seconds: f64,
    },
    Cancel {
        reason: TimelineCancelReason,
    },
    /// Application errors are returned as terminal events in
    /// [`TimelineResponse::applied_commands`] and mirrored in `errors`.
    Error {
        message: String,
    },
}

/// Why an in-progress interaction was cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineCancelReason {
    PointerUnavailable,
    ProjectionReplaced,
    StaleResponse,
}

/// Commands and observable effects produced by one timeline frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TimelineResponse {
    /// Editor revision from which every pending NodeId command was generated.
    pub base_revision: u64,
    /// Pending commands. [`apply_timeline_response`] consumes this vector.
    pub commands: Vec<TimelineCommand>,
    /// Commands successfully applied, plus terminal `Cancel`/`Error` events produced
    /// while applying. This is observational and is never applied a second time.
    pub applied_commands: Vec<TimelineCommand>,
    /// Commands refused because the response was stale or the edit returned an error.
    pub rejected_commands: Vec<TimelineCommand>,
    /// True when UI state or the document changed (or an emitted command requests one).
    pub changed: bool,
    /// True when the host should schedule another paint after applying commands.
    pub repaint: bool,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

impl TimelineResponse {
    /// Start a response whose NodeId commands are valid for `editor`'s current snapshot.
    pub fn for_editor(editor: &Editor) -> Self {
        Self {
            base_revision: editor.revision(),
            ..Self::default()
        }
    }

    fn emit(&mut self, command: TimelineCommand) {
        if matches!(command, TimelineCommand::Cancel { .. }) {
            self.cancelled = true;
        }
        self.commands.push(command);
        self.changed = true;
        self.repaint = true;
    }

    fn ui_changed(&mut self) {
        self.changed = true;
        self.repaint = true;
    }
}

/// Apply commands emitted by [`timeline_ui_commands`] to the mutable legacy model/state.
///
/// Pending commands are consumed. Before any NodeId-bearing edit is attempted, the
/// response's base revision must still match the editor snapshot it was generated from;
/// otherwise the whole pending batch is rejected. Errors are preserved in
/// `response.errors`, while terminal error events live in `response.applied_commands`.
pub fn apply_timeline_response(
    editor: &mut Editor,
    state: &mut TimelineState,
    mut response: TimelineResponse,
) -> TimelineResponse {
    let commands = std::mem::take(&mut response.commands);
    if commands.iter().any(is_edit_command) && response.base_revision != editor.revision() {
        let message = format!(
            "stale timeline response: base revision {}, current revision {}",
            response.base_revision,
            editor.revision()
        );
        response.rejected_commands.extend(commands);
        response.cancelled = true;
        response.changed = true;
        response.repaint = true;
        response.errors.push(message.clone());
        response.applied_commands.push(TimelineCommand::Cancel {
            reason: TimelineCancelReason::StaleResponse,
        });
        response.applied_commands.push(TimelineCommand::Error {
            message: message.clone(),
        });
        state.last_error = Some(message);
        return response;
    }

    let mut successful_edit = false;
    for command in commands {
        if !matches!(command, TimelineCommand::Error { .. }) {
            response.changed = true;
            response.repaint = true;
        }
        match apply_timeline_command(editor, state, &command) {
            Err(error) => {
                let message = error.to_string();
                response.errors.push(message.clone());
                response.rejected_commands.push(command);
                response
                    .applied_commands
                    .push(TimelineCommand::Error { message });
            }
            Ok(()) => {
                if is_edit_command(&command) {
                    successful_edit = true;
                    let doc = editor.document();
                    state.selection.retain(|id| doc.node(*id).is_some());
                }
                response.applied_commands.push(command);
            }
        }
    }
    if let Some(error) = response.errors.last() {
        state.last_error = Some(error.clone());
    } else if successful_edit {
        state.last_error = None;
    }
    response
}

fn is_edit_command(command: &TimelineCommand) -> bool {
    matches!(
        command,
        TimelineCommand::Move { .. }
            | TimelineCommand::TrimIn { .. }
            | TimelineCommand::TrimOut { .. }
            | TimelineCommand::Slide { .. }
            | TimelineCommand::Roll { .. }
            | TimelineCommand::Blade { .. }
            | TimelineCommand::Lift { .. }
            | TimelineCommand::RippleDelete { .. }
    )
}

fn apply_timeline_command(
    editor: &mut Editor,
    state: &mut TimelineState,
    command: &TimelineCommand,
) -> Result<(), otio_edit::EditError> {
    match command {
        TimelineCommand::Move { clip, destination } => match destination {
            MoveDestination::SameTrack { delta } => ops::move_by(editor, *clip, *delta),
            MoveDestination::Track { track, start } => {
                ops::move_clip_cross_track(editor, *clip, *track, *start)
            }
        },
        TimelineCommand::TrimIn { clip, delta } => ops::trim_in_record(editor, *clip, *delta),
        TimelineCommand::TrimOut { clip, delta } => ops::trim_out(editor, *clip, *delta),
        TimelineCommand::Slide { clip, delta } => ops::slide(editor, *clip, *delta),
        TimelineCommand::Roll { left, delta } => ops::roll(editor, *left, *delta),
        TimelineCommand::Blade { track, time } => ops::razor(editor, *track, *time),
        TimelineCommand::Lift { clip } => ops::lift_clip(editor, *clip),
        TimelineCommand::RippleDelete { clip } => ops::ripple_delete_clip(editor, *clip),
        TimelineCommand::TogglePlay => {
            state.playing = !state.playing;
            Ok(())
        }
        TimelineCommand::SetPlayhead { seconds } => {
            state.playhead = seconds.max(0.0);
            Ok(())
        }
        TimelineCommand::ScrubStart => {
            state.scrub_was_playing = state.playing;
            state.playing = false;
            Ok(())
        }
        TimelineCommand::ScrubStop => {
            if state.scrub_was_playing {
                state.playing = true;
            }
            state.scrub_was_playing = false;
            Ok(())
        }
        // Host signals — the widget's own legacy-apply path does not act on them (no Editor mutation,
        // no clock change); they travel in `commands` for the host to consume.
        TimelineCommand::Cancel { .. }
        | TimelineCommand::Error { .. }
        | TimelineCommand::SetBRoll { .. } => Ok(()),
    }
}

/// Draw + interact with the timeline for one frame. This source-compatible wrapper
/// applies emitted edit/transport commands to `editor` and preserves the legacy clock.
pub fn timeline_ui(ui: &mut egui::Ui, editor: &mut Editor, state: &mut TimelineState) {
    timeline_ui_ex(ui, editor, state, &TimelineOpts::default());
}

/// Like [`timeline_ui`], plus host extension points via [`TimelineOpts`] (e.g. thumbnails).
pub fn timeline_ui_ex(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    state: &mut TimelineState,
    opts: &TimelineOpts,
) {
    // The command pass resets replaced projections before emitting any interaction.
    // Apply transport changes before ticking: Pause gets no extra dt, while Play starts
    // advancing in the frame that accepted the toggle.
    let response = timeline_ui_commands_ex(ui, editor, state, opts);
    let response = apply_timeline_response(editor, state, response);
    advance_legacy_playback(ui, editor, state);
    if response.repaint {
        ui.ctx().request_repaint();
    }
}

/// Immutable-editor command API with no host extensions.
pub fn timeline_ui_commands(
    ui: &mut egui::Ui,
    editor: &Editor,
    state: &mut TimelineState,
) -> TimelineResponse {
    timeline_ui_commands_ex(ui, editor, state, &TimelineOpts::default())
}

/// Draw and interact using immutable document data, returning commands for the host.
/// This function never advances playback time and never mutates the [`Editor`].
pub fn timeline_ui_commands_ex(
    ui: &mut egui::Ui,
    editor: &Editor,
    state: &mut TimelineState,
    opts: &TimelineOpts,
) -> TimelineResponse {
    let mut output = TimelineResponse::for_editor(editor);
    reset_replaced_projection(editor, state, &mut output);
    let view_before = (
        state.px_per_sec,
        state.scroll_x,
        state.track_h,
        state.rate,
        state.mark_in,
        state.mark_out,
    );
    // CLASS-D guard: `px_per_sec` and `rate` are PUBLIC, so a host can set them to
    // 0, a negative, or NaN. The time<->pixel closures divide by `px_per_sec` and
    // frame-snapping (`rt`) multiplies by `rate`, so an invalid value yields
    // div-by-zero / NaN coordinates and NaN edits. Enforce the invariants on the
    // LIVE values here, before any use — don't rely on the Ctrl+wheel handler
    // (one writer among possibly many) to keep them sane.
    state.px_per_sec = if state.px_per_sec.is_finite() {
        state.px_per_sec.clamp(PX_PER_SEC_MIN, PX_PER_SEC_MAX)
    } else {
        PX_PER_SEC_MIN // clamp() would pass NaN through, so handle it explicitly
    };
    if state.rate.is_nan() || state.rate <= 0.0 {
        // catches 0.0, negatives, and NaN (+inf is left as-is, as before)
        state.rate = DEFAULT_RATE;
    }
    // Same LIVE-value guard for the public track height (drives lane geometry).
    state.track_h = if state.track_h.is_finite() {
        state.track_h.clamp(TRACK_H_MIN, TRACK_H_MAX)
    } else {
        TRACK_H_DEFAULT
    };

    let tracks = timeline_tracks(editor.document());
    let content_h = RULER_H + tracks.len() as f32 * (state.track_h + TRACK_GAP) + 8.0;
    // Fill the host's available height so the WHOLE panel is the hover / interaction target. Keyboard
    // zoom (`-`/`=`/`f`) and Ctrl+wheel are hover-scoped (`resp.hovered()` below), and a timeline that
    // claimed only its track height left the rest of an embedding dock as DEAD SPACE where those
    // shortcuts silently did nothing when the pointer sat there. Tracks draw top-aligned; the surplus
    // is background. Never below the natural content height (or the 120 px floor).
    let height = content_h.max(ui.available_height()).max(120.0);
    let size = Vec2::new(ui.available_width(), height);
    let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let area = resp.rect;

    // Background.
    painter.rect_filled(area, CornerRadius::ZERO, Color32::from_gray(24));

    let content_left = area.left() + HEADER_W;
    // Capture the zoom/scroll by COPY (not `&state`) so the time<->pixel closures
    // don't hold a borrow of `state` that would clash with the `&mut state`
    // interaction pass below. They are `Copy`, so passing them by value is free.
    let (px_per_sec, scroll_x, track_h) = (state.px_per_sec, state.scroll_x, state.track_h);
    let to_x = move |sec: f64| -> f32 { content_left + (sec as f32 * px_per_sec) - scroll_x };
    let to_sec = move |x: f32| -> f64 { ((x - content_left + scroll_x) / px_per_sec) as f64 };

    // Feed the ruler the SAME captured copies `to_x`/`to_sec` closed over (not
    // `state`), so tick spacing and tick positions share one source of truth —
    // a future reorder mutating `state.px_per_sec` before this call can't desync
    // the ruler's grid from the closures that place clips on it.
    // Ruler labels are real timecode (HH:MM:SS:FF at `rate`) offset by the timeline's
    // `global_start_time`, so the ruler reads like a deck/NLE, not raw seconds-from-zero.
    let start_off = timeline_global_start(editor.document());
    draw_ruler(
        &painter,
        area,
        content_left,
        px_per_sec,
        scroll_x,
        state.rate,
        start_off,
        to_x,
    );

    // Hover/press positions for hit-testing.
    let hover = resp.hover_pos();
    let press = resp.interact_pointer_pos();

    // Transport is command-oriented: the host owns playback time. This immutable API only
    // emits the toggle; the source-compatible wrapper advances its legacy clock separately.
    if resp.hovered() && ui.input(|i| i.key_pressed(egui::Key::Space)) {
        output.emit(TimelineCommand::TogglePlay);
    }

    // In/out marks (record-side of a three-point edit): I/O set the mark at the playhead.
    // Clearing the marks / the marked range is done by the host via the public helpers.
    if resp.hovered() {
        let (mi, mo) = ui.input(|i| (i.key_pressed(egui::Key::I), i.key_pressed(egui::Key::O)));
        if mi {
            state.mark_in = Some(state.playhead);
            output.ui_changed();
        }
        if mo {
            state.mark_out = Some(state.playhead);
            output.ui_changed();
        }
    }

    // Lay out and draw each track + its clips; collect a hit (clip, track, edge).
    let mut hit: Option<(NodeId, NodeId, EdgeZone)> = None;
    // On-screen clip rects, collected while drawing, for marquee hit-testing on release.
    let mut clip_rects: Vec<(NodeId, Rect)> = Vec::new();
    // Rightmost clip end (seconds), accumulated while drawing — used to clamp
    // horizontal scroll to the content extent below.
    let mut content_end_sec = 0.0_f64;
    for (i, &track) in tracks.iter().enumerate() {
        let top = area.top() + RULER_H + i as f32 * (track_h + TRACK_GAP);
        let lane = Rect::from_min_size(
            Pos2::new(area.left(), top),
            Vec2::new(area.width(), track_h),
        );
        // Lane background (alternating) + header.
        let lane_bg = if i % 2 == 0 {
            Color32::from_gray(32)
        } else {
            Color32::from_gray(28)
        };
        painter.rect_filled(lane, CornerRadius::ZERO, lane_bg);
        draw_track_header(
            &painter,
            editor.document(),
            track,
            area.left(),
            top,
            track_h,
        );

        // Clip rectangles (clip to the content area, cull off-screen).
        let layout = state.layout.track_layout(editor, track).to_vec();
        for (clip, range) in &layout {
            let start_sec = range.start_time().to_seconds();
            let end_sec = start_sec + range.duration().to_seconds();
            content_end_sec = content_end_sec.max(end_sec); // track content extent
            let x0 = to_x(start_sec);
            let x1 = to_x(end_sec);
            if x1 < content_left || x0 > area.right() {
                continue; // off-screen
            }
            // Clamp the left edge to the content area (same as the hit-test below).
            let left = x0.max(content_left);
            let r = Rect::from_min_max(
                Pos2::new(left, top + 3.0),
                Pos2::new(x1, top + track_h - 3.0),
            );
            let selected = state.selection.contains(clip);
            let thumb = opts.thumbnail.and_then(|f| f(*clip));
            draw_clip(&painter, editor.document(), *clip, r, selected, thumb);
            clip_rects.push((*clip, r));

            // Hit-test against the ON-SCREEN clip span. The left bound uses the
            // same `x0.max(content_left)` the draw uses, and we additionally
            // require `p.x >= content_left`, so a click in the header column never
            // lands on a clip when `scroll_x` has pushed x0 left of the content
            // area — keeping render and interaction in agreement.
            if let Some(p) = hover.or(press)
                && p.x >= content_left
                && p.x >= left - HIT_PAD
                && p.x <= x1 + HIT_PAD
                && p.y >= top
                && p.y <= top + track_h
            {
                // Edge-zone disambiguation. A clip wide enough for two grab
                // handles plus a body trims at the 6 px edges and moves in the
                // middle. A narrow clip (on-screen width < 2*EDGE_GRAB) would let
                // the edge zones overlap and swallow the body, leaving only TrimIn
                // reachable — so split its width in half instead (left → TrimIn,
                // right → TrimOut, no Body; zoom in to move a narrow clip).
                let zone = if x1 - left < 2.0 * EDGE_GRAB {
                    if p.x - left <= (x1 - left) * 0.5 {
                        EdgeZone::Left
                    } else {
                        EdgeZone::Right
                    }
                } else if p.x - left <= EDGE_GRAB {
                    EdgeZone::Left
                } else if x1 - p.x <= EDGE_GRAB {
                    EdgeZone::Right
                } else {
                    EdgeZone::Body
                };
                hit = Some((*clip, track, zone));
            }
        }
    }

    // Custom edit cursors (Phosphor glyphs) instead of the native OS arrows: while a drag
    // is live the cursor reflects the VERB (4-way move arrows for move/slide, ↔ for
    // trim/roll); otherwise it reflects what the hovered ZONE would do (↔ over an edge,
    // move arrows over a body, scissors anywhere in the Blade tool). We hide the native
    // cursor and paint the glyph on a FOREGROUND layer so it's never clipped or overdrawn.
    let middle_panning = resp.dragged_by(egui::PointerButton::Middle);
    let cursor_glyph: Option<&str> = if middle_panning {
        None
    } else if let Some(drag) = state.drag {
        Some(match drag {
            Drag::Move { .. } | Drag::Slide { .. } => ph::MOVE,
            Drag::TrimIn { .. } | Drag::TrimOut { .. } | Drag::Roll { .. } => ph::ARROWS_HORIZONTAL,
        })
    } else {
        match state.tool {
            Tool::Blade => hover
                .filter(|p| p.x >= content_left && p.y >= area.top() + RULER_H)
                .map(|_| ph::CUT),
            Tool::Edit => hit.map(|(_, _, zone)| match zone {
                EdgeZone::Left | EdgeZone::Right => ph::ARROWS_HORIZONTAL,
                EdgeZone::Body => ph::MOVE,
            }),
        }
    };
    if middle_panning {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if let (Some(glyph), Some(p)) = (cursor_glyph, press.or(hover)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        ui.ctx()
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("otio_timeline_cursor"),
            ))
            .text(
                p,
                Align2::CENTER_CENTER,
                glyph,
                FontId::proportional(22.0),
                Color32::from_gray(240),
            );
    }

    // --- interaction --------------------------------------------------------
    // Alt bypasses snapping (standard NLE modifier).
    let snap_on = !ui.input(|i| i.modifiers.alt);
    // Ctrl at drag-start turns an Edit body-drag into a Slide (source-side modifier).
    let ctrl = ui.input(|i| i.modifiers.ctrl);
    handle_input(
        &resp,
        editor,
        state,
        &mut output,
        &hit,
        area,
        to_sec,
        snap_on,
        ctrl,
        track_h,
    );

    // Ctrl+LEFT-click ANYWHERE on the timeline (clip body, ruler, or empty track) names a B-ROLL
    // point: the time under the pointer. The host compares the ACTIVE clip against ITSELF at this time
    // (b-roll), so the gesture needs a TIME, not a clip. This is a CLICK, not a drag: a Ctrl body-DRAG
    // stays a Slide (handled in `handle_input`) and the two never collide because a click emits no
    // drag; a Ctrl drag on the ruler/empty track scrubs this same point live (below). The empty-space
    // click that would move the PLAYHEAD is gated off under Ctrl in `handle_input`, so this is the sole
    // effect of a Ctrl+click. The host resolves + points its compare source at this time.
    if resp.clicked_by(egui::PointerButton::Primary)
        && ctrl
        && let Some(p) = resp.interact_pointer_pos()
    {
        output.emit(TimelineCommand::SetBRoll {
            seconds: to_sec(p.x).max(0.0),
        });
    }

    // Right-click selects the item under the pointer (unless it is already part of a multi-select)
    // and opens a command-oriented context menu. The menu never mutates the Editor directly.
    if resp.clicked_by(egui::PointerButton::Secondary)
        && let Some((item, _, _)) = hit
        && !state.selection.contains(&item)
    {
        state.selection.clear();
        state.selection.insert(item);
        output.ui_changed();
    }
    resp.context_menu(|ui| {
        if ui.button("Select All").clicked() {
            let selection = selectable_timeline_items(editor.document(), &tracks);
            if state.selection != selection {
                state.selection = selection;
                output.ui_changed();
            }
            ui.close();
        }
        if ui
            .add_enabled(!state.selection.is_empty(), egui::Button::new("Deselect All"))
            .clicked()
        {
            state.selection.clear();
            output.ui_changed();
            ui.close();
        }

        ui.separator();
        let has_selection = !state.selection.is_empty();
        if ui
            .add_enabled(has_selection, egui::Button::new("Delete"))
            .clicked()
        {
            emit_delete_selection(editor, state, &mut output, false);
            ui.close();
        }
        if ui
            .add_enabled(has_selection, egui::Button::new("Ripple Delete"))
            .clicked()
        {
            emit_delete_selection(editor, state, &mut output, true);
            ui.close();
        }

        ui.separator();
        if ui.button("Fit All").clicked() {
            fit_all(state, content_end_sec, area.right() - content_left);
            output.ui_changed();
            ui.close();
        }
        if ui.button("Mark In").clicked() {
            state.mark_in = Some(state.playhead);
            output.ui_changed();
            ui.close();
        }
        if ui.button("Mark Out").clicked() {
            state.mark_out = Some(state.playhead);
            output.ui_changed();
            ui.close();
        }

        ui.separator();
        // Name a B-ROLL point at the current PLAYHEAD time: the host compares the active clip against
        // itself at that moment. It names a TIME, not a clip, so it is always enabled (no selection
        // needed). Mirrors the Ctrl+left-click gesture (which names the clicked time instead).
        if ui
            .button("Roll B from this time")
            .on_hover_text("Compare the active clip against itself at the playhead time. Same as a Ctrl+left-click on the timeline.")
            .clicked()
        {
            emit_broll_at_playhead(state, &mut output);
            ui.close();
        }
    });

    // Keyboard: Delete removes the selection — plain = lift (leave a gap),
    // Shift+Delete = ripple (close the gap). Gated on hover so the timeline only
    // eats Delete when the pointer is over it, not from another focused widget.
    if resp.hovered() && !state.selection.is_empty() {
        let (del, shift) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                i.modifiers.shift,
            )
        });
        if del {
            emit_delete_selection(editor, state, &mut output, shift);
        }
    }

    // Live drag preview: a translucent "ghost" of the dragged clip at the exact snapped
    // target the drop will commit (plus a pointer guide line), so edits are predictable.
    if let (Some(drag), Some(p)) = (state.drag, press) {
        let raw_delta = to_sec(p.x) - state.grab_sec;
        let destination =
            move_destination_track(editor.document(), state, &tracks, area, p.y, track_h);
        let snap_track = destination.or(state.drag_track);
        let d = snap_drag_delta(editor, state, drag, raw_delta, snap_on, snap_track).to_seconds();
        // Ghost geometry is derived from the same operation semantics as the committed edit.
        // In this sequential OTIO composition, trim_in shortens duration while the item's
        // timeline start remains fixed, so its committed OUT edge moves by -delta.
        let (gs, ge, roll_edge) = drag_preview_span(state, drag, d);
        // The Move ghost follows the pointer's lane so a cross-track drop previews on the
        // destination track; the other gestures stay on the source lane.
        let row = match drag {
            Drag::Move { .. } => destination
                .and_then(|track| tracks.iter().position(|&x| x == track))
                .or_else(|| {
                    state
                        .drag_track
                        .and_then(|t| tracks.iter().position(|&x| x == t))
                }),
            _ => state
                .drag_track
                .and_then(|t| tracks.iter().position(|&x| x == t)),
        };
        if let Some(row) = row {
            let top = area.top() + RULER_H + row as f32 * (track_h + TRACK_GAP);
            let accent = Color32::from_rgb(255, 210, 90);
            if roll_edge {
                // A single moving edit point — draw it as a bright vertical band on the lane.
                let x = to_x(gs).max(content_left);
                painter.line_segment(
                    [Pos2::new(x, top), Pos2::new(x, top + track_h)],
                    Stroke::new(2.0, accent),
                );
            } else {
                let x0 = to_x(gs).max(content_left);
                let x1 = to_x(ge);
                let ghost = Rect::from_min_max(
                    Pos2::new(x0.min(x1), top + 3.0),
                    Pos2::new(x0.max(x1), top + track_h - 3.0),
                );
                painter.rect_filled(
                    ghost,
                    CornerRadius::same(3),
                    Color32::from_rgba_unmultiplied(255, 210, 90, 64),
                );
                painter.rect_stroke(
                    ghost,
                    CornerRadius::same(3),
                    Stroke::new(1.5, accent),
                    StrokeKind::Inside,
                );
            }
        }
        draw_drag_preview(&painter, area, content_left, p);
    }

    // Empty-space drag: in the top RULER strip it SCRUBS the playhead (like an NLE); below
    // it, in the track area, it rubber-bands a MARQUEE selection. A clip drag (hit is Some)
    // takes neither path.
    if resp.drag_started_by(egui::PointerButton::Primary)
        && hit.is_none()
        && state.tool != Tool::Blade
        && let Some(p) = press
    {
        // Modifier ranking on an empty-space (ruler / track background) drag, `hit.is_none()`:
        //   Ctrl  -> scrub the B-ROLL time point live (compare reference in time, NOT the transport:
        //            no ScrubStart/Stop, playback untouched). A Ctrl drag on a CLIP BODY is Slide
        //            instead — that path has `hit.is_some()` and is handled in `handle_input`, so it
        //            never reaches here.
        //   Shift -> rubber-band a marquee selection.
        //   plain -> scrub the PLAYHEAD anywhere (grab the time cursor over the ruler OR the track
        //            background and it follows). Previously only the thin ruler strip scrubbed and the
        //            track area marquee'd, so dragging the tall playhead line where it crossed a track
        //            did nothing (it started a marquee) and the cursor appeared to jam.
        if ctrl {
            state.broll_scrubbing = true;
            output.emit(TimelineCommand::SetBRoll {
                seconds: to_sec(p.x).max(0.0),
            });
        } else if ui.input(|i| i.modifiers.shift) {
            state.marquee = Some(p);
            output.ui_changed();
        } else {
            state.scrubbing = true;
            output.emit(TimelineCommand::ScrubStart);
        }
    }
    // While scrubbing, the playhead follows the pointer's time each frame. Update `state.playhead`
    // OPTIMISTICALLY (not only via the command) so the playhead tracks the cursor in the SAME frame
    // it is drawn, instead of lagging a host round-trip (seek -> read-back) behind. A scrubber that
    // owns its own playhead (e.g. egui-timeline) feels smooth for exactly this reason; without this
    // the embedded playhead appeared to "stall" under the cursor while the host caught up. The host
    // still writes the authoritative position back next frame; this only removes the visible lag.
    if state.scrubbing
        && let Some(p) = press
    {
        let seconds = to_sec(p.x).max(0.0);
        state.playhead = seconds;
        output.emit(TimelineCommand::SetPlayhead { seconds });
    }
    if resp.drag_stopped_by(egui::PointerButton::Primary) && state.scrubbing {
        state.scrubbing = false;
        output.emit(TimelineCommand::ScrubStop);
    }
    // While a Ctrl+drag on the time axis is live, the B-roll point follows the pointer each frame so
    // the host updates the compare reference in real time. Unlike the playhead scrub this emits no
    // ScrubStart/Stop and leaves `state.playhead` untouched — b-roll is a time reference, not transport.
    if state.broll_scrubbing
        && let Some(p) = press
    {
        output.emit(TimelineCommand::SetBRoll {
            seconds: to_sec(p.x).max(0.0),
        });
    }
    if resp.drag_stopped_by(egui::PointerButton::Primary) && state.broll_scrubbing {
        state.broll_scrubbing = false;
        output.ui_changed();
    }
    if let (Some(start), Some(p)) = (state.marquee, press) {
        let rect = Rect::from_two_pos(start, p);
        painter.rect_filled(
            rect,
            CornerRadius::ZERO,
            Color32::from_rgba_unmultiplied(120, 160, 220, 40),
        );
        painter.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, Color32::from_rgb(120, 160, 220)),
            StrokeKind::Inside,
        );
    }
    if resp.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(start) = state.marquee.take()
    {
        let end = resp
            .interact_pointer_pos()
            .or_else(|| resp.hover_pos())
            .unwrap_or(start);
        let rect = Rect::from_two_pos(start, end);
        if !ui.input(|i| i.modifiers.shift) {
            state.selection.clear();
        }
        for (id, r) in &clip_rects {
            if rect.intersects(*r) {
                state.selection.insert(*id);
            }
        }
        output.ui_changed();
    }

    // In/out marked range: a translucent band + edge guides (the record-side target of a
    // three-point edit), drawn under the playhead. A single set mark shows one guide.
    match (state.mark_in, state.mark_out) {
        (Some(a), Some(b)) => {
            let (lo, hi) = (a.min(b), a.max(b));
            let xa = to_x(lo).clamp(content_left, area.right());
            let xb = to_x(hi).clamp(content_left, area.right());
            if xb > xa {
                let band = Rect::from_min_max(
                    Pos2::new(xa, area.top() + RULER_H),
                    Pos2::new(xb, area.bottom()),
                );
                painter.rect_filled(
                    band,
                    CornerRadius::ZERO,
                    Color32::from_rgba_unmultiplied(90, 170, 255, 30),
                );
            }
            for m in [lo, hi] {
                let x = to_x(m);
                if x >= content_left && x <= area.right() {
                    painter.line_segment(
                        [
                            Pos2::new(x, area.top() + RULER_H),
                            Pos2::new(x, area.bottom()),
                        ],
                        Stroke::new(1.5, Color32::from_rgb(90, 170, 255)),
                    );
                }
            }
        }
        (m_in, m_out) => {
            if let Some(m) = m_in.or(m_out) {
                let x = to_x(m);
                if x >= content_left && x <= area.right() {
                    painter.line_segment(
                        [
                            Pos2::new(x, area.top() + RULER_H),
                            Pos2::new(x, area.bottom()),
                        ],
                        Stroke::new(1.5, Color32::from_rgb(90, 170, 255)),
                    );
                }
            }
        }
    }

    // Playhead on top.
    let px = to_x(state.playhead);
    if px >= content_left && px <= area.right() {
        painter.line_segment(
            [Pos2::new(px, area.top()), Pos2::new(px, area.bottom())],
            Stroke::new(1.5, Color32::from_rgb(255, 96, 64)),
        );
    }

    // MMB is a viewport-only grab gesture: dragging right reveals earlier time and dragging left
    // reveals later time. It never starts a clip edit, ruler scrub, or marquee selection.
    if middle_panning && state.drag.is_none() {
        let view_w = area.right() - content_left;
        state.scroll_x = pan_scroll(
            state.scroll_x,
            resp.drag_delta().x,
            content_end_sec,
            state.px_per_sec,
            view_w,
        );
    }

    // Zoom with Ctrl+wheel; scroll with wheel. Frozen while a PRIMARY edit drag is in progress:
    // the drag delta is `to_sec(release) - grab_sec`, with `grab_sec` captured in
    // the drag-start frame's px<->sec mapping. Re-mapping mid-drag (zoom/scroll
    // mutate px_per_sec/scroll_x) would make a stationary cursor read a different
    // timeline-second on release and commit a spurious slide/trim. The active
    // gesture owns the viewport.
    if resp.hovered() && state.drag.is_none() {
        // Keyboard zoom, scoped to hover so it never fights typing elsewhere: '-' zooms out, '='
        // (a.k.a. '+') zooms in, and 'f' FITS all clips to the view. Fit is the answer to "where are
        // the clips?" for a playlist of 1-frame stills — at the default 40 px/sec they are a
        // sub-pixel sliver; 'f' snaps the whole [0, content_end] span to the visible width.
        let (kzoom_out, kzoom_in, kfit) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Minus),
                i.key_pressed(egui::Key::Equals) || i.key_pressed(egui::Key::Plus),
                i.key_pressed(egui::Key::F),
            )
        });
        if kzoom_out {
            state.px_per_sec = (state.px_per_sec / 1.25).clamp(PX_PER_SEC_MIN, PX_PER_SEC_MAX);
        }
        if kzoom_in {
            state.px_per_sec = (state.px_per_sec * 1.25).clamp(PX_PER_SEC_MIN, PX_PER_SEC_MAX);
        }
        if kfit {
            fit_all(state, content_end_sec, area.right() - content_left);
        }
        // egui folds Ctrl+wheel (and trackpad pinch) into `zoom_delta()` — a multiplicative factor —
        // and REMOVES that motion from `smooth_scroll_delta`, so reading only the smoothed delta made
        // Ctrl+wheel a silent no-op. Drive the zoom from `zoom_delta()` directly; plain-wheel
        // horizontal scroll keeps the smoothed delta for its inertial feel.
        let (smooth, zoom_delta) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        let view_w = area.right() - content_left;
        if zoom_delta != 1.0 {
            let old_scale = state.px_per_sec;
            let new_scale = (old_scale * zoom_delta).clamp(PX_PER_SEC_MIN, PX_PER_SEC_MAX);
            let pointer_x = hover.map_or(view_w * 0.5, |p| p.x - content_left);
            state.scroll_x = zoom_scroll_about(
                state.scroll_x,
                old_scale,
                new_scale,
                pointer_x,
                content_end_sec,
                view_w,
            );
            state.px_per_sec = new_scale;
        } else if smooth != 0.0 {
            state.scroll_x = clamp_scroll(
                state.scroll_x - smooth,
                content_end_sec,
                state.px_per_sec,
                view_w,
            );
        }
    }

    if view_before
        != (
            state.px_per_sec,
            state.scroll_x,
            state.track_h,
            state.rate,
            state.mark_in,
            state.mark_out,
        )
    {
        output.ui_changed();
    }
    output
}

fn selectable_timeline_items(doc: &Document, tracks: &[NodeId]) -> HashSet<NodeId> {
    let mut selection = HashSet::new();
    for &track in tracks {
        let Some(track) = doc.node(track) else {
            continue;
        };
        for &item in track.children("children") {
            if is_clip(doc, item) || is_gap(doc, item) {
                selection.insert(item);
            }
        }
    }
    selection
}

fn emit_delete_selection(
    editor: &Editor,
    state: &mut TimelineState,
    output: &mut TimelineResponse,
    force_ripple: bool,
) {
    let items: Vec<NodeId> = state.selection.iter().copied().collect();
    for item in items {
        // A selected gap always ripples; deleting a clip lifts unless ripple was requested.
        output.emit(if force_ripple || is_gap(editor.document(), item) {
            TimelineCommand::RippleDelete { clip: item }
        } else {
            TimelineCommand::Lift { clip: item }
        });
    }
    state.selection.clear();
    output.ui_changed();
}

/// Emit a B-ROLL point at the current PLAYHEAD time — the "Roll B from this time" context-menu
/// action. Shared so a test can assert the emission without driving the popup (the same idiom
/// [`emit_delete_selection`] uses). Names a TIME, not a clip: the host compares the active clip
/// against itself at `state.playhead`.
fn emit_broll_at_playhead(state: &TimelineState, output: &mut TimelineResponse) {
    output.emit(TimelineCommand::SetBRoll {
        seconds: state.playhead.max(0.0),
    });
}

fn fit_all(state: &mut TimelineState, content_end_sec: f64, view_width: f32) {
    if content_end_sec <= 0.0 || !content_end_sec.is_finite() {
        return;
    }
    state.px_per_sec = ((view_width.max(1.0) * 0.92) / content_end_sec as f32)
        .clamp(PX_PER_SEC_MIN, PX_PER_SEC_MAX);
    state.scroll_x = 0.0;
}

fn max_scroll(content_end_sec: f64, px_per_sec: f32, view_width: f32) -> f32 {
    if !content_end_sec.is_finite() || !px_per_sec.is_finite() || !view_width.is_finite() {
        return 0.0;
    }
    ((content_end_sec.max(0.0) as f32 * px_per_sec.max(0.0)) - view_width.max(0.0)).max(0.0)
}

fn clamp_scroll(candidate: f32, content_end_sec: f64, px_per_sec: f32, view_width: f32) -> f32 {
    if !candidate.is_finite() {
        return 0.0;
    }
    candidate.clamp(0.0, max_scroll(content_end_sec, px_per_sec, view_width))
}

fn pan_scroll(
    scroll_x: f32,
    drag_delta_x: f32,
    content_end_sec: f64,
    px_per_sec: f32,
    view_width: f32,
) -> f32 {
    clamp_scroll(
        scroll_x - drag_delta_x,
        content_end_sec,
        px_per_sec,
        view_width,
    )
}

fn zoom_scroll_about(
    scroll_x: f32,
    old_px_per_sec: f32,
    new_px_per_sec: f32,
    pointer_x: f32,
    content_end_sec: f64,
    view_width: f32,
) -> f32 {
    if old_px_per_sec <= 0.0 || !old_px_per_sec.is_finite() || !new_px_per_sec.is_finite() {
        return 0.0;
    }
    let pointer_x = pointer_x.clamp(0.0, view_width.max(0.0));
    let anchor_sec = (scroll_x + pointer_x) / old_px_per_sec;
    clamp_scroll(
        anchor_sec * new_px_per_sec - pointer_x,
        content_end_sec,
        new_px_per_sec,
        view_width,
    )
}

/// Which part of a clip the pointer is over.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EdgeZone {
    Left,
    Body,
    Right,
}

/// The clip's `(track, index)` if it lives on a track.
fn track_index(doc: &Document, clip: NodeId) -> Option<(NodeId, usize)> {
    let track = doc.node(clip)?.parent()?;
    let idx = doc
        .node(track)?
        .children("children")
        .iter()
        .position(|&c| c == clip)?;
    Some((track, idx))
}

/// Whether `id` canonicalizes to a `Clip` (not a Gap / Transition).
fn is_clip(doc: &Document, id: NodeId) -> bool {
    doc.node(id)
        .map(|n| canonical_schema(n.schema_name()) == "Clip")
        .unwrap_or(false)
}

/// Whether `id` canonicalizes to a `Gap` (empty space between clips).
fn is_gap(doc: &Document, id: NodeId) -> bool {
    doc.node(id)
        .map(|n| canonical_schema(n.schema_name()) == "Gap")
        .unwrap_or(false)
}

/// The clip immediately before `clip` on its track, iff it is also a clip (a
/// shared edit point → roll candidate); else `None` (track start, or a gap).
fn prev_clip(doc: &Document, clip: NodeId) -> Option<NodeId> {
    let (track, idx) = track_index(doc, clip)?;
    let prev = *doc
        .node(track)?
        .children("children")
        .get(idx.checked_sub(1)?)?;
    is_clip(doc, prev).then_some(prev)
}

/// Whether the sibling immediately after `clip` is also a clip (roll candidate).
fn next_is_clip(doc: &Document, clip: NodeId) -> bool {
    let Some((track, idx)) = track_index(doc, clip) else {
        return false;
    };
    doc.node(track)
        .and_then(|n| n.children("children").get(idx + 1).copied())
        .map(|n| is_clip(doc, n))
        .unwrap_or(false)
}

/// The track-lane index under a Y coordinate (the rows below the ruler), or `None` if
/// above the first lane. Used to hit-test the destination lane of a cross-track drag.
fn lane_at(area: Rect, y: f32, track_h: f32) -> Option<usize> {
    let i = ((y - area.top() - RULER_H) / (track_h + TRACK_GAP)).floor();
    (i >= 0.0).then_some(i as usize)
}

// Cohesive per-frame interaction context (response, model, hit, view transform, modifiers).
// Each param is genuinely needed and bundling into a struct (generic over the `to_sec`
// closure) adds ceremony without clarity, so allow the arg count on this private helper.
#[allow(clippy::too_many_arguments)]
fn handle_input(
    resp: &egui::Response,
    editor: &Editor,
    state: &mut TimelineState,
    output: &mut TimelineResponse,
    hit: &Option<(NodeId, NodeId, EdgeZone)>,
    area: Rect,
    to_sec: impl Fn(f32) -> f64,
    snap_on: bool,
    ctrl: bool,
    track_h: f32,
) {
    if state.tool == Tool::Blade
        && resp.clicked_by(egui::PointerButton::Primary)
        && let (Some((_, track, _)), Some(p)) = (*hit, resp.interact_pointer_pos())
    {
        output.emit(TimelineCommand::Blade {
            track,
            time: rt(to_sec(p.x), state.rate),
        });
        return;
    }

    if resp.drag_started_by(egui::PointerButton::Primary)
        && let (Some((clip, track, zone)), Some(p)) = (*hit, resp.interact_pointer_pos())
    {
        let drag = match state.tool {
            Tool::Edit => match zone {
                EdgeZone::Body => Some(if ctrl {
                    Drag::Slide { clip }
                } else {
                    Drag::Move { clip }
                }),
                EdgeZone::Left => Some(match (ctrl, prev_clip(editor.document(), clip)) {
                    (true, Some(left)) => Drag::Roll {
                        left,
                        edge_is_end: false,
                    },
                    _ => Drag::TrimIn { clip },
                }),
                EdgeZone::Right => Some(if ctrl && next_is_clip(editor.document(), clip) {
                    Drag::Roll {
                        left: clip,
                        edge_is_end: true,
                    }
                } else {
                    Drag::TrimOut { clip }
                }),
            },
            Tool::Blade => None,
        };
        if let Some(drag) = drag {
            state.grab_sec = to_sec(p.x);
            let range = state
                .layout
                .track_layout(editor, track)
                .iter()
                .find(|(candidate, _)| *candidate == clip)
                .map(|(_, range)| {
                    (
                        range.start_time().to_seconds(),
                        range.end_time_exclusive().to_seconds(),
                    )
                });
            state.drag_track = range.map(|_| track);
            if let Some((start, end)) = range {
                state.drag_start_sec = start;
                state.drag_end_sec = end;
            }
            state.drag = Some(drag);
            state.selection.clear();
            state.selection.insert(clip);
            output.ui_changed();
        }
    }

    // Always consume the pending drag on release. If egui cannot provide a release
    // position, report an explicit cancellation instead of silently losing the drop.
    if resp.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(drag) = state.drag.take()
    {
        let position = release_position_or_cancel(
            state,
            output,
            resp.interact_pointer_pos().or_else(|| resp.hover_pos()),
        );
        if let Some(p) = position {
            let raw_delta = to_sec(p.x) - state.grab_sec;
            let tracks = timeline_tracks(editor.document());
            let destination =
                move_destination_track(editor.document(), state, &tracks, area, p.y, track_h);
            let snap_track = destination.or(state.drag_track);
            let delta = snap_drag_delta(editor, state, drag, raw_delta, snap_on, snap_track);
            match drag {
                Drag::Move { clip } => {
                    if let Some(track) = destination {
                        output.emit(TimelineCommand::Move {
                            clip,
                            destination: MoveDestination::Track {
                                track,
                                start: rt(state.drag_start_sec + delta.to_seconds(), state.rate),
                            },
                        });
                    } else if delta.value() != 0.0 {
                        output.emit(TimelineCommand::Move {
                            clip,
                            destination: MoveDestination::SameTrack { delta },
                        });
                    }
                }
                Drag::Slide { clip } if delta.value() != 0.0 => {
                    output.emit(TimelineCommand::Slide { clip, delta });
                }
                Drag::TrimIn { clip } if delta.value() != 0.0 => {
                    output.emit(TimelineCommand::TrimIn { clip, delta });
                }
                Drag::TrimOut { clip } if delta.value() != 0.0 => {
                    output.emit(TimelineCommand::TrimOut { clip, delta });
                }
                Drag::Roll { left, .. } if delta.value() != 0.0 => {
                    output.emit(TimelineCommand::Roll { left, delta });
                }
                _ => output.ui_changed(),
            }
        }
        state.drag_track = None;
    }

    if resp.clicked_by(egui::PointerButton::Primary)
        && state.tool != Tool::Blade
        && let Some((clip, _, _)) = *hit
    {
        state.selection.clear();
        state.selection.insert(clip);
        output.ui_changed();
    }

    // A plain empty-space click moves the playhead. Under Ctrl the click names a B-ROLL point instead
    // (emitted in the main body), so it must NOT also seek the playhead — gate this on `!ctrl`.
    if resp.clicked_by(egui::PointerButton::Primary)
        && !ctrl
        && hit.is_none()
        && let Some(p) = resp.interact_pointer_pos()
    {
        output.emit(TimelineCommand::SetPlayhead {
            seconds: to_sec(p.x).max(0.0),
        });
    }
}

fn release_position_or_cancel(
    state: &mut TimelineState,
    output: &mut TimelineResponse,
    position: Option<Pos2>,
) -> Option<Pos2> {
    if position.is_none() {
        state.drag_track = None;
        output.emit(TimelineCommand::Cancel {
            reason: TimelineCancelReason::PointerUnavailable,
        });
    }
    position
}

fn move_destination_track(
    doc: &Document,
    state: &TimelineState,
    tracks: &[NodeId],
    area: Rect,
    y: f32,
    track_h: f32,
) -> Option<NodeId> {
    let destination = lane_at(area, y, track_h).and_then(|index| tracks.get(index).copied());
    match (destination, state.drag_track) {
        (Some(destination), Some(source))
            if destination != source
                && str_field(doc, destination, "kind") == str_field(doc, source, "kind") =>
        {
            Some(destination)
        }
        _ => None,
    }
}

/// Geometry the current command will commit. Record-side `TrimIn` moves the left edge by
/// `delta` while preserving the clip's record end; its preceding Gap absorbs the inverse
/// duration change.
fn drag_preview_span(state: &TimelineState, drag: Drag, delta: f64) -> (f64, f64, bool) {
    match drag {
        Drag::Move { .. } | Drag::Slide { .. } => (
            state.drag_start_sec + delta,
            state.drag_end_sec + delta,
            false,
        ),
        Drag::TrimIn { .. } => (state.drag_start_sec + delta, state.drag_end_sec, false),
        Drag::TrimOut { .. } => (state.drag_start_sec, state.drag_end_sec + delta, false),
        Drag::Roll { edge_is_end, .. } => {
            let edge = if edge_is_end {
                state.drag_end_sec
            } else {
                state.drag_start_sec
            } + delta;
            (edge, edge, true)
        }
    }
}

/// Frame/snap quantized operation delta. Cross-track moves pass the destination track,
/// so snapping uses destination edges rather than the source lane. Record-side TrimIn
/// snaps the dragged left edge directly.
fn snap_drag_delta(
    editor: &Editor,
    state: &TimelineState,
    drag: Drag,
    raw_delta: f64,
    snap_on: bool,
    snap_track: Option<NodeId>,
) -> RationalTime {
    if !snap_on {
        return rt(raw_delta, state.rate);
    }
    let Some(track) = snap_track else {
        return rt(raw_delta, state.rate);
    };
    let (edge, direction) = match drag {
        Drag::TrimOut { .. }
        | Drag::Roll {
            edge_is_end: true, ..
        } => (state.drag_end_sec, 1.0),
        _ => (state.drag_start_sec, 1.0),
    };
    let mut targets = snap::snap_targets(editor.document(), track);
    targets.push(rt(state.playhead, state.rate));
    let threshold = rt((SNAP_PX / state.px_per_sec) as f64, state.rate);
    let candidate = rt(edge + direction * raw_delta, state.rate);
    let snapped = snap::snap(&targets, candidate, threshold);
    rt((snapped.to_seconds() - edge) / direction, state.rate)
}

// --- drawing helpers --------------------------------------------------------

/// The timeline's `global_start_time` in seconds (0 if absent / not a `Timeline` root).
/// The ruler adds this so timecode reads from the deck start, not from zero.
fn timeline_global_start(doc: &Document) -> f64 {
    let Some(root) = doc.root_id().and_then(|r| doc.node(r)) else {
        return 0.0;
    };
    if canonical_schema(root.schema_name()) != "Timeline" {
        return 0.0;
    }
    root.field("global_start_time")
        .and_then(|f| match f {
            Field::Value(v) => codec::rational_time(v),
            _ => None,
        })
        .map(|t| t.to_seconds())
        .unwrap_or(0.0)
}

// The ruler needs the full view transform (area, content_left, px/sec, scroll, rate, start
// offset) plus the shared `to_x`; all are needed and cohesive, so allow the arg count.
#[allow(clippy::too_many_arguments)]
fn draw_ruler(
    painter: &egui::Painter,
    area: Rect,
    content_left: f32,
    px_per_sec: f32,
    scroll_x: f32,
    rate: f64,
    start_off: f64,
    to_x: impl Fn(f64) -> f32,
) {
    let ruler = Rect::from_min_size(
        Pos2::new(area.left(), area.top()),
        Vec2::new(area.width(), RULER_H),
    );
    painter.rect_filled(ruler, CornerRadius::ZERO, Color32::from_gray(40));

    // Tick every "nice" number of seconds based on zoom. `px_per_sec`/`scroll_x`
    // are the caller's captured copies — the same values `to_x` uses below.
    let target_px = 80.0_f32;
    let secs_per_tick = nice_step((target_px / px_per_sec) as f64);
    let first = (scroll_x as f64 / px_per_sec as f64 / secs_per_tick).floor() * secs_per_tick;
    let mut t = first.max(0.0);
    while to_x(t) < area.right() {
        let x = to_x(t);
        if x >= content_left {
            painter.line_segment(
                [Pos2::new(x, area.top()), Pos2::new(x, area.top() + RULER_H)],
                Stroke::new(1.0, Color32::from_gray(80)),
            );
            painter.text(
                Pos2::new(x + 3.0, area.top() + RULER_H * 0.5),
                Align2::LEFT_CENTER,
                fmt_tc(t + start_off, rate),
                FontId::monospace(10.0),
                Color32::from_gray(180),
            );
        }
        t += secs_per_tick;
    }
}

fn draw_track_header(
    painter: &egui::Painter,
    doc: &Document,
    track: NodeId,
    left: f32,
    top: f32,
    track_h: f32,
) {
    let r = Rect::from_min_size(Pos2::new(left, top), Vec2::new(HEADER_W, track_h));
    painter.rect_filled(r, CornerRadius::ZERO, Color32::from_gray(44));
    let kind = str_field(doc, track, "kind");
    let name = str_field(doc, track, "name");
    let label = if name.is_empty() {
        kind.to_string()
    } else {
        format!("{name}  ({kind})")
    };
    painter.text(
        Pos2::new(left + 8.0, top + track_h * 0.5),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.0),
        Color32::from_gray(210),
    );
}

fn draw_clip(
    painter: &egui::Painter,
    doc: &Document,
    clip: NodeId,
    r: Rect,
    selected: bool,
    thumb: Option<egui::TextureId>,
) {
    let schema = doc
        .node(clip)
        .map(|n| canonical_schema(n.schema_name()))
        .unwrap_or("");
    let (fill, label) = match schema {
        "Gap" => (Color32::from_gray(38), String::new()),
        "Transition" => (Color32::from_rgb(150, 96, 40), ph::TRANSITION.to_owned()),
        _ => (
            Color32::from_rgb(54, 96, 150),
            str_field(doc, clip, "name").to_string(),
        ),
    };
    painter.rect_filled(r, CornerRadius::same(3), fill);
    // A host-supplied thumbnail (e.g. a decoded poster frame) fills the clip body,
    // painted over the flat fill but under the title strip / border / label below.
    if schema != "Gap"
        && let Some(tex) = thumb
    {
        let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
        painter.image(tex, r, uv, Color32::WHITE);
    }
    // Top accent bar (the DaVinci/AE clip "title" strip).
    if schema != "Gap" {
        let bar = Rect::from_min_max(r.min, Pos2::new(r.max.x, r.min.y + 4.0));
        painter.rect_filled(bar, CornerRadius::same(3), fill.gamma_multiply(1.6));
    }
    let stroke = if selected {
        Stroke::new(2.0, Color32::from_rgb(255, 210, 90))
    } else {
        Stroke::new(1.0, Color32::from_black_alpha(120))
    };
    painter.rect_stroke(r, CornerRadius::same(3), stroke, StrokeKind::Inside);
    if !label.is_empty() && r.width() > 24.0 {
        painter.text(
            Pos2::new(r.left() + 5.0, r.center().y),
            Align2::LEFT_CENTER,
            elide(&label, r.width()),
            FontId::proportional(12.0),
            Color32::from_gray(235),
        );
    }
}

/// A translucent vertical guide at the pointer, drawn alongside the live clip ghost
/// (see the drag-preview block in `timeline_ui`) as a precise time reference for the
/// drop. Op-agnostic and cheap; the ghost rect itself is computed in `timeline_ui`
/// where the clip's captured span + track y are in scope.
fn draw_drag_preview(painter: &egui::Painter, area: Rect, content_left: f32, p: Pos2) {
    let x = p.x.max(content_left);
    painter.line_segment(
        [Pos2::new(x, area.top()), Pos2::new(x, area.bottom())],
        Stroke::new(1.0, Color32::from_white_alpha(120)),
    );
}

// --- model navigation -------------------------------------------------------

/// The track node ids of the timeline (root `Timeline` → `tracks` Stack →
/// children), or the children of a bare `Stack` root. Empty otherwise.
fn timeline_tracks(doc: &Document) -> Vec<NodeId> {
    let Some(root) = doc.root_id() else {
        return Vec::new();
    };
    let Some(node) = doc.node(root) else {
        return Vec::new();
    };
    let stack = match canonical_schema(node.schema_name()) {
        "Timeline" => node.child("tracks"),
        "Stack" => Some(root),
        _ => None,
    };
    stack
        .and_then(|s| doc.node(s))
        .map(|s| s.children("children").to_vec())
        .unwrap_or_default()
}

/// Read a value-field as `&str` (empty if absent / not a string).
fn str_field<'d>(doc: &'d Document, id: NodeId, key: &str) -> &'d str {
    doc.node(id)
        .and_then(|n| n.field(key))
        .and_then(|f| match f {
            Field::Value(v) => v.as_str(),
            _ => None,
        })
        .unwrap_or("")
}

// --- small utilities --------------------------------------------------------

/// Seconds → a frame-snapped `RationalTime` at `rate`.
fn rt(sec: f64, rate: f64) -> RationalTime {
    RationalTime::new((sec * rate).round(), rate)
}

/// Record an op result: clear or set the status message (a rejected edit — e.g.
/// past media bounds — is shown, not crashed). On success, reconcile the
/// selection against the (possibly changed) document so ids removed by the edit
/// — e.g. a razor that replaces a clip — don't linger as stale handles and
/// mis-target the next gesture.
/// Whether a pointer gesture is currently in progress: a clip drag/trim, a
/// marquee, or a playhead scrub.
///
/// A host needs this to decide when it is safe to REPLACE the projection it
/// feeds the widget. Swapping the [`Editor`] mid-gesture bumps the projection
/// epoch, which cancels the clip-tied interaction (see
/// `reset_replaced_projection`) — so a rebuild triggered while the user is
/// dragging silently eats the edit. Hosts should defer the rebuild until this
/// returns `false`.
pub fn gesture_active(state: &TimelineState) -> bool {
    state.drag.is_some() || state.marquee.is_some() || state.scrubbing || state.broll_scrubbing
}

/// Clear the in/out marks.
pub fn clear_marks(state: &mut TimelineState) {
    state.mark_in = None;
    state.mark_out = None;
}

/// Ripple-delete (`ripple = true`) or lift every CLIP fully inside the marked
/// `[mark_in, mark_out)` range, across all tracks — the record-side action of a
/// three-point edit (the marks define the target span). No-op unless both marks are set
/// and `in < out`; the marks themselves are left in place. Gaps are ignored.
pub fn clear_marked_range(editor: &mut Editor, state: &mut TimelineState, ripple: bool) {
    let (Some(a), Some(b)) = (state.mark_in, state.mark_out) else {
        return;
    };
    let (lo, hi) = (a.min(b), a.max(b));
    if hi - lo <= 1e-9 {
        return;
    }
    // Collect fully-contained clips first (the ops mutate the tree; each is clip-addressed
    // so it re-resolves its own index as siblings shift).
    let eps = 1e-6;
    let mut victims: Vec<NodeId> = Vec::new();
    for track in timeline_tracks(editor.document()) {
        for (clip, range) in state.layout.track_layout(editor, track).iter() {
            let s = range.start_time().to_seconds();
            let e = s + range.duration().to_seconds();
            if s >= lo - eps && e <= hi + eps && !is_gap(editor.document(), *clip) {
                victims.push(*clip);
            }
        }
    }
    for clip in victims {
        let res = if ripple {
            ops::ripple_delete_clip(editor, clip)
        } else {
            ops::lift_clip(editor, clip)
        };
        report(state, editor, res);
    }
}

fn report(state: &mut TimelineState, editor: &Editor, res: Result<(), otio_edit::EditError>) {
    if res.is_ok() {
        let doc = editor.document();
        state.selection.retain(|id| doc.node(*id).is_some());
    }
    state.last_error = res.err().map(|e| e.to_string());
}

fn reset_replaced_projection(
    editor: &Editor,
    state: &mut TimelineState,
    output: &mut TimelineResponse,
) {
    // Editor revisions are allocated in disjoint 2^20-wide bands by otio-edit.
    let epoch = editor.revision() >> 20;
    match state.projection_epoch {
        None => state.projection_epoch = Some(epoch),
        Some(previous) if previous != epoch => {
            // A CLIP interaction (a clip drag or a marquee) cannot survive a structural rebuild — the
            // clip it targeted may no longer exist — so it is cancelled. A transport SCRUB is
            // different: it moves the playhead in TIME, independent of the clip layout, so it MUST
            // survive a rebuild (e.g. a playlist reconciling its media mid-scrub bumps the epoch).
            // Clearing it here is exactly why the time cursor detached from the pointer the instant a
            // cut opened. Preserve `scrubbing` / `scrub_was_playing`; clear only the clip-tied state.
            let interaction_active = state.drag.is_some() || state.marquee.is_some();
            state.layout.clear();
            state.selection.clear();
            state.drag = None;
            state.drag_track = None;
            state.marquee = None;
            state.last_error = None;
            state.projection_epoch = Some(epoch);
            output.ui_changed();
            if interaction_active {
                output.emit(TimelineCommand::Cancel {
                    reason: TimelineCancelReason::ProjectionReplaced,
                });
            }
        }
        Some(_) => {}
    }
}

fn advance_legacy_playback(ui: &egui::Ui, editor: &Editor, state: &mut TimelineState) {
    if !state.playing {
        return;
    }
    let delta = ui.input(|input| input.stable_dt).min(0.1) as f64;
    state.playhead += delta;
    let content_end = timeline_tracks(editor.document())
        .into_iter()
        .filter_map(|track| otio_algo::range_of_all_children(editor.document(), track).ok())
        .flat_map(|ranges| ranges.into_iter())
        .map(|(_, range)| range.end_time_exclusive().to_seconds())
        .fold(0.0_f64, f64::max);
    let (range_lo, range_hi, looping) = match (state.mark_in, state.mark_out) {
        (Some(a), Some(b)) if (a - b).abs() > 1e-6 => (a.min(b), a.max(b), true),
        _ => (0.0, content_end, false),
    };
    if state.playhead >= range_hi {
        if looping {
            state.playhead = range_lo;
        } else {
            state.playhead = range_hi;
            state.playing = false;
        }
    }
    ui.ctx().request_repaint();
}

/// A "nice" tick step (1/2/5 × 10ⁿ) ≥ `raw` seconds.
fn nice_step(raw: f64) -> f64 {
    if raw <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let n = raw / mag;
    (if n <= 1.0 {
        1.0
    } else if n <= 2.0 {
        2.0
    } else if n <= 5.0 {
        5.0
    } else {
        10.0
    }) * mag
}

/// Format seconds as SMPTE-style timecode `HH:MM:SS:FF` at `rate` frames/sec.
fn fmt_tc(sec: f64, rate: f64) -> String {
    let fps = rate.round().max(1.0) as u64;
    let total = (sec.max(0.0) * rate).round() as u64;
    let (frames, s) = (total % fps, total / fps);
    let (h, m, sx) = (s / 3600, (s % 3600) / 60, s % 60);
    format!("{h:02}:{m:02}:{sx:02}:{frames:02}")
}

/// Truncate a label to roughly fit `width` px (≈7 px/char), adding an ellipsis.
fn elide(s: &str, width: f32) -> String {
    let max = ((width - 8.0) / 7.0).max(1.0) as usize;
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// A ready-made two-track sample timeline (V1 with clips + a gap, A1 with clips),
/// for demos and tests. Clips carry an `ExternalReference` with a generous
/// `available_range` so trim/slide have real media headroom. Returns an
/// [`Editor`] wrapping it, ready to pass to [`timeline_ui`].
pub fn sample_timeline() -> Editor {
    fn tr(start: f64, dur: f64) -> TimeRange {
        TimeRange::new(RationalTime::new(start, 24.0), RationalTime::new(dur, 24.0))
    }
    fn add_clip(doc: &mut Document, track: NodeId, name: &str, src_start: f64, dur: f64) {
        let mr = build::external_reference(doc, &format!("file:///media/{name}.mov"));
        if let Some(n) = doc.node_mut(mr) {
            n.set_value("available_range", codec::time_range_value(tr(0.0, 600.0)));
        }
        let clip = build::clip(doc, name, Some(tr(src_start, dur)), mr);
        let _ = doc.append_child(track, clip);
    }

    let mut doc = Document::new();
    let (tl, stack) = build::timeline(&mut doc, "Demo");
    doc.set_root(tl);

    let v = build::track(&mut doc, "V1", "Video");
    let _ = doc.append_child(stack, v);
    add_clip(&mut doc, v, "intro", 0.0, 48.0);
    add_clip(&mut doc, v, "shotA", 12.0, 72.0);
    let g = build::gap(&mut doc, tr(0.0, 18.0));
    let _ = doc.append_child(v, g);
    add_clip(&mut doc, v, "shotB", 30.0, 60.0);

    // A SECOND video track so cross-track move has a same-kind destination (V1<->V2);
    // the kind-guard still blocks video<->audio, so V2 is what makes the feature usable.
    let v2 = build::track(&mut doc, "V2", "Video");
    let _ = doc.append_child(stack, v2);
    add_clip(&mut doc, v2, "titleA", 0.0, 36.0);
    let g2 = build::gap(&mut doc, tr(0.0, 30.0));
    let _ = doc.append_child(v2, g2);
    add_clip(&mut doc, v2, "bgFill", 0.0, 90.0);

    let a = build::track(&mut doc, "A1", "Audio");
    let _ = doc.append_child(stack, a);
    add_clip(&mut doc, a, "music", 0.0, 120.0);
    add_clip(&mut doc, a, "vo", 0.0, 90.0);

    Editor::new(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Key, Modifiers, RawInput};

    fn response(editor: &Editor, command: TimelineCommand) -> TimelineResponse {
        TimelineResponse {
            commands: vec![command],
            changed: true,
            repaint: true,
            ..TimelineResponse::for_editor(editor)
        }
    }

    #[test]
    fn middle_pan_moves_and_clamps_the_viewport() {
        assert_eq!(pan_scroll(500.0, 40.0, 100.0, 20.0, 800.0), 460.0);
        assert_eq!(pan_scroll(20.0, 40.0, 100.0, 20.0, 800.0), 0.0);
        assert_eq!(pan_scroll(1_500.0, -500.0, 100.0, 20.0, 800.0), 1_200.0);
    }

    #[test]
    fn cursor_centered_zoom_preserves_anchor_time_when_unclamped() {
        let old_scroll = 200.0;
        let old_scale = 20.0;
        let new_scale = 40.0;
        let pointer_x = 300.0;
        let anchor_before = (old_scroll + pointer_x) / old_scale;
        let new_scroll =
            zoom_scroll_about(old_scroll, old_scale, new_scale, pointer_x, 100.0, 800.0);
        let anchor_after = (new_scroll + pointer_x) / new_scale;
        assert!((anchor_before - anchor_after).abs() < f32::EPSILON);
    }

    #[test]
    fn context_select_all_contains_only_editable_items() {
        let editor = sample_timeline();
        let tracks = timeline_tracks(editor.document());
        let selection = selectable_timeline_items(editor.document(), &tracks);
        assert!(!selection.is_empty());
        assert!(
            selection
                .iter()
                .all(|&item| is_clip(editor.document(), item) || is_gap(editor.document(), item))
        );
    }

    #[test]
    fn context_delete_uses_the_same_lift_and_ripple_commands_as_keyboard() {
        let editor = sample_timeline();
        let track = timeline_tracks(editor.document())[0];
        let clip = first_clip(&editor, track);

        let mut state = TimelineState::default();
        state.selection.insert(clip);
        let mut lift = TimelineResponse::for_editor(&editor);
        emit_delete_selection(&editor, &mut state, &mut lift, false);
        assert!(state.selection.is_empty());
        assert!(matches!(
            lift.commands.as_slice(),
            [TimelineCommand::Lift { clip: selected }] if *selected == clip
        ));

        state.selection.insert(clip);
        let mut ripple = TimelineResponse::for_editor(&editor);
        emit_delete_selection(&editor, &mut state, &mut ripple, true);
        assert!(matches!(
            ripple.commands.as_slice(),
            [TimelineCommand::RippleDelete { clip: selected }] if *selected == clip
        ));
    }

    fn first_clip(editor: &Editor, track: NodeId) -> NodeId {
        editor.document().node(track).unwrap().children("children")[0]
    }

    fn assert_edit_applied(
        editor: &mut Editor,
        state: &mut TimelineState,
        command: TimelineCommand,
    ) {
        let revision = editor.revision();
        let expected = command.clone();
        let pending = response(editor, command);
        let result = apply_timeline_response(editor, state, pending);
        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors
        );
        assert!(result.changed && result.repaint);
        assert!(
            result.commands.is_empty(),
            "applied commands must be consumed"
        );
        assert_eq!(result.applied_commands, vec![expected]);
        assert!(result.rejected_commands.is_empty());
        assert!(
            editor.revision() > revision,
            "edit command did not mutate Editor"
        );
    }

    #[test]
    fn edit_commands_generate_undoable_editor_changes() {
        let frame = RationalTime::new(1.0, 24.0);

        let mut editor = sample_timeline();
        let tracks = timeline_tracks(editor.document());
        let clip = first_clip(&editor, tracks[0]);
        assert_edit_applied(
            &mut editor,
            &mut TimelineState::default(),
            TimelineCommand::Move {
                clip,
                destination: MoveDestination::SameTrack { delta: frame },
            },
        );
        assert!(editor.can_undo());

        let mut editor = sample_timeline();
        let tracks = timeline_tracks(editor.document());
        let clip = first_clip(&editor, tracks[0]);
        assert_edit_applied(
            &mut editor,
            &mut TimelineState::default(),
            TimelineCommand::Move {
                clip,
                destination: MoveDestination::Track {
                    track: tracks[1],
                    start: RationalTime::new(200.0, 24.0),
                },
            },
        );

        for kind in 0..4 {
            let mut editor = sample_timeline();
            let track = timeline_tracks(editor.document())[0];
            let clip = first_clip(&editor, track);
            let command = match kind {
                0 => TimelineCommand::TrimIn { clip, delta: frame },
                1 => TimelineCommand::TrimOut { clip, delta: frame },
                2 => TimelineCommand::Slide { clip, delta: frame },
                _ => TimelineCommand::Roll {
                    left: clip,
                    delta: frame,
                },
            };
            assert_edit_applied(&mut editor, &mut TimelineState::default(), command);
        }

        let mut editor = sample_timeline();
        let track = timeline_tracks(editor.document())[0];
        assert_edit_applied(
            &mut editor,
            &mut TimelineState::default(),
            TimelineCommand::Blade {
                track,
                time: RationalTime::new(24.0, 24.0),
            },
        );
    }

    #[test]
    fn transport_commands_apply_without_advancing_a_clock() {
        let mut editor = sample_timeline();
        let mut state = TimelineState::default();
        let commands = TimelineResponse {
            commands: vec![
                TimelineCommand::TogglePlay,
                TimelineCommand::ScrubStart,
                TimelineCommand::SetPlayhead { seconds: 3.25 },
                TimelineCommand::ScrubStop,
            ],
            ..TimelineResponse::for_editor(&editor)
        };
        let result = apply_timeline_response(&mut editor, &mut state, commands);
        assert!(result.errors.is_empty());
        assert!(state.playing, "scrub stop must restore prior playing state");
        assert_eq!(state.playhead, 3.25);
    }

    #[test]
    fn applying_response_consumes_commands_and_cannot_apply_twice() {
        let mut editor = sample_timeline();
        let track = timeline_tracks(editor.document())[0];
        let clip = first_clip(&editor, track);
        let mut state = TimelineState::default();
        let pending = response(
            &editor,
            TimelineCommand::TrimIn {
                clip,
                delta: RationalTime::new(1.0, 24.0),
            },
        );

        let applied = apply_timeline_response(&mut editor, &mut state, pending);
        assert!(applied.commands.is_empty());
        assert_eq!(applied.applied_commands.len(), 1);
        let revision = editor.revision();
        let range_after_first = otio_algo::trimmed_range(editor.document(), clip).unwrap();

        let applied_again = apply_timeline_response(&mut editor, &mut state, applied);
        assert!(applied_again.commands.is_empty());
        assert_eq!(editor.revision(), revision);
        assert_eq!(
            otio_algo::trimmed_range(editor.document(), clip).unwrap(),
            range_after_first
        );
    }

    #[test]
    fn stale_response_rejects_colliding_node_ids_before_any_command() {
        let editor_a = sample_timeline();
        let mut editor_b = sample_timeline();
        let track_a = timeline_tracks(editor_a.document())[0];
        let track_b = timeline_tracks(editor_b.document())[0];
        let clip_a = first_clip(&editor_a, track_a);
        let clip_b = first_clip(&editor_b, track_b);
        assert_eq!(
            clip_a, clip_b,
            "fixture must reproduce deterministic NodeId collision"
        );
        assert_ne!(editor_a.revision(), editor_b.revision());

        let before_revision = editor_b.revision();
        let before_range = otio_algo::trimmed_range(editor_b.document(), clip_b).unwrap();
        let mut pending = TimelineResponse::for_editor(&editor_a);
        pending.commands = vec![
            TimelineCommand::TogglePlay,
            TimelineCommand::TrimIn {
                clip: clip_a,
                delta: RationalTime::new(1.0, 24.0),
            },
        ];
        let mut state = TimelineState::default();

        let result = apply_timeline_response(&mut editor_b, &mut state, pending);
        assert!(result.commands.is_empty());
        assert_eq!(result.rejected_commands.len(), 2);
        assert!(result.cancelled && result.changed && result.repaint);
        assert!(matches!(
            result.applied_commands.as_slice(),
            [
                TimelineCommand::Cancel {
                    reason: TimelineCancelReason::StaleResponse
                },
                TimelineCommand::Error { .. }
            ]
        ));
        assert!(result.errors[0].starts_with("stale timeline response:"));
        assert!(
            !state.playing,
            "stale batches must be rejected before transport side effects"
        );
        assert_eq!(editor_b.revision(), before_revision);
        assert_eq!(
            otio_algo::trimmed_range(editor_b.document(), clip_b).unwrap(),
            before_range
        );
    }

    #[test]
    fn application_error_is_exposed_in_response_and_state() {
        let mut editor = sample_timeline();
        let track = timeline_tracks(editor.document())[0];
        let clip = first_clip(&editor, track);
        let mut state = TimelineState::default();
        let pending = response(
            &editor,
            TimelineCommand::TrimIn {
                clip,
                delta: RationalTime::new(-1.0, 24.0),
            },
        );
        let result = apply_timeline_response(&mut editor, &mut state, pending);
        assert_eq!(result.errors, vec!["edit exceeds media available range"]);
        assert_eq!(
            state.last_error.as_deref(),
            result.errors.first().map(String::as_str)
        );
        assert!(result.commands.is_empty());
        assert!(matches!(
            result.applied_commands.last(),
            Some(TimelineCommand::Error { message })
                if message == "edit exceeds media available range"
        ));
        assert!(matches!(
            result.rejected_commands.as_slice(),
            [TimelineCommand::TrimIn { clip: rejected, .. }] if *rejected == clip
        ));
        assert!(result.changed && result.repaint);
    }

    #[test]
    fn missing_release_position_reports_cancellation() {
        let editor = sample_timeline();
        let track = timeline_tracks(editor.document())[0];
        let mut state = TimelineState {
            drag_track: Some(track),
            ..TimelineState::default()
        };
        let mut output = TimelineResponse::for_editor(&editor);
        assert_eq!(
            release_position_or_cancel(&mut state, &mut output, None),
            None
        );
        assert_eq!(state.drag_track, None);
        assert!(output.cancelled && output.changed && output.repaint);
        assert_eq!(
            output.commands,
            vec![TimelineCommand::Cancel {
                reason: TimelineCancelReason::PointerUnavailable
            }]
        );
    }

    #[test]
    fn trim_in_preview_matches_committed_duration_semantics() {
        let mut editor = sample_timeline();
        let track = timeline_tracks(editor.document())[0];
        let clip = first_clip(&editor, track);
        let before = otio_algo::range_of_all_children(editor.document(), track).unwrap()[0].1;
        let mut state = TimelineState {
            drag_start_sec: before.start_time().to_seconds(),
            drag_end_sec: before.end_time_exclusive().to_seconds(),
            ..TimelineState::default()
        };
        let delta = 0.25;
        let rate = state.rate;
        let (preview_start, preview_end, edge_only) =
            drag_preview_span(&state, Drag::TrimIn { clip }, delta);

        let pending = response(
            &editor,
            TimelineCommand::TrimIn {
                clip,
                delta: rt(delta, rate),
            },
        );
        let result = apply_timeline_response(&mut editor, &mut state, pending);
        assert!(result.errors.is_empty());
        let committed = otio_algo::range_of_all_children(editor.document(), track)
            .unwrap()
            .into_iter()
            .find(|(id, _)| *id == clip)
            .unwrap()
            .1;
        assert!(!edge_only);
        assert!((preview_start - committed.start_time().to_seconds()).abs() < 1e-9);
        assert!((preview_end - committed.end_time_exclusive().to_seconds()).abs() < 1e-9);
    }

    #[test]
    fn record_trim_in_preview_and_snap_follow_left_edge() {
        let editor = sample_timeline();
        let track = timeline_tracks(editor.document())[0];
        let clip = first_clip(&editor, track);
        let before = otio_algo::range_of_all_children(editor.document(), track).unwrap()[0].1;
        let state = TimelineState {
            px_per_sec: 100.0,
            playhead: 10.0,
            drag_start_sec: before.start_time().to_seconds(),
            drag_end_sec: before.end_time_exclusive().to_seconds(),
            ..TimelineState::default()
        };

        let (start, end, edge_only) = drag_preview_span(&state, Drag::TrimIn { clip }, 0.25);
        assert!((start - (state.drag_start_sec + 0.25)).abs() < 1e-9);
        assert_eq!(end, state.drag_end_sec);
        assert!(!edge_only);

        // V1's first clip ends at 2 s. A left edge dragged to 1.96 s snaps there;
        // using the old right-edge/-delta rule would produce a different result.
        let delta = snap_drag_delta(
            &editor,
            &state,
            Drag::TrimIn { clip },
            1.96,
            true,
            Some(track),
        );
        assert!((delta.to_seconds() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn cross_track_move_snaps_against_destination_edges() {
        let editor = sample_timeline();
        let tracks = timeline_tracks(editor.document());
        let clip = first_clip(&editor, tracks[0]);
        let mut state = TimelineState {
            px_per_sec: 100.0,
            playhead: 10.0,
            drag_track: Some(tracks[0]),
            drag_start_sec: 0.0,
            drag_end_sec: 2.0,
            ..TimelineState::default()
        };
        // Destination V2 has an edge at 1.5 s; source V1 does not.
        let delta = snap_drag_delta(
            &editor,
            &state,
            Drag::Move { clip },
            1.46,
            true,
            Some(tracks[1]),
        );
        assert!((delta.to_seconds() - 1.5).abs() < 1e-9);

        // Keep state mutable in this test to pin that snapping does not mutate UI state.
        state.playhead = 10.0;
        assert_eq!(state.drag_track, Some(tracks[0]));
    }

    #[test]
    fn replacing_projection_clears_transient_state_and_cancels_drag() {
        let editor_a = sample_timeline();
        let editor_b = sample_timeline();
        let track = timeline_tracks(editor_a.document())[0];
        let clip = first_clip(&editor_a, track);
        let mut state = TimelineState::default();
        let mut first = TimelineResponse::for_editor(&editor_a);
        reset_replaced_projection(&editor_a, &mut state, &mut first);
        state.selection.insert(clip);
        state.drag = Some(Drag::Move { clip });
        state.drag_track = Some(track);

        let mut output = TimelineResponse::for_editor(&editor_b);
        reset_replaced_projection(&editor_b, &mut state, &mut output);
        assert!(state.selection.is_empty());
        assert!(state.drag.is_none() && state.drag_track.is_none());
        assert!(output.cancelled && output.repaint && output.changed);
        assert!(matches!(
            output.commands.as_slice(),
            [TimelineCommand::Cancel {
                reason: TimelineCancelReason::ProjectionReplaced
            }]
        ));
    }

    fn run_command_frame(
        context: &egui::Context,
        editor: &Editor,
        state: &mut TimelineState,
        events: Vec<Event>,
    ) -> TimelineResponse {
        run_command_frame_mods(context, editor, state, events, Modifiers::NONE)
    }

    // Like `run_command_frame`, but holds `modifiers` for the frame. The widget reads the held
    // modifier state (`i.modifiers.ctrl`) — NOT the per-event modifiers — so a Ctrl gesture must
    // establish it here or the frame reads as unmodified.
    //
    // egui 0.36 removed `RawInput.modifiers`. Held modifiers now live on `InputState`, carry over
    // between frames, and change ONLY through `Event::ModifiersChanged`. Prepending that event is
    // the direct replacement: it sets the state before any of the caller's events are processed.
    fn run_command_frame_mods(
        context: &egui::Context,
        editor: &Editor,
        state: &mut TimelineState,
        events: Vec<Event>,
        modifiers: Modifiers,
    ) -> TimelineResponse {
        let mut events = events;
        events.insert(0, Event::ModifiersChanged(modifiers));
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 500.0))),
            events,
            ..RawInput::default()
        };
        let mut response = None;
        // egui 0.36 made `TexturesDelta` panic on drop when deltas are unapplied ("Deltas need to
        // be handled"). A headless test has no painter to apply them to, so discard them the way
        // the panic message prescribes rather than leaking the output.
        let mut output = context.run_ui(input, |ui| {
            response = Some(timeline_ui_commands(ui, editor, state));
        });
        output.textures_delta.clear();
        response.unwrap()
    }

    #[test]
    fn raw_input_drag_emits_move_command_on_release() {
        let context = egui::Context::default();
        let editor = sample_timeline();
        let mut state = TimelineState::default();
        let press = Pos2::new(130.0, 55.0);
        let _ = run_command_frame(
            &context,
            &editor,
            &mut state,
            vec![Event::PointerMoved(press)],
        );
        let _ = run_command_frame(
            &context,
            &editor,
            &mut state,
            vec![Event::PointerButton {
                pos: press,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            }],
        );
        let _ = run_command_frame(
            &context,
            &editor,
            &mut state,
            vec![Event::PointerMoved(Pos2::new(145.0, 55.0))],
        );
        let _ = run_command_frame(
            &context,
            &editor,
            &mut state,
            vec![Event::PointerMoved(Pos2::new(165.0, 55.0))],
        );
        let result = run_command_frame(
            &context,
            &editor,
            &mut state,
            vec![Event::PointerButton {
                pos: Pos2::new(165.0, 55.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert!(result.changed && result.repaint);
        assert!(matches!(
            result.commands.as_slice(),
            [TimelineCommand::Move {
                destination: MoveDestination::SameTrack { delta },
                ..
            }] if delta.value() != 0.0
        ));
    }

    #[test]
    fn set_broll_is_not_an_edit_and_applies_as_terminal_noop() {
        // A B-roll point is a host signal, not a document edit — so it never requires a matching
        // editor revision and never mutates the model or the playhead.
        assert!(!is_edit_command(&TimelineCommand::SetBRoll {
            seconds: 1.0
        }));

        let mut editor = sample_timeline();
        let mut state = TimelineState::default();
        let mut pending = TimelineResponse::for_editor(&editor);
        // Force a stale base revision: an EDIT batch would be rejected wholesale here; a SetBRoll-only
        // batch must still apply because the stale gate only fires when an edit command is present.
        pending.base_revision = editor.revision().wrapping_add(999);
        pending.commands = vec![TimelineCommand::SetBRoll { seconds: 2.5 }];
        let result = apply_timeline_response(&mut editor, &mut state, pending);
        assert!(result.errors.is_empty() && !result.cancelled);
        assert_eq!(
            result.applied_commands,
            vec![TimelineCommand::SetBRoll { seconds: 2.5 }]
        );
        assert_eq!(state.playhead, 0.0, "b-roll must not move the playhead");
    }

    #[test]
    fn context_menu_rolls_b_from_the_playhead_time() {
        // The "Roll B from this time" menu item emits a B-roll point at the current playhead — tested
        // through the same shared helper the closure calls (see the context-delete test's idiom).
        let editor = sample_timeline();
        let state = TimelineState {
            playhead: 4.5,
            ..Default::default()
        };
        let mut output = TimelineResponse::for_editor(&editor);
        emit_broll_at_playhead(&state, &mut output);
        assert_eq!(
            output.commands,
            vec![TimelineCommand::SetBRoll { seconds: 4.5 }]
        );
    }

    #[test]
    fn raw_input_ctrl_click_emits_broll_at_clicked_time_not_playhead() {
        let context = egui::Context::default();
        let editor = sample_timeline();
        let mut state = TimelineState::default();
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        // Click in the ruler (empty space): a PLAIN click there would seek the playhead; under Ctrl it
        // names a B-roll TIME instead, and the playhead-seek is gated off.
        let pos = Pos2::new(300.0, 10.0);
        let _ = run_command_frame(
            &context,
            &editor,
            &mut state,
            vec![Event::PointerMoved(pos)],
        );
        let _ = run_command_frame_mods(
            &context,
            &editor,
            &mut state,
            vec![Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: ctrl,
            }],
            ctrl,
        );
        let result = run_command_frame_mods(
            &context,
            &editor,
            &mut state,
            vec![Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: ctrl,
            }],
            ctrl,
        );
        let broll: Vec<f64> = result
            .commands
            .iter()
            .filter_map(|c| match c {
                TimelineCommand::SetBRoll { seconds } => Some(*seconds),
                _ => None,
            })
            .collect();
        assert_eq!(broll.len(), 1, "commands: {:?}", result.commands);
        // Non-zero => the emitted time came from the pointer's x (a mapped time), not the playhead (0).
        assert!(
            broll[0] > 0.0,
            "clicked time should be > 0, got {}",
            broll[0]
        );
        assert!(
            !result
                .commands
                .iter()
                .any(|c| matches!(c, TimelineCommand::SetPlayhead { .. })),
            "Ctrl+click must NOT also seek the playhead: {:?}",
            result.commands
        );
    }

    #[test]
    fn raw_input_ruler_drag_scrubs_playhead_plain_and_broll_under_ctrl() {
        let start = Pos2::new(200.0, 10.0);
        let end = Pos2::new(260.0, 10.0);

        // Plain empty-space (ruler) drag scrubs the PLAYHEAD — unchanged behaviour.
        {
            let context = egui::Context::default();
            let editor = sample_timeline();
            let mut state = TimelineState::default();
            let _ = run_command_frame(
                &context,
                &editor,
                &mut state,
                vec![Event::PointerMoved(start)],
            );
            let _ = run_command_frame(
                &context,
                &editor,
                &mut state,
                vec![Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                }],
            );
            let result = run_command_frame(
                &context,
                &editor,
                &mut state,
                vec![Event::PointerMoved(end)],
            );
            assert!(
                result
                    .commands
                    .iter()
                    .any(|c| matches!(c, TimelineCommand::SetPlayhead { .. })),
                "plain ruler drag must scrub the playhead: {:?}",
                result.commands
            );
            assert!(
                !result
                    .commands
                    .iter()
                    .any(|c| matches!(c, TimelineCommand::SetBRoll { .. }))
            );
        }

        // Ctrl+drag on the same empty space scrubs the B-ROLL point, never the playhead, and never
        // touches the transport (no ScrubStart).
        {
            let context = egui::Context::default();
            let editor = sample_timeline();
            let mut state = TimelineState::default();
            let ctrl = Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            };
            let _ = run_command_frame_mods(
                &context,
                &editor,
                &mut state,
                vec![Event::PointerMoved(start)],
                ctrl,
            );
            let _ = run_command_frame_mods(
                &context,
                &editor,
                &mut state,
                vec![Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: ctrl,
                }],
                ctrl,
            );
            let result = run_command_frame_mods(
                &context,
                &editor,
                &mut state,
                vec![Event::PointerMoved(end)],
                ctrl,
            );
            assert!(
                result
                    .commands
                    .iter()
                    .any(|c| matches!(c, TimelineCommand::SetBRoll { .. })),
                "Ctrl ruler drag must scrub the b-roll point: {:?}",
                result.commands
            );
            assert!(
                !result
                    .commands
                    .iter()
                    .any(|c| matches!(c, TimelineCommand::SetPlayhead { .. }))
            );
            assert!(
                !result
                    .commands
                    .iter()
                    .any(|c| matches!(c, TimelineCommand::ScrubStart))
            );
        }
    }

    fn run_legacy_frame(
        context: &egui::Context,
        editor: &mut Editor,
        state: &mut TimelineState,
        events: Vec<Event>,
        predicted_dt: f32,
    ) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 500.0))),
            predicted_dt,
            events,
            ..RawInput::default()
        };
        // See `run_command_frame_mods`: egui 0.36 panics on an unapplied `TexturesDelta` drop.
        let mut output = context.run_ui(input, |ui| timeline_ui(ui, editor, state));
        output.textures_delta.clear();
    }

    fn space_event() -> Event {
        Event::Key {
            key: Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn legacy_wrapper_applies_space_before_ticking() {
        let pointer = Pos2::new(300.0, 100.0);
        let pausing_context = egui::Context::default();
        let mut pausing_editor = sample_timeline();
        let mut pausing = TimelineState::default();
        run_legacy_frame(
            &pausing_context,
            &mut pausing_editor,
            &mut pausing,
            vec![Event::PointerMoved(pointer)],
            0.05,
        );
        pausing.playing = true;
        pausing.playhead = 7.0;
        run_legacy_frame(
            &pausing_context,
            &mut pausing_editor,
            &mut pausing,
            vec![Event::PointerMoved(pointer), space_event()],
            0.05,
        );
        assert!(!pausing.playing);
        assert_eq!(pausing.playhead, 7.0, "Pause must not advance an extra dt");

        let playing_context = egui::Context::default();
        let mut playing_editor = sample_timeline();
        let mut playing = TimelineState::default();
        run_legacy_frame(
            &playing_context,
            &mut playing_editor,
            &mut playing,
            vec![Event::PointerMoved(pointer)],
            0.05,
        );
        playing.playhead = 7.0;
        run_legacy_frame(
            &playing_context,
            &mut playing_editor,
            &mut playing,
            vec![Event::PointerMoved(pointer), space_event()],
            0.05,
        );
        assert!(playing.playing);
        assert!(
            playing.playhead > 7.0 && playing.playhead <= 7.1,
            "Play must begin advancing in the toggle frame, got {}",
            playing.playhead
        );
    }

    #[test]
    fn raw_input_space_emits_toggle_and_requests_repaint_without_ticking() {
        let context = egui::Context::default();
        let editor = sample_timeline();
        let mut state = TimelineState {
            playing: true,
            playhead: 7.0,
            ..TimelineState::default()
        };
        let pointer = Pos2::new(300.0, 100.0);
        let _ = run_command_frame(
            &context,
            &editor,
            &mut state,
            vec![Event::PointerMoved(pointer)],
        );
        let result = run_command_frame(
            &context,
            &editor,
            &mut state,
            vec![
                Event::PointerMoved(pointer),
                Event::Key {
                    key: Key::Space,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        assert_eq!(result.commands, vec![TimelineCommand::TogglePlay]);
        assert!(result.changed && result.repaint);
        assert_eq!(state.playhead, 7.0, "command API must not advance playback");
    }
}
