# math-course — working notes for agents

Keep this file current. It is the working notebook: what the repo is, the contracts an implementer must know, authoring
rules, gates, traps and the CURRENT TODO (`[ ]` / `[x]`). Compress outdated notes into the changelog or delete them.
Acceptance evidence, measurements and review histories go to [docs/acceptance/](docs/acceptance/README.md), not here.
`CLAUDE.md` only imports this file (`@AGENTS.md`). Session state and resume steps: [HANDOFF.md](HANDOFF.md).

## What this is

Interactive, visualization-first textbook "Math for AI": law of cosines and dot product → vectors, matrices, tensors →
derivatives, integrals → probability/statistics → learning machines → attention, plus an advanced track and branch Q
(quantization). Format: roadmap.sh-style knowledge map, mind maps everywhere, live linked scenes (inspiration:
poloclub/transformer-explainer, 3Blue1Brown, PyTorch "Inside the Matrix", Bret Victor). Infrastructure is modeled on
`C:\projects\projects.rust.cg\cgprojs\vllm_en` (course.json single source, progress, lesson links): read its AGENTS.md
before building infrastructure.

Reader: CG/VFX developer (Maya, Houdini, shaders) who asks for very gentle, step-by-step explanations: every symbol in
words, a number after each formula, CG analogies (N·L, xforms, Graph Editor curves). User's own words: `task.md`.
Scope and acceptance: [course completion map](docs/superpowers/plans/2026-10-11-course-completion.md) (98 entries:
76 spine, A1–A10, Q1–Q12, every named scene); program: `CURRICULUM.md`; sidebar: `src/data/course.json`.

## Toolchain

- **bun 1.4.3 required** (`packageManager`). bun 1.3.12 crashes on Astro React SSR (`react-dom/server.bun.js`:
  "Expected CommonJS module to have a function wrapper"). The build runs on bun, no Node.
- TypeScript pinned to 6.x: `@astrojs/check` 0.9.10 peers `^5 || ^6` (TS 7 unsupported). `tsconfig.json` sets
  `"types": ["bun"]` because TS 6 defaults `types` to `[]` and `bun:test` would not resolve.
- Versions (checked 2026-10-10): astro 7.3.8, @astrojs/starlight 0.42.6, @astrojs/react 7.0.1, react 19.3.0,
  @react-three/fiber 9.8.1, @react-three/drei 10.7.9, three 0.186.1, @xyflow/react 12.12.0, elkjs 0.12.0,
  zustand 5.0.15, katex 0.19.0, pyodide 314.0.7, @astrojs/markdown-satteri 0.4.3.
- CI: `.github/workflows/deploy.yml` on `ubuntu-24.04` (pinned; `ubuntu-latest` moves to 26 on 2026-10-19).
  Repo `ssoj13/math-course` (public), Pages https://ssoj13.github.io/math-course/. Working branch `feat/engine`.

## Decisions

- **Bilingual like vllm_en**: English at the root, Russian under `/ru/`; chat with the user in Russian.
- **GitHub Pages**, static only. **Python**: Pyodide cells (lazy, on click) **and** Colab, one notebook source.
- Statistics/probability module is **mandatory**, spine "why squares everywhere" (below). **Mind maps everywhere**
  from one term graph.
- Stack (approved): Astro + Starlight; React islands; r3f + drei (2D and 3D in one engine); @xyflow/react + elkjs
  (maps, computation graphs); zustand; KaTeX; TS `core/` (Tensor with shape/strides + reverse-mode autodiff that
  records the graph we draw; not implemented yet). Rust/wasm only where a measurement proves JS too slow.
- **Look**: scenes and covers follow `docs/design/refs/holo-style-ref.png` EXACTLY, nothing removed (galaxies and
  starfield, glow lines, glass squares with grid, HUD instruments on both sides, rulers/ticks, mirror floor, formula
  plate); `clay-style-ref.png` is the secondary look. Blender chrome (header, N-panel, gizmo, status) stays. If budget
  or readability conflicts with the reference: stop and ask the user, never tone down or reinterpret silently.
  Spec: `docs/design/visual-language.md` (v3 holo + clay).
- **Budgets** (Ruling O, plan Task 11): physical renderer-containing chunk ≤ 300,000 B gzip; every other chunk
  ≤ 100,000; lazy elk/xyflow ≤ 600,000; ≤ 18,000 B increment per task. `chunkSizeWarningLimit: 1015` is a warning
  threshold, not a budget. The byte-ownership aggregate is diagnostic only (it was a controller-invented gate, see
  [6g](docs/acceptance/6g.md)). Task 11 must reconcile the per-lesson ≤ 100 kB payload gate with physical chunks.
- Reuse from vllm_en: course.json → sidebar/numbering/links (`course.ts`, `remark-lesson-links.mjs`,
  `route-links.ts`), `progress.ts`, sidebar override, notebook generator (`notebooks/build/`), `numfmt.ts`
  (FP8/NVFP4/MXFP4, branch Q), quiz-lint (not ported yet).

## Core principles

1. Every number is visible and linked: hover a matrix cell → arrow in scene, formula term, code line light up.
2. Every animation is a scrubbable timeline (step / back / scrub), not a video.
3. Scrubbable numbers in formulas drive the scene.
4. No fake data: the drawn computation graph is the autodiff graph that computes the gradients.
5. Each concept has a **depth stack**: intuition → formula → code (NumPy/TS) → machine (dot product = FMA chain /
   SIMD lanes / summation-order rounding; matmul = tiled GEMM / tensor cores; tensor = strides over flat memory;
   derivative = autodiff tape vs finite differences).
