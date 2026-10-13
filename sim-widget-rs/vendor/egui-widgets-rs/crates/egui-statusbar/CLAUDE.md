# CLAUDE.md — `crates/egui-statusbar`

Conventions: repo-root `CLAUDE.md`.

## What
Reusable, generic sectioned status bar for egui (Maya/Houdini-style resizable
cells). No app types — usable by exr-view and any other egui app. Used by
exr-view's status bar: Frame / Cache (N/M) / GPU·backend / VRAM budget / file-info (flex).

## API
- `StatusBarLayout { widths: Vec<f32> }` (serde) — one width per resizable section
  (the last section flexes). Store in app config; pass back each frame.
- `Section::new(default_width, |ui| { ... })` — a section + its content closure.
- `StatusBar::new().show(ui, &mut layout, &mut sections) -> bool` — draws N sections
  with draggable splitters (drag = resize, double-click = reset); returns `true`
  when the user resized (so you re-save the layout). Section content is clipped
  to its cell. The last section flexes (`default_width` ignored).

## Gotchas
- `layout.widths.len()` must equal `sections.len() - 1`. A mismatch (config from
  a build with a different section count) auto-resets to section defaults.
- egui 0.34; uses `UiBuilder` + `set_clip_rect` for per-cell clipping.
