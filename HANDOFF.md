# HANDOFF — math_en ("Math for AI" interactive textbook)

Written 2026-10-11 by the controller session (math-en). Read this, then `AGENTS.md` (project notebook), then the SDD ledger
`.superpowers/sdd/2026-10-10-engine-and-spikes/progress.md` (gitignored, on disk only). Trust the ledger + `git log` over memory.

## 0. Status in one screen

- **Session state: PAUSED by the operator** (relayed by session warpbro-rs-7f: only WarpBro runs; others idle until the
  operator resumes). No agents or builds running. Everything committed and pushed.
- Repo: `C:\projects\projects.rust.cg\cgprojs\math_en`, GitHub `ssoj13/math-course` (public). Working branch `feat/engine`.
- **Published** 2026-10-11: `main` fast-forwarded to `feat/engine` @ `8868525`, Pages deploy run 38120512145 green
  (test/build/deploy). Live: https://ssoj13.github.io/math-course/ (12 lessons text-only; scene slots are MDX comments;
  engine demo at /math-course/dev/engine/, hidden).
- Gates on `8868525`: `bun test tests/js` 178 pass; `bun run check` 0/0/0; `bun run build` 18 pages, 0 warnings; viz-core
  ~272.7 kB gz (budget 300, Ruling O).
- Local preview: worktree `scratchpad/preview` (detached) served on port 4480 by a long-lived `astro preview` (PID was 66012);
  rebuild there with `git checkout --detach <sha> && bun run build` — the running server serves the new dist.
  Main worktree for `main`: `scratchpad/mainwt`. Scratchpad = `C:\Users\joss1\AppData\Local\Temp\claude\C--projects-projects-rust-cg-cgprojs-math-en\d352e7d6-fa75-40b8-bffa-feea04c2dac6\scratchpad`.

## 1. User requirements (source of truth: `task.md`, user's own words, keep appending in his style)

- Interactive next-gen textbook "Math for AI" modelled on `C:\projects\projects.rust.cg\cgprojs\vllm_en`, roadmap.sh mind maps,
  poloclub/transformer-explainer. From law of cosines / dot product → vectors, matrices, tensors → derivatives, integrals →
  probability/statistics (mandatory "why squares everywhere") → learning machines → attention. Advanced track + branch Q
  (quantization: FP4, MX/NVFP4, NF4, GPTQ/AWQ, SmoothQuant, QAT, how to quantize networks).
- Reader = the user: CG/VFX dev, says he is slow at math → very gentle, step-by-step, every symbol in words, numbers
  after each formula, CG analogies. Mind maps everywhere ("X built on Y" chains down to machine level).
- **Bilingual like vllm_en**: English root, Russian under `/ru/` (Task 6g). Chat with user: Russian.
- GitHub Pages; Pyodide + Colab; React + r3f + react-flow; Rust/wasm only by measurement.
- Visuals: "максимально наглядно, красиво, круто, современно", "как никто ещё не делал", professional DCC (Blender) UI.
  Specific scenes wanted: matrix×vector as projection onto another (orthonormal / non-orthonormal flag) basis in 3D; tensor
  "road" rank 1..N slider in 3D; derivative in 3D with orbit/zoom/guides/hover; partial derivatives 1D..6D with time axes;
  errors + backprop step by step; law of cosines and tensor multiplication illustrations.
- Every scene window: enlarge 2× and full screen (Task 6f).
- **Style (user choice, AskUserQuestion): scenes + covers in `docs/design/refs/holo-style-ref.png` ("2-futuristic")
  EXACTLY — nothing removed** (galaxies/starfield, HUD instruments, glow, glass squares with grid, rulers/ticks, mirror floor,
  formula plate); also likes `clay-style-ref.png` (secondary). Blender chrome stays. The user got angry when the controller
  dropped the starfield: never remove/tone down elements of a chosen reference; if budget/readability conflicts — ask.
- Content additions: fractional/negative powers (new lesson 0.2 `powers`), "square is an area" visualized (2D + 3D widget),
  systems of linear equations + determinants in general (Part 2: 2.7 computing det, 2.8 systems, 2.9 inverse, 2.10 rank,
  2.11 eigen), eigen after matrices.
