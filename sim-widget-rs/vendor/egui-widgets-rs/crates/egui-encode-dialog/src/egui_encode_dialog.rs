//! A generic, data-driven "export / encode settings" modal for egui.
//!
//! This is a reusable port of playa's encode dialog. The original was wired
//! straight into playa's engine: hard-coded H.264 / H.265 / ProRes / AV1 / EXR /
//! PNG tables, direct `playa_io::ffmpeg` encoder probing + encoding, `playa_engine`
//! frame pulling, and `rfd` for the file picker. None of that lives here.
//!
//! Instead the widget is **schema-driven**: the host describes its formats,
//! codecs and per-codec options as plain data ([`EncodeSchema`]), and the dialog
//! renders the matching editors, keeps a working [`EncodeSettings`] map, and on
//! Start hands the chosen values back. The staged layout from the source is kept:
//!
//! ```text
//!   [ output path .............. ] [+]      ← path row (host runs its own Browse)
//!   format:  ( MP4 ) ( MOV ) ( Image Seq )  ← format tabs
//!   codec:   ( H.264 ) ( H.265 ) …          ← codec tabs (greyed if unavailable)
//!   ─────────────────────────────
//!   <per-codec option editors>              ← bool/int/float/choice/text
//!   ─────────────────────────────
//!   frame range:  [x] 1 … 100               ← optional override
//!   ─────────────────────────────
//!                       [ Cancel ] [ Start ]← footer
//! ```
//!
//! ## Model
//! - [`EncodeSchema`] / [`Format`] / [`Codec`] / [`EncodeOption`] — the schema
//!   the caller supplies (read-only to the dialog).
//! - [`EncodeSettings`] / [`OptionValue`] / [`FrameRange`] — the chosen values;
//!   both the dialog's persistent working state and the `Start` payload.
//! - [`EncodeDialog`] — owns the view state + working settings.
//! - [`EncodeDialog::show`] returns an [`EncodeDialogResult`]:
//!   [`Open`](EncodeDialogResult::Open) / [`Cancelled`](EncodeDialogResult::Cancelled)
//!   / [`Start`](EncodeDialogResult::Start).
//!
//! ## What was dropped vs. the source (host's responsibility now)
//! - All ffmpeg encoder probing / encoding (`encode_sequence_from_comp`, the
//!   NVENC/QSV/AMF selection, pixel-format logic) and the progress channel /
//!   worker thread / `EncodeStage` machinery.
//! - The concrete codec & sequence-format enums and their per-format settings
//!   structs — they become the host's [`EncodeSchema`].
//! - `rfd` file dialog: the dialog edits the path as text and exposes a Browse
//!   button only as a [`ShowConfig::show_browse`] flag; the host opens the picker.
//! - playa frame-pull / EXR pass-through layer inspection.

mod schema;
mod settings;

pub use schema::{Codec, EncodeOption, EncodeSchema, Format};
pub use settings::{EncodeSettings, FrameRange, OptionValue};

/// Outcome of one [`EncodeDialog::show`] call.
#[derive(Clone, Debug, PartialEq)]
pub enum EncodeDialogResult {
    /// The dialog is still open; keep calling `show()` each frame.
    Open,
    /// The user cancelled (Cancel button, backdrop click, or Escape). The host
    /// should stop showing the dialog.
    Cancelled,
    /// The user pressed Start. Carries a snapshot of the chosen settings. The
    /// host should stop showing the dialog and begin encoding.
    Start(EncodeSettings),
}

/// Per-`show()` presentation tweaks. Cheap to build inline; defaults are sane.
#[derive(Clone, Debug)]
pub struct ShowConfig {
    /// Window/modal title.
    pub title: String,
    /// Fixed content width in points.
    pub width: f32,
    /// Show a "Browse…" button next to the path field. When the user clicks it
    /// the dialog sets [`EncodeDialog::browse_requested`]; the host opens its own
    /// file picker and writes the result back via [`EncodeDialog::set_output_path`].
    pub show_browse: bool,
    /// Show the optional frame-range row.
    pub show_frame_range: bool,
    /// Label for the Start button (e.g. "Start", "Encode", "Export").
    pub start_label: String,
}

