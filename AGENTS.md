# math-course — working notes for agents

Keep this file current: record structure, decisions, traps and TODOs here (`[ ]` / `[x]`).
`CLAUDE.md` only imports this file (`@AGENTS.md`); edit AGENTS.md.

## What this is

Interactive, visualization-first textbook "Math for AI", from the law of cosines and the dot product
to matrices, tensors, calculus, probability/statistics and attention. Next-generation format:
roadmap.sh-style knowledge map + concept mind maps everywhere + live, linked visualizations
(inspiration: poloclub/transformer-explainer, 3Blue1Brown, PyTorch "Inside the Matrix" (mm), Bret Victor).
Engine is modeled on `C:\projects\projects.rust.cg\cgprojs\vllm_en` (course.json single source,
progress, lesson links) — read its AGENTS.md before building infrastructure.

Reader profile: CG/VFX developer (Maya, Houdini, shaders), self-described as needing very
gentle, step-by-step explanations. Explain via CG analogies (N·L, xforms, Graph Editor curves).

## Toolchain

- **bun 1.4.3 is required** (`packageManager` in package.json). bun 1.3.12 crashes on Astro React SSR: its `react-dom/server.bun.js` loader aborts with "Expected CommonJS module to have a function wrapper". The build runs on bun, no Node needed.

## Decisions (2026-10-10)

- Language: **bilingual like vllm_en**: English at the root, Russian under `/ru/` (Task 6g infrastructure accepted; lesson bodies remain explicit fallbacks); chat stays Russian.
- Hosting: **GitHub Pages** (static; no server-side code).
- Python: **Pyodide** in-browser cells (lazy, on click) **and Colab** notebooks; one notebook source.
- Statistics/probability module: **mandatory**, incl. "why squares everywhere" (see below).
- **Mind maps everywhere**, built from one term graph (see below).
- Stack (proposed, awaiting final OK): Astro + Starlight shell; React islands; react-three-fiber + drei
  (2D and 3D in one engine); @xyflow/react + elkjs (maps, computation graphs); zustand (linked views);
  KaTeX with ids on formula terms; TS `core/` (Tensor with shape/strides + reverse-mode autodiff that records
  the graph we draw). Rust/wasm only where a measurement proves JS too slow.

Latest versions checked 2026-10-10 (npm registry): astro 7.3.8, @astrojs/starlight 0.42.6, @astrojs/react 7.0.1,
react 19.3.0, @react-three/fiber 9.8.1, @react-three/drei 10.7.9, three 0.186.1, @xyflow/react 12.12.0, elkjs 0.12.0,
zustand 5.0.15, katex 0.19.0, pyodide 314.0.7, remark-math 6.0.0, rehype-katex 7.0.1, @astrojs/markdown-satteri 0.4.3.
Tooling: bun 1.3.12.

Reuse from vllm_en: course.json → sidebar/numbering/links (`course.ts`, `remark-lesson-links.mjs`, `route-links.ts`),
`store.ts`, `progress.ts`, sidebar override, notebook generator (`notebooks/build/`), `numfmt.ts` (FP8/NVFP4/MXFP4 —
for branch Q). Open risk: vllm_en uses the `satteri` markdown processor with `mdastPlugins`; verify that remark-math /
KaTeX work with it before relying on `$...$` in MDX.

## Core principles

1. Every number is visible and linked: hover a matrix cell → arrow in scene, formula term, code line light up.
2. Every animation is a scrubbable timeline (step / back / scrub), not a video.
3. Scrubbable numbers in formulas drive the scene.
4. No fake data: the drawn computation graph is the autodiff graph that computes the gradients.
5. Each concept has a **depth stack**: intuition → formula → code (NumPy/TS) → machine
   (e.g. dot product = FMA chain / SIMD lanes / summation-order rounding; matmul = tiled GEMM / tensor cores;
   tensor = strides over flat memory; derivative = autodiff tape vs finite differences).
6. Each advanced topic is anchored to a real AI mechanism (LoRA = low rank/SVD, RoPE = rotations,
   √d in attention = variance of a dot product, LayerNorm = projection+scale, flows = Jacobian determinant).

## Term graph (single source for all mind maps)

`src/data/terms.json` (planned): nodes = terms with a one-line "X is Y built on Z" definition and depth level;
typed edges: `built-on`, `is-a`, `special-case-of`, `equivalent-to`, `used-in`.
Views generated from it: course roadmap, per-lesson local map (1–2 hops), genealogy of a term down to its roots,
depth stack, glossary, auto flashcards ("variance is built on ___"). Build-time validation:
all refs exist, no cycles in `built-on`.
Example chain: attention → softmax + dot product → cosine → law of cosines → Pythagoras.

## "Why squares everywhere" (statistics module spine)

1. Pythagoras: Euclidean distance; squared error = squared distance between data and prediction vectors.
2. The mean minimizes Σ(x−c)² (median minimizes Σ|x−c|): smooth parabola, unique minimum.
3. Differentiability: d/dx x² = 2x vs the kink of |x|.
4. Covariance is an inner product of centered vectors; correlation = cosine of centered vectors;
   uncorrelated = orthogonal; variances of independent variables add = Pythagoras.
5. Gaussian: −log density = squared error, so MSE = maximum likelihood under Gaussian noise.
6. Decompositions are Pythagoras of a projection: total = explained + residual (R²); bias² + variance.
7. Honest downside: outlier sensitivity → L1, Huber.

## Curriculum

Base track: foundations (functions, Pythagoras, unit circle) → vectors (law of cosines → dot product,
projection, cosine similarity, cross product) → matrices (transformations, columns = images of basis,
4 views of matmul incl. 3D cube, determinant, inverse, eigenvectors) → tensors (ML: n-d arrays, image HxWxC,
text BxTxD, strides/views, broadcasting, reductions, einsum) → derivatives (tangent zoom, chain rule,
gradient, Jacobian zoom, gradient descent, backprop graph) → integrals (Riemann sums, FTC, densities,
expectation) → probability & statistics.
Advanced track: orthogonality/QR/least squares, eigen/quadratic forms, SVD/PCA/LoRA, complex numbers/
rotations/Fourier/RoPE, matrix calculus/VJP, Hessian/Newton, optimizers, information theory, normalization,
floating point (bridge to vllm course), change of variables/flows, multivariate Gaussian/score, true tensors
(index notation, co/contravariance, tensor product).
Capstones: linear → logistic regression → MLP with backprop → embeddings → attention head →
transformer block → LoRA, with "zoom down" from any number to the fundamentals.

## TODO

- [x] Stack approved (React + r3f + react-flow; Rust/wasm only by measurement); scope: whole course, built section by section
- [x] Reference section = Part 0 (0.G, 0.3–0.5) + Part 1 (1.1–1.8); storyboard: docs/design/part0-1-storyboard.md
- [x] Plan 1 (engine + spikes) written and reviewed by Fable: docs/superpowers/plans/2026-10-10-engine-and-spikes.md
- [ ] User review of Plan 1; repo name/visibility (proposed ssoj13/math-course, public)
- [ ] Execute Plan 1 (subagent-driven, reviewer per task), then write Plan 2 (12 lessons) after Spike A results
- [ ] Stage 1: engine (shell, React islands, r3f Stage, KaTeX, core Tensor+autodiff with tests, term graph + map)
- [ ] Stage 2: reference lessons: Vectors + Matrix as transformation
- [ ] Stage 3: tensors & matmul (MatmulCube, MemoryStrip, einsum)
- [ ] Stage 4: calculus (Jacobian zoom, comp graph, GD)
- [ ] Stage 5: statistics ("why squares"), remaining base track, advanced track, capstones

## Tests

- Required gates before every commit: `bun test tests/js`, `bun run check` (astro check = TypeScript over the whole
  repo incl. tests, 0 errors/warnings/hints) and `bun run build` (0 warnings).
- `tsconfig.json` sets `"types": ["bun"]`: TypeScript 6 defaults `types` to `[]`, so `bun:test` would not resolve.
  TypeScript is pinned to 6.x because `@astrojs/check` 0.9.10 peers `^5 || ^6` (TS 7 is out but unsupported).
