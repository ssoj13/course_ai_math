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

- Language: **bilingual like vllm_en**: English at the root, Russian under `/ru/` (plan Task 6g); chat stays Russian.
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
- `favicon.svg` 404: there is no `public/` dir yet (predates Task 4).
- [ ] Scene chunk `engine.impl.*.js` is 1009 kB raw / 269 kB gz since Task 6d (three core is not tree-shaken under
  r3f's `import * as THREE`; drei Line = three-stdlib Line2/LineMaterial added ~32 kB in 6b; 6c primitives + demo ~9 kB;
  6d chrome + drei Grid ~31 kB raw / 10 kB gz). `chunkSizeWarningLimit: 1015` in astro.config.mjs; the real gate is the per-page gzip budget of
  scripts/size-report.ts (Task 11; Ruling O: viz core <= 300 kB gz).

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
  (in Parallelogram.tsx) is the shared fill (`STYLE.fillA` = 18 % since 6d, polygonOffset so coplanar lines keep their color) + 1.5 px edge +
  6 px hatch; hatch spacing in px comes from `useWorldPx` (quantized React state, rebuilt only on ~9% scale change).
- **RightAngle**: px-sized square, auto-shown within `tol` = 0.5 degrees of 90 (or `show`); hidden for zero vectors.
- Derived geometry (projection foot etc.) is computed in the lesson .tsx with `useVal` inside a child of Stage and passed
  as literals (see `Projection` in demo/engine.impl.tsx); guard zero vectors there (`project` throws on zero).
- Evidence: `.superpowers/sdd/2026-10-10-engine-and-spikes/task-6c-evidence/` (`cdp-6c.ts`, `final/`).
- [ ] Poly / moving rectangles / AngleArc `ticks` for the 0.5 lesson (design doc) are 6e scope.

### Palette and tokens (Task 5, revised in 6d)

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
header (2D/3D tag, Persp/Ortho in 3D, `Overlays` popover, `Reset view`, scene name, sidebar chevron), the Stage
(viewport), the N-panel (`NPanel.tsx`) and the status bar (`StatusBar.tsx`). Composition API:
`<ViewportCard lesson id name mode extent center? overlays? rows? status? drawIn?>{scene}</ViewportCard>`.
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
- **Strokes**: `Stroke` (view.tsx) = halo + hot outline + core, all transparent so they sort by render order (three
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
- [ ] Formula term tooltips (lessons spec) need a `tips` prop on Formula (Task 7 / lessons).
- [x] `gridMinor` is #444444 (1.17:1 on `vp-bg-top`, tested >= 1.15 and below the major grid); spec table updated.
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
- [ ] Formula: a centred display formula whose live number changes width shifts its line (measured during a drag of
  the 2D demo, `cls-probe.ts`; excluded from CLS by hadRecentInput but an entry): Task 7 should anchor `\s{}` numbers.
- [ ] Point sizes (DP-4 "sun glyph, larger", LC-0 small feet D/E/F, LC-3 larger joint) and the `nl_light` typed-value
  clamp (outside the sphere) are lesson-task scope (Point `size`, a row-level constraint).

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
