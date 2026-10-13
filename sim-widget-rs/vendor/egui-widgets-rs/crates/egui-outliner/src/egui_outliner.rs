//! A generic, app-agnostic collapsible hierarchy / outliner panel for egui 0.34.
//!
//! Renders a caller-owned [`OutlinerModel`] (a forest of [`TreeNode`]s plus a
//! selection list) as a vertically-scrolling tree of recursive
//! [`egui::CollapsingHeader`] rows, with:
//! * selection (plain / ctrl-toggle / shift-range) reported with modifier intent,
//! * an **ancestor-of-selected** dimmed highlight (a node whose subtree contains
//!   a selected node is tinted the same hue as the selection at reduced alpha),
//! * a caller-defined right-click context menu (the widget is generic — it does
//!   NOT know "Activate" / "Load payload"; the host supplies its entries),
//! * double-click → [`OutlinerAction::Activate`],
//! * a depth guard (`> max_depth` → a "… (too deep)" row) so pathological scene
//!   graphs cannot overflow the stack,
//! * optional [`OutlinerAction::ToggleOpen`] events when a node expands/collapses,
//! * an opt-in per-row **visibility eye** toggle (enabled per node via
//!   [`TreeNode::visible`]) that emits [`OutlinerAction::ToggleVisible`] WITHOUT
//!   also selecting the row; laid inline-left by default or, with
//!   [`OutlinerConfig::visibility_on_right`], as a right-aligned Blender-style
//!   column,
//! * **configurable glyphs** ([`OutlinerConfig::glyphs`]) for the disclosure
//!   triangle and the visibility eye — the widget is font-agnostic and just
//!   paints the host's strings; the default is the set's phosphor glyphs
//!   (`egui_widgets_config::icons`), a font-free host overrides them,
//! * opt-in **in-place rename** ([`OutlinerConfig::editable_labels`]): F2 (or a
//!   double-click) on the selected row edits the label and emits
//!   [`OutlinerAction::Rename`] on commit,
//! * an opt-in **filter box** ([`OutlinerConfig::show_filter`]) that keeps only
//!   nodes whose label matches (case-insensitive) plus their ancestors,
//! * **keyboard navigation** (Up/Down move the selection between visible rows,
//!   Left/Right collapse/expand the selected node) while the pointer is over the
//!   panel and no text field is focused,
//! * **empty-matte click** → [`OutlinerAction::ClearSelection`], and an AE-style
//!   **rubber-band** started on leftover space → [`OutlinerAction::SelectMany`]
//!   (the full selection, live while dragging; Ctrl/Shift unions with the set
//!   from drag-start). Dragging a row is still reorder / `DragStarted`,
//! * an opt-in **Maya-style drag** ([`OutlinerConfig::draggable`]): a row can be
//!   dragged onto another row's upper/lower third to **reorder** it among that
//!   row's siblings (an insertion line is shown), or onto the middle third to
//!   **reparent** it as a child of that row (the whole row is highlighted). The
//!   gesture is reported as [`OutlinerAction::Move`]; drops that would place a
//!   node inside its own subtree are refused. The widget never mutates the model
//!   — the caller applies the move to its own forest,
//! * and an optional colour legend explaining the highlight scheme.
//!
//! The widget holds NO tree state of its own: all hierarchy + selection lives in
//! the caller's model, which is read-only here. The widget only reads it and
//! returns [`OutlinerAction`]s describing what the user did, so the caller stays
//! the single owner of truth. Colours, fonts and row metrics come from the
//! shared [`egui_widgets_config`] tokens so the outliner follows the host theme.
//!
//! ## Usage
//! ```no_run
//! # use egui_outliner::{OutlinerModel, OutlinerConfig, TreeNode, ContextItem, OutlinerAction, show};
//! # fn ui_demo(ui: &mut egui::Ui, model: &mut OutlinerModel) {
//! let config = OutlinerConfig::default()
//!     .with_context_items([
//!         ContextItem::new("select", "Select"),
//!         ContextItem::new("frame", "Frame in view"),
//!         ContextItem::new("activate", "Activate").with_separator(),
//!     ]);
//! for action in show(ui, model, &config) {
//!     match action {
//!         OutlinerAction::Select { id, additive, range } => { /* update model.selection */ }
//!         OutlinerAction::Activate { id } => { /* open / frame */ }
//!         OutlinerAction::Context { id, action } => { /* run host command `action` on `id` */ }
//!         OutlinerAction::ContextEmpty { action } => { /* run `action` on the empty area, e.g. add here */ }
//!         OutlinerAction::ToggleOpen { .. } => {}
//!         OutlinerAction::ToggleVisible { id, visible } => { /* set layer `id` visible */ }
//!         OutlinerAction::Rename { id, name } => { /* rename node `id` to `name` */ }
//!         OutlinerAction::Move { id, new_parent, index } => { /* re-link `id` under `new_parent` at `index` */ }
//!         OutlinerAction::Sort { column, ascending } => { /* re-order the model; see below */ }
//!         OutlinerAction::CellClicked { id, column } => { /* e.g. open a colour palette */ }
//!         OutlinerAction::DragStarted { id } => { /* arm a cross-panel drag, if any */ }
//!         OutlinerAction::ClearSelection => { /* model.selection.clear() */ }
//!         OutlinerAction::SelectMany { ids } => { /* marquee: this IS the selection */ }
//!     }
//! }
//! # }
//! ```
//!
//! ## Optional DATA COLUMNS
//!
//! A label-only tree answers "what is here"; a browser also answers "how big, what
//! kind, how fast" — After Effects' Project panel is `Name | label | Type | Size |
//! Frame rate`. Declare [`OutlinerColumn`]s and give each node its [`CellValue`]s:
//!
//! ```no_run
//! # use egui_outliner::{CellValue, ColumnAlign, OutlinerColumn, OutlinerConfig, TreeNode};
//! # use egui::Color32;
//! let config = OutlinerConfig::default()
//!     .with_columns([
//!         OutlinerColumn::new("label", "", 18.0).with_align(ColumnAlign::Center),
//!         OutlinerColumn::new("type", "Type", 90.0).sortable(),
//!         OutlinerColumn::new("size", "Size", 70.0).with_align(ColumnAlign::Right).sortable(),
//!     ])
//!     .with_header(true, true);
//!
//! let row = TreeNode::new(1, "IMG_6759.MOV").with_cells([
//!     CellValue::Color(Color32::RED),
//!     CellValue::from("QuickTime"),
//!     CellValue::from("53.1 MB"),
//! ]);
//! ```
//!
//! Columns are OFF by default, so a caller that declares none gets exactly the
//! label-only tree this widget has always drawn. Sorting is reported, never
//! performed — like [`OutlinerAction::Move`], the caller owns the forest.
//!
//! Gathered from vfx-view's `panels/prim_tree.rs`; decoupled from
//! `usd_core::Prim`, `usd_sdf::Path` and the `DataModel` host glue.

use std::collections::{HashMap, HashSet};

use egui::{
    Color32, Ui,
    text::{LayoutJob, TextFormat},
};
use egui_widgets_config::{WidgetsConfig, prop, semantic};

/// Opaque, caller-defined stable node identifier.
///
/// This crate uses a plain `u64` to stay dependency-free; callers keyed by a
/// `Uuid` or a path string hash/truncate their own key into this (the same
/// convention as `egui-asset-browser` / `egui-track-timeline`). Every action
/// returns the id verbatim, so the caller can map it back to its own object.
pub type NodeId = u64;

// ============================================================================
// Model
// ============================================================================

/// One node in the hierarchy.
///
/// A node carries its own `children`, so [`OutlinerModel`] is just a forest of
/// these. `type_label` is rendered as a dimmed `[Xform]`-style suffix; `icon` is
/// a short text / phosphor glyph drawn before the label; `color` overrides the
/// default (non-selected) row text colour.
/// Horizontal alignment of a data column's content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColumnAlign {
    /// Left-aligned — the default, right for names and type labels.
    #[default]
    Left,
    /// Centred — right for swatches and short flags.
    Center,
    /// Right-aligned — right for numbers, so digits line up by place value.
    Right,
}

/// One trailing DATA column, declared once in [`OutlinerConfig::columns`].
///
/// Columns are the difference between a tree and a browser: a tree shows names, a
/// browser shows names PLUS the facts you sort and compare by (After Effects'
/// Project panel is `Name | label | Type | Size | Frame rate`). They are opt-in —
/// an empty [`OutlinerConfig::columns`] renders exactly the label-only tree this
/// widget has always drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlinerColumn {
    /// Machine key, returned verbatim in [`OutlinerAction::Sort`] and
    /// [`OutlinerAction::CellClicked`]. Never displayed.
    pub id: String,
    /// Header text.
    pub title: String,
    /// Initial width in points. The user's dragged width (when
    /// [`OutlinerConfig::resizable_columns`]) overrides it and is remembered per
    /// panel instance.
    pub width: f32,
    /// Content alignment within the column.
    pub align: ColumnAlign,
    /// Whether clicking the header emits [`OutlinerAction::Sort`]. The widget does
    /// NOT reorder anything — see that action's docs.
    pub sortable: bool,
    /// Pin the column to [`Self::width`]: no resize handle, and any width the user
    /// dragged before is ignored.
    ///
    /// For a column whose content has ONE size — a swatch, a checkbox, an icon —
    /// resizing has nothing to reveal, and the handle actively misleads: it sits on
    /// the column's LEFT edge, so it reads as "the right edge of the name column"
    /// while it actually widens the column to its right. Dragging it left inflated a
    /// 18 pt swatch column to as much as 800 pt of empty space.
    pub fixed: bool,
}

impl OutlinerColumn {
    /// New left-aligned, non-sortable column.
    pub fn new(id: impl Into<String>, title: impl Into<String>, width: f32) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            width,
            align: ColumnAlign::Left,
            sortable: false,
            fixed: false,
        }
    }

    /// Builder: pin the column to its declared width. See [`Self::fixed`].
    pub fn fixed(mut self) -> Self {
        self.fixed = true;
        self
    }

    /// Builder: set the content alignment.
    pub fn with_align(mut self, align: ColumnAlign) -> Self {
        self.align = align;
        self
    }

    /// Builder: make the header clickable, emitting [`OutlinerAction::Sort`].
    pub fn sortable(mut self) -> Self {
        self.sortable = true;
        self
    }
}

/// One node's value in one data column.
///
/// [`CellValue::Color`] exists because a colour is a first-class cell, not a string:
/// AE's label swatch is clickable and drives a palette. Encoding it as text would
/// make the host parse its own output back.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CellValue {
    /// Nothing in this cell (renders blank).
    #[default]
    Empty,
    /// Plain text.
    Text(String),
    /// A filled swatch. Clicking it emits [`OutlinerAction::CellClicked`] and does
    /// NOT select the row, mirroring how the visibility eye consumes its own click.
    Color(Color32),
}

impl From<&str> for CellValue {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

impl From<String> for CellValue {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<Color32> for CellValue {
    fn from(c: Color32) -> Self {
        Self::Color(c)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TreeNode {
    /// Stable, caller-defined id. Returned verbatim in every action.
    pub id: NodeId,
    /// Primary display label (e.g. the prim name).
    pub label: String,
    /// Optional type label, rendered dimmed in brackets after the label
    /// (e.g. `Some("Xform")` → `label  [Xform]`).
    pub type_label: Option<String>,
    /// Child nodes, in display order.
    pub children: Vec<TreeNode>,
    /// Initial expanded state: `Some(true/false)` forces it; `None` auto-decides
    /// by child count (open when `children.len() < OutlinerConfig::auto_open_below`).
    pub default_open: Option<bool>,
    /// Optional short text / phosphor glyph drawn before the label.
    pub icon: Option<String>,
    /// Optional explicit row text colour, used for the default (non-selected,
    /// non-ancestor) state. Selection / ancestor highlighting still take
    /// precedence so the selection stays legible.
    pub color: Option<Color32>,
    /// Optional per-row visibility state. `Some(true/false)` draws a leading
    /// **eye toggle** (filled = visible, hollow = hidden) before the icon/label;
    /// clicking it emits [`OutlinerAction::ToggleVisible`] with the flipped value
    /// and does NOT also select the row. `None` (the default) draws no toggle, so
    /// existing callers are unaffected. Use it for layer-style rows whose
    /// visibility the host gates (e.g. show/hide a scene layer).
    pub visible: Option<bool>,
    /// Values for the trailing data columns, POSITIONAL against
    /// [`OutlinerConfig::columns`]: `cells[i]` fills `columns[i]`.
    ///
    /// Positional rather than keyed because a tree can hold thousands of rows and a
    /// per-row map of column names would allocate for every one of them. Short or
    /// empty vectors are fine — missing cells render blank — so a node that has
    /// nothing to say in a column simply says nothing, and adding a column does not
    /// force every existing node to be rebuilt.
    pub cells: Vec<CellValue>,
}

impl TreeNode {
    /// New leaf node with just an id + label.
    pub fn new(id: NodeId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            type_label: None,
            children: Vec::new(),
            default_open: None,
            icon: None,
            color: None,
            visible: None,
            cells: Vec::new(),
        }
    }

    /// Builder: set the dimmed `[type]` suffix.
    pub fn with_type(mut self, type_label: impl Into<String>) -> Self {
        self.type_label = Some(type_label.into());
        self
    }

    /// Builder: replace the children.
    pub fn with_children(mut self, children: impl IntoIterator<Item = TreeNode>) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    /// Builder: append a single child.
    pub fn child(mut self, child: TreeNode) -> Self {
        self.children.push(child);
        self
    }

    /// Builder: force the initial expanded state (overrides the auto heuristic).
    pub fn with_open(mut self, open: bool) -> Self {
        self.default_open = Some(open);
        self
    }

    /// Builder: set the leading icon / glyph.
    pub fn with_icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Builder: set the explicit default-state row colour.
    pub fn with_color(mut self, color: Color32) -> Self {
        self.color = Some(color);
        self
    }

    /// Builder: enable the per-row visibility eye toggle with an initial state
    /// (`true` = visible). See [`TreeNode::visible`].
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = Some(visible);
        self
    }

    /// Builder: set the data-column values, positional against
    /// [`OutlinerConfig::columns`]. See [`TreeNode::cells`].
    pub fn with_cells(mut self, cells: impl IntoIterator<Item = CellValue>) -> Self {
        self.cells = cells.into_iter().collect();
        self
    }
}

/// The complete, caller-owned outliner state.
///
/// `roots` is the top-level forest; `selection` lists the ids of currently
/// selected nodes (multi-select supported). Ids in `selection` that no longer
/// exist in the tree are simply ignored while rendering.
#[derive(Clone, Debug, Default)]
pub struct OutlinerModel {
    /// Top-level nodes, in display order.
    pub roots: Vec<TreeNode>,
    /// Ids of selected nodes (order is the caller's; not the display order).
    pub selection: Vec<NodeId>,
}

impl OutlinerModel {
    /// Empty model.
    pub fn new() -> Self {
        Self::default()
    }

    /// Model from a forest of roots, with an empty selection.
    pub fn with_roots(roots: impl IntoIterator<Item = TreeNode>) -> Self {
        Self {
            roots: roots.into_iter().collect(),
            selection: Vec::new(),
        }
    }

