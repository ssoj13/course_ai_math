//! Persistent multi-slot image clipboard for egui apps (AE-style Shift+F5 / F5).
//!
//! # Host responsibilities
//! - Choose a root directory (e.g. `{project_cache}/clip/`).
//! - Capture full-resolution pixels and call [`ClipStore::push_rgba8`] /
//!   [`ClipStore::push_f16_rgba`] / [`ClipStore::push_encoded`].
//! - Bind hotkeys; open [`show_clip_picker`] and apply [`ClipPickerOutcome`].
//!
//! This crate never knows about comps, OCIO, or cloud APIs.

#![forbid(unsafe_code)]

mod clip_format;
mod store;

#[cfg(feature = "egui")]
mod picker;

pub use clip_format::ClipFormat;
pub use store::{ClipEntry, ClipError, ClipStore, Result as ClipResult};

#[cfg(feature = "egui")]
pub use picker::{ClipPickerOutcome, ClipPickerState, show_clip_picker, show_clip_picker_ui};
