//! The working/result settings: the concrete values the user has chosen for the
//! currently selected format + codec, plus output path and frame range.
//!
//! [`EncodeSettings`] is both the dialog's persistent working state (it survives
//! across `show()` calls so codec switches don't lose edits) and the payload the
//! dialog hands back via [`crate::EncodeDialogResult::Start`]. The host maps the
//! `format_id` / `codec_id` / per-option values back onto its real encoder.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A single chosen option value. Mirrors the variants of
/// [`crate::EncodeOption`], but holds the *value* rather than the schema.
///
/// `Choice` stores the selected index into the option's `labels`; the host can
/// resolve it back to a label via the schema. `BTreeMap`-friendly ordering is
/// not required — values are looked up by id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum OptionValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    /// Index into the [`crate::EncodeOption::Choice`] `labels`.
    Choice(usize),
    Text(String),
}

impl OptionValue {
    /// Read as bool (returns `None` if the value isn't a [`OptionValue::Bool`]).
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            OptionValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    /// Read as integer.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            OptionValue::Int(v) => Some(*v),
            _ => None,
        }
    }

    /// Read as float.
    pub fn as_float(&self) -> Option<f64> {
        match self {
            OptionValue::Float(v) => Some(*v),
            _ => None,
        }
    }

    /// Read as choice index.
    pub fn as_choice(&self) -> Option<usize> {
        match self {
            OptionValue::Choice(v) => Some(*v),
            _ => None,
        }
    }

    /// Read as text.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            OptionValue::Text(v) => Some(v),
            _ => None,
        }
    }
}

/// Inclusive frame range `[start, end]` to encode.
///
/// The dialog only edits and reports the range — it does not pull frames or know
/// what a "frame" is. The host decides how to interpret it. `enabled == false`
/// means "use the host's own range" (the source dialog's "use active Comp"
/// behavior), in which case `start`/`end` are advisory.
/// `Default` is `{ enabled: false, start: 0, end: 0 }` — i.e. "use the host's
/// range", matching the source dialog's default-on-open behavior.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameRange {
    /// Whether the user has overridden the host's default range.
    pub enabled: bool,
    pub start: i64,
    pub end: i64,
}

/// The chosen encode settings — the dialog's working state and `Start` payload.
///
/// `option_values` holds the per-option values for the **currently selected
/// codec only**, keyed by option id. When the user switches codec the dialog
/// re-seeds this map from the new codec's defaults (so each codec presents its
/// own options); if you need to preserve per-codec edits across switches, persist
/// the whole [`EncodeSettings`] per codec on the host side. This matches the
/// "flat DTO for one selected codec" shape of playa's `EncoderSettings` while
/// staying fully generic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EncodeSettings {
    /// Selected [`crate::Format`] id.
    pub format_id: String,
    /// Selected [`crate::Codec`] id within the format.
    pub codec_id: String,
    /// Output file path, as a plain string. The dialog edits it as text and
    /// keeps its extension in sync with the selected format; the host runs its
    /// own file dialog (the widget intentionally has no `rfd` dependency).
    pub output_path: String,
    /// Inclusive frame range to encode.
    #[serde(default)]
    pub frame_range: FrameRange,
    /// Per-option values for the selected codec, keyed by option id.
    pub option_values: BTreeMap<String, OptionValue>,
}

impl Default for EncodeSettings {
    fn default() -> Self {
        Self {
            format_id: String::new(),
            codec_id: String::new(),
            output_path: "output".to_string(),
            frame_range: FrameRange::default(),
            option_values: BTreeMap::new(),
        }
    }
}

impl EncodeSettings {
    /// Get a chosen option value by id.
    pub fn get(&self, id: &str) -> Option<&OptionValue> {
        self.option_values.get(id)
    }

    /// Get a bool option value by id.
    pub fn get_bool(&self, id: &str) -> Option<bool> {
        self.option_values.get(id).and_then(OptionValue::as_bool)
    }

    /// Get an int option value by id.
    pub fn get_int(&self, id: &str) -> Option<i64> {
        self.option_values.get(id).and_then(OptionValue::as_int)
    }

    /// Get a float option value by id.
    pub fn get_float(&self, id: &str) -> Option<f64> {
        self.option_values.get(id).and_then(OptionValue::as_float)
    }

    /// Get a choice index by id.
    pub fn get_choice(&self, id: &str) -> Option<usize> {
        self.option_values.get(id).and_then(OptionValue::as_choice)
    }

    /// Get a text option value by id.
    pub fn get_text(&self, id: &str) -> Option<&str> {
        self.option_values.get(id).and_then(OptionValue::as_text)
    }
}
