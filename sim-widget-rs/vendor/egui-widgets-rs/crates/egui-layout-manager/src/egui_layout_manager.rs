//! A generic, app-agnostic **named-layout** save/restore widget for egui 0.34.
//!
//! Manages a set of named UI-layout snapshots for any egui app that can turn
//! its own window/dock arrangement into a text blob (typically
//! `serde_json::to_string(&egui_dock::DockState)`). The widget renders a
//! combo-box of saved names plus a Save / Delete / Rename toolbar and reports
//! restore requests back to the host.
//!
//! ## Why the layout is an opaque `String`
//! The widget is deliberately decoupled from `egui_dock` (and from any concrete
//! app layout type). It never serializes or interprets a layout itself — it
//! only stores and hands back **opaque blobs**:
//! * on **Save**, the host passes IN the current serialized layout
//!   (`current_blob`) and the widget files it under a name,
//! * on **Restore**, the widget returns the stored blob for the chosen name via
//!   [`LayoutAction::Restore`] and the host applies it however it likes.
//!
//! This keeps the host the single owner of "what a layout is", so the same
//! widget serves an `egui_dock` app, a custom panel system, or anything else
//! that round-trips to a `String`.
//!
//! ## State ownership
//! * [`LayoutStore`] holds the persistent data (the `name -> blob` map + the
//!   currently-selected name). It is plain `serde` data so the host can embed it
//!   in its own settings and reload the whole named set later.
//! * [`LayoutManager`] holds only the *ephemeral* UI state of the rename dialog.
//!   Keep one per toolbar (e.g. in your app struct); it is `Default`-able and
//!   need not be persisted.
//!
//! ## Usage
//! ```no_run
//! # use egui_layout_manager::{LayoutStore, LayoutManager, LayoutAction};
//! # fn ui_demo(ui: &mut egui::Ui, store: &mut LayoutStore, mgr: &mut LayoutManager) {
//! // Host serializes ITS current layout to a String (e.g. serde_json of a DockState).
//! let current_blob: String = /* serde_json::to_string(&dock_state) */ String::new();
//! if let Some(LayoutAction::Restore(blob)) = mgr.show(ui, store, &current_blob) {
//!     // Host applies the chosen layout: serde_json::from_str::<DockState>(&blob)...
//!     let _ = blob;
//! }
//! # }
//! ```
//!
//! Adapted from playa's named dock-layout feature (`playa-app`'s
//! `capture/apply/select/create/delete/rename_layout` + the timeline toolbar
//! combo) with the `AppSettings` / `egui_dock` / event-bus coupling removed.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Persistent named-layout store: a `name -> serialized blob` map plus the name
/// of the currently-selected layout.
///
/// The blob is an opaque `String` the host produced (and will consume) — the
/// store never inspects it. Insertion order is preserved ([`IndexMap`]) so the
/// combo-box lists layouts in the order they were created. Implements `serde`
/// so the host can persist the whole set in its own settings file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutStore {
    /// Saved layouts, keyed by user-facing name, in creation order.
    layouts: IndexMap<String, String>,
    /// Name of the selected layout, or empty when none is selected.
    current: String,
}

impl LayoutStore {
    /// Empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of saved layouts.
    pub fn len(&self) -> usize {
        self.layouts.len()
    }

    /// `true` when no layout is saved.
    pub fn is_empty(&self) -> bool {
        self.layouts.is_empty()
    }

