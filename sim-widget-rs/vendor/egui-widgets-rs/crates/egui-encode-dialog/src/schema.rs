//! The caller-supplied schema: a description (as plain data) of the formats,
//! codecs and per-codec options the encode dialog should offer.
//!
//! The widget is generic — it has no hard-coded codec tables. Instead the host
//! app builds an [`EncodeSchema`] (e.g. "MP4 → {H.264, H.265}", "MOV → {ProRes}",
//! "Image Sequence → {EXR, PNG}") and the dialog renders the matching editors.
//! When the user presses Start the dialog returns the chosen values keyed by the
//! `id`s declared here, which the host maps back onto its real encoder.

use serde::{Deserialize, Serialize};

/// One editable option for a codec (or, more rarely, shared across a format).
///
/// Each variant carries a stable machine `id` (used as the key in the resulting
/// [`crate::EncodeSettings`] and as the egui widget id) plus a human `label`,
/// followed by the variant-specific bounds / choices / default. The dialog picks
/// the right editor widget per variant: checkbox, slider, combo box or text field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EncodeOption {
    /// On/off toggle, rendered as a checkbox (e.g. "RLE compression").
    Bool {
        id: String,
        label: String,
        default: bool,
    },
    /// Integer in `[min, max]`, rendered as an integer slider
    /// (e.g. CRF 0..=51, PNG compression 0..=9, bitrate kbps).
    Int {
        id: String,
        label: String,
        default: i64,
        min: i64,
        max: i64,
    },
    /// Float in `[min, max]`, rendered as a float slider
    /// (e.g. DWA loss level 0.0..=200.0, framerate-like knobs).
    Float {
        id: String,
        label: String,
        default: f64,
        min: f64,
        max: f64,
    },
    /// One-of-N pick, rendered as a combo box. `labels` are the visible choices;
    /// the chosen value is stored as the selected index (e.g. encoder impl,
    /// preset ladder, ProRes profile, container).
    Choice {
        id: String,
        label: String,
        labels: Vec<String>,
        default: usize,
    },
    /// Free-form string, rendered as a single-line text field.
    Text {
        id: String,
        label: String,
        default: String,
    },
}

impl EncodeOption {
    /// Stable machine id — the key this option's value is stored under.
    pub fn id(&self) -> &str {
        match self {
            EncodeOption::Bool { id, .. }
            | EncodeOption::Int { id, .. }
            | EncodeOption::Float { id, .. }
            | EncodeOption::Choice { id, .. }
            | EncodeOption::Text { id, .. } => id,
        }
    }

    /// Human-readable label shown next to the editor.
    pub fn label(&self) -> &str {
        match self {
            EncodeOption::Bool { label, .. }
            | EncodeOption::Int { label, .. }
            | EncodeOption::Float { label, .. }
            | EncodeOption::Choice { label, .. }
            | EncodeOption::Text { label, .. } => label,
        }
    }

    /// The default value this option starts at — used to seed a fresh
    /// [`crate::OptionValue`] in the settings map.
    pub fn default_value(&self) -> crate::OptionValue {
        match self {
            EncodeOption::Bool { default, .. } => crate::OptionValue::Bool(*default),
            EncodeOption::Int { default, .. } => crate::OptionValue::Int(*default),
            EncodeOption::Float { default, .. } => crate::OptionValue::Float(*default),
            EncodeOption::Choice { default, .. } => crate::OptionValue::Choice(*default),
            EncodeOption::Text { default, .. } => crate::OptionValue::Text(default.clone()),
        }
    }

    /// Whether `value` is meaningful for this option: the right kind, and for a choice an
    /// index that names an entry. Numbers outside `min..=max` still fit: they are kept and
    /// shown as they are (the encoder validates them); a stale choice index names nothing.
    pub fn fits(&self, value: &crate::OptionValue) -> bool {
        use crate::OptionValue as V;
        match (self, value) {
            (EncodeOption::Choice { labels, .. }, V::Choice(i)) => *i < labels.len(),
            (EncodeOption::Bool { .. }, V::Bool(_))
            | (EncodeOption::Int { .. }, V::Int(_))
            | (EncodeOption::Float { .. }, V::Float(_))
            | (EncodeOption::Text { .. }, V::Text(_)) => true,
            _ => false,
        }
    }

    // ---- Small typed constructors so callers don't write the struct-literal
    // boilerplate (and so the `id`/`label` ordering can't be swapped). ----

