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

- Pure tests (`tests/js/*.test.ts`) run under plain `bun test tests/js` with no DOM. Keep modules they import DOM-free.
- DOM/component tests (`*.test.tsx` that mount React) opt in by importing `./dom` as the FIRST import. `tests/js/dom.ts` registers happy-dom via `@happy-dom/global-registrator` (devDependency, exact pin) and sets `IS_REACT_ACT_ENVIRONMENT`. It is not preloaded, so pure tests never see `document`; bun shares one process, so after a DOM file loads, later files do see the globals.
- Mount with `react-dom/client` `createRoot` and wrap updates in `act` from `react`.

## Math authoring

- Plain math in prose: `$a\cdot b$` inline; display math MUST be fenced (`$$` on its own lines). A one-line `$$x$$` is parsed as *inline* math (micromark rule), not display. Rendered to KaTeX HTML at build time by `src/lib/satteri-math.mjs` (satteri `features.math` + `renderStaticMath` in `src/viz/tex.ts`); pages ship no math JS. Katex CSS comes from `customCss`.
- A bad formula fails the build with the `.mdx` path. Raw non-ASCII in math (`°`, `θ`, `→`) is rejected by `renderStaticMath` (KaTeX `strict` alone does not flag them): write `^\circ`, `\theta`, `\to`.
- MDX traps outside math: `{...}` in prose is a JS expression (`{1 + 1}` renders `2`; `a {b}` fails the build with a ReferenceError) - escape as `\{ \}`; a bare `<` followed by a letter starts JSX. Inside `$...$` braces/`<`/`_` are safe (`$\{x\}$`, `$a < b$`).
- `.ts` is importable from the `.mjs` plugin at Astro config load (bun runtime), so there is one OPTS definition in `tex.ts`.
- Linked/scrubbable formulas use the React `Formula` (engine plan Task 3), not this path. Hidden test page: `src/content/docs/dev/math.mdx` (not in sidebar, `pagefind: false`, noindex; still listed in the sitemap - [ ] decide whether to filter `/dev/` from `@astrojs/sitemap`).