impl Default for ShowConfig {
    fn default() -> Self {
        Self {
            title: "Export".to_string(),
            width: 560.0,
            show_browse: true,
            show_frame_range: true,
            start_label: "Start".to_string(),
        }
    }
}

/// The encode dialog: owns the view state and the working [`EncodeSettings`].
///
/// Construct once with [`EncodeDialog::new`] (or [`EncodeDialog::with_settings`]
/// to restore persisted state), then call [`EncodeDialog::show`] every frame
/// while the dialog is open.
pub struct EncodeDialog {
    /// The caller-supplied schema. Owned so the dialog can re-seed option values
    /// on codec switches without the caller re-passing it each frame.
    schema: EncodeSchema,
    /// The working / result settings (selected ids + path + range + option map).
    settings: EncodeSettings,
    /// Set to `true` for one frame after the user clicks Browse. The host reads
    /// [`EncodeDialog::take_browse_request`] and opens its own file picker.
    browse_requested: bool,
}

impl EncodeDialog {
    /// Create a dialog from a schema, selecting the first format + its first
    /// available codec and seeding option values from their defaults.
    pub fn new(schema: EncodeSchema) -> Self {
        let mut settings = EncodeSettings::default();
        let mut dialog = Self {
            schema,
            settings: EncodeSettings::default(),
            browse_requested: false,
        };
        // Seed selection from the first format / first available codec.
        if let Some(format) = dialog.schema.formats.first() {
            settings.format_id = format.id.clone();
            settings.output_path = format!("output.{}", format.extension);
            if let Some(codec) = first_available_codec(format) {
                settings.codec_id = codec.id.clone();
                settings.option_values = seed_values(codec);
            }
        }
        dialog.settings = settings;
        dialog
    }

    /// Create a dialog from a schema and pre-existing settings (restore a
    /// persisted session). The settings are validated/repaired against the
    /// schema: if the stored ids are missing, selection falls back to defaults,
    /// and any options absent from `settings` are seeded from the codec.
    pub fn with_settings(schema: EncodeSchema, settings: EncodeSettings) -> Self {
        let mut dialog = Self {
            schema,
            settings,
            browse_requested: false,
        };
        dialog.repair_selection();
        dialog
    }

    /// Borrow the current working settings (e.g. to persist them on close).
    pub fn settings(&self) -> &EncodeSettings {
        &self.settings
    }

    /// Borrow the schema.
    pub fn schema(&self) -> &EncodeSchema {
        &self.schema
    }

    /// Overwrite the output path (e.g. after the host's file picker returns).
    pub fn set_output_path(&mut self, path: impl Into<String>) {
        self.settings.output_path = path.into();
    }

    /// Was Browse clicked since the last check? Consumes the flag.
    pub fn take_browse_request(&mut self) -> bool {
        std::mem::take(&mut self.browse_requested)
    }