- Pure tests (`tests/js/*.test.ts`) run under plain `bun test tests/js` with no DOM. Keep modules they import DOM-free.
- DOM/component tests (`*.test.tsx` that mount React) opt in by importing `./dom` as the FIRST import. `tests/js/dom.ts` registers happy-dom via `@happy-dom/global-registrator` (devDependency, exact pin) and sets `IS_REACT_ACT_ENVIRONMENT`. It is not preloaded, so pure tests never see `document`; bun shares one process, so after a DOM file loads, later files do see the globals.
- Mount with `react-dom/client` `createRoot` and wrap updates in `act` from `react`.

## Math authoring

- Plain math in prose: `$a\cdot b$` inline; display math MUST be fenced (`$$` on its own lines). A one-line `$$x$$` is parsed as *inline* math (micromark rule), not display. Rendered to KaTeX HTML at build time by `src/lib/satteri-math.mjs` (satteri `features.math` + `renderStaticMath` in `src/viz/tex.ts`); pages ship no math JS. Katex CSS comes from `customCss`.
- A bad formula fails the build with the `.mdx` path. Raw non-ASCII in math (`°`, `θ`, `→`) is rejected by `renderStaticMath` (KaTeX `strict` alone does not flag them): write `^\circ`, `\theta`, `\to`.
- MDX traps outside math: `{...}` in prose is a JS expression (`{1 + 1}` renders `2`; `a {b}` fails the build with a ReferenceError) - escape as `\{ \}`; a bare `<` followed by a letter starts JSX. Inside `$...$` braces/`<`/`_` are safe (`$\{x\}$`, `$a < b$`).
- `.ts` is importable from the `.mjs` plugin at Astro config load (bun runtime), so there is one OPTS definition in `tex.ts`.
- Linked/scrubbable formulas use the React `Formula` (engine plan Task 3), not this path. Hidden test page: `src/content/docs/dev/math.mdx` (not in sidebar, `pagefind: false`, noindex; still listed in the sitemap - [ ] decide whether to filter `/dev/` from `@astrojs/sitemap`).

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

### Scene primitives and dragging (Task 6b)

- Files: `scene/{Grid,Arrow,Point,Segment}.tsx` (primitives), `scene/handles.tsx` (three-free: per-stage drag registry
  `createStage`, `StageCtx`/`useStage`, `usePos`, `useDraggable`, `DragKeys` overlay), `scene/view.tsx` (r3f: `DragLayer`,
  `SLine`, `wppAt`, `usePxScale`), `scene/drag.ts` (pure: `hitHandle`, `snap`, `moved`, `keyDelta`, `worldPerPx`, `rayPlane`).
- Positions are a literal `V` or a store key string; a missing/scalar key throws (no silent fallback). 3D data is y-up.
- React context does not cross drei's View tunnel: Stage creates ONE registry and provides it INSIDE `<View>`, and passes
  the same object to the overlay (`DragKeys`). Primitives outside a Stage throw.
- Drag start, hover cursor and the touch scroll lock all use ONE hit test: `reg.pick` = `hitHandle` over handles
  projected with the View camera (`hitRadius`: 14 px mouse, 22 px touch/pen). There is no hit mesh. Listeners are on
  `.lab-stage` in the capture phase, so a handle press disables OrbitControls (`controls.enabled = false`) before its
  own pointerdown runs on the View div.
- Touch: `touch-action` is latched at touchstart, so toggling it after a hit cannot stop the scroll of that gesture. A
  non-passive `touchstart` that calls `preventDefault()` on a handle hit does. OrbitControls sets an inline
  `touch-action:none` on the 3D View div (again on every reconnect); `.lab-stage-view { touch-action: pan-y !important }`
  beats it, so on phones a vertical swipe over a 3D stage scrolls and a horizontal one orbits.
- `reg.project` returns null outside the clip volume (`ndcToPx`): behind the camera or off-stage, nothing is picked or labelled.
- `moved(vals, keys, raw)`: keys[0] snaps to 0.1, the rest get the same delta ('whole' = translate without reshaping).
  Drags and keyboard (`keyDelta`: arrows 0.1, Shift 1, PageUp/PageDown = z in 3D) share it; `writeMove` skips no-op writes.
- Pixel sizes never come from r3f `size` (stale in a View): `SLine` refreshes LineMaterial `resolution` (and `dashPx`)
  from the stage element per frame; heads/dots/glyphs/markers scale per frame via `wppAt`/`usePxScale`. Per-frame
  helpers in view.tsx use module-level Vector3 temporaries (no allocation per object per frame).
- `hotOn`/`hotOff` in store.ts are the only hot writers for single objects (write only on change; Formula uses hotOn).
  Hover arbitration since 6d: `useHoverable(lesson, id, HOVER_RANK.*)` (see the 6d section), not plain stopPropagation.
- Axis letters and tick numbers are DOM labels since 6d (self-hosted fonts now exist). `label` on Arrow/Point names
  the keyboard button only; visible text is a separate `<Label>`.
- Evidence: `.superpowers/sdd/2026-10-10-engine-and-spikes/task-6b-evidence/` (`cdp-drag.ts`, `final/`).
- [x] Default 3D camera frames the whole extent (6d fix round 1: `fitDistance`).

### Angle, area and label primitives (Task 6c)

- Files: `scene/{AngleArc,RightAngle,AreaTile,Parallelogram,Label}.tsx`; pure helpers in `geom.ts` (`angleArc`,
  `rightOf`, `isRight`, `hatch`, `cross`; `rectOn` removed in 6d) and `drag.ts` (`hitRadius`, `ndcToPx`, `pushAway`).
- **Label** is DOM, not drei `Html`: scene objects register a `LabelSpec` (`useLabel`, three-free, in handles.tsx) with
  the stage registry; the overlay (`Labels`) renders spans (`aria-hidden`, `pointer-events:none`, role color via
  `roleVar` = CSS var, so theme flips need no render); `LabelLayer` calls `placeLabels` in `useFrame(..., 2)` - AFTER
  drei View's priority-1 render, which sets this frame's frustum/aspect and camera matrices (at priority 0 labels lag
  one frame, and on a demand canvas that frame can be the last). Positions are written as `transform`, no React render.
  `away`/`gap` keeps a label's near edge `gap` px off an anchor (arc bisector labels), measured with the label's box.
- **AngleArc** `from`/`to` are DIRECTIONS from `at`, not points. Shorter arc, 0..180 degrees; 180 = semicircle (2D CCW
  from `from`; 3D in the plane of `from` and `normal`). 3D arcs REQUIRE `normal` (thrown at first render). Zero
  direction -> nothing drawn, label hidden, no throw (`angleArc` returns null).
- **AreaTile** outward = RIGHT of a->b (`rightOf`: 2D `[dy, -dx]`, 3D `(b-a) x normal`) = outside of a CCW polygon; same
  convention as docs/design/lessons-law-of-cosines-dot-product.md (`cw(edge)`). `flip` for clockwise data (e.g.
  `flip={signedArea2(a, b) < 0}`). `signed` + `area` (number or fn of vals): bar of height area/|b-a| on the base,
  positive outward, negative inward + hatched, zero/degenerate draws nothing.
- **Parallelogram** 2D: negative orientation (signedArea2 < 0) hatched; 3D plain quad. Collinear -> nothing. `Quad`
  (in Parallelogram.tsx) was the shared flat fill in 6d (`STYLE.fillA` = 18 %, polygonOffset so coplanar lines keep their color) + 1.5 px edge +
  6 px hatch; hatch spacing in px comes from `useWorldPx` (quantized React state, rebuilt only on ~9% scale change).
- **RightAngle**: px-sized square, auto-shown within `tol` = 0.5 degrees of 90 (or `show`); hidden for zero vectors.
- Derived geometry (projection foot etc.) is computed in the lesson .tsx with `useVal` inside a child of Stage and passed
  as literals (see `Projection` in demo/engine.impl.tsx); guard zero vectors there (`project` throws on zero).
- Evidence: `.superpowers/sdd/2026-10-10-engine-and-spikes/task-6c-evidence/` (`cdp-6c.ts`, `final/`).
- [ ] Poly / moving rectangles / AngleArc `ticks` for the 0.5 lesson (design doc) are 6e scope.

