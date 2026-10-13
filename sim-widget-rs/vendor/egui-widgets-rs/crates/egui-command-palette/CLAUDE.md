# egui-command-palette

A searchable **command palette** (Ctrl-P style) for egui 0.34 + a reusable fuzzy
matcher + a fuzzy-search combo box. Hermetic: depends on **egui only** (fuzzy
matching is inline — NO `fuzzy-matcher` crate).

## What it is
- A centered `egui::Modal` with an auto-focused search field over a flat list of
  caller-supplied commands, fuzzily filtered + ranked as you type. Up/Down move the
  highlight, Enter runs it, Esc/backdrop cancels, click runs a row.
- `fuzzy_score()` — the reusable case-insensitive subsequence matcher.
- `fuzzy_combo()` — a `ComboBox` whose popup has a search field filtering items by
  `fuzzy_score` (the long-list companion to the palette).

## Public API
- `struct Command { id: u64, label: String, hint: Option<String> }` — caller-supplied,
  read-only. `id` is the host's opaque key returned on selection. `hint` is dimmed
  right-side text (shortcut/category), NOT matched. Ctors: `Command::new(id,label)`,
  `.hint(s)` builder.
- `struct CommandPalette` (`Default`/`new()`) — owns only transient view-state:
  `query: String`, `selected: usize` (index into the FILTERED list), `needs_focus: bool`.
  - `fn show(&mut self, ctx, commands: &[Command]) -> CommandPaletteResult`
  - `fn reset(&mut self)` — clears query+selection, re-arms auto-focus (call on open).
  - `fn query(&self) -> &str`.
- `enum CommandPaletteResult { Open, Cancelled, Run(u64) }`.
- `fn fuzzy_score(needle, haystack) -> Option<i32>` — the matcher (see scoring).
- `fn fuzzy_combo(ui, selected: &mut Option<u64>, commands, id_salt) -> egui::Response`.

## Result-enum flow (immediate-mode contract)
Mirror of `egui-encode-dialog`. Host owns `commands` + an `open: bool`. On Ctrl-P:
set `open=true; palette.reset()`. While open, call `show()` each frame:
- `Open` → keep going.
- `Cancelled` → `open=false`.
- `Run(id)` → `open=false`, run the action keyed by `id`.
Commands pass by `&` (read-only); palette never mutates them.

## fuzzy_score scoring (simple, documented, tweakable)
Case-insensitive subsequence match. `None` if `needle` isn't a subsequence of
`haystack` (chars must appear in order). Empty needle → `Some(0)`. Higher = better.
Single pass over `haystack` consuming `needle`:
- flat **+1** per matched char (so more-complete matches don't lose for free),
- **+8** word-start bonus (char 0, or right after ` _ - / .`) → acronym hits,
- **+16** consecutive bonus (match directly follows the previous match) → contiguous runs,
- **−1 per leading gap** (first-match position) → earlier matches rank higher.
Relative magnitudes only: contiguous > word-start > earlier position (the usual
palette feel). `CommandPalette` ranks by descending score, stable on ties (caller order).

## Keyboard nav
Read+CONSUME Up/Down/Enter/Esc via `ctx.input_mut(|i| i.consume_key(NONE, key))`
BEFORE drawing the TextEdit, so arrows drive the list instead of the text cursor and
Esc isn't swallowed. Up/Down clamp to the filtered list. Enter → `Run(selected)`.
Esc → `Cancelled` (belt-and-braces with the Modal's own `should_close()`).

## egui 0.34 API notes
- Native `egui::Modal::new(Id).show(ctx, |ui| …)`; cancel on `modal.should_close()`
  (backdrop/Escape) — identical to encode-dialog.
- Auto-focus: `TextEdit::singleline` returns a `Response`; call `response.request_focus()`
  once (gated by `needs_focus`). Re-armed by `reset()`.
- `fuzzy_combo` uses `ComboBox::from_id_salt` and stashes its popup search text in
  `ui.memory(...).data.get_temp/insert_temp::<String>(id)` so it survives across
  frames while the popup is open and is dropped when egui forgets the popup.

## Gotchas
- `selected` indexes the **filtered** list, not `commands`; it's re-clamped every
  frame after filtering (the filtered list can shrink as you type).
- We consume the nav keys globally for the frame — fine because the palette is modal
  (nothing else should react to Up/Down/Enter/Esc while it's up).
- Hovering a row also moves the highlight so mouse + keyboard agree.
- `fuzzy_score` lowercases via `char::to_lowercase` (Unicode-aware) and compares the
  leading folded char — correct for realistic command labels.

## Build / verify
Inherits workspace fields; `[lib] path = "src/egui_command_palette.rs"`. Can't
`cargo build` standalone (workspace-inherited deps) — integrate into the workspace
+ wire into `egui-widgets-demo` to compile. Logic is unit-tested (`fuzzy_score`
ordering, subsequence rejection, `ranked_indices`, `reset`) with no egui context.

## TODO / future
- Optional in-label match highlighting (return matched char indices from a
  `fuzzy_indices` variant and paint them bold). Left out to keep the matcher simple.

(New crate — not gathered from a host engine; written for this library.)
