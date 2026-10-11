# Spike A: shared canvas, lesson islands, evidence

Archived verbatim from AGENTS.md (commit 6d284b0, lines 122-186) on 2026-10-11. The working contract that an
implementer needs is summarised in [AGENTS.md](../../AGENTS.md); this file keeps the original notes, acceptance
history, evidence paths and measurements. Paths under `.superpowers/` are gitignored and exist only on the dev machine.

## Scenes (Spike A result)

Decision (2026-10-10): one shared `<Canvas flat frameloop="demand">` per page (`src/viz/scene/SceneHost.tsx`), and
every `Stage` is a drei `<View>` living in its own Astro island. No `manualChunks`, no per-stage Canvas fallback.
- **Host on demand**: the first `Stage` calls `ensureSceneHost()` (module singleton), which creates
  `<div class="lab-scene-host">` at the end of `<body>` and mounts the Canvas there as its own React root. Pages
  without stages load no three/drei, create no GL context and never run the WebGL probe. (No PageFrame override.)
- `flat` = no tone mapping: r3f's default ACES rendered `#4f7cff` lighter; with `flat` the center pixel is exactly
  `#4f7cff` (measured), so palette colors render as specified.
- `FitView` refits from a ResizeObserver on the stage element: drei's View portal captures `size` once at mount
  (measured stale: 66 px box after 1280->900 px resize, expected 58.8). Ortho zoom = `min(w,h)/(2*extent)`.
- No CLS: `SceneIsland.astro` renders the ViewportCard skeleton `.lab-vp-skel` (28 px header, a 16:10 aspect-ratio
  spacer, the chip row under 720 px when `panel`, 22 px status) that fixes the card height before any script; the
  island (`.lab-vp-card`) fills the same box absolutely. Accessible name = the server `<figcaption>`.
  Task 6h additionally reserves mobile instrument/formula strips via `hud`/`formula` (contract below).
- **Stacking (measured in 6d):** Starlight sets `isolation: isolate` on `.main-pane` (TwoColumnContent.astro), so the
  whole article is ONE stacking context painted below the later, fixed canvas host: no z-index inside the article can
  lift DOM above the scene (grid lines were drawn over the N-panel and tooltips; 6b/6c labels were in fact under the
  canvas too). lab.css opts out with `.main-pane:has(.lab-vp) { isolation: auto }`; then viewport backgrounds (no
  z-index) stay below the canvas and chrome that overlaps the viewport is lifted with z-index 1..6 (below Starlight's
  header/menus). Never add transform/filter/opacity/contain/container-type around a card: each creates a stacking
  context and puts the chrome back under the scene.
- The WebGL drawing buffer is not preserved, so Views need no clear between frames.

### Lesson authoring pattern (islands)

- MDX keeps the `import` of `client:only` components in the server page module (only the `.astro` compiler strips
  them), so importing a scene module from MDX made the prerender evaluate drei/fiber/maath. Those packages have no
  `exports` map, so the server loaded their CJS `main`, which calls `require("three")` -> `THREE_CJS_DEPRECATED`.
  An SSR `noExternal` list did not cure it (more CJS deps behind); not evaluating the code on the server does.
- Each lesson has an **entry** `src/viz/lessons/<slug>.tsx` (demo: `src/viz/demo/engine.tsx`) that MDX imports, and an
  **implementation** `<slug>.impl.tsx` with the scenes. The entry holds only SSR-safe code: the module-level
  `lessonStore(...)` init, DOM-only controls, and `lazyIsland(() => import('./<slug>.impl').then((m) => m.X))`
  wrappers (`src/viz/island.tsx`: `React.lazy` + `Suspense fallback={null}`; SceneIsland already reserves the box).
- Mount scenes with `client:only="react"` inside `<SceneIsland caption="...">`. DOM-only controls (sliders, readouts)
  use `client:load` so they are server-rendered and take their space before hydration (a `client:only` slider caused
  a 0.0085 layout shift).

### Evidence (headless Chrome over CDP, real key/wheel/mouse input, production build via `astro preview`)

Script, JSON reports, screenshots: `.superpowers/sdd/2026-10-10-engine-and-spikes/task-4-evidence/` (`fix1/` = current).
- Home page: 0 canvases, no `.lab-scene-host`, scripts = page/Search/preload-helper/ui-core only (no scene chunk).
- Demo: 1 canvas for 3 stages; no "too many active WebGL contexts".
- Shared store across roots/chunks: the slider (entry chunk, `client:load` root) sets x = 1.50 and the boxes in stages
  A and B (impl chunk, separate `client:only` roots) measure world x = 1.489 (pixel centroid); C moves too. `store.ts`
  is one shared chunk imported by both (one ES module instance per page).
- Scroll 1939 px: C's box stays inside its slot (0 colored pixels outside stage rects). Drag orbits only C.
- Resize after load to 900 px and 360 px: box side 59 / 35 px vs expected 58.8 / 33.8, world x -2.006 / -2.01 at
  x = -2; `scrollWidth` 900 / 360 (no horizontal scroll at 360 px).
- CLS: `.lab-scene-box` heights 395/395/395 at DOMContentLoaded (0 stages mounted) and after hydration; 0
  `layout-shift` entries.
- `prefers-reduced-motion: reduce`: identical numbers. `WEBGL_lose_context` -> `html.lab-no-webgl`, notice
  `display:block` in all 3 stages.

Upstream / known console noise:
- `THREE.Clock: This module has been deprecated` is logged by three >= 0.183 because r3f 9.8.1 (current latest)
  creates `new THREE.Clock()`. three 0.182.0 does not warn (checked on unpkg), but we keep 0.186.1: downgrading four
  releases to silence a deprecation notice is worse than the notice. Revisit when r3f moves to `THREE.Timer`.
- `favicon.svg` 404 predates Task 4 and remains in the 6h report; `public/` now contains the clay matcap asset.
- [ ] Historical Task 6d measurement: scene chunk `engine.impl.*.js` was 1009 kB raw / 269 kB gz (three core is not tree-shaken under
  r3f's `import * as THREE`; drei Line = three-stdlib Line2/LineMaterial added ~32 kB in 6b; 6c primitives + demo ~9 kB;
  6d chrome + drei Grid ~31 kB raw / 10 kB gz). `chunkSizeWarningLimit: 1015` in astro.config.mjs; the real gate is the per-page gzip budget of
  planned scripts/size-report.ts (Task 11; Ruling O: viz core <= 300 kB gz). Current 6h measurements and reviewed
  ownership classification are recorded below; a warning threshold is not a transfer-budget acceptance.

