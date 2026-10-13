# egui-hotkeys - local notebook

> Keep current: decisions, traps, TODO. Root conventions: `../../CLAUDE.md`.

## What it is
Panel-scoped keyboard routing for egui apps, generic over the app's scope and command types.
Ported from warpbro-rs `src/hotkeys.rs` (routing, lines 186-355, and its tests); the
warpbro-specific tables, slot-click helpers and `Scope`/`Command` enums stay in warpbro.
Hermetic: egui only. Lib root `src/egui_hotkeys.rs`.

## Why
Bare letters, arrows and Space mean different things in different panels (the viewport
frames the image, the channel tree folds with arrows, a text field types). One routing rule
for every host instead of per-app `if hovered { .. }` ladders. The model is the one of
Blender / Houdini / Nuke / Qt:
1. **Text input or a modal first.** While `typing`, only Ctrl/Cmd chords are looked up; the rest
   stays with the widget (Qt's ShortcutOverride).
2. **Context chain, most specific first.** Active panel (under the pointer, else the last one
   clicked while it still draws, else `Keymap::fallback`), then its `Keymap::parent`s, then the
   global table. The FIRST table with a binding for the chord wins, so a panel may OVERRIDE a
   global key (e.g. Timeline F = fit the timeline while global F = fit the image).
3. **Conflicts.** Overlapping chords inside ONE table are a bug (`conflicts` finds them; hosts
   assert it in a test). A chord shared by a scope and an ancestor is an override, not a conflict.

How warpbro-rs maps onto it (its code is untouched): its `Scope::Global` table is `global()`; every
other `Scope` is a flat panel (`parent` = None); today's "global reserves chords first" becomes
"panel tables override global", so warpbro must not list a chord in both unless it means the
override (its own disjointness test becomes `conflicts` plus an explicit list of overrides).

## API (all free functions generic over `K: Keymap`)
- `Keymap` (implement on a marker type): `type Scope` (`Copy+Eq+Send+Sync+'static`, one per
  panel), `type Cmd` (`Copy+Eq`), `global()`, `bindings(scope)`, `parent(scope)` (default
  `None` = the global table follows directly; at most `MAX_DEPTH` levels), `fallback()` (default `None`).
- `Binding::{new, physical, additive}` + `.repeats()` - `const fn` constructors, so tables are `const`:
  `new` = exact chord; `physical` = match the key POSITION (digits); `additive` = exact chord
  that ignores Shift (property filters; Ctrl+Plus, since "+" is Shift+= on most layouts).
  `repeats()` also accepts the OS auto-repeat events of a held key (default: one press, one fire).
  `Binding::{is_chord, matches, overlaps}` are public.
- `register(km, ui, scope, rect)` - every panel, every frame, even panels without keys.
- `register_at(km, ctx, layer, scope, rect)` - the same for an overlay of a panel that is drawn
  on its OWN layer (a floating strip is an `egui::Area`): pass that layer, or the pointer over
  the overlay owns nothing. Both share one implementation (`put`).
- `active(km, ctx)` - who owns the keys now.
- `resolve(km, scope, key, physical_key, modifiers, repeat)` -> `(Route::{Global, Scope(s)}, cmd)`:
  walks the chain, pure. A nearer binding WITHOUT `repeats()` shadows a farther repeating one:
  the repeat resolves to nothing (stays for the widgets).
- `conflicts(km, &all_scopes)` -> `Vec<Conflict>`: chord overlaps inside the global table and each
  scope table. Host test: `assert!(conflicts(&km, &Scope::ALL).is_empty())`.
- `pressed(km, ctx, typing, enabled)` -> `Vec<Cmd>` - the whole frame's commands for the
  active chain; events consumed. `consume(km, ctx, route, cmd, typing)` - one command, one
  event, only when the chain resolves that event to exactly `(route, cmd)`. **Both are `take`**,
  which calls `resolve`: one matching codepath. A command `enabled` refuses is not consumed and
  does not fall through to an ancestor either.
- `take_copy`, `take_paste` - the platform Copy / Paste events, left alone in text editors.

## How it works / why it is shaped so
- **Chain, not priority sorting**: the order is structural (scope, parents, global), so no
  table order or "most modifiers first" rule is needed between tables; inside one table the
  first match wins, which is why `conflicts` exists.
- **Exact chords** (`Modifiers::matches_exact`): egui's logical matching lets a Shift binding
  fire without Shift (plain 3 = "set mark 3") and makes Insert vs Shift+Insert depend on table
  order. No "most modifiers first" sorting is needed.
- **Physical keys**: Shift+1 types "!" (US) or a character egui has no `Key` for; the event's
  `physical_key` is always the 1 key. Falls back to the logical key when the platform gives none.
- **Previous-frame geometry**: keys are read at the top of the frame, before any panel is
  drawn, so who owns them is decided from the rectangles registered LAST frame (kept one frame).
  Draw order cannot change the answer, and hiding panels does not lose the keys. Regions are
  clipped to the Ui's clip rect and tested with egui's layer hit test, so a window or popup
  over a panel hides it. A pointer over a panel's rectangle that the layer test rejects (an
  unregistered overlay) owns NOTHING: keys go nowhere, not to the panel underneath.
