//! Data-driven preferences layer for [`crate`] (v0.2).
//!
//! Instead of hand-writing a body closure (v0.1), the host declares its preferences as **data**:
//! a [`PrefsSchema`] of categories -> sections -> typed [`FieldSpec`]s. [`draw_schema`] renders it
//! in the same two-pane vfx-view look, reading and writing the live values in a `box-rs`
//! [`RustBox`] addressed by each field's `path`. Schema in, filled box-rs out.
//!
//! Why box-rs as the value store: it is the project's nested value currency (typed path get/set,
//! `Color`/`Vec3`, JSON/serde), so a filled panel serializes for persistence for free and the same
//! values flow into GTO sessions / other tools without a translation layer.
//!
//! # Design boundary
//! The **schema** (presentation contract: order, widget kind, ranges, enum variants, labels) lives
//! here and is serde-serializable, so a whole panel can be authored in Rust *or* loaded from
//! RON/JSON. The **values** live in box-rs. box-rs stays a pure data container — it carries no UI
//! hints. Bespoke widgets that don't fit a [`Widget`] kind use [`Widget::Custom`], rendered by a
//! host-supplied closure keyed by id.
//!
//! ```ignore
//! let schema = PrefsSchema::new(vec![ /* categories */ ]);
//! let mut values = schema.default_values();          // seed a RustBox from the schema
//! let mut customs = CustomRenderers::new();          // optional Widget::Custom handlers
//! egui_prefs2::draw_schema(ui, &mut panel_state, &schema, &mut values, &mut customs);
//! // `values` now holds the edited state; persist it (values.to_json_value()) whenever you like.
//! ```

use std::collections::HashMap;

use box_rs::{BoxValue, RustBox};
use egui::Ui;
use egui_attr_grid::{
    AttrValue, EditorCtx, ValueEditorLayout, render_value_editor, reset_to, take_reset,
};
use serde::{Deserialize, Serialize};

use crate::{Category, PrefsPanelState, draw, section_grid, section_header};

/// The widget used to edit a field, plus the bounds/choices it needs. Serde-tagged so a schema
/// round-trips through RON/JSON. Each variant maps to one `box-rs` [`BoxValue`] kind (see
/// [`Widget::default_value`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Widget {
    /// A checkbox. Stored as [`BoxValue::Bool`].
    Bool,
    /// An integer slider. Stored as [`BoxValue::I64`]. `step` <= 1 means a continuous integer slider.
    Int { min: i64, max: i64, step: i64 },
    /// A float slider. Stored as [`BoxValue::F64`]. `step` <= 0 means no snapping. `logarithmic`
    /// gives a log-scaled slider (for speed/scale factors); `suffix` is an optional unit label.
    Float {
        min: f64,
        max: f64,
        #[serde(default)]
        step: f64,
        #[serde(default)]
        logarithmic: bool,
        #[serde(default)]
        suffix: String,
    },
    /// A bounded numeric drag field (no track). Stored as [`BoxValue::F64`].
    Drag {
        min: f64,
        max: f64,
        #[serde(default = "one")]
        speed: f64,
    },
    /// A single- or multi-line text field. Stored as [`BoxValue::String`].
    Text {
        #[serde(default)]
        multiline: bool,
    },
    /// An RGBA colour button. Stored as [`BoxValue::Color`] (`[f64; 4]`, sRGB 0..1).
    Color,
    /// A dropdown over `variants`. Stored as [`BoxValue::String`] (the selected variant text).
    Enum { variants: Vec<String> },
    /// Three bounded drag fields (x/y/z). Stored as [`BoxValue::Vec3`].
    Vec3 { min: f64, max: f64 },
    /// Escape hatch: the host renders it via a closure registered in [`CustomRenderers`] under this
    /// id. The closure owns reading/writing its own value(s) in the `RustBox`.
    Custom(String),
}

/// serde default for [`Widget::Drag::speed`].
fn one() -> f64 {
    1.0
}