    /// True if `id` is in the selection.
    pub fn is_selected(&self, id: NodeId) -> bool {
        self.selection.contains(&id)
    }
}

// ============================================================================
// Config + context menu
// ============================================================================

/// One entry in the caller-defined right-click context menu.
///
/// The widget is generic: it knows nothing about what the entries mean. When an
/// entry is chosen on a node, the widget emits [`OutlinerAction::Context`] with
/// this entry's `id` as the machine action key and the node's id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextItem {
    /// Machine action key, echoed back in [`OutlinerAction::Context::action`].
    pub id: String,
    /// Human-readable menu label.
    pub label: String,
    /// Draw a separator line *before* this entry (to group entries).
    pub separator_before: bool,
    /// Nested entries. A non-empty list makes this a submenu rather than a command, and its
    /// own `id` is then never reported — only a leaf's is.
    ///
    /// Exists because a host's menu can carry a *list* the outliner knows nothing about — a
    /// set of special folders, a set of templates — and flattening those into the top level
    /// buries the commands beside them.
    pub children: Vec<ContextItem>,
}

/// Draws one level of a context menu, returning the action id of whatever leaf was picked.
///
/// Recursive so a submenu is drawn exactly like the top level; a menu that renders its own
/// second level differently is how the two drift apart.
fn context_entries(ui: &mut egui::Ui, items: &[ContextItem]) -> Option<String> {
    let mut chosen = None;
    for item in items {
        if item.separator_before {
            ui.separator();
        }
        if item.children.is_empty() {
            if ui.button(&item.label).clicked() {
                chosen = Some(item.id.clone());
                ui.close();
            }
        } else {
            ui.menu_button(&item.label, |ui| {
                if let Some(picked) = context_entries(ui, &item.children) {
                    chosen = Some(picked);
                }
            });
        }
    }
    chosen
}

impl ContextItem {
    /// New entry with an action key + label and no leading separator.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            separator_before: false,
            children: Vec::new(),
        }
    }

    /// Builder: nest `children` under this entry, making it a submenu.
    pub fn with_children(mut self, children: impl IntoIterator<Item = ContextItem>) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    /// Builder: draw a separator before this entry.
    pub fn with_separator(mut self) -> Self {
        self.separator_before = true;
        self
    }
}

/// Glyphs the widget paints for the disclosure triangle and the per-row
/// visibility **eye** toggle.
///
/// The widget is deliberately font-agnostic: it simply draws whatever string
/// the host supplies at the glyph position. [`Default`] is the set's shared
/// phosphor glyphs (the host registers the phosphor font, as for every other
/// icon-driven widget here), consistent with the icons fed via [`TreeNode::icon`];
/// a host without an icon font overrides the strings.
#[derive(Clone, Debug)]
pub struct OutlinerGlyphs {
    /// Painted in the disclosure column when a parent row is **expanded**.
    /// Default [`icons::CARET_DOWN`](egui_widgets_config::icons::CARET_DOWN).
    pub disclosure_open: String,
    /// Painted in the disclosure column when a parent row is **collapsed**.
    /// Default [`icons::CARET_RIGHT`](egui_widgets_config::icons::CARET_RIGHT).
    pub disclosure_closed: String,
    /// Painted in the eye column when a row is **visible**. Default
    /// [`icons::VISIBLE`](egui_widgets_config::icons::VISIBLE).
    pub eye_shown: String,
    /// Painted in the eye column when a row is **hidden**. Default
    /// [`icons::HIDDEN`](egui_widgets_config::icons::HIDDEN).
    pub eye_hidden: String,
}

impl Default for OutlinerGlyphs {
    /// Shared phosphor glyphs (caret disclosure + eye visibility), matching every
    /// other icon-driven widget in the set; the host registers the phosphor font.
    /// Override the `String`s for a font-free host.
    fn default() -> Self {
        use egui_widgets_config::icons;
        Self {
            disclosure_open: icons::CARET_DOWN.to_string(),
            disclosure_closed: icons::CARET_RIGHT.to_string(),
            eye_shown: icons::VISIBLE.to_string(),
            eye_hidden: icons::HIDDEN.to_string(),
        }
    }
}

impl OutlinerGlyphs {
    /// Builder: set the expanded / collapsed disclosure glyphs.
    pub fn with_disclosure(mut self, open: impl Into<String>, closed: impl Into<String>) -> Self {
        self.disclosure_open = open.into();
        self.disclosure_closed = closed.into();
        self
    }

    /// Builder: set the visible / hidden eye glyphs.
    pub fn with_eye(mut self, shown: impl Into<String>, hidden: impl Into<String>) -> Self {
        self.eye_shown = shown.into();
        self.eye_hidden = hidden.into();
        self
    }
}

/// Static, caller-supplied configuration for one outliner instance.
///
/// Cheap to build each frame (or stash once). Numeric tokens default from
/// [`egui_widgets_config`] when left at their sentinel `0.0` (see [`show`]).
#[derive(Clone, Debug, Default)]
pub struct OutlinerConfig {
    /// Per-level indent in points. `0.0` → egui's default indent.
    pub indent: f32,
    /// Row height hint in points. `0.0` → the shared `Metrics::row_h` token.
    pub row_height: f32,
    /// Right-click menu entries, in order. Empty → no context menu.
    pub context_items: Vec<ContextItem>,
    /// Auto-open a node when it has fewer than this many children and its
    /// [`TreeNode::default_open`] is `None`. `0` → fall back to 20 (the
    /// vfx-view default).
    pub auto_open_below: usize,
    /// Hard depth guard: nodes deeper than this render a "… (too deep)" stub
    /// instead of recursing, preventing stack overflow on degenerate graphs.
    /// `0` → 512 (the vfx-view default).
    pub max_depth: usize,
    /// Show the collapsible colour legend at the bottom.
    pub show_legend: bool,
    /// Emit [`OutlinerAction::ToggleOpen`] when a node expands/collapses.
    pub emit_toggle: bool,
    /// Allow **in-place rename** of the selected row. When `true`, pressing F2
    /// (or double-clicking) the selected row turns its label into a text field;
    /// committing (Enter / focus lost) emits [`OutlinerAction::Rename`], Escape
    /// cancels. When `false` (the default) a double-click keeps emitting
    /// [`OutlinerAction::Activate`], so existing callers are unchanged.
    pub editable_labels: bool,
    /// Show a single-line **filter box** at the top of the panel. When `true`,
    /// the tree is reduced to nodes whose label contains the query
    /// (case-insensitive) plus their ancestors (so matches stay reachable). The
    /// query string is widget-local UI state (kept in egui memory, keyed by the
    /// panel's id) — the model stays read-only. Defaults to `false`.
    pub show_filter: bool,
    /// Enable **Maya-style drag** to reorder / reparent rows. When `true`, every
    /// row becomes a drag source and a drop target: dropping a dragged row onto
    /// another row's upper or lower third reorders it before / after that row
    /// among its siblings, and dropping onto the middle third reparents it as a
    /// child of that row. The resulting move is reported as
    /// [`OutlinerAction::Move`]; the widget mutates nothing (the caller applies
    /// the move to its own forest). Drops onto the dragged node itself or into
    /// its own subtree are refused (they would orphan the subtree). Drag is
    /// suppressed while the filter box is filtering, because the displayed order
    /// then differs from the model order and the move indices would be
    /// ambiguous. Defaults to `false`, so existing callers are unaffected.
    pub draggable: bool,
    /// Glyphs painted for the disclosure triangle and the visibility eye.
    /// Defaults (via [`OutlinerGlyphs::default`]) to the shared phosphor icons
    /// (caret + eye), matching the rest of the widget set; the host registers the
    /// phosphor font. A font-free host can override with any codepoints. See
    /// [`OutlinerConfig::with_glyphs`].
    pub glyphs: OutlinerGlyphs,
    /// Lay the per-row visibility **eye** as a right-aligned column at the row's
    /// right edge (Blender / Houdini style) instead of inline right after the
    /// disclosure triangle. The label fills the space between. Click / toggle
    /// behaviour is identical either way (the eye consumes its own click, so it
    /// never selects the row). Only affects rows whose [`TreeNode::visible`] is
    /// `Some` (the rows that draw an eye at all). Defaults to `false`
    /// (inline-left), so existing callers are unaffected.
    pub visibility_on_right: bool,
    /// Trailing DATA columns (After Effects' `Name | label | Type | Size | FPS`).
    ///
    /// Empty (the default) renders the label-only tree this widget has always
    /// drawn, so existing callers are pixel-identical. When non-empty the label
    /// area shrinks by the columns' total width and the cells are painted at
    /// ABSOLUTE offsets from the row's right edge — so they stay aligned no matter
    /// how deep a row is indented, which is the whole point of a columned tree.
    pub columns: Vec<OutlinerColumn>,
    /// Draw the column header strip above the tree. Ignored when [`Self::columns`]
    /// is empty. Defaults to `false`.
    pub show_header: bool,
    /// Let the user drag the header dividers to resize columns. Widths are kept in
    /// egui memory per panel instance, so two outliners never share them.
    /// Defaults to `false`.
    pub resizable_columns: bool,
}

impl OutlinerConfig {
    /// Builder: set the context-menu entries.
    pub fn with_context_items(mut self, items: impl IntoIterator<Item = ContextItem>) -> Self {
        self.context_items = items.into_iter().collect();
        self
    }

    /// Builder: set the per-level indent (points).
    pub fn with_indent(mut self, indent: f32) -> Self {
        self.indent = indent;
        self
    }

    /// Builder: set the row-height hint (points).
    pub fn with_row_height(mut self, row_height: f32) -> Self {
        self.row_height = row_height;
        self
    }

    /// Builder: set the auto-open child-count threshold.
    pub fn with_auto_open_below(mut self, n: usize) -> Self {
        self.auto_open_below = n;
        self
    }

    /// Builder: set the depth guard.
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// Builder: show the colour legend.
    pub fn with_legend(mut self, show: bool) -> Self {
        self.show_legend = show;
        self
    }

    /// Builder: enable expand/collapse events.
    pub fn with_toggle_events(mut self, emit: bool) -> Self {
        self.emit_toggle = emit;
        self
    }

    /// Builder: allow in-place label editing (F2 / double-click on the selected
    /// row → [`OutlinerAction::Rename`]). See [`OutlinerConfig::editable_labels`].
    pub fn with_editable_labels(mut self, editable: bool) -> Self {
        self.editable_labels = editable;
        self
    }

    /// Builder: show the top filter box. See [`OutlinerConfig::show_filter`].
    pub fn with_filter(mut self, show: bool) -> Self {
        self.show_filter = show;
        self
    }

    /// Builder: enable Maya-style drag reorder / reparent. See
    /// [`OutlinerConfig::draggable`].
    pub fn with_draggable(mut self, draggable: bool) -> Self {
        self.draggable = draggable;
        self
    }

    /// Builder: override the disclosure / eye glyphs. See [`OutlinerGlyphs`].
    pub fn with_glyphs(mut self, glyphs: OutlinerGlyphs) -> Self {
        self.glyphs = glyphs;
        self
    }

    /// Builder: declare the trailing data columns. See [`OutlinerConfig::columns`].
    pub fn with_columns(mut self, columns: impl IntoIterator<Item = OutlinerColumn>) -> Self {
        self.columns = columns.into_iter().collect();
        self
    }

    /// Builder: show the column header strip (and, optionally, let it be dragged to
    /// resize). Implies nothing when there are no columns.
    pub fn with_header(mut self, show: bool, resizable: bool) -> Self {
        self.show_header = show;
        self.resizable_columns = resizable;
        self
    }

    /// Builder: lay the visibility eye as a right-aligned column. See
    /// [`OutlinerConfig::visibility_on_right`].
    pub fn with_visibility_on_right(mut self, on_right: bool) -> Self {
        self.visibility_on_right = on_right;
        self
    }

    /// Resolved auto-open threshold (`0` sentinel → 20).
    fn auto_open(&self) -> usize {
        if self.auto_open_below == 0 {
            20
        } else {
            self.auto_open_below
        }
    }

    /// Resolved depth guard (`0` sentinel → 512).
    fn depth_guard(&self) -> usize {
        if self.max_depth == 0 {
            512
        } else {
            self.max_depth
        }
    }
}

// ============================================================================
// Actions
// ============================================================================

