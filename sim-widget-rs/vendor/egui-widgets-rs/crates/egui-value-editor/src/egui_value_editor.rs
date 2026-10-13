//! A generic modal deep-editor for ONE typed value.
//!
//! Open it with a title, a type label and an [`EditValue`]; call [`ValueEditor::show`]
//! each frame; on Apply it returns the edited [`EditValue`] for the host to write
//! back. The widget knows nothing about the host's real types — it carries ints as
//! `i64` and reals as `f64` (the widest common types), so the host narrows on apply
//! (e.g. `i64 -> u32`, `f64 -> f32/half`, the vector arity -> its precise type).
//!
//! ```no_run
//! # use egui_value_editor::{ValueEditor, EditValue, ValueEditorResult};
//! # fn demo(ctx: &egui::Context, ed: &mut ValueEditor) {
//! // host, when the user asks to edit a cell:
//! ed.open("translate", "float3", EditValue::Vec(vec![1.0, 2.0, 3.0]));
//! // per frame:
//! match ed.show(ctx) {
//!     ValueEditorResult::Apply(v) => { /* narrow `v` to your type and store it */ }
//!     ValueEditorResult::Open => {}      // still editing
//!     ValueEditorResult::Closed => {}    // dialog not shown / dismissed
//! }
//! # }
//! ```
//!
//! Hermetic — egui only (+ the shared `egui-widgets-config` tokens). Decoupled
//! from the USD value editor it was gathered from.

use egui::{Context, Grid, RichText, ScrollArea, TextEdit, Ui};
use egui_widgets_config::semantic;

/// A single value the editor can edit, in the widest common representation.
///
/// Integers are `i64`, reals are `f64`; the host narrows to its precise type on
/// [`ValueEditorResult::Apply`]. `ArrayView` / `ReadOnly` are display-only (the
/// host passes a pre-formatted dump; no edit is produced for them).
#[derive(Clone, Debug, PartialEq)]
pub enum EditValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    /// 2..4 components (vector / point / normal / colour).
    Vec(Vec<f64>),
    /// `(x, y, z, w)` — imaginary first, real last (UI order).
    Quat([f64; 4]),
    /// Row-major 4x4.
    Mat4([f64; 16]),
    /// Single-line string / token.
    Text(String),
    /// Multiline string (code-style editor).
    Multiline(String),
    /// Read-only formatted array dump (host pre-formats; not editable).
    ArrayView(String),
    /// Read-only formatted value (unsupported / inspect-only).
    ReadOnly(String),
}

/// Which editor sub-view is active (derived from the opened [`EditValue`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Bool,
    Int,
    Float,
    Vec,
    Quat,
    Mat4,
    Text,
    Multiline,
    Array,
    ReadOnly,
}

/// Outcome of one [`ValueEditor::show`] call.
#[derive(Clone, Debug, PartialEq)]
pub enum ValueEditorResult {
    /// Dialog is not open (or was just dismissed) — no change.
    Closed,
    /// Dialog is open and still being edited.
    Open,
    /// The user pressed Apply; carries the edited value. The dialog stays open
    /// (the host decides whether to [`ValueEditor::close`] it).
    Apply(EditValue),
}

/// Modal deep-editor state. Persist one per host editor; drive it via [`Self::show`].
#[derive(Clone, Debug, Default)]
pub struct ValueEditor {
    open: bool,
    title: String,
    type_label: String,
    kind: Option<Kind>,
    /// Numeric components (scalars: len 1; vectors: 2..4; quats: 4).
    comp_buf: Vec<f64>,
    string_buf: String,
    /// 16 matrix cell edit buffers (row-major).
    matrix_buf: [String; 16],
    bool_buf: bool,
    /// Read-only cached display (Array / ReadOnly).
    cached: String,
    dirty: bool,
}