### Palette and tokens (Task 5, revised in 6d)

The values below record the 6d baseline; Task 6h's current implementation supersedes its palette and stroke/fill recipe
as described below, accepted after independent review. CSS roles remain the shared source; clay adds a light studio look.

- Scene colors come from `usePalette()` (`src/viz/palette.ts`), never hex literals in components. Roles = the spec
  table of docs/design/visual-language.md (`ROLE_VARS`: bgTop/bgBottom, chrome/chrome2, field/fieldHover,
  grid/gridMinor, ink/muted, axisX/Y/Z (Blender), vecA/B/C, light, hot). Values are `--viz-*` vars in `lab.css`,
  defined ONCE under `:root`: the viewport and its chrome are always dark in both site themes (Ruling R). Every value
  is a literal `#rrggbb` (WebGL). `--viz-hot-bg` (formula highlight, hot at 22 %) is CSS-only. Primitives take
  `SceneRole` (`STROKE_ROLES`), each >= 3:1 on both ends of the viewport gradient; ink/muted >= 4.5:1 on chrome
  (`tests/js/tokens.test.ts` parses lab.css and computes WCAG contrast).
- Non-color look = `STYLE` in palette.ts (core 2.5 px, halo 9 px @14 %, hot outline +2 px each side + 30 % hot halo,
  edge 1.5 px, fill 18 %, sector 10 %, hatch 6 px / 1 px, head 14 px, point = 6 px disc (radius 3) + 1.5 px rim, band 6 px @35 %).
  No private style constants in primitives.
- The theme observer stays (page-side elements follow Starlight; a re-read on a flip is harmless).
- Test order trap: after any `./dom` test loads, happy-dom's `localStorage` is read-only; `store.test.ts` uses `Object.defineProperty` for that reason.

### Viewport chrome and visual language (Task 6d)

Every scene is a `ViewportCard` (`src/viz/scene/ViewportCard.tsx`), mounted inside `<SceneIsland caption rows?>`:
header (2D/3D tag, Persp/Ortho in 3D, `Overlays` popover, `Reset view`, wide/full buttons (Task 6f below),
scene name, sidebar chevron), the Stage
(viewport), the N-panel (`NPanel.tsx`) and the status bar (`StatusBar.tsx`). Composition API:
`<ViewportCard lesson id name mode extent center? overlays? rows? status? drawIn?>{scene}</ViewportCard>`.
Task 6h adds `look?`, `instruments?`, `angleKey?` and `formula?`; see the reservation contract below.
Demo: `src/viz/demo/engine{,.impl}.tsx` (DEMO-2D, DEMO-3D). Evidence: `.superpowers/sdd/2026-10-10-engine-and-spikes/task-6d-evidence/`
(`final.ts` + `cdp.ts` harness, `final/report.json`, screenshots; `exp*/` = gizmo experiments).

- **One registry per card**: ViewportCard creates `createStage(mode, { lesson, id, overlays })` and passes it to Stage
  (`reg` prop). It carries the card info (`useOverlay`/`<Show ov>`), `hover` (handle/object/field under the pointer,
  for the status bar), `nav` (reset/align/viewDir/redraw, bound by Stage's Nav inside the View), `gizmo` (DOM gizmo
  update, fed per frame by `GizmoSync`) and `reveal` (draw-in progress).
- **Overlay flags** live in the lesson store as 0/1 under `ov:<card>:<flag>` (absent = the declared default), so the
  scene (canvas React root) reads them like any value; toggling hides objects and never changes lesson values.
- **N-panel rows** `{ id, label, kind: 'num'|'vec'|'ro'|'bool', key?, spec?, snap?, value?, fmt?, visible?, group?, tip?, dims?, chars? }`:
  fields use the ONE scrub engine (`startScrubDrag` in useScrub.ts + `scrubValue`/`keyStep`/`ctrlSnap`/`parseField`/
  `fmtField` in scrub.ts; 4 px per step, Ctrl = `snap` targets or increment (default 10 steps), Shift = 1/10 step
  re-based mid-drag, click or Enter = type (Enter commits, Esc reverts, blur keeps a valid value), arrows/PageUp).
  Touch: fields are `touch-action: pan-y`; a scrub starts only after an 8 px horizontal-dominant move and a vertical
  swipe scrolls the page. `startScrubDrag(e, el, { get, set }, next, onEnd)` owns the writes and the cancel rule for
  Formula, Num and fields alike: `pointercancel` applies no pending frame, restores the value from the press and is
  never a click.
  Read-only rows hover like their scene object. Row ids are term ids (hover <-> hot <-> formula). `bool` rows (6e)
  are checkboxes writing 0/1 (a non-0/1 value throws); `glyph` (lessons spec item 11) is lesson scope.
- **Desktop N-panel = Blender region overlap**: drawn over the right edge of the viewport (200 px, 94 % chrome); the
  scene is re-centered in the free part with `camera.setViewOffset` (Stage `inset`), so labels, hit tests and raycasts
  stay exact. Under 720 px (media query, = spec) the same DOM becomes chips under the viewport that WRAP onto lines
  (Ruling V; no sideways scroller). The reserved height is DERIVED (6e, see "Lesson primitives"): fixed-width chip
  model `scene/chips.ts`; ViewportCard grows the row only as a safety net and never shrinks it. Why overlap:
  the Starlight content column is only 632 px at 1280 px (measured), a side column would leave a 432 px viewport.
  `N` toggles it for the card under the pointer or holding focus.
- **Navigation** (Stage `Nav`): **a plain wheel always scrolls the page (Ruling T)**: DragLayer stops plain `wheel`
  events in the capture phase on `.lab-stage`, before OrbitControls (on the View div) can preventDefault; Ctrl/Cmd+wheel
  and trackpad pinch (ctrl+wheel) zoom, two-finger touch pinch zooms. 2D = Ctrl+wheel zoom (0.5x..8x of fit, relative
  zoom kept on resize), MMB/RMB pan, LMB on empty space does nothing (handles own LMB); 3D = LMB/MMB orbit, Shift+MMB
  pan (three-stdlib OrbitControls swaps ROTATE to pan with Shift), Ctrl+wheel dolly.
  Framing: 2D fit leaves `MARGIN_2D` 16 px around the extent (tick numbers at +-extent stay inside); the 3D home
  camera sits at `fitDistance(FRAME_3D * extent, freeW, h)` along the old three-quarter direction, so the whole
  extent, axis tops and labels are in view at any aspect (closes the 6b framing TODO); ortho fit uses the same radius. MMB `mousedown` is prevented (Chrome's middle-click autoscroll moved the page
  17 px per pan). Persp/Ortho switch hands the pose over (same direction, same apparent size). Reset view and gizmo
  align = smooth view with the shared spring (`motion.ts`, VIEW_MS 250), cut under reduced motion, cancelled when the
  reader grabs the view.
- **Gizmo (fallback order recorded, measured in headless Chrome with several Views, `task-6d-evidence/exp*`):**
  drei `GizmoHelper` + `GizmoViewport` FAIL inside a View of a shared canvas: its Hud renders after the Views without a
  scissor into whatever viewport the last View set (the gizmo of stage B appeared in stage C), renderPriority 1 even
  clears the whole canvas, and it sizes from the View's stale `size`. three `ViewHelper` renders correctly once
  autoClear is off and `location` is fed from the stage rect, but its 128 px size, colors, black negative bubbles and
  constant-speed animation are closure-private. Chosen: the third fallback, a DOM/SVG gizmo (`Gizmo.tsx`, pure
  `gizmoLayout`), crisp, real font, Blender colors, hollow negatives, click again = opposite side. Keyboard: six
  visually hidden `<button>`s in a FIXED order next to the SVG (the bubbles re-stack by depth via DOM order, and
  moving a focused node drops its focus); the focused button's bubble draws the ring.
  2D shows the static `XYCorner`.
- **Grid**: drei `Grid` shader (infinite, `followCamera`, pixel-thin AA lines; 2D no fade, 3D fades at 7 x extent),
  `depthWrite` off; axes, ticks, tick numbers and axis letters are our primitives/labels. Verified at 0.5x..8x.
