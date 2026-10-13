# egui-pattern — local notebook

Keep this file current: what the crate is, how it works, traps.

## What it is
Procedural pattern fills. A `Pattern` is data (up to 3 periodic `Prim`s: line families, dot
grids, a checker); `tile::rasterize` turns it into one seamless antialiased tile; `fill_pattern`
draws a rect of ANY size with it as ONE textured quad (egui texture, `TextureWrapMode::Repeat`).

Why: shape-per-line hatching cost one shape per 6 points of every clip - 177k shapes / ~25 ms
a frame on a 7000-frame comp zoomed in (Playa `timeline_panel_cost`), almost all off screen.

```text
tile.rs          no egui: Dir, Prim, Pattern (+ presets), rasterize -> Tile (coverage u8)
egui_pattern.rs  fill_pattern(painter, rect, &pattern, color, origin)
                 texture cache in ctx temp data, key = pattern bits + pixels_per_point
```

## Rules and traps
- **Periods are whole pixels** (points x ppp, rounded); the tile is the LCM per axis, so repeats are
  exact. Only the four grid-periodic line directions exist (H, V, rising `/`, falling `\`);
  an arbitrary angle does not tile - it would need a shader.
- Periods are measured along the axes (a diagonal every 6 points along a bar); `width` is across
  the line.
- **Supersampling is 16x16**, not 4x4: a regular 4x4 grid is aligned with the diagonal lattice
  and thinned a 1-point hatch to 0.208 of the plane instead of 0.236 (test
  `a_tile_covers_the_share_its_geometry_says` is the oracle).
- `origin` pins the phase: pass the item's own corner, or the pattern crawls when the view pans.
- The texture is a white mask; the colour is the vertex tint.
- Refused patterns (`TileError`) draw nothing and are logged once per (pattern, scale).
- The generator has no egui dependency on purpose: it can move to a standalone crate if a non-egui
  consumer appears.

## Users
- `egui-track-timeline`: `Fill::Pattern(Pattern)` (re-exports `Pattern`, `Prim`, `Dir`).
- Candidate: Playa's viewport checker matte (a 2x2 repeat tile today).