/// A single user action emitted by the outliner this frame.
///
/// All ids are the caller-defined [`TreeNode::id`] values. Nothing here mutates
/// state — this is a pure description of user intent (the decoupled replacement
/// for vfx-view's `PrimTreeAction`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutlinerAction {
    /// A context entry was picked with no row under the pointer.
    ///
    /// Separate from [`OutlinerAction::Context`] rather than making its `id` optional: a
    /// caller that only ever acts on a row keeps its exhaustive match, and one that wants
    /// "add here" on the empty area opts in by handling this.
    ContextEmpty { action: String },
    /// A node header was clicked. `additive` is set for a ctrl/cmd-toggle (toggle
    /// this node, keep the rest); `range` is set for a shift-click (extend from
    /// the anchor to `id`). When both are false it is a plain click → caller
    /// should select only `id`. The caller implements its own selection
    /// semantics; this just reports the modifiers.
    Select {
        id: NodeId,
        /// Ctrl/Cmd was held (toggle, keep existing selection).
        additive: bool,
        /// Shift was held (range-extend from the anchor).
        range: bool,
    },

    /// A node was double-clicked (the generic "open / activate" gesture). Note a
    /// double-click also emits a preceding [`OutlinerAction::Select`].
    Activate { id: NodeId },

    /// A [`ContextItem`] was chosen on a node. `action` is the entry's machine
    /// key ([`ContextItem::id`]); `id` is the node it was invoked on.
    Context { id: NodeId, action: String },

    /// A node was expanded or collapsed. Only emitted when
    /// [`OutlinerConfig::emit_toggle`] is set. `open` is the new state.
    ToggleOpen { id: NodeId, open: bool },

    /// A node's per-row visibility **eye** toggle was clicked (only possible when
    /// the node opted in via [`TreeNode::visible`]). `visible` is the NEW state.
    /// Emitted INSTEAD of [`OutlinerAction::Select`] — clicking the eye never
    /// changes the selection.
    ToggleVisible { id: NodeId, visible: bool },

    /// A sortable column header was clicked. `column` is [`OutlinerColumn::id`] and
    /// `ascending` is the direction the header now shows (clicking the active
    /// column flips it; clicking a different one starts ascending).
    ///
    /// **The widget sorts NOTHING.** It owns no model, so re-ordering here would
    /// mean the rows on screen disagreed with the caller's forest — the same trap
    /// the drag support avoids by reporting [`OutlinerAction::Move`] instead of
    /// moving. The caller re-orders its own data and passes the new forest next
    /// frame; the header arrow reflects what was ASKED for, which is the only thing
    /// the widget can honestly know.
    Sort {
        /// The column's machine key.
        column: String,
        /// `true` = ascending.
        ascending: bool,
    },

    /// A [`CellValue::Color`] swatch was clicked. `column` is the owning
    /// [`OutlinerColumn::id`]. Emitted INSTEAD of [`OutlinerAction::Select`], like
    /// the eye — so a host can open a colour palette without the click also moving
    /// the selection out from under it.
    CellClicked { id: NodeId, column: String },

    /// A row STARTED being dragged (only when [`OutlinerConfig::draggable`]).
    ///
    /// Emitted once, on the frame the drag begins, in ADDITION to the widget's own
    /// reorder/reparent handling — it does not replace [`OutlinerAction::Move`].
    ///
    /// It exists because a drag that LEAVES the outliner is invisible to it: the
    /// widget resolves a drop only over one of its own rows, so releasing anywhere
    /// else is simply no move. A host that also accepts rows elsewhere (dragging a
    /// clip from a project tree onto a timeline) needs to know a drag began in
    /// order to arm that. Without this the two gestures are indistinguishable and
    /// the cross-panel one silently stops working.
    DragStarted { id: NodeId },

    /// A node's label was renamed in place (only possible when
    /// [`OutlinerConfig::editable_labels`] is set). `name` is the committed,
    /// trimmed new label; the caller applies it to its own model.
    Rename { id: NodeId, name: String },

    /// A node was **dragged to a new location** in the hierarchy (only possible
    /// when [`OutlinerConfig::draggable`] is set). The widget mutates nothing;
    /// the caller re-links its own forest to match. The exact contract is:
    ///
    /// * `id` — the dragged node.
    /// * `new_parent` — the node the dragged node should become a child of, or
    ///   `None` to make it a top-level root.
    /// * `index` — the position to insert it at **among `new_parent`'s children
    ///   (or among the roots when `new_parent` is `None`), computed as if the
    ///   dragged node has already been removed from its old location**.
    ///
    /// In other words the caller applies the move as a *remove-then-insert*:
    ///
    /// 1. Detach the node `id` from its current parent (wherever it currently
    ///    lives), keeping the removed subtree.
    /// 2. Resolve the destination child list: `new_parent`'s children, or the
    ///    root list when `new_parent` is `None`.
    /// 3. Insert the detached node into that list at `index` (already valid for
    ///    the post-removal list; clamp to the list length for safety).
    ///
    /// Because `index` is expressed relative to the list *after* the removal, the
    /// caller never has to compensate for the shift caused by moving a node
    /// forward within the same parent. The widget guarantees `new_parent` is
    /// never the dragged node itself nor any node inside its subtree, so the
    /// remove-then-insert can never orphan the moved subtree.
    Move {
        /// The dragged node.
        id: NodeId,
        /// New parent (`None` = a top-level root).
        new_parent: Option<NodeId>,
        /// Insertion index among `new_parent`'s children, post-removal.
        index: usize,
    },

    /// Clicked the empty matte (below the last row, or the viewport behind the
    /// rows). The host should select nothing. Not emitted when Ctrl/Cmd or Shift
    /// is held, so an additive gesture does not wipe the current set.
    ClearSelection,

    /// Live rubber-band selection. `ids` is the FULL selection the host should
    /// apply now, not a delta: emitted on every change while the drag is held.
    ///
    /// Ctrl/Cmd/Shift is resolved inside the widget against the selection at
    /// drag-start, so shrinking the band can drop rows. A host that unions with
    /// the live selection would latch and never unselect.
    ///
    /// Starts only on empty space. Dragging a row is still reorder / `DragStarted`.
    SelectMany { ids: Vec<NodeId> },
}

// ============================================================================
// Highlight helpers (pure — unit-tested without egui)
// ============================================================================

/// Collect the ids of every node that is a *strict ancestor* of any selected
/// node (i.e. some node in its subtree, below itself, is selected).
///
/// This is the tree-walking equivalent of vfx-view's `is_ancestor_of_selected`
/// (which tested `sel.has_prefix(path) && sel != path` on USD paths): a node is
/// an ancestor-of-selected when a descendant — but not the node itself — is in
/// the selection.
pub fn ancestors_of_selected(roots: &[TreeNode], selection: &[NodeId]) -> HashSet<NodeId> {
    let sel: HashSet<NodeId> = selection.iter().copied().collect();
    let mut out = HashSet::new();
    for root in roots {
        walk_ancestors(root, &sel, &mut out);
    }
    out
}

/// Returns whether `node`'s subtree (including `node` itself) contains a selected
/// node, inserting every strict-ancestor id into `out` along the way.
fn walk_ancestors(node: &TreeNode, sel: &HashSet<NodeId>, out: &mut HashSet<NodeId>) -> bool {
    // A child subtree containing a selection makes THIS node an ancestor.
    let mut child_has = false;
    for child in &node.children {
        if walk_ancestors(child, sel, out) {
            child_has = true;
        }
    }
    if child_has {
        out.insert(node.id);
    }
    child_has || sel.contains(&node.id)
}

/// Flatten the forest into pre-order node ids (roots, then their children).
///
/// Handy for callers resolving a shift-range selection: the range runs between
/// two ids in this visual order. Pure / egui-free.
pub fn flatten_ids(roots: &[TreeNode]) -> Vec<NodeId> {
    let mut out = Vec::new();
    for root in roots {
        push_ids(root, &mut out);
    }
    out
}

fn push_ids(node: &TreeNode, out: &mut Vec<NodeId>) {
    out.push(node.id);
    for child in &node.children {
        push_ids(child, out);
    }
}

// ============================================================================
// Rendering
// ============================================================================

/// Render the outliner into `ui` and return this frame's user actions.
///
/// `model` is read-only (the widget never mutates it); apply the returned
/// [`OutlinerAction`]s to your own copy. The panel fills the available width and
/// scrolls vertically.
pub fn show(ui: &mut Ui, model: &OutlinerModel, config: &OutlinerConfig) -> Vec<OutlinerAction> {
    let cfg = WidgetsConfig::get(ui);
    let sem = semantic(ui);

    // Highlight colour scheme (mirrors vfx-view's UIPrimTreeColors):
    // selected = accent; ancestor-of-selected = accent at reduced alpha (same
    // hue, dimmed); default = the live theme text colour (or a node override).
    let selected_color = sem.accent;
    let ancestor_color = sem.accent.gamma_multiply(0.45);
    let default_color = ui.visuals().text_color();

    // Stable per-instance id base: open-state, filter text and rename buffers all
    // hang off it, so two outliners in the same app never share transient state.
    let base_id = ui.id();

    // --- Filter box (opt-in) ------------------------------------------------
    // The query is widget-local (egui memory) so the caller's model stays
    // read-only; when non-empty the rendered forest is a filtered CLONE.
    let filter_id = base_id.with("__filter");
    let mut query: String = egui_widgets_config::state::get_or_default(ui.ctx(), filter_id);
    if config.show_filter {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut query)
                    .hint_text("Filter…")
                    .desired_width(f32::INFINITY),
            );
        });
        egui_widgets_config::state::set(ui.ctx(), filter_id, query.clone());
    }
    let filtering = config.show_filter && !query.trim().is_empty();
    // Folds made WHILE filtering belong to that filter session, not to the user's
    // persisted tree: the filter forces every parent open, so writing a toggle into
    // the persisted flag would silently collapse (or expand) the branch for good once
    // the filter is cleared. The session map is dropped as soon as filtering stops.
    let filter_open_id = base_id.with("__filter_open");
    let filter_open: HashMap<NodeId, bool> = if filtering {
        egui_widgets_config::state::get_or_default(ui.ctx(), filter_open_id)
    } else {
        egui_widgets_config::state::clear::<HashMap<NodeId, bool>>(ui.ctx(), filter_open_id);
        HashMap::new()
    };
    // Drag reorder / reparent runs only when opted in AND not filtering: a filtered
    // view reorders differently from the model, so the move indices would lie.
    let drag_enabled = config.draggable && !filtering;
    let filtered: Option<Vec<TreeNode>> = if filtering {
        Some(filter_roots(&model.roots, &query.to_lowercase()))
    } else {
        None
    };
    // The forest actually rendered this frame (filtered clone or the model's own).
    let roots: &[TreeNode] = filtered.as_deref().unwrap_or(&model.roots);

    // Ancestor highlight is computed over the RENDERED forest so it matches what
    // is on screen (a filtered view has a different ancestor set).
    let ancestors = ancestors_of_selected(roots, &model.selection);

    // Actions the header emits before the rows are drawn (a sort click). Collected
    // here and seeded into the render context so the caller receives them in the
    // same `Vec` as everything else — one channel of user intent, not two.
    let mut header_actions: Vec<OutlinerAction> = Vec::new();

    // In-progress rename (if any), restored from egui memory for this instance.
    let rename_id = base_id.with("__rename");
    let rename: Option<RenameState> = egui_widgets_config::state::get(ui.ctx(), rename_id);

    let indent = if config.indent > 0.0 {
        config.indent
    } else {
        ui.spacing().indent
    };
    let row_h = ui.spacing().interact_size.y;

    // --- Data columns (opt-in) ---------------------------------------------
    // Resolved once per frame and shared by the header and every row, so the two
    // cannot drift apart. Empty columns => `cells_w == 0.0` => every row takes the
    // untouched label-only path.
    let col_widths = resolved_col_widths(ui.ctx(), base_id, &config.columns);
    // ONE geometry for the whole panel. The header and every row read it, so they
    // cannot disagree about where a column starts.
    let content = ui.max_rect();
    let columns = ColumnLayout::new((content.left(), content.right()), &col_widths);
    if !config.columns.is_empty() && config.show_header {
        draw_header(
            ui,
            base_id,
            config,
            &columns,
            sem.weak,
            cfg.fonts.md,
            &mut header_actions,
        );
    }

    let mut ctx = RenderCtx {
        cfg: config,
        base_id,
        font_size: cfg.fonts.md,
        weak: sem.weak,
        selected_color,
        ancestor_color,
        default_color,
        ancestors: &ancestors,
        depth_guard: config.depth_guard(),
        auto_open: config.auto_open(),
        indent,
        row_h,
        filtering,
        filter_open,
        rename,
        rename_result: None,
        visible_order: Vec::new(),
        node_info: HashMap::new(),
        drag_enabled,
        rows: Vec::new(),
        tree_x: None,
        columns,
        actions: header_actions,
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let pane = ui.max_rect();
            ctx.tree_x = Some((pane.left(), pane.right()));
            // Under the rows: a click/drag on leftover empty space is the AE
            // matte. Rows painted after steal hits on themselves; a drag that
            // *started* here keeps going over rows (marquee).
            let underlay = ui.interact(
                pane,
                ctx.base_id.with("__empty"),
                egui::Sense::click_and_drag(),
            );
            if roots.is_empty() {
                ui.add_space(8.0);
                ui.colored_label(ctx.weak, if filtering { "No matches" } else { "Empty" });
            } else {
                for (i, root) in roots.iter().enumerate() {
                    draw_node(ui, model, root, 0, None, i, &mut ctx);
                }
            }
            // Fill the rest of the viewport so a short list still has a matte.
            let rest_h = ui.available_height().max(8.0);
            let (_fill, fill_resp) = ui.allocate_exact_size(
                egui::vec2(ui.available_width().max(1.0), rest_h),
                egui::Sense::click_and_drag(),
            );
            // The same menu on the empty area below the rows: "add something here" is
            // exactly what a right-click on nothing means, and a tree that only offers it
            // over an existing row cannot be populated from empty.
            let matte = underlay.union(fill_resp);
            if !ctx.cfg.context_items.is_empty() {
                let items = ctx.cfg.context_items.clone();
                let mut chosen = None;
                matte.context_menu(|ui| chosen = context_entries(ui, &items));
                if let Some(action) = chosen {
                    ctx.actions.push(OutlinerAction::ContextEmpty { action });
                }
            }
            handle_empty_and_marquee(ui, &matte, &mut ctx, &model.selection);
        });

    // Resolve an in-progress drag against the rows just laid out: paint the drop
    // indicator and, on pointer release, emit a Move. Drag mode only.
    if ctx.drag_enabled {
        resolve_drop(ui, model, &mut ctx);
    }

    // Keyboard navigation only fires while the pointer is over the panel and no
    // text field elsewhere wants the keys (so it never steals global shortcuts
    // or the filter/rename fields' own input).
    if ui.ui_contains_pointer() && !ui.ctx().egui_wants_keyboard_input() {
        handle_keys(ui, model, roots, &mut ctx);
    }
    if filtering {
        egui_widgets_config::state::set(ui.ctx(), filter_open_id, std::mem::take(&mut ctx.filter_open));
    }

    // Apply / persist the rename buffer: begin stores a fresh buffer, commit emits
    // a Rename action and clears it, cancel clears it, and an ongoing edit is
    // written back for the next frame.
    match ctx.rename_result.take() {
        Some(RenameOutcome::Begin(id, label)) => {
            egui_widgets_config::state::set(
                ui.ctx(),
                rename_id,
                RenameState {
                    id,
                    buf: label,
                    focused: false,
                },
            );
        }
        Some(RenameOutcome::Commit(id, name)) => {
            egui_widgets_config::state::clear::<RenameState>(ui.ctx(), rename_id);
            if !name.is_empty() {
                ctx.actions.push(OutlinerAction::Rename { id, name });
            }
        }
        Some(RenameOutcome::Cancel) => {
            egui_widgets_config::state::clear::<RenameState>(ui.ctx(), rename_id);
        }
        None => {
            if let Some(rs) = ctx.rename.take() {
                egui_widgets_config::state::set(ui.ctx(), rename_id, rs);
            }
        }
    }

    if config.show_legend {
        draw_legend(ui, &ctx);
    }

    ctx.actions
}

/// Per-node bookkeeping recorded while drawing, consumed by keyboard navigation
/// after the tree is laid out (which rows are visible, and each row's open state).
#[derive(Clone, Copy)]
struct NodeInfo {
    /// Whether the node has children (so Left/Right can collapse/expand it).
    has_children: bool,
    /// Current expanded state.
    open: bool,
    /// egui memory id holding this node's persisted open flag.
    open_id: egui::Id,
}

/// In-progress in-place rename, stashed in egui memory between frames.
#[derive(Clone)]
struct RenameState {
    /// Node whose label is being edited.
    id: NodeId,
    /// Live edit buffer.
    buf: String,
    /// Set once the text field has grabbed focus (so we request focus only once).
    focused: bool,
}

/// The rename lifecycle transition requested this frame (applied in [`show`]).
enum RenameOutcome {
    /// Start editing `id` seeded with its current label.
    Begin(NodeId, String),
    /// Commit `id`'s edit to the trimmed new name.
    Commit(NodeId, String),
    /// Abandon the edit (Escape).
    Cancel,
}

