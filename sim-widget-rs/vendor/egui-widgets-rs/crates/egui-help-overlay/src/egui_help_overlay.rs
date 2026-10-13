//! A translucent, multi-column keyboard-shortcut / help overlay for egui.
//!
//! Build a few [`HelpSection`]s (each a title + `[key | description]` rows) and
//! drop them into [`help_overlay`] for a floating cheat-sheet card, or render a
//! single [`help_section`] inline. The widget is generic — it owns no key
//! bindings; the caller supplies the content (often `const` tables).
//!
//! (Gathered from playa, whose app-specific key tables were dropped so the crate
//! stays a pure layout/visual.)

/// A single help row: a key/chord label and what it does.
#[derive(Clone, Copy, Debug)]
pub struct HelpEntry {
    pub key: &'static str,
    pub desc: &'static str,
}

impl HelpEntry {
    pub const fn new(key: &'static str, desc: &'static str) -> Self {
        Self { key, desc }
    }
}

/// A titled group of [`HelpEntry`] rows (e.g. "PLAYBACK").
#[derive(Clone, Copy, Debug)]
pub struct HelpSection<'a> {
    pub title: &'a str,
    pub entries: &'a [HelpEntry],
}

impl<'a> HelpSection<'a> {
    pub fn new(title: &'a str, entries: &'a [HelpEntry]) -> Self {
        Self { title, entries }
    }
}

/// Base key-column width (px) at the default body font; scaled up with the font
/// in [`help_section`] so a larger font doesn't clip the key labels.
const KEY_COL_W: f32 = 90.0;

/// Render one titled section: a greyed title, then `[key | desc]` rows with the
/// key column fixed-width so descriptions align.
pub fn help_section(ui: &mut egui::Ui, title: &str, entries: &[HelpEntry]) {
    let tok = egui_widgets_config::WidgetsConfig::get(ui);
    let sem = egui_widgets_config::semantic(ui);
    // 12 pt body label → md font bucket. The key column's width + row height scale
    // with the chosen font (base = the default md size) so a bigger font neither
    // clips the key labels nor overflows the fixed column.
    let font = egui_widgets_config::prop(tok.fonts.md);
    let scale = (tok.fonts.md / egui_widgets_config::Fonts::default().md).max(1.0);
    let key_col_w = KEY_COL_W * scale;
    let key_row_h = (tok.fonts.md + 1.0).max(14.0);
    // Translucent near-white / gold key tint: no clean semantic slot, left as-is.
    let text_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 220);
    let key_color = egui::Color32::from_rgb(255, 200, 100);
    // Greyed section title tracks the host theme's weak text colour.
    let section_color = sem.weak;

    ui.label(
        egui::RichText::new(title)
            .font(font.clone())
            .color(section_color),
    );
    ui.add_space(2.0);
    for e in entries {
        ui.horizontal(|ui| {
            ui.add_sized(
                [key_col_w, key_row_h],
                egui::Label::new(
                    egui::RichText::new(e.key)
                        .font(font.clone())
                        .color(key_color),
                ),
            );
            ui.label(
                egui::RichText::new(e.desc)
                    .font(font.clone())
                    .color(text_color),
            );
        });
    }
    ui.add_space(6.0);
}

/// Render `sections` into a translucent card, distributed round-robin across
/// `columns` vertical columns (clamped to `1..=sections.len()`).
pub fn help_overlay(ui: &mut egui::Ui, sections: &[HelpSection], columns: usize) {
    let columns = columns.clamp(1, sections.len().max(1));
    egui::Frame::NONE
        .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200))
        .inner_margin(12.0)
        .corner_radius(6.0)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for col in 0..columns {
                    ui.vertical(|ui| {
                        ui.set_width(230.0);
                        // Round-robin so columns stay balanced regardless of count.
                        for (i, s) in sections.iter().enumerate() {
                            if i % columns == col {
                                help_section(ui, s.title, s.entries);
                            }
                        }
                    });
                    if col + 1 < columns {
                        ui.add_space(16.0);
                    }
                }
            });
        });
}
