# Engine + Spikes Implementation Plan (Plan 1 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A deployable Astro/Starlight site skeleton for "Math for AI" with the complete visualization engine (shared r3f canvas, linked lesson store, KaTeX formulas with term ids, scrubbing, timeline, Pyodide cells, term graph + map) proven on a draft demo page, so Plan 2 can write the reference section (Part 0 + Part 1) purely by composing primitives.

**Architecture:** Starlight shell reused from `vllm_en` (course.json → sidebar, numbering, links, progress). Visualizations are React islands. All 3D/2D scenes on a page render into ONE fixed `<Canvas>` (mounted by a Starlight `PageFrame` override) through drei `<View>`; scene islands, formula islands and tables share one zustand store per lesson (`vals` + `hot` term ids). Static math is rendered at build time; linked math is a React `Formula` using KaTeX `\htmlData{t=…}`.

**Tech Stack:** bun 1.3.12, astro 7.3.8, @astrojs/starlight 0.42.6, @astrojs/react 7.0.1, react/react-dom 19.3.0, three 0.186.1, @react-three/fiber 9.8.1, @react-three/drei 10.7.9, zustand 5.0.15, katex 0.19.0, @xyflow/react 12.12.0, elkjs 0.12.0, Pyodide (version pinned in Task 8 after verification), TypeScript strict, `bun test`.