    /// Show the modal for one frame. Returns the [`EncodeDialogResult`].
    ///
    /// Uses egui 0.34's native [`egui::Modal`] (centered, backdrop-dimmed,
    /// Escape/backdrop-to-close). Call this every frame while the dialog should
    /// be visible; stop calling it once the result is `Cancelled` or `Start(_)`.
    pub fn show(&mut self, ctx: &egui::Context, config: &ShowConfig) -> EncodeDialogResult {
        let mut result = EncodeDialogResult::Open;

        let modal = egui::Modal::new(egui::Id::new("egui_encode_dialog")).show(ctx, |ui| {
            ui.set_width(config.width);
            ui.heading(&config.title);
            ui.add_space(8.0);

            self.render_path_row(ui, config);
            ui.add_space(8.0);
            ui.separator();

            self.render_format_tabs(ui);
            ui.add_space(4.0);
            self.render_codec_tabs(ui);

            ui.separator();
            ui.add_space(8.0);

            self.render_options(ui);

            if config.show_frame_range {
                ui.add_space(8.0);
                ui.separator();
                self.render_frame_range(ui);
            }

            ui.add_space(12.0);
            ui.separator();

            // Footer: Cancel / Start.
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    result = EncodeDialogResult::Cancelled;
                }
                // Start is only enabled once a codec is selected.
                let ready = !self.settings.codec_id.is_empty();
                ui.add_enabled_ui(ready, |ui| {
                    if ui.button(&config.start_label).clicked() {
                        result = EncodeDialogResult::Start(self.settings.clone());
                    }
                });
            });
        });

        // Backdrop click / Escape → cancel (unless an explicit button already won).
        if result == EncodeDialogResult::Open && modal.should_close() {
            result = EncodeDialogResult::Cancelled;
        }
        result
    }

    // ---- internal rendering helpers ----

    /// Output path row: the shared path field, plus the increment affordance.
    ///
    /// Uses [`egui_file_field::FileField`] rather than a `TextEdit` with a Browse button parked
    /// beside it, so an output path looks and behaves like every other path in a host that uses
    /// this widget set. The Browse contract is unchanged: the field only REPORTS the click and
    /// the host opens its own picker (see [`ShowConfig::show_browse`]).
    fn render_path_row(&mut self, ui: &mut egui::Ui, config: &ShowConfig) {
        ui.horizontal(|ui| {
            ui.label("Output:");

            // The remembered directory is the HOST's business here - it already resolves the
            // output path itself - so this is a throwaway state rather than a stored one.
            let mut state = egui_file_field::FileFieldState::default();
            let r = egui_file_field::FileField::new(&mut self.settings.output_path, &mut state)
                .kind(egui_file_field::PickKind::File)
                .hint("output")
                .show(ui);
            if config.show_browse && r.browse_clicked() {
                self.browse_requested = true;
            }

            // Increment the trailing number in the filename (file001 → file002),
            // preserving zero-padding width — the source dialog's "+" affordance.
            if ui
                .button("+")
                .on_hover_text("Increment number in filename (file001 → file002)")
                .clicked()
            {
                self.settings.output_path = increment_filename(&self.settings.output_path);
            }
        });
    }

    /// Format selector row (e.g. MP4 / MOV / Image Sequence).
    fn render_format_tabs(&mut self, ui: &mut egui::Ui) {
        // Collect (id, label, extension) up front to avoid borrowing `self.schema`
        // while we mutate `self.settings` inside the loop.
        let formats: Vec<(String, String, String)> = self
            .schema
            .formats
            .iter()
            .map(|f| (f.id.clone(), f.label.clone(), f.extension.clone()))
            .collect();

        ui.horizontal(|ui| {
            ui.label("Format:");
            for (id, label, ext) in &formats {
                let selected = self.settings.format_id == *id;
                let btn = egui::Button::new(label)
                    .selected(selected)
                    .min_size(egui::vec2(80.0, 0.0));
                if ui.add(btn).clicked() && !selected {
                    self.select_format(id, ext);
                }
            }
        });
    }

    /// Codec selector row for the current format. Unavailable codecs are shown
    /// greyed-out with a ✗ hover, matching the source's affordance.
    fn render_codec_tabs(&mut self, ui: &mut egui::Ui) {
        let Some(format) = self.schema.format(&self.settings.format_id) else {
            return;
        };
        // Snapshot codec identity/availability so we can mutate settings in-loop.
        let codecs: Vec<(String, String, bool)> = format
            .codecs
            .iter()
            .map(|c| (c.id.clone(), c.label.clone(), c.available))
            .collect();

        ui.horizontal(|ui| {
            ui.label("Codec:");
            for (id, label, available) in &codecs {
                let selected = self.settings.codec_id == *id;
                ui.add_enabled_ui(*available, |ui| {
                    let btn = egui::Button::new(label)
                        .selected(selected)
                        .min_size(egui::vec2(90.0, 0.0));
                    if ui.add(btn).clicked() && !selected {
                        self.select_codec(id);
                    }
                });
                if !available {
                    ui.label(egui_widgets_config::icons::CLOSE)
                        .on_hover_text(format!("{label} not available"));
                }
            }
        });
    }

    /// Render the editors for the current codec's options, plus its optional hint.
    fn render_options(&mut self, ui: &mut egui::Ui) {
        // Clone the option schema for the current codec so we can mutate
        // `self.settings.option_values` while iterating.
        let (options, hint) = match self
            .schema
            .format(&self.settings.format_id)
            .and_then(|f| f.codec(&self.settings.codec_id))
        {
            Some(codec) => (codec.options.clone(), codec.hint.clone()),
            None => {
                ui.label("No codec selected.");
                return;
            }
        };

        if options.is_empty() {
            ui.label("No options for this codec.");
        }
        for option in &options {
            render_option_editor(ui, option, &mut self.settings);
        }

        if let Some(hint) = hint {
            ui.add_space(4.0);
            ui.label(hint);
        }
    }

    /// Optional frame-range row: an enable checkbox + two integer drag values.
    fn render_frame_range(&mut self, ui: &mut egui::Ui) {
        let range = &mut self.settings.frame_range;
        ui.horizontal(|ui| {
            ui.checkbox(&mut range.enabled, "Frame range");
            ui.add_enabled_ui(range.enabled, |ui| {
                ui.add(egui::DragValue::new(&mut range.start).prefix("start "));
                ui.add(egui::DragValue::new(&mut range.end).prefix("end "));
            });
            if !range.enabled {
                ui.label("(use host's default range)");
            }
        });
        // Keep the range well-formed: end >= start.
        if range.end < range.start {
            range.end = range.start;
        }
    }

    // ---- selection / state transitions ----

    /// Switch to a different format: update id, sync the path extension, and
    /// select that format's first available codec.
    fn select_format(&mut self, format_id: &str, extension: &str) {
        self.settings.format_id = format_id.to_string();
        self.settings.output_path = replace_extension(&self.settings.output_path, extension);
        let codec = self
            .schema
            .format(format_id)
            .and_then(first_available_codec)
            .cloned();
        match codec {
            Some(codec) => {
                self.settings.codec_id = codec.id.clone();
                self.settings.option_values = seed_values(&codec);
            }
            None => {
                self.settings.codec_id.clear();
                self.settings.option_values.clear();
            }
        }
    }

    /// Switch codec within the current format and re-seed its option values.
    fn select_codec(&mut self, codec_id: &str) {
        let codec = self
            .schema
            .format(&self.settings.format_id)
            .and_then(|f| f.codec(codec_id))
            .cloned();
        if let Some(codec) = codec {
            self.settings.codec_id = codec.id.clone();
            self.settings.option_values = seed_values(&codec);
        }
    }

    /// Validate restored settings against the schema, repairing any dangling ids
    /// and filling in option values missing from the persisted map.
    fn repair_selection(&mut self) {
        // Format: fall back to the first if the stored id is unknown.
        let format = self
            .schema
            .format(&self.settings.format_id)
            .or_else(|| self.schema.formats.first())
            .cloned();
        let Some(format) = format else {
            // Empty schema — nothing to select.
            self.settings.format_id.clear();
            self.settings.codec_id.clear();
            self.settings.option_values.clear();
            return;
        };
        self.settings.format_id = format.id.clone();

        // Codec: keep the stored one if it still exists, else first available.
        let codec = format
            .codec(&self.settings.codec_id)
            .or_else(|| first_available_codec(&format))
            .cloned();
        let Some(codec) = codec else {
            self.settings.codec_id.clear();
            self.settings.option_values.clear();
            return;
        };
        self.settings.codec_id = codec.id.clone();

        // Drop values whose ids no longer belong to this codec, then give every current
        // option a value that fits it: missing ones and ones that no longer fit (wrong
        // kind, a choice index past a list that shrank) take the default. This entry
        // boundary is the one place stored settings are repaired; showing never writes.
        let valid_ids: std::collections::BTreeSet<&str> =
            codec.options.iter().map(EncodeOption::id).collect();
        self.settings
            .option_values
            .retain(|k, _| valid_ids.contains(k.as_str()));
        for option in &codec.options {
            let value = self
                .settings
                .option_values
                .entry(option.id().to_string())
                .or_insert_with(|| option.default_value());
            if !option.fits(value) {
                *value = option.default_value();
            }
        }
    }
}