/// Per-`show` rendering context: resolved tokens + the action accumulator,
/// threaded through the recursion so the node fn stays terse.
struct RenderCtx<'a> {
    cfg: &'a OutlinerConfig,
    /// Stable per-instance id base (open-state / rename keys derive from it).
    base_id: egui::Id,
    font_size: f32,
    weak: Color32,
    selected_color: Color32,
    ancestor_color: Color32,
    default_color: Color32,
    ancestors: &'a HashSet<NodeId>,
    depth_guard: usize,
    auto_open: usize,
    /// Per-level indent in points.
    indent: f32,
    /// Row height hint (for the eye / disclosure glyph columns).
    row_h: f32,
    /// A non-empty filter is active this frame (force-open nodes so matches show).
    filtering: bool,
    /// Folds toggled during the current filter session ([`RenderCtx::set_open`]);
    /// empty, and unused, when not filtering.
    filter_open: HashMap<NodeId, bool>,
    /// The rename state restored from memory (taken by the row that owns it).
    rename: Option<RenameState>,
    /// The rename transition requested this frame, applied by [`show`].
    rename_result: Option<RenameOutcome>,
    /// Ids of the rows actually drawn, in visual (top-to-bottom) order — the
    /// order Up/Down keyboard navigation walks.
    visible_order: Vec<NodeId>,
    /// Open-state bookkeeping per drawn node, for Left/Right key handling.
    node_info: HashMap<NodeId, NodeInfo>,
    /// Drag reorder / reparent is active this frame (`config.draggable` AND not
    /// filtering — see [`OutlinerConfig::draggable`]). Gates ALL drag logic so a
    /// non-draggable outliner behaves exactly as before.
    drag_enabled: bool,
    /// Model coordinates of every drawn row, in visual order, consumed by
    /// [`resolve_drop`] and the empty-space marquee after the tree is laid out.
    rows: Vec<RowGeom>,
    /// Horizontal extent of the tree column (drag mode only), for pointer
    /// hit-testing a full-width row band.
    tree_x: Option<(f32, f32)>,
    /// WHERE every column is, computed once per frame and shared by the header
    /// and every row. Empty `cells` == the label-only tree.
    columns: ColumnLayout,
    actions: Vec<OutlinerAction>,
}

impl RenderCtx<'_> {
    /// THE open state of a parent row: while filtering, the filter session's own
    /// fold (open by default, so matches stay reachable); otherwise the persisted
    /// flag, falling back to `default_open`.
    fn is_open(&self, ui: &Ui, id: NodeId, open_id: egui::Id, default_open: bool) -> bool {
        if self.filtering {
            self.filter_open.get(&id).copied().unwrap_or(true)
        } else {
            egui_widgets_config::state::get::<bool>(ui.ctx(), open_id).unwrap_or(default_open)
        }
    }

    /// THE one writer of a fold (disclosure click and Left/Right keys alike): into
    /// the filter session while filtering, else the persisted flag; then the
    /// optional [`OutlinerAction::ToggleOpen`].
    fn set_open(&mut self, ui: &Ui, id: NodeId, open_id: egui::Id, open: bool) {
        if self.filtering {
            self.filter_open.insert(id, open);
        } else {
            egui_widgets_config::state::set(ui.ctx(), open_id, open);
        }
        if self.cfg.emit_toggle {
            self.actions.push(OutlinerAction::ToggleOpen { id, open });
        }
    }
}

// ============================================================================
// Drag & drop (opt-in reorder / reparent)
// ============================================================================

/// The drag payload carried while a row is dragged: just the dragged node's id.
/// Parked in egui's [`egui::DragAndDrop`] channel so it bridges the gap between
/// the source row (which starts the drag) and the target row (which resolves the
/// drop). `Send + Sync` as egui's payload store requires.
#[derive(Clone, Copy)]
struct DragPayload {
    /// The node being dragged.
    id: NodeId,
}

/// Which third of a target row the pointer is over, deciding reorder vs reparent:
/// the upper / lower third inserts a sibling before / after the row (an insertion
/// line), the middle third drops the node AS A CHILD of the row (a row highlight).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DropZone {
    /// Upper third: insert immediately before the target, as a sibling.
    Before,
    /// Middle third: drop as a child of the target (reparent).
    Into,
    /// Lower third: insert immediately after the target, as a sibling.
    After,
}

/// Per-row model coordinates recorded while drawing, consumed by [`resolve_drop`]
/// and by the empty-space marquee. `Copy` so the resolver can lift a row out of
/// [`RenderCtx::rows`] without holding a borrow of `ctx`.
#[derive(Clone, Copy)]
struct RowGeom {
    /// This row's node id.
    id: NodeId,
    /// The node's parent id in the model (`None` for a root).
    parent: Option<NodeId>,
    /// The node's index among its parent's children. Equals the display order
    /// because drag is disabled while filtering, so the model order is shown 1:1.
    index: usize,
    /// How many children this node itself has (for the "append as child" slot).
    num_children: usize,
    /// Nesting depth (0 = root), used to inset the reorder insertion line.
    depth: usize,
    /// The drawn row rectangle (screen space) for pointer hit-testing + indicator.
    rect: egui::Rect,
}

/// Collect `root_id`'s node plus every id in its subtree (itself included). A drop
/// is refused when its resulting parent lands inside this set, because re-linking
/// a node under itself or one of its own descendants would detach (orphan) the
/// whole subtree. Pure / egui-free.
fn subtree_ids(roots: &[TreeNode], root_id: NodeId) -> HashSet<NodeId> {
    let mut out = HashSet::new();
    if let Some(n) = find_node(roots, root_id) {
        collect_ids(n, &mut out);
    }
    out
}

/// Depth-first search for a node by id in the forest.
fn find_node(roots: &[TreeNode], id: NodeId) -> Option<&TreeNode> {
    for n in roots {
        if n.id == id {
            return Some(n);
        }
        if let Some(found) = find_node(&n.children, id) {
            return Some(found);
        }
    }
    None
}

/// Insert `node`'s id and all descendant ids into `out`.
fn collect_ids(node: &TreeNode, out: &mut HashSet<NodeId>) {
    out.insert(node.id);
    for c in &node.children {
        collect_ids(c, out);
    }
}

/// Adjust a target slot for the fact that the dragged node is *first removed* from
/// its current parent: when the destination parent is the dragged node's current
/// parent and the node sits before the slot, everything after it shifts down by
/// one, so the post-removal index is one less. This is what lets the caller apply
/// the [`OutlinerAction::Move`] as a plain remove-then-insert without
/// compensating for the shift itself.
fn adjust_for_removal(
    drag_parent: Option<NodeId>,
    drag_index: usize,
    target_parent: Option<NodeId>,
    slot: usize,
) -> usize {
    if drag_parent == target_parent && drag_index < slot {
        slot - 1
    } else {
        slot
    }
}

/// Resolve a hovered (`target`, `zone`) into a concrete `(new_parent, index)` for
/// [`OutlinerAction::Move`], or `None` when the drop is invalid — i.e. the
/// resulting parent is inside `forbidden` (the dragged node's subtree, from
/// [`subtree_ids`]), which would orphan the moved subtree.
///
/// `index` is returned relative to the destination child list *after* the dragged
/// node has been removed from its old parent (see [`adjust_for_removal`]).
fn resolve_move(
    drag: &RowGeom,
    target: &RowGeom,
    zone: DropZone,
    forbidden: &HashSet<NodeId>,
) -> Option<(Option<NodeId>, usize)> {
    // The destination parent + the slot in that parent's ORIGINAL child list.
    let (new_parent, slot) = match zone {
        // Reparent: append after the target's existing children.
        DropZone::Into => (Some(target.id), target.num_children),
        // Reorder: insert before / after the target among its siblings.
        DropZone::Before => (target.parent, target.index),
        DropZone::After => (target.parent, target.index + 1),
    };
    // Guard: never re-link a node under itself or one of its own descendants.
    if let Some(p) = new_parent
        && forbidden.contains(&p)
    {
        return None;
    }
    let index = adjust_for_removal(drag.parent, drag.index, new_parent, slot);
    Some((new_parent, index))
}

/// The state a drag across the eye column is painting, remembered between
/// frames. `None` while no such drag is in flight.
///
/// A drag paints ONE state rather than flipping each eye it crosses: flipping
/// would undo itself the moment the pointer wobbled back over a row, which is
/// exactly what makes per-row clicking tiring in the first place. The state is
/// decided by the eye the drag started on — press a shown eye and you are
/// hiding, press a hidden one and you are showing.
#[derive(Clone, Copy)]
struct EyePaint(bool);

fn eye_paint_id(ctx: &RenderCtx) -> egui::Id {
    ctx.base_id.with("eye-paint")
}

/// Emit a visibility change for `node` and, when it has children, its whole
/// subtree.
///
/// Done here rather than left to the host: the widget is the one holding the
/// tree, so every consumer would otherwise write the same recursive walk, and
/// each would be free to get the "a parent with no eye still has children with
/// eyes" case wrong. A leaf is unaffected — this only widens what a row that
/// already groups things means.
fn emit_visible(node: &TreeNode, visible: bool, actions: &mut Vec<OutlinerAction>) {
    if node.visible.is_some() {
        actions.push(OutlinerAction::ToggleVisible {
            id: node.id,
            visible,
        });
    }
    for child in &node.children {
        emit_visible(child, visible, actions);
    }
}

/// Paint one row's visibility **eye** toggle in the current layout position and
/// emit [`OutlinerAction::ToggleVisible`] when clicked or dragged across.
///
/// Shared by both eye layouts (inline-left and the right-aligned column) so the
/// click / toggle semantics stay identical: the eye owns its own click response,
/// so toggling visibility never also selects the row. Glyphs and colours come
/// from the caller's config / theme.
///
/// Two ways to move several eyes at once, because a tree of any size makes
/// one-at-a-time clicking the slowest part of the widget:
///
/// - **Drag** through the eye column to paint the same state onto every row the
///   pointer crosses.
/// - **A parent's eye** carries its subtree with it.
fn draw_eye(ui: &mut Ui, node: &TreeNode, vis: bool, ctx: &mut RenderCtx) {
    let (eye_rect, eye_resp) =
        ui.allocate_exact_size(egui::vec2(16.0, ctx.row_h), egui::Sense::click_and_drag());
    // Host-supplied glyphs (default: the shared phosphor set); font-agnostic.
    let sym: &str = if vis {
        &ctx.cfg.glyphs.eye_shown
    } else {
        &ctx.cfg.glyphs.eye_hidden
    };
    let col = if vis { ctx.default_color } else { ctx.weak };
    ui.painter().text(
        eye_rect.center(),
        egui::Align2::CENTER_CENTER,
        sym,
        prop(ctx.font_size),
        col,
    );

    let paint_id = eye_paint_id(ctx);
    // A press starts the paint and applies to the row pressed; the drag that
    // follows carries the same state onto its neighbours.
    if eye_resp.drag_started() || eye_resp.clicked() {
        let target = !vis;
        ui.ctx()
            .data_mut(|d| d.insert_temp(paint_id, EyePaint(target)));
        emit_visible(node, target, &mut ctx.actions);
        return;
    }

    let painting: Option<EyePaint> = ui.ctx().data(|d| d.get_temp(paint_id));
    let Some(EyePaint(target)) = painting else {
        return;
    };
    // The drag lives on the eye it started from, so this row sees no drag of
    // its own — the pointer position is what says it has been crossed.
    let held = ui.input(|i| i.pointer.any_down());
    if !held {
        ui.ctx().data_mut(|d| d.remove::<EyePaint>(paint_id));
        return;
    }
    let over = ui
        .input(|i| i.pointer.interact_pos())
        .is_some_and(|p| eye_rect.contains(p));
    if over && vis != target {
        emit_visible(node, target, &mut ctx.actions);
    }
}

/// Draw one row's label (or, while renaming, its in-place text field) plus all
/// the label interactions: select / range-select, double-click activate-or-rename,
/// the right-click context menu, and the drag payload.
///
/// Factored out of [`draw_node`] so the inline-eye and right-aligned-eye layouts
/// share a single source of truth for label behaviour instead of duplicating it.
fn draw_row_label(
    ui: &mut Ui,
    node: &TreeNode,
    ctx: &mut RenderCtx,
    is_selected: bool,
    row_color: Color32,
    editing: bool,
) {
    if editing {
        // Replace the label with a text field for the duration of the edit.
        let mut rs = ctx.rename.take().expect("editing implies a rename state");
        let resp = ui.add(egui::TextEdit::singleline(&mut rs.buf).desired_width(f32::INFINITY));
        if !rs.focused {
            resp.request_focus();
            rs.focused = true;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.rename_result = Some(RenameOutcome::Cancel);
        } else if resp.lost_focus() {
            // Enter or a click elsewhere commits the trimmed buffer.
            ctx.rename_result = Some(RenameOutcome::Commit(node.id, rs.buf.trim().to_string()));
        } else {
            ctx.rename = Some(rs); // still editing next frame
        }
    } else {
        let header_text = node_header_text(node, row_color, ctx.weak, ctx.font_size);
        let resp = ui.selectable_label(is_selected, header_text);
        // In drag mode the row ALSO senses dragging on the SAME widget/id: a
        // plain click still selects, but a press that moves starts a drag
        // (egui disambiguates click vs drag), so a drag never emits Select /
        // Activate.
        let resp = if ctx.drag_enabled {
            resp.interact(egui::Sense::drag())
        } else {
            resp
        };
        let mods = ui.input(|i| i.modifiers);
        if resp.clicked() {
            ctx.actions.push(OutlinerAction::Select {
                id: node.id,
                additive: mods.command || mods.ctrl,
                range: mods.shift,
            });
        }
        if resp.double_clicked() {
            ctx.actions.push(OutlinerAction::Select {
                id: node.id,
                additive: mods.command || mods.ctrl,
                range: mods.shift,
            });
            // Double-click ALWAYS activates, even in an editable tree.
            //
            // It used to start a rename instead, which silently took away the
            // primary action: in an asset browser double-click means "open this",
            // and a tree that renames instead leaves opening unreachable by mouse.
            // Every file manager, and After Effects itself, splits these -
            // double-click opens, F2/Enter renames - so renaming stays keyboard-only
            // (see the F2/Enter handler).
            ctx.actions.push(OutlinerAction::Activate { id: node.id });
        }

        // Caller-defined right-click menu.
        if !ctx.cfg.context_items.is_empty() {
            let items = ctx.cfg.context_items.clone();
            let mut chosen = None;
            resp.context_menu(|ui| chosen = context_entries(ui, &items));
            if let Some(action) = chosen {
                ctx.actions.push(OutlinerAction::Context {
                    id: node.id,
                    action,
                });
            }
        }

        // Publish the drag payload the frame this row starts being dragged.
        // `dnd_set_drag_payload` gates on `drag_started`, so a plain click
        // never turns into a move.
        if ctx.drag_enabled {
            resp.dnd_set_drag_payload(DragPayload { id: node.id });
            // Tell the HOST too, so a drag that leaves this panel can mean
            // something elsewhere (see `OutlinerAction::DragStarted`).
            if resp.drag_started() {
                ctx.actions
                    .push(OutlinerAction::DragStarted { id: node.id });
            }
        }
    }
}

// ============================================================================
// Data columns (opt-in)
// ============================================================================

/// Smallest a column may be dragged to, so a column can never be lost entirely.
const MIN_COL_W: f32 = 24.0;

/// Smallest the name column may be squeezed to by wide data columns.
const MIN_NAME_W: f32 = 48.0;

/// Memory key for one column's user-dragged width (per panel instance, per column).
fn col_width_id(base_id: egui::Id, col_id: &str) -> egui::Id {
    base_id.with(("egui_outliner_col_w", col_id))
}

/// Memory key for the active sort: `(column id, ascending)`.
fn sort_id(base_id: egui::Id) -> egui::Id {
    base_id.with("egui_outliner_sort")
}

