//! A `[label | value]` attribute editor grid for egui.
//!
//! The caller builds a flat `Vec<`[`AttrField`]`>` from its own model (key,
//! value, optional UI hints, sort order), hands it to [`render_grid`], and gets
//! back the rows the user edited this frame. The grid sorts by `order`, draws a
//! two-column table with a resizable label/value splitter (via
//! [`egui_splitter_table`]), renders a type-appropriate editor per value, and
//! dims rows whose key is in the `mixed` set (multi-selection differing values).
//!
//! Hermetic — no application types. Map your engine's value type to [`AttrValue`]
//! (editable scalars/vectors/matrices, numeric `List`, or a read-only
//! [`AttrValue::Label`]) and back from the returned changes.

mod actions;
pub mod preset;
mod random;

pub use actions::{ValueAction, ValueActions, randomize_fields, reset_to, take_reset};
pub use random::Random;

use std::collections::HashSet;

use egui::{ComboBox, Ui};
use egui_splitter_table::{Column, SplitterTable, SplitterTableState};
pub use egui_widgets_config::{AttrMetrics, ValueEditorLayout, segmented, segmented_tips};
use egui_widgets_config::{cell_ui, icon_button_tip, icon_toggle, icons};
use serde::{Deserialize, Serialize};

/// A generic attribute value the grid can display and edit.
///
/// Scalars, vectors and matrices are editable; [`List`](Self::List) renders its
/// numeric elements inline (non-numeric elements show read-only); anything the
/// host can't or won't expose for editing (uuid, map, set, json, …) maps to
/// [`Label`](Self::Label) and renders as plain text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AttrValue {
    Bool(bool),
    Str(String),
    Int8(i8),
    Int(i32),
    Int64(i64),
    UInt(u32),
    Float(f32),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Mat3([[f32; 3]; 3]),
    Mat4([[f32; 4]; 4]),
    /// Nested list; numeric elements are edited inline.
    List(Vec<AttrValue>),
    /// Read-only display text (uuid / map / set / json / anything not editable).
    Label(String),
}

impl AttrValue {
    /// Component `i` of a value as `f64`: the scalar itself (`i == 0`), a vector's or a numeric
    /// list's element. What a Ctrl-click on one component field resets to.
    pub(crate) fn component(&self, i: usize) -> Option<f64> {
        match self {
            AttrValue::Vec3(v) => v.get(i).map(|c| f64::from(*c)),
            AttrValue::Vec4(v) => v.get(i).map(|c| f64::from(*c)),
            AttrValue::List(items) => items.get(i).and_then(AttrValue::as_f64),
            scalar => scalar.as_f64().filter(|_| i == 0),
        }
    }

    /// Write a scalar from `f64` (rounded and saturated for integer kinds); true when the value
    /// changed. Other kinds are left alone.
    pub(crate) fn set_f64(&mut self, v: f64) -> bool {
        let before = self.clone();
        match self {
            AttrValue::Float(x) => *x = v as f32,
            AttrValue::Int8(x) => *x = v.round() as i8,
            AttrValue::Int(x) => *x = v.round() as i32,
            AttrValue::UInt(x) => *x = v.round() as u32,
            AttrValue::Int64(x) => *x = v.round() as i64,
            _ => {}
        }
        *self != before
    }

    /// A scalar as `f64` (what a slider's default is made from); None for everything else.
    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            AttrValue::Float(v) => Some(f64::from(*v)),
            AttrValue::Int8(v) => Some(f64::from(*v)),
            AttrValue::Int(v) => Some(f64::from(*v)),
            AttrValue::UInt(v) => Some(f64::from(*v)),
            AttrValue::Int64(v) => Some(*v as f64),
            _ => None,
        }
    }
}

/// One attribute row: key, value, optional UI hints, and a sort order.
///
/// `ui_options` mirror the host's schema hints:
/// - [`AttrValue::Str`]: a non-empty list becomes a combo box (`file` / `folder` / `font`
///   are render hints instead).
/// - Numeric scalars (float or integer): `["min","max"]` (optional `"step"`) becomes a slider
///   with a value box. `"log"` (or `"logarithmic"`) opts into a logarithmic scale for values
///   spanning orders of magnitude; `"soft"` makes the range a soft one: the slider covers it,
///   the value box accepts anything (the host keeps any hard limit). The slider fills the cell
///   and its box is as wide as the widest formatted min / max / value of any ranged row of the
///   grid (at least [`ValueEditorLayout::numeric_width`], at most three times it), so every row
///   ends at one edge and no value clips.
/// - [`AttrValue::Str`] choices add a `"segmented"` token to draw them as an always-visible
///   button strip instead of a combo: for two to four short choices (off / selected / all).
/// - [`AttrValue::Vec3`] / [`AttrValue::Vec4`] with a `"color"` token: channel fields plus the
///   HDR picker swatch.
///
/// `order` sorts rows ascending (lower first); ties break by key.
#[derive(Clone, Debug, PartialEq)]
pub struct AttrField {
    pub key: String,
    pub value: AttrValue,
    pub ui_options: Vec<String>,
    pub order: f32,
    /// Per-channel rows of a vector (X / Y / Z, or R / G / B for a `color` row), shown under it
    /// when expanded ([`ChannelExpansion`]); each its own scalar field with its own key, so a
    /// host can animate and key channels separately. Empty: no caret.
    pub channels: Vec<AttrField>,
    /// What the field does, shown when hovering its label (an inactive row shows its reason
    /// instead). None: the key.
    pub hint: Option<String>,
    /// What the value resets to: Ctrl / Cmd-click on a slider, or "Reset to default" in the row
    /// menu. Same type as `value`. None: no reset.
    pub default: Option<AttrValue>,
    /// Opt-in randomise ("Randomise" in the row menu), drawn from this policy. None: not offered.
    pub random: Option<Random>,
    /// A Vec3 / Vec4 row offers a lock toggle: while locked, editing one component scales all of
    /// them by the same ratio (uniform scale). The lock state lives in egui memory, per row key.
    pub uniform: bool,
    /// This row's own value actions: it offers only those of them the grid's
    /// [`AttrGridConfig::actions`] also enables (a row can narrow the set, never widen it).
    /// None: the grid's set.
    pub actions: Option<ValueActions>,
}

impl AttrField {
    /// A field with no UI hints and order 0.
    pub fn new(key: impl Into<String>, value: AttrValue) -> Self {
        Self {
            key: key.into(),
            value,
            ui_options: Vec::new(),
            order: 0.0,
            channels: Vec::new(),
            hint: None,
            default: None,
            random: None,
            uniform: false,
            actions: None,
        }
    }

    /// Narrow this row's value actions (see [`Self::actions`]).
    pub fn with_actions(mut self, actions: ValueActions) -> Self {
        self.actions = Some(actions);
        self
    }

    /// The value actions this row offers under `config`.
    fn value_actions(&self, config: &AttrGridConfig) -> ValueActions {
        self.actions
            .map_or(config.actions, |own| config.actions.and(own))
    }

    /// Offer the uniform-scale lock (see [`Self::uniform`]).
    pub fn with_uniform(mut self) -> Self {
        self.uniform = true;
        self
    }

    /// The value the row resets to (see [`Self::default`]).
    ///
    /// A default of another type than the value is refused (debug builds assert): a reset must
    /// never change a field's type.
    pub fn with_default(mut self, default: AttrValue) -> Self {
        let ok = actions::same_kind(&self.value, &default);
        debug_assert!(
            ok,
            "default {default:?} does not match the value of `{}`",
            self.key
        );
        if ok {
            self.default = Some(default);
        }
        self
    }

    /// Offer "Randomise" on the row, drawn from `random` (see [`Self::random`]).
    pub fn with_random(mut self, random: Random) -> Self {
        self.random = Some(random);
        self
    }

    /// Hover text of the label (see [`Self::hint`]).
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_channels(mut self, channels: Vec<AttrField>) -> Self {
        self.channels = channels;
        self
    }

    pub fn with_order(mut self, order: f32) -> Self {
        self.order = order;
        self
    }

    pub fn with_ui_options(mut self, ui_options: Vec<String>) -> Self {
        self.ui_options = ui_options;
        self
    }
}

/// Persisted grid state: the label-column width ([`SplitterTableState`]) and which rows show
/// their channels. `serde(default)`: hosts persist it inside their whole app state, where a field
/// this struct gains must not fail the load (and with it every other setting).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AttrGridState {
    pub table: SplitterTableState,
    /// Channel rows shown, unless the host owns them ([`AttrGridHooks::expansion`]).
    pub channels: ChannelExpansion,
    /// The displayed rows (field, channel) and per-field "a channel is animated" of the last
    /// frame, reused so a frame builds no new lists.
    #[serde(skip)]
    rows: Vec<(usize, Option<usize>)>,
    #[serde(skip)]
    auto: Vec<bool>,
    /// The label widget id of each displayed row, filled by the label column (which the table
    /// draws for ALL rows before the value column) so each editor is named by its OWN label.
    #[serde(skip)]
    labels: Vec<Option<egui::Id>>,
    /// Hashes of the keys whose editor the HOST drew (its `editor` hook returned Some) in the last
    /// frame, and the list being built this frame: the row menu, drawn before the editor, offers
    /// the value actions only to rows the grid itself edits.
    #[serde(skip)]
    overridden: Vec<u64>,
    #[serde(skip)]
    overridden_next: Vec<u64>,
    /// The value-box width measured last frame with the hash of its inputs: a steady frame
    /// measures nothing. Per grid (this state is the host's per-grid salt).
    #[serde(skip)]
    box_width: Option<(u64, f32)>,
}

/// Which rows show their channels: what the user chose, else automatic (a host passes `auto`,
/// e.g. "a channel is animated"), so keyed channels show without a click and stay hidden once
/// the user closed them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelExpansion {
    open: HashSet<String>,
    closed: HashSet<String>,
}

impl ChannelExpansion {
    /// Are `key`'s channels shown, given its automatic state?
    pub fn is_open(&self, key: &str, auto: bool) -> bool {
        self.open.contains(key) || (auto && !self.closed.contains(key))
    }
    /// Flip what `key` shows now, as the user's explicit choice.
    pub fn toggle(&mut self, key: &str, auto: bool) {
        self.set(key, !self.is_open(key, auto));
    }
    /// Show (`true`) or hide `key`'s channels as the user's explicit choice.
    pub fn set(&mut self, key: &str, open: bool) {
        self.open.remove(key);
        self.closed.remove(key);
        if open {
            self.open.insert(key.to_owned());
        } else {
            self.closed.insert(key.to_owned());
        }
    }
}

/// A row's animation, from the host ([`AttrGridHooks::anim`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnimState {
    /// The value has keys (a curve), not a static value.
    pub animated: bool,
    /// One of the keys is at the current time.
    pub keyed: bool,
}

/// A click on a row's animation controls, for the host to apply ([`AttrGridHooks::anim_intent`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimIntent {
    /// Start animating (key the current value at the current time).
    Animate,
    /// Stop animating: drop the curve, keep the value at the current time.
    Static,
    /// Key the current value at the current time.
    Key,
    /// Remove the key at the current time.
    Unkey,
}

/// A row's animation controls in a gutter `cell` (stopwatch, key): the grid's prefix gutter,
/// and any host row that shows the same attribute (e.g. a timeline lane). Some(intent) on a
/// click.
pub fn animation_controls(
    ui: &mut Ui,
    config: &AttrGridConfig,
    cell: egui::Rect,
    anim: AnimState,
) -> Option<AnimIntent> {
    let (timer, key) = config.icon_slots(ui, cell);
    let mut intent = None;
    let timer = icon_toggle(ui, timer, icons::TIMER, anim.animated, true);
    timer.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            timer.enabled(),
            if anim.animated {
                "Stop animating"
            } else {
                "Animate"
            },
        )
    });
    if timer.on_hover_text("Animate / stop animating").clicked() {
        intent = Some(if anim.animated {
            AnimIntent::Static
        } else {
            AnimIntent::Animate
        });
    }
    let key = icon_toggle(ui, key, icons::KEY, anim.keyed, true);
    key.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            key.enabled(),
            if anim.keyed { "Delete key" } else { "Set key" },
        )
    });
    if key
        .on_hover_text("Add / remove a key at the current time")
        .clicked()
    {
        intent = Some(if anim.keyed {
            AnimIntent::Unkey
        } else {
            AnimIntent::Key
        });
    }
    intent
}

/// The channel caret in the right icon slot of a gutter `cell` (the grid's actions gutter, or a
/// host row showing the same vector); true when clicked.
pub fn channel_caret(ui: &mut Ui, config: &AttrGridConfig, cell: egui::Rect, open: bool) -> bool {
    let (_, caret) = config.icon_slots(ui, cell);
    let icon = if open {
        icons::CARET_DOWN
    } else {
        icons::CARET_RIGHT
    };
    icon_toggle(ui, caret, icon, false, true)
        .on_hover_text("Show channels")
        .clicked()
}

/// The drag or text-edit gesture an edit belongs to (the dragged widget, or the focused text
/// field), so a host can merge one gesture's edits into one undo step.
pub fn edit_gesture(ctx: &egui::Context) -> Option<u64> {
    ctx.dragged_id()
        .or_else(|| {
            if ctx.text_edit_focused() {
                ctx.memory(|memory| memory.focused())
            } else {
                None
            }
        })
        .map(|id| id.value())
}

/// Where a pending Browse click is parked between the grid and its host.
fn browse_request_id() -> egui::Id {
    egui::Id::new("attr_grid_browse_request")
}

/// The attribute whose Browse icon was clicked, if any — call once per frame after the grid.
///
/// The grid cannot open a picker: which one to use belongs to the host (playa offers in-app or
/// native in Preferences). So the click is recorded and collected here.
pub fn take_browse_request(ctx: &egui::Context) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(browse_request_id()))
}

/// Remember where a pick came from, so this row opens there next time.
///
/// Call after the host's picker returns, with the same attribute key.
pub fn remember_browsed(ctx: &egui::Context, key: &str, picked: &str) {
    let id = egui::Id::new(("attr_grid_path", key));
    let mut st: egui_file_field::FileFieldState = ctx.data(|d| d.get_temp(id).unwrap_or_default());
    st.remember(picked);
    ctx.data_mut(|d| d.insert_temp(id, st));
}

/// Does this row's hint list ask for a path field?
///
/// `file` and `folder` are HINTS, like the `log` token on a Float slider - they say how to render,
/// not what the value may be.
pub fn is_path_hint(ui_options: &[String]) -> bool {
    ui_options
        .iter()
        .any(|o| o.eq_ignore_ascii_case("file") || o.eq_ignore_ascii_case("folder"))
}

/// Does this row's hint list ask for a font-family dropdown?
///
/// Same kind of token as `file` / `folder`: it says HOW to render a string, not what it may hold.
pub fn is_font_hint(ui_options: &[String]) -> bool {
    ui_options.iter().any(|o| o.eq_ignore_ascii_case("font"))
}

/// Where the host's font-family list is parked for the grid to read.
fn font_list_id() -> egui::Id {
    egui::Id::new("attr_grid_font_list")
}

/// Publish the families the host can actually render. Call once, not per frame.
///
/// The grid cannot enumerate fonts for the same reason it cannot open a picker: the authoritative
/// database belongs to whatever shapes the text. Without this a `font`-hinted row falls back to a
/// plain text field, which still edits the value correctly - a missing list degrades, it does not
/// break.
///
/// Stored as `Arc` because egui's store hands back a CLONE on every read, and cloning a system
/// font list once per row per frame is exactly the kind of per-frame allocation that does not show
/// up in a profile as one big cost.
pub fn install_font_families(
    ctx: &egui::Context,
    list: std::sync::Arc<egui_font_picker::FontList>,
) {
    ctx.data_mut(|d| d.insert_temp(font_list_id(), list));
}

/// Default starting width (points) of the label column.
const DEFAULT_LABEL_WIDTH: f32 = 180.0;

/// Render the attribute grid. `fields` is sorted in place by `order` then key;
/// `mixed` keys render dimmed. Returns `(key, new_value)` for every row the user
/// changed this frame.
pub fn render_grid(
    ui: &mut Ui,
    fields: &mut [AttrField],
    state: &mut AttrGridState,
    mixed: &HashSet<String>,
) -> Vec<(String, AttrValue)> {
    render_grid_impl(
        ui,
        fields,
        state,
        mixed,
        None,
        &AttrGridConfig::default(),
        &mut DefaultGridHooks,
    )
}

/// Render a vertically bounded, scrollable grid that invokes editors only for visible rows.
///
/// Use this for long top-level attribute lists. The body scrolls; column splitter state is shared
/// with [`render_grid`] (neither draws a header row — the grid is a self-evident `label : value`).
pub fn render_grid_virtualized(
    ui: &mut Ui,
    fields: &mut [AttrField],
    state: &mut AttrGridState,
    mixed: &HashSet<String>,
    max_body_height: f32,
) -> Vec<(String, AttrValue)> {
    render_grid_impl(
        ui,
        fields,
        state,
        mixed,
        Some(max_body_height),
        &AttrGridConfig::default(),
        &mut DefaultGridHooks,
    )
}

/// Optional cells in the same attribute grid. Zero widths draw no controls.
#[derive(Clone, Copy, Debug, Default)]
pub struct AttrGridConfig {
    /// Skip sorting when the caller maintains cached order/key sorted fields.
    pub fields_sorted: bool,
    pub prefix_width: f32,
    pub action_width: f32,
    /// Optional space reserved for the editor before clipping label text.
    pub min_editor_width: f32,
    pub row_height: Option<f32>,
    pub value_layout: ValueEditorLayout,
    /// Side of the row icons (animation controls, channel caret); 0 = the style's icon width.
    pub icon_side: f32,
    /// Gap between the two icons of a gutter.
    pub icon_gap: f32,
    /// Value-action buttons drawn in each row's action gutter, to the right of the host's
    /// `action_width` area (default none). Needs `icon_side > 0`: a button is one icon wide.
    pub actions: ValueActions,
    /// One width (px) for the value boxes of ranged rows, overriding the grid's own measure. A
    /// host that draws one grid per section sets it from [`value_box_width`] over the fields of
    /// ALL sections, so every box of the editor is the same width. None: this grid measures its
    /// own rows.
    pub value_box_width: Option<f32>,
}

