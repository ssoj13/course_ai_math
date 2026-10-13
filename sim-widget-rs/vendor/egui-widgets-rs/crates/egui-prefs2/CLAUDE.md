# egui-prefs2 — the vfx-view "Settings" two-pane panel, reusable

Local working notes. Keep current — append findings, prune stale.
Author convention: Alex Joss <joss13@gmail.com>.

## What / why

A drop-in **Settings / Preferences panel** with the vfx-view look: a fixed 150px icon+label
category **sidebar** (accent bar on the selected row) and a scrolling **content pane** (big-icon
category header → sectioned body → right-aligned "Reset to Defaults" footer). Drops into any egui
container (a dock tab or a window). Look, spacing, and font sizes are copied verbatim from
vfx-view `panels/preferences.rs` so adopting apps match pixel-for-pixel. Lib root
`src/egui_prefs2.rs`.

Distinct from `egui-prefs` (the tree-view **modal** `PrefsWindow` + Apply/OK/Cancel over a cloned
working copy). egui-prefs2 is the flat two-pane look; the two now share their sidebar/section
primitives (see "Sync" below), so they read as ONE design language.

## Two APIs, one look

- **v0.1 closure-driven** — `draw(ui, &mut PrefsPanelState, &[Category], body: FnMut(ui, idx),
  reset: Option<FnMut()>)`. Host writes a `|ui, idx| match idx { … }` body using the exported
  primitives `section_header` / `section_grid` / `category_header` / `sidebar_item`.
- **v0.2 data-driven** (`schema` module) — the host declares prefs as DATA:
  `PrefsSchema { categories → sections → FieldSpec { path, label, widget, default } }` (all serde,
  so RON/JSON-loadable). `draw_schema(ui, state, &schema, &mut RustBox, &mut CustomRenderers)`
  renders it and reads/writes each field's value in a **box-rs** `RustBox` addressed by `path`.
  Schema in → filled box-rs out (persist via `RustBox::to_json_value`).

`Widget` kinds: `Bool | Int{min,max,step} | Float{min,max,step,logarithmic,suffix} | Drag |
Text{multiline} | Color | Enum{variants} | Vec3 | Custom(id)`. Ergonomic ctors:
`W::int/float/float_unit/float_log/drag/enum_of/text/multiline/vec3/custom`.

## Design boundary (why box-rs)

- **Schema** (presentation contract: order, widget kind, ranges, enum variants, labels) lives HERE
  and is serde. **Values** live in box-rs — the project's nested value currency (typed path
  get/set + `Color`/`Vec3`, serde), so a filled panel serializes for persistence for free and the
  same values flow into GTO sessions / other tools with no translation layer.
- box-rs stays a PURE data container — it carries no UI hints. Bespoke widgets that don't fit a
  `Widget` kind use `Widget::Custom(id)`, rendered by a host closure in `CustomRenderers` (a
  `HashMap<String, Box<dyn FnMut(&mut Ui, &mut RustBox)>>`). box-rs required **zero changes** for
  this layer (its path API + `Color`/`Vec3` already cover everything).

## Font sync (the one non-obvious mechanic)

`draw` forces the content pane's `Body` + `Button` text styles to `PrefsFonts::body` (11.0) via
`sync_body_font`, so section headers / grid labels / sliders / combos all render at one size and
match the sidebar — independent of the host's (usually larger) global Body. Sidebar rows use
`PrefsFonts::sidebar` (11.0); the category header uses `heading` (13.5). vfx-view gets this free
because its app-wide Body is already small; as a standalone crate we must set it or the content
dwarfs the sidebar.

## Colours

`Widget::Color` renders via `egui_colorpicker::color_button_srgba` (the suite's own picker), NOT
egui's built-in `color_edit_button_srgba`, so every colour control across egui-widgets-rs shares
one look + behaviour. box-rs `Color` is `[f64;4]` sRGB 0..1; `color_from`/`color_to` bridge to
`Color32`.

## Deps

`egui`, `serde`, `box-rs` (ssh git-ref `branch=main`, package `box-rs` — standalone serde-only
repo), `egui-colorpicker` (path). box-rs is consumed by git-ref (NO dev `[patch]` — push-to-main +
git-ref), and `RustBox`/`BoxValue` are re-exported so hosts don't need a direct box-rs dep.

## Gotchas

- `PrefsPanelState` is serde (selected category + fonts) — persist it with the host config so the
  panel reopens where the user left it.
- `default_values()` seeds a fresh `RustBox` from the schema (explicit `FieldSpec::default` or
  widget-derived); it's also the Reset target, so every schema `path` exists → later writes always
  hit an existing slot. A write failure is a `debug_assert` (schema/values drift, a programmer
  error), never a silently-swallowed `Err`.

## Consumers

- `egui-widgets-demo` → the **egui-prefs2** page IS `vfx_settings_schema()` (the vfx-view Settings
  reference reproduced as data; 4 bespoke categories via `Custom`), and the **Settings** page is
  `app_settings_schema()` (box-rs SSOT for the demo's own fonts + WidgetsConfig tokens, applied to
  egui each frame).
- `egui-prefs` re-exports egui-prefs2's `sidebar_item`/`section_header`/`Category` and its modal
  uses `sidebar_item` + `category_header` → shared visual SSOT.