impl Widget {
    /// Integer slider `min..=max` snapping by `step` (`step <= 1` = continuous).
    pub fn int(min: i64, max: i64, step: i64) -> Self {
        Widget::Int { min, max, step }
    }

    /// Float slider `min..=max` snapping by `step` (`step <= 0` = none), no suffix, linear.
    pub fn float(min: f64, max: f64, step: f64) -> Self {
        Widget::Float {
            min,
            max,
            step,
            logarithmic: false,
            suffix: String::new(),
        }
    }

    /// Float slider with a unit `suffix` (e.g. `" pt"`, `" deg"`).
    pub fn float_unit(min: f64, max: f64, step: f64, suffix: impl Into<String>) -> Self {
        Widget::Float {
            min,
            max,
            step,
            logarithmic: false,
            suffix: suffix.into(),
        }
    }

    /// Log-scaled float slider (for speed/scale factors) `min..=max`.
    pub fn float_log(min: f64, max: f64) -> Self {
        Widget::Float {
            min,
            max,
            step: 0.0,
            logarithmic: true,
            suffix: String::new(),
        }
    }

    /// Bounded numeric drag field.
    pub fn drag(min: f64, max: f64, speed: f64) -> Self {
        Widget::Drag { min, max, speed }
    }

    /// Dropdown over the given variant labels.
    pub fn enum_of<S: AsRef<str>>(variants: &[S]) -> Self {
        Widget::Enum {
            variants: variants.iter().map(|s| s.as_ref().to_string()).collect(),
        }
    }

    /// Single-line text field.
    pub fn text() -> Self {
        Widget::Text { multiline: false }
    }

    /// Multi-line text field.
    pub fn multiline() -> Self {
        Widget::Text { multiline: true }
    }

    /// Three bounded drag fields (x/y/z).
    pub fn vec3(min: f64, max: f64) -> Self {
        Widget::Vec3 { min, max }
    }

    /// Escape hatch rendered by a host closure registered under `id`.
    pub fn custom(id: impl Into<String>) -> Self {
        Widget::Custom(id.into())
    }

    /// The value written when a field has no explicit [`FieldSpec::default`] — derived from the
    /// widget so every schema path exists after [`PrefsSchema::default_values`]. `Custom` returns
    /// `None` (its value, if any, is owned by the host closure).
    fn default_value(&self) -> Option<BoxValue> {
        Some(match self {
            Widget::Bool => BoxValue::Bool(false),
            Widget::Int { min, .. } => BoxValue::I64(*min),
            Widget::Float { min, .. } | Widget::Drag { min, .. } => BoxValue::F64(*min),
            Widget::Vec3 { min, .. } => BoxValue::Vec3([*min; 3]),
            Widget::Text { .. } => BoxValue::String(String::new()),
            Widget::Color => BoxValue::Color([1.0, 1.0, 1.0, 1.0]),
            Widget::Enum { variants } => {
                BoxValue::String(variants.first().cloned().unwrap_or_default())
            }
            Widget::Custom(_) => return None,
        })
    }
}

/// One editable field: where its value lives (`path` in the [`RustBox`]), how it is labelled, and
/// which [`Widget`] edits it. `default` (when non-`Null`) seeds the value store and drives Reset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldSpec {
    /// box-rs path to this field's value (e.g. `"material.roughness"`).
    pub path: String,
    /// Left-column label.
    pub label: String,
    /// Optional hover tooltip.
    #[serde(default)]
    pub tooltip: Option<String>,
    /// The editing widget.
    pub widget: Widget,
    /// Initial/reset value. `Null` (the default) means "derive from the widget" (see
    /// [`Widget::default_value`]).
    #[serde(default)]
    pub default: BoxValue,
}

impl FieldSpec {
    pub fn new(path: impl Into<String>, label: impl Into<String>, widget: Widget) -> Self {
        Self {
            path: path.into(),
            label: label.into(),
            tooltip: None,
            widget,
            default: BoxValue::Null,
        }
    }

