# egui-outliner

Working notebook for this crate. Update it as the crate evolves.

> 2026-07: `OutlinerGlyphs::default()` now returns SHARED PHOSPHOR icons
> (`icons::CARET_DOWN`/`CARET_RIGHT`/`VISIBLE`/`HIDDEN`), not the old Unicode
> marks (▾▸●○). Host must register the phosphor font (already the set-wide
> convention); a font-free host can still override the `String`s.
>
> 2026-07: added a `builder` module — generic scaffolding (`nid`/`selectable`/
> `header`/`vis_header`/`vis_leaf`/`info`) for category+entity+visibility trees,
> harvested (decoupled) from colmap-rs's `panels/recon_tree.rs`. That colmap panel
> is an ADAPTER over THIS widget, not a separate outliner — so no 2nd crate; the
> reusable part is these builders.

## What it is
A **generic, app-agnostic collapsible hierarchy / outliner panel** for egui 0.34.
It draws a vertically-scrolling tree of recursive `egui::CollapsingHeader` rows
with selection (single / ctrl-toggle / shift-range), an **ancestor-of-selected**
dimmed highlight, a caller-defined right-click context menu, double-click to
activate, a depth guard against pathological graphs, optional expand/collapse
events, an optional colour legend, opt-in **per-row visibility eye toggles**,
opt-in **in-place rename** (F2 / double-click), an opt-in **filter box**, and
**keyboard navigation** (Up/Down select, Left/Right collapse/expand).

The widget is **stateless w.r.t. tree data**: the caller owns an `OutlinerModel`
(a forest of `TreeNode` + a `selection` id list) and passes it in read-only; the
widget returns a `Vec<OutlinerAction>` describing what the user did. No app types
leak in. Colours/fonts/metrics come from `egui-widgets-config` tokens.

## Gathered from
Ported from `vfx-rs/crates/view/vfx-view/src/panels/prim_tree.rs` (the USD prim
tree) and FULLY decoupled from USD/pxr:
- `usd_sdf::Path` ids → plain `u64` (`NodeId`); callers hash/truncate their UUID.
- `usd_core::Prim` recursion → an owned `Vec<TreeNode>` forest.
- `PrimTreeAction` event sink → returned `Vec<OutlinerAction>`.
- USD glue DROPPED: variant-set combos, draw-mode combo, Imageable visibility,
  payload load/unload, model-path / camera entries. The host supplies its own
  right-click menu via `OutlinerConfig::context_items`; the widget is generic and
  knows nothing about "Activate" / "Load payload".
- Kept generic: recursive `CollapsingHeader` rendering, the `>512` depth guard
  (`… (too deep)`), the selected / ancestor-of-selected / default colour scheme,
  click→Select with ctrl/shift modifier decode, double-click→Activate, the
  vertical `ScrollArea`, and the colour legend.

## API (src/egui_outliner.rs — single file)
- `pub type NodeId = u64;`
- `TreeNode { id, label, type_label: Option<String>, children: Vec<TreeNode>,
  default_open: Option<bool>, icon: Option<String>, color: Option<Color32>,
  visible: Option<bool> }`
  + `new` + builders (`with_type`, `with_children`, `child`, `with_open`,
  `with_icon`, `with_color`, `with_visible`). `visible = Some(_)` draws a leading
  eye toggle (● visible / ○ hidden); `None` (default) draws nothing.
- `OutlinerModel { roots: Vec<TreeNode>, selection: Vec<NodeId> }`
  + `new`, `with_roots`, `is_selected(id)`.
- `ContextItem { id: String, label: String, separator_before: bool }`
  + `new`, `with_separator`.
- `OutlinerConfig { indent, row_height, context_items, auto_open_below, max_depth,
  show_legend, emit_toggle, editable_labels, show_filter }` + builders
  (`with_editable_labels`, `with_filter`). `0.0`/`0` numeric fields fall back to
  tokens / vfx-view defaults (`auto_open_below`→20, `max_depth`→512). The two new
  flags default **false** so existing call sites are unchanged.
- `OutlinerAction::{ Select, Activate, Context, ToggleOpen, ToggleVisible, Rename,
  Move, Sort, CellClicked, DragStarted, ClearSelection, SelectMany }`.
  Empty-matte click clears; rubber-band reports the full selection live while
  dragging (Ctrl/Shift unions with the drag-start set, not the live one).
