//! A path text field with a Browse icon on the end, and a memory of where to start.
//!
//! # What it is and is not
//!
//! It is the field, the icon, and the last directory the user browsed from — persisted, so a
//! session that reopens lands where it left off rather than in the process's working directory.
//!
//! It does **not** open a dialog. This widget set keeps pickers out on purpose (see `egui-arbor`,
//! `egui-asset-browser`), and the reason is not taste: which picker to use is the HOST's decision.
//! playa offers in-app (`egui-file-dialog`) or native (`rfd`) in Preferences, and a widget that
//! opened its own would silently override the setting the user chose. So this answers "Browse was
//! clicked, and here is where to start", and takes back whatever was picked.

use serde::{Deserialize, Serialize};

/// What the Browse button is for — it only changes the icon and the tooltip.
///
/// The host decides what its picker actually does; this is a hint, so the button does not say
/// "file" while opening a folder chooser.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PickKind {
    #[default]
    File,
    Folder,
}

/// The part worth persisting: where to start browsing next time.
///
/// Serialised by the host alongside its own settings. Kept separate from the path itself, which
/// belongs to whatever the field is editing — a node attr, a preference, a dialog form — and is
/// nobody's business here.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFieldState {
    /// Directory the last pick came from. Empty until something is picked.
    last_dir: String,
}

impl FileFieldState {
    /// Where a picker should open.
    ///
    /// The parent of whatever the field currently holds, or else the remembered directory. A field
    /// with a path in it must not send the user back to somewhere they browsed an hour ago.
    ///
    /// Empty means "you decide", which is the honest answer when there is nothing to go on — a
    /// picker opening in the process's working directory because a widget invented one is worse
    /// than letting the host pick its own default.
    pub fn start_dir(&self, current: &str) -> String {
        let cur = current.trim();
        if !cur.is_empty()
            && let Some(parent) = std::path::Path::new(cur).parent()
            && parent.is_dir()
        {
            return parent.to_string_lossy().into_owned();
        }
        self.last_dir.clone()
    }

    /// Record where a pick came from. Call after the host's picker returns.
    pub fn remember(&mut self, picked: &str) {
        if let Some(parent) = std::path::Path::new(picked).parent() {
            self.last_dir = parent.to_string_lossy().into_owned();
        }
    }
}

/// What the user did this frame.
pub struct FileFieldResponse {
    /// The text field's own response, for `changed()` / focus / hover.
    pub response: egui::Response,
    browse: bool,
}

impl FileFieldResponse {
    /// Open your picker, starting at [`FileFieldState::start_dir`].
    pub fn browse_clicked(&self) -> bool {
        self.browse
    }

    /// The path text was edited by hand this frame.
    pub fn changed(&self) -> bool {
        self.response.changed()
    }
}

/// A path text field with a Browse icon on its right.
pub struct FileField<'a> {
    path: &'a mut String,
    state: &'a mut FileFieldState,
    kind: PickKind,
    hint: Option<String>,
    tooltip: Option<String>,
    desired_width: Option<f32>,
}

impl<'a> FileField<'a> {
    pub fn new(path: &'a mut String, state: &'a mut FileFieldState) -> Self {
        Self {
            path,
            state,
            kind: PickKind::default(),
            hint: None,
            tooltip: None,
            desired_width: None,
        }
    }

    pub fn kind(mut self, kind: PickKind) -> Self {
        self.kind = kind;
        self
    }

    /// Greyed placeholder while the field is empty — say what an empty field MEANS.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Width of the text part. Default: whatever is left after the button.
    pub fn desired_width(mut self, w: f32) -> Self {
        self.desired_width = Some(w);
        self
    }

    /// Where a picker should open for the path currently in the field.
    pub fn start_dir(&self) -> String {
        self.state.start_dir(self.path)
    }

    pub fn show(self, ui: &mut egui::Ui) -> FileFieldResponse {
        // The button is sized from the row height so the pair reads as ONE control, rather than a
        // field with a chip parked beside it.
        let btn = ui.spacing().interact_size.y;
        let gap = ui.spacing().item_spacing.x;
        let text_w = self
            .desired_width
            .unwrap_or_else(|| (ui.available_width() - btn - gap).max(24.0));

        let (icon, what) = match self.kind {
            PickKind::File => (egui_widgets_config::icons::FILE, "Choose a file"),
            PickKind::Folder => (egui_widgets_config::icons::FOLDER_OPEN, "Choose a folder"),
        };
        let tip = self.tooltip.clone().unwrap_or_else(|| what.to_string());

        let mut browse = false;
        let mut edit = egui::TextEdit::singleline(self.path).desired_width(text_w);
        if let Some(hint) = &self.hint {
            edit = edit.hint_text(hint.clone());
        }

        let response = ui
            .horizontal(|ui| {
                let r = ui.add(edit);
                if ui
                    .add_sized([btn, btn], egui::Button::new(icon))
                    .on_hover_text(tip)
                    .clicked()
                {
                    browse = true;
                }
                r
            })
            .inner;

        FileFieldResponse { response, browse }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The field's OWN path wins over the remembered directory: a field pointing somewhere should
    /// open there, not where the user happened to browse an hour ago.
    #[test]
    fn the_current_path_beats_the_remembered_directory() {
        let mut st = FileFieldState::default();
        st.remember("/somewhere/else/old.mov");
        let here = std::env::current_dir().expect("cwd");
        let file = here.join("probe.mov");
        let start = st.start_dir(&file.to_string_lossy());
        assert_eq!(
            std::path::Path::new(&start),
            here,
            "a filled field opens beside its own file"
        );
    }

    /// With an empty field, the remembered directory is all there is.
    #[test]
    fn an_empty_field_falls_back_to_the_last_directory() {
        let mut st = FileFieldState::default();
        st.remember("/plates/shot_010/take.mov");
        let start = st.start_dir("");
        assert!(start.ends_with("shot_010"), "{start}");
    }

    /// Nothing to go on means EMPTY, not a guess.
    #[test]
    fn with_nothing_known_it_says_so() {
        let st = FileFieldState::default();
        assert!(st.start_dir("").is_empty());
        assert!(st.start_dir("   ").is_empty());
    }

    /// A path whose parent does not exist falls back rather than pointing at a missing directory.
    #[test]
    fn a_parent_that_is_not_there_falls_back() {
        let mut st = FileFieldState::default();
        st.remember("/plates/shot_010/take.mov");
        let start = st.start_dir("/no/such/directory/anywhere/file.mov");
        assert!(start.ends_with("shot_010"), "{start}");
    }

    /// Round-trips, because the host persists it with its own settings.
    #[test]
    fn state_survives_serde() {
        let mut st = FileFieldState::default();
        st.remember("/a/b/c.exr");
        let json = serde_json::to_string(&st).expect("to json");
        let back: FileFieldState = serde_json::from_str(&json).expect("from json");
        assert_eq!(back, st);
    }
}
