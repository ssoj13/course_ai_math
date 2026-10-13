//! Generic named-preset bar — "save / recall / delete named bundles of state".
//!
//! The payload type `T` is whatever the caller wants to snapshot (a parameter
//! struct, a look, a set of attribute rows, …). This widget owns ONLY the named
//! slots, the current selection and the Save-As name buffer, plus the row UI.
//! The domain work — how to *capture* the live state into a `T` and how to
//! *apply* a recalled `T` — stays with the caller: [`preset_bar`] returns a
//! [`PresetAction`] that the caller performs AFTER the call. That keeps the
//! borrow of the live state out of the widget entirely, and keeps this crate
//! free of any application type.
//!
//! Built-in slots and user snapshots share ONE combo, and recall is *weighted*
//! (100 / 75 / 50 / 25 %) as in Maya: a preset reads as a nudge toward a look
//! rather than an all-or-nothing switch. The widget does not blend anything —
//! only the host knows which of its fields are continuous and which are
//! discrete. `egui-attr-grid` ships a ready blend for `AttrValue` rows.
//!
//! Gathered from gitnexus-rs (`crates/egui-widgets/src/preset.rs`).

use egui_widgets_config::icons;
use serde::{Deserialize, Serialize};

/// One named preset slot holding a payload `T`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preset<T> {
    /// Display name (unique within the store).
    pub name: String,
    /// The captured state this slot restores.
    pub payload: T,
}

/// Persisted collection of named presets + the current selection + a scratch
/// Save-As name buffer. Drop this on any serde-persisted panel state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PresetStore<T> {
    /// All saved slots, in display order.
    pub presets: Vec<Preset<T>>,
    /// Name of the selected slot (empty = none).
    #[serde(default)]
    pub selected: String,
    /// Scratch buffer for the Save-As name field (not meaningfully persisted).
    #[serde(skip)]
    pub name_buf: String,
}

impl<T> Default for PresetStore<T> {
    fn default() -> Self {
        Self {
            presets: Vec::new(),
            selected: String::new(),
            name_buf: String::new(),
        }
    }
}

impl<T> PresetStore<T> {
    /// Insert or overwrite a slot by name, preserving order on overwrite.
    pub fn upsert(&mut self, name: impl Into<String>, payload: T) {
        let name = name.into();
        if let Some(slot) = self.presets.iter_mut().find(|p| p.name == name) {
            slot.payload = payload;
        } else {
            self.presets.push(Preset { name, payload });
        }
    }

    /// Ensure a built-in slot exists at the FRONT, regenerated from `payload`
    /// (any slot of the same name is replaced).
    ///
    /// Use for a canonical "default" baseline that must always be present AND
    /// current: a persisted store from an older build may hold a stale copy of
    /// it, and silently keeping that would make "default" mean whatever the
    /// defaults were the day the file was written.
    pub fn ensure_builtin(&mut self, name: &str, payload: T) {
        self.presets.retain(|p| p.name != name);
        self.presets.insert(
            0,
            Preset {
                name: name.to_string(),
                payload,
            },
        );
    }

    /// Borrow a payload by name.
    pub fn get(&self, name: &str) -> Option<&T> {
        self.presets
            .iter()
            .find(|p| p.name == name)
            .map(|p| &p.payload)
    }

    /// Remove a slot by name; clears the selection if it pointed there.
    pub fn remove(&mut self, name: &str) {
        self.presets.retain(|p| p.name != name);
        if self.selected == name {
            self.selected.clear();
        }
    }
}

/// What the user asked the preset bar to do this frame.
///
/// The caller performs the domain work: applying a recalled payload, or taking a
/// snapshot of the live state to store.
#[derive(Clone, Debug, PartialEq)]
pub enum PresetAction {
    /// Apply the preset with this name.
    Load(String),
    /// Create or overwrite a slot with this (trimmed, non-empty) name from the
    /// live state.
    Save(String),
    /// Delete the slot with this name.
    Delete(String),
}