/// Render the editor widget matching a single [`EncodeOption`] and write the
/// edited value back into `settings.option_values` under the option's id.
fn render_option_editor(ui: &mut egui::Ui, option: &EncodeOption, settings: &mut EncodeSettings) {
    // Ensure a value exists (defensive; selection seeding normally guarantees it).
    let value = settings
        .option_values
        .entry(option.id().to_string())
        .or_insert_with(|| option.default_value());

    match option {
        EncodeOption::Bool { label, .. } => {
            if let OptionValue::Bool(v) = value {
                ui.checkbox(v, label);
            }
        }
        EncodeOption::Int {
            label, min, max, ..
        } => {
            if let OptionValue::Int(v) = value {
                ui.horizontal(|ui| {
                    ui.label(format!("{label}:"));
                    ui.add(egui_slider::SlimSlider::new(v, *min..=*max));
                });
            }
        }
        EncodeOption::Float {
            label, min, max, ..
        } => {
            if let OptionValue::Float(v) = value {
                ui.horizontal(|ui| {
                    ui.label(format!("{label}:"));
                    ui.add(egui_slider::SlimSlider::new(v, *min..=*max));
                });
            }
        }
        EncodeOption::Choice {
            id, label, labels, ..
        } => {
            if let OptionValue::Choice(idx) = value {
                // Entry repair (`repair_selection`) keeps the index valid; one that slips
                // in later is shown blank, not reset: showing never writes a value.
                let selected_text = labels.get(*idx).map(String::as_str).unwrap_or("");
                ui.horizontal(|ui| {
                    ui.label(format!("{label}:"));
                    egui::ComboBox::from_id_salt(format!("encode_opt_{id}"))
                        .selected_text(selected_text)
                        .show_ui(ui, |ui| {
                            for (i, choice) in labels.iter().enumerate() {
                                ui.selectable_value(idx, i, choice);
                            }
                        });
                });
            }
        }
        EncodeOption::Text { label, .. } => {
            if let OptionValue::Text(v) = value {
                ui.horizontal(|ui| {
                    ui.label(format!("{label}:"));
                    ui.text_edit_singleline(v);
                });
            }
        }
    }
}

