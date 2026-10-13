# egui-asset-browser

Working notebook for this crate. Update it as the crate evolves.

## What it is
A **generic, app-agnostic media / asset browser panel** for egui 0.34. It draws
a scrollable list of asset rows grouped by kind, with selection, hover, a
right-click "Create" context menu, double-click-to-open, a per-row delete
control, optional in-place rename, and drag-out start reporting.

The widget is **stateless w.r.t. asset data**: the caller owns an
`AssetBrowserModel` and passes it in read-only; the widget returns a
`Vec<AssetAction>` describing what the user did. The caller applies those actions
to its own model. No app types leak in.

## Gathered from
Ported from `playa/crates/playa-ui/src/widgets/project/project_ui.rs` (the media
pool / project panel) and decoupled from the playa engine:
- `uuid::Uuid` ids → plain `u64`.
- `playa_engine::{Project, Player, Node}` → flat `AssetBrowserModel` + `AssetItem`.
- `ProjectActions` event sink → returned `Vec<AssetAction>`.
- `rfd` file dialogs → NOT run here; reported as `AddMedia` / `Open` so the
  caller opens its own dialog.
- Hard-coded engine icon table (File/Comp/Camera/Text/AI/Ref) → caller-supplied
  `KindStyle` map keyed by the free-form `kind` string.

## Model (src/model.rs)
- `AssetItem { id: u64, name: String, kind: String, thumbnail: Option<TextureId>, subtitle: Option<String> }`
  - `kind` is a free-form group label; drives grouping + per-kind style.
  - Builder helpers: `AssetItem::new`, `.with_subtitle`, `.with_thumbnail`.
- `AssetBrowserModel { items: Vec<AssetItem>, selection: Vec<u64> }` — the flat,
  caller-owned state. `is_selected(id)` helper.
- `KindStyle { icon: String, color: Color32 }` — per-kind icon + accent colour.

## Actions (src/action.rs)
`AssetAction` enum (all ids are `AssetItem::id`):
- `Open { id }` — double-click. Preceded by a `Select`.
- `Select { id, additive, range }` — click; `additive`=ctrl/cmd toggle,
  `range`=shift range-extend. Caller implements its own selection semantics.
- `ClearSelection` — click on empty space.
- `Create { kind }` — a "Create" entry chosen (`kind` = the caller label).
- `Delete { id }` — delete box / context "Delete".
- `Rename { id, new_name }` — committed in-place rename (only if enabled).
- `AddMedia` — "Add media" button (caller runs the file dialog).
- `BeginDrag { id }` — drag-out gesture started; pair with sibling
  `egui-drag-overlay` to paint a ghost (NOT a dependency).

## Config + entry point (src/egui_asset_browser.rs)
- `AssetBrowserConfig { create_kinds, kind_styles, show_add_media, allow_rename, group_by_kind }`
  with builders `.with_create_kinds`, `.with_kind_style`, `.with_add_media`,
  `.with_rename`, `.with_grouping`.
- `pub fn show(ui, &AssetBrowserModel, &AssetBrowserConfig) -> Vec<AssetAction>`.

## Deps
- Runtime: `egui = "0.34"` only. NO uuid, NO rfd, NO playa, NO serde
  (serde would only be needed if view-state persistence were added — it is not).

## Gotchas
- The panel-rect interaction is reserved BEFORE the rows so per-row click targets
  (allocated on top) win click precedence — matches egui's smaller-widget-first
  rule, same trick as playa.
- The delete box is its own `interact` target so clicking it does not also
  trigger row selection.
- Selection state is NOT held here — the widget only reports modifier intent in
  `Select`. The shift-range anchor logic from playa was intentionally dropped:
  the widget doesn't own the selection, so the caller (which does) resolves the
  range. The `range` flag tells it to.
- Two bits of egui *ephemeral* data are used: the id of the row being renamed and
  its text buffer (keyed off `ui.id()`); both are cleared on commit/cancel.
- egui 0.34.3 (the resolved patch) bumped its own MSRV to 1.92; our declared
  `rust-version = "1.88"` is this crate's source-level minimum. If `cargo` here
  refuses to build, it's the egui patch MSRV, not our code.

## Dropped / generalized features (vs playa)
- Save/Load project buttons, +Folder, AINode "Generate" entry, the active-comp
  green stripe, and the fps/frame badge formatting — all engine-specific. The
  badge slot survives as the generic `subtitle` field; the active stripe and the
  AI generate action are caller concerns now.
- Double-click on empty space opening a file dialog → dropped (no dialogs here);
  use the `AddMedia` button instead.
- `SSE`/preview-comp "is_listed" filtering — caller decides what goes in `items`.

## Verify
From this dir: `cargo check --message-format=short` and
`cargo clippy -- -D warnings` → both clean (0 errors, 0 warnings).
```

## TODO
- [ ] Optional grid/thumbnail view mode (currently list-only; `thumbnail` is
      already rendered per-row).
- [ ] Optional `serde` feature if view-state (scroll, group collapse) ever needs
      persisting.