    /// Attach a hover tooltip.
    pub fn tip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Set the initial/reset value explicitly (overrides the widget-derived default).
    pub fn default(mut self, value: BoxValue) -> Self {
        self.default = value;
        self
    }
}

/// A group of fields under an optional header, laid out as a two-column grid (label | widget).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// Section header text (a [`section_header`]); `None` draws the fields with no header.
    #[serde(default)]
    pub header: Option<String>,
    pub fields: Vec<FieldSpec>,
}

impl Section {
    pub fn new(header: impl Into<String>, fields: Vec<FieldSpec>) -> Self {
        Self {
            header: Some(header.into()),
            fields,
        }
    }

    /// A section with no header.
    pub fn bare(fields: Vec<FieldSpec>) -> Self {
        Self {
            header: None,
            fields,
        }
    }
}

/// A sidebar category: its icon + label and the sections shown in the content pane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SchemaCategory {
    pub icon: String,
    pub label: String,
    pub sections: Vec<Section>,
}

impl SchemaCategory {
    pub fn new(icon: impl Into<String>, label: impl Into<String>, sections: Vec<Section>) -> Self {
        Self {
            icon: icon.into(),
            label: label.into(),
            sections,
        }
    }
}

/// A complete preferences schema: the ordered category list driving both the sidebar and the
/// content. Serde-serializable, so it can be authored in Rust or loaded from RON/JSON.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PrefsSchema {
    pub categories: Vec<SchemaCategory>,
}

impl PrefsSchema {
    pub fn new(categories: Vec<SchemaCategory>) -> Self {
        Self { categories }
    }

    /// Build a fresh [`RustBox`] with every field set to its default (explicit [`FieldSpec::default`]
    /// or widget-derived). Use it to seed the value store and as the Reset target — after this every
    /// schema `path` exists, so later writes always hit an existing slot.
    pub fn default_values(&self) -> RustBox {
        let mut b = RustBox::new();
        for cat in &self.categories {
            for sec in &cat.sections {
                for f in &sec.fields {
                    let v = if f.default != BoxValue::Null {
                        Some(f.default.clone())
                    } else {
                        f.widget.default_value()
                    };
                    if let Some(v) = v {
                        put(&mut b, &f.path, v);
                    }
                }
            }
        }
        b
    }
}

/// Host-supplied renderers for [`Widget::Custom`] fields, keyed by the custom id. Each closure gets
/// the field's `ui` cell and mutable access to the whole value store.
pub type CustomRenderers<'a> = HashMap<String, Box<dyn FnMut(&mut Ui, &mut RustBox) + 'a>>;

/// Draw a data-driven preferences panel: the [`PrefsSchema`] supplies the two-pane categories and
/// per-category sections; edits are written straight into `values` by path. A "Reset to Defaults"
/// footer restores `values` to [`PrefsSchema::default_values`]. `customs` handles any
/// [`Widget::Custom`] fields (pass an empty map if there are none).
pub fn draw_schema(
    ui: &mut Ui,
    state: &mut PrefsPanelState,
    schema: &PrefsSchema,
    values: &mut RustBox,
    customs: &mut CustomRenderers<'_>,
) {
    // Borrow the schema's owned strings as the two-pane sidebar entries.
    let cats: Vec<Category<'_>> = schema
        .categories
        .iter()
        .map(|c| Category::new(&c.icon, &c.label))
        .collect();

    let mut do_reset = false;
    draw(
        ui,
        state,
        &cats,
        |ui, idx| render_category(ui, values, &schema.categories[idx], idx, customs),
        Some(|| do_reset = true),
    );
    if do_reset {
        *values = schema.default_values();
    }
}

