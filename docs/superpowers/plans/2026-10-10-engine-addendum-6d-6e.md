# Engine addendum: Tasks 6d (visual language) and 6e (lesson primitives)

Belongs to Plan 1 (`2026-10-10-engine-and-spikes.md`); same Global Constraints apply. Inserted by controller
Rulings N/P (vertical slice: first two lessons right after Task 7). Specs:
`docs/design/visual-language.md` (6d) and `docs/design/lessons-law-of-cosines-dot-product.md` §5 (6e).
Principle from the user: prefer standard, proven components (three/addons, drei) over custom builds.

### Task 6d: Visual language — Blender-grade viewport chrome on standard DCC components

Spec: docs/design/visual-language.md (v2, Blender reference). User requirements: professional DCC look (Blender UI/UX),
maximal clarity, beautiful and modern; prefer standard three/drei components.

**Files:** src/styles/lab.css (tokens), src/viz/palette.ts (roles per spec), new src/viz/scene/{ViewportCard.tsx,Gizmo.tsx,NPanel.tsx,StatusBar.tsx,motion.ts},
Stage/primitives styling, SceneIsland.astro (card frame), astro.config.mjs (fonts), package.json (@fontsource packages), demo, tests.

1. **Tokens & fonts.** Viewport + chrome always dark (spec table: vp-bg gradient, chrome, field, grid, ink/muted, Blender axis
   colors #FF3352/#8BDC00/#2890FF, vecA #56B4E9, vecB #E07BD0, vecC #3DD6B0, light #FFE27A, hot #FFAA40). Page-side elements
   keep Starlight theme colors. Self-host Inter (+ Inter Display for headings), Atkinson Hyperlegible Next (prose), JetBrains Mono
   (numbers/fields) via @fontsource (verify package names; exact pins; only used weights). Test: lab.css defines every --viz-* role;
   contrast test computes ≥ 3:1 for every stroke role vs vp-bg-top/bottom and ≥ 4.5:1 for ink/muted vs chrome.
2. **ViewportCard** (Blender anatomy): header (2D/3D badge, Persp/Ortho toggle in 3D, Overlays popover with the scene's declared
   overlay flags stored in the lesson store, Reset view with smooth view animation, scene name), viewport, collapsible N-panel
   (`N` key + chevron; < 720 px becomes chips under the viewport), status bar with context hints from the hovered object.
   The server-rendered card frame reserves exact size (no CLS).
3. **NPanel rows**: Blender-style property rows (label | value field). Fields reuse the existing scrub engine (useScrub/keyStep):
   horizontal drag, Ctrl snap, Shift fine, click-to-type with validation, arrow keys; read-only rows for derived values; every row
   carries a term id (hover ↔ hot). Replaces ad-hoc HUD chips.
4. **Gizmo**: drei GizmoHelper + GizmoViewport (Blender colors, letters, hollow negative axes; click-to-align with smooth view) —
   verify it renders inside a drei View portal; fallback order: three/addons ViewHelper in the same View, then a small custom tripod;
   2D scenes show a tiny static XY corner. Record the result in AGENTS.md.
5. **Grid**: drei Grid shader (infinite, fading, palette colors) replacing the line grid if correct in ortho 2D at all zooms; axes and
   ticks stay our primitives.
6. **Strokes & selection**: core 2.5 px + halo 9 px @14 %; hot = Blender active outline (+2 px hot-colored stroke behind core, halo 30 %);
   arrow heads sharp/concave; points as 6 px discs with dark rim; area fill 18 % + 1.5 px edge; negative = 45° hatch; projection
   shadow band. Tooltips Blender-like (500 ms delay; title, one line, formula term).
7. **Motion**: motion.ts critically damped spring (no overshoot, unit-tested) for indirect changes only; smooth view ~250 ms for
   Reset/gizmo align; first-view draw-in once (600 ms); prefers-reduced-motion → cuts.
8. **Demo** rebuilt as two ViewportCards (2D, 3D) showing every element; CDP screenshots in both site themes and at 360 px.
Gates: tests, check, build 0 warnings, viz-core ≤ 300 kB gz (Ruling O). Commit: `feat(viz): Blender-grade viewport chrome, tokens, fonts, light strokes, gizmo, N-panel, motion`.

### Task 6e: Lesson primitives (from lessons spec §5)

**Files:** `src/viz/scene/drag.ts`, `view.tsx`/`handles.tsx` (custom move), new `src/viz/scene/{HalfPlane,LitSphere,Circle}.tsx`,
new `src/viz/{Ledger,Challenge,Toggle}.tsx`, Arrow (leg ids), tests, demo.

Implement exactly the contracts in `docs/design/lessons-law-of-cosines-dot-product.md` §5 items 1–9:
custom handle mapping `Draggable.move?(vals, raw) => Record<string, Val>` (DragLayer + keyboard nudge both use it);
pure `magnet(v, targets, zone)` + AngleArc `ticks`; HalfPlane; LitSphere (ShaderMaterial: `mix(dark, base, max(0, N·L))`,
`raw` mode tints negatives; point light uniform from a store key; no scene lights; flat canvas); Ledger (DOM SVG, signed hatched
last bar); Challenge (sticky success, progress key `challenge:<lesson>:<n>`, `step['<timeline>:max']` support in Timeline is Task 7's —
coordinate: Timeline writes `<id>:max`); Circle (full loop, own parametrization with orthonormal basis from `normal`);
Arrow leg ids `${id}:x|y|z`; Toggle (role switch, writes 0/1). Pure helpers unit-tested; DOM components DOM-tested;
LitSphere verified by CDP pixel sampling (brightness at the probe = max(0, N·L) within 1/255).
Gates as above. Commit: `feat(viz): lesson primitives — custom drag mapping, magnet, half-plane, lit sphere, ledger, challenge, circle, toggle`.

### Task 6f: Viewport size modes — enlarge 2× and full screen

User requirement (task.md): every scene window can be enlarged two times and expanded to the whole screen, immediately.
Blender analogue: Ctrl+Space "Toggle Maximize Area" and Ctrl+Alt+Space "Toggle Fullscreen Area".

**Files:** ViewportCard.tsx (header buttons), Stage.tsx / SceneHost.tsx (fit + invalidation), lab.css, hints, tests, demo.

1. **Modes:** `normal` → `wide` (2×) → `full`, per card, shown as two header buttons right of Reset view (icons with
   tooltips "Enlarge 2×" / "Full screen", pressed state, `aria-pressed`). Shortcuts while the card has focus or hover:
   Ctrl+Space = wide, Ctrl+Alt+Space or F = full; Esc returns to normal. Only one card can be `wide`/`full` at a time.
2. **Wide (2×):** the card leaves the 632 px article column: width `min(2 × column, 100vw − 32 px)`, centred on the page,
   same aspect ratio (so ~2× linear size), the page reflows below it (user-initiated, so not CLS); the N-panel docks inside
   on the right as in Blender. On screens where 2× does not fit, wide = full width minus gutters.
3. **Full:** the card covers the window (fixed, inset 0, chrome on top, N-panel docked, status bar at the bottom), page
   scroll is locked, and the browser goes full screen via the Fullscreen API on `document.documentElement` — NOT on the
   card element: the shared WebGL canvas lives at the end of `<body>`, outside the card, and element fullscreen would hide
   it. Where the Fullscreen API is missing (iPhone Safari) the fixed overlay alone is used. Leaving browser full screen
   (Esc / system gesture, `fullscreenchange`) returns the card to normal.
4. **Rendering:** the View follows the card rect in every mode (FitView ResizeObserver refits; ortho zoom keeps the same
   world extent, so the scene gets bigger, not wider); the canvas stays above the overlay background and below the chrome
   (z-index order recorded in AGENTS.md); `invalidate()` on every mode change; camera, orbit and store state survive the
   switch; focus stays on the card; reduced motion = cut, otherwise a ~200 ms size transition via motion.ts.
5. **Tests:** pure mode reducer (one active card, Esc, fullscreenchange); DOM test of the header buttons and shortcuts;
   CDP in the production build: in wide and full the stage pixel rect equals the card viewport rect (±1 px), the scene is
   drawn at the new size (pixel sample), returning to normal restores the layout with 0 layout-shift entries, 360 px works.
Gates as above. Commit: `feat(viz): viewport size modes — enlarge 2x and full screen`.

Execution order (ledger Ruling N/P): 6c → 6d → 6e → 6f → 7 → lessons law-of-cosines + dot-product → merge to main + deploy → 8 → 9 → 10 → 11.

## Illustration roadmap (user requirements, task.md) — scheduled with the sections that use them

| Scene (design doc) | Lessons | Engine work it needs (future plan tasks) |
|---|---|---|
| S1 missing rectangles, S2 dot product = area = lighting (`signature-illustrations.md`, `lessons-law-of-cosines-dot-product.md`) | 0.5, 1.4 | 6c–6e, 7 (this plan) + polygon cut/rearrange helper if the lessons spec asks |
| Matrix × vector = shadows on new rulers, orthonormal flag, Gram heatmap (`backlog-basis-projection-scene.md`) | 1.6, 2.2, 2.5, 2.7, A1 | oblique projection segment, ruler primitive, Gram–Schmidt in geom.ts, 3×3 heatmap, keep-orthonormal drag constraint |
| S3 matmul light box (C = shadow on the wall), S4 einsum plugs & cables (`signature-illustrations.md`) | 2.4, 3.6 | instanced cell box, slice scrubber, socket/cable graph (React Flow or r3f) |
| Tensor road rank 0→N, real data, index↔memory strides, reductions (`backlog-tensor-road-scene.md`) | 3.1–3.5 | InstancedMesh cells, nested layout fn, memory strip, TS core Tensor strides |
| S6 + D1–D6 derivatives: zoom-until-straight, slices, tangent plane, gradient, directional derivative, fields, time inputs, 6-partials bar chart, slice explorer (`backlog-derivatives-scenes.md`) | 4.1–4.7, 5.1–5.5 | tube/surface meshes, marching squares, three/addons MarchingCubes, slice heat textures, instanced arrow field, sparkline, lockable slider group, microscope second View |
| B1–B3 error as square tiles, forward/backward computation graph with chain rule per node, learning loop (`backlog-loss-backprop-scenes.md`) | 5.6, 8.2, 8.5, 9.1–9.4 | TS autodiff tape with node ids, React Flow tape view, particle edges, loss surface, step controller |
| S5 attention is lighting (`signature-illustrations.md`) | 10.1–10.3 | reuses LitSphere/lighting, softmax exposure meter |
| three.js editor "sandbox" page (open a lesson scene in a full editor) | optional | evaluate after Part 1 ships |

Rule for every scene: Blender-grade ViewportCard (visual-language.md), linked term ids across scene / N-panel / formula / code,
direct manipulation 1:1, keyboard + touch parity, reduced motion, no fake data.