/// The grid configuration of `metrics`: the animation controls in the prefix gutter and the row
/// toggles in the actions gutter, two icons each.
pub fn grid_config(metrics: AttrMetrics) -> AttrGridConfig {
    let normal = metrics.normalized();
    AttrGridConfig {
        prefix_width: metrics.gutter(),
        action_width: metrics.gutter(),
        // Three numeric fields and, on a colour row, the swatch after them: the fields
        // never shrink below `numeric_width` before the pane stops giving the editor room.
        min_editor_width: 3.0 * (normal.numeric_width + normal.component_gap) + normal.field_height,
        row_height: Some(metrics.row_height()),
        value_layout: metrics.value_layout(),
        fields_sorted: true,
        icon_side: normal.icon_side,
        icon_gap: normal.component_gap,
        actions: Default::default(),
        value_box_width: None,
    }
}

impl AttrGridConfig {
    /// The two icon slots of a gutter cell (left, right), each `icon_side` wide: where the grid
    /// puts its animation controls (prefix) and channel caret (actions, right slot), and where a
    /// host's own row icons line up with them.
    pub fn icon_slots(&self, ui: &Ui, cell: egui::Rect) -> (egui::Rect, egui::Rect) {
        let side = if self.icon_side > 0.0 {
            self.icon_side
        } else {
            ui.spacing().icon_width
        };
        let gap = self.icon_gap.max(0.0);
        let width = side.min(((cell.width() - gap) / 2.0).max(0.0));
        let height = side.min(cell.height());
        let left = egui::Rect::from_min_size(
            egui::pos2(cell.left(), cell.center().y - height / 2.0),
            egui::vec2(width, height),
        );
        let right = egui::Rect::from_min_size(
            egui::pos2(left.right() + gap, left.top()),
            egui::vec2(width, height),
        );
        (left, right)
    }

    /// Width of the value-action buttons, one `icon_side` slot each with `icon_gap` between.
    pub fn actions_width(&self) -> f32 {
        let n = self.actions.count();
        if n == 0 || !self.icon_side.is_finite() || self.icon_side <= 0.0 {
            return 0.0;
        }
        n as f32 * self.icon_side + (n - 1) as f32 * self.icon_gap.max(0.0)
    }

    /// Partition an actions gutter into the host's area (`action_width`: caret, host icons) and
    /// the value-action buttons after it.
    pub fn action_cells(&self, cell: egui::Rect) -> (egui::Rect, egui::Rect) {
        // The buttons come first in the budget; the host's area gets what is left.
        let buttons = self.actions_width().min(cell.width().max(0.0));
        let host = bounded_gutter(self.action_width, cell.width() - buttons, 1.0);
        (
            egui::Rect::from_min_size(cell.min, egui::vec2(host, cell.height())),
            egui::Rect::from_min_max(egui::pos2(cell.left() + host, cell.top()), cell.max),
        )
    }

    /// Responsive display width; a narrow pane does not overwrite the user's saved splitter.
    pub fn fitted_label_width(&self, available: f32, requested: f32) -> f32 {
        if !self.min_editor_width.is_finite() || self.min_editor_width <= 0.0 {
            return requested;
        }
        let actions = if self.action_width.is_finite() {
            self.action_width.max(0.0) + self.actions_width()
        } else {
            0.0
        };
        let reserved = self.min_editor_width
            + actions
            + 2.0 * SplitterTable::CELL_PADDING.x
            + SplitterTable::DEFAULT_SPLITTER_WIDTH;
        let maximum = (available - reserved).max(SplitterTable::DEFAULT_MIN_WIDTH);
        if requested.is_finite() {
            requested.max(SplitterTable::DEFAULT_MIN_WIDTH).min(maximum)
        } else {
            DEFAULT_LABEL_WIDTH.min(maximum)
        }
    }
    /// Partition a padded label cell into optional controls and its label.
    pub fn label_cells(&self, cell: egui::Rect) -> (egui::Rect, egui::Rect) {
        let width = bounded_gutter(self.prefix_width, cell.width(), 0.5);
        (
            egui::Rect::from_min_size(cell.min, egui::vec2(width, cell.height())),
            egui::Rect::from_min_max(egui::pos2(cell.left() + width, cell.top()), cell.max),
        )
    }

    /// Partition a padded value cell into its editor and optional actions.
    pub fn value_cells(&self, cell: egui::Rect) -> (egui::Rect, egui::Rect) {
        // The 35% cap is for the host's part only: a value-action button that cannot be drawn
        // would be a dead one.
        let host = bounded_gutter(self.action_width, cell.width(), 0.35);
        let width = (host + self.actions_width()).min(cell.width().max(0.0));
        (
            egui::Rect::from_min_max(cell.min, egui::pos2(cell.right() - width, cell.bottom())),
            egui::Rect::from_min_max(egui::pos2(cell.right() - width, cell.top()), cell.max),
        )
    }

    /// Project the default two-column grid onto a host-owned row (e.g. a timeline).
    /// Uses the same splitter, padding and cell partitions as the rendered grid.
    pub fn row_rects(
        &self,
        row: egui::Rect,
        label_width: f32,
    ) -> egui_widgets_config::attr_layout::AttrRowRects {
        let label_width = self.fitted_label_width(row.width(), label_width);
        let split = (row.left() + label_width).min(row.right());
        let label_cell = egui::Rect::from_min_max(row.min, egui::pos2(split, row.bottom()))
            .shrink2(SplitterTable::CELL_PADDING);
        let value_cell = egui::Rect::from_min_max(
            egui::pos2(
                (split + SplitterTable::DEFAULT_SPLITTER_WIDTH).min(row.right()),
                row.top(),
            ),
            row.max,
        )
        .shrink2(SplitterTable::CELL_PADDING);
        let (prefix, label) = self.label_cells(label_cell);
        let (value, actions) = self.value_cells(value_cell);
        egui_widgets_config::attr_layout::AttrRowRects {
            row,
            prefix,
            label,
            value,
            actions,
            trailing: egui::Rect::from_min_max(value_cell.max, value_cell.max),
        }
    }
}

fn bounded_gutter(requested: f32, available: f32, fraction: f32) -> f32 {
    if requested.is_finite() {
        requested.max(0.0).min(available.max(0.0) * fraction)
    } else {
        0.0
    }
}

/// Borrowed, statically dispatched host hooks. The grid knows no animation/domain types.
pub trait AttrGridHooks {
    fn display_label<'a>(&'a self, field: &'a AttrField) -> &'a str {
        &field.key
    }
    /// The prefix gutter of a row without animation controls ([`Self::anim`] is None).
    fn prefix(&mut self, _ui: &mut Ui, _field: &AttrField) {}
    /// The actions gutter; a row with channels keeps its right icon slot for the caret.
    fn actions(&mut self, _ui: &mut Ui, _field: &AttrField) {}
    /// Some: the row is animatable, the grid draws its controls (animation on / off, key at the
    /// current time) in the prefix gutter and reports clicks to [`Self::anim_intent`].
    fn anim(&self, _field: &AttrField) -> Option<AnimState> {
        None
    }
    fn anim_intent(&mut self, _field: &AttrField, _intent: AnimIntent) {}
    /// Some: the host owns the channel expansion (e.g. shared with a timeline); None: the grid
    /// state's own.
    fn expansion(&mut self) -> Option<&mut ChannelExpansion> {
        None
    }
    /// Some(reason): the row is shown but inactive (greyed, not editable), the reason as its
    /// tooltip, e.g. a parameter the current mode does not use.
    fn disabled(&self, _field: &AttrField) -> Option<String> {
        None
    }
    /// The user clicked "randomise across all objects" on `field`: the grid has already randomised
    /// this row's value (reported as an edit); the host does the same on its other selected objects.
    fn random_all(&mut self, _field: &AttrField) {}
    /// Entries of the row's right-click menu (on its label); none by default.
    fn context_menu(&mut self, _ui: &mut Ui, _field: &AttrField) {}
    /// Some overrides the typed editor; None renders the normal editor. `extra` is what the grid
    /// knows about the row (its label's id, to name your controls by; the validated default). A row
    /// whose editor you draw offers only Copy among the value actions.
    fn editor(
        &mut self,
        _ui: &mut Ui,
        _field: &mut AttrField,
        _mixed: bool,
        _layout: &ValueEditorLayout,
        _extra: &EditorCtx,
    ) -> Option<bool> {
        None
    }
    /// May `value` be written into `field` by a value action (reset, paste, randomise)? The host's
    /// invariants: a UInt that is an index into a list, a value only some of a multi-selection can
    /// take. A refused value is no edit, and its button / menu entry is disabled.
    fn accepts(&self, _field: &AttrField, _value: &AttrValue) -> bool {
        true
    }
}
struct DefaultGridHooks;
impl AttrGridHooks for DefaultGridHooks {}
/// Render the existing grid with optional gutter/actions and host-specific value adapters.
/// Section chrome and the two persisted splitter columns remain owned by existing callers.
pub fn render_grid_with_config(
    ui: &mut Ui,
    fields: &mut [AttrField],
    state: &mut AttrGridState,
    mixed: &HashSet<String>,
    config: &AttrGridConfig,
    hooks: &mut impl AttrGridHooks,
) -> Vec<(String, AttrValue)> {
    render_grid_impl(ui, fields, state, mixed, None, config, hooks)
}
/// The configured equivalent of render_grid_virtualized.
pub fn render_grid_virtualized_with_config(
    ui: &mut Ui,
    fields: &mut [AttrField],
    state: &mut AttrGridState,
    mixed: &HashSet<String>,
    max_body_height: f32,
    config: &AttrGridConfig,
    hooks: &mut impl AttrGridHooks,
) -> Vec<(String, AttrValue)> {
    render_grid_impl(
        ui,
        fields,
        state,
        mixed,
        Some(max_body_height),
        config,
        hooks,
    )
}
fn render_grid_impl(
    ui: &mut Ui,
    fields: &mut [AttrField],
    state: &mut AttrGridState,
    mixed: &HashSet<String>,
    max_body_height: Option<f32>,
    config: &AttrGridConfig,
    hooks: &mut impl AttrGridHooks,
) -> Vec<(String, AttrValue)> {
    if fields.is_empty() {
        ui.label("(no attributes)");
        return Vec::new();
    }
    // Buttons with no icon side configured use the style's icon size, never a silent zero.
    let resolved;
    let config =
        if config.actions.count() > 0 && !config.icon_side.is_finite() || config.icon_side <= 0.0 {
            resolved = AttrGridConfig {
                icon_side: ui.spacing().icon_width,
                ..*config
            };
            &resolved
        } else {
            config
        };

    if !config.fields_sorted {
        fields.sort_by(|a, b| {
            a.order
                .partial_cmp(&b.order)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.key.cmp(&b.key))
        });
    }

    let natural_height = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(ui.spacing().interact_size.y);
    let row_height = config
        .row_height
        .filter(|h| h.is_finite())
        .unwrap_or(natural_height)
        .max(0.0);

    // The value boxes of ranged rows share one width (see [`value_box_width`]).
    let value_layout = ValueEditorLayout {
        numeric_width: grid_box_width(ui, state, fields, config),
        ..config.value_layout
    };
    let mut changed: Vec<(String, AttrValue)> = Vec::new();
    // No header row: every attribute grid is a self-evident `label : value` list, so the
    // "Attribute | Value" banner is pure chrome. Column-resize still works — the splitter hit-tests
    // the body rect when there is no header (see `egui-splitter-table`).
    static COLUMNS: std::sync::LazyLock<[Column; 2]> =
        std::sync::LazyLock::new(|| [Column::new("", DEFAULT_LABEL_WIDTH), Column::new("", 0.0)]);
    let mut table = SplitterTable::new(Vec::new())
        .row_height(row_height)
        .show_header(false);
    if let Some(max_body_height) = max_body_height {
        table = table.max_body_height(max_body_height);
    }

    let responsive = config.min_editor_width.is_finite() && config.min_editor_width > 0.0;
    let requested_width = if responsive {
        state.table.widths.resize(1, DEFAULT_LABEL_WIDTH);
        state.table.widths[0]
    } else {
        DEFAULT_LABEL_WIDTH
    };
    let fitted_width = config.fitted_label_width(ui.available_width(), requested_width);
    if responsive {
        state.table.widths[0] = fitted_width;
    }

    // The displayed rows: each field, then its channels when expanded (shown automatically
    // while a channel is animated).
    let mut rows = std::mem::take(&mut state.rows);
    rows.clear();
    state.auto.clear();
    state.auto.extend(fields.iter().map(|field| {
        field
            .channels
            .iter()
            .any(|c| hooks.anim(c).is_some_and(|a| a.animated))
    }));
    {
        let expansion = match hooks.expansion() {
            Some(expansion) => &*expansion,
            None => &state.channels,
        };
        for (index, field) in fields.iter().enumerate() {
            rows.push((index, None));
            let count = subrow_count(field);
            if count > 0 && expansion.is_open(&field.key, state.auto[index]) {
                rows.extend((0..count).map(|c| (index, Some(c))));
            }
        }
    }
    let mut toggled: Option<(String, bool)> = None;
    // The row's inactive reason, asked once per row (both cells read it).
    let mut inactive: Option<(usize, Option<String>)> = None;
    // The row label widget ids (drawn in column 0 for every row, read by the editors in column 1).
    let mut labels = std::mem::take(&mut state.labels);
    labels.clear();
    labels.resize(rows.len(), None);
    let prev_owned = std::mem::take(&mut state.overridden);
    let mut owned_now = std::mem::take(&mut state.overridden_next);
    owned_now.clear();
    table.show_with_columns(
        ui,
        &mut state.table,
        rows.len(),
        &*COLUMNS,
        |ui, row, col| {
            let (index, channel) = rows[row];
            let parent_color = is_color_hint(&fields[index].ui_options);
            let channel_count = fields[index].channels.len();
            // A matrix has no channel fields of the host's: its rows are made here, edited as
            // vectors, and written back into the matrix after the row is drawn.
            let matrix = channel.is_some() && channel_count == 0;
            let reported = changed.len();
            let mut sub: Option<AttrField> = None;
            // Disjoint field borrows: `key`/`ui_options` shared, `value` mutable.
            let field = match channel {
                Some(c) if matrix => sub.insert(matrix_row_field(&fields[index], c)),
                Some(c) => &mut fields[index].channels[c],
                None => &mut fields[index],
            };
            let cell = ui.max_rect();
            if inactive.as_ref().is_none_or(|(r, _)| *r != row) {
                inactive = Some((row, hooks.disabled(field)));
            }
            let disabled = inactive.as_ref().and_then(|(_, reason)| reason.as_deref());
            if col == 0 {
                let (prefix, label) = config.label_cells(cell);
                if prefix.width() > 0.0 {
                    cell_ui(ui, prefix, (&field.key, "prefix"), |ui| {
                        if disabled.is_some() {
                            ui.disable();
                        }
                        match hooks.anim(field) {
                            Some(anim) => {
                                let cell = ui.max_rect();
                                if let Some(intent) = animation_controls(ui, config, cell, anim) {
                                    hooks.anim_intent(field, intent);
                                }
                            }
                            None => hooks.prefix(ui, field),
                        }
                    });
                }
                // An owned id: the closure below needs the field mutably (the row menu edits it).
                let label_cell = egui::Id::new((&field.key, "label"));
                cell_ui(ui, label, label_cell, |ui| {
                    // Inactive rows keep a live label (greyed text) so the reason tooltip and the
                    // context menu still work; only the editing cells are disabled.
                    let text = match channel {
                        Some(c) if matrix => MATRIX_ROW_LABELS[c.min(3)],
                        Some(c) => channel_label(c, channel_count, parent_color),
                        None => hooks.display_label(field),
                    };
                    let text = match disabled {
                        Some(_) => egui::RichText::new(text).color(ui.visuals().weak_text_color()),
                        None => egui::RichText::new(text),
                    };
                    if channel.is_some() {
                        // Channels indent under their vector.
                        ui.add_space(ui.spacing().indent);
                    }
                    let response = ui.add(
                        egui::Label::new(text)
                            .truncate()
                            .sense(egui::Sense::click()),
                    );
                    labels[row] = Some(response.id);
                    let hover = disabled.or(field.hint.as_deref()).unwrap_or(&field.key);
                    let response = response.on_hover_text(hover);
                    response.context_menu(|ui| {
                        // The host's entries first, then (after a separator, if there were any)
                        // the value actions the grid's one switch enables.
                        let top = ui.cursor().top();
                        hooks.context_menu(ui, field);
                        if ui.cursor().top() > top {
                            ui.separator();
                        }
                        let owned = prev_owned.contains(&egui::Id::new(&field.key).value());
                        let flags = field.value_actions(config);
                        let clicked =
                            actions::menu(ui, field, flags, disabled.is_none(), owned, &|f, v| {
                                hooks.accepts(f, v)
                            });
                        if let Some(action) = clicked {
                            let edited =
                                actions::run(ui.ctx(), field, action, &|f, v| hooks.accepts(f, v));
                            if edited {
                                changed.push((field.key.clone(), field.value.clone()));
                            }
                            if action == ValueAction::RandomAll && edited {
                                hooks.random_all(field);
                            }
                        }
                    });
                });
            } else {
                let dimmed = mixed.contains(&field.key);
                let (editor, actions) = config.value_cells(cell);
                let editor_id = ui.make_persistent_id((&field.key, "value"));
                let (edited, owned) = cell_ui(ui, editor, editor_id, |ui| {
                    if disabled.is_some() {
                        ui.disable();
                    }
                    let extra = EditorCtx {
                        default: actions::resolved_default(
                            &field.value,
                            field.default.as_ref(),
                            &field.ui_options,
                        )
                        .map(std::borrow::Cow::into_owned),
                        label_id: labels[row],
                        uniform: field.uniform,
                    };
                    // Drop a request nobody took, then run the editor.
                    actions::take_reset(ui.ctx());
                    let custom = hooks.editor(ui, field, dimmed, &value_layout, &extra);
                    let owned = custom.is_some();
                    let edited = custom.unwrap_or_else(|| {
                        render_value_editor(
                            ui,
                            &field.key,
                            &mut field.value,
                            dimmed,
                            &field.ui_options,
                            &value_layout,
                            &extra,
                        )
                    });
                    // The Ctrl / Cmd-click reset an editor asked for goes where the menu's goes.
                    let mut edited = edited;
                    if let Some(part) = actions::take_reset(ui.ctx()) {
                        edited |= actions::run_part(
                            ui.ctx(),
                            field,
                            ValueAction::Reset,
                            part,
                            &|f, v| hooks.accepts(f, v),
                        );
                    }
                    // On top of the (disabled) editor, so the reason shows anywhere in the cell.
                    if let Some(reason) = disabled {
                        ui.interact(ui.max_rect(), ui.id().with("why"), egui::Sense::hover())
                            .on_hover_text(reason);
                    }
                    (edited, owned)
                })
                .inner;
                if owned {
                    owned_now.push(egui::Id::new(&field.key).value());
                }
                // An inactive row never reports an edit, whatever its widgets did.
                if edited && disabled.is_none() {
                    changed.push((field.key.clone(), field.value.clone()));
                }
                if actions.width() > 0.0 {
                    // An owned id: the closure needs the field mutably (the buttons edit it).
                    let actions_cell = egui::Id::new((&field.key, "actions"));
                    cell_ui(ui, actions, actions_cell, |ui| {
                        let (host, buttons) = config.action_cells(ui.max_rect());
                        if host.width() > 0.0 && subrow_count(field) > 0 {
                            // The caret is navigation, live on an inactive row too.
                            let open = rows.get(row + 1).is_some_and(|next| next.1.is_some());
                            if channel_caret(ui, config, host, open) {
                                toggled = Some((field.key.clone(), !open));
                            }
                        }
                        // Value actions: copy works on an inactive row, nothing else does.
                        let mut fired = None;
                        let slots = config.actions;
                        let mine = field.value_actions(config);
                        let mut x = buttons.left();
                        for action in ValueAction::ALL.into_iter().filter(|a| slots.has(*a)) {
                            let slot = egui::Rect::from_min_size(
                                egui::pos2(x, buttons.top()),
                                egui::vec2(config.icon_side, buttons.height()),
                            );
                            x += config.icon_side + config.icon_gap.max(0.0);
                            // Never past the gutter: a slot that does not fit is not drawn.
                            if slot.right() > buttons.right() + 0.01
                                || !mine.has(action)
                                || !actions::applicable(field, action, owned)
                            {
                                continue;
                            }
                            let on = actions::enabled(
                                ui.ctx(),
                                field,
                                disabled.is_none(),
                                action,
                                &|f, v| hooks.accepts(f, v),
                            );
                            if value_button(ui, slot, action, on).clicked() {
                                fired = Some(action);
                            }
                        }
                        if let Some(action) = fired {
                            let edited =
                                actions::run(ui.ctx(), field, action, &|f, v| hooks.accepts(f, v));
                            if edited {
                                changed.push((field.key.clone(), field.value.clone()));
                            }
                            if action == ValueAction::RandomAll && edited {
                                hooks.random_all(field);
                            }
                        }
                        if disabled.is_some() {
                            ui.disable();
                        }
                        if host.width() > 0.0 {
                            cell_ui(ui, host, actions_cell.with("host"), |ui| {
                                hooks.actions(ui, field)
                            });
                        }
                    });
                }
            }
            // An edited matrix row is reported as the whole matrix, under the matrix's key.
            if let (true, Some(c), Some(sub)) = (changed.len() > reported, channel, sub.as_ref()) {
                changed.truncate(reported);
                let parent = &mut fields[index];
                match (&mut parent.value, &sub.value) {
                    (AttrValue::Mat3(m), AttrValue::Vec3(v)) => m[c] = *v,
                    (AttrValue::Mat4(m), AttrValue::Vec4(v)) => m[c] = *v,
                    _ => {}
                }
                changed.push((parent.key.clone(), parent.value.clone()));
            }
        },
    );
    if let Some((key, open)) = toggled {
        match hooks.expansion() {
            Some(expansion) => expansion.set(&key, open),
            None => state.channels.set(&key, open),
        }
    }
    state.rows = rows;
    state.labels = labels;
    state.overridden = owned_now;
    state.overridden_next = prev_owned;

    if responsive && state.table.widths[0] == fitted_width {
        state.table.widths[0] = requested_width;
    }
    changed
}

