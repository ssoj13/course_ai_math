# Math for AI

**Live site: <https://ssoj13.github.io/course_ai_math/>** — English; Russian: <https://ssoj13.github.io/course_ai_math/ru/>.

An interactive, visualization-first textbook of the math behind AI: from the law of cosines and the dot product to vectors, matrices and tensors, and on toward calculus, statistics and attention. It is written for CG/VFX developers (Maya, Houdini, shaders) and explains step by step: every symbol in words, a worked number after each formula, CG analogies such as N·L and transforms, and live scenes linked to the formulas. The full program is in `CURRICULUM.md`; the modules published so far are listed below.

## Course map

The structure lives in one file, `src/data/course.json`; lesson numbers, the sidebar, prev/next links and the roadmap are derived from it.

- **Start here** - Why this course: what AI actually computes; How it all connects: one dot product, scaled up.
- **Ground: distance and angle** - Squares, roots and the plane; Powers: whole, zero, negative and fractional; Distance and Pythagoras; Angles, radians and the unit circle; The law of cosines.
- **Vectors** - A vector: an arrow and a list of numbers; Adding, scaling and linear combinations; Length, Σ notation and normalization; The dot product, derived from the law of cosines; Projection: shadows on an axis; Basis, span, independence and dimension; Cosine similarity and embeddings; The cross product (side branch); Waves as vectors: the DFT is a list of dot products.
- **Matrices** - Linear transformations; A matrix = images of the basis vectors; M·v two ways.
- **Tensors** - What a tensor is: the road from a number to N dimensions; Data as tensors: images, text, sound and meshes; Memory: flat buffer, strides, reshape / view / transpose / contiguous; Elementwise ops and broadcasting; Reductions: sum, mean, max along an axis.

Plus a course roadmap and a glossary.

## Interactive parts

- **Scenes** - lessons carry live 2D/3D scenes (React Three Fiber) with draggable handles, linked to the formulas and number tables on the page.
- **Python cells** - most lessons include runnable Python cells executed in the browser by Pyodide in a web worker. Pyodide is not bundled: it is loaded from jsDelivr only when you first press Run.
- **Term maps** - a term graph (`src/data/terms.json`) drives the per-lesson maps and the glossary.
- **Quizzes and progress** - lesson quizzes, and a "lesson done" marker in the page footer that is tracked in the sidebar.

## Developing the site

Astro + [Starlight](https://starlight.astro.build/) with React islands; package manager and runtime [bun](https://bun.sh/) **1.4.3** (pinned in `package.json`; older bun crashes during Astro's prerender).

```powershell
bun install
bun run dev                   # http://localhost:4321/course_ai_math/
bun run test                  # JS tests (bun test tests/js)
bun run check                 # astro check (types)
bun run build                 # static output in dist/
bun run preview               # serve dist/ locally
bun run i18n:stamp <ru page>  # after updating a Russian page to match its changed English source
```

The site build needs no Rust toolchain. Pushes to `main` are tested (`bun test tests/js`, `bun run check`), built and deployed to GitHub Pages by `.github/workflows/deploy.yml`.

English pages live in `src/content/docs/<module>/<lesson>.mdx`, Russian ones in `src/content/docs/ru/` with the same paths; `tests/js/i18n-sync.test.ts` keeps them in sync.

`notebooks/build/embeddings.py` regenerates `src/data/embeddings-small.json` for the cosine-similarity lesson (`uv run notebooks/build/embeddings.py`).

`sim-widget-rs/` is a separate Rust workspace (an explainer runtime under development); it is not part of the site build. See `sim-widget-rs/README.md`.

## Repository layout

```text
src/data/            course.json (course structure, single source of truth), terms.json (term graph), embeddings-small.json
src/content/docs/    lessons (MDX) by module, roadmap, glossary, dev pages; ru/ = the Russian edition
src/components/      Astro components (PyCell, Quiz, LessonMap, CourseMap, Glossary, ...); overrides/ = Starlight overrides
src/viz/             interactive scenes: lessons/ (per-lesson scenes), scene/ (shared primitives), py/ (Pyodide worker), terms/, demo/
src/lib/             course rules, link plugins, shared client code
src/i18n/            UI strings and locale handling
src/styles/          shared site styles (lab.css)
src/pages/           404 page
scripts/             i18n-stamp.ts, tensor-fixtures.py
notebooks/build/     data-building scripts (embeddings.py)
tests/js/            bun tests; tests/fixtures/ = test data
sim-widget-rs/       Rust explainer runtime workspace (separate from the site)
docs/                design notes, acceptance evidence, plans
plans/               implementation plans for the explainer runtime
.github/workflows/   test, build and GitHub Pages deployment
CURRICULUM.md        full course program
AGENTS.md            working notes for contributors and coding agents; HANDOFF.md = session state
```
