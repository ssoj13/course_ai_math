//! egui tile picker for [`ClipStore`](super::ClipStore).

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{ColorImage, Context, RichText, Sense, TextureHandle, Ui, Vec2};

use super::store::ClipStore;

/// Outcome of one picker frame.
#[derive(Debug, Clone, Default)]
pub enum ClipPickerOutcome {
    #[default]
    None,
    Closed,
    Picked(PathBuf),
    Deleted(u32),
}

/// Ephemeral UI state for the clip picker window.
#[derive(Default)]
pub struct ClipPickerState {
    pub open: bool,
    pub selected: Option<u32>,
    textures: HashMap<u32, TextureHandle>,
}

impl ClipPickerState {
    pub fn open_picker(&mut self) {
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.selected = None;
    }
}

/// Draw the floating clip picker. Host owns when to call this (typically every frame).
pub fn show_clip_picker(
    ctx: &Context,
    store: &ClipStore,
    state: &mut ClipPickerState,
) -> ClipPickerOutcome {
    if !state.open {
        return ClipPickerOutcome::None;
    }

    let mut outcome = ClipPickerOutcome::None;
    let mut open = state.open;

    egui::Window::new("Image Clipboard")
        .open(&mut open)
        .default_size([480.0, 360.0])
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            outcome = draw_picker_body(ui, store, state);
        });

    if !open && state.open {
        state.open = false;
        if matches!(outcome, ClipPickerOutcome::None) {
            outcome = ClipPickerOutcome::Closed;
        }
    } else {
        state.open = open;
    }

    outcome
}

/// Draw the picker into an arbitrary `Ui` (embedded panel).
pub fn show_clip_picker_ui(
    ui: &mut Ui,
    store: &ClipStore,
    state: &mut ClipPickerState,
) -> ClipPickerOutcome {
    draw_picker_body(ui, store, state)
}

fn draw_picker_body(
    ui: &mut Ui,
    store: &ClipStore,
    state: &mut ClipPickerState,
) -> ClipPickerOutcome {
    let mut outcome = ClipPickerOutcome::None;

    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} clip(s)", store.list().len())).weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                state.open = false;
                outcome = ClipPickerOutcome::Closed;
            }
            if let Some(id) = state.selected
                && ui.button("Delete").clicked()
            {
                outcome = ClipPickerOutcome::Deleted(id);
            }
            if let Some(id) = state.selected
                && let Some(entry) = store.get(id)
                && ui.button("Pick").clicked()
            {
                outcome = ClipPickerOutcome::Picked(entry.path(store.root()));
            }
        });
    });
    ui.separator();

    if store.is_empty() {
        ui.weak("Clipboard empty — capture with Shift+F5.");
        return outcome;
    }

    let tile = Vec2::splat(96.0);
    let entries: Vec<_> = store.list().to_vec();
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for entry in &entries {
                let selected = state.selected == Some(entry.id);
                let tex = ensure_thumb(ui.ctx(), store, state, entry.id);

                let (rect, response) =
                    ui.allocate_exact_size(tile + Vec2::new(0.0, 18.0), Sense::click());
                let image_rect = egui::Rect::from_min_size(rect.min, tile);

                if let Some(tex) = tex {
                    ui.painter().image(
                        tex.id(),
                        image_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                } else {
                    ui.painter()
                        .rect_filled(image_rect, 2.0, egui::Color32::from_gray(40));
                }

                if selected {
                    ui.painter().rect_stroke(
                        image_rect.expand(1.0),
                        2.0,
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(80, 160, 255)),
                        egui::StrokeKind::Outside,
                    );
                }

                let label = format!("{:04}.{}", entry.id, entry.format.extension());
                ui.painter().text(
                    egui::pos2(rect.min.x + 2.0, rect.max.y - 14.0),
                    egui::Align2::LEFT_TOP,
                    label,
                    egui::FontId::proportional(11.0),
                    ui.visuals().text_color(),
                );

                if response.clicked() {
                    state.selected = Some(entry.id);
                }
                if response.double_clicked() {
                    outcome = ClipPickerOutcome::Picked(entry.path(store.root()));
                }
                response.on_hover_text(entry.path(store.root()).display().to_string());
            }
        });
    });

    outcome
}

fn ensure_thumb(
    ctx: &Context,
    store: &ClipStore,
    state: &mut ClipPickerState,
    id: u32,
) -> Option<TextureHandle> {
    if let Some(t) = state.textures.get(&id) {
        return Some(t.clone());
    }
    let entry = store.get(id)?;
    let thumb_path = entry.thumb_path(store.root()).or_else(|| {
        // Fall back to full image for JPEG/PNG if thumb missing.
        if entry.format.is_ai_friendly() {
            Some(entry.path(store.root()))
        } else {
            None
        }
    })?;
    let bytes = std::fs::read(&thumb_path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    let handle = ctx.load_texture(format!("clip_thumb_{id}"), color, Default::default());
    state.textures.insert(id, handle.clone());
    Some(handle)
}