/// Resolve every column's live width: the user's dragged override if there is one,
/// otherwise the caller's declared width.
///
/// The clamp applies ONLY to a dragged width — it exists so a column cannot be
/// dragged out of existence or made to swallow the panel. A DECLARED width is
/// taken verbatim, because the caller knows what the column is for: AE's label
/// swatch is about 16 pt wide, and a floor that silently widened it would make the
/// widget disagree with the thing it is imitating.
///
/// A [`OutlinerColumn::fixed`] column skips the lookup entirely. Reading the stored
/// width would also re-apply `MIN_COL_W`, so a swatch column narrower than the floor
/// could never get back to its declared width once dragged even once.
fn resolved_col_widths(
    ctx: &egui::Context,
    base_id: egui::Id,
    cols: &[OutlinerColumn],
) -> Vec<f32> {
    cols.iter()
        .map(|c| {
            if c.fixed {
                return c.width;
            }
            match egui_widgets_config::state::get::<f32>(ctx, col_width_id(base_id, &c.id)) {
                Some(dragged) => dragged.clamp(MIN_COL_W, 800.0),
                None => c.width,
            }
        })
        .collect()
}

/// Where every column starts and ends, in absolute screen x.
///
/// Computed ONCE per frame and read by the header and by every row. That shared
/// geometry is the whole reason the columns line up: a row does no arithmetic of
/// its own, so a deeply indented row cannot drift out of step with a shallow one.
/// The tree - indent, disclosure, label - lives entirely inside [`Self::name`], so
/// depth moves nothing to its right.
#[derive(Clone, Debug, Default)]
struct ColumnLayout {
    /// `(left, width)` of the flexible NAME column.
    name: (f32, f32),
    /// `(left, width)` of each data column, parallel to `OutlinerConfig::columns`.
    cells: Vec<(f32, f32)>,
}

impl ColumnLayout {
    /// Lay `widths` out inside `x_range`, giving the name column what is left.
    fn new(x_range: (f32, f32), widths: &[f32]) -> Self {
        let cells_w: f32 = widths.iter().sum();
        let name_w = (x_range.1 - x_range.0 - cells_w).max(MIN_NAME_W);
        let mut x = x_range.0 + name_w;
        let cells = widths
            .iter()
            .map(|w| {
                let at = (x, *w);
                x += *w;
                at
            })
            .collect();
        Self {
            name: (x_range.0, name_w),
            cells,
        }
    }

    /// True when there are no data columns — the label-only tree.
    fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The name column as a rect on `row`.
    fn name_rect(&self, row: egui::Rect) -> egui::Rect {
        egui::Rect::from_min_size(
            egui::pos2(self.name.0, row.top()),
            egui::vec2(self.name.1, row.height()),
        )
    }

    /// Data column `i` as a rect on `row`.
    fn cell_rect(&self, i: usize, row: egui::Rect) -> Option<egui::Rect> {
        let (x, w) = *self.cells.get(i)?;
        Some(egui::Rect::from_min_size(
            egui::pos2(x, row.top()),
            egui::vec2(w, row.height()),
        ))
    }
}

/// Draw the column header strip: the name header, then one header per data column.
///
/// A divider sits at each data column's LEFT edge and resizes THAT column, so the
/// flexible name column absorbs the difference and dragging never reflows the
/// columns to its right. Widths persist in egui memory per panel instance.
///
/// Clicking a sortable header emits [`OutlinerAction::Sort`] and flips the arrow;
/// the widget re-orders nothing (see that action's docs).
fn draw_header(
    ui: &mut Ui,
    base_id: egui::Id,
    cfg: &OutlinerConfig,
    layout: &ColumnLayout,
    weak: Color32,
    font_size: f32,
    actions: &mut Vec<OutlinerAction>,
) {
    let cols = &cfg.columns;
    let resizable = cfg.resizable_columns;
    let mut sort: Option<(String, bool)> =
        egui_widgets_config::state::get(ui.ctx(), sort_id(base_id));
    let row_h = ui.spacing().interact_size.y;

    // One full-width strip, then paint into the SHARED column geometry - the same
    // rects every row will use.
    let (strip, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_h),
        egui::Sense::hover(),
    );
    {
        let name_rect = layout.name_rect(strip);
        ui.painter().text(
            egui::pos2(name_rect.left() + 4.0, name_rect.center().y),
            egui::Align2::LEFT_CENTER,
            "Name",
            prop(font_size),
            weak,
        );

        for (i, col) in cols.iter().enumerate() {
            let Some(rect) = layout.cell_rect(i, strip) else {
                continue;
            };
            let w = rect.width();
            let sense = if col.sortable {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            };
            let resp = ui.interact(rect, base_id.with(("__hdr", &col.id)), sense);

            // Header text + the sort arrow for the active column.
            let active = sort.as_ref().is_some_and(|(c, _)| c == &col.id);
            let arrow = match (&sort, active) {
                (Some((_, asc)), true) => {
                    if *asc {
                        egui_widgets_config::icons::CARET_UP
                    } else {
                        egui_widgets_config::icons::CARET_DOWN
                    }
                }
                _ => "",
            };
            let text = if arrow.is_empty() {
                col.title.to_string()
            } else {
                format!("{} {arrow}", col.title)
            };
            let (anchor, x) = match col.align {
                ColumnAlign::Left => (egui::Align2::LEFT_CENTER, rect.left() + 4.0),
                ColumnAlign::Center => (egui::Align2::CENTER_CENTER, rect.center().x),
                ColumnAlign::Right => (egui::Align2::RIGHT_CENTER, rect.right() - 4.0),
            };
            ui.painter().text(
                egui::pos2(x, rect.center().y),
                anchor,
                text,
                prop(font_size),
                weak,
            );

            if col.sortable && resp.clicked() {
                // Same column flips direction; a new column starts ascending.
                let ascending = if active {
                    !sort.as_ref().map(|(_, a)| *a).unwrap_or(true)
                } else {
                    true
                };
                sort = Some((col.id.clone(), ascending));
                egui_widgets_config::state::set(
                    ui.ctx(),
                    sort_id(base_id),
                    (col.id.clone(), ascending),
                );
                actions.push(OutlinerAction::Sort {
                    column: col.id.clone(),
                    ascending,
                });
            }

            // Divider at this column's left edge, dragged to resize this column.
            //
            // A `fixed` column keeps the LINE but loses the HANDLE: the separator is
            // what makes the columns readable, while the drag on it only ever widened
            // a column whose content has a single size.
            if resizable {
                if !col.fixed {
                    let handle = egui::Rect::from_min_max(
                        egui::pos2(rect.left() - 2.0, rect.top()),
                        egui::pos2(rect.left() + 2.0, rect.bottom()),
                    );
                    let id = col_width_id(base_id, &col.id).with("__drag");
                    let dr = ui.interact(handle, id, egui::Sense::drag());
                    if dr.hovered() || dr.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if dr.dragged() {
                        // Dragging LEFT widens the column (its right edge is fixed by
                        // the columns after it), which is what the cursor implies.
                        let next = (w - dr.drag_delta().x).clamp(MIN_COL_W, 800.0);
                        egui_widgets_config::state::set(
                            ui.ctx(),
                            col_width_id(base_id, &col.id),
                            next,
                        );
                    }
                }
                ui.painter().vline(
                    rect.left(),
                    rect.y_range(),
                    egui::Stroke::new(1.0, weak.gamma_multiply(0.5)),
                );
            }
        }
    }
    ui.separator();
}

/// Paint one row's data cells into the SHARED column geometry.
///
/// Takes the row's rect and asks [`ColumnLayout`] where each column is, rather
/// than allocating its own widths in sequence. That is what makes a cell land
/// under its own header no matter how deep the row is indented - the property
/// that makes a columned tree readable at all, and the one that broke when this
/// laid itself out inline after the label.
///
/// A [`CellValue::Color`] swatch is clickable and consumes its own click, exactly
/// like the visibility eye, so opening a colour palette never also moves the
/// selection.
fn draw_cells(ui: &mut Ui, node: &TreeNode, row: egui::Rect, ctx: &mut RenderCtx) {
    for (i, col) in ctx.cfg.columns.iter().enumerate() {
        let Some(rect) = ctx.columns.cell_rect(i, row) else {
            continue;
        };
        // A node may carry fewer cells than there are columns; the rest are blank.
        let value = node.cells.get(i).unwrap_or(&CellValue::Empty);
        let sense = if matches!(value, CellValue::Color(_)) {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let resp = ui.interact(rect, ctx.base_id.with(("__cell", node.id, i)), sense);

        match value {
            CellValue::Empty => {}
            CellValue::Text(text) => {
                let (anchor, x) = match col.align {
                    ColumnAlign::Left => (egui::Align2::LEFT_CENTER, rect.left() + 4.0),
                    ColumnAlign::Center => (egui::Align2::CENTER_CENTER, rect.center().x),
                    ColumnAlign::Right => (egui::Align2::RIGHT_CENTER, rect.right() - 4.0),
                };
                ui.painter().text(
                    egui::pos2(x, rect.center().y),
                    anchor,
                    text,
                    prop(ctx.font_size),
                    ctx.weak,
                );
            }
            CellValue::Color(color) => {
                // A small centred swatch rather than a filled cell: a full-bleed
                // colour would read as a selection highlight.
                let side = (ctx.row_h - 6.0).max(6.0);
                let sw = egui::Rect::from_center_size(rect.center(), egui::vec2(side, side));
                ui.painter().rect_filled(sw, 2.0, *color);
                if resp.clicked() {
                    ctx.actions.push(OutlinerAction::CellClicked {
                        id: node.id,
                        column: col.id.clone(),
                    });
                }
            }
        }
    }
}

/// Draw one node and (recursively) its children, with a depth guard.
///
/// Each row is laid out by hand as `[indent][disclosure][eye?][label]` so the
/// per-row visibility toggle and rename field compose cleanly with the tree
/// structure (egui's `CollapsingHeader` owns the whole row and cannot host a
/// leading widget). Open state is kept in egui memory keyed off the caller's
/// stable [`NodeId`], so it survives frame-to-frame regardless of label changes.
///
/// `parent` / `index` are the node's model coordinates (its parent id and its
/// position among that parent's children); they are recorded per row so the drag
/// resolver can turn a drop into an [`OutlinerAction::Move`]. They are ignored
/// unless [`RenderCtx::drag_enabled`].
fn draw_node(
    ui: &mut Ui,
    model: &OutlinerModel,
    node: &TreeNode,
    depth: usize,
    parent: Option<NodeId>,
    index: usize,
    ctx: &mut RenderCtx,
) {
    // Guard against stack overflow from degenerate hierarchies.
    if depth > ctx.depth_guard {
        ui.label("… (too deep)");
        return;
    }

    let has_children = !node.children.is_empty();
    let is_selected = model.is_selected(node.id);
    let is_ancestor = !is_selected && ctx.ancestors.contains(&node.id);
    let row_color = if is_selected {
        ctx.selected_color
    } else if is_ancestor {
        ctx.ancestor_color
    } else {
        node.color.unwrap_or(ctx.default_color)
    };

    // Resolve the open state: while filtering every parent is forced open so the
    // matches beneath it stay reachable; otherwise use the persisted flag falling
    // back to the caller's `default_open` / auto-open heuristic.
    let default_open = node
        .default_open
        .unwrap_or(node.children.len() < ctx.auto_open);
    let open_id = ctx.base_id.with(("egui_outliner_open", node.id));
    let mut open = has_children && ctx.is_open(ui, node.id, open_id, default_open);

    // Record this row in visual order for keyboard navigation.
    ctx.visible_order.push(node.id);

    // Whether this row is currently being renamed in place.
    let editing = ctx.cfg.editable_labels && ctx.rename.as_ref().map(|r| r.id) == Some(node.id);

    // The tree half of the row. Factored out of the layout branch below so the
    // no-columns path stays EXACTLY what it always was (colmap-rs / vfx-view are
    // the existing consumers) instead of being a duplicated near-copy.
    let tree_part = |ui: &mut Ui, ctx: &mut RenderCtx, open: &mut bool| {
        if depth > 0 {
            ui.add_space(depth as f32 * ctx.indent);
        }

        // Disclosure column (fixed width so labels align across leaf / parent rows).
        let (tri_rect, tri_resp) =
            ui.allocate_exact_size(egui::vec2(14.0, ctx.row_h), egui::Sense::click());
        if has_children {
            // Host-supplied glyphs (default: the shared phosphor set); font-agnostic.
            let sym: &str = if *open {
                &ctx.cfg.glyphs.disclosure_open
            } else {
                &ctx.cfg.glyphs.disclosure_closed
            };
            ui.painter().text(
                tri_rect.center(),
                egui::Align2::CENTER_CENTER,
                sym,
                prop(ctx.font_size),
                ctx.weak,
            );
            if tri_resp.clicked() {
                *open = !*open;
                ctx.set_open(ui, node.id, open_id, *open);
            }
        }

        // Per-row visibility eye. Two layouts share one drawing helper
        // ([`draw_eye`]) and one label helper ([`draw_row_label`]) so click /
        // toggle behaviour is identical regardless of where the eye sits:
        //  * default (inline-left): eye right after the disclosure, then label;
        //  * `visibility_on_right`: label fills the middle, eye pinned to the
        //    row's right edge via a right-to-left sub-layout (Blender-style).
        // The eye owns its own click response, so it never selects the row.
        match (node.visible, ctx.cfg.visibility_on_right) {
            (Some(vis), true) => {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Allocated first in a right-to-left layout → sits rightmost.
                    draw_eye(ui, node, vis, ctx);
                    // The label consumes the remaining width to the eye's left.
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        draw_row_label(ui, node, ctx, is_selected, row_color, editing);
                    });
                });
            }
            (Some(vis), false) => {
                draw_eye(ui, node, vis, ctx);
                draw_row_label(ui, node, ctx, is_selected, row_color, editing);
            }
            (None, _) => {
                draw_row_label(ui, node, ctx, is_selected, row_color, editing);
            }
        }
    };

    let row_rect = if ctx.columns.is_empty() {
        // No data columns: the original label-only tree, laid out inline.
        ui.horizontal(|ui| tree_part(ui, ctx, &mut open))
            .response
            .rect
    } else {
        // Columned: take the WHOLE row as one rect first, so every part of this
        // row is placed by the shared `ColumnLayout` instead of flowing after
        // whatever the previous part happened to measure. Row-wide interaction
        // (select, context menu, drag-to-reparent) also needs the full-width rect,
        // which is why the row is allocated whole rather than per column.
        let (row, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), ctx.row_h),
            egui::Sense::hover(),
        );
        // The tree - indent, disclosure, eye, label - lives ENTIRELY inside the
        // name column, clipped to it. That containment is what keeps depth from
        // moving anything to its right.
        let name_rect = ctx.columns.name_rect(row);
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(name_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        child.set_clip_rect(name_rect.intersect(ui.clip_rect()));
        tree_part(&mut child, ctx, &mut open);
        draw_cells(ui, node, row, ctx);
        row
    };

    ctx.node_info.insert(
        node.id,
        NodeInfo {
            has_children,
            open,
            open_id,
        },
    );

    // Record this row's model coordinates + screen rect for the drag resolver
    // AND the marquee (empty-space rubber band needs every visible row's rect).
    ctx.rows.push(RowGeom {
        id: node.id,
        parent,
        index,
        num_children: node.children.len(),
        depth,
        rect: row_rect,
    });

    // Recurse into visible children, threading each child's sibling index.
    if has_children && open {
        for (i, child) in node.children.iter().enumerate() {
            draw_node(ui, model, child, depth + 1, Some(node.id), i, ctx);
        }
    }
}