- Process: "советуйся с Fable" (use model `fable` for design/content), skills, orchestrate; push yourself; publish Pages on request.
- Hard rules (global): never `git reset`; no reverting files via git without explicit permission (take from commits instead);
  no "Co-Authored-By Anthropic" / attribution lines in commits/docs; no swearing; English code/comments; comments explain why.

## 2. Architecture facts (details in AGENTS.md)

- Astro 7.3.8 + Starlight 0.42.6, React 19.3, three 0.186.1, r3f 9.8.1, drei 10.7.9, zustand 5, KaTeX 0.19 (build-time via
  `src/lib/satteri-math.mjs`), bun **1.4.3 required** (1.3.12 crashes). TypeScript 6 + `@astrojs/check` gate.
- One shared `<Canvas flat frameloop="demand">` (`src/viz/scene/SceneHost.tsx`, created on demand) + drei `<View>` per Stage.
- Lazy-entry pattern: MDX imports `src/viz/lessons/<slug>.tsx` (SSR-safe store init + `lazyIsland`), three/drei only in
  `<slug>.impl.tsx`. Tests must not import three/drei (THREE_CJS_DEPRECATED).
- Store `src/viz/store.ts`: `lessonStore(id, init)`, `set(key, v, how)`, `writeMove(lesson, set, how, ease)`; drag + keyboard
  share ONE path (`move(vals, raw, src) -> {set, ease}`), clamp to N-panel row ranges; magnet-caught vectors clamp by uniform
  scale (direction kept).
- Chrome (6d): ViewportCard header (2D/3D, Persp/Ortho, Overlays, Reset view), custom DOM gizmo (drei GizmoHelper renders
  into the wrong View on a shared canvas), NPanel (reuses scrub engine), StatusBar hints `{mouse, touch}`, plain wheel scrolls
  page / Ctrl+wheel zooms (Ruling T), chips wrap < 720 px with auto-derived line reservation.
- 6e primitives: custom move mapping, magnet + AngleArc ticks, HalfPlane, LitSphere (`max(0,N·L)`, CDP-verified ≤0.53/255),
  Ledger, Challenge (`challenge:<lesson>:<n>`, `step['<timeline>:max']`), Circle, N-panel bool rows (no Toggle component).
- Notation (course-wide): vectors bold `\mathbf{a}`, lengths `|\mathbf{a}|`, plain letters = numbers.
- Quiz: `src/components/Quiz.astro` (4 options, plain-text options). Positions balanced by hand; **TODO port quiz-lint**
  (length rank + position balance) from vllm_en — `tests/js/quiz-lint.test.ts` does not exist.

## 3. Plans and specs

- `CURRICULUM.md` (program; updated: 0.2 powers, Part 2 2.6–2.11). `src/data/course.json` (sidebar; `powers` line is only
  on the unmerged powers branch).
- Plan 1: `docs/superpowers/plans/2026-10-10-engine-and-spikes.md` (Tasks 0–11). Addendum:
  `docs/superpowers/plans/2026-10-10-engine-addendum-6d-6e.md` (6d, 6e, **6h, 6g, 6f**, illustration roadmap).
  **Execution order: 6e ✅ → 6h → 6g → 6f → 7 → lessons law-of-cosines + dot-product with scenes → merge/deploy → 8 → 9 → 10 → 11.**
- Specs: `docs/design/visual-language.md` (**v3 holo + clay**; cover prompt templates + geometry checklist),
  `docs/design/lessons-law-of-cosines-dot-product.md` (scenes LC-0..3, DP-1..4; §5 engine contracts),
  `docs/design/part0-1-storyboard.md` (§7 scenes for 0.G/0.2 on the powers branch), `signature-illustrations.md`, backlogs
  `docs/design/backlog-*.md` (basis projection, tensor road, derivatives D1–D6, loss/backprop B1–B3).
- SDD scripts: `C:/Users/joss1/.claude/plugins/cache/claude-plugins-official/superpowers/6.4.1/skills/subagent-driven-development/scripts/{task-brief,review-package}`;
  briefs/review packages for addendum tasks land in `.superpowers/sdd/2026-10-10-engine-addendum-6d-6e/`; reports, reviews,
  rubric (`reviewer-instructions.md`, `re-reviewer-instructions.md`, `constraints.md`, `content-brief.md`) in
  `.superpowers/sdd/2026-10-10-engine-and-spikes/`.

