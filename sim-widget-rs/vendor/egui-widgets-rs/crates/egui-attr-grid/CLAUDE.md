# CLAUDE.md — `crates/egui-attr-grid`

> Working notebook for this subdir. Keep current: layout, gotchas, TODOs.

## What / why
THE attribute editor (one widget set; `egui-attr`, `egui-attr-table`, `egui-vector` were folded
into it on 2026-10-06, see root CHANGELOG for the migration table). The host builds a flat
`Vec<AttrField>` from its model every frame, calls `render_grid_with_config`, applies the
returned `(key, AttrValue)` edits. Default look = WarpBro's: compact `AttrMetrics`, SlimSlider rows.

## Layout
- `src/egui_attr_grid.rs`: `AttrValue`/`AttrField`, grid + config + hooks, value editors, tests
  (`tests` = grid geometry, `row_tests` = reset / paste / randomise / segmented / slider fill / AccessKit).
- `src/actions.rs`: reset, typed clipboard (egui temp data, one slot), paste, randomise, and the
  label context-menu entries. Plain functions over the field; the menu and tests share them.
- `src/random.rs`: `Pcg` + `Random` policy (opt-in per field: only fields with `random` get the entry).
- `segmented` strip and `AttrMetrics` live in `egui-widgets-config` (re-exported here; `grid_config(metrics)` builds the
  `AttrGridConfig`). The grid edits a string row with the `segmented` token through `segmented_pick` (index-based, borrowed
  labels); a strip whose labels do not fit becomes a dropdown, never truncated text.
- `src/preset.rs`: named presets over rows.

## Rules and traps
- Sliders: `range_slider` fills the cell (`SlimSlider::fill`), box = the grid-wide value-box width: the widest `egui_slider::field_text` of any ranged row's span min / max (the
  current value only when outside a soft span; decimals from the step hint else the span via `field_decimals`, shared with the
  slider that draws them), clamped to `[numeric_width, 3x]`, cached in `AttrGridState::box_width`. One grid per section: a host
  measures ALL sections with `value_box_width` and passes `AttrGridConfig::value_box_width`;
  height = `row_height`; Ctrl/Cmd-click resets to `AttrField::default`. No size constants here.
- Row menu order: host `hooks.context_menu` entries, a separator only if the host added any, then
  Reset / Copy / Paste / Randomise. Inactive rows can copy only.
- Paste accepts the same `AttrValue` variant (lists: same length + kinds); a string row with choices
  accepts only a listed choice; hard-range scalars are clamped on reset / paste (`soft` is not).
- The label cell id is an owned `egui::Id` (the label closure borrows the field mutably for the menu).
- `fixed_numeric_editor` takes every Vec3 / Vec4 that is not a `color` row, so the colour arms in
  `render_value_editor_ex` need no guard and there is no Vec3 / Vec4 fallback.
- Labels: the table draws column 0 for ALL rows before column 1, so label ids live in a per-row
  `AttrGridState::labels` vec (a single "last label" slot named only the last row's editor).
- The uniform lock scales from `UniformLock.base`; `last` tells its own writes from outside changes.
- Numeric rows (scalar, Vec3 / Vec4, list, colour) end at the editor cell's right edge like slider boxes:
  `ValueEditorLayout::spread_cells`, `egui_colorpicker::color_edit_flush`. A `uniform` row's lock
  takes the last `icon_width`, so that row ends one icon earlier. Hosts' own numeric rows use `ValueEditorLayout::spread_cells` too.
- Value-action buttons live in the actions gutter AFTER the host's `action_width` area
  (`AttrGridConfig::action_cells`); the grid's `value_cells` already includes their width.

- ONE switch: `AttrGridConfig::actions` drives the gutter buttons AND the label menu (default none).
  A row whose editor the host drew (`hooks.editor` Some) offers only Copy; the menu learns that from
  the PREVIOUS frame (`AttrGridState::overridden`), the buttons from the same frame. `hooks.accepts`
  is the host's veto (`actions::run` rolls a refused value back).
- `render_value_editor(.., &EditorCtx)` is the one editor entry; `egui-prefs2` calls it too, so a
  Preferences row IS an attribute row. Colour rows: `egui_colorpicker::color_edit` (`ColorRow`).
- The grid never sizes from an `egui::Grid` cell's available width (it feeds back); prefs2 passes ONE
  editor width per pane.

## TODO
- [ ] Keyboard (arrow keys) on SlimSlider and AccessKit `SetValue` / increment actions.