- **Strokes (6d baseline; superseded by the 6h recipe below)**: `Stroke` (view.tsx) = halo + hot outline + core, all transparent so they sort by render order (three
  draws opaque before transparent; an opaque core would be painted over by its own halo). Arrow heads are concave
  triangles (2D) / cones (3D) with matching outline/halo heads; points are camera-facing discs with a dark rim.
- **Hover priority**: in 2D everything is coplanar, so r3f's hit order is scene order; `useHoverable(lesson, id, rank)`
  lets only the highest `HOVER_RANK` under the pointer claim hot/status (point > beam > guide > arc > fill) and
  re-claims on move.
- **Motion** (`motion.ts`, pure, unit-tested): critically damped `springStep` (exact, frame-rate independent, never
  overshoots), `springProgress` (0..1, exactly 1 when settled), `omegaFor(ms)`, `lerpPose`/`axisPose`/`poseOf`.
  Draw-in (`<ViewportCard drawIn>`, one per lesson): `reg.reveal` 0 -> 1 over DRAW_MS 600 when the stage is 40 %
  visible: beams grow from their tails, fills/arcs fade, points/labels in the second half. Reduced motion: cuts.
- **Fonts**: Astro Fonts API (`fonts` in astro.config.mjs, `fontProviders.local()`, variants parsed from the
  @fontsource CSS by `src/lib/fonts.mjs`), emitted by `<Font>` in `src/components/overrides/Head.astro`; vars
  `--lab-font-prose|ui|mono`. `display: 'optional'` + metric-matched fallbacks: with `swap` the late face re-wrapped
  the intro paragraph (CLS 0.049); with optional, 3 cold loads had 0 shifts and the faces in use. No preload (the
  local provider cannot filter preloads by subset).
- Status bar hints: `status={{ hints: { [handleId|objectId]: Hint }, idle?: Hint, state?: Hint }}` where a Hint is
  device-neutral text (string or `(vals) => string|null`) or `{ mouse, touch }` for instructions (touch: null = use
  the built-in touch hint); never guessed from words;
  built-ins for any handle, field, the gizmo and idle navigation (`DEFAULT_HINTS`). Tooltips (`useTip`, 500 ms,
  title + line + TeX via `renderStaticMath`) on N-panel rows and header buttons.
- [x] Indirect value changes are eased on screen (Ruling U, done in 6e: `vals`/`shown` in the store).
- [x] Formula term tooltip API (Task 7): `tips` prop implemented; actual per-lesson tooltip authoring remains lesson scope.
- [x] Task 6d baseline: `gridMinor` was #444444 (1.17:1 on `vp-bg-top`, tested >= 1.15 and below the major grid); spec table updated.
- [x] Wheel scrolls the page; Ctrl+wheel zooms (Ruling T): `gateWheel` (handles.tsx) stops plain wheels in the
  capture phase; DOM-tested against real OrbitControls (happy-dom's WheelEvent drops modifier init fields: the test
  sets ctrlKey explicitly). Touch devices get `TOUCH_HINTS` and the `touch` side of `{ mouse, touch }` hints.
- Overlays popover closes on a pointerdown whose composedPath excludes it, on Escape, and on focusout with a NON-null
  relatedTarget outside (Tab-out). A null relatedTarget is Chrome's focus fixup when the mouse presses non-focusable
  popover text: closing then would swallow the click (CDP-verified with real mouse input).
- Angle labels pass `sides` (6e; was `wedge`): the wedge is measured on SCREEN (`screenWedge`), then `fitWedge` pushes
  the label out along the projected bisector until its box fits between the projected sides (max 80 px extra).
- Tests import no three/drei (bun would print THREE_CJS_DEPRECATED): pure helpers live in three-free modules
  (`geom.ts` tickValues/tickText, `Header.tsx` Overlays/Icon, Gizmo/NPanel/StatusBar/handles).

### Lesson primitives (Task 6e, lessons spec §5 items 1-9 + Rulings U and the 6d carry-overs)

Demo: three more cards on dev/engine (DEMO-POLAR vertex drags + Ledger, DEMO-SIGN half-planes + Challenges, DEMO-LAMP
lit sphere); rows, derived values and drag mappings of the demo are in the three-free `src/viz/demo/engine.rows.ts`
(imported by the entry for MDX/SceneIsland, by the scenes and by tests). Evidence:
`.superpowers/sdd/2026-10-10-engine-and-spikes/task-6e-evidence/` (`e6.ts` + `cdp.ts`, `final/report.json`,
screenshots 1280 and 360/320; `cls-probe.ts` finds which elements move during a drag).

- **Eased writes (Ruling U)**: the lesson store has `vals` (truth: every number, formula, challenge) and `shown`
  (what scenes draw). `set(key, v, 'ease')` jumps `vals` and springs `shown` with `springStep` (SPRING_MS, keeps
  velocity on retarget, lands exactly, never overshoots); `'cut'` (default) moves both. Scene code reads `shown`
  automatically: Stage provides `ShownCtx`, and `useVal`/`useVals`/`usePos`, `pick`, labels and the focus ring read
  it. Ease: keyboard nudges of handles, N-panel arrows/step arrows/typed values, Formula/Num arrow keys. Cut: pointer
  drags and scrubs (1:1), checkboxes, overlays. Reduced motion = cut. A requestAnimationFrame that calls back
  synchronously (test stubs) is detected in `nextFrame` (it would recurse into a stack overflow).
- **Custom drag mapping**: `Draggable.move?(vals, raw, src: 'drag' | 'key') => Move` with
  `Move = { set: Record<string, Val>; ease?: string[] }` replaces `moved`. ONE write path for drag and key:
  `reg.apply(d, vals, raw, src)` = mapping (or grid `moved`) -> `clampMove` to the card's `ranges` (from the N-panel
  rows, `rowRanges`; a drag can never leave what a field of the same key allows, which the chip model relies on;
  component-wise, except a vector key in `ease` (magnet-caught direction): scaled uniformly so a·b = 0 survives) ->
  `writeMove(lesson, set, how, ease)` (drag = cut, key = ease, keys in `ease` always ease). A mapping lists a key in
  `ease` when `magnet()` changed it: a pointer-drag catch glides while the other keys follow 1:1; the repeated caught
  value is skipped (`sameVal`) so the spring keeps running, leaving the zone is a cut. Pressing a glyph mid-ease cuts
  the ease of its keys first (it is grabbed where it is drawn). Point takes `move` and `keys`, and `at` may be a
  function of the values (a derived vertex); Arrow takes `move` (tip/whole).
- **Magnet** `magnet(v, targets, zone, from?)` (drag.ts): nearest target within the zone. TRAP: a keyboard step of 0.1
  is smaller than the zone (1.4 degrees at radius 4 vs 2.5), so a plain magnet pulls every key step back onto the
  target it left: pass `from` (the current value) for `src === 'key'`; the magnet then only catches steps that move
  toward a target. Vector magnets (DP-2 b to the perpendicular) need the same rule (`moveSb` in the demo).
  AngleArc `ticks` = 6 px radial marks (pixel-stable via `useWorldPx`) at the targets, from `from` toward the arc side.
- **HalfPlane** `{ lesson, id, normal: V|key, side, role, opacity? }`: 2D = square of 50 x extent clipped by the line
  through O (`halfPlanePoly`), 3D = single-sided disc of radius `extent` perpendicular to the normal (`pos` = face seen
  from +normal). Render order -1, no depth writes, hot doubles the opacity. `reg.extent` is set by Stage.
- **LitSphere** `{ lesson, id, center, r, light: key, mode, roles: { base, dark?, neg? } }`: ShaderMaterial, exact
  per-fragment sphere normal, point light uniform from the store (shown values), `mix` in sRGB values with NO
  colorspace chunk, so the pixel encodes N·L linearly. CDP: 16 probes (lit, near the terminator at N·L 0.07, unlit,
  raw negative) all within 0.42/255 of `dark + (base - dark) * max(0, N·L)` (raw: `mix(dark, neg, -N·L)`). Method:
  P's ring gives its exact projected px; the pixel is sampled after P was moved away (its glyph covers it).