- `pub fn show(ui, &OutlinerModel, &OutlinerConfig) -> Vec<OutlinerAction>`.
- Pure helpers (egui-free, unit-tested): `ancestors_of_selected(roots, sel)
  -> HashSet<NodeId>`, `flatten_ids(roots) -> Vec<NodeId>`.

## How highlight + selection work
- **ancestor-of-selected**: `ancestors_of_selected` walks the forest once; a node
  is marked when any *strict descendant* is in `selection` (the tree equivalent
  of vfx-view's `has_prefix && sel != path`). Returned as a `HashSet` so the
  per-row lookup is O(1).
- Row colour precedence: selected (`semantic.accent`) > ancestor
  (`accent.gamma_multiply(0.45)` — same hue, dimmed) > `TreeNode::color` override
  > theme `text_color()`.
- **modifiers**: `show` reads `ui.input(|i| i.modifiers)` per click →
  `additive = ctrl||cmd`, `range = shift`. The widget does NOT own the selection
  — it only reports intent; the caller resolves the actual range using
  `flatten_ids` for visual order.

## Tokens used (egui-widgets-config)
- `WidgetsConfig::get(ui)` → `fonts.md` (header font size via `prop`).
- `semantic(ui)` → `accent` (selected + dimmed ancestor) and `weak` (the dimmed
  `[type]` suffix, legend heading, empty-state label).
- Default row text = live `ui.visuals().text_color()`.

## Deps
- Runtime: `egui = "0.34"` + `egui-widgets-config` (path). NO usd_*/pxr, NO serde
  (Color32 serde would pull egui's serde feature; not needed — the host owns
  persistence).

## Row layout & interaction (since the visibility/rename/nav pass)
Rows are now laid out by hand as `[indent][disclosure ▸/▾][eye?][label]` (egui's
`CollapsingHeader` owns the whole row and can't host a leading widget). Clicking
the disclosure toggles open; clicking the label Selects; double-click Activates
(or begins rename when `editable_labels`); the eye toggle emits `ToggleVisible`
and consumes its own click (never Selects). Widget-local UI state (open flags,
filter text, rename buffer) lives in egui memory keyed off `ui.id()` via
`egui_widgets_config::state` (transient) — the model stays read-only.

## Gotchas
- Open state is stored in egui temp data under `ui.id().with(("egui_outliner_open",
  node.id))` — keyed off the caller `NodeId` (not the label) so renaming a node
  doesn't reset its expansion, and scoped per instance so two outliners don't
  share it. Transient (not persisted across restart); the `default_open` /
  auto-open heuristic re-seeds it each session.
- `ToggleOpen` is emitted directly when the user toggles (disclosure click or
  Left/Right key), only when `emit_toggle` is set.
- The filter builds a filtered CLONE of the forest (matches + ancestors; a
  matched node keeps its full subtree). While filtering, parents are force-open.
- Folds are read and written ONLY through `RenderCtx::is_open` / `set_open`
  (disclosure click and Left/Right keys). While filtering they go to a
  filter-session map (`base_id.with("__filter_open")`, open by default), cleared
  when the filter empties, so a fold made while filtering never overwrites the
  persisted flag (it used to: the forced `true` was toggled to `false` and the
  branch came back collapsed). Test: `folding_while_filtering_leaves_the_persisted_fold_alone`.
- `OutlinerGlyphs::default()` is the shared phosphor set (`icons::CARET_DOWN`/
  `CARET_RIGHT`/`VISIBLE`/`HIDDEN`); only a font-free host needs `with_glyphs`.
- Keyboard nav only runs while the pointer is over the panel and no text field
  wants the keyboard (`ui.ctx().egui_wants_keyboard_input()`), so it never steals
  global shortcuts or the filter/rename input.
- The dimmed `[type]` suffix needs a per-run colour, so the header text is a
  `LayoutJob` (icon + label in row colour, suffix in weak) rather than a plain
  `RichText`.

## Verify
From repo root: `cargo test -p egui-outliner` (unit + doctest), `cargo clippy -p egui-outliner --all-targets -- -D warnings`.
Marquee hit tests: `marquee_hits_rows_the_band_covers`, `marquee_misses_when_the_band_is_beside_the_rows`.

## TODO
- [ ] `// TODO(outliner): drag reparent` — drag-to-reorder / reparent not done in
  this pass.
- [ ] Optional `serde` feature if tree view-state ever needs persisting.