/// Render one category's sections into the content pane.
fn render_category(
    ui: &mut Ui,
    values: &mut RustBox,
    cat: &SchemaCategory,
    cat_idx: usize,
    customs: &mut CustomRenderers<'_>,
) {
    for (si, section) in cat.sections.iter().enumerate() {
        if let Some(h) = &section.header {
            section_header(ui, h);
        }
        // Custom fields own their layout (often nested Grids, e.g. vfx-view color_panel).
        // Putting them inside `section_grid` nests Grid-in-Grid and shatters the columns.
        let (customs_only, plain): (Vec<_>, Vec<_>) = section
            .fields
            .iter()
            .partition(|f| matches!(f.widget, Widget::Custom(_)));

        if !plain.is_empty() {
            let editor_w = editor_width(ui);
            section_grid(ui, ("egui_prefs2_schema", cat_idx, si), |ui| {
                for f in &plain {
                    let resp = ui.label(&f.label);
                    let label_id = resp.id;
                    if let Some(tip) = &f.tooltip {
                        resp.on_hover_text(tip);
                    }
                    render_field(ui, values, f, customs, editor_w, Some(label_id));
                    ui.end_row();
                }
            });
        }

        for f in customs_only {
            // Optional label above a full-bleed custom body (empty label = vfx-view style).
            if !f.label.is_empty() {
                let resp = ui.label(&f.label);
                if let Some(tip) = &f.tooltip {
                    resp.on_hover_text(tip);
                }
            }
            render_field(ui, values, f, customs, 0.0, None);
        }
    }
}

/// Widest editor column, in numeric field widths (46 px each by default, about 370 px): a pane can
/// be very wide, and a slider or vector filling all of it reads worse than one of a comfortable
/// size. The editors fill what they are given, so this is the cap.
const EDITOR_FIELDS: f32 = 8.0;
/// Narrowest editor column, in numeric field widths: three fields of a vector.
const EDITOR_MIN_FIELDS: f32 = 3.0;

/// The width of every editor in a pane: what is left of it after the label column, within the
/// bounds above. Computed ONCE from the pane, not from each Grid cell: a cell's available width is
/// whatever the previous frame's widest widget made the column, so editors that fill it would each
/// settle at a different width.
fn editor_width(ui: &Ui) -> f32 {
    let m = crate::metrics(ui);
    let unit = ValueEditorLayout::default().numeric_width;
    (ui.available_width() - m.label_column - m.column_gap)
        .clamp(unit * EDITOR_MIN_FIELDS, unit * EDITOR_FIELDS)
}

/// Hint tokens of the shared attribute editor; an enum variant spelled like one cannot go through
/// its combo (the editor would take it for a render hint), so such an enum keeps its own dropdown.
const RESERVED: [&str; 4] = ["file", "folder", "font", "segmented"];

/// A field's current value as the shared attribute editor's [`AttrValue`], or None for a widget it
/// does not draw (`Custom`, multi-line text). `box_value` is what the value store holds, or the
/// default being converted.
fn attr_of(widget: &Widget, box_value: Option<&BoxValue>) -> Option<AttrValue> {
    let v = box_value;
    Some(match widget {
        Widget::Bool => AttrValue::Bool(v.and_then(BoxValue::as_bool).unwrap_or(false)),
        Widget::Int { min, .. } => AttrValue::Int64(v.and_then(BoxValue::as_i64).unwrap_or(*min)),
        // The editor holds f32; a value is only converted back when the user edits it.
        Widget::Float { min, .. } | Widget::Drag { min, .. } => {
            AttrValue::Float(v.and_then(BoxValue::as_f64).unwrap_or(*min) as f32)
        }
        Widget::Text { multiline: false } => {
            AttrValue::Str(v.and_then(BoxValue::as_str).unwrap_or_default().to_owned())
        }
        Widget::Color => {
            let c = v.and_then(BoxValue::as_color).unwrap_or([1.0; 4]);
            AttrValue::Vec4(c.map(|x| x as f32))
        }
        Widget::Enum { variants } => AttrValue::Str(
            v.and_then(BoxValue::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| variants.first().cloned().unwrap_or_default()),
        ),
        Widget::Vec3 { min, .. } => {
            let a = v.and_then(BoxValue::as_vec3).unwrap_or([*min; 3]);
            AttrValue::Vec3(a.map(|x| x as f32))
        }
        Widget::Text { multiline: true } | Widget::Custom(_) => return None,
    })
}