    /// Saved layout names in creation order.
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.layouts.keys()
    }

    /// Name of the currently-selected layout (empty when none).
    pub fn current(&self) -> &str {
        &self.current
    }

    /// Save `blob` under `name` (overwriting any existing entry with that name)
    /// and make it the selected layout. Mirrors playa's `create_layout` but with
    /// a caller-supplied blob instead of capturing app state internally.
    pub fn save(&mut self, name: impl Into<String>, blob: impl Into<String>) {
        let name = name.into();
        self.layouts.insert(name.clone(), blob.into());
        self.current = name;
    }

    /// Auto-name (`"Layout 1"`, `"Layout 2"`, …), save `blob`, and select it.
    /// Returns the generated name. Used by the toolbar's Save button so a click
    /// captures the current layout without prompting for a name.
    pub fn save_auto(&mut self, blob: impl Into<String>) -> String {
        let mut n = 1;
        let name = loop {
            let candidate = format!("Layout {n}");
            if !self.layouts.contains_key(&candidate) {
                break candidate;
            }
            n += 1;
        };
        self.save(name.clone(), blob);
        name
    }

    /// Borrow the blob stored under `name`, if present.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.layouts.get(name).map(String::as_str)
    }

    /// Select `name` and return its blob for the host to apply. Returns `None`
    /// (leaving the selection unchanged) when `name` is unknown.
    pub fn select(&mut self, name: &str) -> Option<String> {
        let blob = self.layouts.get(name)?.clone();
        self.current = name.to_string();
        Some(blob)
    }

    /// Delete `name`. Clears the selection if the deleted layout was selected.
    /// Returns `true` when an entry was removed.
    pub fn delete(&mut self, name: &str) -> bool {
        // `shift_remove` preserves the order of the remaining entries (unlike
        // `swap_remove`), so the combo-box list stays stable after a delete.
        if self.layouts.shift_remove(name).is_some() {
            if self.current == name {
                self.current.clear();
            }
            true
        } else {
            false
        }
    }

    /// Rename `old` to `new`, keeping its blob and list position. Updates the
    /// selection if the renamed layout was selected. No-op (returns `false`) if
    /// `old` is unknown or `new` already exists.
    pub fn rename(&mut self, old: &str, new: &str) -> bool {
        if old == new {
            return false;
        }
        if self.layouts.contains_key(new) {
            return false;
        }
        let Some(index) = self.layouts.get_index_of(old) else {
            return false;
        };
        // Remove the old key then move the freshly-inserted new key back to the
        // old slot so rename does not reorder the list.
        let blob = self.layouts.shift_remove(old).unwrap_or_default();
        let (new_index, _) = self.layouts.insert_full(new.to_string(), blob);
        self.layouts.move_index(new_index, index);
        if self.current == old {
            self.current = new.to_string();
        }
        true
    }
}

/// What the user asked the host to do. The store handles Save / Delete / Rename
/// internally (they only mutate the store), so the sole action the host must act
/// on is applying a restored layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutAction {
    /// The user selected a layout; the host should apply this blob (its own
    /// layout type deserialized from the `String`).
    Restore(String),
}

/// Glyphs for the three toolbar buttons. Defaults are the shared
/// [`egui_widgets_config::icons`] phosphor glyphs (add / trash / rename) so the
/// toolbar matches every other icon-driven widget in the set; the host registers
/// the phosphor font once. Override the `String`s for a font-free host.
#[derive(Debug, Clone)]
pub struct LayoutGlyphs {
    /// Save/create button label.
    pub save: String,
    /// Delete button label.
    pub delete: String,
    /// Rename button label.
    pub rename: String,
}

impl Default for LayoutGlyphs {
    fn default() -> Self {
        use egui_widgets_config::icons;
        Self {
            save: icons::ADD.to_string(),
            delete: icons::TRASH.to_string(),
            rename: icons::RENAME.to_string(),
        }
    }
}

/// The named-layout toolbar widget. Holds only the ephemeral rename-dialog state
/// (the persistent data lives in [`LayoutStore`]).
#[derive(Debug, Clone, Default)]
pub struct LayoutManager {
    /// Button glyphs (see [`LayoutGlyphs`]).
    pub glyphs: LayoutGlyphs,
    /// Combo-box width in points.
    combo_width: f32,
    /// Whether the rename dialog window is open.
    rename_open: bool,
    /// The name being renamed (needed to call [`LayoutStore::rename`]).
    rename_old: String,
    /// The editable rename buffer.
    rename_buf: String,
}

impl LayoutManager {
    /// New manager with default glyphs and a 120pt combo.
    pub fn new() -> Self {
        Self {
            combo_width: 120.0,
            ..Default::default()
        }
    }

    /// Override the combo-box width (points).
    pub fn with_combo_width(mut self, width: f32) -> Self {
        self.combo_width = width;
        self
    }

    /// Override the toolbar glyphs.
    pub fn with_glyphs(mut self, glyphs: LayoutGlyphs) -> Self {
        self.glyphs = glyphs;
        self
    }

