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

- Language: **English** (chat with the user stays Russian).
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
  Arrow/Point/Segment `stopPropagation()` on pointerover so they win the hover over area fills underneath.
- Axis letters and tick numbers are DOM labels since 6d (self-hosted fonts now exist). `label` on Arrow/Point names
  the keyboard button only; visible text is a separate `<Label>`.
- Evidence: `.superpowers/sdd/2026-10-10-engine-and-spikes/task-6b-evidence/` (`cdp-drag.ts`, `final/`).
- [ ] Default 3D camera (Stage) does not frame the whole `extent`: grid corners and the top of the y axis are cut off.

### Angle, area and label primitives (Task 6c)

- Files: `scene/{AngleArc,RightAngle,AreaTile,Parallelogram,Label}.tsx`; pure helpers in `geom.ts` (`angleArc`,
  `rightOf`, `rectOn`, `isRight`, `hatch`, `cross`) and `drag.ts` (`hitRadius`, `ndcToPx`, `pushAway`).
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
  (in Parallelogram.tsx) is the shared fill (16%, polygonOffset so coplanar lines keep their color) + 1.5 px edge +
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
  edge 1.5 px, fill 18 %, sector 10 %, hatch 6 px / 1 px, head 14 px, point 4.5 + 1.5 px rim, band 6 px @35 %).
  No private style constants in primitives.
- The theme observer stays (page-side elements follow Starlight; a re-read on a flip is harmless).
- Test order trap: after any `./dom` test loads, happy-dom's `localStorage` is read-only; `store.test.ts` uses `Object.defineProperty` for that reason.

### Viewport chrome and visual language (Task 6d)

Every scene is a `ViewportCard` (`src/viz/scene/ViewportCard.tsx`), mounted inside `<SceneIsland caption panel?>`:
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
- **N-panel rows** `{ id, label, kind: 'num'|'vec'|'ro', key?, spec?, snap?, value?, fmt?, visible?, group?, tip? }`:
  fields use the ONE scrub engine (`startScrubDrag` in useScrub.ts + `scrubValue`/`keyStep`/`ctrlSnap`/`parseField`/
  `fmtField` in scrub.ts; 4 px per step, Ctrl = `snap` targets or increment (default 10 steps), Shift = 1/10 step
  re-based mid-drag, click or Enter = type (Enter commits, Esc reverts, blur keeps a valid value), arrows/PageUp).
  Read-only rows hover like their scene object. Row ids are term ids (hover <-> hot <-> formula). `bool` rows and
  `glyph` (lessons spec item 11) are not built: 6e (Toggle) / lesson scope.
- **Desktop N-panel = Blender region overlap**: drawn over the right edge of the viewport (200 px, 94 % chrome); the
  scene is re-centered in the free part with `camera.setViewOffset` (Stage `inset`), so labels, hit tests and raycasts
  stay exact. Under 720 px (media query, = spec) the same DOM is a scrollable chip row under the viewport. Why overlap:
  the Starlight content column is only 632 px at 1280 px (measured), a side column would leave a 432 px viewport.
  `N` toggles it for the card under the pointer or holding focus.
- **Navigation** (Stage `Nav`): 2D = wheel zoom (0.5x..8x of fit, relative zoom kept on resize), MMB/RMB pan, LMB on
  empty space does nothing (handles own LMB); 3D = LMB/MMB orbit, Shift+MMB pan (three-stdlib OrbitControls swaps
  ROTATE to pan with Shift), wheel dolly. MMB `mousedown` is prevented (Chrome's middle-click autoscroll moved the page
  17 px per pan). Persp/Ortho switch hands the pose over (same direction, same apparent size). Reset view and gizmo
  align = smooth view with the shared spring (`motion.ts`, VIEW_MS 250), cut under reduced motion, cancelled when the
  reader grabs the view.
- **Gizmo (fallback order recorded, measured in headless Chrome with several Views, `task-6d-evidence/exp*`):**
  drei `GizmoHelper` + `GizmoViewport` FAIL inside a View of a shared canvas: its Hud renders after the Views without a
  scissor into whatever viewport the last View set (the gizmo of stage B appeared in stage C), renderPriority 1 even
  clears the whole canvas, and it sizes from the View's stale `size`. three `ViewHelper` renders correctly once
  autoClear is off and `location` is fed from the stage rect, but its 128 px size, colors, black negative bubbles and
  constant-speed animation are closure-private. Chosen: the third fallback, a DOM/SVG gizmo (`Gizmo.tsx`, pure
  `gizmoLayout`), crisp, real font, keyboard buttons, Blender colors, hollow negatives, click again = opposite side.
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
- Status bar hints: `status={{ hints: { [handleId|objectId]: string | (vals) => string|null }, idle?, state? }}`;
  built-ins for any handle, field, the gizmo and idle navigation (`DEFAULT_HINTS`). Tooltips (`useTip`, 500 ms,
  title + line + TeX via `renderStaticMath`) on N-panel rows and header buttons.
- [ ] Indirect value changes are not yet eased on screen (keyboard nudges, typed values jump): needs a direct/indirect
  write channel in the store; build it with 6e/7 (magnet catch, timeline steps, Poly tweens), reusing `springStep`.
- [ ] Formula term tooltips (lessons spec) need a `tips` prop on Formula (Task 7 / lessons).
- [ ] Grid tokens: `gridMinor` #3C3C3C is 1.03:1 on `vp-bg-top` #3A3A3A, so minor lines vanish in the upper half of
  a viewport (spec values kept; designer to decide).
- [ ] Wheel over a viewport zooms instead of scrolling the page (spec: "Wheel zoom"); consider Ctrl+wheel if readers
  complain.
