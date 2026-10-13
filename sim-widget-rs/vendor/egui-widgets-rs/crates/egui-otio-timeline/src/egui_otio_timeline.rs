//! egui-otio-timeline — a thin re-export of the otio-rs OTIO timeline widget.
//!
//! `otio-timeline` already ships the complete egui NLE timeline widget over
//! `otio-edit`: [`otio_timeline::timeline_ui`] draws tracks + clips + playhead and
//! turns mouse gestures into `otio_edit::ops` (Move / Slide / Trim / Razor) with
//! undo/redo. This crate only RE-EXPORTS it (and the `otio-edit` editor) under the
//! egui-widgets-rs `egui-*` naming — there is no wrapper logic, so downstream code
//! uses the upstream API unchanged:
//!
//! ```ignore
//! use egui_otio_timeline::{otio_edit, otio_timeline};
//!
//! let mut editor = otio_edit::Editor::default(); // or otio_edit::sample …
//! let mut state = otio_timeline::TimelineState::default();
//! // each frame, inside an egui Ui:
//! otio_timeline::timeline_ui(ui, &mut editor, &mut state);
//! ```
//!
//! The widget is **egui-only (no wgpu)**, so unlike `egui-nodes-view` this stays a
//! light, wasm-friendly re-export; it only pulls the otio-rs model/edit tree.

/// The OTIO edit engine (`Editor`, `ops`, undo/redo) — re-exported so a host
/// depends on this one crate instead of reaching into otio-rs.
pub use otio_edit;
/// The egui timeline widget (`timeline_ui`, `TimelineState`, `Tool`, …).
pub use otio_timeline;