    /// Render the combo + Save/Delete/Rename toolbar (and the rename dialog when
    /// open) inline in `ui`.
    ///
    /// * `store` — the persistent named-layout set; mutated in place on
    ///   Save/Delete/Rename/Select.
    /// * `current_blob` — the host's *current* serialized layout, filed away when
    ///   the user clicks Save.
    ///
    /// Returns [`LayoutAction::Restore`] when the user picked a layout to apply,
    /// otherwise `None`.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut LayoutStore,
        current_blob: &str,
    ) -> Option<LayoutAction> {
        // `combo_width` is 0.0 for a `Default`-constructed manager; fall back to
        // a sane width so `LayoutManager::default()` also renders reasonably.
        let combo_width = if self.combo_width > 0.0 {
            self.combo_width
        } else {
            120.0
        };
        let mut action = None;

        ui.horizontal(|ui| {
            let has_selection = !store.current().is_empty();
            let display = if has_selection {
                store.current().to_string()
            } else {
                "(none)".to_string()
            };

            // Combo of saved names — selecting one restores it.
            egui::ComboBox::from_id_salt("layout_selector")
                .selected_text(display)
                .width(combo_width)
                .show_ui(ui, |ui| {
                    // Clone the names so we can call `store.select` (a `&mut`
                    // borrow) inside the loop without aliasing the iterator.
                    let names: Vec<String> = store.names().cloned().collect();
                    for name in names {
                        let selected = store.current() == name;
                        if ui.selectable_label(selected, &name).clicked()
                            && let Some(blob) = store.select(&name)
                        {
                            action = Some(LayoutAction::Restore(blob));
                        }
                    }
                });

            // Save: capture the host's current layout under an auto name.
            if ui
                .button(&self.glyphs.save)
                .on_hover_text("Save current layout")
                .clicked()
            {
                store.save_auto(current_blob.to_string());
            }

            // Delete: remove the selected layout.
            if ui
                .add_enabled(has_selection, egui::Button::new(&self.glyphs.delete))
                .on_hover_text("Delete current layout")
                .clicked()
            {
                let name = store.current().to_string();
                store.delete(&name);
            }

            // Rename: open the inline dialog seeded with the current name.
            if ui
                .add_enabled(has_selection, egui::Button::new(&self.glyphs.rename))
                .on_hover_text("Rename current layout")
                .clicked()
            {
                self.rename_open = true;
                self.rename_old = store.current().to_string();
                self.rename_buf = store.current().to_string();
            }
        });

        self.rename_dialog(ui, store);
        action
    }

    /// The rename dialog window (only shown while `rename_open`). Commits via
    /// [`LayoutStore::rename`] on OK / Enter.
    fn rename_dialog(&mut self, ui: &mut egui::Ui, store: &mut LayoutStore) {
        if !self.rename_open {
            return;
        }
        let mut close = false;
        let mut commit = false;

        egui::Window::new("Rename Layout")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    let response = ui.text_edit_singleline(&mut self.rename_buf);
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        commit = true;
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        commit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });

        if commit {
            let new_name = self.rename_buf.trim().to_string();
            if !new_name.is_empty() {
                store.rename(&self.rename_old, &new_name);
            }
            close = true;
        }
        if close {
            self.rename_open = false;
            self.rename_old.clear();
            self.rename_buf.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Save → restore → delete round-trip on the pure store (no UI).
    #[test]
    fn save_restore_delete_roundtrip() {
        let mut store = LayoutStore::new();
        assert!(store.is_empty());

        store.save("A", "blob-a");
        store.save("B", "blob-b");
        assert_eq!(store.len(), 2);
        assert_eq!(store.current(), "B"); // last saved becomes current

        // Restore returns the exact blob and updates the selection.
        assert_eq!(store.select("A").as_deref(), Some("blob-a"));
        assert_eq!(store.current(), "A");
        assert_eq!(store.select("missing"), None);
        assert_eq!(store.current(), "A"); // unchanged on unknown name

        // Delete the selected layout clears the selection.
        assert!(store.delete("A"));
        assert!(store.current().is_empty());
        assert_eq!(store.len(), 1);
        assert!(!store.delete("A")); // already gone
    }

    /// Rename keeps the blob and list order, and follows the selection.
    #[test]
    fn rename_preserves_blob_order_and_selection() {
        let mut store = LayoutStore::new();
        store.save("one", "b1");
        store.save("two", "b2");
        store.select("one");

        assert!(store.rename("one", "uno"));
        assert_eq!(store.current(), "uno");
        assert_eq!(store.get("uno"), Some("b1"));
        assert_eq!(store.get("one"), None);
        // Order preserved: "uno" still first.
        let names: Vec<&String> = store.names().collect();
        assert_eq!(names, vec!["uno", "two"]);

        // Rename onto an existing name is refused.
        assert!(!store.rename("uno", "two"));
        // Renaming an unknown name is a no-op.
        assert!(!store.rename("ghost", "x"));
    }

    /// Auto-naming skips names already in use.
    #[test]
    fn save_auto_generates_unique_names() {
        let mut store = LayoutStore::new();
        assert_eq!(store.save_auto("a"), "Layout 1");
        assert_eq!(store.save_auto("b"), "Layout 2");
        store.save("Layout 3", "c"); // occupy the next slot manually
        assert_eq!(store.save_auto("d"), "Layout 4");
    }

    /// The store round-trips through serde (host persistence).
    #[test]
    fn serde_roundtrip() {
        let mut store = LayoutStore::new();
        store.save("A", "blob-a");
        store.save("B", "blob-b");
        store.select("A");

        let json = serde_json::to_string(&store).expect("serialize");
        let back: LayoutStore = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.current(), "A");
        assert_eq!(back.get("A"), Some("blob-a"));
        assert_eq!(back.get("B"), Some("blob-b"));
        let names: Vec<&String> = back.names().collect();
        assert_eq!(names, vec!["A", "B"]);
    }
}