- **Circle** `{ lesson, id, center, r, normal?, role, dashPx? }` (values or functions of vals): 64-segment closed loop
  (`circlePoints`, basis from `planeBasis`); r < 1e-9 draws nothing; 3D requires `normal`.
- **Arrow legs**: one hoverable dashed leg per component, axis-coloured, ids `${id}:x|y|z` (exported `legSegs`).
- **Ledger** (DOM SVG, page-side theme) `{ lesson, items, total, scale, max, zeroLabel?, fmt?, label }`: pure
  `ledgerLayout` + `spreadLabels`; negative last item = hatched band hanging from the stack top with a down arrow,
  zero = `light` line + zeroLabel. Moving parts are placed with CSS `translateY` (y attributes were layout shifts).
- **Challenge** (DOM) `{ lesson, n, text, hint?, success?, check(vals, step) }`: sticky; stored as
  `challenge:<lesson>:<n>` under KEYS.challenges (`src/lib/challenges.ts`, separate from progress.ts so scene bundles
  do not pull course.json); shows stored success after hydration. Step tasks read `step['<timeline>:max']` (Task 7's
  Timeline writes it).
- **Chip row reservation (derived)**: rows are declared once (three-free module) and passed to BOTH
  `<SceneIsland rows={ROWS}>` and the ViewportCard; `chips.ts` computes each chip's width from the row (label length
  and value budget in 7 px characters: num/vec from the spec's range ends and `fmt`, ro from `chars` (default 7),
  vec needs `dims`), greedy-wraps at the 320 px basis and gives the height (`1 + 2*5 + 20n + 4(n-1)`). NPanel sets the
  widths/gaps as CSS variables (one source), chip labels are mono 11 px. The card throws if its rows give another
  line count than the frame (`chipLines` = explicit override). CDP: lines and height equal the model at 320 px for
  all 6 cards, fit at 360 px, and 0 layout-shift entries during drags at 320/360.
- **No layout shift from live text**: N-panel values sit in `.lab-np-val`, placed by a transform from a fixed anchor;
  a centred or right-aligned text whose width changes moves its own edge, which Chrome reports as a shift (measured:
  one entry per drag). Same for the Challenge status and the status bar (keyed by text). NPanel reports a value wider
  than its box once per row (`console.error`): raise `chars` or shorten `fmt`.
- [x] Formula: a centred display formula whose live number changes width shifted its line (measured during a drag of
  the 2D demo, `cls-probe.ts`; excluded from CLS by hadRecentInput but an entry). Task 7 anchors `\s{}` numeric widths;
  raw SSR math/font/media shifts remain open for reference C7/C9.
- [ ] Point sizes (DP-4 "sun glyph, larger", LC-0 small feet D/E/F, LC-3 larger joint) and the `nl_light` typed-value
  clamp (outside the sphere) are lesson-task scope (Point `size`, a row-level constraint).

### Holographic and clay rendering (Task 6h, accepted)

Task 6h is accepted through `90c522a`: initial implementation `f295950`, fix 1 and fix 2 have completed independent
review. This acceptance covers Task 6h, not the whole engine or course. Original report and evidence:
`.superpowers/sdd/2026-10-10-engine-addendum-6d-6e/task-6h-report.md` and its sibling `task-6h-evidence/`.
Sibling `task-6h-fix1-report.md` and `task-6h-fix2-report.md` supersede the initial measurements.
`task-6h-rereview1.md` closed I1–I6/M1–M3 but found N1; `task-6h-rereview2.md` closes N1 with
0 Critical / 0 Important / 0 Minor findings. No Task 6h finding remains open.

- **Palette / glow**: `ROLE_VARS` now includes stars, nebulae, vignette, floor, formula plate and clay roles. Holo
  backgrounds are #0c1220 / #05070f; major/minor grids #1e2d4d / #141c30; vector A/B/C are
  #4fd8ff / #d85cc8 / #3df0b4. `STYLE` defines full stroke widths 2 px core, 7 px inner glow at .38, 18 px
  outer glow at .10. The core mixes 35% white into the role color; additive glows overlap (inner contribution .48).
  Pixel probes must use actual half-widths: inner sample radius 2 px, outer 7 px, background beyond 9 px.
  Arrow heads share the filament recipe; glass edges use their specific .30 inner alpha. Reduced motion keeps
  the hot state static and retains every visual layer.
- **Glass / measurements**: `Quad` lazily loads `Glass` for the .12-to-.06 fill gradient, world-space subdivisions
  (hidden below 14 px per world unit), luminous edges/corners and sign hatch. Its loading material is transparent;
  Glass invalidates on mount so demand rendering does not leave a white fallback frame. `Ruler` and `Dimension`
  add measurements and filled arrowheads. Camera/angle instruments use actual scene state, with derived row readouts.
  Grid coordinates project absolute world positions onto an orthonormal plane basis (global XY in 2D), preserving
  half-unit world phase under skew/translation; fill-gradient UVs are separate. Dimension constructs its 14 CSS-pixel
  perpendicular offset on screen and unprojects each endpoint at its depth; invisible/degenerate projections hide.
  Instrument azimuth uses camera world direction, so pan preserves it and orbit changes it.
  Two framed miniature galaxy panels and the ringed orientation planet remain part of the instrument strip.
- **Mobile reservation API**: pass `instruments` and optionally `angleKey` to `ViewportCard`; pass `formula`
  as a React node for the chamfered `FormulaPlate`. Match these with boolean `hud` and `formula` props on
  `SceneIsland`, alongside the same `rows` array. Under 720 px the server frame reserves 100 px for instruments,
  or 176 px for instruments plus formula. Keep this pairing when authoring the combined layout; it prevents
  hydration shifts. Instrument decoration is pointer-transparent so it does not intercept vertex drags.
  The mobile ruler has its own 44×80 overview coordinates and 11 px text; clay strip/readout surfaces are light.
  Tall formula content scrolls within its plate; the reported wheel/End check is not a browser/text-zoom or physical
  phone-swipe test, and focusable scrub-term access in a tall plate remains unverified.
- **No automatic omissions**: phones and low-power devices keep backgrounds, galaxies, instruments, glass and floor
  elements. Do not branch on `hardwareConcurrency` or silently hide reference elements at breakpoints. Existing
  reader-controlled overlay flags choose visibility; reduced motion changes motion, not content.
- **Mirrors**: `MirrorCtx` suppresses handle and label registration and hover claims; reflected descendants also
  receive no-op raycasts. Reflection material shaders fade by world depth, with the floor-plane veil from
  `FloorFade` (which wraps `Floor`, not an extra screen-space quad). The [1,-1,1] scale has a negative determinant:
  only `LineMaterial` and `CircleGeometry` reflection materials are forced to `DoubleSide`. This retains elevated
  Point/halo/glass-corner billboards while preserving clay FrontSide surfaces and BackSide hot outlines. Reflected
  clay surfaces write depth before the outline. Never restore an unconditional side override. Fix 2 records top/elevated
  point probes and retained clay-outline checks; this does not establish exhaustive primitive coverage.
- **Clay**: `look="clay"` selects the separate light studio. Lazy `ClayTile` solids have rounded edges, matcap
  shading, sign hatch, outline and footprint shadows; bevel radius is .06 times the smallest side. Subject guards
  cover Arrow, Segment, Point, Quad, Parallelogram and AreaTile before degenerate exits. Clay solids and measurement
  guides such as Grid/Ruler are allowed. `public/matcap-clay.png` is a deterministic generated lookup (4,276 bytes), not a new dependency.
- **Lazy boundaries / demos**: Stage → HoloLayer, Quad → Glass, and clay demo → ClayTile are dynamic imports;
  keep the SSR-safe entry/implementation pattern and no `manualChunks`. `src/viz/demo/holo{,.impl}.tsx` adds
  DEMO-HOLO (existing law-of-cosines values and vertex drags), DEMO-FLOOR and DEMO-CLAY. The dev/engine page now has
  nine cards on one shared demand canvas; hidden dev/holo-probe supplies isolated pixel measurements.
