//! `egui-jobs-table` — a generic, sortable/filterable table of async jobs.
//!
//! # What this is
//! A reusable egui widget that renders a queue of background jobs / tasks as a
//! table: one row per job, each with a coloured **status badge**
//! (queued / running / done / failed / cancelled), an optional **progress
//! bar**, caller-supplied **data columns**, and per-row **action buttons**
//! (cancel / retry / open / remove).
//!
//! # The model is flat and caller-owned
//! The widget owns NO job state. The host passes an immutable slice of
//! [`JobRow`] plus a mutable [`JobsTableState`] (sort + filter + selection),
//! and the widget returns a `Vec<`[`JobAction`]`>` describing what the user
//! clicked. The host applies those actions to its own queue however it likes.
//! This keeps the widget decoupled from any concrete job engine — there is no
//! network code, no async runtime, and no application-specific types here.
//!
//! ```no_run
//! # use egui_jobs_table::{JobsTable, JobsTableState, JobRow, JobStatus, JobAction};
//! # fn demo(ui: &mut egui::Ui, rows: &[JobRow], state: &mut JobsTableState) {
//! let headers = ["Submitted", "Name", "State"].map(String::from).to_vec();
//! let actions = JobsTable::new(headers).show(ui, rows, state);
//! for action in actions {
//!     match action {
//!         JobAction::Cancel { id } => { /* host: queue.cancel(id) */ }
//!         JobAction::Retry { id }  => { /* host: queue.retry(id)  */ }
//!         JobAction::Open { id }   => { /* host: reveal output    */ }
//!         JobAction::Remove { id } => { /* host: queue.remove(id) */ }
//!         JobAction::Edit { id }   => { /* host: reopen the submission */ }
//!         JobAction::Custom { id, key } => { /* host: its own row button, by name */ }
//!         JobAction::Select { id, selected } => { /* host: track selection if desired */ }
//!     }
//! }
//! # }
//! ```
//!
//! # Gathered from / what was dropped
//! Gathered from the playa `playa-jobs-ui` panel (`panel.rs`) and decoupled
//! from `playa-jobs-core` (`JobQueue`, `Job`, `JobState`). The Seedance submit
//! dialog (`dialog.rs`) was intentionally **dropped**: it carried provider-
//! specific endpoints, per-second cost pricing, and a fal.ai params builder
//! that have no place in a generic table. Sort by an opaque "cost" column was
//! likewise dropped; sort the caller's own columns instead.

#![forbid(unsafe_code)]

mod table;

pub use table::{
    Accent, JobAction, JobRow, JobStatus, JobsTable, JobsTableState, RowActions, RowExtra,
};