impl ValueEditor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the editor for `value`, seeding the buffers. `title` names the field,
    /// `type_label` is the dimmed type string shown in the header.
    pub fn open(
        &mut self,
        title: impl Into<String>,
        type_label: impl Into<String>,
        value: EditValue,
    ) {
        self.open = true;
        self.title = title.into();
        self.type_label = type_label.into();
        self.dirty = false;
        self.cached.clear();
        self.string_buf.clear();
        self.matrix_buf = Default::default();
        self.bool_buf = false;
        self.comp_buf.clear();

        self.kind = Some(match value {
            EditValue::Bool(b) => {
                self.bool_buf = b;
                Kind::Bool
            }
            EditValue::Int(i) => {
                self.comp_buf = vec![i as f64];
                Kind::Int
            }
            EditValue::Float(f) => {
                self.comp_buf = vec![f];
                Kind::Float
            }
            EditValue::Vec(v) => {
                self.comp_buf = v;
                Kind::Vec
            }
            EditValue::Quat(q) => {
                self.comp_buf = q.to_vec();
                Kind::Quat
            }
            EditValue::Mat4(m) => {
                for (i, v) in m.iter().enumerate() {
                    self.matrix_buf[i] = format!("{v:.6}");
                }
                Kind::Mat4
            }
            EditValue::Text(s) => {
                self.string_buf = s;
                Kind::Text
            }
            EditValue::Multiline(s) => {
                self.string_buf = s;
                Kind::Multiline
            }
            EditValue::ArrayView(s) => {
                self.cached = s;
                Kind::Array
            }
            EditValue::ReadOnly(s) => {
                self.cached = s;
                Kind::ReadOnly
            }
        });
    }

    /// Close the editor (no value produced).
    pub fn close(&mut self) {
        self.open = false;
        self.kind = None;
        self.dirty = false;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Rebuild an [`EditValue`] from the current buffers (only for editable kinds).
    fn rebuild(&self) -> Option<EditValue> {
        Some(match self.kind? {
            Kind::Bool => EditValue::Bool(self.bool_buf),
            Kind::Int => {
                EditValue::Int(self.comp_buf.first().copied().unwrap_or(0.0).round() as i64)
            }
            Kind::Float => EditValue::Float(self.comp_buf.first().copied().unwrap_or(0.0)),
            Kind::Vec => EditValue::Vec(self.comp_buf.clone()),
            Kind::Quat => {
                let g = |i: usize| self.comp_buf.get(i).copied().unwrap_or(0.0);
                EditValue::Quat([g(0), g(1), g(2), g(3)])
            }
            Kind::Mat4 => {
                let mut m = [0.0f64; 16];
                for (i, cell) in self.matrix_buf.iter().enumerate() {
                    m[i] = cell.trim().parse::<f64>().unwrap_or(0.0);
                }
                EditValue::Mat4(m)
            }
            Kind::Text => EditValue::Text(self.string_buf.clone()),
            Kind::Multiline => EditValue::Multiline(self.string_buf.clone()),
            // Read-only kinds never become dirty, so Apply is unreachable for them.
            Kind::Array | Kind::ReadOnly => return None,
        })
    }

    /// Render the modal (if open) and report the outcome for this frame.
    pub fn show(&mut self, ctx: &Context) -> ValueEditorResult {
        if !self.open {
            return ValueEditorResult::Closed;
        }

        let mut window_open = self.open;
        let mut applied: Option<EditValue> = None;
        let mut close_clicked = false;

        egui::Window::new(format!("Edit: {}", self.title))
            .open(&mut window_open)
            .resizable(true)
            .default_size([500.0, 400.0])
            .show(ctx, |ui| {
                let sem = semantic(ui);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Type: {}", self.type_label))
                            .small()
                            .color(sem.weak),
                    );
                });
                ui.separator();

                self.draw_body(ui);

                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(self.dirty, egui::Button::new("Apply"))
                        .clicked()
                    {
                        applied = self.rebuild();
                        self.dirty = false;
                    }
                    if ui.button("Close").clicked() {
                        close_clicked = true;
                    }
                });
            });

        // The window's [x] toggled `window_open` to false; "Close" did too.
        self.open = window_open && !close_clicked;
        if !self.open {
            self.kind = None;
        }

        if let Some(v) = applied {
            ValueEditorResult::Apply(v)
        } else if self.open {
            ValueEditorResult::Open
        } else {
            ValueEditorResult::Closed
        }
    }

    /// Draw the active sub-view, mutating the buffers + `dirty`.
    fn draw_body(&mut self, ui: &mut Ui) {
        let Some(kind) = self.kind else { return };
        match kind {
            Kind::ReadOnly => read_only(ui, &self.cached),
            Kind::Array => {
                ui.label(RichText::new("Array contents (read-only):").strong());
                read_only(ui, &self.cached);
            }
            Kind::Text => {
                if ui
                    .add(
                        TextEdit::singleline(&mut self.string_buf)
                            .desired_width(ui.available_width()),
                    )
                    .changed()
                {
                    self.dirty = true;
                }
            }
            Kind::Multiline => {
                if ui
                    .add(
                        TextEdit::multiline(&mut self.string_buf)
                            .desired_width(ui.available_width())
                            .desired_rows(10)
                            .code_editor(),
                    )
                    .changed()
                {
                    self.dirty = true;
                }
            }
            Kind::Bool => {
                if ui.checkbox(&mut self.bool_buf, "value").changed() {
                    self.dirty = true;
                }
            }
            Kind::Int => {
                if let Some(v) = self.comp_buf.first_mut()
                    && ui.add(egui::DragValue::new(v).speed(1.0)).changed()
                {
                    *v = v.round();
                    self.dirty = true;
                }
            }
            Kind::Float => {
                if let Some(v) = self.comp_buf.first_mut()
                    && ui.add(egui::DragValue::new(v).speed(0.01)).changed()
                {
                    self.dirty = true;
                }
            }
            Kind::Vec | Kind::Quat => {
                const LABELS: [&str; 4] = ["x", "y", "z", "w"];
                let mut changed = false;
                ui.horizontal(|ui| {
                    for (i, v) in self.comp_buf.iter_mut().enumerate() {
                        ui.label(LABELS.get(i).copied().unwrap_or("?"));
                        if ui.add(egui::DragValue::new(v).speed(0.01)).changed() {
                            changed = true;
                        }
                    }
                });
                if changed {
                    self.dirty = true;
                }
            }
            Kind::Mat4 => {
                ui.label(RichText::new("4x4 Matrix:").strong());
                let mut changed = false;
                Grid::new("value_editor_matrix")
                    .num_columns(4)
                    .spacing([4.0, 4.0])
                    .show(ui, |ui| {
                        for row in 0..4 {
                            for col in 0..4 {
                                let idx = row * 4 + col;
                                if ui
                                    .add(
                                        TextEdit::singleline(&mut self.matrix_buf[idx])
                                            .desired_width(80.0)
                                            .font(egui::TextStyle::Monospace),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            }
                            ui.end_row();
                        }
                    });
                if changed {
                    self.dirty = true;
                }
            }
        }
    }
}