    /// Build a [`EncodeOption::Bool`].
    pub fn boolean(id: impl Into<String>, label: impl Into<String>, default: bool) -> Self {
        EncodeOption::Bool {
            id: id.into(),
            label: label.into(),
            default,
        }
    }

    /// Build a [`EncodeOption::Int`].
    pub fn int(
        id: impl Into<String>,
        label: impl Into<String>,
        default: i64,
        min: i64,
        max: i64,
    ) -> Self {
        EncodeOption::Int {
            id: id.into(),
            label: label.into(),
            default,
            min,
            max,
        }
    }

    /// Build a [`EncodeOption::Float`].
    pub fn float(
        id: impl Into<String>,
        label: impl Into<String>,
        default: f64,
        min: f64,
        max: f64,
    ) -> Self {
        EncodeOption::Float {
            id: id.into(),
            label: label.into(),
            default,
            min,
            max,
        }
    }

    /// Build a [`EncodeOption::Choice`] from any iterable of label strings.
    pub fn choice(
        id: impl Into<String>,
        label: impl Into<String>,
        labels: impl IntoIterator<Item = impl Into<String>>,
        default: usize,
    ) -> Self {
        EncodeOption::Choice {
            id: id.into(),
            label: label.into(),
            labels: labels.into_iter().map(Into::into).collect(),
            default,
        }
    }

    /// Build a [`EncodeOption::Text`].
    pub fn text(
        id: impl Into<String>,
        label: impl Into<String>,
        default: impl Into<String>,
    ) -> Self {
        EncodeOption::Text {
            id: id.into(),
            label: label.into(),
            default: default.into(),
        }
    }
}

/// A codec/encoder within a [`Format`] (e.g. "H.264", "ProRes", "EXR").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Codec {
    /// Stable machine id (stored in [`crate::EncodeSettings::codec_id`]).
    pub id: String,
    /// Display label shown on the codec tab/button.
    pub label: String,
    /// Whether the codec is selectable. Unavailable codecs are still listed
    /// (greyed out) so the user sees what *could* be available — mirrors playa's
    /// "encoder not available ✗" affordance without baking in the availability
    /// probe (the host computes it).
    #[serde(default = "default_true")]
    pub available: bool,
    /// Optional one-line hint shown under the options (e.g. "Best compression,
    /// slower encoding").
    #[serde(default)]
    pub hint: Option<String>,
    /// The per-codec editable options, rendered top-to-bottom.
    pub options: Vec<EncodeOption>,
}

fn default_true() -> bool {
    true
}

impl Codec {
    /// Convenience constructor for an available codec with no hint.
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        options: impl IntoIterator<Item = EncodeOption>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            available: true,
            hint: None,
            options: options.into_iter().collect(),
        }
    }

    /// Builder: mark this codec available or not.
    pub fn available(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    /// Builder: attach a one-line hint.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// An output format / container (e.g. "MP4", "MOV", "Image Sequence").
///
/// The `extension` is used to keep the output path in sync when the user
/// switches formats (the staged-path UX from the source dialog).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Format {
    /// Stable machine id (stored in [`crate::EncodeSettings::format_id`]).
    pub id: String,
    /// Display label shown on the format tab/button.
    pub label: String,
    /// Default file extension for this format (e.g. "mp4", "exr"), without the
    /// dot. Applied to the output path when the format is selected.
    pub extension: String,
    /// The codecs offered by this format.
    pub codecs: Vec<Codec>,
}

impl Format {
    /// Convenience constructor.
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        extension: impl Into<String>,
        codecs: impl IntoIterator<Item = Codec>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            extension: extension.into(),
            codecs: codecs.into_iter().collect(),
        }
    }
}

/// The full schema the host hands the dialog: a flat list of formats, each with
/// its codecs and per-codec options. This is the single source of truth for what
/// the dialog renders; the resulting [`crate::EncodeSettings`] references its ids.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EncodeSchema {
    /// All available output formats. The first one is selected by default.
    pub formats: Vec<Format>,
}

impl EncodeSchema {
    /// Build a schema from a list of formats.
    pub fn new(formats: impl IntoIterator<Item = Format>) -> Self {
        Self {
            formats: formats.into_iter().collect(),
        }
    }

    /// Look up a format by id.
    pub fn format(&self, id: &str) -> Option<&Format> {
        self.formats.iter().find(|f| f.id == id)
    }
}

impl Format {
    /// Look up a codec by id within this format.
    pub fn codec(&self, id: &str) -> Option<&Codec> {
        self.codecs.iter().find(|c| c.id == id)
    }
}
