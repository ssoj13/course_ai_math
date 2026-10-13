# egui-puffin (`egui-puffin`, import `egui_puffin`)

In-game **puffin** profiler flamegraph rendered with egui 0.34. A vendored +
patched copy of upstream `puffin_egui` 0.30.0 (Embark Studios), bumped from
egui ^0.33 to 0.34, and renamed to this library's `egui-*` convention.

## Why it lives here
Was its own repo (`ssoj13/puffin-egui-rs`). Consolidated into egui-widgets-rs so
the profiler ships from the SAME git-ref as every other widget — playa and
gitnexus-rs now consume it from `egui-widgets-rs.git`.

## Gotchas
- **Package renamed `puffin_egui` → `egui-puffin`** (folder `crates/egui-puffin`).
  Import path is `egui_puffin` (hyphen→underscore). Downstream code uses
  `use egui_puffin::…` and the Cargo feature uses `dep:egui-puffin`. Upstream on
  crates.io is still `puffin_egui` — keep that name only in prose/credits.
- Deps are kept **explicit** (not `*.workspace = true`) to stay faithful to the
  vendored copy; the `egui = "0.34"` pin already matches the workspace, so a
  single egui resolves in the lock. `egui_extras` uses `default-features = false`
  + `serde` like upstream.
- Member of the egui-widgets-rs workspace — it has NO `[workspace]` table of its
  own (a member can't also be a workspace root).
- Public API: `profiler_ui(ui)` (in-panel), `profiler_window(ctx)` (floating
  window), `show_viewport_if_enabled(ctx)` (native viewport). Re-exports `puffin`
  so callers can use `egui_puffin::puffin::*` macros.
- The 0.33→0.34 patch: deferred-viewport callback takes `&mut Ui` (was `&Context`)
  and `ViewportClass::Embedded` → `EmbeddedWindow`. See `src/lib.rs`.

## Demo
`egui-widgets-demo` has a **Profiler** page (Chrome group) that turns puffin
scopes on, records a small synthetic scope tree each frame, advances the frame,
and renders the live flamegraph via `egui_puffin::profiler_ui`.
