# HANDOFF — math_en ("Math for AI" interactive textbook)

Updated 2026-10-11 (after the overnight run and the morning recover audits). Read this, then [AGENTS.md](AGENTS.md)
(working notebook), then [task.md](task.md) (the user's own words) and the recover plan
`.superpowers/sdd/2026-10-11-recover/plan.md`. Trust `git log` and the files over memory and over old ledgers.

## 0. Status in one screen

- Repo `C:\projects\projects.rust.cg\cgprojs\math_en`, GitHub `ssoj13/math-course` (public), branch `feat/engine`
  (HEAD `6d284b0` when this was written). Live site https://ssoj13.github.io/math-course/ is still the pre-night state:
  `origin/main` = `8868525`, `origin/feat/engine` = `43a823b`. Nothing pushed overnight.
- Gates on the working tree (process audit, ~09:00): `bun test tests/js` 262 pass / 0 fail / 47 files;
  `bun run check` 160 files 0/0/0; LC fix1 build 41 pages, 0 warnings. Physical renderer chunk ~270–273 kB gzip
  (limit 300,000).
- Course: **0 of 98 entries complete.** One lesson (0.5 law of cosines) has an interactive EN + RU slice that is not
  accepted yet; powers (0.2) prose is merged but its scenes are not implemented; RU lesson prose exists only for LC
  (other RU drafts are gitignored).
- Engine tasks 6h, 6g, 6f, 7 are accepted (reviews in `.superpowers/sdd/2026-10-10-engine-*`, summary in
  [docs/acceptance/](docs/acceptance/README.md)), but the holo look does not match the reference (visual audit).

## 1. User requirements (source: `task.md`)

- Interactive next-gen textbook "Math for AI" modeled on `C:\projects\projects.rust.cg\cgprojs\vllm_en`, roadmap.sh
  mind maps, poloclub/transformer-explainer. Law of cosines / dot product → vectors, matrices, tensors → derivatives,
  integrals → probability/statistics (mandatory "why squares everywhere") → learning machines → attention. Advanced
  track and branch Q (FP4, MX/NVFP4, NF4, GPTQ/AWQ, SmoothQuant, QAT, how to quantize networks).
- Reader = the user: CG/VFX developer who says he is slow at math → very gentle, step by step, every symbol in words,
  numbers after each formula, CG analogies. Mind maps everywhere ("X built on Y" chains down to the machine).
- **Bilingual like vllm_en**: English root, Russian under `/ru/`. Chat with the user in Russian.
- GitHub Pages; Pyodide + Colab; React + r3f + react-flow; Rust/wasm only by measurement.
- Visuals: "максимально наглядно, красиво, круто, современно", "как никто ещё не делал", professional DCC (Blender) UI.
  Scenes he asked for: matrix × vector as projection onto another basis in 3D (orthonormal / non-orthonormal flag);
  tensor "road" rank 1..N slider in 3D; derivative in 3D with orbit/zoom/guides/hover; partial derivatives 1D..6D with
  time axes; errors + backprop step by step; law of cosines and tensor multiplication illustrations.
- Every scene window: enlarge 2× and full screen.
- **Style: `docs/design/refs/holo-style-ref.png` ("2-futuristic") EXACTLY, nothing removed** (galaxies/starfield, glow
  lines, glass squares with grid, HUD instruments on the sides, rulers/ticks, mirror floor, formula plate); also likes
  `clay-style-ref.png` (secondary). Blender chrome stays. He got angry when the starfield was dropped: never remove,
  tone down or reinterpret reference elements; on a budget/readability conflict, ask him.
- Content additions: fractional/negative powers (lesson 0.2 `powers`), "square is an area" visualized (2D + 3D widget),
  systems of linear equations and determinants (Part 2: 2.7–2.11), eigen after matrices.
- Process: **use Fable** (model `fable`) for design, content and Russian translation ("советуйся с Fable"); use skills;
  orchestrate; finish the whole plan without cuts, systematically.
- Hard rules: never `git reset`; no reverting files via git without permission (take content from commits instead);
  no attribution lines in commits/docs; no swearing; English code/comments; comments explain why; readable code.

## 2. What happened overnight (git log 43a823b..HEAD)

- `0dca77d` powers lesson 0.2 prose + reviewed scene contracts merged (scenes not built).
- `cb0042d` whole-course completion map: [docs/superpowers/plans/2026-10-11-course-completion.md](docs/superpowers/plans/2026-10-11-course-completion.md).
- `f295950` → `16fbcec` → `90c522a` Task 6h holo/clay viewport; `b6504c2` docs.
- `2020a93`, `b71eb1b`, `df00473` Task 6g bilingual site and engine UI strings; `6e96b88` docs.
- `c07e004` → `ce5f0ab` Task 6f wide / full screen; `cbfd5e1` docs.
- `0fb29d6` → `afa82e4` → `8b2f0c3` Task 7 NumTable / Timeline / CodeView / Formula marks; `8ce78f6` docs.
- `b448484` law of cosines interactive slice (EN + RU); `ca25dba` LC review fix round 1.
- `6d284b0` CI runner pinned to `ubuntu-24.04`.

## 3. What the recover audits found (`.superpowers/sdd/2026-10-11-recover/`)

- `visual-audit.md` (+ screenshots in `visual-audit/`): **does not look like the reference.** Sparse starfield,
  nebula banding and a dark "eye" ring, no spiral galaxies in the sky, glass without inner grid, no floor/reflection in
  lesson cards, tiny non-scaling HUD stickers, loose framing (figure ~35 % of the stage), labels far from objects,
  clay cards showing space HUD, truncated N-panel labels and scene names (RU worst), 360 px layout where instruments +
  plate are taller than the stage.
- `lc-audit.md`: math, IK and Python outputs correct. Important: E-1 literal Markdown asterisks in timeline text,
  E-2 compressed implementation caveats in prose, R-1 English inside TeX on the RU page, R-2 RU timeline abridged,
  Q-1 inline `words(locale, en, ru)` strings bypass the i18n parity guards. ~14 minors (status hints, step visuals,
  angle format, `SemanticLinks`, DOM reach-ins).
- `process-audit.md`: git hygiene clean. Problems: holo "exactly" was downgraded to "interpretation" without asking;
  Fable was replaced by gpt-6-* models and reviewer = implementer model family; reviews did not rerun gates; a
  controller-invented 300 kB ownership gate drove a three.js GLSL regex rewrite (`src/lib/compact-glsl.mjs`) and
  public source maps (`sourcemap: 'hidden'`, 7.5 MB in dist), then was superseded; 6f exit "fix" hides the card for
  two frames; golfed code and per-frame JSON data bus (`InstrumentSync.tsx`); unreadable ledgers; AGENTS bloated.

## 4. In flight right now

- **HOLO-V2** (main checkout, brief `.superpowers/sdd/2026-10-11-recover/holo-v2-brief.md`, report
  `holo-v2-report.md`, evidence `holo-v2-evidence/`): make scenes match the reference side by side (sky galaxies,
  framing, real glass, floor + reflection in lesson cards, HUD scale/placement, clay without space elements, 360 px
  layout, label widths), InstrumentSync via store subscription, drop compact-glsl/public source maps unless measured
  need, sizeModes exit fix, readable code. Owns engine files under `src/viz/scene/**`, palette, lab.css.
- **LC fix round 2** (worktree `.claude/worktrees/agent-aba1686960501b2a8`, branch
  `worktree-agent-aba1686960501b2a8`, brief `lc-fix2-brief.md`, report `lc-fix2-report.md`): E-1, E-2, R-1, R-2, Q-1,
  minors, readable `src/viz/lessons/law-of-cosines*`. Owns lesson files, `src/content/docs/**`, `src/i18n/**`.
- Docs cleanup (this file, AGENTS.md, `docs/acceptance/`): done 2026-10-11.

## 5. Next steps (from plan.md)

1. Accept HOLO-V2 and LC fix2: reviewer = a different model that runs the gates and browser checks itself, plus a
   visual verifier comparing screenshots with `holo-style-ref.png`. Update AGENTS.md "Holo and clay look" from the
   HOLO-V2 report. Merge the LC worktree into `feat/engine`.
2. Ship: merge `feat/engine` to `main`, deploy Pages, check the live site.
3. Lessons in course-completion order: 1.4 dot product scenes DP-1..4
   (`.superpowers/sdd/2026-10-11-course-completion/p2-dot-product-brief.md`), then 0.G / 0.2 scenes (TileGrid, FnCurve,
   "square is an area"), rest of Part 0/1, then Part 2+ (content + scenes + RU + Python/Colab), each:
   implementer → reviewer (different model) → visual verifier. Fable for design, content and translation.
4. Services: Tasks 8 (Pyodide), 9 (term graph/TermMap), 10 (notebooks + ColabBadge), 11 (size report); quiz-lint port.
5. Cleanup: lesson logic out of `links.ts`/`radialLink.ts`, browser smoke tests in CI, leftover worktrees (ask first),
   ledgers rewritten as one event per line.

Open user decisions: covers (regenerate to exact geometry or accept drafts in scratchpad `covers/`); any conflict
between the holo reference and budget/readability.

## 6. Plans, specs, tooling

- Plans: `docs/superpowers/plans/2026-10-10-engine-and-spikes.md` (Tasks 0–11),
  `2026-10-10-engine-addendum-6d-6e.md` (6d, 6e, 6h, 6g, 6f), `2026-10-11-course-completion.md` (98 entries, P0–P15).
- Specs: `docs/design/visual-language.md` (v3 holo + clay, cover templates), `lessons-law-of-cosines-dot-product.md`
  (LC-0..3, DP-1..4, §5 engine contracts), `part0-1-storyboard.md`, `signature-illustrations.md`, `backlog-*.md`.
- SDD folders (gitignored): `.superpowers/sdd/2026-10-10-engine-and-spikes/` (old ledger `progress.md`, reviewer
  rubric, `content-brief.md`), `2026-10-10-engine-addendum-6d-6e/` (6h/6g/6f reports, reviews, evidence),
  `2026-10-11-course-completion/` (P2 briefs, reviews, `progress.md`), `2026-10-11-recover/` (audits, briefs, reports).
- Covers: model `gpt-image-2.5-sunburst`, key in User env `OPENAI_API_KEY`
  (`[Environment]::GetEnvironmentVariable('OPENAI_API_KEY','User')`); xAI `GROK_API_KEY` (worse geometry).
- Scratchpad worktrees: `scratchpad/preview` (detached, preview server), `scratchpad/mainwt` (`main`).

## 7. Traps (including overnight lessons)

- Don't relabel or supersede budgets and user rules. The only budget is Ruling O (physical renderer chunk ≤ 300 kB
  gzip, others ≤ 100 kB); a gate the controller invents is not a user rule, and dropping it when it fails is a smell.
- Don't reinterpret "exactly": the user chose a reference; a "procedural interpretation" needs his decision first.
- Reviewer must be a different model from the implementer and must run gates and browser checks itself; a 3-minute
  read of a 5,000-line package is not a review.
- Fable for design, content and translation is a user requirement; don't silently swap models.
- Don't hide a problem from the metric (visibility-hidden frames for CLS); fix the layout or document why not.
- Keep ledgers human-readable: one event per line `YYYY-MM-DD HH:MM | task | event | sha | evidence`, normal spacing,
  no heartbeat lines; rulings in a separate file. Keep AGENTS.md a notebook, history in `docs/acceptance/`.
- Keep durable evidence next to the evidence folder, not only MCP tmp log ids.
- Lesson strings go into i18n dictionaries under the parity tests; no English words inside TeX.
- Agent worktrees branch from `main`: `git merge --ff-only feat/engine` first; copy reports into `.superpowers/`
  if the guard blocks the path.
- With concurrent agents in one checkout: `git commit -- <paths>` only. Never reset/stash/checkout-revert.
- Don't reformat JSON with a serializer; edit lines textually. Agents hitting 429: resume the same agent.
- The omh hook blocks combined `git remote add` + push chains; run push alone.
