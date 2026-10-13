//! Named presets over attribute rows — the [`egui_preset`] bar wired to this
//! grid's [`AttrValue`].
//!
//! A preset here is simply a snapshot of `(key, value)` rows, so recalling one
//! produces exactly what [`render_grid`](crate::render_grid) produces: a
//! `Vec<(String, AttrValue)>` of changed rows. The host feeds it into the SAME
//! apply path it already uses for user edits — there is no second way for a
//! value to reach the model, and therefore no second way for it to go wrong.
//!
//! Recall is all-or-nothing: the stored value replaces the live one. Partial
//! application used to live here as a per-`AttrValue` blend, and it went out with
//! the bar that offered it — a control nobody reached for, paid for with a whole
//! blending vocabulary (what is half a string? half an enum?).

use crate::{AttrField, AttrValue};
pub use egui_preset::{Preset, PresetAction, PresetStore};

/// A preset store over attribute snapshots.
pub type AttrPresets = PresetStore<Vec<(String, AttrValue)>>;

/// What the bar did this frame.
///
/// The two outcomes are reported separately because the host does different
/// things with them: `changes` go into its model, while `store_changed` says the
/// PRESET LIST itself must be written to wherever the host persists it. Folding
/// them into one flag loses saved presets — a Save alters no attribute, so a host
/// watching only `changes` never learns it has something new to write, and the
/// preset is gone at the next start.
#[derive(Default)]
pub struct PresetOutcome {
    /// Rows a recall changed, in the same shape [`render_grid`](crate::render_grid) returns.
    pub changes: Vec<(String, AttrValue)>,
    /// The preset list was created, overwritten or deleted this frame.
    pub store_changed: bool,
}

/// Capture the editable rows of `fields` as a preset payload.
///
/// [`AttrValue::Label`] rows are skipped: the grid defines them as read-only
/// display text, so storing them would create a preset that promises to restore
/// something the grid will never write back.
pub fn snapshot(fields: &[AttrField]) -> Vec<(String, AttrValue)> {
    fields
        .iter()
        .filter(|f| !matches!(f.value, AttrValue::Label(_)))
        .map(|f| (f.key.clone(), f.value.clone()))
        .collect()
}

/// Draw the preset bar for an attribute grid.
///
/// Save takes a snapshot of `fields` (see [`snapshot`]); Load returns the rows a
/// recall changed, in the same shape [`render_grid`](crate::render_grid) returns,
/// so preset values reach the model through the host's existing apply path.
///
/// Only rows that actually differ from the live value come back, so recalling a
/// preset that matches the current state reads as "no edit" rather than as a
/// rewrite of every attribute.
///
/// Keys present in the preset but absent from `fields` are ignored: a preset
/// outlives the schema that produced it, and a stale key is not a reason to
/// refuse the rest.
pub fn attr_preset_bar(
    ui: &mut egui::Ui,
    store: &mut AttrPresets,
    fields: &[AttrField],
    id_salt: &str,
) -> PresetOutcome {
    let Some(action) = egui_preset::preset_bar(ui, store, id_salt) else {
        return PresetOutcome::default();
    };

    match action {
        PresetAction::Load(name) => {
            let Some(payload) = store.get(&name) else {
                return PresetOutcome::default();
            };
            PresetOutcome {
                changes: fields
                    .iter()
                    .filter_map(|f| {
                        let stored = payload.iter().find(|(k, _)| *k == f.key).map(|(_, v)| v)?;
                        (stored != &f.value).then(|| (f.key.clone(), stored.clone()))
                    })
                    .collect(),
                store_changed: false,
            }
        }
        PresetAction::Save(name) => {
            store.upsert(name.clone(), snapshot(fields));
            store.selected = name;
            PresetOutcome {
                store_changed: true,
                ..Default::default()
            }
        }
        PresetAction::Delete(name) => {
            store.remove(&name);
            PresetOutcome {
                store_changed: true,
                ..Default::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Vec<AttrField> {
        vec![
            AttrField::new("size", AttrValue::Float(10.0)),
            AttrField::new("count", AttrValue::Int(0)),
            AttrField::new("mode", AttrValue::Str("normal".into())),
            AttrField::new("path", AttrValue::Label("read only".into())),
        ]
    }

    #[test]
    fn snapshot_skips_read_only_labels() {
        let snap = snapshot(&fields());
        assert_eq!(snap.len(), 3);
        assert!(!snap.iter().any(|(k, _)| k == "path"));
    }

    /// A snapshot round-trips through the store unchanged: what was captured is
    /// exactly what a recall would hand back.
    #[test]
    fn snapshot_round_trips_through_the_store() {
        let mut store: AttrPresets = AttrPresets::default();
        store.upsert("look", snapshot(&fields()));

        let stored = store.get("look").expect("slot missing");
        assert_eq!(stored.len(), 3);
        assert_eq!(
            stored.iter().find(|(k, _)| k == "mode").map(|(_, v)| v),
            Some(&AttrValue::Str("normal".into()))
        );
    }
}