/// The editor hints of a widget: slider range, step, log scale, unit, colour, choices.
fn hints_of(widget: &Widget) -> Vec<String> {
    let num = |x: f64| x.to_string();
    match widget {
        Widget::Int { min, max, step } => {
            let mut h = vec![num(*min as f64), num(*max as f64)];
            if *step > 1 {
                h.push(num(*step as f64));
            }
            h
        }
        Widget::Float {
            min,
            max,
            step,
            logarithmic,
            suffix,
        } => {
            // No step = no snapping; the editor's own default would quantise a free float.
            let mut h = vec![num(*min), num(*max), num(step.max(0.0))];
            if *logarithmic {
                h.push("log".into());
            }
            if !suffix.is_empty() {
                h.push(format!("suffix={suffix}"));
            }
            h
        }
        // A bounded drag field is drawn as a slider over the same bounds.
        Widget::Drag { min, max, .. } => vec![num(*min), num(*max), "0".into()],
        Widget::Color => vec!["color".into()],
        Widget::Enum { variants } => variants.clone(),
        _ => Vec::new(),
    }
}

/// An edited editor value back into the store's kind.
fn box_of(widget: &Widget, value: &AttrValue) -> Option<BoxValue> {
    Some(match (widget, value) {
        (Widget::Bool, AttrValue::Bool(v)) => BoxValue::Bool(*v),
        (Widget::Int { .. }, AttrValue::Int64(v)) => BoxValue::I64(*v),
        (Widget::Float { .. } | Widget::Drag { .. }, AttrValue::Float(v)) => {
            BoxValue::F64(f64::from(*v))
        }
        (Widget::Text { .. } | Widget::Enum { .. }, AttrValue::Str(v)) => {
            BoxValue::String(v.clone())
        }
        (Widget::Color, AttrValue::Vec4(c)) => BoxValue::Color(c.map(f64::from)),
        // The bounds of a vector are kept on write: the editor itself is unbounded.
        (Widget::Vec3 { min, max }, AttrValue::Vec3(a)) => {
            BoxValue::Vec3(a.map(|x| f64::from(x).clamp(*min, *max)))
        }
        _ => return None,
    })
}