## 4. Open work, exact resume steps

1. **Powers lesson (branch `worktree-agent-a2f45b37cd3458629`, worktree `.claude/worktrees/agent-a2f45b37cd3458629`)**:
   commits d443271, b625e40, 1ba3533, merge bd5d8c5, ad6ca92, 36205db. Partial re-review `content-powers-rereview.md`:
   Needs fixes — (I) x/A/V coupling: N-panel/Formula/Num write single keys, `writeMove` only from handles → spec a store
   hook (or engine addition in 7.13) + store key for the `k` row; (I) TileGrid thin-axis threshold 0.12 misclassifies x=0.1.
   Not yet checked: repo-wide gamma grep, storyboard minors (M11…), gates. → resume writer agent a2f45b37cd3458629 (Fable)
   or a new Fable writer with that file; then scoped re-review; then `git merge --no-ff` into feat/engine.
2. **Task 6h (holo viewport)**: brief `.superpowers/sdd/2026-10-10-engine-addendum-6d-6e/task-6h-brief.md`; resume note at
   top of `task-6h-report.md` (nothing implemented yet). Dispatch opus implementer, BASE = feat/engine HEAD. Binding: reference
   AS IS; holo/clay layer in its own lazy chunk; restore `chunkSizeWarningLimit` (temporarily 1030 in astro.config.mjs) —
   fix the cause, don't raise limits; stop and ask on budget conflicts. Then reviewer (opus) + verifier with screenshots
   side-by-side vs reference.
3. **6g** bilingual (port vllm_en i18n: course-rules.mjs, course.ts loc/href, src/i18n ui.ts/core.ts + UiStrings, i18n tests;
   all engine UI strings). Then Russian translation pass per reviewed lesson (Fable).
4. **6f** enlarge 2× / full screen (Fullscreen API on documentElement, not the card — shared canvas lives in body).
5. **Task 7** (NumTable/Timeline/CodeView + Formula `marks`; Timeline writes `<id>:max`; fix Formula width layout shift).
6. Wire scenes into lessons 0.5 and 1.4 per spec, then 0.G/0.2 scenes (TileGrid, FnCurve), merge to main, deploy.
7. Tasks 8 (Pyodide), 9 (term graph/TermMap), 10 (notebooks + ColabBadge), 11 (size report; pin CI ubuntu image — ubuntu-latest
   moves to 26 on 2026-10-19). Final whole-branch review on the most capable model with deferred minors from the ledger.
8. Covers: templates in visual-language.md §Covers (model `gpt-image-2.5-sunburst`, key in **User** env `OPENAI_API_KEY`, read via
   `[Environment]::GetEnvironmentVariable('OPENAI_API_KEY','User')`; xAI key `GROK_API_KEY`, models grok-imagine-image*, worse
   geometry). Drafts: scratchpad `covers/ground-v2-2.png` (best; bottom "square" is a rectangle), `covers/vectors.png`
   (perpendicular not perpendicular to a). Waiting for user: regenerate to perfect geometry or accept. Always run the
   checklist; 2–4 tries per cover. Image models can't hold exact geometry → lesson diagrams only via the engine.
9. TODOs: port quiz-lint; `/dev/` pages in sitemap decision; Formula focus restore; DragLayer tests; storyboard §2/§6 still
   say 0.G merged 0.1+0.2.

## 5. Traps learned

- Agent worktrees (`isolation: worktree`) branch from **main**, not feat/engine → tell agents to `git merge --ff-only feat/engine`
  first; their reports can't be written to the shared `.superpowers` path (guard) → copy them over.
- When committing from the controller while an implementer works in the main checkout: always `git commit -- <paths>`.
- Don't reformat JSON with json.dump (CRLF/format churn) — insert lines textually.
- Agents may hit API rate limits (429) mid-task: resume the SAME agent via SendMessage; partial edits stay in the tree.
- Spec bugs found by agents were real (IK elbow 112°, float outputs, DP-2 perpendicular side `ccw`) — verify and fix the spec.
- omh hook blocks combined `git remote add` + push chains; run push alone.