- **Atmosphere**: continuous multi-scale cloud/dust fields, irregular spiral arms and luminous cores replace the
  initial patterned rings; miniature panels use diffuse wisps. Seeded NDC imagery remains static through zoom/orbit.
  This is a procedural interpretation of the reference, with all chosen elements retained, not a photographic copy.
- **GLSL / accounting**: `src/lib/compact-glsl.mjs` compacts the pinned Three module's shader whitespace; tests
  preserve lexical tokens and preprocessor directive interiors/line breaks for all 86 selected strings. No Three
  alias, dependency addition, feature removal, `manualChunks` or raised raw warning threshold is used. Hidden source
  maps support exact generated-byte ownership accounting in `task-6h-evidence/audit.ts`; they are attribution evidence.
- **Reviewed evidence**: production headless Chrome at 1280/360 px, real CDP drag/hover/orbit,
  reduced-motion layout checks, glow/glass/matcap/reflection probes and mirror DOM tests. Fix 2 records 189 passing
  tests, check over 110 files with 0 errors / 0 warnings / 0 hints, and a 20-page warning-free build. These are implementer results, not
  rerun by this documentation update. The independent reviewer inspected supplied gate completions, reproduced the
  ownership audit read-only, and checked focused culling/material evidence; broad gates were not independently rerun.
  Physical-device coverage, exhaustive reflections and long GPU stress remain
  unproven. Procedural galaxies/SVG panels are stylized, not pixel-identical reference reproductions. Task 6h did not establish
  bilingual UI; the later accepted Task 6g implementation is recorded below.

**Accepted Task 6h measurements (`90c522a`), independently reproduced ownership audit:**

| Measurement | Gzip bytes | Scope |
| --- | ---: | --- |
| Attributed reusable core | 299,949 | Ownership metric; 51 bytes below 300,000, not independent network transfer |
| Full physical scene-entry closure | 453,267 | Includes React/KaTeX; excludes HTML/search/fonts/texture |
| Preserved `cb0042d` baseline closure | 437,679 | Increase 15,588 bytes against the 18,000-byte delta limit |
| Actual renderer-containing chunk | 271,291 | 993,290 raw bytes; whole-chunk transfer |

Attribution retains every reusable module/dependency and unbounded/inter-module glue; exclusions are actual
demo/lesson payload and external React/react-dom/scheduler/KaTeX. Hidden maps identify exact emitted intervals;
same-source enclosed unmapped runs count with that source. Retained intervals are concatenated in original order
per chunk and gzipped, not allocated proportionally from mixed-chunk gzip. `audit.json` and `attributed-*.js` retain
the evidence. The full physical closure remains the transfer measurement; filename subtotals are superseded.
The historical 51-byte attributed margin was fragile. Later Task 6g budget-authority clarification supersedes the
controller-added 300,000-byte ownership aggregate hard gate; original physical limits remain unchanged (see below).
`chunkSizeWarningLimit` remains 1015; it is not a transfer budget.
Task 11's course-wide per-page reporting remains required.

### Bilingual infrastructure (Task 6g, accepted)

English remains Starlight's root locale; Russian routes are `/ru/` inside the deployment base. Source
`2020a93` + `b71eb1b` ports the reference course's locale rules and dictionary-slicing design; `df00473` fixes the
real translation-parity guard. Task 6g is accepted after independent initial review and scoped rereview of I1
(`task-6g-review.md`, `task-6g-rereview1.md`: 0 Critical / 0 Important / 0 Minor remain). This accepts infrastructure,
not full Russian lesson prose or the whole engine/course. Implementation and evidence:
`.superpowers/sdd/2026-10-10-engine-addendum-6d-6e/task-6g-report.md` and sibling `task-6g-evidence/`.

- **Locale API**: `src/lib/course-rules.mjs` supplies `Locale` (`en | ru`), `asLocale`, `fromStarlight`,
  `localePrefix`, `splitLocale`, `localizedUrl(base, path, locale)` and `courseText`. Invalid locale or missing
  translated manifest fields throws; English manifest fields stay top-level, Russian fields live in `ru`.
  Use `pageLocale(Astro)` from server-only `src/i18n/ui.ts`. Browser/shared rendering imports dictionary-free
  `src/i18n/core.ts`: `t` for plain text, `th` for HTML keys with escaped placeholders, `msg`/`tm` for deferred
  messages, `docLocale()` for DOM scripts. Never value-import `ui.ts` into a client graph; type-only imports are erased.
- **SSR and islands**: MDX uses `EngineDemo.astro` / `Probe.astro` wrappers. Astro resolves locale, translated captions
  and row reservations. Pass serializable locale to localized React entries; reconstruct row callbacks with shared
  locale-aware factories, rather than serialize functions. Preserve SSR-safe entry → lazy implementation boundaries.
  `LocaleRoot`, `localized` and `useLocale` in `src/i18n/react.tsx` provide explicit locale without a hydration
  effect/server-global locale. Stage re-provides `LocaleContext` **inside drei View**; its tunnel does not carry
  surrounding context automatically. DOM-only `client:load` controls must render the right locale in SSR and their
  first hydrated render.
- **Exact namespace contract**: `UiStrings` emits each declared namespace once per page, for that page's locale only,
  using `Astro.locals` to deduplicate. JSON escapes `<` so text cannot close the script. Namespace declarations must
  equal the full reachable client module graph, including lazy implementations, row factories, reexports and workers;
  this is module-graph coverage, not export-level tree-shaking. The scanner follows TS/TSX/MJS/JS value imports and
  excludes type-only imports. Missing namespaces/keys/placeholders fail instead of silently rendering English.

| Wrapper/widget | Required `UiStrings ns` set |
| --- | --- |
| EngineDemo | `viewport, gizmo, hud, status, challenge, rows, demo, holo` |
| Probe | `viewport, gizmo, status, hud, probe` |
| LessonDone | `done, progress` |
| Quiz | `quiz` |
| Sidebar progress script | `progress` |

EngineDemo's Demo2D/3D/Cosines/Polar/Sign/Lamp/Holo/Floor/Clay roots use `client:only="react"`;
DemoDotFormula/Ledger/Challenges/VecReadout use `client:load`. Every localized root receives explicit locale;
DemoVecReadout displays mathematical values only and needs no locale prop.

- **Semantic invariants**: translate labels, groups, hints, tooltips, captions, messages and accessible names; never
  translate lesson/card/row/handle/term IDs, numerical store keys, ranges, mappings or TeX. Tests lock all six row ID
  lists, including `light`/`bright`. Status hints use explicit `[N]` tokens, not English-word detection. Mathematical
  geometry/store functions remain unchanged. The consolidated N-panel observer preserves desktop inset and the
  never-below-server mobile chip reservation.
- **Fallback inventory**: all 19 current MDX bodies remain English Starlight fallbacks with a Russian notice.
  `src/i18n/untranslated.json` is exact: `course/roadmap`, `dev/engine`, `dev/holo-probe`, `dev/math`, `glossary`,
  `ground/law-of-cosines`, `ground/powers`, `ground/pythagoras`, `ground/squares-and-plane`, `ground/unit-circle`,
  `index`, `vectors/basis`, `vectors/cosine-similarity`, `vectors/cross-product`, `vectors/dot-product`,
  `vectors/length`, `vectors/projection`, `vectors/vector`, `vectors/vector-arithmetic`.
  Manifest/navigation, roadmap component content, widgets and engine presentation are translated; full Russian lesson
  prose is a separate reviewed pass. Fallback body links retain English targets; generated localized hero/sidebar/
  previous-next links preserve the selected locale. Static 404 has recovery links in both languages.
- **Translation validation**: preserve `translatedFrom` Git blob stamps using `scripts/i18n-stamp.ts`; inventory,
  stamps, MDX props/code/link structure and math expressions are checked. The real Russian-page gate calls shared
  `comparePages()` with ordered math comparison. Correctly stamped complete-page fixtures accept translated prose
  with unchanged TeX and reject changed inline/display TeX; removing the comparison makes the parity fixture fail
  (mutation evidence assessed in rereview). New tests cover key/placeholder/tag parity,
  escaping, missing slices/keys/placeholders, fresh dictionary-free browser instances, deployment-base routes and
  semantic identity, row callback/TeX invariants, negative transitive dictionary/missing-HUD fixtures and changed-TeX
  rejection. Actual `renderToString` → `hydrateRoot` EN/RU Formula/Challenge tests check hydration parity.