/// Scrollable read-only monospace dump.
fn read_only(ui: &mut Ui, display: &str) {
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(300.0)
        .show(ui, |ui| {
            ui.label(RichText::new(display).monospace().small());
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened(v: EditValue) -> ValueEditor {
        let mut ed = ValueEditor::new();
        ed.open("t", "ty", v);
        ed
    }

    #[test]
    fn open_seeds_and_rebuild_roundtrips() {
        // scalars
        assert_eq!(
            opened(EditValue::Bool(true)).rebuild(),
            Some(EditValue::Bool(true))
        );
        assert_eq!(opened(EditValue::Int(7)).rebuild(), Some(EditValue::Int(7)));
        assert_eq!(
            opened(EditValue::Float(1.5)).rebuild(),
            Some(EditValue::Float(1.5))
        );
        // vector + quat
        assert_eq!(
            opened(EditValue::Vec(vec![1.0, 2.0, 3.0])).rebuild(),
            Some(EditValue::Vec(vec![1.0, 2.0, 3.0]))
        );
        assert_eq!(
            opened(EditValue::Quat([0.0, 0.0, 0.0, 1.0])).rebuild(),
            Some(EditValue::Quat([0.0, 0.0, 0.0, 1.0]))
        );
    }

    #[test]
    fn matrix_roundtrips_through_string_buffers() {
        let m: [f64; 16] = std::array::from_fn(|i| i as f64 * 0.5);
        let ed = opened(EditValue::Mat4(m));
        // buffers parse back to the same values.
        assert_eq!(ed.rebuild(), Some(EditValue::Mat4(m)));
    }

    #[test]
    fn readonly_kinds_produce_no_value() {
        assert_eq!(opened(EditValue::ReadOnly("x".into())).rebuild(), None);
        assert_eq!(opened(EditValue::ArrayView("[..]".into())).rebuild(), None);
    }

    #[test]
    fn open_close_toggles_state() {
        let mut ed = ValueEditor::new();
        assert!(!ed.is_open());
        ed.open("t", "ty", EditValue::Int(0));
        assert!(ed.is_open());
        ed.close();
        assert!(!ed.is_open());
    }
}