#[derive(Clone)]
struct MarqueeGesture {
    start: egui::Pos2,
    additive: bool,
    /// Selection at drag-start. Additive marquees union with THIS, not the live
    /// selection, or shrinking the band could never drop a row.
    baseline: Vec<NodeId>,
    /// Last set we emitted. `None` = nothing sent yet this gesture.
    last_ids: Option<Vec<NodeId>>,
}

/// Visible rows whose rect intersects `band`, in the order they were drawn.
fn ids_hit_by_marquee(rows: &[RowGeom], band: egui::Rect) -> Vec<NodeId> {
    rows.iter()
        .filter(|r| r.rect.intersects(band))
        .map(|r| r.id)
        .collect()
}

/// The selection a marquee should apply: hits alone, or hits unioned with the
/// drag-start snapshot when additive.
fn marquee_selection(hits: &[NodeId], baseline: &[NodeId], additive: bool) -> Vec<NodeId> {
    if !additive {
        return hits.to_vec();
    }
    let mut out = baseline.to_vec();
    for id in hits {
        if !out.contains(id) {
            out.push(*id);
        }
    }
    out
}

/// Empty-matte click (clear) and rubber-band (marquee). A row drag in flight
/// owns the pointer — do not steal it.
fn handle_empty_and_marquee(
    ui: &mut Ui,
    empty: &egui::Response,
    ctx: &mut RenderCtx,
    selection: &[NodeId],
) {
    if ctx.rename.is_some() {
        return;
    }
    let row_drag = egui::DragAndDrop::payload::<DragPayload>(ui.ctx()).is_some();
    let marquee_id = ctx.base_id.with("__marquee");
    let mods = ui.input(|i| i.modifiers);
    let additive = mods.command || mods.ctrl || mods.shift;

    if !row_drag && empty.clicked() && !additive {
        ctx.actions.push(OutlinerAction::ClearSelection);
    }

    if !row_drag
        && empty.drag_started()
        && !empty.clicked()
        && let Some(p) = empty.interact_pointer_pos()
    {
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                marquee_id,
                MarqueeGesture {
                    start: p,
                    additive,
                    baseline: selection.to_vec(),
                    last_ids: None,
                },
            )
        });
    }

    let Some(mut gesture) = ui.ctx().data(|d| d.get_temp::<MarqueeGesture>(marquee_id)) else {
        return;
    };
    ui.ctx().request_repaint();
    let ptr = ui
        .input(|i| i.pointer.interact_pos())
        .unwrap_or(gesture.start);
    let band = egui::Rect::from_two_pos(gesture.start, ptr);
    let fill = ctx.selected_color.gamma_multiply(0.20);
    ui.painter().rect(
        band,
        0.0,
        fill,
        egui::Stroke::new(1.0, ctx.selected_color),
        egui::StrokeKind::Inside,
    );

    let ids = marquee_selection(
        &ids_hit_by_marquee(&ctx.rows, band),
        &gesture.baseline,
        gesture.additive,
    );
    if gesture.last_ids.as_ref() != Some(&ids) {
        ctx.actions
            .push(OutlinerAction::SelectMany { ids: ids.clone() });
        gesture.last_ids = Some(ids);
    }

    if ui.input(|i| i.pointer.primary_released()) {
        ui.ctx()
            .data_mut(|d| d.remove::<MarqueeGesture>(marquee_id));
    } else {
        ui.ctx().data_mut(|d| d.insert_temp(marquee_id, gesture));
    }
}

/// Resolve an in-progress drag against the rows laid out this frame (drag mode
/// only). Reads the active [`DragPayload`], finds the row under the pointer,
/// decides reorder-vs-reparent from which third of that row the pointer is in,
/// refuses drops that would orphan the subtree, paints the drop indicator, and —
/// on pointer release — pushes the resulting [`OutlinerAction::Move`].
///
/// The widget never mutates the model here: it only reports the move.
fn resolve_drop(ui: &mut Ui, model: &OutlinerModel, ctx: &mut RenderCtx) {
    // Nothing to do unless a row of ours is currently being dragged.
    let Some(payload) = egui::DragAndDrop::payload::<DragPayload>(ui.ctx()) else {
        return;
    };
    let drag_id = payload.id;
    // We need the dragged row's own model coordinates, the tree column extent, and
    // the pointer position; bail if any is missing (e.g. dragged row scrolled off).
    let (Some(drag), Some((x0, x1)), Some(ptr)) = (
        ctx.rows.iter().find(|r| r.id == drag_id).copied(),
        ctx.tree_x,
        ui.input(|i| i.pointer.interact_pos()),
    ) else {
        return;
    };

    // The row whose vertical band the pointer is in (full width of the column).
    let Some(target) = ctx
        .rows
        .iter()
        .find(|r| ptr.y >= r.rect.top() && ptr.y < r.rect.bottom() && ptr.x >= x0 && ptr.x <= x1)
        .copied()
    else {
        return;
    };

    // Upper third → Before, middle third → Into (reparent), lower third → After.
    let frac = ((ptr.y - target.rect.top()) / target.rect.height().max(1.0)).clamp(0.0, 1.0);
    let zone = if frac < 1.0 / 3.0 {
        DropZone::Before
    } else if frac > 2.0 / 3.0 {
        DropZone::After
    } else {
        DropZone::Into
    };

    // Guard + resolve; an invalid drop draws no indicator and emits nothing (the
    // payload is cleared by egui's own end-of-frame release handler).
    let forbidden = subtree_ids(&model.roots, drag_id);
    let Some((new_parent, index)) = resolve_move(&drag, &target, zone, &forbidden) else {
        return;
    };

    // Drop indicator: an insertion line for a reorder, a row highlight for a
    // reparent. Drawn on top of the tree (after the scroll area laid it out).
    let accent = ctx.selected_color;
    let painter = ui.painter();
    match zone {
        DropZone::Into => {
            painter.rect(
                target.rect,
                2.0,
                accent.gamma_multiply(0.20),
                egui::Stroke::new(1.5, accent),
                egui::StrokeKind::Inside,
            );
        }
        DropZone::Before | DropZone::After => {
            let y = if zone == DropZone::Before {
                target.rect.top()
            } else {
                target.rect.bottom()
            };
            // Inset by one indent level past the target's own depth, so the line
            // reads at the sibling depth the node would land at.
            let x_start = (x0 + (target.depth as f32 + 1.0) * ctx.indent).min(x1);
            painter.hline(x_start..=x1, y, egui::Stroke::new(2.0, accent));
            painter.circle_filled(egui::pos2(x_start, y), 3.0, accent);
        }
    }

    // Commit on release. egui also clears the payload at end of frame, but we take
    // it explicitly so the move fires exactly once.
    if ui.input(|i| i.pointer.any_released()) {
        egui::DragAndDrop::clear_payload(ui.ctx());
        ctx.actions.push(OutlinerAction::Move {
            id: drag_id,
            new_parent,
            index,
        });
    }
}

/// Handle Up/Down (move selection between visible rows), Left/Right (collapse /
/// expand the selected node) and F2 / Enter (begin rename, when editable). Called
/// only while the pointer is over the panel and no text field wants the keyboard.
fn handle_keys(ui: &mut Ui, model: &OutlinerModel, roots: &[TreeNode], ctx: &mut RenderCtx) {
    let (up, down, left, right, f2, enter) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::ArrowLeft),
            i.key_pressed(egui::Key::ArrowRight),
            i.key_pressed(egui::Key::F2),
            i.key_pressed(egui::Key::Enter),
        )
    });
    if !(up || down || left || right || f2 || enter) {
        return;
    }

    // Up/Down: move the selection along the visible row order.
    if up || down {
        let next = {
            let order = &ctx.visible_order;
            if order.is_empty() {
                None
            } else {
                let cur = model
                    .selection
                    .iter()
                    .find_map(|s| order.iter().position(|id| id == s));
                let idx = match (cur, down) {
                    (Some(i), true) => (i + 1).min(order.len() - 1),
                    (Some(i), false) => i.saturating_sub(1),
                    (None, true) => 0,
                    (None, false) => order.len() - 1,
                };
                Some(order[idx])
            }
        };
        if let Some(id) = next {
            ctx.actions.push(OutlinerAction::Select {
                id,
                additive: false,
                range: false,
            });
        }
    }

    // Left/Right: collapse / expand the selected node (if it has children).
    if (left || right)
        && let Some(sel) = model.selection.first().copied()
        && let Some(info) = ctx.node_info.get(&sel).copied()
        && info.has_children
    {
        let want_open = right;
        if info.open != want_open {
            ctx.set_open(ui, sel, info.open_id, want_open);
        }
    }

    // F2 or Enter: begin an in-place rename of the selected node (editable trees
    // only). Keyboard-only ON PURPOSE - double-click belongs to "open", see the
    // double-click handler. Enter is accepted alongside F2 because that is the
    // rename key on macOS and in several NLEs, and neither means anything else
    // here while a row is selected.
    //
    // `rename_result.is_none()` is what makes Enter COMMIT rather than restart.
    // The committing Enter is still in this frame's input when `handle_keys` runs,
    // and the field has already surrendered focus by then, so the guard on the call
    // site (`!egui_wants_keyboard_input`) no longer holds. Without this check the
    // assignment below overwrote the `Commit` the row had just produced with a fresh
    // `Begin`: no Rename action was ever emitted and the field reopened showing the
    // ORIGINAL label - i.e. "I typed a new name, pressed Enter, and nothing
    // happened".
    if (f2 || enter)
        && ctx.cfg.editable_labels
        && ctx.rename_result.is_none()
        && let Some(sel) = model.selection.first().copied()
        && let Some(label) = find_label(roots, sel)
    {
        ctx.rename_result = Some(RenameOutcome::Begin(sel, label));
    }
}

/// Find a node's label by id in the rendered forest (used to seed a rename).
fn find_label(roots: &[TreeNode], id: NodeId) -> Option<String> {
    for n in roots {
        if n.id == id {
            return Some(n.label.clone());
        }
        if let Some(l) = find_label(&n.children, id) {
            return Some(l);
        }
    }
    None
}

/// Build a filtered CLONE of the forest: keep every node whose label matches
/// `query_lc` (already lower-cased) plus the ancestors on the path to a match, so
/// matches stay reachable. A matching node keeps its full subtree browsable.
fn filter_roots(roots: &[TreeNode], query_lc: &str) -> Vec<TreeNode> {
    roots
        .iter()
        .filter_map(|n| filter_node(n, query_lc))
        .collect()
}

/// Recursive half of [`filter_roots`]: `Some` when this node (or a descendant)
/// matches, `None` when the whole subtree should be pruned.
fn filter_node(node: &TreeNode, query_lc: &str) -> Option<TreeNode> {
    if node.label.to_lowercase().contains(query_lc) {
        // A direct match keeps its entire subtree so the user can browse into it.
        return Some(node.clone());
    }
    let kids: Vec<TreeNode> = node
        .children
        .iter()
        .filter_map(|c| filter_node(c, query_lc))
        .collect();
    if kids.is_empty() {
        None
    } else {
        let mut n = node.clone();
        n.children = kids;
        Some(n)
    }
}

/// Build the collapsing-header text: optional `icon`, the `label` in `row_color`,
/// then a dimmed `[type_label]` suffix — assembled as a [`LayoutJob`] so the
/// suffix can carry its own (weak) colour.
fn node_header_text(node: &TreeNode, row_color: Color32, weak: Color32, size: f32) -> LayoutJob {
    let font = prop(size);
    let mut job = LayoutJob::default();
    if let Some(icon) = &node.icon {
        job.append(icon, 0.0, fmt(font.clone(), row_color));
        job.append(" ", 0.0, fmt(font.clone(), row_color));
    }
    job.append(&node.label, 0.0, fmt(font.clone(), row_color));
    if let Some(t) = &node.type_label {
        job.append(&format!("  [{t}]"), 0.0, fmt(font, weak));
    }
    job
}

/// A [`TextFormat`] with a given font + colour (the only fields the header uses).
fn fmt(font_id: egui::FontId, color: Color32) -> TextFormat {
    TextFormat {
        font_id,
        color,
        ..Default::default()
    }
}