- **Reviewed gate evidence**: Bun 1.4.3; 221 tests pass, 0 fail, 1,980 expectations across 33 files; Astro check
  over 125 files has 0 errors / 0 warnings / 0 hints; production build emits 39 pages with 0 warnings. Existing
  interaction, geometry, material, contrast, drag/cancel/easing, chip, shader and palette checks remain in the suite.
  Reviewers assessed supplied gate/browser logs and the mutation result; they did not rerun broad gates or browser
  checks. Required gates are run by the controller before the documentation commit.
- **Browser evidence and limits**: separate `browser-en.json` / `browser-ru.json` and screenshot folders supersede
  exploratory combined runs. Each locale covers 1280/360/320 px no-JS SSR and cache-disabled cold hydration: one
  canvas/nine cards, zero layout shifts/overflow and matching reserved heights. Real CDP keyboard editing to θ=90
  completes a challenge and updates HUD; N toggles desktop inset; touch emulation selects touch hints; context loss
  displays nine localized no-WebGL notices. This is headless Chromium/SwiftShader, not physical touch, multi-browser,
  multi-GPU or long-duration stress certification. No hydration/shader errors were observed; known favicon404 and
  THREE.Clock noise remain. Preserve wider course verification requirements.

**Task 6g transfer accounting — authority correction, not a larger physical limit:**

| Final reported metric | Bytes | Meaning |
| --- | ---: | --- |
| Actual renderer-containing chunk | 270,880 gzip / 992,372 raw | Physical viz-core; original limit 300,000 gzip |
| Largest other physical dependency (palette/KaTeX) | 79,596 gzip | Other-chunk limit 100,000 |
| Full transitive physical scene-entry JS closure | 453,038 gzip | Honest dependency closure, not a 100 kB lesson-payload pass |
| Reusable ownership aggregate | 300,416 gzip | Diagnostic; exceeds former extra aggregate gate by 416 |
| Inline EN slices | 7,226 raw / 2,696 gzip | Report separately from JS closure |
| Inline RU slices | 10,243 raw / 3,527 gzip | Report separately from JS closure |

The sibling `task-6g-budget-ruling.md` identifies original Task 11 viz-core as the **physical renderer-containing chunk**.
Its 300,000-byte limit, other chunks ≤100,000, lazy elk/xyflow ≤600,000, existing ≤18,000-byte increment and raw warning
threshold 1015 remain unchanged. Renderer delta from accepted 6h is −411 bytes; no elk/xyflow chunks are introduced.
The controller-added 300,000-byte ownership aggregate hard gate is superseded, not passed or relabeled as transfer.
Keep all shared source/glue counted; no new exclusions. If a separately user-approved aggregate ceiling is found,
restore it and resolve the conflict with the user. Task 11 must explicitly reconcile the distinct per-lesson ≤100 kB
payload gate with physical chunk limits and full downloads; no filename-only accounting or omission of dependencies.

The source-map ownership method stays diagnostic: exact retained intervals are concatenated and gzipped per chunk;
`audit.ts` / `audit.json` retain accounting, while `physical.json` records real chunks and inline slice transfer.
Actual savings include whitespace-only GLSL compaction with all 86 shader token/directive sequences preserved,
selected status fallback lookup and one panel observer. No decimal-float rewrite remains; no feature removal,
warning suppression, `manualChunks`, ownership hiding or physical-cap increase is authorized. The independent review assessed these
measurements; the test-only `df00473` fix leaves production bundles and browser evidence unchanged.

### Viewport size modes (Task 6f, accepted)

Task 6f is accepted through `ce5f0ab22b3d5f7ccdb3748040067b0f713aa4ef` (initial source `c07e004`).
Independent `task-6f-rereview1.md` closes I1 with 0 Critical / 0 Important / 0 Minor new findings.
Reports: `.superpowers/sdd/2026-10-10-engine-addendum-6d-6e/task-6f-report.md`, sibling
`task-6f-fix1-report.md` and `task-6f-evidence/fix1/final/`; final evidence supersedes initial exit measurements.

- **Authority / controls**: `scene/sizeModes.ts` owns one normal/wide/full external store across Astro roots,
  keyed by lesson:card; `useSizeState()` supplies an SSR-normal snapshot. Localized wide/full buttons follow
  Reset view. Ctrl+Space toggles wide; Ctrl+Alt+Space or F toggles full; Escape returns normal.
  Focused card precedes hover; ignore repeated/default-prevented events. Editable controls retain their keys;
  full-mode Tab wraps within card controls.
- **Wide**: widen the actual server figure to `min(2 × normal width, document width − 32px)`, center with
  margin-left and retain the 16:10 viewport plus chip/HUD/formula reservations. Following prose reflows.
  Hide and make inert Starlight `.sidebar-pane` / `.right-sidebar` while retaining their layout allocation;
  restore exact inline visibility value/priority and original inert attribute on exit, owner change or unmount.
  Hover activation moves outside focus to the card and restores it on exit. Add no stacking-context wrapper.
- **Full**: fix the card inset-0 and synchronously request the Fullscreen API on `document.documentElement`
  from the initiating event. Move the existing `.lab-scene-host` DOM node into the card, above its background
  and below chrome; restore its exact original parent/sibling position. Keep the React root, Canvas, Stage,
  registry and lesson state mounted; no per-card Canvas or Stage portal to another root. Explicit View visibility
  prevents inactive sibling Views clearing/painting over the selected full View.
- **Lifecycle / camera**: save/restore body/html overflow, scroll and focus. Missing/rejected API keeps the
  fixed overlay; late request completion after exit/unregister, owner switching and Astro unmount are guarded.
  Keep orbit/pan target, relative zoom, Persp/Ortho, values/step/hot, overlays and panel state. Reader transitions
  use `springProgress` (150 ms parameter, about 200 ms settling), cut under reduced motion; invalidate each frame.
- **External browser exit**: settle normal immediately, cut actual card chrome paint through two RAF callbacks,
  then restore exact inline visibility/priority and connected saved focus. New mode/owner or cleanup finishes
  and cancels pending reveal work. The existing host returns under BODY immediately; Canvas stays visible.
  `Stage.Nav.redraw` synchronously reads the actual DOM box and refits with the same relative-zoom/inset policy
  before invalidation; normal resize fits in a layout effect. This prevents an old full-size fit painting first.
  The brief chrome cut is visible (7–16.2 ms between sampled hidden/revealed frames); two RAFs give no
  wall-clock maximum under browser throttling. No observer gap or synthetic input masks shifts.
- **Reviewed evidence**: six EN/RU ×1280/360/320 production cases retain 120 mode snapshots; 24/24 repeated
  actual uncued exits, six source-rectangle traces and two Ortho/edit/focus/responsive-resize cases pass.
  Geometry pixels, GL/View/Stage alignment within 1 px, header/corner hit tests and real Overlays clicks confirm
  visibility and preservation. Final hydration and delayed external-exit phases have zero raw shift entries.
- **Reported gates**: Bun 1.4.3; 229 tests pass, 0 fail, 2,035 expectations; check covers 128 files with
  0 errors / 0 warnings / 0 hints; build emits 39 pages with 0 warnings. Reviewer assessed supplied logs;
  this documentation update does not rerun gates; the controller runs required gates before its commit.
- **Transfer**: physical renderer 273,319 gzip, largest other chunk 79,596, below unchanged 300,000 / 100,000
  limits. Lazy elk/xyflow 600,000 and raw warning threshold 1015 stay unchanged. Ownership aggregate 302,860
  and full JS closure 455,475 gzip remain diagnostic/closure measurements, not another physical-cap pass.
- [ ] **Baseline media/font stability remains OPEN before reference C7/C9 acceptance**: accepted-6g archive
  controls reproduce EN/RU1280 reduced-motion media reflow; glyph-face cause and individual mobile attribution
  remain unproven. Final media phases retain five no-input entries (1,588 total raw entries across the matrix);
  zero mode-exit shifts do not mean zero-all-CLS. This work does not fix or waive that baseline issue.