/// Render one field's widget, reading its current value from `values` and writing edits back.
///
/// Every kind but `Custom` and multi-line text goes through the attribute editor of
/// `egui-attr-grid` ([`render_value_editor`]), so a Preferences row looks and behaves exactly like
/// an Attribute Editor row: fill-width slider with an aligned value box, Ctrl-click reset to the
/// field's default, edge-aligned vector and colour rows, the same combo. `label_id` is the row
/// label's widget id: it names the row's slider in the accessibility tree.
fn render_field(
    ui: &mut Ui,
    values: &mut RustBox,
    f: &FieldSpec,
    customs: &mut CustomRenderers<'_>,
    width: f32,
    label_id: Option<egui::Id>,
) {
    if let Widget::Custom(id) = &f.widget {
        match customs.get_mut(id) {
            Some(cb) => cb(ui, values),
            None => {
                ui.weak(format!("[custom '{id}' not provided]"));
            }
        }
        return;
    }
    let layout = ValueEditorLayout::default();
    if let Widget::Text { multiline: true } = &f.widget {
        let mut v = values.get_str(&f.path).unwrap_or_default().to_string();
        if ui
            .add(egui::TextEdit::multiline(&mut v).desired_width(width))
            .changed()
        {
            put(values, &f.path, BoxValue::String(v));
        }
        return;
    }
    let Some(mut value) = attr_of(&f.widget, values.get_path(&f.path)) else {
        return;
    };
    let hints = hints_of(&f.widget);
    if let Widget::Enum { variants } = &f.widget
        && variants
            .iter()
            .any(|v| RESERVED.iter().any(|r| v.eq_ignore_ascii_case(r)))
    {
        // A variant that reads as a render hint: keep a plain dropdown (see RESERVED).
        let AttrValue::Str(cur) = &mut value else {
            return;
        };
        let before = cur.clone();
        egui::ComboBox::from_id_salt(&f.path)
            .width(width)
            .selected_text(cur.as_str())
            .show_ui(ui, |ui| {
                for var in variants {
                    ui.selectable_value(cur, var.clone(), var);
                }
            });
        if *cur != before {
            put(values, &f.path, BoxValue::String(cur.clone()));
        }
        return;
    }
    // The reset target: the field's explicit default, else the widget's own.
    let default = {
        let own = (f.default != BoxValue::Null).then_some(&f.default);
        let derived = f.widget.default_value();
        attr_of(&f.widget, own.or(derived.as_ref()))
    };
    let extra = EditorCtx {
        default,
        label_id,
        ..EditorCtx::default()
    };
    let mut changed = ui
        .scope(|ui| {
            ui.set_width(width);
            take_reset(ui.ctx());
            render_value_editor(ui, &f.path, &mut value, false, &hints, &layout, &extra)
        })
        .inner;
    // Ctrl / Cmd-click reset: the editor only asks, the grid crate's one reset path writes.
    if let Some(part) = take_reset(ui.ctx()) {
        changed |= reset_to(&mut value, extra.default.as_ref(), &hints, part);
    }
    if changed && let Some(v) = box_of(&f.widget, &value) {
        put(values, &f.path, v);
    }
}