/// Collapsible colour legend explaining the highlight scheme (matches the
/// vfx-view prim-tree legend, reduced to the generic entries).
fn draw_legend(ui: &mut Ui, ctx: &RenderCtx) {
    egui::CollapsingHeader::new(egui::RichText::new("Legend").small().color(ctx.weak))
        .default_open(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let entries: [(&str, Color32); 3] = [
                ("Selected", ctx.selected_color),
                ("Ancestor of selected", ctx.ancestor_color),
                ("Default", ctx.default_color),
            ];
            for (label, color) in entries {
                ui.horizontal(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2.0, color);
                    ui.label(egui::RichText::new(label).small());
                });
            }
        });
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a small tree:
    /// ```text
    /// 1 ─ 2 ─ 4
    ///   └ 3
    /// 5
    /// ```
    fn sample() -> Vec<TreeNode> {
        vec![
            TreeNode::new(1, "root")
                .child(TreeNode::new(2, "a").child(TreeNode::new(4, "leaf")))
                .child(TreeNode::new(3, "b")),
            TreeNode::new(5, "other"),
        ]
    }

    /// A parent's eye carries its whole subtree.
    ///
    /// The case worth pinning is the middle one: a grouping row that has **no**
    /// eye of its own still has descendants that do, and skipping its children
    /// because it had nothing to emit for itself is the obvious way to write
    /// this wrong.
    #[test]
    fn a_parent_eye_reaches_every_descendant_that_has_one() {
        let tree = TreeNode::new(1, "root")
            .with_visible(true)
            .child(
                // No eye on this row, but its child has one.
                TreeNode::new(2, "group").child(TreeNode::new(4, "leaf").with_visible(true)),
            )
            .child(TreeNode::new(3, "b").with_visible(true));

        let mut acts = Vec::new();
        emit_visible(&tree, false, &mut acts);

        let hidden: Vec<NodeId> = acts
            .iter()
            .filter_map(|a| match a {
                OutlinerAction::ToggleVisible { id, visible: false } => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(
            hidden,
            vec![1, 4, 3],
            "parent, the eyeless group's leaf, and b"
        );
        assert_eq!(acts.len(), 3, "a row with no eye emits nothing for itself");
    }

    fn row(id: NodeId, y: f32) -> RowGeom {
        RowGeom {
            id,
            parent: None,
            index: 0,
            num_children: 0,
            depth: 0,
            rect: egui::Rect::from_min_size(egui::pos2(0.0, y), egui::vec2(100.0, 10.0)),
        }
    }

    #[test]
    fn marquee_hits_rows_the_band_covers() {
        let rows = [row(1, 0.0), row(2, 10.0), row(3, 20.0)];
        let band = egui::Rect::from_min_max(egui::pos2(0.0, 12.0), egui::pos2(50.0, 25.0));
        assert_eq!(ids_hit_by_marquee(&rows, band), vec![2, 3]);
    }

    #[test]
    fn additive_marquee_unions_with_the_drag_start_set() {
        let hits = vec![2, 3];
        let baseline = vec![1, 9];
        assert_eq!(marquee_selection(&hits, &baseline, true), vec![1, 9, 2, 3]);
    }

    #[test]
    fn shrinking_an_additive_marquee_drops_only_what_the_band_added() {
        let hits = vec![2];
        let baseline = vec![1];
        assert_eq!(marquee_selection(&hits, &baseline, true), vec![1, 2]);
        assert_eq!(marquee_selection(&[], &baseline, true), vec![1]);
    }

    #[test]
    fn a_plain_marquee_is_exactly_the_hits() {
        assert_eq!(marquee_selection(&[2, 3], &[1], false), vec![2, 3]);
        assert!(marquee_selection(&[], &[1], false).is_empty());
    }

    #[test]
    fn marquee_misses_when_the_band_is_beside_the_rows() {
        let rows = [row(1, 0.0), row(2, 10.0)];
        let band = egui::Rect::from_min_max(egui::pos2(200.0, 0.0), egui::pos2(250.0, 40.0));
        assert!(ids_hit_by_marquee(&rows, band).is_empty());
    }

    /// A leaf is unaffected by the subtree rule — it is still one eye, one row.
    #[test]
    fn a_leaf_eye_moves_only_itself() {
        let leaf = TreeNode::new(9, "leaf").with_visible(false);
        let mut acts = Vec::new();
        emit_visible(&leaf, true, &mut acts);
        assert!(matches!(
            acts.as_slice(),
            [OutlinerAction::ToggleVisible {
                id: 9,
                visible: true
            }]
        ));
    }

    /// Run one headless egui frame at a KNOWN panel width and hand the `Ui` to
    /// `body` (the `run_ui` idiom this repo's egui-splitter-table already uses).
    ///
    /// The column code is LAYOUT code: asserting on it without laying it out would
    /// only test my arithmetic, not the widget. The explicit `screen_rect` is what
    /// makes the name-vs-columns split a real division rather than a no-op against
    /// an unbounded width.
    fn frame(body: impl FnOnce(&mut Ui)) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            ..Default::default()
        };
        let mut body = Some(body);
        let mut out = ctx.run_ui(input, |ui| {
            if let Some(body) = body.take() {
                body(ui);
            }
        });
        // epaint 0.36 panics when a `FullOutput` is dropped with an unapplied
        // `TexturesDelta` — the font atlas a headless run still builds and that
        // no painter here will ever upload. Discarding it used to be silent;
        // now it has to be said out loud, which is the point of the assertion.
        out.textures_delta.clear();
    }

    /// The AE-style column set used by the tests below.
    fn columns() -> Vec<OutlinerColumn> {
        vec![
            OutlinerColumn::new("label", "", 18.0).with_align(ColumnAlign::Center),
            OutlinerColumn::new("type", "Type", 90.0).sortable(),
            OutlinerColumn::new("size", "Size", 70.0)
                .with_align(ColumnAlign::Right)
                .sortable(),
        ]
    }

    /// THE alignment property, and the one that regressed: a cell's x position
    /// comes from the shared layout and therefore does not depend on the row - so
    /// a deeply indented row and a root row put the same column in the same place.
    ///
    /// This is what "the tree lives inside the name column" buys, and asserting it
    /// on the geometry (rather than by eye) is why the geometry is a value.
    #[test]
    fn a_column_lands_at_the_same_x_on_every_row() {
        let widths = vec![18.0, 90.0, 70.0];
        let layout = ColumnLayout::new((0.0, 400.0), &widths);

        let shallow = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 17.0));
        let deep = egui::Rect::from_min_size(egui::pos2(0.0, 340.0), egui::vec2(400.0, 17.0));

        for i in 0..widths.len() {
            let a = layout.cell_rect(i, shallow).expect("cell");
            let b = layout.cell_rect(i, deep).expect("cell");
            assert_eq!(a.left(), b.left(), "column {i} drifted between rows");
            assert_eq!(a.width(), b.width(), "column {i} changed width per row");
        }
    }

    /// Columns are contiguous, in order, and end at the panel's right edge - no
    /// gaps to leak a wide label through and no overlap to paint over a neighbour.
    #[test]
    fn columns_tile_the_row_left_to_right() {
        let widths = vec![18.0, 90.0, 70.0];
        let layout = ColumnLayout::new((10.0, 410.0), &widths);
        let row = egui::Rect::from_min_size(egui::pos2(10.0, 0.0), egui::vec2(400.0, 17.0));

        let name = layout.name_rect(row);
        assert_eq!(name.left(), 10.0);
        // Name takes everything the data columns do not.
        assert_eq!(name.width(), 400.0 - 178.0);

        let mut x = name.right();
        for (i, w) in widths.iter().enumerate() {
            let r = layout.cell_rect(i, row).expect("cell");
            assert_eq!(r.left(), x, "column {i} does not start where {} ended", i);
            assert_eq!(r.width(), *w);
            x = r.right();
        }
        assert_eq!(x, 410.0, "the columns do not reach the right edge");
    }

    /// A panel too narrow for the columns keeps the name readable rather than
    /// letting it collapse to nothing (or go negative and invert the rects).
    #[test]
    fn a_narrow_panel_keeps_a_usable_name_column() {
        let layout = ColumnLayout::new((0.0, 100.0), &[18.0, 90.0, 70.0]);
        assert_eq!(layout.name.1, MIN_NAME_W);
        let row = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 17.0));
        for i in 0..3 {
            let r = layout.cell_rect(i, row).expect("cell");
            assert!(r.width() > 0.0, "column {i} inverted");
        }
    }

    /// Typing a new name and pressing Enter must emit exactly one
    /// [`OutlinerAction::Rename`] carrying the TYPED name.
    ///
    /// The committing Enter is still in the frame's input when `handle_keys` runs at
    /// the end of `show`, and the edit field has already surrendered focus by then -
    /// so the "begin a rename" branch used to fire on the very key that was meant to
    /// end one, overwriting the commit with a fresh begin. Nothing reached the
    /// caller and the field reopened with the old label.
    #[test]
    fn typing_a_name_and_pressing_enter_commits_it() {
        let ctx = egui::Context::default();
        let model = OutlinerModel {
            roots: vec![TreeNode::new(7, "old")],
            selection: vec![7],
        };
        let config = OutlinerConfig::default().with_editable_labels(true);

        // Pointer inside the panel, or keyboard handling is skipped entirely.
        let at = egui::pos2(40.0, 40.0);
        let mut collected: Vec<OutlinerAction> = Vec::new();

        // Not `mut`: the closure captures nothing by mutable reference — the
        // output vector is passed in per call.
        let frame = |events: Vec<egui::Event>, out: &mut Vec<OutlinerAction>| {
            let input = egui::RawInput {
                events: [vec![egui::Event::PointerMoved(at)], events].concat(),
                ..Default::default()
            };
            let mut frame = ctx.run_ui(input, |ui| {
                out.extend(show(ui, &model, &config));
            });
            // See `run_ui` above: epaint 0.36 refuses to drop an unapplied
            // texture delta quietly.
            frame.textures_delta.clear();
        };

        let key = |key: egui::Key, pressed: bool| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };

        frame(vec![], &mut collected); // settle: the pointer must be known to be inside
        frame(vec![key(egui::Key::Enter, true)], &mut collected); // begin the rename
        frame(vec![], &mut collected); // the field appears and takes focus
        frame(vec![egui::Event::Text("new".to_string())], &mut collected); // type
        frame(vec![key(egui::Key::Enter, true)], &mut collected); // commit

        let renames: Vec<_> = collected
            .iter()
            .filter_map(|a| match a {
                OutlinerAction::Rename { id, name } => Some((*id, name.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            renames,
            vec![(7, "oldnew".to_string())],
            "Enter did not commit the typed name (collected: {collected:?})"
        );
    }

    /// A fold made while a filter is active belongs to that filter session: the
    /// user's persisted fold survives it, and the session is dropped with the filter.
    ///
    /// The bug this pins: the filter forces every parent open, so a toggle wrote
    /// `!true == false` into the persisted flag and the branch came back collapsed
    /// once the filter was cleared, whatever the user had chosen before.
    #[test]
    fn folding_while_filtering_leaves_the_persisted_fold_alone() {
        let ctx = egui::Context::default();
        let model = OutlinerModel {
            roots: sample(),
            selection: vec![1],
        };
        let config = OutlinerConfig::default().with_filter(true);
        // Pointer inside the panel, or keyboard handling is skipped entirely.
        let at = egui::pos2(40.0, 60.0);
        // One frame with `query` in the filter box; returns node 1's persisted fold
        // and the filter-session map as they stand after `show`.
        let run = |query: &str, events: Vec<egui::Event>, expand_first: bool| {
            let input = egui::RawInput {
                events: [vec![egui::Event::PointerMoved(at)], events].concat(),
                ..Default::default()
            };
            let mut seen = (None, None);
            let mut out = ctx.run_ui(input, |ui| {
                let base = ui.id();
                let open_id = base.with(("egui_outliner_open", 1 as NodeId));
                if expand_first {
                    egui_widgets_config::state::set(ui.ctx(), open_id, true);
                }
                egui_widgets_config::state::set(ui.ctx(), base.with("__filter"), query.to_string());
                let actions = show(ui, &model, &config);
                assert!(actions.iter().all(|a| !matches!(a, OutlinerAction::Rename { .. })));
                seen = (
                    egui_widgets_config::state::get::<bool>(ui.ctx(), open_id),
                    egui_widgets_config::state::get::<HashMap<NodeId, bool>>(
                        ui.ctx(),
                        base.with("__filter_open"),
                    ),
                );
            });
            out.textures_delta.clear();
            seen
        };
        let left = egui::Event::Key {
            key: egui::Key::ArrowLeft,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };

        // The user expanded node 1.
        assert_eq!(run("", vec![], true).0, Some(true));
        // Filter for its grandchild; node 1 is shown (forced) open as an ancestor.
        run("leaf", vec![], false);
        // Collapse node 1 with the keyboard while filtering.
        let (persisted, session) = run("leaf", vec![left], false);
        assert_eq!(persisted, Some(true), "a fold made while filtering leaked into the persisted tree");
        assert_eq!(
            session.and_then(|m| m.get(&1).copied()),
            Some(false),
            "the fold is not kept for the filter session"
        );
        // Clearing the filter restores the user's tree and drops the session.
        let (persisted, session) = run("", vec![], false);
        assert_eq!(persisted, Some(true));
        assert_eq!(session, None, "the filter session outlived the filter");
    }

    /// A `fixed` column keeps its declared width even when a width was dragged and
    /// stored for it earlier - otherwise a swatch column narrower than `MIN_COL_W`
    /// could never return to its own size once touched.
    #[test]
    fn a_fixed_column_ignores_a_stored_dragged_width() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("fixed-col-test");
        let cols = vec![
            OutlinerColumn::new("swatch", "", 18.0).fixed(),
            OutlinerColumn::new("type", "Type", 88.0),
        ];
        for c in &cols {
            egui_widgets_config::state::set(&ctx, col_width_id(id, &c.id), 400.0_f32);
        }
        let widths = resolved_col_widths(&ctx, id, &cols);
        assert_eq!(widths[0], 18.0, "fixed column was resized");
        assert_eq!(widths[1], 400.0, "resizable column lost its dragged width");
    }

    /// With no columns declared the strip is zero-width, and that zero is the flag
    /// every row checks to take the original label-only path. This is the guard
    /// that keeps the existing consumers (colmap-rs, vfx-view) unchanged.
    #[test]
    fn no_columns_means_a_zero_width_strip() {
        let ctx = egui::Context::default();
        let widths = resolved_col_widths(&ctx, egui::Id::new("t"), &[]);
        assert!(widths.is_empty());
        assert_eq!(widths.iter().sum::<f32>(), 0.0);
        assert!(OutlinerConfig::default().columns.is_empty());
    }

    /// Untouched columns report exactly the width the caller declared.
    #[test]
    fn column_widths_fall_back_to_the_declared_width() {
        let ctx = egui::Context::default();
        let widths = resolved_col_widths(&ctx, egui::Id::new("t"), &columns());
        assert_eq!(widths, vec![18.0, 90.0, 70.0]);
    }

    /// A dragged width wins over the declared one and is CLAMPED, so a column can
    /// neither be dragged out of existence nor swallow the panel.
    #[test]
    fn a_dragged_width_overrides_and_is_clamped() {
        // Inside a frame: egui's memory only lives for the duration of a pass, so a
        // `set` outside one is silently dropped.
        frame(|ui| {
            let ctx = ui.ctx().clone();
            let base = egui::Id::new("t");
            let cols = columns();

            // NOTE the `f32` suffixes: the store is typed, and an unsuffixed float
            // literal is `f64`, which `get::<f32>` would simply not find.
            egui_widgets_config::state::set(&ctx, col_width_id(base, "type"), 140.0_f32);
            assert_eq!(resolved_col_widths(&ctx, base, &cols)[1], 140.0);

            egui_widgets_config::state::set(&ctx, col_width_id(base, "type"), 0.0_f32);
            assert_eq!(
                resolved_col_widths(&ctx, base, &cols)[1],
                MIN_COL_W,
                "a column dragged to nothing must survive at the floor width"
            );

            // The floor never touches a DECLARED width - the swatch column stays the
            // 18 pt the caller asked for.
            assert_eq!(resolved_col_widths(&ctx, base, &cols)[0], 18.0);
        });
    }

    /// Widths are keyed per PANEL INSTANCE: two outliners in one app must not
    /// resize each other's columns.
    #[test]
    fn column_widths_do_not_leak_between_instances() {
        frame(|ui| {
            let ctx = ui.ctx().clone();
            let cols = columns();
            egui_widgets_config::state::set(
                &ctx,
                col_width_id(egui::Id::new("a"), "type"),
                140.0_f32,
            );
            assert_eq!(
                resolved_col_widths(&ctx, egui::Id::new("a"), &cols)[1],
                140.0
            );
            assert_eq!(
                resolved_col_widths(&ctx, egui::Id::new("b"), &cols)[1],
                90.0,
                "resizing one outliner's column must not resize another's"
            );
        });
    }

    /// A node may carry FEWER cells than there are columns (or none at all) - the
    /// missing ones render blank. Adding a column must not force every existing
    /// node to be rebuilt, so this has to be a non-event rather than a panic.
    #[test]
    fn a_node_with_missing_cells_renders() {
        let model = OutlinerModel {
            roots: vec![
                TreeNode::new(1, "full").with_cells([
                    CellValue::Color(Color32::RED),
                    CellValue::Text("Composition".into()),
                    CellValue::Text("53.1 MB".into()),
                ]),
                // One cell for three columns.
                TreeNode::new(2, "short").with_cells([CellValue::Text("Folder".into())]),
                // No cells at all.
                TreeNode::new(3, "bare"),
            ],
            selection: vec![],
        };
        let cfg = OutlinerConfig::default()
            .with_columns(columns())
            .with_header(true, true);
        frame(|ui| {
            let actions = show(ui, &model, &cfg);
            // Nothing was clicked, so a well-behaved frame reports nothing.
            assert!(actions.is_empty(), "a passive frame emitted {actions:?}");
        });
    }

    /// The whole point of the reserved strip: a DEEPLY indented row must still
    /// leave the columns their full width, or cells would drift right as the tree
    /// gets deeper and stop lining up under their headers.
    #[test]
    fn deep_indent_does_not_eat_the_column_strip() {
        let deep = TreeNode::new(1, "a")
            .child(TreeNode::new(2, "b").child(TreeNode::new(3, "c").child(TreeNode::new(4, "d"))));
        let model = OutlinerModel {
            roots: vec![deep],
            selection: vec![],
        };
        let cfg = OutlinerConfig::default()
            .with_columns(columns())
            .with_header(true, false)
            .with_indent(24.0);
        frame(|ui| {
            let _ = show(ui, &model, &cfg);
        });
    }

    #[test]
    fn ancestors_of_deep_selection() {
        let roots = sample();
        // Select the deepest leaf (4): its strict ancestors are 2 and 1.
        let anc = ancestors_of_selected(&roots, &[4]);
        assert!(anc.contains(&1));
        assert!(anc.contains(&2));
        // Not the node itself, not its uncle, not the unrelated root.
        assert!(!anc.contains(&4));
        assert!(!anc.contains(&3));
        assert!(!anc.contains(&5));
    }

    #[test]
    fn ancestors_of_multiple_selection() {
        let roots = sample();
        // Selecting 3 and 4 marks 1 (ancestor of both) and 2 (ancestor of 4).
        let anc = ancestors_of_selected(&roots, &[3, 4]);
        assert!(anc.contains(&1));
        assert!(anc.contains(&2));
        assert!(!anc.contains(&3));
        assert!(!anc.contains(&4));
    }

    #[test]
    fn no_selection_has_no_ancestors() {
        let roots = sample();
        assert!(ancestors_of_selected(&roots, &[]).is_empty());
        // Selecting only a root makes nothing an ancestor (no descendant selected).
        assert!(ancestors_of_selected(&roots, &[5]).is_empty());
    }

    #[test]
    fn flatten_is_preorder() {
        let roots = sample();
        assert_eq!(flatten_ids(&roots), vec![1, 2, 4, 3, 5]);
    }

    #[test]
    fn filter_keeps_matches_and_ancestors() {
        let roots = sample();
        // "leaf" (id 4) matches: its ancestors 1 and 2 must survive, its uncle 3
        // and the unrelated root 5 must be pruned.
        let f = filter_roots(&roots, "leaf");
        assert_eq!(flatten_ids(&f), vec![1, 2, 4]);
    }

    #[test]
    fn filter_match_keeps_full_subtree() {
        let roots = sample();
        // Matching the parent "root" (id 1) keeps its whole subtree browsable.
        let f = filter_roots(&roots, "root");
        assert_eq!(flatten_ids(&f), vec![1, 2, 4, 3]);
    }

    #[test]
    fn filter_is_case_insensitive() {
        let roots = sample();
        // Caller lower-cases the query before calling; "OTHER" -> "other".
        let f = filter_roots(&roots, &"OTHER".to_lowercase());
        assert_eq!(flatten_ids(&f), vec![5]);
    }

    #[test]
    fn selection_membership() {
        let model = OutlinerModel {
            roots: sample(),
            selection: vec![2, 4],
        };
        assert!(model.is_selected(2));
        assert!(model.is_selected(4));
        assert!(!model.is_selected(1));
    }

    #[test]
    fn config_sentinels_resolve() {
        let c = OutlinerConfig::default();
        assert_eq!(c.auto_open(), 20);
        assert_eq!(c.depth_guard(), 512);
        let c = OutlinerConfig::default()
            .with_auto_open_below(5)
            .with_max_depth(64);
        assert_eq!(c.auto_open(), 5);
        assert_eq!(c.depth_guard(), 64);
    }

    #[test]
    fn builders_compose() {
        let n = TreeNode::new(7, "cam")
            .with_type("Camera")
            .with_icon("[K]")
            .with_open(true)
            .with_color(Color32::RED)
            .with_visible(false);
        assert_eq!(n.type_label.as_deref(), Some("Camera"));
        assert_eq!(n.icon.as_deref(), Some("[K]"));
        assert_eq!(n.default_open, Some(true));
        assert_eq!(n.color, Some(Color32::RED));
        assert_eq!(n.visible, Some(false));
    }

    #[test]
    fn visible_defaults_to_none() {
        // Existing callers that never touch visibility keep `None` (no eye drawn).
        assert_eq!(TreeNode::new(1, "x").visible, None);
    }

    #[test]
    fn config_opt_ins_default_off() {
        // New config flags must default off so existing call sites are unchanged.
        let c = OutlinerConfig::default();
        assert!(!c.editable_labels);
        assert!(!c.show_filter);
        assert!(!c.draggable);
        let c = OutlinerConfig::default()
            .with_editable_labels(true)
            .with_filter(true)
            .with_draggable(true);
        assert!(c.editable_labels);
        assert!(c.show_filter);
        assert!(c.draggable);
    }

    /// A `RowGeom` with a dummy rect — the reorder math is purely combinatorial.
    fn geom(id: NodeId, parent: Option<NodeId>, index: usize, num_children: usize) -> RowGeom {
        RowGeom {
            id,
            parent,
            index,
            num_children,
            depth: 0,
            rect: egui::Rect::ZERO,
        }
    }

    #[test]
    fn subtree_ids_includes_self_and_descendants() {
        let roots = sample();
        // Node 2's subtree is {2, 4}; node 5 is a lone leaf {5}.
        assert_eq!(subtree_ids(&roots, 2), HashSet::from([2, 4]));
        assert_eq!(subtree_ids(&roots, 5), HashSet::from([5]));
        // A missing id yields an empty (nothing-forbidden) set.
        assert!(subtree_ids(&roots, 999).is_empty());
    }

    #[test]
    fn adjust_for_removal_shifts_only_same_parent_forward() {
        // Same parent, dragged sits before the slot → slot shifts down by one.
        assert_eq!(adjust_for_removal(Some(1), 0, Some(1), 3), 2);
        // Same parent, dragged sits after the slot → no shift.
        assert_eq!(adjust_for_removal(Some(1), 4, Some(1), 3), 3);
        // Different parent → never shift.
        assert_eq!(adjust_for_removal(Some(2), 0, Some(1), 3), 3);
        // Root-to-root (None == None) forward → shift.
        assert_eq!(adjust_for_removal(None, 0, None, 2), 1);
    }

    #[test]
    fn move_reparent_appends_as_child() {
        // Drag root 5 INTO node 2 (which has 1 child) → appended at the end.
        let drag = geom(5, None, 1, 0);
        let target = geom(2, Some(1), 0, 1);
        let forbidden = HashSet::from([5]);
        assert_eq!(
            resolve_move(&drag, &target, DropZone::Into, &forbidden),
            Some((Some(2), 1))
        );
    }

    #[test]
    fn move_reparent_into_own_current_parent_dedups() {
        // Node 4 is already the only child of 2; dropping it INTO 2 again must
        // land it at index 0 (append AFTER removing it: 1 child - 1 = 0).
        let drag = geom(4, Some(2), 0, 0);
        let target = geom(2, Some(1), 0, 1);
        let forbidden = HashSet::from([4]);
        assert_eq!(
            resolve_move(&drag, &target, DropZone::Into, &forbidden),
            Some((Some(2), 0))
        );
    }

    #[test]
    fn move_reorder_before_and_after_other_parent() {
        // Drag root 5 relative to root 1 (index 0). 5 is at root index 1.
        let drag = geom(5, None, 1, 0);
        let target = geom(1, None, 0, 2);
        let forbidden = HashSet::from([5]);
        // Before 1: slot 0; 5 is after it in the same list so no shift → 0.
        assert_eq!(
            resolve_move(&drag, &target, DropZone::Before, &forbidden),
            Some((None, 0))
        );
        // After 1: slot 1; 5 sits at index 1 (not < 1) so no shift → 1.
        assert_eq!(
            resolve_move(&drag, &target, DropZone::After, &forbidden),
            Some((None, 1))
        );
    }

    #[test]
    fn move_same_parent_forward_compensates_shift() {
        // Roots 1(idx0) and 5(idx1). Drag 1 relative to 5, same (root) parent.
        let drag = geom(1, None, 0, 2);
        let target = geom(5, None, 1, 0);
        let forbidden = HashSet::from([1]);
        // Before 5: slot 1, 1 removed (0 < 1) → 0 (no visible move).
        assert_eq!(
            resolve_move(&drag, &target, DropZone::Before, &forbidden),
            Some((None, 0))
        );
        // After 5: slot 2, 1 removed (0 < 2) → 1 (1 ends up last).
        assert_eq!(
            resolve_move(&drag, &target, DropZone::After, &forbidden),
            Some((None, 1))
        );
    }

    #[test]
    fn move_rejects_self_and_descendant_drops() {
        let roots = sample();
        let forbidden = subtree_ids(&roots, 2); // {2, 4}
        let drag = geom(2, Some(1), 0, 1);
        // INTO itself → refused.
        let onto_self = geom(2, Some(1), 0, 1);
        assert_eq!(
            resolve_move(&drag, &onto_self, DropZone::Into, &forbidden),
            None
        );
        // INTO its own descendant 4 → refused.
        let desc = geom(4, Some(2), 0, 0);
        assert_eq!(resolve_move(&drag, &desc, DropZone::Into, &forbidden), None);
        // Reorder BEFORE a descendant (whose parent 2 is inside the subtree) →
        // refused (its new parent would be inside the moved subtree).
        assert_eq!(
            resolve_move(&drag, &desc, DropZone::Before, &forbidden),
            None
        );
        // Reordering among the roots (parent None) is always allowed.
        let root_target = geom(5, None, 1, 0);
        assert!(resolve_move(&drag, &root_target, DropZone::Before, &forbidden).is_some());
    }
}