- Evidence is headless Chromium/CDP/touch emulation; physical device/multi-GPU coverage remains unproven.
  Size-mode acceptance does not complete the engine, 98-entry course, Russian lesson prose or Task 11 accounting.

### Linked lesson controls and placement (Task 7, accepted)

Accepted at `8b2f0c3b1ec92a775aac0324f2529e8a352a95c7`: BASE `cbfd5e1` → initial `0fb29d6` →
fix1 `afa82e4` → fix2 `8b2f0c3`. Independent `task-7-review.md`, `task-7-rereview1.md` and
`task-7-rereview2.md` close I1/I2/I3/M1 and residual R1; 0 Critical / 0 Important / 0 Minor remain.
Reports and evidence: `.superpowers/sdd/2026-10-10-engine-and-spikes/task-7{,-fix1,-fix2}-report.md`
and corresponding `task-7{,-fix1,-fix2}-evidence/`. This accepts Task 7, not reference lessons or the course.

- **Linked tables/code**: `NumTable {lesson,rows:{label,key}[],op,labels?,marks?}` takes one/two vectors;
  op = components/product/square/diff. Pure `rowsFor` checks finite inputs and product/diff dimensions;
  cells use a0…/p0…/sq0…/d0…, product/square totals use sum. Match IDs across scene, Formula and code in
  the same lesson. `CodeView {code,lang:'python'|'ts'}` stays plain pre/code; linked lines additionally
  require lesson and `lines:Record<number,string>` (1-based). No runtime syntax highlighter.
- **Timeline / hot authority**: `Timeline {lesson,id,steps}` uses nonempty unique
  `TimelineStep {id,text,phases?:number,focus?:string}`; phases are positive integers. Back/Next/range
  set bounded current indices, never autoplay. Reveals use `step[id]`, Challenge history uses
  `step[id+':max']`; `step[id+':phases']` supplies active placement phases. Author focus as a term ID.
  One coordinator per lesson gives first focused mount initial guide ownership, then last changed selection;
  unmount picks the remaining guide by change order. Interaction overrides; empty hot restores current guide.
  `hotSource:'interaction'|'guide'|null` distinguishes authority from visible IDs. `setHot(ids,source?)`
  defaults to interaction; Timeline writes guide. Empty hot has null authority; authority-only changes retain
  Set identity. No-op requires matching IDs and authority. hotOn claims interaction even for a same-ID guide;
  hotOff clears only matching interaction. Step/list/unmount cannot steal stationary same-ID interaction.
  This retains the existing one-visible-hot policy, not simultaneous independent pointer/focus arbitration.
- **Atomic values / cancel**: initialize `lessonStore(lesson,init,links)` once at module scope.
  `setMany(patch,how?,ease?)` resolves LessonLinks before pruning/notification; no-op checks truth, shown
  and active ease. An unchanged-target cut cancels motion and publishes coherent shown values immediately.
  powersLinks bounds x/A/V to 4/16/64, preserves from provenance, rejects conflicts/incoherent defaults;
  canonical shown x derives A=x²/V=x³ each frame. n/k integer adapters synchronize timeline/history without
  fractional exponent easing. Shared Formula/Num/NPanel scrubs use `checkpoint(keys):()=>void` to restore
  relevant truth/shown/ease/provenance/history on cancel; unrelated edits survive, cancel is never a click.
- **Formula**: required lesson/tex/label; optional texNarrow/scrub/fmt/derived/marks/tips/block. Marks map
  term IDs to hidden/struck; tips use `Tip {title?,text,tex?}` and the existing 500ms mouse mechanism.
  Current props update values/slider ARIA; active KaTeX HTML is exposed, hidden terms untabbable, reserve
  clones aria-hidden. Commit snapshots preserve semantic occurrence focus; size modes restore the connected
  node or same stable Formula root/occurrence, never a global search. texNarrow selects below 420px Formula
  width; inactive template reserves height. Scrub min/max/0 formatted widths anchor right-aligned numbers.
- **Poly / fields**: Poly has pure `pts(vals,step)`, optional `path(vals,from,to,progress)`, fixed vertex
  count and finite coplanar 2D/3D points. Value changes cut; step placements spring by phases (reduced motion
  cuts). Degenerate fill/edge hide while label remains; edge solid/dashed/none. Explicit LC paths preserve
  cyclic correspondence, shear area and rigid-angle rotation; debt hatch is independent of winding.
  Poly and Quad/signed AreaTile share CSS-pixel hatch stripes. `FieldCell {lesson,row,size?}` defaults lg,
  reusing RowView/writes/tips/hot. Glyph rect/disc/sum sizes sm32×20/lg64×40; chip width adds sm32+4px.
- **Radial exception**: radialLink declares `LinkGroup.rangePolicy?:'radial'`; stable
  `LessonState.radialKeys:ReadonlySet<string>` feeds clampMove's fourth argument and NPanel. Only these
  keys bypass ordinary component boxes: surface fixes radius, outside enforces minimum radius without upper
  cap. Preserve requested nonzero direction; zero uses prior valid/declared initial direction. Internal
  FieldProps.radial selects finite unbounded parsing/grid/steps and truthful ARIA spinbutton without min/max;
  ordinary fields remain bounded sliders, ordinary clamps/caught scaling remain. Never bypass via easing.
  Large values need honest display/character budgets; measured 103 is not every-magnitude width proof.
  Point.size is a finite positive CSS-pixel diameter. Reference sun/joint/feet authoring remains lesson scope.
- **SSR / locales**: formulaHeight belongs to SceneIsland, not ViewportCardProps: finite ≥76, default 76;
  linked demo authors 116 via inherited CSS variable for SSR/live plate. Reserve before hydration, no later
  grow. Powers page-side Formula is two SSR lines at 320px. Reuse identical SSR/live rows and formula flags,
  one shared demand Canvas, SSR-safe entry → lazy implementation and DOM-only client:load. Explicit EN/RU
  locale and reachable UiStrings namespaces remain required; translate UI, preserve semantic IDs and TeX.
- **Evidence / limits**: final reported Bun 1.4.3 gates: 252 pass / 0 fail / 2,532 expectations; check 154 files
  with 0 errors/warnings/hints; build 41 pages / 0 warnings. Reviewers assessed supplied logs, not broad reruns.
  Renderer 270,182 / largest other 84,115 gzip remain under unchanged 300,000 / 100,000; lazy elk/xyflow 600,000 and
  raw threshold 1015 unchanged; Task11 accounting remains. Fix2 same-id/modes-touch/fingerprint evidence
  covers stationary p0 through keyboard step3 until mouse-out restores distance, native exits/focus/cancel.
  Fix1 theme/radial/reduced-motion evidence is reused only for unchanged code. Raw RU hydration/input shifts
  remain; recent input does not erase them. Exact font cause and dynamic media stability, reference C7/C9,
  physical-device coverage, Russian lesson prose and all 98-entry course delivery stay OPEN.

## Whole-course execution (2026-10-11)

The operator has resumed and authorized full-course implementation. Historical pending Plan 1/repository approval and
pause notes above and in HANDOFF are not current blockers. The executable
[course completion map](docs/superpowers/plans/2026-10-11-course-completion.md) covers all **98 entries** (76 spine,
A1–A10, Q1–Q12), every named scene, bilingual content/UI, Python/Colab, term maps, machine depth, verification and release.

- [x] Whole-course completion/acceptance map written; no curriculum or required scene removed.
- [ ] Execute its prerequisite phases and lesson batches; record evidence per lesson and scene before marking complete.
- [ ] Preserve every chosen holo reference element, including miniature galaxy panels and the planet/ring instrument;
  adapt layout and measure cost instead of silently dropping elements.
- [ ] Complete all required defects even if a review iteration cap is reached; change diagnosis/reviewer, not scope.

Add real lesson content and scenes with manifest entries; do not substitute syllabus stubs for a completed course.
Powers prose and reviewed scene contracts were integrated into feat/engine at `0dca77d`; all 13 English lessons remain
subject to the map's full acceptance contract. Interactive powers scenes are not yet implemented.