**Spec:** `AGENTS.md` (decisions, principles), `CURRICULUM.md` (structure), `docs/design/part0-1-storyboard.md` (Fable's storyboard: §4 primitives, §5 risks R1–R11). Reference engine: `C:\projects\projects.rust.cg\cgprojs\vllm_en` (read its `AGENTS.md` "Widget rules").

## Global Constraints

- Site: GitHub Pages project site, `site: 'https://ssoj13.github.io'`, `base: '/math-course'`, repo `ssoj13/math-course` (name to be confirmed by the user before Task 11 pushes anything).
- Content language English; code comments explain *why*, not what.
- Storage prefix `math-course:` and event name `math-course:store` (the `ssoj13.github.io` origin is shared with the vllm course — keys must never collide).
- No network and no heavy work on page load; Pyodide, elk layout and anything > 50 kB gz load on click or `client:visible`.
- Anything importing three / r3f / drei is `client:only="react"`; the `.astro` wrapper prints the caption server-side and reserves height (no layout shift).
- Every island/card root has class `not-content` (Starlight typography leak, see vllm_en traps).
- Works in Starlight light and dark themes and at 360 px width without horizontal page scroll.
- `prefers-reduced-motion: reduce` → no tweens, hard cuts.
- No hard-coded colors in components: semantic roles from `src/viz/palette.ts` (CSS vars in `src/styles/lab.css`).
- Git: commit after every task; messages in English, conventional style; **no Co-Authored-By / AI attribution lines**; never `git reset`.
- Dev/demo pages live in `src/content/docs/dev/` and are **not drafts** (drafts are excluded from production builds, so CI would build and size-check no viz code at all). They are hidden instead: not in the sidebar (the sidebar is explicit), frontmatter `pagefind: false` and `head: [{ tag: 'meta', attrs: { name: 'robots', content: 'noindex' } }]`.
- **Composition lives in `.tsx`, never in MDX** (Astro serializes island props to JSON: functions such as `fmt`/`derived` cannot cross, and MDX children of a React island arrive as an HTML string, not React nodes). Each lesson/demo has one module `src/viz/lessons/<slug>.tsx` (demo: `src/viz/demo/engine.tsx`) exporting named island components; MDX only mounts them: `<DotExplorer client:only="react" />` with serializable props (strings/numbers) at most. Lesson stores are initialized at module level of that file (`const S = lessonStore('dot-product', { a: [3, 1], b: [1, 2] })`), never during render.
- `tsconfig.json`: `"extends": "astro/tsconfigs/strict"`, `"compilerOptions": { "jsx": "react-jsx", "jsxImportSource": "react" }`. React 19 types: use `React.JSX.Element`, not global `JSX`.
- Vite workers are ES modules: `vite: { worker: { format: 'es' } }`; runtime CDN imports carry `/* @vite-ignore */`.
- KaTeX runs with `strict: 'error'` except `htmlExtension`: write `^\circ`, `\to`, `\theta` — never raw Unicode (`°`, `→`, `θ`) inside math.

**Execution order** (dependencies, risk first): Task 0 → 1 → 2 → 4 (Spike A) → 5 → 3 → 6a → 6b → 6c → 7 → 8 → 9 → 10 → 11.

## Review Focus

1. **Shared-origin storage collision** — another ssoj13.github.io course writes `progress`; ours must read only `math-course:progress`. Test in Task 0 (`store.test.ts`).
2. **Touch drag vs page scroll** — on a phone, swiping over a 2D stage must scroll the page unless a draggable handle was hit; a drag must not scroll. Test in Task 6 (pure hit-test function + demo-page check at 360 px).
3. **No WebGL / context lost** — the caption and the formula/table twins must remain readable and an inline notice must appear instead of a blank box. Test in Task 4 (`webglAvailable()` stub + demo check).
4. **Theme switch mid-session** — toggling Starlight dark/light must recolor live scenes without reload. Test in Task 5 (`readPalette` unit test + demo check).
5. **Pyodide CDN blocked/offline** — clicking Run must show an actionable error with the Colab link, not hang. Test in Task 8 (worker timeout path unit-tested with a fake loader).

---

## File map

```text
astro.config.mjs                     Starlight + React integration, sidebar from course.json, base /math-course
package.json / bun.lock / tsconfig.json / .gitignore
src/data/course.json                 modules + lessons (Part 0, Part 1)
src/data/terms.json                  term graph (Task 9; Plan 2 fills it)
src/content.config.ts                docs + i18n collections (copied)
src/content/docs/index.mdx           home
src/content/docs/course/roadmap.mdx  roadmap (TermMap of lessons arrives in Plan 2)
src/content/docs/<module>/<lesson>.mdx  12 stub pages (title + summary only)
src/content/docs/dev/engine.mdx      hidden (not draft) demo page mounting the demo islands
src/viz/demo/engine.tsx              demo compositions (islands) — composition is always .tsx
src/lib/satteri-math.mjs             satteri mdast plugin: math/inlineMath → KaTeX HTML at build time
src/lib/{course.ts,remark-lesson-links.mjs,route-links.ts,store.ts,progress.ts,element.ts,ui.ts}  ported
src/components/{LessonDone,Quiz,ColabBadge}.astro + overrides/{Sidebar,PageTitle,Footer,PageFrame}.astro
src/styles/lab.css                   ported tokens + viz palette vars
src/viz/store.ts                     lesson store (vals, hot, step)
src/viz/palette.ts                   semantic color roles from CSS vars, theme observer
src/viz/geom.ts                      pure 2D/3D helpers (projection, angle, arcs, hit tests)
src/viz/tex.ts                       KaTeX render with term ids + scrub markers (pure)
src/viz/Formula.tsx  Num.tsx  NumTable.tsx  Timeline.tsx  CodeView.tsx
src/viz/scene/{SceneHost.tsx,Stage.tsx,Grid.tsx,Arrow.tsx,Point.tsx,Segment.tsx,AngleArc.tsx,AreaTile.tsx,Parallelogram.tsx,Label.tsx,webgl.ts}
src/viz/SceneIsland.astro            wrapper: caption + reserved height + client:only island
src/viz/py/{py.worker.ts,pyclient.ts,PyCell.tsx}
src/viz/terms/{graph.ts,TermMap.tsx,elk.worker.ts}
tests/js/*.test.ts                   bun tests
scripts/size-report.ts               bundle budget check
.github/workflows/deploy.yml         test + build + deploy
notebooks/build/                     generator ported from vllm_en (Task 10)
```

---

### Task 0: Repository, scaffold and ported shell

**Files:**
- Create: `.gitignore`, `package.json`, `tsconfig.json`, `astro.config.mjs`, `src/content.config.ts`, `src/content/i18n/en.json`, `src/data/course.json`, `src/content/docs/index.mdx`, `src/content/docs/course/roadmap.mdx`, 12 lesson stubs, `src/styles/lab.css`
- Copy then adapt from `vllm_en`: `src/lib/course.ts`, `src/lib/remark-lesson-links.mjs`, `src/lib/route-links.ts`, `src/lib/store.ts`, `src/lib/progress.ts`, `src/lib/element.ts`, `src/lib/ui.ts`, `src/lib/quiz.ts`, `src/components/LessonDone.astro`, `src/components/Quiz.astro`, `src/components/ColabBadge.astro`, `src/components/overrides/{Sidebar,PageTitle,Footer}.astro`
- Test: `tests/js/course.test.ts`, `tests/js/store.test.ts`

**Interfaces:**
- Produces: `href(slug, hash?)`, `pageHref(path)`, `lessonByPath(path)`, `adjacent(slug)`, `lessons`, `modules` (unchanged API from vllm_en); `load/save/watch/KEYS` with prefix `math-course:`.

- [ ] **Step 1: git init and package.json**

```powershell
git init -b main
bun init -y   # then overwrite package.json below
```

```json
{
  "name": "math-course",
  "type": "module",
  "version": "0.0.1",
  "private": true,
  "scripts": {
    "dev": "astro dev",
    "build": "astro build",
    "preview": "astro preview",
    "test": "bun test tests/js",
    "size": "bun scripts/size-report.ts"
  },
  "dependencies": {
    "@astrojs/react": "7.0.1",
    "@astrojs/starlight": "0.42.6",
    "@astrojs/markdown-satteri": "0.4.3",
    "@react-three/drei": "10.7.9",
    "@react-three/fiber": "9.8.1",
    "@xyflow/react": "12.12.0",
    "astro": "7.3.8",
    "elkjs": "0.12.0",
    "katex": "0.19.0",
    "react": "19.3.0",
    "react-dom": "19.3.0",
    "sharp": "^0.35.3",
    "three": "0.186.1",
    "zustand": "5.0.15"
  },
  "devDependencies": {
    "@types/katex": "latest",
    "@types/react": "latest",
    "@types/react-dom": "latest",
    "@types/three": "latest"
  }
}
```

Then `bun install` and replace every `latest` in devDependencies with the exact installed version from `bun.lock` (exact pins everywhere).

`.gitignore`: copy vllm_en's Astro/Node/Python/OS sections (drop `training/data/` and `.cache/` comment lines about the lab).

- [ ] **Step 2: course.json for Part 0 + Part 1**

```json
{
  "modules": [
    {
      "slug": "ground",
      "title": "Ground: distance and angle",
      "summary": "Squares, Pythagoras, the unit circle and the law of cosines — everything the dot product is built from.",
      "lessons": [
        { "slug": "squares-and-plane", "title": "Squares, roots and the plane", "short": "Squares and the plane", "summary": "A square is an area, a root undoes it, a point is two readings on two rulers." },
        { "slug": "pythagoras", "title": "Distance and Pythagoras", "summary": "Distance between points in 2D, 3D and any number of dimensions." },
        { "slug": "unit-circle", "title": "Angles, radians and the unit circle", "short": "The unit circle", "summary": "cos and sin are the shadows of a point on the unit circle." },
        { "slug": "law-of-cosines", "title": "The law of cosines", "summary": "Pythagoras plus a correction term for a corner that is not square; two-bone IK." }
      ]
    },
    {
      "slug": "vectors",
      "title": "Vectors",
      "summary": "Arrows as lists of numbers: length, the dot product, projection, bases and embeddings.",
      "lessons": [
        { "slug": "vector", "title": "A vector: an arrow and a list of numbers", "short": "What a vector is", "summary": "Point vs direction, components, the vector between two points." },
        { "slug": "vector-arithmetic", "title": "Adding, scaling and linear combinations", "short": "Linear combinations", "summary": "Head-to-tail addition, scaling, blend shapes as linear combinations." },
        { "slug": "length", "title": "Length, Σ notation and normalization", "short": "Length and normalization", "summary": "Pythagoras over all components; unit vectors." },
        { "slug": "dot-product", "title": "The dot product, derived from the law of cosines", "short": "The dot product", "summary": "Σ aᵢbᵢ = |a||b| cos θ, and what its sign means." },
        { "slug": "projection", "title": "Projection: shadows on an axis", "short": "Projection", "summary": "Coordinates are dot products with the basis vectors." },
        { "slug": "basis", "title": "Basis, span, independence and dimension", "short": "Basis and span", "summary": "Rulers for space; orthonormal change of basis." },
        { "slug": "cosine-similarity", "title": "Cosine similarity and embeddings", "short": "Cosine similarity", "summary": "Meaning as direction: comparing embeddings." },
        { "slug": "cross-product", "title": "The cross product (side branch)", "short": "Cross product", "summary": "Normals, parallelogram area and orientation in 3D.", "branch": true }
      ]
    }
  ]
}
```

Add `branch?: boolean` to `LessonJson` in `course.ts` (doc comment: "side branch: shown dashed on the roadmap; still numbered in course order").

- [ ] **Step 3: Port lib files with these exact adaptations**

- `store.ts`: `PREFIX = 'math-course:'`, event `'math-course:store'` (both places), doc comment mentions the shared origin; `KEYS` reduced to `{ progress: 'progress' }` plus `lessonState: 'lesson'` (prefix for per-lesson persisted viz state, see Task 2).
- `progress.ts`: drop `LEGACY_IDS`/migration (no legacy data exists); `normalize` keeps only known slugs.
- `course.ts`: drop `legacy`; add `branch`.
- `remark-lesson-links.mjs`: `PAGE_PATHS` = `glossary → glossary`, `roadmap → course/roadmap`, `home → ''`.
- `route-links.ts`, `element.ts`, `ui.ts`, `quiz.ts`, `LessonDone.astro`, `Quiz.astro`, overrides: copy verbatim, then fix imports that no longer exist.
- `ColabBadge.astro`: URL `https://colab.research.google.com/github/ssoj13/math-course/blob/main/notebooks/<notebook>.ipynb`; note text "free CPU runtime is enough for this course".
- `lab.css`: copy verbatim (viz vars are added in Task 5).

- [ ] **Step 4: astro.config.mjs**

Copy vllm_en's config and change: `BASE = '/math-course'`; remove `redirects`, `dropOrtWasm`, `chunkSizeWarningLimit`; add `import react from '@astrojs/react'` and `integrations: [starlight({...}), react()]`; title `'Math for AI'`; description `'An interactive, visualization-first course: from the law of cosines and the dot product to tensors, calculus, statistics and attention.'`; social/editLink → `ssoj13/math-course`; sidebar: Home, Course roadmap, module groups (label `${i + 1}. ${m.title}`, item labels `${num}. ${short ?? title}` with ` ↳` prefix when `branch`), Reference → Glossary. Keep `PageFrame` out until Task 4. Also: `vite: { worker: { format: 'es' } }`; `customCss: ['katex/dist/katex.min.css', './src/styles/lab.css']`; markdown processor `satteri({ features: { math: true }, mdastPlugins: [courseLinks({ base: BASE })] })` (the math plugin is added in Task 1). `tsconfig.json` per Global Constraints. Note: `index.mdx` hero action `page:roadmap` is resolved only by `route-links.ts` middleware — it must be ported before the build is run.

- [ ] **Step 5: Stub pages**

Each lesson file `src/content/docs/<module>/<slug>.mdx`:

```mdx
---
title: The dot product, derived from the law of cosines
description: Σ aᵢbᵢ = |a||b| cos θ, and what its sign means.
---

This lesson is being written. See the [course roadmap](page:roadmap).
```

`index.mdx` (splash, hero action `page:roadmap`), `course/roadmap.mdx` (plain list for now), `glossary.mdx` (empty heading).

- [ ] **Step 6: Write the failing tests**

`tests/js/course.test.ts`:

```ts
import { describe, expect, test } from 'bun:test';
import { adjacent, lessons, modules } from '../../src/lib/course';

describe('course manifest', () => {
	test('lessons are numbered globally in course order', () => {
		expect(lessons.map((l) => l.num)).toEqual(lessons.map((_, i) => i + 1));
		expect(lessons[0].slug).toBe('squares-and-plane');
		expect(lessons.at(-1)?.slug).toBe('cross-product');
	});
	test('slugs are unique', () => {
		expect(new Set(lessons.map((l) => l.slug)).size).toBe(lessons.length);
	});
	test('prev/next cross module boundaries', () => {
		expect(adjacent('law-of-cosines').next?.slug).toBe('vector');
		expect(adjacent('vector').prev?.slug).toBe('law-of-cosines');
	});
	test('branch flag is carried through', () => {
		expect(modules[1].lessons.find((l) => l.slug === 'cross-product')?.branch).toBe(true);
	});
});
```

`tests/js/store.test.ts` (Review Focus 1):

```ts
import { beforeEach, expect, test } from 'bun:test';

// No `implements Storage`: the DOM Storage type has an index signature this mock does not need.
class MemStorage {
	#m = new Map<string, string>();
	get length() { return this.#m.size; }
	clear() { this.#m.clear(); }
	getItem(k: string) { return this.#m.get(k) ?? null; }
	key(i: number) { return [...this.#m.keys()][i] ?? null; }
	removeItem(k: string) { this.#m.delete(k); }
	setItem(k: string, v: string) { this.#m.set(k, v); }
}

beforeEach(() => {
	const g = globalThis as Record<string, unknown>;
	g.localStorage = new MemStorage();
	g.sessionStorage = new MemStorage();
	g.window = globalThis;
	g.CustomEvent ??= class extends Event { detail: unknown; constructor(t: string, i?: { detail?: unknown }) { super(t); this.detail = i?.detail; } };
});

test('keys are namespaced so another course on the shared origin cannot collide', async () => {
	const { KEYS, load, save } = await import('../../src/lib/store');
	localStorage.setItem('progress', JSON.stringify({ foreign: true }));
	localStorage.setItem('vllm-course:progress', JSON.stringify({ foreign: true }));
	expect(load(KEYS.progress, {})).toEqual({});
	save(KEYS.progress, { pythagoras: true });
	expect(localStorage.getItem('math-course:progress')).toBe('{"pythagoras":true}');
	expect(localStorage.getItem('vllm-course:progress')).toBe('{"foreign":true}');
});
```

- [ ] **Step 7: Run tests and build**

Run: `bun test tests/js` → Expected: all PASS (if `course.test.ts` fails on `branch`, the `course.ts` adaptation is missing).
Run: `bun run build` → Expected: build succeeds; the only tolerated warning is none (vllm's i18n collection fix is copied). Any other warning: investigate and fix, do not suppress.

- [ ] **Step 8: Commit**

```powershell
git add -A
git commit -m "feat: scaffold math course shell from vllm engine (course manifest, progress, links)"
```

---

### Task 1: Static math in MDX via satteri's math feature

Satteri (Astro 7's Markdown/MDX processor) parses `$…$` / `$$…$$` when `features.math` is on (verified: `satteri/dist/compile.d.ts:111-118` `features.math`, default false; nodes `math`/`inlineMath` in `types.d.ts:12-17`; `features`+`mdastPlugins` are passed to MDX in `@astrojs/markdown-satteri/dist/mdx/create-processor.js:54,84`; visitor ctx has `replaceNode(node, { type: 'html', value })`). Re-verify these line numbers in the installed versions before coding. Linked/scrubbable formulas always use the React `Formula` (Task 3); this task is only plain math in prose.

**Files:**
- Create: `src/lib/satteri-math.mjs`, `src/viz/tex.ts` (only `renderStaticMath` + shared `OPTS` for now; Task 3 extends), `src/content/docs/dev/math.mdx` (hidden test page)
- Modify: `astro.config.mjs` (`mdastPlugins: [courseLinks({ base: BASE }), katexMath()]`), `AGENTS.md` ("## Math authoring")
- Test: `tests/js/math-render.test.ts`

**Interfaces:**
- Produces: `$…$` inline and `$$…$$` block math in all lesson MDX, rendered to KaTeX HTML at build time; a bad formula fails the build with the file name. `renderStaticMath(tex: string, block: boolean): string` in `src/viz/tex.ts`.

- [ ] **Step 1: Failing test**

```ts
import { expect, test } from 'bun:test';
import { renderStaticMath } from '../../src/viz/tex';
test('inline and block', () => {
	expect(renderStaticMath(String.raw`\sum_i a_i b_i`, false)).toContain('class="katex"');
	expect(renderStaticMath(String.raw`\|a\| < b`, true)).toContain('katex-display');
});
test('invalid TeX throws so the build fails loudly', () => {
	expect(() => renderStaticMath(String.raw`\frac{1}{`, false)).toThrow();
});
test('raw Unicode in math is rejected (write \\circ, \\theta)', () => {
	expect(() => renderStaticMath('60°', false)).toThrow();
});
```

- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** `tex.ts` (the `OPTS` block and `renderStaticMath` exactly as in Task 3 Step 3) and the plugin:

```js
/**
 * Satteri mdast plugin: `math` / `inlineMath` nodes (enabled by `features: { math: true }`) become KaTeX HTML at
 * build time, so pages ship no math JS. Errors throw with the file name: a broken formula must fail the build.
 */
import { fileURLToPath } from 'node:url';
import { renderStaticMath } from '../viz/tex.ts';

export default function katexMath() {
	const render = (block) => (node, ctx) => {
		try {
			ctx.replaceNode(node, { type: 'html', value: renderStaticMath(node.value, block) });
		} catch (e) {
			const where = ctx.fileURL ? fileURLToPath(ctx.fileURL) : 'unknown file';
			throw new Error(`${where}: bad math "${node.value}": ${e instanceof Error ? e.message : e}`);
		}
	};
	return { name: 'katex-math', math: render(true), inlineMath: render(false) };
}
```

If importing a `.ts` file from the `.mjs` config path fails at config load, move `renderStaticMath` + `OPTS` into `src/lib/katex-opts.mjs` (plain JS) and import it from both `tex.ts` and the plugin — one definition of the options either way.
- [ ] **Step 4: Run** tests → PASS. Build with `dev/math.mdx` containing: inline `$a\cdot b$`, block `$$|a-b|^2 = |a|^2+|b|^2-2|a||b|\cos\theta$$`, `$a < b$`, `$\{x\}$`, math inside a JSX attribute-free paragraph next to `{expressions}`; open it in dev in light and dark. Record in AGENTS.md "## Math authoring": syntax, the no-Unicode rule, any MDX escaping trap found (e.g. `{`/`}` and `_` outside math).
- [ ] **Step 5: Commit** `git commit -m "feat: build-time KaTeX math in MDX via satteri math feature"`.

---

### Task 2: Lesson store (vals, hot, step)

**Files:**
- Create: `src/viz/store.ts`
- Test: `tests/js/viz-store.test.ts`

**Interfaces:**
- Produces:

```ts
export type Val = number | readonly number[];
export interface LessonState {
	vals: Readonly<Record<string, Val>>;
	hot: ReadonlySet<string>;          // highlighted term ids
	step: Readonly<Record<string, number>>; // timeline id -> current step index
	set(key: string, v: Val): void;
	setHot(ids: Iterable<string>): void;
	setStep(timeline: string, i: number): void;
}
export function lessonStore(lesson: string, init?: Record<string, Val>): StoreApi<LessonState>;
export function useVal<T extends Val>(lesson: string, key: string, fallback: T): T;
export function useHot(lesson: string, id: string): boolean;
export function useStep(lesson: string, timeline: string): number;
```

- [ ] **Step 1: Failing test**

```ts
import { expect, test } from 'bun:test';
import { lessonStore } from '../../src/viz/store';

test('one store per lesson id, shared by every caller (islands share it via the module singleton)', () => {
	const a = lessonStore('dot-product', { theta: 60 });
	const b = lessonStore('dot-product', { theta: 999 }); // later init must not overwrite
	expect(a).toBe(b);
	expect(a.getState().vals.theta).toBe(60);
	expect(lessonStore('projection')).not.toBe(a);
});

test('init merges keys that are missing (two islands may declare different defaults)', () => {
	lessonStore('basis', { u1: [1, 0] });
	const s = lessonStore('basis', { u2: [0, 1] });
	expect(s.getState().vals).toEqual({ u1: [1, 0], u2: [0, 1] });
});

test('set, hot and step', () => {
	const s = lessonStore('length');
	s.getState().set('v', [3, 4]);
	s.getState().setHot(['norm', 'square']);
	s.getState().setStep('derive', 2);
	expect(s.getState().vals.v).toEqual([3, 4]);
	expect([...s.getState().hot].sort()).toEqual(['norm', 'square']);
	expect(s.getState().step.derive).toBe(2);
});

test('setStep clamps negative to 0', () => {
	const s = lessonStore('vector');
	s.getState().setStep('t', -3);
	expect(s.getState().step.t).toBe(0);
});
```

- [ ] **Step 2: Run** `bun test tests/js/viz-store.test.ts` → FAIL (module not found).
- [ ] **Step 3: Implement**

```ts
/**
 * Per-lesson shared state for linked views. Islands are separate React roots, so the link between a scene,
 * a formula and a table is this module-level registry: every island that imports it from the same Vite chunk
 * gets the same store for the same lesson id (verified by Spike A in Task 4).
 * `hot` holds term ids (the ids used in `\htmlData{t=…}`, NumTable cells and scene objects) so hovering any
 * view highlights the same thing everywhere.
 * Call `lessonStore(id, init)` with `init` only at module level of a lesson's .tsx file: merging init during a
 * React render would update a store other mounted islands subscribe to ("Cannot update a component while
 * rendering a different component"). Components use the hooks, which never initialize.
 */
import { useStore } from 'zustand';
import { createStore, type StoreApi } from 'zustand/vanilla';

export type Val = number | readonly number[];

export interface LessonState {
	vals: Readonly<Record<string, Val>>;
	hot: ReadonlySet<string>;
	step: Readonly<Record<string, number>>;
	set(key: string, v: Val): void;
	setHot(ids: Iterable<string>): void;
	setStep(timeline: string, i: number): void;
}

const stores = new Map<string, StoreApi<LessonState>>();

export function lessonStore(lesson: string, init: Record<string, Val> = {}): StoreApi<LessonState> {
	let s = stores.get(lesson);
	if (!s) {
		s = createStore<LessonState>()((set) => ({
			vals: {},
			hot: new Set(),
			step: {},
			set: (key, v) => set((st) => ({ vals: { ...st.vals, [key]: v } })),
			setHot: (ids) => set({ hot: new Set(ids) }),
			setStep: (t, i) => set((st) => ({ step: { ...st.step, [t]: Math.max(0, Math.trunc(i)) } })),
		}));
		stores.set(lesson, s);
	}
	const missing = Object.entries(init).filter(([k]) => !(k in s.getState().vals));
	if (missing.length) s.setState((st) => ({ vals: { ...st.vals, ...Object.fromEntries(missing) } }));
	return s;
}

export function useVal<T extends Val>(lesson: string, key: string, fallback: T): T {
	return useStore(lessonStore(lesson), (st) => (st.vals[key] as T | undefined) ?? fallback);
}

export function useHot(lesson: string, id: string): boolean {
	return useStore(lessonStore(lesson), (st) => st.hot.has(id));
}

export function useStep(lesson: string, timeline: string): number {
	return useStore(lessonStore(lesson), (st) => st.step[timeline] ?? 0);
}
```

- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `git commit -am "feat(viz): per-lesson linked-view store"` (add new files first).

---

### Task 3: KaTeX with term ids and scrub markers; `Formula` and `Num`

**Files:**
- Modify/Create: `src/viz/tex.ts`, `src/viz/Formula.tsx`, `src/viz/Num.tsx`, `src/viz/scrub.ts`
- Test: `tests/js/tex.test.ts`, `tests/js/scrub.test.ts`

**Interfaces:**
- Consumes: `lessonStore`, `useVal`, `useHot` (Task 2).
- Produces:

```ts
// tex.ts
export function renderStaticMath(tex: string, block: boolean): string;        // used by Task 1's path
export type Derived = Record<string, (vals: Record<string, Val>) => number>; // e.g. { dot: (v) => dot(v.a, v.b) }
export function renderLinked(tex: string, vals: Record<string, Val>, fmt?: Record<string, (v: number) => string>, block?: boolean, derived?: Derived): string;
// In TeX: \t{id}{body} marks a term; \s{key} inserts the current value of vals[key] as a scrub handle.
// scrub.ts
export interface ScrubSpec { min: number; max: number; step: number }
export function scrubValue(start: number, dxPx: number, spec: ScrubSpec, pxPerStep?: number): number;
export function keyStep(v: number, key: string, spec: ScrubSpec): number | null; // ArrowUp/Right +step, Down/Left -step, PageUp/Down ×10
// Formula.tsx
export interface FormulaProps { lesson: string; tex: string; scrub?: Record<string, ScrubSpec>; fmt?: Record<string, (v: number) => string>; derived?: Derived; block?: boolean; label: string }
export function Formula(p: FormulaProps): React.JSX.Element;
// Derived values are computed from `vals` before expansion and may be shown with \s{key} (read-only: not in `scrub`).
// Formula/Num are used from lesson .tsx modules only (functions in props cannot cross an Astro island boundary).
// useScrub.ts — the one pointer-drag implementation, shared by Formula and Num
export function startScrubDrag(e: React.PointerEvent, captureEl: HTMLElement, onValue: (dxPx: number) => void): void;
// Num.tsx — scrubbable number in prose
export function Num(p: { lesson: string; k: string; spec: ScrubSpec; fmt?: (v: number) => string; label: string }): React.JSX.Element;
```

Authoring macros (author-friendly, expanded before KaTeX): `\t{dot}{a\cdot b}` → `\htmlData{t=dot}{a\cdot b}`; `\s{theta}` → `\htmlData{s=theta}{60^\circ}` (value formatted with `fmt.theta` or 2 decimals).

- [ ] **Step 1: Failing tests**

```ts
// tests/js/tex.test.ts
import { expect, test } from 'bun:test';
import { renderLinked, renderStaticMath } from '../../src/viz/tex';

test('term macro becomes a data-t span', () => {
	const html = renderLinked(String.raw`\t{dot}{a\cdot b} = \t{norm-a}{|a|}`, {});
	expect(html).toContain('data-t="dot"');
	expect(html).toContain('data-t="norm-a"');
	expect(html).not.toContain('katex-error');
});

test('scrub macro inserts the formatted current value with a data-s handle', () => {
	const html = renderLinked(String.raw`\theta = \s{theta}`, { theta: 60 }, { theta: (v) => `${v}^\\circ` });
	expect(html).toContain('data-s="theta"');
	expect(html).toMatch(/60/);
});

test('only htmlData is trusted (no href/style injection)', () => {
	// KaTeX does not throw for an untrusted command; it renders it as unsupported text. Assert no link is emitted.
	const html = renderLinked(String.raw`\href{javascript:alert(1)}{x} \htmlStyle{color:red}{y}`, {});
	expect(html).not.toContain('href=');
	expect(html).not.toContain('javascript:');
	expect(html).not.toContain('color:red');
});

test('derived values are computed before expansion and shown read-only', () => {
	const html = renderLinked(String.raw`a\cdot b = \s{dot}`, { a: [1, 2], b: [3, 4] }, undefined, false, {
		dot: (v) => (v.a as number[])[0] * (v.b as number[])[0] + (v.a as number[])[1] * (v.b as number[])[1],
	});
	expect(html).toContain('data-s="dot"');
	expect(html).toMatch(/11/);
});

test('static math', () => {
	expect(renderStaticMath(String.raw`\sum_i a_i b_i`, true)).toContain('katex-display');
});
```

```ts
// tests/js/scrub.test.ts
import { expect, test } from 'bun:test';
import { keyStep, scrubValue } from '../../src/viz/scrub';
const spec = { min: 0, max: 180, step: 1 };
test('drag maps pixels to steps and clamps', () => {
	expect(scrubValue(60, 40, spec, 4)).toBe(70);
	expect(scrubValue(60, -1000, spec, 4)).toBe(0);
	expect(scrubValue(60, 1000, spec, 4)).toBe(180);
});
test('keys', () => {
	expect(keyStep(60, 'ArrowUp', spec)).toBe(61);
	expect(keyStep(60, 'PageDown', spec)).toBe(50);
	expect(keyStep(0, 'ArrowLeft', spec)).toBe(0);
	expect(keyStep(60, 'a', spec)).toBeNull();
});
test('step snapping avoids float drift', () => {
	expect(scrubValue(0.1, 8, { min: 0, max: 1, step: 0.1 }, 4)).toBe(0.3);
});
```

- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement `tex.ts`**

```ts
/**
 * KaTeX rendering for the course. Linked formulas mark terms with `\t{id}{…}` and live values with `\s{key}`;
 * both expand to KaTeX's `\htmlData`, which emits `data-*` attributes we use for hover-linking and scrubbing.
 * `\htmlData` (not `\htmlId`) because one term may appear in several formulas on a page and ids must be unique.
 * trust is limited to `\htmlData` so content can never inject links or styles.
 */
import katex from 'katex';
import type { Val } from './store';

const OPTS = {
	throwOnError: true,
	output: 'html' as const,
	trust: (ctx: { command: string }) => ctx.command === '\\htmlData',
	strict: (code: string) => (code === 'htmlExtension' ? 'ignore' : 'error'),
};

export function renderStaticMath(tex: string, block: boolean): string {
	return katex.renderToString(tex, { ...OPTS, displayMode: block });
}

/** Expands `\t{id}{` → `\htmlData{t=id}{` and `\s{key}` → `\htmlData{s=key}{<value>}`. */
export function expand(tex: string, vals: Record<string, Val>, fmt: Record<string, (v: number) => string> = {}): string {
	return tex
		.replace(/\\t\{([\w-]+)\}\{/g, (_, id: string) => `\\htmlData{t=${id}}{`)
		.replace(/\\s\{([\w-]+)\}/g, (_, k: string) => {
			const v = vals[k];
			if (typeof v !== 'number') throw new Error(`Formula: no numeric value for scrub key "${k}"`);
			const shown = fmt[k]?.(v) ?? String(Math.round(v * 100) / 100);
			return `\\htmlData{s=${k}}{${shown}}`;
		});
}

export type Derived = Record<string, (vals: Record<string, Val>) => number>;

export function renderLinked(tex: string, vals: Record<string, Val>, fmt?: Record<string, (v: number) => string>, block = false, derived: Derived = {}): string {
	const all = { ...vals, ...Object.fromEntries(Object.entries(derived).map(([k, f]) => [k, f(vals)])) };
	return katex.renderToString(expand(tex, all, fmt), { ...OPTS, displayMode: block });
}
```

`scrub.ts`:

```ts
/** Pure scrub math shared by Formula, Num and scene handles: values snap to `step` multiples from `min`. */
export interface ScrubSpec { min: number; max: number; step: number }

const snap = (v: number, s: ScrubSpec) => {
	const n = Math.round((v - s.min) / s.step);
	const out = s.min + n * s.step;
	// Re-round to the step's decimals so 0.1 steps never print 0.30000000000000004.
	const dec = Math.max(0, -Math.floor(Math.log10(s.step)));
	return Math.min(s.max, Math.max(s.min, Number(out.toFixed(dec))));
};

export function scrubValue(start: number, dxPx: number, spec: ScrubSpec, pxPerStep = 4): number {
	return snap(start + Math.round(dxPx / pxPerStep) * spec.step, spec);
}

export function keyStep(v: number, key: string, spec: ScrubSpec): number | null {
	const d = { ArrowUp: 1, ArrowRight: 1, ArrowDown: -1, ArrowLeft: -1, PageUp: 10, PageDown: -10 }[key];
	return d === undefined ? null : snap(v + d * spec.step, spec);
}
```

- [ ] **Step 4: Implement `Formula.tsx` and `Num.tsx`**

Behavior (code as below; keep it this small):

```tsx
/**
 * A linked formula: hover/focus a term → it and every view showing the same term id light up;
 * drag/arrow-key a `\s{key}` value → the store updates and every view follows.
 * Re-rendering KaTeX on each value change is ~1 ms, throttled to animation frames.
 */
import { useEffect, useMemo, useRef } from 'react';
import { useStore } from 'zustand';
import { lessonStore } from './store';
import { renderLinked, type Derived } from './tex';
import { keyStep, scrubValue, type ScrubSpec } from './scrub';
import { startScrubDrag } from './useScrub';

export interface FormulaProps { lesson: string; tex: string; scrub?: Record<string, ScrubSpec>; fmt?: Record<string, (v: number) => string>; derived?: Derived; block?: boolean; label: string }

export function Formula(p: FormulaProps): React.JSX.Element {
	const store = lessonStore(p.lesson);
	const vals = useStore(store, (s) => s.vals);
	const hot = useStore(store, (s) => s.hot);
	const html = useMemo(() => renderLinked(p.tex, vals, p.fmt, p.block, p.derived), [p.tex, vals, p.fmt, p.block, p.derived]);
	const root = useRef<HTMLDivElement>(null);

	// Reflect `hot` onto term spans after every render (KaTeX output is plain HTML, not React).
	useEffect(() => {
		root.current?.querySelectorAll<HTMLElement>('[data-t]').forEach((el) => el.classList.toggle('is-hot', hot.has(el.dataset.t!)));
		root.current?.querySelectorAll<HTMLElement>('[data-s]').forEach((el) => {
			const k = el.dataset.s!;
			const spec = p.scrub?.[k];
			if (!spec) return;
			el.tabIndex = 0;
			el.setAttribute('role', 'slider');
			el.setAttribute('aria-label', k);
			el.setAttribute('aria-valuemin', String(spec.min));
			el.setAttribute('aria-valuemax', String(spec.max));
			el.setAttribute('aria-valuenow', String(vals[k]));
		});
	}, [html, hot, vals, p.scrub]);

	const term = (e: React.SyntheticEvent) => (e.target as HTMLElement).closest<HTMLElement>('[data-t]')?.dataset.t;
	const handle = (e: React.SyntheticEvent) => (e.target as HTMLElement).closest<HTMLElement>('[data-s]');

	const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
		// The handle span is only read for its key: the first value change re-renders innerHTML and removes it,
		// so capture and listeners live on the root div, which survives re-renders.
		const k = handle(e)?.dataset.s;
		const spec = k ? p.scrub?.[k] : undefined;
		if (!k || !spec) return;
		const v0 = store.getState().vals[k] as number;
		startScrubDrag(e, e.currentTarget, (dx) => store.getState().set(k, scrubValue(v0, dx, spec)));
	};

	const onKeyDown = (e: React.KeyboardEvent) => {
		const k = handle(e)?.dataset.s;
		const spec = k ? p.scrub?.[k] : undefined;
		if (!k || !spec) return;
		const v = keyStep(store.getState().vals[k] as number, e.key, spec);
		if (v === null) return;
		e.preventDefault();
		store.getState().set(k, v);
	};

	return (
		<div
			ref={root}
			className={`lab-formula not-content${p.block ? ' is-block' : ''}`}
			aria-label={p.label}
			onPointerOver={(e) => { const t = term(e); if (t) store.getState().setHot([t]); }}
			onPointerOut={(e) => {
				// Moving between nested KaTeX spans of the same term fires pointerout; clear only when leaving the term.
				const next = (e.relatedTarget as HTMLElement | null)?.closest?.('[data-t]');
				if (!next) store.getState().setHot([]);
			}}
			onFocus={(e) => { const t = term(e); if (t) store.getState().setHot([t]); }}
			onPointerDown={onPointerDown}
			onKeyDown={onKeyDown}
			dangerouslySetInnerHTML={{ __html: html }}
		/>
	);
}
```

Note for the implementer: React re-sets `innerHTML` only when `html` changes, which also drops focus from a `[data-s]` handle during keyboard scrubbing. After re-render, restore focus to the handle with the same `data-s` if it had focus (store the key in a ref in `onKeyDown`, refocus in the effect). Write this, it is required for keyboard accessibility.

`useScrub.ts` (the single drag loop):

```ts
/**
 * Pointer-drag loop for scrubbable numbers. Capture goes on an element that outlives value re-renders
 * (Formula's root div, Num's own span). Updates are throttled to animation frames.
 */
export function startScrubDrag(e: React.PointerEvent, captureEl: HTMLElement, onValue: (dxPx: number) => void): void {
	e.preventDefault();
	captureEl.setPointerCapture(e.pointerId);
	const x0 = e.clientX;
	let raf = 0;
	const ctl = new AbortController();
	captureEl.addEventListener('pointermove', (ev) => {
		cancelAnimationFrame(raf);
		raf = requestAnimationFrame(() => onValue(ev.clientX - x0));
	}, { signal: ctl.signal });
	const end = () => { cancelAnimationFrame(raf); ctl.abort(); };
	captureEl.addEventListener('pointerup', end, { signal: ctl.signal });
	captureEl.addEventListener('pointercancel', end, { signal: ctl.signal });
}
```

`Num.tsx`: a `<span role="slider" tabIndex={0} className="lab-num">` showing `fmt(v)`, `onPointerDown` → `startScrubDrag(e, e.currentTarget, …)`, `onKeyDown` → `keyStep`. No second drag implementation.

CSS (append to `lab.css`): `.lab-formula [data-t]{border-radius:3px;transition:background-color .12s}` `.lab-formula [data-t].is-hot{background:var(--viz-hot-bg)}` `.lab-formula [data-s], .lab-num{cursor:ew-resize;border-bottom:1px dashed currentColor;touch-action:none}` and a `@media (prefers-reduced-motion: reduce){.lab-formula [data-t]{transition:none}}`. KaTeX CSS is already global via `customCss` (Task 0).

Demo: in `src/viz/demo/engine.tsx` export `DemoFormula` (module-level `lessonStore('demo', { theta: 60 })`, formula `\theta = \s{theta}`, scrub 0..180) and mount it in `dev/engine.mdx` with `client:only="react"`. Browser check: drag the value continuously for > 1 s (must keep updating — regression check for the removed-span bug), arrow keys keep focus on the handle.

- [ ] **Step 5: Run** `bun test tests/js` → PASS.
- [ ] **Step 6: Commit** `git commit -m "feat(viz): linked KaTeX formulas with term ids and scrubbable values"`.

---

### Task 4: Spike A — one shared canvas, scenes as drei Views in separate islands

**Files:**
- Create: `src/viz/scene/SceneHost.tsx`, `src/viz/scene/Stage.tsx`, `src/viz/scene/webgl.ts`, `src/viz/SceneIsland.astro`, `src/components/overrides/PageFrame.astro`, `src/viz/demo/engine.tsx`, `src/content/docs/dev/engine.mdx` (hidden, not draft — see Global Constraints)
- Modify: `astro.config.mjs` (add `PageFrame` override), `AGENTS.md` (record result)
- Test: `tests/js/webgl.test.ts`

**Interfaces:**
- Produces:

```ts
// Stage.tsx
export function Stage(p: { lesson: string; mode: '2d' | '3d'; extent: number; caption: string; children: React.ReactNode; overlay?: React.ReactNode }): React.JSX.Element;
// `overlay` = DOM content drawn over the scene (keyboard handle buttons, labels). It is a SIBLING of the drei
// <View> div inside a positioned `.lab-stage` wrapper, never a child: drei's View raycasts only when
// event.target is the View div itself, so DOM children would swallow hover/drag.
// webgl.ts
export function webglAvailable(make?: () => HTMLCanvasElement): boolean;
```

- `SceneIsland.astro`: `<SceneIsland caption="…" height="20rem">` + a React child with `client:only="react"`; prints the caption in a `<figcaption>`, reserves `min-height`.

Design (primary path, from storyboard R1): `PageFrame` override renders the stock frame plus `<SceneHost client:only="react" />`. `SceneHost` = a `position:fixed; inset:0; pointer-events:none; z-index:0` `<Canvas frameloop="demand" eventSource={document.body}>` containing `<View.Port />`. Each `Stage` renders drei `<View>` (a DOM div in the article flow, `pointer-events:auto`) whose children (camera, grid, arrows) are tunneled into the shared canvas. Any store change, scroll or resize calls `invalidate()` (drei updates a View's scissor rect only inside a rendered frame — with `frameloop="demand"` and no invalidate on scroll, scenes would stay where they were on screen).

`PageFrame.astro` (verified slots of Starlight 0.42's PageFrame: `header`, `sidebar`, default — re-check in `node_modules/@astrojs/starlight/components/PageFrame.astro`):

```astro
---
/**
 * Starlight PageFrame override: the stock frame plus the page's single shared WebGL canvas (SceneHost).
 * Slots must be forwarded or the header and sidebar disappear.
 */
import Default from '@astrojs/starlight/components/PageFrame.astro';
import SceneHost from '../../viz/scene/SceneHost';
---

<Default {...Astro.props}>
	<slot name="header" slot="header" />
	<slot name="sidebar" slot="sidebar" />
	<slot />
</Default>
<SceneHost client:only="react" />
```

Labels: drei `Html` inside a View portals into the root's `events.connected`, which every View overwrites with its own div, so with several Views labels can land in the wrong one. `Label` therefore is NOT drei `Html`: it is DOM inside the Stage `overlay`, positioned by projecting the world point with the View's camera (`vector.project(camera)` → percent of the stage box), recomputed on store change and camera change (orbit). Record in AGENTS.md if Spike A shows a simpler correct path.

- [ ] **Step 1: Failing test for `webglAvailable`** (Review Focus 3)

```ts
import { expect, test } from 'bun:test';
import { webglAvailable } from '../../src/viz/scene/webgl';
test('false when no context can be created', () => {
	expect(webglAvailable(() => ({ getContext: () => null }) as unknown as HTMLCanvasElement)).toBe(false);
});
test('true when webgl2 is available', () => {
	expect(webglAvailable(() => ({ getContext: (k: string) => (k === 'webgl2' ? {} : null) }) as unknown as HTMLCanvasElement)).toBe(true);
});
```

Implement `webgl.ts`:

```ts
/** Feature check before mounting the shared canvas; without WebGL the captions/formulas/tables still teach. */
export function webglAvailable(make: () => HTMLCanvasElement = () => document.createElement('canvas')): boolean {
	try {
		const c = make();
		return !!(c.getContext('webgl2') ?? c.getContext('webgl'));
	} catch {
		return false;
	}
}
```

- [ ] **Step 2: Implement SceneHost + Stage (primary path)**

```tsx
// SceneHost.tsx
/**
 * The one WebGL context of a page. Browsers cap live contexts (~16 per page on desktop Chromium, fewer on
 * mobile; the oldest is lost), and a lesson has several scenes, so every Stage is a drei <View> rendered here.
 * frameloop="demand": a textbook page must not burn GPU while idle; stages call invalidate() on change.
 */
import { Canvas, useThree } from '@react-three/fiber';
import { View } from '@react-three/drei';
import { useEffect, useState } from 'react';
import { webglAvailable } from './webgl';

/** drei recomputes each View's scissor rect only while rendering; on-demand rendering must be poked on scroll/resize. */
function InvalidateOnScroll() {
	const invalidate = useThree((s) => s.invalidate);
	useEffect(() => {
		let raf = 0;
		const poke = () => { cancelAnimationFrame(raf); raf = requestAnimationFrame(() => invalidate()); };
		window.addEventListener('scroll', poke, { passive: true });
		window.addEventListener('resize', poke);
		return () => { cancelAnimationFrame(raf); window.removeEventListener('scroll', poke); window.removeEventListener('resize', poke); };
	}, [invalidate]);
	return null;
}

const markNoWebgl = () => document.documentElement.classList.add('lab-no-webgl'); // CSS shows the inline notice in every stage

export default function SceneHost(): React.JSX.Element | null {
	const [ok] = useState(webglAvailable);
	useEffect(() => { if (!ok) markNoWebgl(); }, [ok]);
	if (!ok) return null;
	return (
		<Canvas
			className="lab-scene-host"
			frameloop="demand"
			eventSource={document.body}
			onCreated={({ gl }) => gl.domElement.addEventListener('webglcontextlost', markNoWebgl)}
			style={{ position: 'fixed', inset: 0, pointerEvents: 'none', zIndex: 0 }}
		>
			<InvalidateOnScroll />
			<View.Port />
		</Canvas>
	);
}
```

```tsx
// Stage.tsx
/** A scene slot in the article. 2D = orthographic camera (pixel-stable lines); 3D = perspective + orbit. */
import { OrbitControls, OrthographicCamera, PerspectiveCamera, View } from '@react-three/drei';
import { useThree } from '@react-three/fiber';
import { useEffect } from 'react';
import { lessonStore } from '../store';

function InvalidateOnStore({ lesson }: { lesson: string }) {
	const invalidate = useThree((s) => s.invalidate);
	useEffect(() => lessonStore(lesson).subscribe(() => invalidate()), [lesson, invalidate]);
	return null;
}

export function Stage(p: { lesson: string; mode: '2d' | '3d'; extent: number; caption: string; children: React.ReactNode; overlay?: React.ReactNode }): React.JSX.Element {
	return (
		<div className="lab-stage not-content" role="group" aria-label={p.caption}>
			<View className="lab-stage-view">
				<InvalidateOnStore lesson={p.lesson} />
				{p.mode === '2d' ? (
					<>
						<OrthographicCamera makeDefault position={[0, 0, 10]} zoom={1} />
						<FitOrtho extent={p.extent} />
					</>
				) : (
					<>
						<PerspectiveCamera makeDefault position={[p.extent * 1.6, p.extent * 1.2, p.extent * 1.8]} fov={40} />
						<OrbitControls enableDamping={false} minDistance={p.extent} maxDistance={p.extent * 6} makeDefault />
					</>
				)}
				{p.children}
			</View>
			{/* Sibling of the View div (see interface note): DOM here never blocks scene raycasts outside its own boxes. */}
			<div className="lab-stage-overlay">{p.overlay}</div>
			<p className="lab-stage-nogl">This scene needs WebGL, which is unavailable here. The formula and table next to it show the same values.</p>
		</div>
	);
}
```

The 2D orthographic zoom must fit `extent` world units into the element's smaller side: implement `FitOrtho` (reads `size` from `useThree`, sets `camera.zoom = Math.min(size.width, size.height) / (2 * extent)`, `camera.updateProjectionMatrix()`, `invalidate()`) and render it inside the 2D branch.

CSS (lab.css): `.lab-stage{position:relative;aspect-ratio:16/10;touch-action:pan-y}` `.lab-stage-view{position:absolute;inset:0}` `.lab-stage-overlay{position:absolute;inset:0;pointer-events:none}` `.lab-stage-overlay > *{pointer-events:auto}` `.lab-stage-nogl{display:none}` `.lab-no-webgl .lab-stage-nogl{display:block}`.

- [ ] **Step 3: Demo page with three stages in three separate islands, all on lesson id `demo`** — in `src/viz/demo/engine.tsx`: module-level `lessonStore('demo', { x: 0 })`; export `DemoBoxA`, `DemoBoxB`, `DemoBoxC` (each a `Stage` drawing a box at `useVal('demo','x',0)`) and a tiny `DemoSlider` (an `<input type="range">` writing `x`; Formula arrives in Task 3). `dev/engine.mdx` mounts each inside `<SceneIsland caption="…">…client:only="react"…</SceneIsland>`, with two screens of prose between B and C to test scrolling.
- [ ] **Step 4: Verify in the browser** (`bun run dev`, open `/math-course/dev/engine/`) using the claude-in-chrome tools: (1) console has no errors and no "too many active WebGL contexts"; (2) `document.querySelectorAll('canvas').length === 1`; (3) moving the slider moves all three boxes (screenshots before/after); (4) scrolling keeps boxes aligned with their slots (screenshot mid-scroll); (5) toggling to `prefers-reduced-motion` changes nothing broken; (6) in DevTools force-lose the context (`WEBGL_lose_context`) → the no-WebGL notice appears in every stage.
- [ ] **Step 5: Decide.** If (2) or (3) fails because islands get separate tunnel/store instances, check `dist` for duplicated `drei`/`zustand` chunks; try `vite.build.rollupOptions.output.manualChunks: { viz: ['three','@react-three/fiber','@react-three/drei','zustand'] }`. If it still fails, switch to the fallback: `Stage` renders its own `<Canvas frameloop="demand">` (no View), `client:visible`, unmounting via IntersectionObserver when off-screen, and AGENTS.md gets the rule "max 6 stages per page". Record the outcome and evidence in AGENTS.md "## Scenes (Spike A result)".
- [ ] **Step 6: Commit** `git commit -m "spike(viz): shared canvas with drei View across islands"`.

---

### Task 5: Spike C — theme palette for WebGL

**Files:**
- Create: `src/viz/palette.ts`
- Modify: `src/styles/lab.css` (viz vars for both themes)
- Test: `tests/js/palette.test.ts`

**Interfaces:**
- Produces: `type Role = 'vecA'|'vecB'|'vecC'|'axisX'|'axisY'|'axisZ'|'grid'|'gridMinor'|'shadow'|'hot'|'ink'|'muted'|'fill'`; `readPalette(get?: (name: string) => string): Record<Role, string>`; `usePalette(): Record<Role, string>` — `useSyncExternalStore` over ONE module-level `MutationObserver` on `document.documentElement` (`attributeFilter: ['data-theme']`); `Stage` does `useEffect(() => invalidate(), [palette])` so a theme switch re-renders the scenes.

- [ ] **Step 1:** Confirm the theme attribute (reviewed: Starlight 0.42.6 `components/ThemeProvider.astro:13` sets `document.documentElement.dataset.theme` to `'light'` or `'dark'` — always present). Re-check in the installed package and write the file:line into the test file's header comment. CSS: dark values under `:root`, light under `:root[data-theme='light']` (Starlight's own convention).
- [ ] **Step 2: Failing test**

```ts
import { expect, test } from 'bun:test';
import { readPalette } from '../../src/viz/palette';
test('reads every role from --viz-* custom properties', () => {
	const css: Record<string, string> = { '--viz-vec-a': ' #1f77b4 ', '--viz-vec-b': '#ff7f0e' };
	const p = readPalette((n) => css[n] ?? '#000000');
	expect(p.vecA).toBe('#1f77b4');
	expect(p.vecB).toBe('#ff7f0e');
	expect(Object.keys(p).length).toBe(13);
});
test('missing variable is an error, not a silent black', () => {
	expect(() => readPalette(() => '')).toThrow(/--viz-/);
});
```

- [ ] **Step 3: Implement** `palette.ts` (map Role → `--viz-*` name; trim; throw `Error(\`missing CSS variable ${name}\`)` on empty) and `lab.css` vars: blue/orange pair for a/b (color-blind safe, not red/green), axes x red-ish / y green-ish / z blue-ish *with labels always shown* (color is never the only cue), grid low-contrast; light values under `:root[data-theme='light']`, dark under `:root`. Add `--viz-hot-bg` for formula highlight. Then wire `usePalette` into `Stage` (invalidate on change) and into the demo boxes.
- [ ] **Step 4: Browser check** on the demo page: toggle theme → box/grid colors change without reload (screenshot both).
- [ ] **Step 5: Commit** `git commit -m "feat(viz): theme-aware palette roles for scenes"`.

---

### Task 6: Geometry helpers and scene primitives — executed as three reviewable tasks

- **6a** = Steps 1–4 below (pure `geom.ts` + `drag.ts` with tests). Commit `feat(viz): geometry and hit-test helpers`.
- **6b** = Grid, Arrow, Point, Segment + the drag/keyboard machinery (Step 5) + demo. Commit `feat(viz): draggable arrows, points and grid`.
- **6c** = AngleArc, RightAngle, AreaTile, Parallelogram, Label (overlay projection, see Task 4) + the full demo (Step 6). Commit `feat(viz): angle, area and label primitives`.

**Files:**
- Create: `src/viz/geom.ts`, `src/viz/scene/{Grid,Arrow,Point,Segment,AngleArc,AreaTile,Parallelogram,Label}.tsx`, `src/viz/scene/drag.ts`
- Test: `tests/js/geom.test.ts`, `tests/js/drag.test.ts`

**Interfaces:**
- Produces (`geom.ts`, all pure, arrays `[x,y]` or `[x,y,z]`):

```ts
export type V = readonly number[];
export const add: (a: V, b: V) => number[];
export const sub: (a: V, b: V) => number[];
export const scale: (a: V, s: number) => number[];
export const dot: (a: V, b: V) => number;
export const norm: (a: V) => number;
export function normalize(a: V): number[];                 // throws on zero length (callers must guard; no silent NaN)
export function angleBetween(a: V, b: V): number;         // radians, acos clamped to [-1,1]
export function project(a: V, onto: V): number[];         // vector projection
export function arcPoints(center: V, from: V, to: V, r: number, n?: number): number[][]; // 2D/3D arc for AngleArc
export function signedArea2(a: V, b: V): number;          // a.x*b.y - a.y*b.x
```

- `drag.ts`: `hitHandle(pointerPx: [number, number], handlesPx: { id: string; at: [number, number] }[], radiusPx = 14): string | null` — Review Focus 2: only a hit starts a drag; otherwise the event is left alone so the page scrolls.
- Components: `Arrow { lesson; id; from?: V | string; to: V | string; role: Role; label?: string; drag?: 'tip' | 'whole' | 'none'; legs?: boolean }` — a `string` means "store key"; drag writes the store; `useHot(lesson, id)` thickens/brightens; hovering sets hot. `Point { lesson; id; at; drag?; label? }`, `Segment { lesson; id; a; b; dashed?; role }`, `AngleArc { lesson; id; at; from; to; r; label? }`, `RightAngle { at; u; v; size }`, `AreaTile { lesson; id; a; b; role; signed?: boolean }` (square built outward on segment a→b), `Parallelogram { lesson; id; a; b; role }`, `Grid { extent; step; minor?: number; axes?: boolean }`, `Label { at: V | string; children }` — DOM in the Stage `overlay`, positioned by projecting `at` with the View camera (NOT drei `Html`, see Task 4); `pointer-events:none`; plain text/number, never heavy KaTeX.

- [ ] **Step 1: Failing tests** — `geom.test.ts`:

```ts
import { expect, test } from 'bun:test';
import { angleBetween, arcPoints, dot, norm, normalize, project, signedArea2 } from '../../src/viz/geom';
test('dot/norm/angle', () => {
	expect(dot([1, 2], [3, 4])).toBe(11);
	expect(norm([3, 4])).toBe(5);
	expect(angleBetween([1, 0], [0, 1])).toBeCloseTo(Math.PI / 2);
	expect(angleBetween([1, 0], [1, 1e-17])).toBe(0); // acos clamp: no NaN from rounding above 1
});
test('projection and perpendicular remainder', () => {
	const p = project([3, 4], [1, 0]);
	// toEqual distinguishes -0 from 0, which scale() may produce; compare numerically.
	expect(p[0]).toBeCloseTo(3);
	expect(p[1]).toBeCloseTo(0);
	expect(dot(p, [0, 4])).toBeCloseTo(0);
});
test('normalize zero throws', () => {
	expect(() => normalize([0, 0])).toThrow();
});
test('signed area orientation', () => {
	expect(signedArea2([1, 0], [0, 1])).toBe(1);
	expect(signedArea2([0, 1], [1, 0])).toBe(-1);
});
test('arc endpoints lie on the radius', () => {
	const pts = arcPoints([0, 0], [1, 0], [0, 1], 2, 8);
	expect(pts[0][0]).toBeCloseTo(2);
	expect(pts.at(-1)![1]).toBeCloseTo(2);
	expect(pts.length).toBe(9);
});
```

`drag.test.ts`:

```ts
import { expect, test } from 'bun:test';
import { hitHandle } from '../../src/viz/scene/drag';
test('a miss returns null so the page can scroll', () => {
	expect(hitHandle([100, 100], [{ id: 'a', at: [200, 200] }])).toBeNull();
});
test('nearest handle within radius wins', () => {
	expect(hitHandle([100, 100], [{ id: 'a', at: [110, 100] }, { id: 'b', at: [104, 100] }])).toBe('b');
});
```

- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** `geom.ts` and `drag.ts` exactly per the signatures (arc: slerp-free — angle from `atan2` in the plane spanned by `from`,`to`; in 3D build the plane basis with Gram–Schmidt of `from`, `to`). **Step 4: Run** → PASS.
- [ ] **Step 5: Components.** Lines with pixel-stable width: drei `Line` (`lineWidth` in px). Arrow head: a small `Cone` (3D) / triangle (2D) oriented along the shaft. Dragging: on `pointerdown` over a handle mesh (invisible hit sphere radius = 14 px in world units derived from the camera), `setPointerCapture`, intersect the pointer ray with the drag plane (2D: z=0; 3D: plane through the handle facing the camera), write `store.set(key, [x,y(,z)])` snapped to 0.1. Touch: CSS `touch-action: pan-y` on `.lab-stage`; switch to `none` only between a handle hit and pointerup (use `hitHandle` on the projected handle positions). Keyboard: each draggable gets a visually hidden `<button>` in the Stage DOM overlay (`aria-label="Move tip of a"`) moving by 0.1 with arrows (one shared overlay component).
- [ ] **Step 6: Demo page** — 2D stage: grid, draggable `a` and `b`, AngleArc, projection Segment, AreaTile on `a`; 3D stage: two arrows with legs; Formula `\t{dot}{a\cdot b} = \s{dot}` with `derived={{ dot: (v) => dot(v.a, v.b) }}` (Task 3 `Derived`) showing a live readout. Browser check incl. 360 px emulation: page scrolls when swiping on empty stage area, drag works on a handle.
- [ ] **Step 7: Commit** `git commit -m "feat(viz): geometry helpers and draggable scene primitives"`.

---

### Task 7: NumTable, Timeline, CodeView

**Files:**
- Create: `src/viz/NumTable.tsx`, `src/viz/Timeline.tsx`, `src/viz/CodeView.tsx`, `src/viz/table.ts`
- Test: `tests/js/table.test.ts`

**Interfaces:**
- `table.ts` (pure): `rowsFor(op: 'components'|'product'|'square'|'diff', a: V, b?: V): { cells: { t: string; v: number }[]; total?: number }` — term ids `a0,a1…`, `p0,p1…` (products), `sq0…`, `d0…`, total id `sum`.
- `NumTable { lesson; rows: { label: string; key: string }[]; op; labels?: string[] }` — cells carry `data-t`, hover sets hot, hot highlights.
- `Timeline { lesson; id; steps: { id: string; text: string }[] }` — Back / Next buttons + `<input type="range">`, writes `setStep(id,i)`; step text in an `aria-live="polite"` region; scenes/formulas read `useStep(lesson,id)` to reveal/strike.
- `CodeView { code: string; lang: 'python'|'ts'; lines?: Record<number, string> }` — lines mapped to term ids; highlighting via build-time Shiki is not available inside React islands, so render plain `<pre>` with line spans and the existing Starlight code font; no runtime highlighter dependency.

- [ ] **Step 1: Failing test**

```ts
import { expect, test } from 'bun:test';
import { rowsFor } from '../../src/viz/table';
test('product rows and total for the dot product', () => {
	const r = rowsFor('product', [1, 2], [3, 4]);
	expect(r.cells).toEqual([{ t: 'p0', v: 3 }, { t: 'p1', v: 8 }]);
	expect(r.total).toBe(11);
});
test('length mismatch is an error', () => {
	expect(() => rowsFor('product', [1, 2], [3])).toThrow();
});
```

- [ ] **Step 2–4:** run (FAIL) → implement → run (PASS).
- [ ] **Step 5:** add to the demo page: NumTable of products for a, b linked to the Formula; a 4-step Timeline that reveals terms of `|a-b|^2` (strike-through class on revealed-cancelled terms via `data-t` + CSS `.is-struck`). Browser check: hover a product cell → the matching formula term highlights and vice versa; keyboard Next/Back works.
- [ ] **Step 6: Commit** `git commit -m "feat(viz): linked number tables, stepped timeline, code view"`.

---

### Task 8: Spike D — Pyodide cell

**Files:**
- Create: `src/viz/py/py.worker.ts`, `src/viz/py/pyclient.ts`, `src/viz/py/PyCell.tsx`
- Test: `tests/js/pyclient.test.ts`

**Interfaces:**
- `pyclient.ts`: `createPyClient(spawn: () => WorkerLike, timeoutMs = 60000): { run(code: string): Promise<{ out: string; err?: string }>; dispose(): void }` — one worker per page (module singleton `sharedPy()`), lazy spawn on first `run`.
- `PyCell { code: string; colab?: string }` — editable `<textarea>` (monospace), Run button, output `<pre>`, first-run note "Downloads Python + NumPy (~tens of MB, cached afterwards)", error → `.lab-msg.is-err` + ColabBadge link.

- [ ] **Step 1: Confirm version.** Reviewed 2026-10-10: `https://cdn.jsdelivr.net/pyodide/v314.0.7/full/pyodide.mjs` → 200 (Pyodide's version tracks CPython 3.14). Re-check the URL, then write `PYODIDE_VERSION = '314.0.7'` + the check date into `py.worker.ts` header comment.

Protocol (fixed by the tests below): the client assigns `worker.onmessage =` (not `addEventListener`); the worker loads Pyodide lazily inside the handling of the first `run` message; exactly one reply `{ id, out, err? }` per `run`; no separate init message.
- [ ] **Step 2: Failing test** (Review Focus 5) — fake worker that never answers:

```ts
import { expect, test } from 'bun:test';
import { createPyClient } from '../../src/viz/py/pyclient';
test('a worker that never answers times out with an actionable error', async () => {
	const fake = { postMessage() {}, terminate() {}, onmessage: null, onerror: null } as unknown as Worker;
	const py = createPyClient(() => fake, 20);
	const r = await py.run('print(1)');
	expect(r.err).toMatch(/could not load Python/i);
});
test('echoes output from the worker protocol', async () => {
	const w: any = { terminate() {}, postMessage(m: { id: number }) { queueMicrotask(() => w.onmessage({ data: { id: m.id, out: '11\n' } })); } };
	const r = await createPyClient(() => w, 1000).run('print(11)');
	expect(r.out).toBe('11\n');
});
```

- [ ] **Step 3: Implement.** Worker: `const { loadPyodide } = await import(/* @vite-ignore */ \`https://cdn.jsdelivr.net/pyodide/v${VER}/full/pyodide.mjs\`)`; `py = await loadPyodide()`; `await py.loadPackage('numpy')`; per message: collect stdout/stderr via `py.setStdout({ batched })`/`setStderr`, `await py.runPythonAsync(code)`, reply `{id,out,err?}`. Client: id-correlated promises; timeout message: "Could not load Python in the browser (network blocked or offline). Use the Colab notebook instead." and terminate+reset the worker. Register the worker with `new Worker(new URL('./py.worker.ts', import.meta.url), { type: 'module' })`.
- [ ] **Step 4: Run** tests → PASS. **Browser check** on demo page: `import numpy as np; print(np.dot([1,2],[3,4]))` → `11`; network tab shows nothing Pyodide-related before the click.
- [ ] **Step 5: Commit** `git commit -m "feat(viz): lazy Pyodide cell with timeout and Colab fallback"`.

---

### Task 9: Term graph, validation and TermMap

**Files:**
- Create: `src/data/terms.json` (seed with the 12 Part-0 terms from the storyboard §3: square, sqrt, coordinate, point, distance, pythagoras, angle, radian, unit-circle, cosine, sine, law-of-cosines), `src/viz/terms/graph.ts`, `src/viz/terms/TermMap.tsx`, `src/viz/terms/elk.worker.ts`
- Test: `tests/js/terms.test.ts`

**Interfaces:**

```ts
export type EdgeType = 'built-on' | 'is-a' | 'special-case-of' | 'equivalent-to' | 'used-in';
export type Depth = 'ground' | 'vectors' | 'machine';
export interface Term { id: string; label: string; def: string; depth: Depth; lessons: string[]; edges: { type: EdgeType; to: string }[] }
export interface TermGraph { terms: Term[] }
export function validate(g: TermGraph, lessonSlugs: readonly string[]): string[];   // list of problems; [] = ok
export function neighborhood(g: TermGraph, focus: string, hops: 1 | 2): { nodes: Term[]; edges: { from: string; to: string; type: EdgeType }[] };
export function ancestry(g: TermGraph, id: string): string[];                       // all built-on ancestors, topological, roots first
```

- [ ] **Step 1: Failing tests**

```ts
import { expect, test } from 'bun:test';
import data from '../../src/data/terms.json';
import { lessons } from '../../src/lib/course';
import { ancestry, neighborhood, validate, type TermGraph } from '../../src/viz/terms/graph';

const g = data as TermGraph;
const slugs = lessons.map((l) => l.slug);

test('the shipped term graph is valid', () => {
	expect(validate(g, slugs)).toEqual([]);
});
test('detects unknown refs, duplicate ids, unknown lessons and built-on cycles', () => {
	const bad: TermGraph = { terms: [
		{ id: 'a', label: 'A', def: 'A is A.', depth: 'ground', lessons: ['nope'], edges: [{ type: 'built-on', to: 'b' }] },
		{ id: 'b', label: 'B', def: 'B is B.', depth: 'ground', lessons: [], edges: [{ type: 'built-on', to: 'a' }, { type: 'used-in', to: 'zzz' }] },
		{ id: 'b', label: 'B2', def: '', depth: 'ground', lessons: [], edges: [] },
	] };
	const p = validate(bad, slugs).join('\n');
	expect(p).toMatch(/duplicate id "b"/);
	expect(p).toMatch(/unknown term "zzz"/);
	expect(p).toMatch(/unknown lesson "nope"/);
	expect(p).toMatch(/cycle.*a.*b/);
	expect(p).toMatch(/empty definition/);
});
test('ancestry of law-of-cosines reaches square', () => {
	expect(ancestry(g, 'law-of-cosines')).toContain('square');
	expect(ancestry(g, 'law-of-cosines').at(-1)).not.toBe('law-of-cosines');
});
test('neighborhood includes both directions', () => {
	const n = neighborhood(g, 'pythagoras', 1).nodes.map((t) => t.id);
	expect(n).toContain('square');        // built on
	expect(n).toContain('law-of-cosines'); // something built on it
});
```

- [ ] **Step 2–4:** run (FAIL) → implement `graph.ts` (cycle detection: DFS with colors over `built-on` edges only; error text `built-on cycle: a → b → a`) → run (PASS). Write the seed `terms.json` using storyboard §3 definitions verbatim for those ids.
- [ ] **Step 5: TermMap.** `TermMap { focus: string; hops?: 1|2 }`, `client:visible`; layout computed in `elk.worker.ts` (`elkjs/lib/elk.bundled.js`, `algorithm: 'layered'`, `elk.direction: 'UP'` so foundations are at the bottom, like "built on"); nodes styled by `depth`; edge style by type (built-on solid with arrow, used-in dotted, equivalent-to double line, special-case/is-a dashed); click a node → its first lesson via `href()`; import `@xyflow/react/dist/style.css` in the TermMap module; focus node highlighted; fit view; full-screen toggle on mobile. Dark/light via CSS vars (React Flow `colorMode` from `data-theme`).
- [ ] **Step 6:** demo page `<TermMap focus="law-of-cosines" hops={2} />`; browser check incl. 360 px.
- [ ] **Step 7: Commit** `git commit -m "feat(terms): validated term graph and interactive term map"`.

---

### Task 10: Notebooks generator and ColabBadge

**Files:**
- Copy/adapt: `notebooks/build/` from `vllm_en` (generator, validation, deterministic ids), `notebooks/README.md`
- Create: `notebooks/build/nb00_smoke.py` (a one-cell smoke notebook; the per-lesson NumPy notebooks for 0.G–1.8 are written in Plan 2), `pyproject.toml` (`[tool.uv]` with `nbformat` only)

- [ ] **Step 1:** Read vllm_en `notebooks/build/build_notebooks.py` + `nb_common.py`; port the generator with vllm-specific cells removed; keep validation + syntax-check + deterministic cell ids.
- [ ] **Step 2:** `00-smoke` notebook: `import numpy as np; print(np.dot([1, 2], [3, 4]))`.
- [ ] **Step 3: Run** `uv run --no-project --with nbformat python notebooks/build/build_notebooks.py` → writes `notebooks/00-smoke.ipynb`; rerun → no diff (deterministic).
- [ ] **Step 4: Commit** `git commit -m "feat(notebooks): deterministic Colab notebook generator"`.

---

### Task 11: Spike E — bundle budget, CI and first deploy

**Files:**
- Create: `scripts/size-report.ts`, `.github/workflows/deploy.yml`, `README.md`
- Modify: `AGENTS.md` (budgets + measured sizes)

- [ ] **Step 1: size-report.ts** — after `astro build`, gzip-measure every `dist/_astro/*.js` with `Bun.gzipSync`; classify: chunk containing `three`'s `WebGLRenderer` = `viz-core`; budgets: `viz-core ≤ 250 kB gz`, any other chunk ≤ 100 kB gz except files matching `elk|xyflow` (≤ 600 kB, lazy) ; print a table; exit 1 on violation. Test it by running on the current build.
- [ ] **Step 2: Measure.** If `viz-core` exceeds the budget, inspect with `bunx vite-bundle-visualizer` or rollup stats; fix by importing drei components individually if tree-shaking fails (verify drei's exports), not by raising the budget. Record measured numbers in AGENTS.md.
- [ ] **Step 3: deploy.yml** — vllm_en's workflow minus the Python lab tests; `test` job runs `bun test tests/js` and `bun run build && bun run size`; same pinned action SHAs and bun 1.3.12.
- [ ] **Step 4: README.md** — what the course is, `bun install`, `bun run dev`, `bun test tests/js`, notebooks command, live URL.
- [ ] **Step 5: Ask the user** to confirm the repo name/visibility, then (only after confirmation) `gh repo create ssoj13/math-course --public --source . --push` and enable Pages (GitHub Actions source). Verify the live URL loads the home page; the hidden dev pages are not in the sidebar or search, and the size report ran on a build that includes them.
- [ ] **Step 6: Commit** `git commit -m "build: bundle budget, CI and Pages deploy"`.

---

## Self-review notes (done while writing)

- Storyboard §4 primitives → Tasks 2–9 (Stage, Grid, Arrow, Point, Segment, AngleArc/RightAngle, AreaTile, Parallelogram, Label, Formula, Num, NumTable, Timeline, CodeView, PyCell, TermMap). "Not yet" list honored: no Tensor/autodiff, MatmulCube, MemoryStrip, animation library, drei Text.
- Risks R1→Task 4, R2→Task 4 Step 5, R3→Global constraints + SceneIsland, R4→Task 5, R5→Task 3, R6→Task 8, R7→Task 11, R8→Tasks 3/6/7 (sliders, keyboard, aria-live), R9→Task 6 (touch), R10→Global constraints, R11→Plan 2 (embeddings data).
- Independent review (Fable, 2026-10-10) applied: C1 composition-in-tsx rule; C2 drag capture on the surviving root; C3 `\href` test asserts output; C4 React 19 `React.JSX`, tsconfig jsx; C5 Task 1 uses satteri `features.math`; C6 hidden (not draft) dev pages so CI measures viz code; I1 PageFrame slot forwarding; I2 invalidate on scroll/resize; I3 overlay as View sibling; I4 Label via overlay projection, not drei Html; I5 store init at module level only; I6 ES workers + `@vite-ignore`, Pyodide 314.0.7 confirmed; I7 worker protocol; I8 theme attr confirmed + useSyncExternalStore; I9 Task 6 split 6a/6b/6c; I10 execution order; minors M1–M6.
- Plan 2 (content: 12 lessons, terms for Part 1, notebooks, roadmap TermMap of lessons, exercises/quizzes) is written after Spike A/B outcomes are known, because authoring syntax depends on them.