6. Each advanced topic is anchored to a real AI mechanism (LoRA = low rank/SVD, RoPE = rotations, √d in attention =
   variance of a dot product, LayerNorm = projection + scale, flows = Jacobian determinant).

## Term graph (planned: `src/data/terms.json`)

Nodes = terms with a one-line "X is Y built on Z" definition and depth level; typed edges `built-on`, `is-a`,
`special-case-of`, `equivalent-to`, `used-in`. Views: course roadmap, per-lesson local map (1–2 hops), genealogy down
to the roots, depth stack, glossary, auto flashcards ("variance is built on ___"). Build-time validation: all refs
exist, no cycles in `built-on`. Example: attention → softmax + dot product → cosine → law of cosines → Pythagoras.

Implemented (Task 9, 2026-10-11):
- `src/data/terms.json` (62 terms: Part 0/1 + forward terms `float`, `matrix`, `determinant`, `attention` with
  `lessons: []`). English fields top-level, Russian under `ru` (course.json convention). `lessons[0]` = where the
  term is introduced. Depth: ground | vectors | matrices | ai | machine. Edges are stored once on the term they read
  from (`X built-on Y`); `is-a` counts as a foundation edge with `built-on` (acyclic, genealogy follows both); a
  `used-in` that restates a reversed `built-on` is rejected; `equivalent-to` is stored on one side only.
- `src/viz/terms/graph.ts` (pure: validate, ancestry, neighborhood, genealogy, relations), `data.ts` (server/tests:
  parsed graph, `mapData` resolves locale texts + lesson/glossary URLs; islands never load terms.json),
  `TermMap.tsx` (SSR entry: card, legend, accessible list view; `local` | `genealogy` mode), lazy
  `TermMap.impl.tsx` (React Flow) + `layout.ts` (ELK protocol client) + `elk.worker.ts`.