/// True when a Float slider's `ui_options` request a logarithmic scale.
///
/// Any token equal to `"log"` or `"logarithmic"` (case-insensitive, in any
/// position after `min`/`max`) opts in; its absence keeps the default linear
/// scale. Kept as a free function so the parse rule is unit-testable without
/// standing up egui.
fn is_log_scale(ui_options: &[String]) -> bool {
    ui_options
        .iter()
        .any(|s| s.eq_ignore_ascii_case("log") || s.eq_ignore_ascii_case("logarithmic"))
}

/// Labels of the rows of an expanded matrix.
const MATRIX_ROW_LABELS: [&str; 4] = ["Row 1", "Row 2", "Row 3", "Row 4"];

/// How many rows sit under `field` when it is expanded: its channel fields, or the rows of a
/// matrix (same caret, same expansion state).
fn subrow_count(field: &AttrField) -> usize {
    if !field.channels.is_empty() {
        return field.channels.len();
    }
    match field.value {
        AttrValue::Mat3(_) => 3,
        AttrValue::Mat4(_) => 4,
        _ => 0,
    }
}

/// Row `r` of a matrix field as a field of its own (a vector, keyed `key[r]`).
fn matrix_row_field(field: &AttrField, r: usize) -> AttrField {
    let value = match &field.value {
        AttrValue::Mat3(m) => m.get(r).map(|row| AttrValue::Vec3(*row)),
        AttrValue::Mat4(m) => m.get(r).map(|row| AttrValue::Vec4(*row)),
        _ => None,
    };
    AttrField::new(
        format!("{}[{r}]", field.key),
        value.unwrap_or_else(|| AttrValue::Label(String::new())),
    )
}

/// The collapsed face of a matrix row: its size. The rows are edited when it is expanded.
fn matrix_summary(ui: &mut Ui, n: usize) {
    ui.add(egui::Label::new(
        egui::RichText::new(format!("{n}x{n} matrix")).weak(),
    ))
    .on_hover_text("Expand to edit the rows");
}

/// A single-line text field filling the cell, so string rows end at the same edge as numbers.
fn text_field(ui: &mut Ui, text: &mut String) -> bool {
    let width = ui.available_width();
    ui.add(egui::TextEdit::singleline(text).desired_width(width))
        .changed()
}

/// A value-action button in `slot`: the action's icon in a square of the style's icon width,
/// named for assistive technology and with the action's label as its tooltip.
fn value_button(
    ui: &mut Ui,
    slot: egui::Rect,
    action: ValueAction,
    enabled: bool,
) -> egui::Response {
    icon_button_tip(
        ui,
        slot,
        action.glyph(),
        action.name(),
        action.label(),
        enabled,
    )
}

/// The colour row options of a hint list: `swatch_left` puts the swatch before the channel
/// fields, `decimals=N` sets the digits shown (default [`egui_colorpicker::COLOR_DECIMALS_DEFAULT`]).
fn color_opts(ui_options: &[String]) -> (bool, usize) {
    let left = ui_options
        .iter()
        .any(|o| o.eq_ignore_ascii_case("swatch_left"));
    let decimals = ui_options
        .iter()
        .find_map(|o| {
            o.to_ascii_lowercase()
                .strip_prefix("decimals=")?
                .parse()
                .ok()
        })
        .unwrap_or(egui_colorpicker::COLOR_DECIMALS_DEFAULT);
    (left, decimals)
}

/// Does this row's hint list make its slider range a soft one (the value box takes any value)?
fn is_soft(ui_options: &[String]) -> bool {
    ui_options.iter().any(|o| o.eq_ignore_ascii_case("soft"))
}

/// Does this string row's hint list ask for a button strip instead of a combo?
pub(crate) fn is_segmented_hint(ui_options: &[String]) -> bool {
    ui_options
        .iter()
        .any(|o| o.eq_ignore_ascii_case("segmented"))
}

/// The choices of a string row: its hints minus the `segmented` render token.
fn choices(ui_options: &[String]) -> impl Iterator<Item = &str> + Clone {
    ui_options
        .iter()
        .map(String::as_str)
        .filter(|o| !o.eq_ignore_ascii_case("segmented"))
}

/// Is a string row with these hints a choice (combo or strip), not a path / font / free text?
fn is_choice_row(ui_options: &[String]) -> bool {
    !is_path_hint(ui_options) && !is_font_hint(ui_options) && choices(ui_options).next().is_some()
}

/// The `[min, max]` slider range leading a numeric row's hints, if any.
fn slider_range(ui_options: &[String]) -> Option<(f64, f64)> {
    let min = ui_options.first()?.parse::<f64>().ok()?;
    let max = ui_options.get(1)?.parse::<f64>().ok()?;
    (min.is_finite() && max.is_finite() && min < max).then_some((min, max))
}

/// The label of channel `index` of a `count`-channel vector: R G B A for a colour, X Y Z W
/// otherwise.
fn channel_label(index: usize, count: usize, color: bool) -> &'static str {
    let names: [&str; 4] = if color {
        ["R", "G", "B", "A"]
    } else {
        ["X", "Y", "Z", "W"]
    };
    names
        .get(index)
        .copied()
        .filter(|_| count <= 4)
        .unwrap_or("·")
}

/// Does this row's hint list ask for a colour editor (`color` / `colour`)?
pub fn is_color_hint(ui_options: &[String]) -> bool {
    ui_options
        .iter()
        .any(|o| o.eq_ignore_ascii_case("color") || o.eq_ignore_ascii_case("colour"))
}

/// The slider + value box of a ranged numeric scalar. `default_step` quantises a linear
/// slider when the hints give no step (integers step by 1); a log slider keeps egui's smart
/// aim. With `soft` the value box accepts any value of the type.
///
/// The rail fills the cell and the box is `layout.numeric_width` wide (the grid passes the
/// grid-wide width of [`value_box_width`]), so every row ends at one
/// edge and [`AttrMetrics`] scales sliders like the other cells; Ctrl / Cmd-click resets to the
/// row's default.
fn range_slider<Num: egui::emath::Numeric>(
    ui: &mut Ui,
    value: &mut Num,
    ui_options: &[String],
    default_step: f64,
    layout: &ValueEditorLayout,
    extra: &EditorCtx,
) -> bool {
    let Some((min, max)) = slider_range(ui_options) else {
        return false;
    };
    let step = ui_options
        .get(2)
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(default_step);
    let logarithmic = is_log_scale(ui_options);
    let mut slider = egui_slider::SlimSlider::new(value, Num::from_f64(min)..=Num::from_f64(max))
        .logarithmic(logarithmic)
        .fill(true)
        .decimals(row_decimals(ui_options, Num::INTEGRAL, min, max))
        .field_width(layout.numeric_width);
    if let Some(h) = layout.row_height {
        slider = slider.row_height(h);
    }
    if !logarithmic {
        slider = slider.step_by(step);
    }
    if is_soft(ui_options) {
        slider = slider.field_range(Num::MIN..=Num::MAX);
    }
    if let Some(suffix) = ui_options.iter().find_map(|o| o.strip_prefix("suffix=")) {
        slider = slider.suffix(suffix);
    }
    // With a default the Ctrl / Cmd-click is a reset request, not a jump: the caller applies it.
    let resettable = extra.default.as_ref().is_some_and(|d| d.as_f64().is_some());
    if resettable {
        slider = slider.defer_reset();
    }
    if let Some(label) = extra.label_id {
        slider = slider.labelled_by(label);
    }
    let response = ui.add(slider);
    if resettable && egui_widgets_config::reset::ctrl_click(ui, &response) {
        actions::request_reset(ui.ctx(), None);
    }
    response.changed()
}

/// A ranged numeric row's value-box inputs: its span, the decimals its box shows and its
/// suffix; None for any other row.
fn box_spec(f: &AttrField) -> Option<(f64, f64, usize, &str)> {
    if !matches!(
        f.value,
        AttrValue::Float(_) | AttrValue::Int(_) | AttrValue::UInt(_) | AttrValue::Int64(_)
    ) {
        return None;
    }
    let (lo, hi) = slider_range(&f.ui_options)?;
    let suffix = f
        .ui_options
        .iter()
        .find_map(|o| o.strip_prefix("suffix="))
        .unwrap_or("");
    let integral = !matches!(f.value, AttrValue::Float(_));
    Some((
        lo,
        hi,
        row_decimals(&f.ui_options, integral, lo, hi),
        suffix,
    ))
}

/// The decimals of a ranged row's value box, from its `step` hint else its span: the ONE source
/// both the slider (which draws them) and [`value_box_width`] (which measures them) read.
fn row_decimals(ui_options: &[String], integral: bool, lo: f64, hi: f64) -> usize {
    let step = ui_options.get(2).and_then(|s| s.parse::<f64>().ok());
    egui_slider::field_decimals(integral, step, lo, hi)
}

/// The width of the value boxes of ranged numeric rows over `fields`: the widest formatted span
/// minimum / maximum of any such row ([`egui_slider::field_text`], at the row's decimals and
/// suffix, in the box's font), plus the box padding, clamped to `[base, 3 x base]`. The current
/// value counts only when it lies outside its span (a soft row), so the width does not move
/// while a value is dragged. A host that draws one grid per section passes the fields of ALL
/// sections and sets [`AttrGridConfig::value_box_width`] from the result, so every box of the
/// editor is as wide. `base` is [`ValueEditorLayout::numeric_width`]; an invalid one (NaN, not
/// positive) falls back to the default.
pub fn value_box_width<'a>(
    ui: &Ui,
    fields: impl IntoIterator<Item = &'a AttrField>,
    base: f32,
) -> f32 {
    let base = sane_base(base);
    let font = egui_slider::field_font(ui.style());
    let pad = 2.0 * ui.spacing().button_padding.x + 2.0;
    let color = ui.visuals().text_color();
    let mut widest = 0.0_f32;
    for f in fields {
        let Some((lo, hi, decimals, suffix)) = box_spec(f) else {
            continue;
        };
        let current = f
            .value
            .as_f64()
            .filter(|v| !(lo.min(hi)..=lo.max(hi)).contains(v));
        for v in [Some(lo), Some(hi), current].into_iter().flatten() {
            let text = egui_slider::field_text(v, decimals, suffix);
            let w = ui.fonts_mut(|fonts| fonts.layout_no_wrap(text, font.clone(), color).size().x);
            widest = widest.max(w);
        }
    }
    (widest + pad).clamp(base, 3.0 * base)
}

/// `base` if it is a usable width, else the layout default (a NaN would panic `f32::clamp`).
fn sane_base(base: f32) -> f32 {
    if base.is_finite() && base > 0.0 {
        base
    } else {
        ValueEditorLayout::default().numeric_width
    }
}