/// Draw the preset bar: one row, three controls.
///
/// ```text
/// [ name................ ][v][+][-]
/// ```
///
/// - the TEXT FIELD is the name;
/// - the ARROW lists every stored preset; picking one LOADS it immediately and
///   puts its name in the field. No separate Load button: choosing a preset is
///   the act of applying it;
/// - PLUS stores the live state under the name in the field, creating it or
///   overwriting it. Enter in the field does the same - the button is there so
///   the action is visible, the key so it is quick;
/// - MINUS deletes the preset named in the field, and is disabled when no such
///   preset exists, so it cannot pretend to act.
///
/// Edits made after picking a preset do NOT flow back into it. A preset changes
/// only when you save it, so a session of experimenting cannot quietly rewrite
/// the look you had stored.
///
/// The previous bar spread the same jobs over a combo, four weight buttons, a
/// Save, a Del, a second text field and a Save As - two rows to do what fits in
/// one, with two different ways to save.
///
/// Returns the requested [`PresetAction`] for the caller to perform after this
/// returns, so the caller's own state is not borrowed across the widget.
/// `id_salt` keeps multiple bars in one window independent.
pub fn preset_bar<T>(
    ui: &mut egui::Ui,
    store: &mut PresetStore<T>,
    id_salt: &str,
) -> Option<PresetAction> {
    let mut action: Option<PresetAction> = None;

    ui.horizontal(|ui| {
        // Поле имени занимает то, что осталось от двух кнопок справа: имя пресета
        // читать важнее, чем видеть их с запасом.
        let w = (ui.available_width() - 84.0).max(60.0);
        let field = ui.add(
            egui::TextEdit::singleline(&mut store.name_buf)
                .desired_width(w)
                .hint_text("preset name"),
        );
        let name = store.name_buf.trim().to_string();

        // `lost_focus` вместе с нажатым Enter — идиома egui для «ввод завершён».
        // Просто `lost_focus` срабатывал бы и при клике мимо поля.
        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !name.is_empty() {
            action = Some(PresetAction::Save(name.clone()));
        }

        // Стрелка списка. Пусто — кнопка неактивна: выпадающий список без строк
        // выглядит как поломка.
        let has_any = !store.presets.is_empty();
        let mut picked: Option<String> = None;
        ui.add_enabled_ui(has_any, |ui| {
            egui::ComboBox::from_id_salt(id_salt)
                .selected_text("")
                .width(18.0)
                .show_ui(ui, |ui| {
                    for p in store.presets.iter() {
                        if ui
                            .selectable_label(store.selected == p.name, &p.name)
                            .clicked()
                        {
                            picked = Some(p.name.clone());
                        }
                    }
                });
        });
        if let Some(n) = picked {
            store.name_buf = n.clone();
            store.selected = n.clone();
            action = Some(PresetAction::Load(n));
        }

        let exists = store.presets.iter().any(|p| p.name == name);
        if ui
            .add_enabled(!name.is_empty(), egui::Button::new(icons::ADD))
            .on_hover_text(if exists {
                "Overwrite this preset with the current settings (Enter)"
            } else {
                "Save the current settings under this name (Enter)"
            })
            .on_disabled_hover_text("Type a name first")
            .clicked()
        {
            action = Some(PresetAction::Save(name.clone()));
        }
        if ui
            .add_enabled(exists, egui::Button::new(icons::REMOVE))
            .on_hover_text("Delete this preset")
            .on_disabled_hover_text("No preset with this name")
            .clicked()
        {
            action = Some(PresetAction::Delete(name));
        }
    });

    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_overwrites_in_place() {
        let mut s: PresetStore<i32> = PresetStore::default();
        s.upsert("a", 1);
        s.upsert("b", 2);
        s.upsert("a", 9);
        assert_eq!(s.presets.len(), 2);
        assert_eq!(s.get("a"), Some(&9));
        // Order is display order and must survive an overwrite.
        assert_eq!(s.presets[0].name, "a");
    }

    /// A built-in goes to the front and is REGENERATED, not merely inserted:
    /// a store restored from an older build carries a stale copy of it.
    #[test]
    fn builtin_is_regenerated_at_the_front() {
        let mut s: PresetStore<i32> = PresetStore::default();
        s.upsert("user", 1);
        s.upsert("default", 111); // stale copy, as if restored from disk
        s.ensure_builtin("default", 42);
        assert_eq!(s.presets[0].name, "default");
        assert_eq!(s.get("default"), Some(&42));
        assert_eq!(s.presets.len(), 2, "no duplicate slot: {:?}", s.presets);
    }

    #[test]
    fn remove_clears_a_selection_pointing_at_it() {
        let mut s: PresetStore<i32> = PresetStore::default();
        s.upsert("a", 1);
        s.selected = "a".to_string();
        s.remove("a");
        assert!(s.presets.is_empty());
        assert!(s.selected.is_empty(), "selection left dangling");
    }

    /// The scratch name buffer must NOT come back from disk — it is UI noise,
    /// and a restored half-typed name in the field looks like a bug.
    #[test]
    fn store_round_trips_without_the_scratch_buffer() {
        let mut s: PresetStore<i32> = PresetStore::default();
        s.upsert("a", 1);
        s.selected = "a".to_string();
        s.name_buf = "half typed".to_string();

        let json = serde_json::to_string(&s).expect("serialise");
        let back: PresetStore<i32> = serde_json::from_str(&json).expect("deserialise");

        assert_eq!(back.get("a"), Some(&1));
        assert_eq!(back.selected, "a");
        assert!(back.name_buf.is_empty(), "scratch buffer was persisted");
    }
}