/// First selectable (available) codec of a format, falling back to the first
/// codec at all if none are flagged available (so the UI never shows an empty
/// codec area for a non-empty format).
fn first_available_codec(format: &Format) -> Option<&Codec> {
    format
        .codecs
        .iter()
        .find(|c| c.available)
        .or_else(|| format.codecs.first())
}

/// Build a fresh option-value map for a codec from its option defaults.
fn seed_values(codec: &Codec) -> std::collections::BTreeMap<String, OptionValue> {
    codec
        .options
        .iter()
        .map(|o| (o.id().to_string(), o.default_value()))
        .collect()
}

/// Replace (or append) the extension on a path-like string. Operates on the
/// trailing path component so directory dots aren't disturbed.
fn replace_extension(path: &str, ext: &str) -> String {
    // Split off the final component to avoid touching dots in parent dirs.
    let (dir, file) = match path.rfind(['/', '\\']) {
        Some(sep) => (&path[..=sep], &path[sep + 1..]),
        None => ("", path),
    };
    let stem = match file.rfind('.') {
        Some(dot) => &file[..dot],
        None => file,
    };
    format!("{dir}{stem}.{ext}")
}

/// Increment the last run of digits in a filename's stem, preserving zero-pad
/// width (file009.mp4 → file010.mp4; render.mp4 → render001.mp4). Ported from the
/// source dialog's `increment_filename`, generalized to a plain string path.
fn increment_filename(path: &str) -> String {
    let (dir, file) = match path.rfind(['/', '\\']) {
        Some(sep) => (&path[..=sep], &path[sep + 1..]),
        None => ("", path),
    };
    let (stem, ext) = match file.rfind('.') {
        Some(dot) => (&file[..dot], &file[dot..]), // ext keeps its leading dot
        None => (file, ""),
    };

    // Locate the last digit run in the stem.
    let bytes = stem.as_bytes();
    let mut run_end = None;
    let mut run_start = None;
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        if bytes[i].is_ascii_digit() {
            if run_end.is_none() {
                run_end = Some(i + 1);
            }
            run_start = Some(i);
        } else if run_end.is_some() {
            break; // hit a non-digit after the trailing run
        }
    }

    let new_stem = match (run_start, run_end) {
        (Some(s), Some(e)) => {
            let digits = &stem[s..e];
            let width = digits.len();
            // Parse as u64 to tolerate long numbers; on overflow just append.
            match digits.parse::<u64>() {
                Ok(n) => {
                    let next = n.saturating_add(1);
                    let grown = format!("{next}");
                    let w = width.max(grown.len());
                    format!("{}{:0w$}{}", &stem[..s], next, &stem[e..], w = w)
                }
                Err(_) => format!("{stem}001"),
            }
        }
        _ => format!("{stem}001"),
    };

    format!("{dir}{new_stem}{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_schema() -> EncodeSchema {
        EncodeSchema::new([
            Format::new(
                "mp4",
                "MP4",
                "mp4",
                [
                    Codec::new(
                        "h264",
                        "H.264",
                        [
                            EncodeOption::choice("impl", "Encoder", ["Auto", "HW", "SW"], 0),
                            EncodeOption::int("crf", "CRF", 23, 0, 51),
                            EncodeOption::choice("preset", "Preset", ["fast", "medium", "slow"], 1),
                        ],
                    )
                    .hint("Good default codec"),
                    Codec::new(
                        "h265",
                        "H.265",
                        [EncodeOption::int("crf", "CRF", 28, 0, 51)],
                    )
                    .available(false),
                ],
            ),
            Format::new(
                "seq",
                "Image Sequence",
                "exr",
                [Codec::new(
                    "exr",
                    "EXR",
                    [
                        EncodeOption::choice("comp", "Compression", ["None", "ZIP", "PIZ"], 1),
                        EncodeOption::float("dwa", "DWA loss", 45.0, 0.0, 200.0),
                        EncodeOption::boolean("alpha", "Alpha", true),
                    ],
                )],
            ),
        ])
    }

    #[test]
    fn new_selects_first_format_and_available_codec() {
        let d = EncodeDialog::new(demo_schema());
        assert_eq!(d.settings().format_id, "mp4");
        assert_eq!(d.settings().codec_id, "h264"); // h264 available, picked first
        assert_eq!(d.settings().output_path, "output.mp4");
        assert_eq!(d.settings().get_int("crf"), Some(23));
        assert_eq!(d.settings().get_choice("preset"), Some(1));
    }

    #[test]
    fn select_format_syncs_extension_and_codec() {
        let mut d = EncodeDialog::new(demo_schema());
        d.select_format("seq", "exr");
        assert_eq!(d.settings().format_id, "seq");
        assert_eq!(d.settings().codec_id, "exr");
        assert_eq!(d.settings().output_path, "output.exr");
        assert_eq!(d.settings().get_bool("alpha"), Some(true));
        // h264-only ids must be gone after re-seeding.
        assert!(d.settings().get("preset").is_none());
    }

    #[test]
    fn repair_fills_missing_and_drops_unknown() {
        let mut s = EncodeSettings {
            format_id: "mp4".into(),
            codec_id: "h264".into(),
            output_path: "x.mp4".into(),
            ..Default::default()
        };
        // Stale id that isn't in the schema + missing the real ones.
        s.option_values.insert("bogus".into(), OptionValue::Int(7));
        let d = EncodeDialog::with_settings(demo_schema(), s);
        assert!(d.settings().get("bogus").is_none());
        assert_eq!(d.settings().get_int("crf"), Some(23)); // seeded default
        assert_eq!(d.settings().get_choice("impl"), Some(0));
    }

    /// Stored values that no longer fit their option are repaired once, at entry: a choice
    /// index past a list that shrank and a value of the wrong kind take the default, while
    /// an out-of-range number is a real value and is kept.
    #[test]
    fn repair_resets_values_that_no_longer_fit() {
        let mut s = EncodeSettings {
            format_id: "mp4".into(),
            codec_id: "h264".into(),
            output_path: "x.mp4".into(),
            ..Default::default()
        };
        s.option_values.insert("impl".into(), OptionValue::Choice(9));
        s.option_values.insert("crf".into(), OptionValue::Text("x".into()));
        let d = EncodeDialog::with_settings(demo_schema(), s.clone());
        assert_eq!(d.settings().get_choice("impl"), Some(0));
        assert_eq!(d.settings().get_int("crf"), Some(23));

        s.option_values.insert("crf".into(), OptionValue::Int(99));
        let d = EncodeDialog::with_settings(demo_schema(), s);
        assert_eq!(d.settings().get_int("crf"), Some(99));
    }

    #[test]
    fn increment_filename_preserves_padding() {
        assert_eq!(increment_filename("a/file009.mp4"), "a/file010.mp4");
        assert_eq!(increment_filename("test999.mp4"), "test1000.mp4");
        assert_eq!(increment_filename("render.mp4"), "render001.mp4");
        assert_eq!(increment_filename("noext"), "noext001");
    }

    #[test]
    fn replace_extension_handles_dirs() {
        assert_eq!(replace_extension("a.b/out.mp4", "exr"), "a.b/out.exr");
        assert_eq!(replace_extension("out", "mp4"), "out.mp4");
    }

    /// Showing the dialog never writes an option value: a stored value outside the option's
    /// range (a persisted or hand-edited setting) is drawn as it is, not clamped into it.
    #[test]
    fn showing_leaves_out_of_range_option_values_untouched() {
        for (format, codec, id, value) in [
            ("mp4", "h264", "crf", OptionValue::Int(99)),
            ("seq", "exr", "dwa", OptionValue::Float(900.0)),
        ] {
            let mut s = EncodeSettings {
                format_id: format.into(),
                codec_id: codec.into(),
                output_path: "out.x".into(),
                ..Default::default()
            };
            s.option_values.insert(id.into(), value.clone());
            let mut d = EncodeDialog::with_settings(demo_schema(), s);
            assert_eq!(d.settings().get(id), Some(&value), "the setup must keep the value");
            let before = d.settings().clone();
            let ctx = egui::Context::default();
            for _ in 0..3 {
                let mut result = EncodeDialogResult::Open;
                let mut out = ctx.run_ui(Default::default(), |ui| {
                    result = d.show(ui.ctx(), &ShowConfig::default());
                });
                out.textures_delta.clear();
                assert_eq!(result, EncodeDialogResult::Open, "showing reported an outcome");
            }
            assert_eq!(*d.settings(), before, "showing changed the settings ({id})");
        }
    }
}
