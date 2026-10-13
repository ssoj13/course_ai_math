//! Runnable demo: an eframe window showing the otio-timeline widget over a
//! sample timeline. Run with `cargo run --example demo`.
//!
//! - Edit tool: click a clip to select; drag the body to move it (Ctrl+drag to slide),
//!   drag an edge to trim (or roll a shared cut). Blade tool: click a clip to split it.
//! - Ctrl+Z / Ctrl+Y (or Ctrl+Shift+Z): undo / redo.
//! - Ctrl + mouse wheel: zoom; wheel: scroll.

use egui_widgets_config::icons as ph;
use otio_edit::Editor;
use otio_timeline::{
    PX_PER_SEC_MAX, PX_PER_SEC_MIN, TRACK_H_MAX, TRACK_H_MIN, TimelineOpts, TimelineState, Tool,
    sample_timeline, timeline_ui_ex,
};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "otio-timeline demo",
        options,
        Box::new(|cc| {
            // Register the Phosphor icon font so the toolbar glyphs and the widget's
            // drawn edit cursors (hand / ↔ / scissors) render instead of tofu boxes.
            let mut fonts = egui::FontDefinitions::default();
            egui_widgets_config::add_icon_font(&mut fonts);
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(DemoApp::new()))
        }),
    )
}

struct DemoApp {
    editor: Editor,
    state: TimelineState,
    /// Lazily-built demo thumbnail texture, shared by every clip to exercise the
    /// `TimelineOpts::thumbnail` provider API. A real host would key it per clip.
    thumb: Option<egui::TextureHandle>,
}

impl DemoApp {
    fn new() -> Self {
        Self {
            editor: sample_timeline(),
            state: TimelineState::default(),
            thumb: None,
        }
    }
}

impl eframe::App for DemoApp {
    // `logic` runs before `ui` (no painting) — the place for keyboard shortcuts.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.input(|i| {
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Z) {
                if i.modifiers.shift {
                    self.editor.redo();
                } else {
                    self.editor.undo();
                }
            }
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Y) {
                self.editor.redo();
            }
        });
    }

    // eframe hands the root `Ui`; nest panels with `show` (egui 0.36 renamed `show_inside`).
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("toolbar").show(root, |ui| {
            ui.horizontal(|ui| {
                // Two tools, DaVinci-style: one context-sensitive Edit + a Blade. Edit does
                // move/slide/trim/roll by grab zone (Ctrl+drag body = slide); Blade splits.
                ui.selectable_value(
                    &mut self.state.tool,
                    Tool::Edit,
                    format!("{} Edit", ph::SELECT),
                );
                ui.selectable_value(
                    &mut self.state.tool,
                    Tool::Blade,
                    format!("{} Blade", ph::CUT),
                );
                ui.separator();
                if ui
                    .add_enabled(self.editor.can_undo(), egui::Button::new("↶ Undo"))
                    .clicked()
                {
                    self.editor.undo();
                }
                if ui
                    .add_enabled(self.editor.can_redo(), egui::Button::new("↷ Redo"))
                    .clicked()
                {
                    self.editor.redo();
                }
                ui.separator();
                ui.label("Zoom");
                // Range from the widget's own zoom bounds so the slider and the
                // internal clamp can't disagree (single source of truth).
                ui.add(
                    egui::Slider::new(&mut self.state.px_per_sec, PX_PER_SEC_MIN..=PX_PER_SEC_MAX)
                        .logarithmic(true),
                );
                ui.separator();
                ui.label("Track H");
                // Range from the widget's own track-height bounds (single source of truth).
                ui.add(egui::Slider::new(
                    &mut self.state.track_h,
                    TRACK_H_MIN..=TRACK_H_MAX,
                ));
                ui.separator();
                // Three-point record-side: mark in/out at the playhead (or press I/O),
                // clear them, or ripple-delete the marked range.
                if ui
                    .button(format!("{} In", ph::LOGIN))
                    .on_hover_text("mark in at playhead (I)")
                    .clicked()
                {
                    self.state.mark_in = Some(self.state.playhead);
                }
                if ui
                    .button(format!("{} Out", ph::LOGOUT))
                    .on_hover_text("mark out at playhead (O)")
                    .clicked()
                {
                    self.state.mark_out = Some(self.state.playhead);
                }
                if ui
                    .button(format!("{} Clear", ph::CLOSE))
                    .on_hover_text("clear in/out marks")
                    .clicked()
                {
                    otio_timeline::clear_marks(&mut self.state);
                }
                if ui
                    .button(format!("{} Range", ph::TRASH))
                    .on_hover_text("ripple-delete the marked range")
                    .clicked()
                {
                    otio_timeline::clear_marked_range(&mut self.editor, &mut self.state, true);
                }
                ui.separator();
                if let Some(label) = self.editor.undo_label() {
                    ui.weak(format!("next undo: {label}"));
                }
            });
        });

        egui::Panel::bottom("status").show(root, |ui| {
            match &self.state.last_error {
                Some(e) => {
                    ui.colored_label(
                        egui_widgets_config::semantic(ui).warn,
                        format!("{} {e}", ph::WARNING),
                    )
                }
                None => ui.weak(format!(
                    "playhead {:.2}s   rev {}",
                    self.state.playhead,
                    self.editor.revision()
                )),
            };
        });

        egui::CentralPanel::default().show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Build the demo thumbnail once, then hand the widget a provider that maps
                // every clip to it — the minimal exercise of the thumbnail mini-API.
                let tid = self
                    .thumb
                    .get_or_insert_with(|| {
                        ui.ctx().load_texture(
                            "clip-thumb",
                            make_thumb(),
                            egui::TextureOptions::LINEAR,
                        )
                    })
                    .id();
                let provider = move |_clip: otio_timeline::NodeId| Some(tid);
                let opts = TimelineOpts {
                    thumbnail: Some(&provider),
                };
                timeline_ui_ex(ui, &mut self.editor, &mut self.state, &opts);
            });
        });
    }
}

// The sample timeline now lives in the library (`otio_timeline::sample_timeline`)
// so the gallery demo and this example share one builder.

/// A tiny procedural gradient standing in for a decoded poster frame, so the demo can
/// show the `TimelineOpts::thumbnail` provider painting clip bodies without any I/O.
fn make_thumb() -> egui::ColorImage {
    let (w, h) = (48usize, 27usize);
    let mut px = Vec::with_capacity(w * h * 4);
    for _y in 0..h {
        for x in 0..w {
            let t = x as f32 / (w - 1) as f32;
            px.extend_from_slice(&[
                (40.0 + t * 60.0) as u8,
                (90.0 + t * 120.0) as u8,
                (150.0 + t * 90.0) as u8,
                255,
            ]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &px)
}