/// Write `val` at `path`, surfacing a write failure. A failure means the schema path could not be
/// created in the value store (a schema/values mismatch — a programmer error), so it is a debug
/// assertion and a release no-op rather than a silently-swallowed `Err`.
fn put(values: &mut RustBox, path: &str, val: BoxValue) {
    if let Err(e) = values.set_path(path, val) {
        debug_assert!(false, "egui-prefs2: write to '{path}' failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    fn schema() -> PrefsSchema {
        PrefsSchema::new(vec![SchemaCategory::new(
            "",
            "General",
            vec![Section::bare(vec![
                FieldSpec::new("a.on", "On", Widget::Bool),
                FieldSpec::new("a.count", "Count", Widget::int(0, 10, 1)).default(BoxValue::I64(4)),
                FieldSpec::new("a.gain", "Gain", Widget::float_unit(0.0, 2.0, 0.0, " dB")),
                FieldSpec::new("a.name", "Name", Widget::text()),
                FieldSpec::new("a.tint", "Tint", Widget::Color),
                FieldSpec::new("a.mode", "Mode", Widget::enum_of(&["Fast", "File"])),
                FieldSpec::new("a.pos", "Pos", Widget::vec3(-1.0, 1.0)),
            ])],
        )])
    }

    #[test]
    fn every_kind_maps_to_the_attribute_editor_and_back() {
        let schema = schema();
        let values = schema.default_values();
        for f in &schema.categories[0].sections[0].fields {
            let v = attr_of(&f.widget, values.get_path(&f.path)).expect("drawn by the editor");
            let back = box_of(&f.widget, &v).expect("converts back");
            assert_eq!(values.get_path(&f.path), Some(&back), "{}", f.path);
        }
        assert_eq!(
            hints_of(&Widget::int(0, 10, 1)),
            ["0", "10"],
            "step 1 is the editor's default"
        );
        assert_eq!(
            hints_of(&Widget::float(0.0, 1.0, 0.0))[2],
            "0",
            "no step: no snapping"
        );
        assert!(hints_of(&Widget::float_log(0.1, 10.0)).contains(&"log".to_owned()));
        assert!(
            hints_of(&Widget::float_unit(0.0, 1.0, 0.0, "px")).contains(&"suffix=px".to_owned())
        );
        // The bounds of a vector survive an edit past them.
        let w = Widget::vec3(-1.0, 1.0);
        assert_eq!(
            box_of(&w, &AttrValue::Vec3([5.0, -5.0, 0.5])),
            Some(BoxValue::Vec3([1.0, -1.0, 0.5]))
        );
    }

    #[test]
    fn schema_rows_are_attribute_editor_controls() {
        let schema = schema();
        let harness = Harness::new_ui_state(
            |ui, (schema, values): &mut (PrefsSchema, RustBox)| {
                let mut customs = CustomRenderers::new();
                for f in &schema.categories[0].sections[0].fields {
                    let label = ui.label(&f.label);
                    render_field(ui, values, f, &mut customs, 200.0, Some(label.id));
                }
            },
            (schema.clone(), schema.default_values()),
        );
        // Int and Float rows are sliders (Count, Gain), their value boxes spin buttons; the Vec3
        // adds three more and the RGBA tint four. A drag value without a track would have no slider role.
        assert_eq!(
            harness
                .get_all_by_role(egui::accesskit::Role::Slider)
                .count(),
            2
        );
        assert_eq!(
            harness
                .get_all_by_role(egui::accesskit::Role::SpinButton)
                .count(),
            2 + 3 + 4
        );
        assert_eq!(
            harness
                .get_all_by_role(egui::accesskit::Role::CheckBox)
                .count(),
            1
        );
        // The explicit default is the reset target of the Count slider.
        let count = harness
            .get_all_by_role(egui::accesskit::Role::Slider)
            .next()
            .unwrap();
        assert_eq!(count.accesskit_node().numeric_value(), Some(4.0));
        // The reserved-looking variant "File" keeps a working dropdown with its value.
        assert_eq!(
            harness
                .get_all_by_role(egui::accesskit::Role::ComboBox)
                .count(),
            1
        );
    }

    /// A schema slider is named by its row label, so assistive technology (and AccessKit-driven
    /// tests) find it by role + label.
    #[test]
    fn schema_sliders_are_named_by_their_row_label() {
        let schema = schema();
        let harness = Harness::new_ui_state(
            |ui, (schema, values, state): &mut (PrefsSchema, RustBox, PrefsPanelState)| {
                let mut customs = CustomRenderers::new();
                draw_schema(ui, state, schema, values, &mut customs);
            },
            (
                schema.clone(),
                schema.default_values(),
                PrefsPanelState::default(),
            ),
        );
        for label in ["Count", "Gain"] {
            let node = harness.get_by_role_and_label(egui::accesskit::Role::Slider, label);
            assert!(node.accesskit_node().numeric_value().is_some(), "{label}");
        }
    }

    /// A Preferences page as it renders now, to `$WIDGET_SHOTS/w1-prefs2.png`:
    /// `WIDGET_SHOTS=dir cargo test -p egui-prefs2 -- --ignored shots`. Needs a GPU.
    #[test]
    #[ignore = "writes a screenshot; needs a GPU adapter and WIDGET_SHOTS"]
    fn shots() {
        let Some(dir) = std::env::var_os("WIDGET_SHOTS") else {
            return;
        };
        let mut harness = Harness::builder()
            .with_size(egui::vec2(720.0, 400.0))
            .with_pixels_per_point(2.0)
            .build_ui_state(
                |ui, (schema, values, state): &mut (PrefsSchema, RustBox, PrefsPanelState)| {
                    let mut customs = CustomRenderers::new();
                    draw_schema(ui, state, schema, values, &mut customs);
                },
                (
                    schema(),
                    schema().default_values(),
                    PrefsPanelState::default(),
                ),
            );
        // An egui Grid sizes its columns from the last frame: let it converge.
        harness.run_steps(10);
        let image = harness.render().expect("render");
        image
            .save(std::path::Path::new(&dir).join("w1-prefs2.png"))
            .expect("save");
    }
}
