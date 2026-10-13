# egui-arbor

Working notebook for this crate. Update it as the crate evolves.

> 2026-08-14: vendored kyjohnso/egui-arbor 0.2.1 into this workspace and pinned
> to egui 0.36. **Not** `egui-outliner`. Do not merge, rename-into, or share
> types with that crate — playa's Project panel owns `egui-outliner`.

## What it is
Blender-style hierarchical tree for egui. Trait-based: the host implements
`OutlinerNode` on its own node type and `OutlinerActions` for clicks / rename /
visibility / lock / drop. The widget borrows `&[N]` for one frame and reports
via `OutlinerResponse`. Persistent expand/edit/selection lives in egui memory
(`OutlinerState`).

## Why it is a separate crate
`egui-outliner` is our AE/USD-style column tree (`OutlinerModel` + `Vec<Action>`,
phosphor glyphs, filter, legend). Arbor is a different contract: owned children
on the node (`children_mut`), drag-drop Before/After/Inside, action icons,
box-select. Folding one into the other would break both hosts.

## Provenance
Copied from https://github.com/kyjohnso/egui-arbor @ 0.2.1 (commit 68d1ab5).
Original license **MIT OR Apache-2.0** (kept on this crate; do not inherit the
workspace BSD-3). Upstream `Cargo.toml` still declared `egui = "0.31"`; the
code already used 0.32-era `StrokeKind`. Bevy example + `bevy_egui` were left
behind — they pin egui 0.31 and do not belong in this workspace.

## API (keep arbor names)
- `Outliner::new(id).show(ui, nodes, &mut actions) -> OutlinerResponse`
- `OutlinerNode` / `OutlinerActions` / `TreeOperations`
- `DefaultActions<Id>` + `event_log`
- `Style`, `DragDropVisuals`, optional `serde` on `OutlinerState`

## egui 0.36
Workspace pin. Known 0.36 traps that bit other crates: `Context::run` is gone;
panel `show` takes `&mut Ui` when inside `App::ui`. Arbor only uses `Ui` +
`Painter` + `ctx.data_mut` / `pointer_*` — no `Context::run`.

## Used by
Intended consumer: playa AIview version tree (iterate / `parent_gen_uuid`),
via ssh git-ref on `egui-widgets-rs` `main`. Not wired yet.
