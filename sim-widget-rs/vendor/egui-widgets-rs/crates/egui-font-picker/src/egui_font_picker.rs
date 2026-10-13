//! A font-family dropdown with a filter.
//!
//! # What it is and is not
//!
//! It is the combo box, the filter and the "current family" text. It does **not** enumerate
//! fonts, and that is the same decision `egui-file-field` makes about pickers: which font system
//! is authoritative belongs to the HOST. An app shapes text with something specific - cosmic-text,
//! fontdb, the platform API - and a widget that went looking for fonts by another route would
//! cheerfully offer a family the renderer cannot use. So the host passes in the names it can
//! actually render.
//!
//! # Generic families come first
//!
//! CSS-style generics (`sans-serif`, `serif`, `monospace`, …) are not fonts; they are a request
//! the shaper resolves against whatever is installed. They sort to the top because they are the
//! portable choice - a project that says `sans-serif` opens correctly on a machine that has never
//! heard of the designer's favourite face, and they are usually the default a host ships with.

#![forbid(unsafe_code)]

/// CSS generic families, in the order they are offered.
///
/// Kept here rather than left to the host: every text shaper worth the name understands these,
/// and a host that omitted them would leave the user unable to pick the portable answer.
pub const GENERIC_FAMILIES: &[&str] = &["sans-serif", "serif", "monospace", "cursive", "fantasy"];

/// The families a host can render, ready to draw.
///
/// Build it ONCE from the host's font database and keep it: enumerating a system font database is
/// far too expensive to redo per frame, and the answer only changes when fonts are installed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontList {
    families: Vec<String>,
}

impl FontList {
    /// Sorted, de-duplicated, generics first.
    ///
    /// Sorting is case-insensitive because a font list that puts `Arial` above `arial narrow` but
    /// below `Zapf` is a list nobody can scan. Duplicates are real: the same family often appears
    /// once per installed style.
    pub fn new(names: impl IntoIterator<Item = String>) -> Self {
        let mut rest: Vec<String> = names
            .into_iter()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .filter(|n| !GENERIC_FAMILIES.iter().any(|g| g.eq_ignore_ascii_case(n)))
            .collect();
        rest.sort_by_key(|n| n.to_lowercase());
        rest.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

        let mut families: Vec<String> = GENERIC_FAMILIES.iter().map(|g| (*g).to_string()).collect();
        families.extend(rest);
        Self { families }
    }

    /// Nothing but the generics - the host has not supplied a database yet.
    pub fn is_generic_only(&self) -> bool {
        self.families.len() <= GENERIC_FAMILIES.len()
    }

    /// Every offered family, generics first.
    pub fn families(&self) -> &[String] {
        &self.families
    }
}

/// How many entries before the popup grows a filter box.
///
/// Below this a filter is just a control in the way; above it, scrolling a system font list to
/// find "Roboto" is hopeless without one.
const FILTER_THRESHOLD: usize = 12;

/// A dropdown of font families.
///
/// The current value is a plain `String` and is NOT required to be in the list: a project may name
/// a font this machine does not have, and silently rewriting that to something installed would
/// lose the author's intent on the next save. Such a value shows as-is and is marked.
pub struct FontPicker<'a> {
    current: &'a mut String,
    list: &'a FontList,
    desired_width: Option<f32>,
}

impl<'a> FontPicker<'a> {
    pub fn new(current: &'a mut String, list: &'a FontList) -> Self {
        Self {
            current,
            list,
            desired_width: None,
        }
    }

    /// Width of the closed combo box.
    pub fn desired_width(mut self, w: f32) -> Self {
        self.desired_width = Some(w);
        self
    }

    /// Draw it. The response is `changed()` when a different family was chosen.
    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let id = ui.id().with("font_picker");
        // The filter is per-popup transient state, not something to persist: a filter that
        // survived the popup closing would greet the next open with a stale search.
        let mut filter: String = ui.data(|d| d.get_temp(id).unwrap_or_default());

        let known = self
            .list
            .families()
            .iter()
            .any(|f| f.eq_ignore_ascii_case(self.current));
        let label = if self.current.trim().is_empty() {
            "(default)".to_string()
        } else if known {
            self.current.clone()
        } else {
            // Named but not installed. Saying so beats showing it as if it will render.
            format!("{} (missing)", self.current)
        };

        let mut combo = egui::ComboBox::from_id_salt(id).selected_text(label);
        if let Some(w) = self.desired_width {
            combo = combo.width(w);
        }

        let mut picked: Option<String> = None;
        let mut inner = combo.show_ui(ui, |ui| {
            let many = self.list.families().len() > FILTER_THRESHOLD;
            if many {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut filter)
                        .hint_text("Filter…")
                        .desired_width(f32::INFINITY),
                );
                if r.changed() {
                    ui.data_mut(|d| d.insert_temp(id, filter.clone()));
                }
                ui.separator();
            }

            let needle = filter.trim().to_lowercase();
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for family in self.list.families() {
                        if !needle.is_empty() && !family.to_lowercase().contains(&needle) {
                            continue;
                        }
                        let selected = family.eq_ignore_ascii_case(self.current);
                        if ui.selectable_label(selected, family).clicked() {
                            picked = Some(family.clone());
                            ui.close();
                        }
                    }
                });
        });

        if let Some(family) = picked
            && family != *self.current
        {
            *self.current = family;
            inner.response.mark_changed();
        }
        inner.response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generics_lead_and_are_never_duplicated() {
        let list = FontList::new([
            "Roboto".to_string(),
            "sans-serif".to_string(),
            "Arial".to_string(),
            "SANS-SERIF".to_string(),
        ]);
        assert_eq!(&list.families()[..GENERIC_FAMILIES.len()], GENERIC_FAMILIES);
        assert_eq!(
            list.families()
                .iter()
                .filter(|f| f.eq_ignore_ascii_case("sans-serif"))
                .count(),
            1,
            "a host database that also lists a generic must not double it"
        );
    }

    /// Case-insensitive ordering, because a case-sensitive sort scatters a font list into two
    /// alphabets and nobody can scan it.
    #[test]
    fn installed_families_sort_case_insensitively() {
        let list = FontList::new([
            "zapf".to_string(),
            "Arial".to_string(),
            "arial narrow".to_string(),
        ]);
        let rest = &list.families()[GENERIC_FAMILIES.len()..];
        assert_eq!(rest, ["Arial", "arial narrow", "zapf"]);
    }

    #[test]
    fn duplicates_from_per_style_entries_collapse() {
        let list = FontList::new(vec!["Roboto".to_string(); 8]);
        let rest = &list.families()[GENERIC_FAMILIES.len()..];
        assert_eq!(rest, ["Roboto"]);
    }

    #[test]
    fn an_empty_database_still_offers_the_portable_choices() {
        let list = FontList::new(Vec::new());
        assert!(list.is_generic_only());
        assert_eq!(list.families(), GENERIC_FAMILIES);
    }
}
