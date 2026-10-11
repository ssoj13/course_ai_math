# Visual language — "a professional DCC viewport, and the light"

Subject: a math-for-AI textbook for a CG/VFX developer. Reference for UI/UX (user requirement): **Blender** — a
professional DCC look, not "edtech". The reader's home turf is a viewport: navigation gizmo, X red / Y green / Z blue,
an N-panel of draggable number fields, a status bar that tells you what the mouse does, an orange outline on what is
selected, and a light whose brightness *is* a dot product. The course's thesis starts there (N·L), so the identity is built
from that world. Signature illustrations: `signature-illustrations.md` (everything is light, shadows and areas).

Single job of every scene: make one relation visible so clearly that the formula next to it feels obvious.

## Signature

**Every scene is a small, real DCC viewport; vectors in it are beams of light.** Blender-grade chrome (header, gizmo,
N-panel, status bar) frames a calm dark viewport; vectors are crisp light strokes with a faint halo; projections are
literal shadows; the lit sphere computes brightness as exactly `max(0, N·L)`. The prose around it stays quiet.

## Viewport anatomy (Blender-like)

```
┌ header ─────────────────────────────────────────────────────────────┐
│ ◧ 2D ◫ 3D │ Persp/Ortho │ Overlays ▾ │ Reset view        LC-1 · Areas│
├─────────────────────────────────────────────────────────┬───────────┤
│                                                (gizmo) │ N-panel   │
│        viewport (dark gradient, fading grid)    X Y Z  │ a  [3.00 ]│
│                                                        │ θ  [60.0°]│
│                                                        │ a·b 4.50  │
├─────────────────────────────────────────────────────────┴───────────┤
│ status: LMB drag tip · Shift 1.0 steps · MMB orbit · Ctrl+Wheel zoom │
└──────────────────────────────────────────────────────────────────────┘
```
- **Header** (28 px): view toggles, Overlays popover (grid, legs, labels, shadows, angles — each scene declares which
  exist), Reset view, size buttons **Enlarge 2×** (Ctrl+Space, like Blender's Toggle Maximize Area) and **Full screen**
  (Ctrl+Alt+Space / F; Esc exits), scene name right-aligned in muted mono.
- **Navigation gizmo** top-right (Blender position): drei `GizmoHelper` + `GizmoViewport`, circles with X/Y/Z letters,
  negative axes as hollow circles; click to align (3D); hidden in 2D scenes (2D shows a tiny static XY corner instead).
- **N-panel** (right, 200 px, collapsible with `N` key and a chevron): Blender-style property rows — label left, value
  field right; fields are drag-to-scrub (horizontal drag, Ctrl = snap, Shift = fine), click to type, arrow keys; read-only
  values render as flat rows. These rows are the linked "numbers view" (term ids → hover lights scene + formula).
  At < 720 px the N-panel becomes a row of chips under the viewport.
- **Status bar** (22 px): context hints for what the pointer is over ("Drag tip of a", "MMB orbit"), like Blender's.
  Touch devices get touch hints (drag, two-finger pinch), never mouse buttons.
- **Wheel**: a plain wheel always scrolls the lesson page, never the scene (a card must not trap reading);
  Ctrl/Cmd+wheel and pinch zoom.
- **Selection/hot**: Blender's active outline — a 2 px light-orange outline + halo on the hovered/linked object;
  never changes the object's own color (color = meaning, outline = attention).
- **Tooltips**: Blender-like, delayed 500 ms, title + one-line explanation + the formula term.

## Tokens

The viewport and its chrome are **always dark**, in both site themes (like an embedded Blender viewport or a code
block): colors are calibrated once against one background and stay exact. Page prose, formulas and cards outside the
viewport follow the Starlight theme.

| role | value | use |
|---|---|---|
| `vp-bg-top` / `vp-bg-bottom` | `#3A3A3A` / `#282828` | viewport vertical gradient (Blender default theme feel) |
| `chrome` / `chrome-2` | `#2B2B2B` / `#232323` | header, N-panel, status bar |
| `field` / `field-hover` | `#545454` / `#656565` | number fields (Blender widget grey) |
| `grid` / `gridMinor` | `#4A4A4A` / `#444444` | drei shader Grid, fades with distance (minor >= 1.15:1 on `vp-bg-top`, so it stays visible but quiet) |
| `ink` / `muted` | `#E6E6E6` / `#9A9A9A` | labels, ticks, chrome text |
| `axisX` / `axisY` / `axisZ` | `#FF3352` / `#8BDC00` / `#2890FF` | Blender's own axis colors (gizmo, axes, component legs) |
| `vecA` | `#56B4E9` (sky blue) | vector a |
| `vecB` | `#E07BD0` (orchid) | vector b — chosen to clash with neither the red X axis nor the orange selection |
| `vecC` | `#3DD6B0` (bluish green) | results: a−b, a+b, projections |
| `light` | `#FFE27A` | lamps, the N·L light |
| `hot` | `#FFAA40` | active outline/halo (Blender selection orange) |
| `neg` | 45° hatch of the owning color | negative signed quantities (sign never color-only) |

Page-side (theme-following) tokens keep Starlight's own colors; formula term highlight uses `hot` at 22 % alpha.
Contrast: every stroke color ≥ 3:1 against `vp-bg-*`; text ≥ 4.5:1 against `chrome`.

## Type (self-hosted via @fontsource; no runtime font CDN)

- UI chrome, headings: **Inter** (Blender 4.x ships Inter as its UI font) — headings Inter Display 650–700, tight tracking;
  chrome 12–13 px.
- Prose: **Atkinson Hyperlegible Next** — maximal legibility for long explanations.
- Numbers, fields, HUD, code readouts: **JetBrains Mono** (tabular figures, clear 0/O, 1/l).
- Math: KaTeX fonts.

## Scene rendering rules

- Lines: core 2.5 px (drei `Line`, pixel-stable) + halo 9 px at 14 % alpha of the same color. Hot = Blender outline: +2 px
  `hot` outline stroke behind the core, halo 30 %.
- Arrow heads sharp, slightly concave base. Points: 6 px discs with 1.5 px dark rim (Blender vertex look).
- Area tiles: fill 18 % + 1.5 px edge; negative = 45° hatch. Angle arcs: 1.5 px + 10 % sector, value on the bisector.
- Shadows/projections: soft 6 px band on the target line under a dashed drop line.
- Grid: drei `Grid` shader, infinite, fading; axes drawn on top in axis colors, ticks + labels in `muted` mono.
- Lit sphere: unlit custom shader `albedo * max(0, N·L)`; "raw N·L" toggle tints negatives cool.

## Motion

- Direct manipulation is 1:1 (never animated). Indirect changes (timeline, keyboard, snap, reset view) use a critically
  damped spring (~180 ms). Camera "Reset view"/gizmo align animate like Blender's smooth view (~250 ms).
- One orchestrated moment per lesson: the first scene draws itself once on first view (600 ms). Nothing idles.
- `prefers-reduced-motion`: hard cuts everywhere.

## Layout of a lesson page

```
eyebrow (mono caps): LESSON 4 · VECTORS
H1 (Inter Display)
hook paragraph (Atkinson)
┌ viewport card (header · viewport+N-panel · status) ┐   full column width, 16:10 viewport
└────────────────────────────────────────────────────┘
formula (linked)   |  timeline ◀ step 2/4 ▶            side by side ≥ 900 px, stacked below
```

## Self-critique (applied)

- v1 used an indigo "night" palette and a casual display face → replaced by Blender-grade neutral chrome and Inter, per the
  user's "professional, like a DCC" requirement; the viewport is always dark so the vector colors are calibrated once.
- vecB amber clashed with Blender's orange selection → orchid; vecA sky blue stays clear of the Z-axis blue by lightness.
- Bloom rejected (bundle, GPU variance, blurs exact values); halo strokes give the light feel without lying about geometry.
- Numbers appear only where order is real (lesson numbers, timeline steps).
