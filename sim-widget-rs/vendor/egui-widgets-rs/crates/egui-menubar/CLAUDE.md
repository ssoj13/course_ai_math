# egui-menubar — working notes (keep updated)

## What this is
A declarative menu bar / context menu for egui. The menu is **data**: a `&'static [MenuNode]`
tree whose leaves name actions by an opaque `&'static str` id. The crate draws the tree, asks the
host how each item should look, and returns the clicked id. It never learns what an action does.

## Why id-driven instead of closures in the tree
A menu built inline grows its own private list of what the app can do, and that list drifts from
the keyboard shortcuts — the classic "the menu says Ctrl+S but Ctrl+S does something else", or an
action that exists on the keyboard and nowhere in the UI. Naming actions by id forces the host to
keep ONE registry that the menu, the hotkey table and any command palette all read from.

playa is the reference consumer: `playa-ui/src/actions.rs` holds `ACTIONS` (id → label, scope,
hotkey, event factory) and builds BOTH the menu and `HotkeyHandler` from it.

## API surface (small on purpose)
- `MenuNode::{Sub(label, children), Item(id), Sep, Dyn(id)}` — that is the whole vocabulary.
  `Dyn(id)` (2026-09-29, first host: exr-view) is a slot the HOST draws, in place, through
  `MenuBar::show_full(ui, state, dynamic, extra)`'s `dynamic(id, ui)`: a recent-files list, a
  slider, a radio group - things that are not one action. `show`/`show_with_extra`/`show_context`
  draw an unfilled slot as a disabled `?id` row (visible, never silently dropped).
  **Adding the variant broke exhaustive `match MenuNode` in hosts**: playa's
  `playa-ui/src/commands.rs` walk needs a `MenuNode::Dyn(_) => {}` arm when it next updates.
- `MenuBar::new(&MENU).show(ui, |id| ItemState { .. }) -> Option<&'static str>` — bar form.
- `MenuBar::show_context(..)` — same tree inside `response.context_menu(..)`, so a right-click
  menu cannot disagree with the bar about labels or availability.
- `ItemState { label, shortcut, enabled, checked }` with `from_label(..)` + builders.

## Load-bearing details
- **Everything mutable lives in `ItemState`**, resolved per draw. That is what lets a `&'static`
  tree describe a menu whose items grey out, toggle and re-label at runtime. Do not add state
  fields to `MenuNode` — a static tree that has to be rebuilt is no longer static.
- **`ItemState::default()` has `enabled: true`.** A host that forgets to fill it in gets a usable
  menu, not a dead one. Guard test: `item_state_defaults_to_enabled`.
- **`state` is called only for VISIBLE items** (inside an open submenu), so it may query real app
  state without costing anything while the menus are closed.
- **Separator hygiene is automatic** in `draw_children`: a `Sep` only emits once something has
  been drawn since the last one, and a trailing `Sep` is dropped. Hosts can concatenate optional
  groups without hand-managing the rules.
- **`shortcut` is text only** — this crate never binds keys. Feed it from the same registry the
  hotkey table is built from, or the menu will confidently print a lie.
- A bare `Item` at bar level draws as a button beside the menus (one-click action), not a menu.

## For hosts: verify the tree against your registry
The tree is walkable (see `ids()` in the tests). Assert in BOTH directions: every `Item` id names
a real action, and — if you want no unreachable actions — every menu-worthy action appears in the
tree. playa learned this the hard way with timeline lanes: three attributes shipped implemented,
honoured by the engine, and reachable from nowhere in the UI.