// ============================================================================
// Tree builder helpers (generic scaffolding for a category / entity tree)
// ============================================================================

/// Small, app-agnostic helpers for building an [`OutlinerModel`]'s roots the way a
/// typical entity browser does: **category headers** (with an icon + a visibility
/// eye), **selectable leaves** (an entity row), and inert **info rows** ("… N more").
///
/// This is the reusable scaffolding harvested from colmap-rs's reconstruction
/// outliner (`panels/recon_tree.rs`), decoupled from any app data: the caller keeps
/// its OWN `HashMap<NodeId, Sel>` (to resolve [`OutlinerAction::Select`]) and
/// `HashMap<NodeId, Vis>` (to resolve [`OutlinerAction::ToggleVisible`]); these
/// builders assign each node a STABLE id from a string key via [`nid`] and record
/// the caller's payload under that id, so the build side and the action side can
/// never drift. Node ids are deterministic across frames (same key → same id).
///
/// ```
/// use std::collections::HashMap;
/// use egui_outliner::builder as b;
/// # #[derive(Clone)] enum Sel { Image(u32) }
/// # #[derive(Clone, Copy)] enum Layer { Frames }
/// let mut sel: HashMap<u64, Sel> = HashMap::new();
/// let mut vis: HashMap<u64, Layer> = HashMap::new();
/// let leaf = b::selectable(&mut sel, "img:1", Sel::Image(1), "#1: shot.jpg");
/// let cat  = b::vis_header(&mut vis, Layer::Frames, "cat:frames", "", "Frames (1)", vec![leaf], true, true);
/// assert_eq!(cat.id, b::nid("cat:frames"));
/// ```
pub mod builder {
    use super::{NodeId, TreeNode};
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};

    /// A STABLE [`NodeId`] from a string key: a deterministic hash seeded with fixed
    /// keys, so the same key hashes to the same id every frame (and run). Use the
    /// same key on the build side and when recording a node's payload so a node's
    /// persisted open-state / selection never drift.
    pub fn nid(key: &str) -> NodeId {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        h.finish()
    }

    /// A selectable leaf row: id from `key`, `label` shown; records `nid(key) -> sel`
    /// in the caller's selection map so [`super::OutlinerAction::Select`] resolves
    /// back to `sel`.
    pub fn selectable<Sel>(
        sel_map: &mut HashMap<NodeId, Sel>,
        key: &str,
        sel: Sel,
        label: impl Into<String>,
    ) -> TreeNode {
        let id = nid(key);
        sel_map.insert(id, sel);
        TreeNode::new(id, label)
    }

    /// A non-selectable category header (`icon` + `label`) holding `children`, with an
    /// initial `open` state. Not recorded in any map — clicking it only twirls.
    pub fn header(
        key: &str,
        icon: &str,
        label: impl Into<String>,
        children: Vec<TreeNode>,
        open: bool,
    ) -> TreeNode {
        TreeNode::new(nid(key), label)
            .with_icon(icon)
            .with_children(children)
            .with_open(open)
    }

    /// A category header that also carries a visibility eye (initial `visible`).
    /// Records `nid(key) -> vis` so [`super::OutlinerAction::ToggleVisible`] flips the
    /// caller's matching layer flag. Still non-selectable (label click is inert).
    #[allow(clippy::too_many_arguments)]
    pub fn vis_header<Vis>(
        vis_map: &mut HashMap<NodeId, Vis>,
        vis: Vis,
        key: &str,
        icon: &str,
        label: impl Into<String>,
        children: Vec<TreeNode>,
        open: bool,
        visible: bool,
    ) -> TreeNode {
        vis_map.insert(nid(key), vis);
        header(key, icon, label, children, open).with_visible(visible)
    }

    /// A visibility-only leaf (an eye, no children, no browse-selection) — for a layer
    /// that has no browsable entities but must stay toggleable. Records `nid(key) -> vis`.
    pub fn vis_leaf<Vis>(
        vis_map: &mut HashMap<NodeId, Vis>,
        vis: Vis,
        key: &str,
        icon: &str,
        label: impl Into<String>,
        visible: bool,
    ) -> TreeNode {
        let id = nid(key);
        vis_map.insert(id, vis);
        TreeNode::new(id, label)
            .with_icon(icon)
            .with_visible(visible)
    }

    /// A plain, inert info row (e.g. a trailing "… (N more)" summary): id from `key`,
    /// not in any map, so its `Select` / `ToggleVisible` are silently ignored.
    pub fn info(key: &str, label: impl Into<String>) -> TreeNode {
        TreeNode::new(nid(key), label)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn nid_is_stable_and_distinct() {
            assert_eq!(nid("cat:frames"), nid("cat:frames"));
            assert_ne!(nid("cat:frames"), nid("cat:cameras"));
        }

        #[test]
        fn builders_record_payloads_under_node_id() {
            let mut sel: HashMap<NodeId, u32> = HashMap::new();
            let mut vis: HashMap<NodeId, u8> = HashMap::new();
            let leaf = selectable(&mut sel, "img:7", 7u32, "#7");
            assert_eq!(leaf.id, nid("img:7"));
            assert_eq!(sel.get(&nid("img:7")), Some(&7));

            let cat = vis_header(&mut vis, 1u8, "cat:f", "", "Frames", vec![leaf], true, true);
            assert_eq!(cat.id, nid("cat:f"));
            assert_eq!(vis.get(&nid("cat:f")), Some(&1));
            // Header is NOT selectable (never recorded in the selection map).
            assert!(!sel.contains_key(&nid("cat:f")));
        }
    }
}