/// This grid's value-box width: the host's [`AttrGridConfig::value_box_width`], else
/// [`value_box_width`] over `fields`, cached in the grid's `state` against a hash of everything
/// the measure depends on (font, padding, scale, the ranged rows' hints and out-of-span values),
/// so a steady frame measures nothing.
fn grid_box_width(
    ui: &Ui,
    state: &mut AttrGridState,
    fields: &[AttrField],
    config: &AttrGridConfig,
) -> f32 {
    use std::hash::{Hash, Hasher};
    let base = sane_base(config.value_layout.numeric_width);
    if let Some(w) = config.value_box_width.filter(|w| w.is_finite() && *w > 0.0) {
        return w;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    base.to_bits().hash(&mut hasher);
    egui_slider::field_font(ui.style()).hash(&mut hasher);
    ui.spacing().button_padding.x.to_bits().hash(&mut hasher);
    ui.pixels_per_point().to_bits().hash(&mut hasher);
    for f in fields.iter().filter(|f| box_spec(f).is_some()) {
        f.ui_options.hash(&mut hasher);
        if let (Some((lo, hi, ..)), Some(v)) = (box_spec(f), f.value.as_f64())
            && !(lo.min(hi)..=lo.max(hi)).contains(&v)
        {
            v.to_bits().hash(&mut hasher);
        }
    }
    let key = hasher.finish();
    if let Some((k, w)) = state.box_width
        && k == key
    {
        return w;
    }
    let w = value_box_width(ui, fields, base);
    state.box_width = Some((key, w));
    w
}

/// What an editor knows about its row beyond the value and the hints.
#[derive(Clone, Debug, Default)]
pub struct EditorCtx {
    /// The row's default ([`AttrField::default`], as the grid validates it: the value's type):
    /// Ctrl / Cmd-click on a numeric field resets to it.
    pub default: Option<AttrValue>,
    /// The row label's widget id, naming the editor in the accessibility tree.
    pub label_id: Option<egui::Id>,
    /// The row offers the uniform-scale lock ([`AttrField::uniform`]).
    pub uniform: bool,
}

/// Render the value editor for one attribute. `mixed` dims the editor (the value differs across
/// a multi-selection). Returns `true` if the user changed it. `layout` sizes the numeric cells
/// (the grid passes its own; [`ValueEditorLayout::default`] elsewhere) and `extra` carries the
/// row's default (Ctrl / Cmd-click resets a numeric field to it), its label id and its lock option.
///
/// - [`AttrValue::Str`] with `ui_options` → combo box of those options, or a button strip with
///   the `segmented` token.
/// - A numeric scalar with `ui_options` `[min, max, (step)]` → slider; add a `"log"` token for a
///   logarithmic scale, `"soft"` for a soft range, `"suffix=px"` for a unit.
/// - Otherwise a type-appropriate editor; non-editable kinds render read-only.
pub fn render_value_editor(
    ui: &mut Ui,
    key: &str,
    value: &mut AttrValue,
    mixed: bool,
    ui_options: &[String],
    layout: &ValueEditorLayout,
    extra: &EditorCtx,
) -> bool {
    let weak = ui.visuals().weak_text_color();
    let mut changed = false;

    ui.scope(|ui| {
        if mixed {
            ui.visuals_mut().override_text_color = Some(weak);
        }

        if let Some(edited) = fixed_numeric_editor(ui, key, value, ui_options, layout, extra) {
            changed |= edited;
            return;
        }
        match value {
            // String hinted `file` / `folder` → a path field with a Browse icon.
            //
            // Checked BEFORE the combo branch: those tokens are hints, not choices, and a
            // hinted path would otherwise render as a one-entry dropdown of the word "file".
            AttrValue::Str(current) if is_path_hint(ui_options) => {
                let folder = ui_options.iter().any(|o| o.eq_ignore_ascii_case("folder"));
                let kind = if folder {
                    egui_file_field::PickKind::Folder
                } else {
                    egui_file_field::PickKind::File
                };
                // Where this row last browsed from lives in egui's own store, keyed by the
                // attribute name: `render_value_editor` is a stateless free function and giving
                // it a state parameter would change a signature every caller already uses.
                let mem_id = egui::Id::new(("attr_grid_path", key));
                let mut st: egui_file_field::FileFieldState =
                    ui.data(|d| d.get_temp(mem_id).unwrap_or_default());
                let r = egui_file_field::FileField::new(current, &mut st)
                    .kind(kind)
                    .hint("(none)")
                    .show(ui);
                if r.browse_clicked() {
                    // The grid does not open pickers either. It records the ask; the host takes it
                    // with `take_browse_request` and opens whichever picker the user chose.
                    ui.data_mut(|d| d.insert_temp(browse_request_id(), key.to_string()));
                }
                ui.data_mut(|d| d.insert_temp(mem_id, st));
                changed |= r.changed();
            }
            // String hinted `font` → a family dropdown with a filter.
            //
            // Before the combo branch for the same reason the path branch is: `font` is a hint,
            // and a hinted family would otherwise become a one-entry dropdown of the word "font".
            AttrValue::Str(current) if is_font_hint(ui_options) => {
                match ui.data(|d| {
                    d.get_temp::<std::sync::Arc<egui_font_picker::FontList>>(font_list_id())
                }) {
                    Some(list) => {
                        let r = egui_font_picker::FontPicker::new(current, &list).show(ui);
                        changed |= r.changed();
                    }
                    // No host list installed: edit it as text rather than offering an empty
                    // dropdown the user cannot get a value out of.
                    None => {
                        changed |= text_field(ui, current);
                    }
                }
            }
            // String choices hinted `segmented` → an always-visible button strip.
            AttrValue::Str(current)
                if is_segmented_hint(ui_options) && is_choice_row(ui_options) =>
            {
                let list = choices(ui_options);
                let n = list.clone().count();
                let known = list.clone().position(|c| c == current);
                // A value that is not a choice still shows, as one more selected segment; a mixed
                // row selects nothing (its objects differ).
                let unlisted = known.is_none() && !mixed;
                let selected = if mixed {
                    None
                } else {
                    known.or(unlisted.then_some(n))
                };
                let at = |i: usize| {
                    std::borrow::Cow::Borrowed(if i < n {
                        list.clone().nth(i).unwrap_or("")
                    } else {
                        current.as_str()
                    })
                };
                let total = n + usize::from(unlisted);
                let picked = egui_widgets_config::segmented_pick(ui, total, selected, at, |_| "");
                if let Some(i) = picked
                    && i < n
                    && let Some(choice) = choices(ui_options).nth(i)
                {
                    *current = choice.to_owned();
                    changed = true;
                }
            }
            // String with choices → combo box.
            AttrValue::Str(current) if is_choice_row(ui_options) => {
                let mut selected = current.clone();
                let shown = ComboBox::from_id_salt(format!("attr_{key}"))
                    .width(ui.available_width())
                    .selected_text(&selected)
                    .show_ui(ui, |ui| {
                        for opt in choices(ui_options) {
                            ui.selectable_value(&mut selected, opt.to_owned(), opt);
                        }
                    });
                if let Some(label) = extra.label_id {
                    shown.response.labelled_by(label);
                }
                if &selected != current {
                    *current = selected;
                    changed = true;
                }
            }

            // Numeric scalar with [min, max, (step)] → slider + value box; `log` / `soft` tokens.
            AttrValue::Float(v) if slider_range(ui_options).is_some() => {
                changed |= range_slider(ui, v, ui_options, 0.01, layout, extra);
            }
            AttrValue::Int(v) if slider_range(ui_options).is_some() => {
                changed |= range_slider(ui, v, ui_options, 1.0, layout, extra);
            }
            AttrValue::UInt(v) if slider_range(ui_options).is_some() => {
                changed |= range_slider(ui, v, ui_options, 1.0, layout, extra);
            }
            AttrValue::Int64(v) if slider_range(ui_options).is_some() => {
                changed |= range_slider(ui, v, ui_options, 1.0, layout, extra);
            }
            // `color` hint → channel fields + HDR picker swatch.
            // `fixed_numeric_editor` took every other Vec3 / Vec4 above, so what reaches here
            // IS a colour row.
            AttrValue::Vec3(rgb) => {
                // The swatch ends at the cell's right edge, like a slider's value box.
                let (left, decimals) = color_opts(ui_options);
                let mut c = [rgb[0], rgb[1], rgb[2], 1.0];
                if egui_colorpicker::color_edit_flush(ui, &mut c, 3, left, decimals).changed() {
                    *rgb = [c[0], c[1], c[2]];
                    changed = true;
                }
            }
            AttrValue::Vec4(rgba) => {
                let (left, decimals) = color_opts(ui_options);
                changed |=
                    egui_colorpicker::color_edit_flush(ui, rgba, 4, left, decimals).changed();
            }

            AttrValue::Bool(v) => {
                changed |= ui.checkbox(v, "").changed();
            }
            AttrValue::Str(s) => {
                changed |= text_field(ui, s);
            }
            // Plain numeric scalars were drawn by `fixed_numeric_editor` above (a ranged one by the
            // slider arms before it): nothing reaches here.
            AttrValue::Int8(_)
            | AttrValue::Int(_)
            | AttrValue::UInt(_)
            | AttrValue::Int64(_)
            | AttrValue::Float(_) => {}
            AttrValue::Mat3(_) => matrix_summary(ui, 3),
            AttrValue::Mat4(_) => matrix_summary(ui, 4),
            AttrValue::List(items) => {
                // Inline-edit numeric elements (e.g. chromaticities, timecode);
                // non-numeric elements render read-only.
                ui.horizontal_wrapped(|ui| {
                    for item in items.iter_mut() {
                        match item {
                            AttrValue::Int(v) => {
                                changed |= ui.add(egui::DragValue::new(v).speed(1.0)).changed();
                            }
                            AttrValue::Int64(v) => {
                                changed |= ui.add(egui::DragValue::new(v).speed(1.0)).changed();
                            }
                            AttrValue::UInt(v) => {
                                let mut t = *v as i64;
                                if ui.add(egui::DragValue::new(&mut t).speed(1.0)).changed() {
                                    *v = t.max(0) as u32;
                                    changed = true;
                                }
                            }
                            AttrValue::Float(v) => {
                                changed |= ui.add(egui::DragValue::new(v).speed(0.01)).changed();
                            }
                            other => {
                                ui.label(format!("{other:?}"));
                            }
                        }
                    }
                });
            }
            AttrValue::Label(s) => {
                ui.label(s.as_str());
            }
        }
    });

    changed
}

/// One numeric atom in a clipped, fixed cell; vector axes are part of the control. With
/// `resettable`, a Ctrl / Cmd-click asks the caller to reset component `part` of the row.
fn numeric_atom(
    ui: &mut Ui,
    value: &mut AttrValue,
    axis: &'static str,
    resettable: bool,
    part: Option<usize>,
) -> bool {
    ui.spacing_mut().interact_size.x = ui.available_width().max(0.0);
    let response = match value {
        AttrValue::Int8(v) => ui.add(egui::DragValue::new(v).speed(1.0).prefix(axis)),
        AttrValue::Int(v) => ui.add(egui::DragValue::new(v).speed(1.0).prefix(axis)),
        AttrValue::Int64(v) => ui.add(egui::DragValue::new(v).speed(1.0).prefix(axis)),
        AttrValue::UInt(v) => ui.add(egui::DragValue::new(v).speed(1.0).prefix(axis)),
        AttrValue::Float(v) => ui.add(egui::DragValue::new(v).speed(0.1).prefix(axis)),
        other => {
            ui.label(format!("{other:?}"));
            return false;
        }
    };
    if resettable && egui_widgets_config::reset::ctrl_click(ui, &response) {
        actions::request_reset(ui.ctx(), part);
    }
    response.changed()
}

/// The uniform lock of a Vec3 / Vec4 row, kept in egui memory: the proportions to scale from
/// (`base`): the values when the lock was switched on, then the values at the start of each edit
/// gesture (a drag or a typed edit), except that a flat zero vector keeps the proportions it had,
/// so a vector dragged to zero and released can still come back.
#[derive(Clone, Copy, Debug, Default)]
struct UniformLock {
    base: [f32; 4],
}

/// Scale every component from the locked `base` proportions by the ratio the edited one reached
/// (`now[i] / base[i]`). Scaling from `base`, not from the previous frame, is what lets a vector
/// dragged to zero come back to its proportions instead of staying flat. A base component that
/// is ~0 gives no ratio, so the change is added to every base component instead.
fn propagate_uniform(base: &[f32], before: &[f32], now: &mut [f32]) {
    /// Below this a component counts as unchanged / as zero for the ratio.
    const ZERO: f32 = 1e-6;
    let n = now.len().min(before.len()).min(base.len());
    let Some(i) = (0..n).find(|&i| (now[i] - before[i]).abs() > ZERO) else {
        return;
    };
    let (anchor, new) = (base[i], now[i]);
    for j in (0..n).filter(|&j| j != i) {
        now[j] = if anchor.abs() > ZERO {
            base[j] * (new / anchor)
        } else {
            base[j] + (new - anchor)
        };
    }
}

/// The numeric rows: plain scalars and Vec3 / Vec4 (and numeric lists). Every component field
/// shares the cell equally, so the last one ends at the cell's right edge, the edge of a slider's
/// value box; a `uniform` row gives the last `icon_width` to its lock toggle instead. A Ctrl / Cmd
/// click on a field resets it to `extra.default`.
fn fixed_numeric_editor(
    ui: &mut Ui,
    key: &str,
    value: &mut AttrValue,
    options: &[String],
    layout: &ValueEditorLayout,
    extra: &EditorCtx,
) -> Option<bool> {
    // Ranged scalars draw a slider and colour rows a picker (render_value_editor_with_layout).
    let scalar = matches!(
        value,
        AttrValue::Float(_) | AttrValue::Int(_) | AttrValue::UInt(_) | AttrValue::Int64(_)
    );
    if (scalar && slider_range(options).is_some())
        || (matches!(value, AttrValue::Vec3(_) | AttrValue::Vec4(_)) && is_color_hint(options))
    {
        return None;
    }
    let count = match value {
        AttrValue::Int8(_)
        | AttrValue::Int(_)
        | AttrValue::Int64(_)
        | AttrValue::UInt(_)
        | AttrValue::Float(_) => 1,
        AttrValue::Vec3(_) => 3,
        AttrValue::Vec4(_) => 4,
        AttrValue::List(items)
            if items.iter().all(|item| {
                matches!(
                    item,
                    AttrValue::Int8(_)
                        | AttrValue::Int(_)
                        | AttrValue::Int64(_)
                        | AttrValue::UInt(_)
                        | AttrValue::Float(_)
                )
            }) =>
        {
            items.len()
        }
        _ => return None,
    };
    let height = layout
        .row_height
        .filter(|v| v.is_finite())
        .unwrap_or(ui.spacing().interact_size.y)
        .max(0.0)
        .min(ui.max_rect().height().max(0.0));
    let width = ui.available_width();
    let width = if width.is_finite() {
        width.max(0.0)
    } else {
        layout.numeric_width.max(0.0) * count.max(3) as f32
    };
    let (mut rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let mut changed = false;
    let axes = ["X ", "Y ", "Z ", "W "];
    let components = |value: &AttrValue| match value {
        AttrValue::Vec3(v) => [v[0], v[1], v[2], 0.0],
        AttrValue::Vec4(v) => *v,
        _ => [0.0; 4],
    };
    let before = components(value);
    // The lock toggle takes the right end of the cell; the components share the rest.
    let mut lock = None;
    if extra.uniform && matches!(value, AttrValue::Vec3(_) | AttrValue::Vec4(_)) {
        let side = ui
            .spacing()
            .icon_width
            .min(rect.width() / 2.0)
            .min(rect.height());
        let slot = egui::Rect::from_min_max(egui::pos2(rect.right() - side, rect.top()), rect.max);
        rect.max.x -= side + layout.gap.max(0.0);
        // Scoped by the cell's id, which is the grid's and the row's.
        let id = ui.id().with("uniform");
        lock = ui.data(|d| d.get_temp::<UniformLock>(id));
        if icon_toggle(ui, slot, icons::LOCK, lock.is_some(), true)
            .on_hover_text("Uniform: all components move together")
            .clicked()
        {
            lock = match lock {
                Some(_) => None,
                None => Some(UniformLock { base: before }),
            };
            ui.data_mut(|d| {
                if let Some(l) = lock {
                    d.insert_temp(id, l);
                } else {
                    d.remove_temp::<UniformLock>(id);
                }
            });
        }
        // Outside a gesture the current values are the proportions to scale from (a drag then
        // starts from the values it began with); a flat zero vector has none, so it keeps its base.
        if let Some(l) = &mut lock
            && edit_gesture(ui.ctx()).is_none()
            && before.iter().any(|c| c.abs() > 1e-6)
        {
            l.base = before;
        }
    }
    // One vector component field: its response, and a Ctrl / Cmd-click reset to the default's.
    let default = extra.default.as_ref();
    let component = |ui: &mut Ui, v: &mut f32, i: usize, cell: egui::Rect| -> bool {
        cell_ui(ui, cell, (key, i), |ui| {
            let r = ui
                .add_sized(
                    cell.size(),
                    egui::DragValue::new(v).speed(0.1).prefix(axes[i]),
                )
                .on_hover_text(axes[i].trim());
            if default.is_some_and(|d| d.component(i).is_some()) && egui_widgets_config::reset::ctrl_click(ui, &r) {
                actions::request_reset(ui.ctx(), Some(i));
            }
            r.changed()
        })
        .inner
    };
    match &mut *value {
        AttrValue::Vec3(values) => {
            for (i, (v, cell)) in values
                .iter_mut()
                .zip(layout.spread_cells(rect, 3))
                .enumerate()
            {
                changed |= component(ui, v, i, cell);
            }
        }
        AttrValue::Vec4(values) => {
            for (i, (v, cell)) in values
                .iter_mut()
                .zip(layout.spread_cells(rect, 4))
                .enumerate()
            {
                changed |= component(ui, v, i, cell);
            }
        }
        AttrValue::List(items) => {
            for (i, (item, cell)) in items
                .iter_mut()
                .zip(layout.spread_cells(rect, count))
                .enumerate()
            {
                let can = default.is_some_and(|d| d.component(i).is_some());
                changed |= cell_ui(ui, cell, (key, i), |ui| {
                    numeric_atom(ui, item, "", can, Some(i))
                })
                .inner;
            }
        }
        scalar => {
            if let Some(cell) = layout.spread_cells(rect, 1).next() {
                let can = default.is_some_and(|d| d.component(0).is_some());
                changed |= cell_ui(ui, cell, (key, 0), |ui| {
                    numeric_atom(ui, scalar, "", can, None)
                })
                .inner;
            }
        }
    }
    if let Some(l) = lock {
        if changed {
            let n = if matches!(value, AttrValue::Vec3(_)) {
                3
            } else {
                4
            };
            let mut now = components(value);
            propagate_uniform(&l.base[..n], &before[..n], &mut now[..n]);
            match &mut *value {
                AttrValue::Vec3(v) => v.copy_from_slice(&now[..3]),
                AttrValue::Vec4(v) => *v = now,
                _ => {}
            }
        }
        ui.data_mut(|d| d.insert_temp(ui.id().with("uniform"), l));
    }
    Some(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_hints_parse_slider_ranges_and_colour_tokens() {
        let hints = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(slider_range(&hints(&["0", "10"])), Some((0.0, 10.0)));
        assert_eq!(
            slider_range(&hints(&["0.5", "2", "0.1", "log", "soft"])),
            Some((0.5, 2.0))
        );
        assert_eq!(
            slider_range(&hints(&["5", "1"])),
            None,
            "an empty range draws no slider"
        );
        assert_eq!(slider_range(&hints(&["color"])), None);
        assert!(is_color_hint(&hints(&["color"])) && is_color_hint(&hints(&["0", "1", "Colour"])));
        assert!(!is_color_hint(&hints(&["0", "1"])));
    }

    #[test]
    fn state_saved_before_a_field_existed_still_loads() {
        let state: AttrGridState = serde_json::from_str(r#"{"table":{"widths":[120.0]}}"#).unwrap();
        assert_eq!(state.channels, ChannelExpansion::default());
    }

    #[test]
    fn channel_expansion_follows_the_user_then_the_automatic_state() {
        let mut e = ChannelExpansion::default();
        assert!(
            !e.is_open("p", false) && e.is_open("p", true),
            "automatic until chosen"
        );
        e.toggle("p", true);
        assert!(
            !e.is_open("p", true),
            "closed by the user stays closed while animated"
        );
        e.toggle("p", true);
        assert!(e.is_open("p", false), "opened by the user stays open");
    }

    #[test]
    fn channels_show_when_animated_and_animation_controls_report_intents() {
        struct Hooks {
            animated: HashSet<String>,
            keyed: HashSet<String>,
            drawn: Vec<(String, egui::Rect)>,
            intents: Vec<(String, AnimIntent)>,
        }
        impl AttrGridHooks for Hooks {
            fn anim(&self, field: &AttrField) -> Option<AnimState> {
                Some(AnimState {
                    animated: self.animated.contains(&field.key),
                    keyed: self.keyed.contains(&field.key),
                })
            }
            fn anim_intent(&mut self, field: &AttrField, intent: AnimIntent) {
                self.intents.push((field.key.clone(), intent));
            }
            fn editor(
                &mut self,
                ui: &mut Ui,
                field: &mut AttrField,
                _mixed: bool,
                _layout: &ValueEditorLayout,
                _: &EditorCtx,
            ) -> Option<bool> {
                self.drawn.push((field.key.clone(), ui.max_rect()));
                None
            }
        }
        let channel = |i: usize, v: f32| AttrField::new(format!("/p/{i}"), AttrValue::Float(v));
        let mut fields = [
            AttrField::new("/p", AttrValue::Vec3([1.0, 2.0, 3.0])).with_channels(vec![
                channel(0, 1.0),
                channel(1, 2.0),
                channel(2, 3.0),
            ]),
            AttrField::new("/q", AttrValue::Float(0.5)).with_order(1.0),
        ];
        let config = grid_config(AttrMetrics::default());
        let mut hooks = Hooks {
            animated: HashSet::from(["/p/1".to_owned()]),
            keyed: HashSet::new(),
            drawn: Vec::new(),
            intents: Vec::new(),
        };
        let ctx = egui::Context::default();
        let mut state = AttrGridState::default();
        // Returns the grid's left edge.
        let mut frame = |events: Vec<egui::Event>, hooks: &mut Hooks, state: &mut AttrGridState| {
            hooks.drawn.clear();
            let mut left = 0.0;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    left = ui.max_rect().left();
                    render_grid_with_config(
                        ui,
                        &mut fields,
                        state,
                        &HashSet::new(),
                        &config,
                        hooks,
                    );
                },
            );
            output.textures_delta.clear();
            left
        };
        frame(Vec::new(), &mut hooks, &mut state);
        let keys: Vec<_> = hooks.drawn.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            ["/p", "/p/0", "/p/1", "/p/2", "/q"],
            "an animated channel opens its vector"
        );

        state.channels.set("/p", false);
        let left = frame(Vec::new(), &mut hooks, &mut state);
        let keys: Vec<_> = hooks.drawn.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["/p", "/q"], "closed by the user");

        // Click the stopwatch, then the key, of /q (prefix gutter: the first two icon slots).
        let row = hooks.drawn.iter().find(|(k, _)| k == "/q").unwrap().1;
        let side = config.icon_side;
        let x0 = left + egui_splitter_table::SplitterTable::CELL_PADDING.x;
        for x in [x0 + side * 0.5, x0 + side * 1.5 + config.icon_gap] {
            let pos = egui::pos2(x, row.center().y);
            let press = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            frame(
                vec![egui::Event::PointerMoved(pos), press(true)],
                &mut hooks,
                &mut state,
            );
            frame(vec![press(false)], &mut hooks, &mut state);
        }
        assert_eq!(
            hooks.intents,
            [
                ("/q".to_owned(), AnimIntent::Animate),
                ("/q".to_owned(), AnimIntent::Key)
            ]
        );
    }

    #[test]
    fn showing_an_out_of_range_value_never_edits_it() {
        let hints = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut fields = [
            AttrField::new("/axis", AttrValue::UInt(5)).with_ui_options(hints(&["0", "2", "1"])),
            AttrField::new("/gain", AttrValue::Float(7.5)).with_ui_options(hints(&["0", "1"])),
        ];
        let ctx = egui::Context::default();
        let mut state = AttrGridState::default();
        for _ in 0..2 {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                let changed = render_grid(ui, &mut fields, &mut state, &HashSet::new());
                assert!(changed.is_empty(), "a draw is not an edit: {changed:?}");
            });
            output.textures_delta.clear();
        }
        assert_eq!(fields[0].value, AttrValue::UInt(5));
        assert_eq!(fields[1].value, AttrValue::Float(7.5));
    }

    #[test]
    fn disabled_rows_ignore_edits_and_ranged_rows_draw_sliders() {
        #[derive(Default)]
        struct Hooks {
            rects: std::collections::HashMap<String, egui::Rect>,
        }
        impl AttrGridHooks for Hooks {
            fn disabled(&self, field: &AttrField) -> Option<String> {
                (field.key == "/off").then(|| "Not used here".to_owned())
            }
            fn editor(
                &mut self,
                ui: &mut Ui,
                field: &mut AttrField,
                _mixed: bool,
                _layout: &ValueEditorLayout,
                _: &EditorCtx,
            ) -> Option<bool> {
                self.rects.insert(field.key.clone(), ui.max_rect());
                None
            }
        }
        let hints = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut fields = [
            AttrField::new("/off", AttrValue::Float(1.0))
                .with_order(0.0)
                .with_ui_options(hints(&["0", "2"])),
            AttrField::new("/on", AttrValue::Float(1.0))
                .with_order(1.0)
                .with_ui_options(hints(&["0", "2"])),
        ];
        let ctx = egui::Context::default();
        let mut state = AttrGridState::default();
        let mut hooks = Hooks::default();
        let mut edited = Vec::new();
        let mut frame = |events: Vec<egui::Event>, fields: &mut [AttrField], hooks: &mut Hooks| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    edited.extend(render_grid_with_config(
                        ui,
                        fields,
                        &mut state,
                        &HashSet::new(),
                        &AttrGridConfig::default(),
                        hooks,
                    ));
                },
            );
            output.textures_delta.clear();
        };
        frame(Vec::new(), &mut fields, &mut hooks);
        // Click each slider rail a little right of its start: a live slider jumps there.
        for key in ["/off", "/on"] {
            let rect = hooks.rects[key];
            let pos = egui::pos2(rect.left() + 12.0, rect.center().y);
            let press = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            frame(
                vec![egui::Event::PointerMoved(pos), press(true)],
                &mut fields,
                &mut hooks,
            );
            frame(vec![press(false)], &mut fields, &mut hooks);
        }
        assert_eq!(
            fields[0].value,
            AttrValue::Float(1.0),
            "a disabled row does not edit"
        );
        assert_ne!(
            fields[1].value,
            AttrValue::Float(1.0),
            "a ranged row is a live slider"
        );
        assert!(edited.iter().all(|(key, _)| key != "/off"));
    }

    #[test]
    fn optional_hooks_keep_grid_state_keys_and_reserved_cells_aligned() {
        #[derive(Default)]
        struct Hooks {
            prefixes: Vec<egui::Rect>,
            values: Vec<(String, egui::Rect)>,
            actions: Vec<egui::Rect>,
            mixed: Vec<bool>,
            edit: bool,
        }
        impl AttrGridHooks for Hooks {
            fn display_label<'a>(&'a self, field: &'a AttrField) -> &'a str {
                field.key.rsplit('/').next().unwrap()
            }
            fn prefix(&mut self, ui: &mut Ui, field: &AttrField) {
                self.prefixes.push(ui.max_rect());
                if field.key != "/visible" {
                    egui_widgets_config::square_icon(ui, ui.max_rect(), "*", true);
                }
            }
            fn actions(&mut self, ui: &mut Ui, _field: &AttrField) {
                self.actions.push(ui.max_rect());
            }
            fn editor(
                &mut self,
                ui: &mut Ui,
                field: &mut AttrField,
                mixed: bool,
                _layout: &ValueEditorLayout,
                _: &EditorCtx,
            ) -> Option<bool> {
                self.values.push((field.key.clone(), ui.max_rect()));
                self.mixed.push(mixed);
                if self.edit && field.key == "/count" {
                    field.value = AttrValue::UInt(11);
                    Some(true)
                } else {
                    None
                }
            }
        }
        let ctx = egui::Context::default();
        let mut state = AttrGridState::default();
        let mut fields = [
            AttrField::new("/count", AttrValue::UInt(10)).with_order(0.0),
            AttrField::new("/position", AttrValue::Vec3([1.0, 2000.0, -0.01])).with_order(1.0),
            AttrField::new("/visible", AttrValue::Bool(true)).with_order(2.0),
        ];
        let mut hooks = Hooks::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.set_max_width(600.0);
            let changes = render_grid_with_config(
                ui,
                &mut fields,
                &mut state,
                &HashSet::new(),
                &AttrGridConfig::default(),
                &mut hooks,
            );
            assert!(changes.is_empty());
        });
        output.textures_delta.clear();
        assert!(hooks.prefixes.is_empty() && hooks.actions.is_empty());
        let original_x = hooks.values[0].1.left();
        hooks.values.clear();
        hooks.edit = true;
        let config = AttrGridConfig {
            prefix_width: 48.0,
            action_width: 24.0,
            row_height: Some(24.0),
            ..Default::default()
        };
        let mut changes = Vec::new();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.set_max_width(600.0);
            changes = render_grid_with_config(
                ui,
                &mut fields,
                &mut state,
                &HashSet::from(["/position".into()]),
                &config,
                &mut hooks,
            );
        });
        output.textures_delta.clear();
        assert_eq!(changes, vec![("/count".into(), AttrValue::UInt(11))]);
        assert_eq!(hooks.prefixes.len(), 3);
        assert_eq!(hooks.actions.len(), 3);
        assert_eq!(hooks.values[0].1.left(), original_x);
        for ((_, value), (prefix, action)) in hooks
            .values
            .iter()
            .zip(hooks.prefixes.iter().zip(&hooks.actions))
        {
            assert_eq!(value.center().y, prefix.center().y);
            assert_eq!(value.center().y, action.center().y);
            assert_eq!(value.left(), hooks.values[0].1.left());
            assert_eq!(value.right(), action.left());
            assert_eq!(prefix.width(), 48.0);
            assert_eq!(action.width(), 24.0);
        }
        assert!(hooks.mixed.last().is_some_and(|mixed| !mixed));
        assert_eq!(fields[1].value, AttrValue::Vec3([1.0, 2000.0, -0.01]));
    }

    #[test]
    fn cached_sorted_path_matches_default_and_narrow_gutters_leave_editors() {
        #[derive(Default)]
        struct Hooks {
            keys: Vec<String>,
            editors: Vec<egui::Rect>,
            actions: Vec<egui::Rect>,
        }
        impl AttrGridHooks for Hooks {
            fn editor(
                &mut self,
                ui: &mut Ui,
                field: &mut AttrField,
                _: bool,
                _: &ValueEditorLayout,
                _: &EditorCtx,
            ) -> Option<bool> {
                self.keys.push(field.key.clone());
                self.editors.push(ui.max_rect());
                Some(false)
            }
            fn actions(&mut self, ui: &mut Ui, _: &AttrField) {
                self.actions.push(ui.max_rect());
            }
        }
        let ctx = egui::Context::default();
        let mut fields = [
            AttrField::new("b", AttrValue::UInt(1)).with_order(1.0),
            AttrField::new("a", AttrValue::UInt(2)).with_order(1.0),
            AttrField::new("z", AttrValue::UInt(3)).with_order(0.0),
        ];
        let mut state = AttrGridState::default();
        let mut first = Hooks::default();
        let mut config = AttrGridConfig {
            prefix_width: 52.0,
            action_width: 52.0,
            ..Default::default()
        };
        let mut out = ctx.run_ui(Default::default(), |ui| {
            ui.set_max_width(220.0);
            render_grid_with_config(
                ui,
                &mut fields,
                &mut state,
                &HashSet::new(),
                &config,
                &mut first,
            );
        });
        out.textures_delta.clear();
        assert_eq!(first.keys, ["z", "a", "b"]);
        config.fields_sorted = true;
        let mut cached = Hooks::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            ui.set_max_width(220.0);
            render_grid_with_config(
                ui,
                &mut fields,
                &mut state,
                &HashSet::new(),
                &config,
                &mut cached,
            );
        });
        out.textures_delta.clear();
        assert_eq!(cached.keys, first.keys);
        assert_eq!(cached.editors, first.editors);
        for (editor, action) in cached.editors.iter().zip(&cached.actions) {
            assert!(editor.width() > 0.0);
            assert!(action.width() <= 0.35 * (editor.width() + action.width()) + 0.001);
            assert!((editor.right() - action.left()).abs() < 0.05);
        }
    }

    #[test]
    fn numeric_controls_fit_padded_row_and_keep_scalar_vector_baselines() {
        let ctx = egui::Context::default();
        let mut centers = Vec::new();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            for (index, mut value) in [AttrValue::Float(1.0), AttrValue::Vec3([1.0, 2.0, 3.0])]
                .into_iter()
                .enumerate()
            {
                let target = egui::Rect::from_min_size(
                    egui::pos2(20.0, 30.0 + index as f32 * 24.0),
                    egui::vec2(160.0, 22.0),
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(target)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                let layout = ValueEditorLayout {
                    row_height: Some(24.0),
                    ..Default::default()
                };
                assert_eq!(
                    fixed_numeric_editor(
                        &mut child,
                        "row",
                        &mut value,
                        &[],
                        &layout,
                        &EditorCtx::default()
                    ),
                    Some(false)
                );
                assert!(child.min_rect().height() <= target.height() + 0.01);
                centers.push(target.center().y);
            }
        });
        let text_centers: Vec<f32> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::epaint::Shape::Text(text) = &shape.shape {
                    Some(text.pos.y + text.galley.size().y * 0.5)
                } else {
                    None
                }
            })
            .collect();
        output.textures_delta.clear();
        // egui may run more than one sizing pass; every actual text baseline must agree.
        assert!(text_centers.len() >= 4);
        for center in text_centers {
            assert!(centers.iter().any(|target| (center - target).abs() <= 0.5));
        }
    }

    #[test]
    fn heterogeneous_lists_use_existing_editor_and_numeric_lists_keep_all_entries() {
        let ctx = egui::Context::default();
        let mut mixed = AttrValue::List(vec![AttrValue::UInt(7), AttrValue::Bool(true)]);
        let original = mixed.clone();
        let mut numeric = AttrValue::List((0..8).map(AttrValue::UInt).collect());
        let mut out = ctx.run_ui(Default::default(), |ui| {
            assert_eq!(
                fixed_numeric_editor(
                    ui,
                    "mixed",
                    &mut mixed,
                    &[],
                    &ValueEditorLayout::default(),
                    &EditorCtx::default()
                ),
                None
            );
            assert_eq!(
                fixed_numeric_editor(
                    ui,
                    "numeric",
                    &mut numeric,
                    &[],
                    &ValueEditorLayout::default(),
                    &EditorCtx::default()
                ),
                Some(false)
            );
        });
        out.textures_delta.clear();
        assert_eq!(mixed, original);
        assert_eq!(
            numeric,
            AttrValue::List((0..8).map(AttrValue::UInt).collect())
        );
    }

    #[test]
    fn narrow_grid_reserves_editor_space_without_losing_saved_splitter() {
        struct Probe(Option<egui::Rect>);
        impl AttrGridHooks for Probe {
            fn editor(
                &mut self,
                ui: &mut Ui,
                _: &mut AttrField,
                _: bool,
                _: &ValueEditorLayout,
                _: &EditorCtx,
            ) -> Option<bool> {
                self.0 = Some(ui.max_rect());
                Some(false)
            }
        }
        let config = AttrGridConfig {
            min_editor_width: 112.0,
            action_width: 26.0,
            ..Default::default()
        };
        let mut state = AttrGridState::default();
        state.table.widths = vec![210.0];
        let mut fields = [AttrField::new("Translate", AttrValue::Vec3([0.0; 3]))];
        let ctx = egui::Context::default();
        for width in [300.0, 460.0] {
            let mut probe = Probe(None);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 100.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        render_grid_with_config(
                            ui,
                            &mut fields,
                            &mut state,
                            &HashSet::new(),
                            &config,
                            &mut probe,
                        );
                    });
                },
            );
            output.textures_delta.clear();
            assert!(probe.0.unwrap().width() >= 112.0 - 0.01);
            assert_eq!(state.table.widths, [210.0]);
        }
    }

    #[test]
    fn fields_sort_by_order_then_key() {
        let mut fields = [
            AttrField::new("b", AttrValue::Int(1)).with_order(1.0),
            AttrField::new("a", AttrValue::Int(2)).with_order(1.0),
            AttrField::new("z", AttrValue::Int(3)).with_order(0.0),
        ];
        // Mirror render_grid's sort (without egui).
        fields.sort_by(|a, b| {
            a.order
                .partial_cmp(&b.order)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.key.cmp(&b.key))
        });
        let keys: Vec<&str> = fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["z", "a", "b"]);
    }

    #[test]
    fn log_scale_detected_case_insensitively_anywhere() {
        let opts = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Default (no token) stays linear — backward-safe.
        assert!(!is_log_scale(&opts(&["0.0", "1.0"])));
        assert!(!is_log_scale(&opts(&["0.0", "1.0", "0.01"])));
        // Token opts in, in any position and any case.
        assert!(is_log_scale(&opts(&["0.001", "10.0", "log"])));
        assert!(is_log_scale(&opts(&[
            "0.001",
            "10.0",
            "0.001",
            "Logarithmic"
        ])));
        assert!(is_log_scale(&opts(&["LOG", "0.0", "1.0"])));
    }

    #[test]
    fn attr_value_roundtrips_serde() {
        let v = AttrValue::List(vec![AttrValue::Float(0.5), AttrValue::Int64(42)]);
        let json = serde_json::to_string(&v).unwrap();
        let back: AttrValue = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

/// Reset, typed copy / paste, randomise, the segmented strip, slider fill and AccessKit names.
#[cfg(test)]
mod row_tests {
    use super::*;
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    fn hints(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Records each row's editor rect (and leaves the editor to the grid).
    #[derive(Default)]
    struct Probe(std::collections::HashMap<String, egui::Rect>);
    impl AttrGridHooks for Probe {
        fn editor(
            &mut self,
            ui: &mut Ui,
            field: &mut AttrField,
            _: bool,
            _: &ValueEditorLayout,
            _: &EditorCtx,
        ) -> Option<bool> {
            self.0.insert(field.key.clone(), ui.max_rect());
            None
        }
    }

    /// One frame of the grid with the given events; returns what the user changed.
    fn frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        fields: &mut [AttrField],
        state: &mut AttrGridState,
        probe: &mut Probe,
    ) -> Vec<(String, AttrValue)> {
        let mut changed = Vec::new();
        let mut out = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                changed = render_grid_with_config(
                    ui,
                    fields,
                    state,
                    &HashSet::new(),
                    &AttrGridConfig::default(),
                    probe,
                );
            },
        );
        out.textures_delta.clear();
        changed
    }

    #[test]
    fn ctrl_click_on_a_slider_resets_to_the_field_default() {
        let ctx = egui::Context::default();
        let mut fields = [AttrField::new("/gain", AttrValue::Float(0.9))
            .with_ui_options(hints(&["0", "1"]))
            .with_default(AttrValue::Float(0.25))];
        let (mut state, mut probe) = (AttrGridState::default(), Probe::default());
        frame(&ctx, Vec::new(), &mut fields, &mut state, &mut probe);
        let rect = probe.0["/gain"];
        let pos = egui::pos2(rect.left() + rect.width() * 0.3, rect.center().y);
        let ctrl = egui::Modifiers::COMMAND;
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: ctrl,
        };
        let down = vec![
            egui::Event::ModifiersChanged(ctrl),
            egui::Event::PointerMoved(pos),
            press(true),
        ];
        frame(&ctx, down, &mut fields, &mut state, &mut probe);
        let changed = frame(
            &ctx,
            vec![press(false)],
            &mut fields,
            &mut state,
            &mut probe,
        );
        assert_eq!(fields[0].value, AttrValue::Float(0.25));
        assert_eq!(
            changed,
            [("/gain".to_owned(), AttrValue::Float(0.25))],
            "the reset is an edit"
        );
    }

    #[test]
    fn reset_restores_default_clamped_and_is_a_noop_without_one() {
        let mut f = AttrField::new("/a", AttrValue::Float(0.9)).with_ui_options(hints(&["0", "1"]));
        assert!(!actions::reset(&mut f), "no default, nothing to reset");
        f.default = Some(AttrValue::Float(0.9));
        assert!(!actions::reset(&mut f), "already at the default");
        f.default = Some(AttrValue::Float(7.0));
        assert!(actions::reset(&mut f));
        assert_eq!(f.value, AttrValue::Float(1.0), "held in the hard range");
    }

    #[test]
    fn paste_writes_only_a_matching_type_and_a_valid_choice() {
        let ctx = egui::Context::default();
        let mut float = AttrField::new("/f", AttrValue::Float(1.0));
        let mut vec3 = AttrField::new("/v", AttrValue::Vec3([0.0; 3]));
        let mut mode = AttrField::new("/m", AttrValue::Str("a".into())).with_ui_options(hints(&[
            "a",
            "b",
            "segmented",
        ]));
        assert!(!actions::paste(&ctx, &mut float), "empty clipboard");

        actions::clipboard_set(&ctx, AttrValue::Float(3.5));
        assert!(
            !actions::paste(&ctx, &mut vec3),
            "a float does not go into a vector"
        );
        assert!(!actions::paste(&ctx, &mut mode), "nor into a string");
        assert!(actions::paste(&ctx, &mut float));
        assert_eq!(float.value, AttrValue::Float(3.5));

        actions::clipboard_set(&ctx, AttrValue::Str("zzz".into()));
        assert!(!actions::paste(&ctx, &mut mode), "not one of the choices");
        actions::clipboard_set(&ctx, AttrValue::Str("b".into()));
        assert!(actions::paste(&ctx, &mut mode));
        assert_eq!(mode.value, AttrValue::Str("b".into()));

        assert!(actions::same_kind(
            &AttrValue::List(vec![AttrValue::Int(1)]),
            &AttrValue::List(vec![AttrValue::Int(9)])
        ));
        assert!(!actions::same_kind(
            &AttrValue::List(vec![AttrValue::Int(1)]),
            &AttrValue::List(vec![AttrValue::Int(1), AttrValue::Int(2)])
        ));
        assert!(!actions::same_kind(&AttrValue::Int(1), &AttrValue::UInt(1)));
    }

    #[test]
    fn randomise_is_seeded_bounded_and_opt_in() {
        let mut plain = AttrField::new("/a", AttrValue::Float(0.5));
        assert!(
            !actions::randomize(&mut plain, &mut random::Pcg::new(1)),
            "no policy, no randomise"
        );

        let mk = || {
            AttrField::new("/a", AttrValue::Float(0.5))
                .with_ui_options(hints(&["0", "1"]))
                .with_random(Random::Normal {
                    mean: 0.5,
                    std: 50.0,
                })
        };
        let (mut a, mut b) = (mk(), mk());
        assert!(actions::randomize(&mut a, &mut random::Pcg::new(7)));
        actions::randomize(&mut b, &mut random::Pcg::new(7));
        assert_eq!(a.value, b.value, "the same seed gives the same value");
        let AttrValue::Float(v) = a.value else {
            panic!("still a float")
        };
        assert!((0.0..=1.0).contains(&v), "held in the slider range: {v}");

        let mut color = AttrField::new("/c", AttrValue::Vec4([0.0, 0.0, 0.0, 0.5]))
            .with_ui_options(hints(&["color"]))
            .with_random(Random::uniform(0.0..=1.0));
        actions::randomize(&mut color, &mut random::Pcg::new(3));
        let AttrValue::Vec4(c) = color.value else {
            panic!("still a colour")
        };
        assert_eq!(c[3], 0.5, "alpha is kept");
    }

    #[test]
    fn segmented_hint_draws_a_strip_whose_click_selects_a_choice() {
        let ctx = egui::Context::default();
        let mut fields = [AttrField::new("/mode", AttrValue::Str("off".into()))
            .with_ui_options(hints(&["off", "selected", "all", "segmented"]))];
        let (mut state, mut probe) = (AttrGridState::default(), Probe::default());
        frame(&ctx, Vec::new(), &mut fields, &mut state, &mut probe);
        let rect = probe.0["/mode"];
        // The middle of the third of three equal segments (the token is no segment).
        let pos = egui::pos2(rect.left() + rect.width() * 5.0 / 6.0, rect.center().y);
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(
            &ctx,
            vec![egui::Event::PointerMoved(pos), press(true)],
            &mut fields,
            &mut state,
            &mut probe,
        );
        let changed = frame(
            &ctx,
            vec![press(false)],
            &mut fields,
            &mut state,
            &mut probe,
        );
        assert_eq!(fields[0].value, AttrValue::Str("all".into()));
        assert_eq!(changed.len(), 1);
    }

    /// `SliderClamping::Edits` at grid level: a stored value outside the row's range is shown as
    /// it is (a draw is not an edit), and an edit of it IS clamped to the range.
    #[test]
    fn a_stored_out_of_range_value_is_kept_on_show_but_an_edit_is_clamped() {
        let mut harness = Harness::new_ui_state(
            |ui, st: &mut (Vec<AttrField>, AttrGridState)| {
                let (fields, state) = st;
                render_grid(ui, fields, state, &HashSet::new());
            },
            (
                vec![AttrField::new("/gain", AttrValue::Float(7.5)).with_ui_options(hints(&["0", "1"]))],
                AttrGridState::default(),
            ),
        );
        harness.run();
        assert_eq!(harness.state().0[0].value, AttrValue::Float(7.5), "shown, not clamped");
        harness.get_by_role(egui::accesskit::Role::SpinButton).click();
        harness.run();
        harness.get_by_role(egui::accesskit::Role::SpinButton).type_text("9");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(harness.state().0[0].value, AttrValue::Float(1.0), "an edit is clamped to the range");
    }

    #[test]
    fn segmented_strips_are_findable_by_label_and_report_selection() {
        let harness = Harness::new_ui_state(
            |ui, mode: &mut String| {
                let mut fields = [AttrField::new("/mode", AttrValue::Str(mode.clone()))
                    .with_ui_options(hints(&["off", "all", "segmented"]))];
                let mut state = AttrGridState::default();
                for (_, v) in render_grid(ui, &mut fields, &mut state, &HashSet::new()) {
                    if let AttrValue::Str(s) = v {
                        *mode = s;
                    }
                }
            },
            "off".to_owned(),
        );
        let on = harness.get_by_label("off");
        assert_eq!(
            on.accesskit_node().toggled(),
            Some(egui::accesskit::Toggled::True)
        );
        assert_eq!(
            on.accesskit_node().role(),
            egui::accesskit::Role::RadioButton
        );
        harness.get_by_label("all").click();
    }

    #[test]
    fn slider_rows_end_at_one_edge_with_a_metric_sized_box() {
        // The compact metrics a real host applies: font and padding fit the 36 px box.
        let metrics = AttrMetrics::default();
        let config = grid_config(metrics);
        let harness = Harness::new_ui_state(
            move |ui, st: &mut (Vec<AttrField>, AttrGridState, Probe)| {
                metrics.apply(ui);
                let (fields, state, probe) = st;
                render_grid_with_config(ui, fields, state, &HashSet::new(), &config, probe);
            },
            (
                vec![
                    AttrField::new("/a", AttrValue::Float(0.5))
                        .with_ui_options(hints(&["0", "1"]))
                        .with_order(0.0),
                    AttrField::new("/b", AttrValue::Int(3))
                        .with_ui_options(hints(&["0", "10"]))
                        .with_order(1.0),
                ],
                AttrGridState::default(),
                Probe::default(),
            ),
        );
        let boxes: Vec<egui::Rect> = harness
            .get_all_by_role(egui::accesskit::Role::SpinButton)
            .map(|n| n.rect())
            .collect();
        assert_eq!(boxes.len(), 2);
        assert!(
            boxes
                .iter()
                .all(|b| (b.width() - metrics.numeric_width).abs() < 0.01),
            "box width from the metrics: {boxes:?}"
        );
        assert_eq!(boxes[0].right(), boxes[1].right(), "one right edge");
        let cell = harness.state().2.0["/a"];
        assert!(
            (boxes[0].right() - cell.right()).abs() < 0.01,
            "flush with the editor cell"
        );
    }

    /// A span of 0..16384 and a 0.0001-step value widen the grid's value boxes (all rows alike,
    /// one right edge) so the formatted text fits; plain rows keep the metric width.
    #[test]
    fn value_boxes_widen_to_the_formatted_extremes() {
        let metrics = AttrMetrics::default();
        let config = grid_config(metrics);
        let mut harness = Harness::new_ui_state(
            move |ui, st: &mut (Vec<AttrField>, AttrGridState, Probe)| {
                metrics.apply(ui);
                let (fields, state, probe) = st;
                render_grid_with_config(ui, fields, state, &HashSet::new(), &config, probe);
            },
            (
                vec![
                    AttrField::new("/a", AttrValue::Float(0.02))
                        .with_ui_options(hints(&["0", "1", "0.0001"]))
                        .with_order(0.0),
                    AttrField::new("/b", AttrValue::Int(16384))
                        .with_ui_options(hints(&["0", "16384"]))
                        .with_order(1.0),
                ],
                AttrGridState::default(),
                Probe::default(),
            ),
        );
        harness.run();
        let boxes: Vec<egui::Rect> = harness
            .get_all_by_role(egui::accesskit::Role::SpinButton)
            .map(|n| n.rect())
            .collect();
        assert_eq!(boxes.len(), 2);
        let fields = harness.state().0.clone();
        let mut output = egui::Context::default().run_ui(Default::default(), |ui| {
            metrics.apply(ui);
            let w = value_box_width(ui, &fields, metrics.numeric_width);
            assert!(w > metrics.numeric_width && w <= 3.0 * metrics.numeric_width);
            // The box draws `field_text`: row /a (step 0.0001) at 4 decimals, /b whole. The
            // width holds the widest of those with the padding, and is what the grid drew.
            let font = egui_slider::field_font(ui.style());
            let widest = ["1.0000", "16384"]
                .iter()
                .map(|t| {
                    ui.fonts_mut(|f| {
                        f.layout_no_wrap((*t).into(), font.clone(), egui::Color32::WHITE)
                            .size()
                            .x
                    })
                })
                .fold(0.0, f32::max);
            assert!(
                w >= widest + 2.0 * ui.spacing().button_padding.x,
                "{w} vs {widest}"
            );
            assert_eq!(egui_slider::field_text(0.02, 4, ""), "0.0200");
            assert!(
                (boxes[0].width() - w).abs() < 0.01,
                "drawn {boxes:?} vs {w}"
            );
        });
        output.textures_delta.clear();
        assert!(
            boxes
                .iter()
                .all(|b| b.width() > metrics.numeric_width + 1.0),
            "boxes widened: {boxes:?}"
        );
        assert!((boxes[0].width() - boxes[1].width()).abs() < 0.01);
        assert_eq!(boxes[0].right(), boxes[1].right(), "one right edge");
    }

    /// The grid measures once: a second frame over the same rows reuses the width cached in its
    /// state (a planted width under the same key is what gets drawn), a value dragged inside its
    /// span changes nothing, and a host width overrides the measure.
    #[test]
    fn value_box_width_is_cached_stable_and_overridable() {
        let metrics = AttrMetrics::default();
        let config = grid_config(metrics);
        let mut harness = Harness::builder().with_max_steps(8).build_ui_state(
            move |ui, st: &mut (Vec<AttrField>, AttrGridState, Probe, AttrGridConfig)| {
                metrics.apply(ui);
                let (fields, state, probe, config) = st;
                render_grid_with_config(ui, fields, state, &HashSet::new(), config, probe);
            },
            (
                vec![
                    AttrField::new("/a", AttrValue::Float(0.02))
                        .with_ui_options(hints(&["0", "1", "0.0001"])),
                ],
                AttrGridState::default(),
                Probe::default(),
                config,
            ),
        );
        harness.run();
        let (key, width) = harness.state().1.box_width.expect("measured");
        harness.state_mut().0[0].value = AttrValue::Float(0.9);
        harness.run();
        assert_eq!(
            harness.state().1.box_width,
            Some((key, width)),
            "value in span"
        );
        harness.state_mut().1.box_width = Some((key, 77.0));
        harness.run();
        let drawn = harness
            .get_by_role(egui::accesskit::Role::SpinButton)
            .rect();
        assert!(
            (drawn.width() - 77.0).abs() < 0.01,
            "cache hit drew {drawn:?}"
        );
        harness.state_mut().3.value_box_width = Some(90.0);
        harness.run();
        let drawn = harness
            .get_by_role(egui::accesskit::Role::SpinButton)
            .rect();
        assert!(
            (drawn.width() - 90.0).abs() < 0.01,
            "host width drew {drawn:?}"
        );
    }

    /// A NaN or non-positive base width is replaced, not fed to `f32::clamp`.
    #[test]
    fn value_box_width_survives_an_invalid_base() {
        let mut output = egui::Context::default().run_ui(Default::default(), |ui| {
            let field =
                AttrField::new("/a", AttrValue::Float(0.5)).with_ui_options(hints(&["0", "1"]));
            for base in [f32::NAN, -1.0, 0.0, f32::INFINITY] {
                let w = value_box_width(ui, [&field], base);
                assert!(w.is_finite() && w > 0.0, "{base} gave {w}");
            }
        });
        output.textures_delta.clear();
    }

    #[test]
    fn grid_sliders_expose_role_value_and_the_row_label() {
        let harness = Harness::new_ui_state(
            |ui, v: &mut f32| {
                let mut fields = [AttrField::new("Gain", AttrValue::Float(*v))
                    .with_ui_options(hints(&["0", "2"]))];
                let mut state = AttrGridState::default();
                render_grid(ui, &mut fields, &mut state, &HashSet::new());
            },
            1.5_f32,
        );
        let node = harness.get_by_role_and_label(egui::accesskit::Role::Slider, "Gain");
        assert_eq!(node.accesskit_node().numeric_value(), Some(1.5));
    }

    #[test]
    fn choice_editor_is_named_by_its_row_and_can_select() {
        let mut harness = Harness::new_ui_state(
            |ui, (fields, state): &mut (Vec<AttrField>, AttrGridState)| {
                render_grid(ui, fields, state, &HashSet::new());
            },
            (
                vec![
                    AttrField::new("Edge Mode", AttrValue::Str("Clamp".into()))
                        .with_ui_options(hints(&["Clamp", "Wrap"])),
                ],
                AttrGridState::default(),
            ),
        );
        let combo = harness.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Edge Mode");
        assert_eq!(combo.value().as_deref(), Some("Clamp"));
        combo.click();
        harness.run();
        harness.get_by_label("Wrap").click();
        harness.run();
        assert_eq!(harness.state().0[0].value, AttrValue::Str("Wrap".into()));
    }

    #[test]
    fn animation_actions_are_named_and_report_the_named_intent() {
        let mut harness = Harness::new_ui_state(
            |ui, (anim, intents): &mut (AnimState, Vec<AnimIntent>)| {
                let config = grid_config(AttrMetrics::default());
                let cell = ui
                    .allocate_exact_size(egui::vec2(40.0, 24.0), egui::Sense::hover())
                    .0;
                if let Some(intent) = animation_controls(ui, &config, cell, *anim) {
                    match intent {
                        AnimIntent::Animate => anim.animated = true,
                        AnimIntent::Static => anim.animated = false,
                        AnimIntent::Key => anim.keyed = true,
                        AnimIntent::Unkey => anim.keyed = false,
                    }
                    intents.push(intent);
                }
            },
            (
                AnimState {
                    animated: false,
                    keyed: false,
                },
                Vec::new(),
            ),
        );
        for name in ["Set key", "Delete key", "Animate", "Stop animating"] {
            harness
                .get_by_role_and_label(egui::accesskit::Role::Button, name)
                .click();
            harness.run();
        }
        assert_eq!(
            harness.state().1,
            [
                AnimIntent::Key,
                AnimIntent::Unkey,
                AnimIntent::Animate,
                AnimIntent::Static
            ]
        );
    }

    // --- value-action gutter, numeric alignment, uniform lock ---

    /// Editor rects per key, plus the rows the host was told to randomise across all objects.
    #[derive(Default)]
    struct ActProbe {
        rects: std::collections::HashMap<String, egui::Rect>,
        random_all: Vec<String>,
    }
    impl AttrGridHooks for ActProbe {
        fn editor(
            &mut self,
            ui: &mut Ui,
            field: &mut AttrField,
            _: bool,
            _: &ValueEditorLayout,
            _: &EditorCtx,
        ) -> Option<bool> {
            self.rects.insert(field.key.clone(), ui.max_rect());
            None
        }
        fn random_all(&mut self, field: &AttrField) {
            self.random_all.push(field.key.clone());
        }
    }

    /// One frame under `config`; the edits it reported.
    fn frame_cfg(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        fields: &mut [AttrField],
        state: &mut AttrGridState,
        probe: &mut ActProbe,
        config: &AttrGridConfig,
    ) -> Vec<(String, AttrValue)> {
        let mut changed = Vec::new();
        let mut out = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                AttrMetrics::default().apply(ui);
                changed =
                    render_grid_with_config(ui, fields, state, &HashSet::new(), config, probe);
            },
        );
        out.textures_delta.clear();
        changed
    }

    /// A press-release at `pos`; every edit the two frames reported.
    fn click_at(
        ctx: &egui::Context,
        pos: egui::Pos2,
        fields: &mut [AttrField],
        state: &mut AttrGridState,
        probe: &mut ActProbe,
        config: &AttrGridConfig,
    ) -> Vec<(String, AttrValue)> {
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let down = vec![egui::Event::PointerMoved(pos), press(true)];
        let mut all = frame_cfg(ctx, down, fields, state, probe, config);
        all.extend(frame_cfg(
            ctx,
            vec![press(false)],
            fields,
            state,
            probe,
            config,
        ));
        all
    }

    #[test]
    fn value_action_buttons_reset_copy_paste_and_fan_out_randomise() {
        let ctx = egui::Context::default();
        let config = AttrGridConfig {
            actions: ValueActions {
                random_all: true,
                ..ValueActions::BASIC
            },
            ..grid_config(AttrMetrics::default())
        };
        let mut fields = [
            AttrField::new("/a", AttrValue::Float(0.9))
                .with_default(AttrValue::Float(0.5))
                .with_random(Random::uniform(0.0..=1.0))
                .with_order(0.0),
            AttrField::new("/b", AttrValue::Float(0.1)).with_order(1.0),
        ];
        let (mut state, mut probe) = (AttrGridState::default(), ActProbe::default());
        frame_cfg(
            &ctx,
            Vec::new(),
            &mut fields,
            &mut state,
            &mut probe,
            &config,
        );
        // Slot `i` of a row: after the editor and the host's `action_width` area.
        let slot = |probe: &ActProbe, key: &str, i: usize| {
            let r = probe.rects[key];
            let step = config.icon_side + config.icon_gap;
            egui::pos2(
                r.right() + config.action_width + step * i as f32 + config.icon_side / 2.0,
                r.center().y,
            )
        };

        let pos = slot(&probe, "/a", 0);
        let edits = click_at(&ctx, pos, &mut fields, &mut state, &mut probe, &config);
        assert_eq!(
            fields[0].value,
            AttrValue::Float(0.5),
            "0 resets to the default"
        );
        assert_eq!(edits, [("/a".to_owned(), AttrValue::Float(0.5))]);

        fields[0].value = AttrValue::Float(0.9);
        let pos = slot(&probe, "/a", 1);
        let edits = click_at(&ctx, pos, &mut fields, &mut state, &mut probe, &config);
        assert!(edits.is_empty(), "copy edits nothing");
        let pos = slot(&probe, "/b", 2);
        let edits = click_at(&ctx, pos, &mut fields, &mut state, &mut probe, &config);
        assert_eq!(
            fields[1].value,
            AttrValue::Float(0.9),
            "P pastes the copied float"
        );
        assert_eq!(edits, [("/b".to_owned(), AttrValue::Float(0.9))]);

        let pos = slot(&probe, "/a", 3);
        let edits = click_at(&ctx, pos, &mut fields, &mut state, &mut probe, &config);
        assert_eq!(edits.len(), 1, "Ra randomises this row");
        assert_eq!(probe.random_all, ["/a"], "and tells the host to fan out");
        let pos = slot(&probe, "/b", 3);
        click_at(&ctx, pos, &mut fields, &mut state, &mut probe, &config);
        assert_eq!(probe.random_all, ["/a"], "a row without a policy has no Ra");
    }

    #[test]
    fn no_actions_by_default_leaves_the_gutter_to_the_host() {
        let config = grid_config(AttrMetrics::default());
        assert_eq!(config.actions, ValueActions::default());
        assert_eq!(config.actions_width(), 0.0);
        let cell = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 18.0));
        assert_eq!(config.value_cells(cell).1.width(), config.action_width);
        let with = AttrGridConfig {
            actions: ValueActions::BASIC,
            ..config
        };
        let n = 3.0;
        assert_eq!(
            with.actions_width(),
            n * config.icon_side + (n - 1.0) * config.icon_gap
        );
        let (host, buttons) = with.action_cells(with.value_cells(cell).1);
        assert_eq!(host.width(), config.action_width);
        assert_eq!(buttons.width(), with.actions_width());
    }

    #[test]
    fn scalars_vectors_and_sliders_all_end_at_the_editor_cell_edge() {
        let metrics = AttrMetrics::default();
        let config = grid_config(metrics);
        let slider_hints = hints(&["0", "1"]);
        let harness = Harness::new_ui_state(
            move |ui, st: &mut (Vec<AttrField>, AttrGridState, ActProbe)| {
                metrics.apply(ui);
                let (fields, state, probe) = st;
                render_grid_with_config(ui, fields, state, &HashSet::new(), &config, probe);
            },
            (
                vec![
                    AttrField::new("/s", AttrValue::Float(0.5))
                        .with_ui_options(slider_hints)
                        .with_order(0.0),
                    AttrField::new("/v3", AttrValue::Vec3([1.0, 2.0, 3.0])).with_order(1.0),
                    AttrField::new("/f", AttrValue::Float(2.0)).with_order(2.0),
                    AttrField::new("/v4", AttrValue::Vec4([1.0; 4])).with_order(3.0),
                ],
                AttrGridState::default(),
                ActProbe::default(),
            ),
        );
        let boxes: Vec<egui::Rect> = harness
            .get_all_by_role(egui::accesskit::Role::SpinButton)
            .map(|n| n.rect())
            .collect();
        assert_eq!(boxes.len(), 1 + 3 + 1 + 4);
        let edge = harness.state().2.rects["/s"].right();
        for last in [0, 3, 4, 8] {
            assert!(
                (boxes[last].right() - edge).abs() < 0.01,
                "field {last}: {boxes:?} vs {edge}"
            );
        }
        assert!(
            boxes[1].width() > 0.0 && (boxes[1].width() - boxes[2].width()).abs() < 0.5,
            "{boxes:?}"
        );
    }

    #[test]
    fn uniform_scales_from_the_locked_base_so_zero_comes_back() {
        let base = [1.0, 2.0, 3.0];
        // Edit x 1 -> 2: everything doubles.
        let mut now = [2.0, 2.0, 3.0];
        propagate_uniform(&base, &base, &mut now);
        assert_eq!(now, [2.0, 4.0, 6.0]);
        // Drag x all the way to 0: the whole vector is 0 ...
        let mut now = [0.0, 2.0, 3.0];
        propagate_uniform(&base, &[2.0, 4.0, 6.0], &mut now);
        assert_eq!(now, [0.0, 0.0, 0.0]);
        // ... and back to 1: the proportions are restored (a frame-to-frame ratio would stay 0).
        let mut now = [1.0, 0.0, 0.0];
        propagate_uniform(&base, &[0.0, 0.0, 0.0], &mut now);
        assert_eq!(now, base);
        // A zero base component gives no ratio: the change is added instead.
        let mut now = [0.5, 0.0, 0.0];
        propagate_uniform(&[0.0; 3], &[0.0; 3], &mut now);
        assert_eq!(now, [0.5, 0.5, 0.5]);
        let mut same = [1.0, 2.0];
        propagate_uniform(&[1.0, 2.0], &[1.0, 2.0], &mut same);
        assert_eq!(same, [1.0, 2.0], "no edit, no change");
    }

    #[test]
    fn uniform_lock_row_scales_all_components_when_one_is_dragged() {
        let ctx = egui::Context::default();
        let config = grid_config(AttrMetrics::default());
        let mut fields = [AttrField::new("/s", AttrValue::Vec3([1.0, 2.0, 3.0])).with_uniform()];
        let (mut state, mut probe) = (AttrGridState::default(), ActProbe::default());
        frame_cfg(
            &ctx,
            Vec::new(),
            &mut fields,
            &mut state,
            &mut probe,
            &config,
        );
        let r = probe.rects["/s"];
        // Switch the lock on with a click on its toggle (the right end of the cell).
        let side = AttrMetrics::default().icon_side;
        let toggle = egui::pos2(r.right() - side / 2.0, r.center().y);
        click_at(&ctx, toggle, &mut fields, &mut state, &mut probe, &config);
        // Drag the first of the three components right, then left past its start.
        let start = egui::pos2(r.left() + 6.0, r.center().y);
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let down = vec![egui::Event::PointerMoved(start), press(start, true)];
        frame_cfg(&ctx, down, &mut fields, &mut state, &mut probe, &config);
        for dx in [4.0, 8.0, 12.0] {
            let to = vec![egui::Event::PointerMoved(start + egui::vec2(dx, 0.0))];
            frame_cfg(&ctx, to, &mut fields, &mut state, &mut probe, &config);
        }
        let AttrValue::Vec3(v) = fields[0].value else {
            panic!("still a vector")
        };
        assert!(v[0] > 1.0, "the dragged component moved: {v:?}");
        let ratio = v[0];
        assert!(
            (v[1] - 2.0 * ratio).abs() < 1e-3 && (v[2] - 3.0 * ratio).abs() < 1e-3,
            "{v:?}"
        );
        // Back toward the start: the proportions of the BASE still hold at every step.
        let to = vec![egui::Event::PointerMoved(start)];
        frame_cfg(&ctx, to, &mut fields, &mut state, &mut probe, &config);
        let AttrValue::Vec3(v) = fields[0].value else {
            panic!("still a vector")
        };
        assert!(
            (v[1] - 2.0 * v[0]).abs() < 1e-3 && (v[2] - 3.0 * v[0]).abs() < 1e-3,
            "{v:?}"
        );
        frame_cfg(
            &ctx,
            vec![press(start, false)],
            &mut fields,
            &mut state,
            &mut probe,
            &config,
        );
    }

    #[test]
    fn every_slider_is_named_by_its_own_row_label() {
        let slider = hints(&["0", "2"]);
        let harness = Harness::new_ui_state(
            |ui, st: &mut (Vec<AttrField>, AttrGridState)| {
                let (fields, state) = st;
                render_grid(ui, fields, state, &HashSet::new());
            },
            (
                vec![
                    AttrField::new("Alpha", AttrValue::Float(0.5))
                        .with_ui_options(slider.clone())
                        .with_order(0.0),
                    AttrField::new("Beta", AttrValue::Float(1.5))
                        .with_ui_options(slider.clone())
                        .with_order(1.0),
                    AttrField::new("Gamma", AttrValue::Float(1.0))
                        .with_ui_options(slider)
                        .with_order(2.0),
                ],
                AttrGridState::default(),
            ),
        );
        for (name, value) in [("Alpha", 0.5), ("Beta", 1.5), ("Gamma", 1.0)] {
            let node = harness.get_by_role_and_label(egui::accesskit::Role::Slider, name);
            assert_eq!(node.accesskit_node().numeric_value(), Some(value), "{name}");
        }
    }

    #[test]
    fn a_default_of_another_type_is_refused_and_a_reset_is_clamped_and_honest() {
        let mut f = AttrField::new("/a", AttrValue::Float(0.5)).with_ui_options(hints(&["0", "1"]));
        // The pub field can be written directly: the reset still refuses another type.
        f.default = Some(AttrValue::Int(3));
        assert!(
            !actions::reset(&mut f),
            "an Int default never turns a Float into an Int"
        );
        assert_eq!(f.value, AttrValue::Float(0.5));
        assert!(actions::resolved_default(&f.value, f.default.as_ref(), &f.ui_options).is_none());
        f.default = Some(AttrValue::Float(9.0));
        let d = actions::resolved_default(&f.value, f.default.as_ref(), &f.ui_options).unwrap();
        assert_eq!(
            *d,
            AttrValue::Float(1.0),
            "the default is held in the hard range, like a reset"
        );
        f.default = Some(AttrValue::Float(0.5));
        assert!(!actions::reset(&mut f), "already there: not an edit");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "does not match")]
    fn with_default_of_another_type_asserts_in_debug() {
        let _ = AttrField::new("/a", AttrValue::Float(0.5)).with_default(AttrValue::Bool(true));
    }

    #[test]
    fn ctrl_click_resets_one_component_or_scalar_through_the_default() {
        let ctx = egui::Context::default();
        let config = grid_config(AttrMetrics::default());
        let mut fields = [
            AttrField::new("/v", AttrValue::Vec3([5.0, 6.0, 7.0]))
                .with_default(AttrValue::Vec3([0.0, 1.0, 0.0]))
                .with_order(0.0),
            AttrField::new("/f", AttrValue::Float(9.0))
                .with_default(AttrValue::Float(2.0))
                .with_order(1.0),
        ];
        let (mut state, mut probe) = (AttrGridState::default(), ActProbe::default());
        frame_cfg(
            &ctx,
            Vec::new(),
            &mut fields,
            &mut state,
            &mut probe,
            &config,
        );
        let ctrl = egui::Modifiers::COMMAND;
        let ctrl_click = |ctx: &egui::Context,
                          pos: egui::Pos2,
                          fields: &mut [AttrField],
                          state: &mut AttrGridState,
                          probe: &mut ActProbe| {
            let press = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: ctrl,
            };
            let down = vec![
                egui::Event::ModifiersChanged(ctrl),
                egui::Event::PointerMoved(pos),
                press(true),
            ];
            let mut all = frame_cfg(ctx, down, fields, state, probe, &config);
            all.extend(frame_cfg(
                ctx,
                vec![press(false)],
                fields,
                state,
                probe,
                &config,
            ));
            all
        };
        // The middle of three equal components.
        let r = probe.rects["/v"];
        let mid = egui::pos2(r.center().x, r.center().y);
        let edits = ctrl_click(&ctx, mid, &mut fields, &mut state, &mut probe);
        assert_eq!(
            fields[0].value,
            AttrValue::Vec3([5.0, 1.0, 7.0]),
            "only that component resets"
        );
        assert_eq!(edits.len(), 1);
        let r = probe.rects["/f"];
        let edits = ctrl_click(&ctx, r.center(), &mut fields, &mut state, &mut probe);
        assert_eq!(fields[1].value, AttrValue::Float(2.0));
        assert_eq!(edits.len(), 1);
        // At the default already: the same click is no edit.
        let edits = ctrl_click(&ctx, r.center(), &mut fields, &mut state, &mut probe);
        assert!(edits.is_empty(), "{edits:?}");
    }

    #[test]
    fn colour_row_options_come_from_hints() {
        assert_eq!(
            color_opts(&hints(&["color"])),
            (false, egui_colorpicker::COLOR_DECIMALS_DEFAULT)
        );
        assert_eq!(
            color_opts(&hints(&["color", "swatch_left", "Decimals=1"])),
            (true, 1)
        );
    }

    #[test]
    fn narrow_gutters_keep_every_button_inside_and_leave_the_host_what_is_left() {
        let config = AttrGridConfig {
            actions: ValueActions::BASIC,
            ..grid_config(AttrMetrics::default())
        };
        for width in [30.0, 80.0, 120.0, 200.0, 600.0] {
            let cell = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(width, 18.0));
            let (editor, gutter) = config.value_cells(cell);
            assert!((editor.width() + gutter.width() - width).abs() < 1e-3);
            let (host, buttons) = config.action_cells(gutter);
            assert!(
                host.width() + buttons.width() <= gutter.width() + 1e-3,
                "{width}"
            );
            assert!(host.right() <= buttons.left() + 1e-3, "{width}");
            if width >= 80.0 {
                assert!(
                    buttons.width() >= config.actions_width() - 1e-3,
                    "all three fit at {width}"
                );
            }
        }
    }

    #[test]
    fn buttons_without_an_icon_side_use_the_style_icon_size() {
        let ctx = egui::Context::default();
        let config = AttrGridConfig {
            actions: ValueActions::BASIC,
            icon_side: 0.0,
            ..grid_config(AttrMetrics::default())
        };
        let mut fields =
            [AttrField::new("/a", AttrValue::Float(9.0)).with_default(AttrValue::Float(2.0))];
        let (mut state, mut probe) = (AttrGridState::default(), ActProbe::default());
        frame_cfg(
            &ctx,
            Vec::new(),
            &mut fields,
            &mut state,
            &mut probe,
            &config,
        );
        let r = probe.rects["/a"];
        // `AttrMetrics::apply` made the style icon width 12: the first slot sits after the host area.
        let pos = egui::pos2(r.right() + config.action_width + 6.0, r.center().y);
        click_at(&ctx, pos, &mut fields, &mut state, &mut probe, &config);
        assert_eq!(
            fields[0].value,
            AttrValue::Float(2.0),
            "the reset button exists and works"
        );
    }

    #[test]
    fn a_value_outside_the_choices_still_shows_and_a_strip_that_cannot_fit_is_a_dropdown_inside_the_cell()
     {
        let long = "an extremely long choice label that cannot fit";
        let harness = Harness::new_ui_state(
            move |ui, st: &mut (Vec<AttrField>, AttrGridState, ActProbe)| {
                let (fields, state, probe) = st;
                ui.set_max_width(240.0);
                render_grid_with_config(
                    ui,
                    fields,
                    state,
                    &HashSet::new(),
                    &AttrGridConfig::default(),
                    probe,
                );
            },
            (
                vec![
                    AttrField::new("/m", AttrValue::Str("legacy".into()))
                        .with_ui_options(hints(&["a", long, "segmented"])),
                ],
                AttrGridState::default(),
                ActProbe::default(),
            ),
        );
        // The long choice cannot fit its segment: no truncated strip, a dropdown showing the
        // out-of-list value in full.
        assert_eq!(
            harness
                .query_all_by_role(egui::accesskit::Role::RadioButton)
                .count(),
            0
        );
        let node = harness.get_by_role(egui::accesskit::Role::ComboBox);
        assert_eq!(node.value().as_deref(), Some("legacy"));
        let cell = harness.state().2.rects["/m"];
        assert!(
            node.rect().right() <= cell.right() + 0.5,
            "{:?} vs {cell:?}",
            node.rect()
        );
    }

    #[test]
    fn randomise_that_changes_nothing_is_not_an_edit() {
        let mut f =
            AttrField::new("/a", AttrValue::Float(0.5)).with_random(Random::uniform(0.5..=0.5));
        assert!(!actions::randomize(&mut f, &mut random::Pcg::new(1)));
    }

    #[test]
    fn section_randomise_edits_only_fields_with_a_policy() {
        let mut fields = [
            AttrField::new("/a", AttrValue::Float(0.5)).with_random(Random::uniform(10.0..=20.0)),
            AttrField::new("/b", AttrValue::Float(0.5)),
        ];
        let edits = randomize_fields(&mut fields, 42);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].0, "/a");
        assert_eq!(fields[1].value, AttrValue::Float(0.5));
    }

    #[test]
    fn text_and_choice_fields_fill_the_cell_like_the_numbers() {
        let metrics = AttrMetrics::default();
        let config = grid_config(metrics);
        let harness = Harness::new_ui_state(
            move |ui, st: &mut (Vec<AttrField>, AttrGridState, ActProbe)| {
                metrics.apply(ui);
                let (fields, state, probe) = st;
                render_grid_with_config(ui, fields, state, &HashSet::new(), &config, probe);
            },
            (
                vec![AttrField::new("/name", AttrValue::Str("nebula".into()))],
                AttrGridState::default(),
                ActProbe::default(),
            ),
        );
        let edit = harness.get_by_role(egui::accesskit::Role::TextInput).rect();
        let edge = harness.state().2.rects["/name"].right();
        assert!((edit.right() - edge).abs() < 1.0, "{edit:?} vs {edge}");
    }

    // --- one switch for buttons and menu, host-owned editors, host veto, matrices ---

    /// Drawn editors by key: "/own" is the host's (returns Some), "/veto" refuses every value
    /// action; everything else is the grid's.
    #[derive(Default)]
    struct HostProbe {
        rects: std::collections::HashMap<String, egui::Rect>,
    }
    impl AttrGridHooks for HostProbe {
        fn editor(
            &mut self,
            ui: &mut Ui,
            field: &mut AttrField,
            _: bool,
            _: &ValueEditorLayout,
            _: &EditorCtx,
        ) -> Option<bool> {
            self.rects.insert(field.key.clone(), ui.max_rect());
            (field.key == "/own").then(|| {
                ui.label("host editor");
                false
            })
        }
        fn accepts(&self, field: &AttrField, _: &AttrValue) -> bool {
            !field.key.starts_with("/veto")
        }
    }

    fn host_frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        fields: &mut [AttrField],
        state: &mut AttrGridState,
        probe: &mut HostProbe,
        config: &AttrGridConfig,
    ) -> Vec<(String, AttrValue)> {
        let mut changed = Vec::new();
        let mut out = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                AttrMetrics::default().apply(ui);
                changed =
                    render_grid_with_config(ui, fields, state, &HashSet::new(), config, probe);
            },
        );
        out.textures_delta.clear();
        changed
    }

    /// A vetoed default is not written by a Ctrl / Cmd-click on a slider, a DragValue scalar or a
    /// vector component, while the same gesture on an accepted row resets it.
    #[test]
    fn ctrl_click_reset_honours_the_host_veto() {
        let ctx = egui::Context::default();
        let config = grid_config(AttrMetrics::default());
        let mut fields = [
            AttrField::new("/veto_slider", AttrValue::Float(0.9))
                .with_ui_options(hints(&["0", "1"]))
                .with_default(AttrValue::Float(0.25))
                .with_order(0.0),
            AttrField::new("/veto_int", AttrValue::Int(9))
                .with_default(AttrValue::Int(2))
                .with_order(1.0),
            AttrField::new("/veto_vec", AttrValue::Vec3([5.0, 6.0, 7.0]))
                .with_default(AttrValue::Vec3([0.0, 1.0, 0.0]))
                .with_order(2.0),
            AttrField::new("/ok_int", AttrValue::Int(9))
                .with_default(AttrValue::Int(2))
                .with_order(3.0),
        ];
        let (mut state, mut probe) = (AttrGridState::default(), HostProbe::default());
        host_frame(
            &ctx,
            Vec::new(),
            &mut fields,
            &mut state,
            &mut probe,
            &config,
        );
        let ctrl = egui::Modifiers::COMMAND;
        let mut ctrl_click = |key: &str, fields: &mut [AttrField]| {
            let r = probe.rects[key];
            let pos = egui::pos2(r.left() + r.width() * 0.4, r.center().y);
            let press = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: ctrl,
            };
            let down = vec![
                egui::Event::ModifiersChanged(ctrl),
                egui::Event::PointerMoved(pos),
                press(true),
            ];
            let mut all = host_frame(&ctx, down, fields, &mut state, &mut probe, &config);
            all.extend(host_frame(
                &ctx,
                vec![press(false)],
                fields,
                &mut state,
                &mut probe,
                &config,
            ));
            all
        };
        for key in ["/veto_slider", "/veto_int", "/veto_vec"] {
            let before = fields
                .iter()
                .find(|f| f.key == key)
                .map(|f| f.value.clone());
            let edits = ctrl_click(key, &mut fields);
            let after = fields
                .iter()
                .find(|f| f.key == key)
                .map(|f| f.value.clone());
            assert_eq!(before, after, "{key}: a vetoed default must not be written");
            assert!(edits.is_empty(), "{key}: no edit reported, got {edits:?}");
        }
        let edits = ctrl_click("/ok_int", &mut fields);
        assert_eq!(
            fields[3].value,
            AttrValue::Int(2),
            "an accepted default resets"
        );
        assert_eq!(edits, [("/ok_int".to_owned(), AttrValue::Int(2))]);
    }

    #[test]
    fn value_actions_skip_host_editor_rows_and_honour_the_host_veto() {
        let ctx = egui::Context::default();
        let config = AttrGridConfig {
            actions: ValueActions::BASIC,
            ..grid_config(AttrMetrics::default())
        };
        let mut fields = [
            AttrField::new("/a", AttrValue::Float(0.9)).with_order(0.0),
            AttrField::new("/own", AttrValue::Float(0.1))
                .with_default(AttrValue::Float(0.5))
                .with_order(1.0),
            AttrField::new("/veto", AttrValue::Float(0.2))
                .with_default(AttrValue::Float(0.7))
                .with_order(2.0),
        ];
        let (mut state, mut probe) = (AttrGridState::default(), HostProbe::default());
        host_frame(
            &ctx,
            Vec::new(),
            &mut fields,
            &mut state,
            &mut probe,
            &config,
        );
        let slot = |probe: &HostProbe, key: &str, i: usize| {
            let r = probe.rects[key];
            let step = config.icon_side + config.icon_gap;
            egui::pos2(
                r.right() + config.action_width + step * i as f32 + config.icon_side / 2.0,
                r.center().y,
            )
        };
        let click = |ctx: &egui::Context,
                     pos: egui::Pos2,
                     fields: &mut [AttrField],
                     state: &mut AttrGridState,
                     probe: &mut HostProbe| {
            let press = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            let mut all = host_frame(
                ctx,
                vec![egui::Event::PointerMoved(pos), press(true)],
                fields,
                state,
                probe,
                &config,
            );
            all.extend(host_frame(
                ctx,
                vec![press(false)],
                fields,
                state,
                probe,
                &config,
            ));
            all
        };
        // Copy 0.9 from /a, then try Paste and Reset on the host-editor row: nothing is drawn.
        let pos = slot(&probe, "/a", 1);
        click(&ctx, pos, &mut fields, &mut state, &mut probe);
        for i in [0, 2] {
            let pos = slot(&probe, "/own", i);
            let edits = click(&ctx, pos, &mut fields, &mut state, &mut probe);
            assert!(
                edits.is_empty(),
                "slot {i} of a host-editor row must do nothing: {edits:?}"
            );
        }
        assert_eq!(fields[1].value, AttrValue::Float(0.1));
        // The veto row has the buttons, but the host refuses what they would write.
        for i in [0, 2] {
            let pos = slot(&probe, "/veto", i);
            let edits = click(&ctx, pos, &mut fields, &mut state, &mut probe);
            assert!(edits.is_empty(), "slot {i} vetoed: {edits:?}");
        }
        assert_eq!(fields[2].value, AttrValue::Float(0.2));
        // The ordinary row still pastes.
        let pos = slot(&probe, "/a", 2);
        click(&ctx, pos, &mut fields, &mut state, &mut probe);
        assert_eq!(fields[0].value, AttrValue::Float(0.9));
    }

    #[test]
    fn the_row_menu_follows_the_one_switch_and_is_absent_by_default() {
        let menu_has = |config: AttrGridConfig, entry: &str| {
            let harness = Harness::new_ui_state(
                move |ui, st: &mut (Vec<AttrField>, AttrGridState)| {
                    let (fields, state) = st;
                    AttrMetrics::default().apply(ui);
                    render_grid_with_config(
                        ui,
                        fields,
                        state,
                        &HashSet::new(),
                        &config,
                        &mut HostProbe::default(),
                    );
                },
                (
                    vec![
                        AttrField::new("Alpha", AttrValue::Float(0.5))
                            .with_default(AttrValue::Float(0.1)),
                    ],
                    AttrGridState::default(),
                ),
            );
            let mut harness = harness;
            harness.get_by_label("Alpha").click_secondary();
            harness.run();
            harness.query_by_label(entry).is_some()
        };
        let off = grid_config(AttrMetrics::default());
        assert!(
            !menu_has(off, "Copy value"),
            "no menu entries unless the host asks"
        );
        assert!(!menu_has(off, "Reset to default"));
        let on = AttrGridConfig {
            actions: ValueActions::BASIC,
            ..off
        };
        assert!(menu_has(on, "Copy value"));
        assert!(menu_has(on, "Reset to default"));
        assert!(!menu_has(on, "Randomise"), "only the enabled actions");
    }

    #[test]
    fn a_row_can_narrow_the_grid_actions_but_not_widen_them() {
        let config = AttrGridConfig {
            actions: ValueActions {
                copy: true,
                ..ValueActions::default()
            },
            ..grid_config(AttrMetrics::default())
        };
        let none = AttrField::new("/a", AttrValue::Float(0.5)).with_actions(ValueActions::BASIC);
        assert_eq!(
            none.value_actions(&config),
            ValueActions {
                copy: true,
                ..Default::default()
            }
        );
        let narrowed = AttrField::new("/a", AttrValue::Float(0.5)).with_actions(ValueActions {
            paste: true,
            ..Default::default()
        });
        assert_eq!(narrowed.value_actions(&config), ValueActions::default());
        assert_eq!(
            AttrField::new("/a", AttrValue::Float(0.5)).value_actions(&config),
            config.actions
        );
    }

    #[test]
    fn randomise_without_a_policy_draws_from_the_slider_range() {
        let mut f =
            AttrField::new("/a", AttrValue::Float(50.0)).with_ui_options(hints(&["2", "3"]));
        assert!(actions::applicable(&f, ValueAction::RandomOne, false));
        assert!(actions::randomize(&mut f, &mut random::Pcg::new(5)));
        let AttrValue::Float(v) = f.value else {
            panic!("still a float")
        };
        assert!((2.0..=3.0).contains(&v), "{v}");
        let plain = AttrField::new("/b", AttrValue::Float(1.0));
        assert!(
            !actions::applicable(&plain, ValueAction::RandomOne, false),
            "no range, no draw"
        );
        assert!(
            !actions::applicable(&f, ValueAction::RandomOne, true),
            "never on a host editor row"
        );
    }

    #[test]
    fn a_matrix_is_a_collapsed_summary_that_expands_to_edited_rows() {
        let metrics = AttrMetrics::default();
        let config = grid_config(metrics);
        let ctx = egui::Context::default();
        let mut fields = [AttrField::new(
            "/m",
            AttrValue::Mat3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
        )];
        let (mut state, mut probe) = (AttrGridState::default(), ActProbe::default());
        let spin_rows = |ctx: &egui::Context,
                         fields: &mut [AttrField],
                         state: &mut AttrGridState,
                         probe: &mut ActProbe| {
            let mut keys = Vec::new();
            let mut out = ctx.run_ui(Default::default(), |ui| {
                metrics.apply(ui);
                render_grid_with_config(ui, fields, state, &HashSet::new(), &config, probe);
            });
            out.textures_delta.clear();
            keys.extend(probe.rects.keys().cloned());
            keys.sort();
            keys
        };
        // Collapsed: one row (the summary), no editors of the grid's own.
        assert_eq!(spin_rows(&ctx, &mut fields, &mut state, &mut probe), ["/m"]);
        // Expanded through the same state the vector channels use: three rows of three fields.
        state.channels.set("/m", true);
        let keys = spin_rows(&ctx, &mut fields, &mut state, &mut probe);
        assert_eq!(keys, ["/m", "/m[0]", "/m[1]", "/m[2]"]);
        // Edit the middle row's first field: the grid reports the whole matrix under "/m".
        let r = probe.rects["/m[1]"];
        let pos = egui::pos2(r.left() + 8.0, r.center().y);
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let mut edits = Vec::new();
        for events in [
            vec![egui::Event::PointerMoved(pos), press(true)],
            vec![egui::Event::PointerMoved(pos + egui::vec2(20.0, 0.0))],
            vec![egui::Event::PointerMoved(pos + egui::vec2(40.0, 0.0))],
            vec![press(false)],
        ] {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| {
                    metrics.apply(ui);
                    edits.extend(render_grid_with_config(
                        ui,
                        &mut fields,
                        &mut state,
                        &HashSet::new(),
                        &config,
                        &mut probe,
                    ));
                },
            );
            out.textures_delta.clear();
        }
        let AttrValue::Mat3(m) = fields[0].value else {
            panic!("still a matrix")
        };
        assert!(
            m[1][0] != 0.0 && m[0] == [1.0, 0.0, 0.0] && m[2] == [0.0, 0.0, 1.0],
            "{m:?}"
        );
        assert!(
            !edits.is_empty()
                && edits
                    .iter()
                    .all(|(k, v)| k == "/m" && matches!(v, AttrValue::Mat3(_)))
        );
    }

    /// Renders the grid as the demo page shows it, at a normal and a narrow width, to PNGs in
    /// `$WIDGET_SHOTS`: `WIDGET_SHOTS=dir cargo test -p egui-attr-grid -- --ignored shots`. Needs a GPU.
    #[test]
    #[ignore = "writes screenshots; needs a GPU adapter and WIDGET_SHOTS"]
    fn shots() {
        let Some(dir) = std::env::var_os("WIDGET_SHOTS") else {
            return;
        };
        let metrics = AttrMetrics::default();
        let config = AttrGridConfig {
            prefix_width: 0.0,
            action_width: metrics.gutter(),
            actions: ValueActions {
                random_one: true,
                ..ValueActions::BASIC
            },
            ..grid_config(metrics)
        };
        let model = || {
            let h = hints;
            vec![
                AttrField::new("enabled", AttrValue::Bool(true))
                    .with_default(AttrValue::Bool(true)),
                AttrField::new("name", AttrValue::Str("nebula".into())),
                AttrField::new("mode", AttrValue::Str("selected".into()))
                    .with_ui_options(h(&["off", "selected", "all", "segmented"]))
                    .with_default(AttrValue::Str("off".into())),
                AttrField::new("samples", AttrValue::UInt(64))
                    .with_ui_options(h(&["8", "256"]))
                    .with_default(AttrValue::UInt(32)),
                AttrField::new("exposure", AttrValue::Float(1.5))
                    .with_ui_options(h(&["-6", "6", "0.01", "soft", "suffix= EV"]))
                    .with_default(AttrValue::Float(0.0)),
                AttrField::new("fine", AttrValue::Float(0.02))
                    .with_ui_options(h(&["0", "1", "0.0001"]))
                    .with_default(AttrValue::Float(0.0)),
                AttrField::new("resolution", AttrValue::Int(16384))
                    .with_ui_options(h(&["0", "16384"]))
                    .with_default(AttrValue::Int(1024)),
                AttrField::new("translate", AttrValue::Vec3([0.0, 1.0, -2.5])).with_uniform(),
                AttrField::new("colour", AttrValue::Vec3([0.2, 0.08, 0.35]))
                    .with_ui_options(h(&["color"])),
                AttrField::new(
                    "matrix",
                    AttrValue::Mat3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]),
                ),
            ]
        };
        for (name, width) in [
            ("w1-attr-grid.png", 560.0),
            ("w1-attr-grid-narrow.png", 300.0),
        ] {
            let mut harness = Harness::builder()
                .with_size(egui::vec2(width, 360.0))
                .with_pixels_per_point(2.0)
                .build_ui_state(
                    move |ui, st: &mut (Vec<AttrField>, AttrGridState)| {
                        metrics.apply(ui);
                        let (fields, state) = st;
                        render_grid_with_config(
                            ui,
                            fields,
                            state,
                            &HashSet::new(),
                            &config,
                            &mut ActProbe::default(),
                        );
                    },
                    (model(), AttrGridState::default()),
                );
            harness.run_steps(10);
            let image = harness.render().expect("render");
            image
                .save(std::path::Path::new(&dir).join(name))
                .expect("save");
        }
    }
}