- TRAP: `elkjs/lib/elk.bundled.js` cannot run inside a Web Worker (its embedded worker build hijacks `self.onmessage`
  and its constructor throws). The worker imports `elkjs/lib/elk-worker.min.js` (installs ELK's dispatcher) and
  layout.ts speaks its `{ id, cmd }` protocol, registering `layered` first.
- Placement: `<LessonMap />` under `## Term map` / `## Карта терминов` at the end of every lesson (route-derived
  lesson; Russian fallback pages get a Russian map); `<Glossary />` in `glossary.mdx` + `ru/glossary.mdx`.
  `tests/js/terms.test.ts` requires both; `UiStrings ns={['terms']}`.
- Lesson card (fix round 1, controller ruling I2): focus = terms the lesson INTRODUCES (`lessons[0]`); the card draws
  them + their direct foundations (`foundationsIn`; machine terms not expanded), <= 12 nodes, box height from
  `layerCount` (`mapHeight`, server-reserved via `--lab-tm-box`). Full screen + list = the 1-hop neighbourhood.
  Labels > 12 chars wrap onto two lines (`wrapLabel`, shared by server height and client boxes). Opening zoom floor
  0.88 (10.6 px labels): below it the card opens on the focus terms; one-finger drag pans inside the box (touch too).
  Genealogy always opens on the whole tree (floor 0) and also shows the foundations of its machine terms.
- `validate()` enforces course order: no foundation edge to a term introduced in a later lesson.
- TRAP: React Flow store reuse across a genealogy retarget fitted the OLD nodes; the provider is keyed by the view
  and the opening view goes through React Flow's own `fitView` + `fitViewOptions` (`openView`).
- ELK `nodePlacement: NETWORK_SIMPLEX` packs layers tighter than the default (measured on the DP card).
- Budget (`bun run build`, 2026-10-11): elk worker 439 kB gz + map impl 60 kB gz = 499 kB lazy (<= 600 kB); entry
  2.7 kB gz + 3.7 kB gz CSS on map pages only; no map code on pages without a map.
- [ ] Depth stack view, flashcards and the roadmap overlay (C6) are not built yet (out of Task 9 scope; C6 open).

## "Why squares everywhere" (statistics spine)

1. Pythagoras: Euclidean distance; squared error = squared distance between data and prediction vectors.
2. The mean minimizes Σ(x−c)² (median minimizes Σ|x−c|): smooth parabola, unique minimum.
3. Differentiability: d/dx x² = 2x vs the kink of |x|.
4. Covariance = inner product of centered vectors; correlation = cosine; uncorrelated = orthogonal; variances of
   independent variables add = Pythagoras.
5. Gaussian: −log density = squared error, so MSE = maximum likelihood under Gaussian noise.
6. Decompositions are Pythagoras of a projection: total = explained + residual (R²); bias² + variance.
7. Honest downside: outlier sensitivity → L1, Huber.

## Architecture contracts

### Scene host and lesson islands ([spike A](docs/acceptance/spike-a.md))

- One shared `<Canvas flat frameloop="demand">` per page (`src/viz/scene/SceneHost.tsx`); every `Stage` is a drei
  `<View>` in its own Astro island. The first Stage calls `ensureSceneHost()`, which appends
  `<div class="lab-scene-host">` to `<body>` and mounts the Canvas as its own React root. Pages without stages load no
  three/drei and create no GL context. No `manualChunks`, no per-stage Canvas.
- `flat` = no tone mapping, so palette colors render exactly. The drawing buffer is not preserved (Views need no clear).
- `FitView` refits from a ResizeObserver on the stage element (drei's View captures `size` once at mount). Ortho
  zoom = `min(w,h)/(2*extent)`.
- **Lesson pattern**: MDX imports an SSR-safe entry `src/viz/lessons/<slug>.tsx` (module-level `lessonStore(...)`,
  DOM-only controls, `lazyIsland(() => import('./<slug>.impl').then((m) => m.X))` from `src/viz/island.tsx`); scenes
  with three/drei live in `<slug>.impl.tsx`. Scenes mount `client:only="react"` inside
  `<SceneIsland caption="...">` (`src/viz/SceneIsland.astro`); DOM-only controls use `client:load` so they take their
  space before hydration. Why: MDX keeps `client:only` imports in the server module, and drei/fiber CJS `main` calls
  `require("three")` → `THREE_CJS_DEPRECATED` at prerender.
- `SceneIsland` renders the skeleton `.lab-vp-skel` (28 px header, 16:10 spacer, chip rows under 720 px, 22 px status)
  so nothing shifts on hydration; the island `.lab-vp-card` fills the same box. Accessible name = server figcaption.
- **Stacking**: Starlight's `.main-pane` has `isolation: isolate`; lab.css opts out with
  `.main-pane:has(.lab-vp) { isolation: auto }`. Chrome over the viewport uses z-index 1..6. Never add
  transform/filter/opacity/contain/container-type around a card: each creates a stacking context and puts chrome
  under the canvas.

### Store, links and writes (`src/viz/store.ts`, `src/viz/links.ts`)

- `lessonStore(lesson, init, links)` once at module scope. State: `vals` (truth: numbers, formulas, challenges),
  `shown` (what scenes draw), `hot` + `hotSource`, `radialKeys`, `step`.
- `set(key, v, how)` / `setMany(patch, how?, ease?)`: `'cut'` (default) moves both; `'ease'` jumps `vals` and springs
  `shown` (`springStep`, SPRING_MS, keeps velocity on retarget, lands exactly). `setMany` resolves `LessonLinks`
  before pruning/notification; no-op checks truth, shown and active ease; an unchanged-target cut cancels motion.
- Ease for keyboard nudges, N-panel arrows/typed values, Formula/Num arrow keys; cut for pointer drags and scrubs,
  checkboxes, overlays; reduced motion = cut. Scene code reads `shown` automatically via `ShownCtx` (Stage provides it):
  `useVal`/`useVals`/`usePos`, pick, labels, focus ring.
- `checkpoint(keys) => restore` is the cancel path for Formula/Num/NPanel scrubs: restores only those keys' truth,
  shown, ease, provenance and history; `pointercancel` is never a click.
- Hot: `setHot(ids, source?)` (default `'interaction'`; Timeline writes `'guide'`); `hotOn`/`hotOff` are the only hot
  writers for single objects (hotOff clears only matching interaction). Empty hot restores the current guide. One
  visible hot set per lesson.
- Links: `LessonLinks { groups?: LinkGroup[]; timelines?: Record<string, StepLink> }`. `LinkGroup.rangePolicy:
  'radial'` puts keys in `radialKeys`, which bypass component boxes in `clampMove` (surface fixes radius, outside
  enforces a minimum radius; zero direction falls back to the prior valid direction). `powersLinks()` (x/A/V bounded to
  4/16/64, integer n/k timeline adapters) currently lives in links.ts — lesson logic in the engine, see TODO.
- Integer timeline keys sync through `StepLink` adapters; `boundTimeline(id, count, phases)`, `setStep`.

### Drag, keyboard and clamp (`scene/handles.tsx`, `scene/drag.ts`, `scene/view.tsx`)

- Positions are a literal `V` or a store key; a missing/scalar key throws. 3D data is y-up.
- One registry per card: `createStage(mode, { lesson, id, overlays })`; React context does not cross drei's View
  tunnel, so Stage provides the registry (and `LocaleContext`) INSIDE `<View>` and passes the same object to the
  overlay. Primitives outside a Stage throw.
- One hit test `reg.pick` = `hitHandle` over projected handles (14 px mouse, 22 px touch/pen) for drag start, hover
  cursor and touch scroll lock; no hit meshes. Listeners are capture-phase on `.lab-stage`, so a handle press disables
  OrbitControls first. `reg.project` returns null outside the clip volume.
- ONE write path for drag and key: `Draggable.move?(vals, raw, src: 'drag'|'key') => { set, ease? }` (default grid
  `moved`: keys[0] snaps to 0.1, the rest get the same delta) → `reg.apply` → `clampMove(set, ranges, caught, radial)`
  to the N-panel row ranges (`rowRanges`) → `writeMove(lesson, set, how, ease)` (drag = cut, key = ease). A
  magnet-caught vector is scaled uniformly so its direction (e.g. a·b = 0) survives.
- `magnet(v, targets, zone, from?)`: pass `from` for `src === 'key'`, else a 0.1 key step inside the zone snaps back.
- Keyboard: arrows 0.1, Shift 1, PageUp/PageDown = z in 3D (`keyDelta`); `DragKeys` overlay buttons per handle.
- Touch: `.lab-stage-view { touch-action: pan-y !important }` beats OrbitControls' inline `none`; a non-passive
  `touchstart` with `preventDefault()` on a handle hit stops scroll for that gesture.
- Pixel sizes never come from r3f `size` (stale in a View): `SLine` refreshes LineMaterial resolution per frame;
  heads/dots/glyphs scale via `wppAt`/`usePxScale`; per-frame helpers use module-level temporaries (no allocation).
- Hover arbitration: `useHoverable(lesson, id, HOVER_RANK.*)` (point > beam > guide > arc > fill).

### Primitives

- 6b/6c ([6b](docs/acceptance/6b.md), [6c](docs/acceptance/6c.md)): Grid, Arrow (legs `${id}:x|y|z`, `move` tip/whole),
  Point (`move`, `keys`, `at` may be a function of vals, `size` = CSS-px diameter), Segment, AngleArc (`from`/`to` are
  DIRECTIONS; shorter arc 0..180; 3D requires `normal`; zero direction draws nothing; `ticks` at magnet targets),
  RightAngle (auto within 0.5°), AreaTile (outward = right of a→b; `flip` for clockwise; `signed` + `area`, negative =
  inward + hatched), Parallelogram/`Quad`, Label.
- **Label** is DOM, not drei Html: `useLabel` registers a `LabelSpec`; `LabelLayer` places spans in
  `useFrame(..., 2)` (after View's priority-1 render, else one frame lag). `away`/`gap` and `sides` (`screenWedge`
  + `fitWedge`, max 80 px) for angle labels. Role colors are CSS vars (theme flips need no render).
- 6e ([6e](docs/acceptance/6e.md)): HalfPlane, LitSphere (per-fragment N·L, sRGB mix without colorspace chunk),
  Circle, Ledger (DOM SVG; moving parts via CSS translateY), Challenge (`challenge:<lesson>:<n>` in
  `src/lib/challenges.ts`; step tasks read `step['<timeline>:max']`), N-panel `bool` rows.
- 7 ([7](docs/acceptance/7.md)): NumTable (op components/product/square/diff; cell ids a0…/p0…/sq0…/d0…, totals
  `sum`), CodeView (plain pre/code; linked `lines` 1-based), Timeline (`steps: {id,text,phases?,focus?}[]`, no
  autoplay, writes `step[id]`, `id:max`, `id:phases`), Formula (`tex`, `texNarrow`, `scrub`, `marks`
  hidden/struck/dim, `tips`), Poly (`pts(vals, step)`, optional `path` for piece motion; values cut, steps spring),
  FieldCell (`{lesson,row,size}`).
- Formula layouts (fix wave 2026-10-11): with `texNarrow` BOTH layouts are server-rendered (`data-variant`
  wide/narrow) and a container query on the formula's own width (< 420 px, `.lab-formula.has-narrow`) shows one: no
  hydration switch, no height reserved for the hidden layout (that reserve was the 100-380 px hole under DP/LC
  timelines). Multi-line derivations use `alignedRows(rows, marks)` (tex.ts): a line whose mark is `hidden` is left
  OUT of the TeX (KaTeX positions aligned lines absolutely, so CSS-hidden lines kept their height); revealing a line is a
  timeline step = reader input (hadRecentInput, not CLS). `is-hidden` still hides terms INSIDE a shown line. An empty
  source renders no math. Focus restore (`formulaFocus.ts`) stays inside the variant that had focus.
- Label placement (`separate` in handles.tsx): tick numbers are fixed obstacles for object labels (a tick printed over
  another tick is hidden), the navigation widget (gizmo / XY corner) is an obstacle, a slot cutting the label at the
  free stage edge (N-panel inset excluded) is not free, and a label with `away` keeps moving in its push direction
  before other sides. `Label` takes `away`/`gap`. Registration order = priority (region names after values).
- Derived geometry (projection foot etc.) is computed in the lesson with `useVal` and passed as literals; guard zero
  vectors (`project` throws on zero).
- 0.2 powers: `FnCurve` (`scene/FnCurve.tsx`: y = f(t, vals) sampled uniformly on [from, to] on `Stroke`; a non-finite
  sample throws). TileGrid `fill` may be a function returning undefined (a plain row). TileGrid lays out from the
  store's `shown` record only; a prop that changes with React state (role) does not re-layout by itself, so the ladder
  remounts a rung (`key`) when its colour changes. A card's mode is fixed: 0.2-4 switches 2D/3D by remounting the
  card on the `d3` key (focus moved to the new checkbox). The ladder division glides with the eased `kf` (lesson link
  `ladderLink`: k cuts, kf eases), not with a TileGrid morph.

### Viewport chrome ([6d](docs/acceptance/6d.md))

- `<ViewportCard lesson id name mode extent center? overlays? rows? status? drawIn? look? instruments? angleKey?
  formula?>` inside `<SceneIsland caption rows? hud? formula? formulaHeight?>`: header (2D/3D, Persp/Ortho, Overlays,
  Reset view, wide/full, scene name), Stage, N-panel, status bar.
- Overlay flags: store keys `ov:<card>:<flag>` (0/1). N-panel rows `{ id, label, kind: 'num'|'vec'|'ro'|'bool', key?,
  spec?, snap?, value?, fmt?, visible?, group?, tip?, dims?, chars? }`; row ids are term ids (hover ↔ hot ↔ formula).
- One scrub engine for fields, Formula and Num: `startScrubDrag(e, el, {get, set}, next, onEnd)` (`src/viz/useScrub.ts`)
  + `src/viz/scrub.ts` (4 px per step, Ctrl snap, Shift 1/10, click/Enter types, Esc reverts). Touch scrub starts after
  an 8 px horizontal-dominant move.
- Desktop N-panel overlaps the viewport's right 200 px (Blender region overlap); the scene re-centers via
  `camera.setViewOffset`. Under 720 px it becomes wrapping chips; the reserved height is derived by
  `scene/chips.ts` from the SAME rows passed to SceneIsland and ViewportCard (card throws on mismatch). `N` toggles it.
- Navigation (Ruling T): plain wheel scrolls the page (`gateWheel`); Ctrl/Cmd+wheel and pinch zoom. 2D: MMB/RMB pan.
  3D: LMB/MMB orbit, Shift+MMB pan. Home view = View All of the content (`contentBox`, aux furniture excluded).
  2D (`fit2d`): margin 24 px; the figure stays clear of the XY corner widget (CORNER_2D 50 px on the left OR the
  bottom, whichever keeps it larger). 3D (`fit3d`): per-object enclosure = the tighter of its box and bounding sphere
  (`Part[]`; a sphere's box corners left DP-4's ball at a third of the view), and the figure stays left of or below the
  gizmo square (measured from `.lab-gizmo`).
- Gizmo is DOM/SVG (`Gizmo.tsx`): drei GizmoHelper renders into the wrong View on a shared canvas. Grid = drei `Grid`.
- Status hints: `status={{ hints: { [id]: Hint }, idle?, state? }}`, Hint = text or `(vals) => string|null` or
  `{ mouse, touch }`; tokens like `[N]` are explicit. Tooltips via `useTip` (500 ms).
- Motion (`scene/motion.ts`): critically damped `springStep`, `springProgress`, `omegaFor`; draw-in `reg.reveal` over
  600 ms when 40 % visible. Fonts via Astro Fonts API, `display: 'optional'` + metric-matched fallbacks.
- Live text never moves its own edge: values sit in fixed boxes placed by transform (else CLS entries).

### Holo and clay look ([6h](docs/acceptance/6h.md)) — being reworked by HOLO-V2

- Colors come only from `usePalette()` (`src/viz/palette.ts`, roles = `--viz-*` vars in lab.css, literal `#rrggbb`,
  viewport always dark); non-color look = `STYLE` in palette.ts; no private constants in primitives. Contrast is
  tested in `tests/js/tokens.test.ts`.
- Current 6h recipe: stroke core 2 px (35 % white mix), inner glow 7 px @ .38, outer 18 px @ .10, additive.
  `Quad` lazily loads `Glass` (gradient, world-space subdivisions, luminous edges); Ruler, Dimension, Instruments
  (`InstrumentSync`), Background, Floor/`FloorFade` (mirror), FormulaPlate. Lazy boundaries: Stage → HoloLayer,
  Quad → Glass, clay → ClayTile. `look="clay"` = light studio with matcap `public/matcap-clay.png`.
- Mobile reservation: `instruments`/`formula` on the card ↔ `hud`/`formula` on SceneIsland (100 px or 176 px under
  720 px); `formulaHeight` (finite ≥ 76) belongs to SceneIsland.
- Mirrors: `MirrorCtx` suppresses handle/label registration and hover; only LineMaterial and CircleGeometry
  reflections are forced `DoubleSide` (negative-determinant scale). Never restore an unconditional side override.
- No automatic omissions: never branch on `hardwareConcurrency` or hide reference elements at breakpoints.
- The visual audit found the result far from the reference; HOLO-V2 (see HANDOFF) replaces parts of this section.
  Update it from the HOLO-V2 report when accepted.

### Size modes ([6f](docs/acceptance/6f.md))

- `scene/sizeModes.ts` owns one normal/wide/full store keyed by lesson:card; `useSizeState()` is SSR-normal.
  Keys: Ctrl+Space wide; Ctrl+Alt+Space or F full; Escape normal (focused card before hovered).
- Wide: figure width `min(2 × normal, document − 32px)`, sidebars hidden + inert (exact inline values restored).
- Full: card fixed inset-0 + Fullscreen API on `document.documentElement` (the shared canvas lives in body); the
  existing `.lab-scene-host` node moves into the card and back to its exact position. React root, Canvas, Stage and
  state stay mounted. Camera pose, overlays, panel state survive; `Stage.Nav.redraw` refits synchronously.
- External fullscreen exit hides card chrome for two RAFs to avoid a shift (audit: a metric workaround, see TODO).

### Internationalization ([6g](docs/acceptance/6g.md))

- `src/lib/course-rules.mjs`: `Locale` (`en|ru`), `asLocale`, `localePrefix`, `splitLocale`,
  `localizedUrl(base, path, locale)`, `courseText`; English manifest fields top-level, Russian under `ru`; missing
  fields throw.
- Server: `pageLocale(Astro)` from `src/i18n/ui.ts` (dictionaries; never value-import into a client graph). Client:
  dictionary-free `src/i18n/core.ts` (`t`, `th` for HTML with escaped placeholders, `msg`/`tm`, `docLocale()`).
  React: `LocaleRoot`, `localized`, `useLocale` (`src/i18n/react.tsx`); pass serializable locale to islands and
  rebuild row callbacks from locale-aware factories.
- `<UiStrings ns=[...]>` (`src/components/UiStrings.astro`) emits each namespace once per page for that locale. The
  declared set must equal the reachable client module graph (incl. lazy impls, row factories, workers); missing
  namespaces/keys/placeholders fail. Current sets: EngineDemo `viewport, gizmo, hud, status, challenge, rows, demo,
  holo`; Probe `viewport, gizmo, status, hud, probe`; LessonDone `done, progress`; Quiz `quiz`; PyCell `py`;
  sidebar `progress`.
- Translate labels, hints, captions, accessible names; never translate ids, store keys, ranges or TeX.
- Russian pages carry `translatedFrom` (git blob of the EN source, `scripts/i18n-stamp.ts`); the parity test compares
  MDX props/code/links and TeX in order. `src/i18n/untranslated.json` lists English fallback pages exactly.
- Lesson UI strings belong in namespaced dictionaries under the parity tests, not inline `words(locale, en, ru)`
  pairs (LC fix2 moves them).

### Python cells (Task 8)

- Authoring: wrap an unchanged fence: `<PyCell>`, blank line, the ```` ```python ```` fence, blank line, `</PyCell>`
  (`import PyCell from '.../components/PyCell.astro'`). Optional `colab="<notebook>"` (failure fallback link; no
  notebooks exist before Task 10, so no lesson passes it) and `rows={n}` (output pane lines when `outputRows`
  misjudges, e.g. a matrix printed without a `# [[...` comment). RU pages must wrap the same code (pyclient.test.ts).
- The fence is the single code source: `fenceCode` (`src/viz/py/fence.ts`) reads Expressive Code's copy payload
  (`data-code`, lines joined by U+007F) from the rendered slot; the build fails unless exactly one python block.
- Runtime: `<lab-py>` (`src/viz/py/cell.ts`, ~1.5 kB gz) -> `sharedPy()` (`pyclient.ts`) -> one module worker per
  page (`py.worker.ts`) importing Pyodide 314.0.7 from jsDelivr only on the first Run (9.2 MB cold, measured); NumPy
  via `loadPackagesFromImports`; each run in fresh globals; stdout+stderr one stream (`isatty` = line-buffered).
  Two clocks per run, split by the worker's `{ id, ready }` notice (Python + packages loaded): 120 s before it
  (timeout = `fail: 'load'`), 30 s after it (`'timeout'`, worker terminated). Worker load errors, `error` events and a
  `new Worker` that throws are `'load'`; Python exceptions `'py'` via `cleanTrace` (drops `/lib/` runner frames).
  Every run settles (queue never poisoned); Run again = retry. Idle cells `watch()` the client and switch between
  the first-run note and "Python is loaded" for the whole page.
- Layout: SSR with reserved sizes (status 2 lines, 3 under 40rem; output pane `--py-rows`); Edit swaps views inside
  one `.py-code` box at the sub-pixel height. Change status classes with `classList` only: assigning `className`
  dropped Astro's scoped class and its min-height (a measured shift).
- Pyodide/wasm has no FP exception flags: NumPy prints `nan` for `arccos(-1.625)` without the `RuntimeWarning`, and
  fp32 `np.dot` sums differ from desktop BLAS. Lesson comments promising either describe desktop Python only.

### Tensor core (`src/core`, completion map P5)

Three-free, DOM-free; import from `src/core` (index re-exports errors/dtype/tensor/einsum). It enters a page bundle
only through a scene that imports it. Tests: `tests/js/tensor-core.test.ts` over NumPy fixtures
(`python -I scripts/tensor-fixtures.py` -> `tests/fixtures/tensor/core.json`).
- `Tensor {data: Float64Array|Float32Array, shape, strides, offset, writable}`; strides/offset in ELEMENTS (NumPy
  bytes / itemsize). The constructor checks every reachable element lies in `data`. Views share `data`: `slice`
  (NumPy basic slicing, negative steps), `transpose(axes?)`, `reshape` (contiguous only, one -1; a non-contiguous view
  goes through `contiguous()` explicitly), `expandDims`, `squeeze`, `broadcastTo` (stride 0, read-only).
  `contiguous` returns the SAME tensor when already contiguous and writable: never use it to get a private copy.
- `tensor(nested)`, `zeros`, `full`, `arange`, `rand(shape, {seed, range=[-3,3], step?})` over `mulberry32(seed)`
  (bit-exact Python port in the fixture script); `flat`/`toNested` read logical order; `item()` for one element.
- Elementwise `add/sub/mul/div/zip/map` with NumPy broadcasting (plain numbers are weak scalars: no f32->f64
  promotion). Reductions `sum/mean/max {axis, keepdims, arith, order}`; empty max/mean throw; NaN propagates in max.
- Accumulation (`dtype.ts`): default fp64 arithmetic and `sequential` order whatever the storage; `arith: 'fp32'` =
  `Math.fround` on every operand, product and partial sum; `order: 'pairwise'` = bottom-up adjacent pairs, odd node
  carried (`sumSteps` lists the exact merges). `fp16/bf16/int8` ids throw `DtypeError` until Branch Q passes an
  `Arith {id, round}` object. Result storage is f32 only if every operand is f32.
- `parseEinsum(op, arity?)` -> kept/summed letters; `planEinsum(op, shapes | {letter: extent})` -> `Plan` (outShape,
  sumShape, `sumLen` = shaft length, cells); `einsum(op | plan, ...tensors, opts?)` reads through strides (transposes
  are never materialized); `products(plan, ts, cell)` = the tiles of one cell; `explain(plan, cell, ts?, opts)` =
  per term the operand indices/values/product plus the merges with partial values (`value` equals the einsum element
  bit for bit). Grammar = course subset: explicit `->`, a-z only, no ellipsis/traces/whitespace, >= 1 operand (the
  DotUnit passes arity 2); an outer product has a shaft of length 1. Summed letters flatten row-major (last innermost).
  `matmul` (NumPy: 1-D promotion, broadcast batch) and `dot` (1-D -> number) are einsum wrappers.
- Typed errors (`errors.ts`): `SpecError {op, token, pos}`, `ShapeError`, `IndexError`, `DtypeError`, all
  `CoreError` with `code`; nothing is silently repaired. Fixture example of order: ten 0.1 sum to 0.9999999999999999
  (fp64), 1.0000001 (fp32 sequential), 1.0 (fp32 pairwise).

### DotUnit (`src/viz/dotunit`, docs/design/intro-and-dotunit.md)

- Pure: `layout.ts` (`assignAxes`/`layoutEinsum(plan)`: spec 1.2 axis rules, LOD 1-3, cap 64 -> ShapeError), `model.ts`
  (presets, seeded values, `sceneData`/`dotData`/`columnView`: every number from core `explain`/`einsum`), `choreo.ts`
  (`Frame` buffers; `dotFrame` = timeline `unit` 0-9, `passFrame` = timeline `pass` 0-4). Tests: `tests/js/dotunit.test.ts`.
- Render: `Boxes.tsx` = one InstancedMesh per scene (shader fill/edge/hatch wipe/flash, bounds proxy for framing; refit
  ONE update after a frame was written). `unit.impl.tsx` (DotUnit, face 1 = engine primitives), `einsum.impl.tsx`
  (playground, matvec rows/columns, ladder). Entries `dotunit.tsx`, `einsum.tsx`; wrapper `DotUnit.astro kind=...`.
- Stores: `dotunit` (du_a/du_b n-vectors + du_a3/du_b3 via link groups, n 2..8), `einsum`, `matvec`, `ladder`.
- `Nav.frame(dir)` (Stage): View All from a direction that stays home until Reset view (face 2 is framed from the front).
- Playground `extent` follows the drawn bounds (the camera distance range scales with extent).
- Polish pass: printed fp64/fp32 + sequential/pairwise via core explain (geometry stays fp64; err row), memory strip,
  LOD-2 shaft lines / LOD-3 sweep, code line follows `du_cur`, 5 challenges, Arrow `opacity` (palette `fadeAlphas`),
  cubes framed front-left-top, `Tag dense` = hover-only below 480 px. TRAP: a ShaderMaterial writing to the canvas
  needs sRGB components (three's Color.set gives linear: Boxes uses `convertLinearToSRGB`).
- Start-here term maps: `<LessonMap spine={[...]} />` = `spineIn` (backbone terms joined to nearest backbone foundations).
- [ ] Open: fp16 readout (reserved, DtypeError), physical-device perf, cube readability at 360 px (small figure).
## Authoring rules

- Math in prose: `$a\cdot b$` inline; display math fenced (`$$` on its own lines; a one-line `$$x$$` is inline).
  Rendered at build time by `src/lib/satteri-math.mjs` + `renderStaticMath` (`src/viz/tex.ts`, one OPTS); pages ship
  no math JS. A bad formula fails the build with the .mdx path.
- No raw non-ASCII in math (`°`, `θ`, `→`): write `^\circ`, `\theta`, `\to` (renderStaticMath rejects them).
- No English words inside TeX (`\text{shadow of}`): TeX is compared verbatim between EN and RU, so it cannot be
  translated; put words in prose.
- MDX: `{...}` in prose is a JS expression (escape `\{ \}`); bare `<` + letter starts JSX. Inside `$...$` they are safe.
- Linked/scrubbable formulas use the React `Formula`, not static math. Hidden test pages live in
  `src/content/docs/dev/` (`pagefind: false`, noindex).
- Notation: vectors bold `\mathbf{a}`, lengths `|\mathbf{a}|`, plain letters = numbers; angles in whole degrees
  (`fmtAngle`).
- Prose for the reader: one idea per sentence, a number after each claim, no implementation caveats in lesson text.
- Quiz: `src/components/Quiz.astro` (4 options, plain text, positions balanced by hand).
- Covers: templates in visual-language.md §Covers; image models cannot hold exact geometry, so lesson diagrams come
  only from the engine.

## Tests and gates

- Before every commit: `bun test tests/js`, `bun run check` (0 errors / 0 warnings / 0 hints) and `bun run build`
  (0 warnings). Physical renderer chunk ≤ 300,000 B gzip.
- Pure tests (`tests/js/*.test.ts`) run without DOM; keep the modules they import DOM-free and three-free.
- DOM tests (`*.test.tsx`) import `./dom` FIRST (happy-dom via `@happy-dom/global-registrator`, sets
  `IS_REACT_ACT_ENVIRONMENT`); mount with `createRoot`, wrap updates in `act`.
- Reviewers run gates and browser checks themselves; a reviewer is a different model from the implementer.

## Traps

- bun shares one process: after a DOM test loads, later files see `document`; happy-dom's `localStorage` becomes
  read-only (store.test.ts uses `Object.defineProperty`).
- happy-dom's WheelEvent drops modifier init fields: set `ctrlKey` explicitly in tests.
- Tests importing three/drei print THREE_CJS_DEPRECATED: keep pure helpers in three-free modules (`geom.ts`, Header,
  Gizmo, NPanel, StatusBar, handles).
- `THREE.Clock` deprecation is logged by r3f 9.8.1 with three ≥ 0.183; known noise, kept (do not downgrade three).
- MMB `mousedown` must be prevented (Chrome autoscroll moved the page while panning).
- Overlays popover: close on focusout only with a non-null `relatedTarget` (null = Chrome focus fixup, closing would
  swallow the click).
- A `requestAnimationFrame` stub that calls back synchronously recurses; `nextFrame` detects it.
- `display: 'swap'` fonts re-wrapped the intro (CLS 0.049); keep `optional`.
- Agent worktrees branch from `main`: run `git merge --ff-only feat/engine` first; reports from worktrees may need
  copying into `.superpowers/`.
- Commit by explicit path (`git commit -- <paths>`) while other agents work in the same checkout. Never reset/stash.
- Don't reformat JSON with a serializer (CRLF/format churn); edit lines textually.
- Agents hit 429 rate limits mid-task: resume the same agent with SendMessage; partial edits stay in the tree.
- Budgets and user rules are not relabeled or superseded by the controller; "exactly" means exactly. Ask the user.

## TODO (current)

- [x] Engine tasks 6b–6h, 6g, 6f, 7 accepted (see docs/acceptance). CI runner pinned (6d284b0).
- [ ] HOLO-V2: scenes match `holo-style-ref.png` (visual audit items 1–10; InstrumentSync via store subscription, no
  per-frame JSON/MutationObserver; drop compact-glsl and public source maps unless measured need; sizeModes exit fix;
  readable code). In flight in the main checkout.
- [ ] LC fix round 2 (0.5 law of cosines): E-1, E-2, R-1, R-2, Q-1 + minors. In flight in a worktree.
- [ ] Move lesson logic out of the engine (`powersLinks` in links.ts, `scene/radialLink.ts`) into `src/viz/lessons/`.
- [ ] Engine `hotLinks` option for lessonStore (replace lesson-local `SemanticLinks`), NPanel row `emphasis` flag.
- [ ] Palette bypass literals (Grid.tsx, Background.tsx clay sky, Instruments gradient stops, look.ts 35 %).
- [ ] Browser smoke tests in `tests/browser/` + CI (glow/glass/floor/fullscreen/CLS are not regression-protected).
- [ ] Baseline media/font CLS before reference C7/C9 acceptance (open since 6f). Fix wave measured the same no-input
  KaTeX/text entries before and after (dp-ru-1280 0.0438, others < 0.0002); nothing new.
- [ ] Phone chip rows keep invisible slots for rows a later step reveals (LC-0 at 360 has a blank band); a reflow on
  reveal would be input-driven, but chips.ts / the SSR frame contract would have to change.
- [ ] Merge to main, deploy, check live (nothing deployed since 8868525).
- [ ] 1.4 dot product scenes DP-1..4; 0.G scenes. [x] 0.2 powers scenes 0.2-1..5 EN + RU (batch report
  `.superpowers/sdd/2026-10-11-recover/batch-powers-report.md`); open: layer explode hover of 0.2-2.
- [ ] Remaining lessons in course-completion order (P3 services → P4 Part 0/1 → P5..P15), each EN + RU, scenes,
  Python/Colab, term map; implementer → reviewer (different model) → visual verifier.
- [x] Task 8 (Pyodide cells) implemented; awaiting review. 0.G squares-and-plane python block not wrapped yet
  (file owned by the 0.G agent): wrap it like the others.
- [ ] Tasks 9 (term graph/TermMap), 10 (notebooks + ColabBadge; then pass `colab` to PyCell), 11 (size report).
- [ ] Port quiz-lint from vllm_en (`tests/js/quiz-lint.test.ts` missing).
- [ ] Decide whether `/dev/` pages are filtered from `@astrojs/sitemap`; favicon.svg 404.
- [ ] Brittle `Function.toString()` comparison in `tests/js/i18n-course.test.ts:37`.
- [ ] Clean leftover worktrees/branches (with operator approval).
- [ ] Covers: regenerate to exact geometry or accept drafts (user decision).
- [x] "Start here" module (`start`: `why-this-course`, `how-it-connects`; EN + RU prose; terms `model`, `parameter`,
  `multiply-add`) precedes Ground in course.json (2026-10-11). Scenes INTRO-1..3 are `{/* scene: … */}` slots; the
  procedural DotUnit system (`scene = f(einsum spec, values)`, presets for 1.4/2.2/2.4/3.6/3.7/attention, zoom ladder)
  is specified in `docs/design/intro-and-dotunit.md`. Trap: terms.json enforces course order on `built-on`/`is-a`
  edges, so terms introduced in `start` must be roots (later terms may point at them, not the reverse).
- [x] Tensor core `src/core` (einsum plan/explain, fp32/pairwise, NumPy fixtures).
- [x] 3.1 tensor road (`tensors/tensor-road`, module `tensors` after `vectors`; Part 2 "Matrices" goes BEFORE it when
  built). Cells reuse DotUnit `Boxes`/`Frame` (one instanced draw, cap 4096); layout/model/examples pure in
  `tensor-road.math.ts`; image channels lie side by side (C = x block axis). Report:
  `.superpowers/sdd/2026-10-11-recover/tensor-road-report.md`. Open: 3.2-3.7 not built; FieldCell labels are
  low-contrast on the light page theme (lesson wraps them in a dark strip; engine fix pending).
- [ ] Build INTRO-1..3 and the DotUnit (`src/viz/dotunit/`: `layoutEinsum`, LOD, Tiles) per that spec, on `Plan`.

## Changelog

- 2026-10-10: stack approved; Plan 1 + addendum (6d–6h, 6g, 6f, 7) written; spike A, 6b–6e accepted.
- 2026-10-11 night: 6h (`90c522a`), 6g (`df00473`), 6f (`ce5f0ab`), 7 (`8b2f0c3`) accepted; powers prose merged
  (`0dca77d`); LC interactive slice (`b448484`, `ca25dba`).
- 2026-10-11: recover audits (visual, LC, process); AGENTS.md slimmed, history moved to docs/acceptance/.
- 2026-10-11: live-check fix wave (report `.superpowers/sdd/2026-10-11-recover/fixwave-report.md`): formula
  container query + alignedRows, label obstacles, corner-aware 2D/3D framing, LC-2 detail triangle piece, LC cards
  followed by their own controls (`tests/js/lesson-composition.test.ts`), DP three-faces panel.