- **Owner**: a press inside a registered panel makes it the retained owner; a press outside
  every panel drops it at once (same frame, `active()` is `None` while a button is pressed).
  The owner is valid only while its scope registered a region last frame: a closed tab or
  hidden panels fall back to `Keymap::fallback` and the stale owner is forgotten (it does not
  return when the panel reopens).
- **Exactly one event per consume**. **Auto-repeat** (`repeat: true`) events count only for a
  `.repeats()` binding (steps, zoom); a plain binding ignores them and the event stays for the
  widgets, so a held toggle does not flicker. The flag is checked in `Binding::matches`, i.e. in
  the one `resolve` path.
- **Namespacing**: temp-data ids hash the keymap TYPE (`TypeId::of::<K>()`), so two keymaps
  in one app do not share regions or owner (tested).
- **`Keymap::fallback`**: the owner before any click and after a click outside every panel.
  A viewer wants its viewport (F / R / G / B work right after launch). Not a hack: it is the
  last link of `active`'s chain, so obscured pointers still own nothing.

## Text-editing rule (reconciled with warpbro)
`typing: bool` is supplied by the host (`ctx.egui_wants_keyboard_input()` plus any modal it
runs outside egui). While set, ONLY chords with Ctrl/Cmd fire (they are never text); bare
keys, Space and arrows stay with the widget. One rule, `may_fire`, applied in `take`.
warpbro's old `consume` blocked EVERYTHING while `text_edit_focused()`; porting warpbro to
this crate changes that: Ctrl+Z / Ctrl+D then fire inside a text field. If warpbro wants the
old behaviour it passes `typing` and filters its own global commands (no crate option for it,
on purpose).

## Gotchas
- egui drops a second `pressed` event of a key that is still down: tests that press the same
  key twice must release it in between (or use two different chords).
- `register` must see the same `Ui` layer the panel is drawn in (it stores `ui.layer_id()`).
- `additive` bindings must not contain Shift in their modifiers.
- egui turns the `repeat` event of a key that is not down into a plain press: a repeat test must
  press the key first.


## Tests (`cargo test -p egui-hotkeys`, 21)
The five warpbro tests ported generically, plus: exact-one-event, auto-repeat (only `.repeats()` bindings fire), batch
`pressed` (consumes only what it takes, leaves refused/unbound keys), scope-specific tables,
typing rule (both entry points), owner clear + fallback (startup and after a click outside),
keymap namespacing, copy/paste, `register_at` (strip owns hover and click), stale owner falls
back, other-viewport regions ignored, clipped panel. Hand-mutation checked in a separate
worktree: logical instead of exact matching, no physical lookup, global-first chain, parents ignored, repeat leaking past a nearer binding, `conflicts` blind to the global table / scope tables / additive overlaps, typing rule off / too strict, `consume` ignoring the route, owner
never cleared, `active` without the layer test, `consume` removing every matching event, viewport
filter removed, clip intersect removed, stale owner kept / not forgotten, `register_at` with the
wrong layer or no region: all killed.

## TODO
- [ ] warpbro-rs adopts this crate (replace `src/hotkeys.rs` routing; its tables become a `Keymap`).
- [x] exr-view adopts it (see its `commands.rs`; the toolbar strip registers via `register_at`).
