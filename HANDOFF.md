# HANDOFF — math_en ("Math for AI" interactive textbook)

Updated 2026-10-11 ~20:15 by controller session math-en (Claude). User ordered: stop agents, commit, merge into main. Read this, then [AGENTS.md](AGENTS.md) (working
notebook: contracts, traps, TODO), [task.md](task.md) (the user's own words, Russian), the recover plan/ledger
`.superpowers/sdd/2026-10-11-recover/plan.md` (gitignored, on disk only, append-only log of everything below), and
[docs/superpowers/plans/2026-10-11-course-completion.md](docs/superpowers/plans/2026-10-11-course-completion.md)
(full scope + acceptance per lesson). Trust `git log` and files over memory.

## 0. Status in one screen

- Repo `C:\projects\projects.rust.cg\cgprojs\math_en`, GitHub `ssoj13/math-course` (public). Branch `feat/engine`
  HEAD `8aef269`; `main` = `8aef269` (pushed). Pages deploys verified green up to 5dc9fef; 8aef269 pushed after.
  Live: https://ssoj13.github.io/math-course/ (RU: /math-course/ru/).
- Gates on `8aef269`: `bun test tests/js` 844 pass / 0 fail; `bun run check` 0/0/0; `bun run build` 55 pages, 0 warnings; renderer chunk ~268 kB gzip (limit 300,000); largest other 87 kB (limit 100,000);
  lazy elk 432 kB (limit 600,000).
- Publish: worktree `scratchpad/mainwt` (= main): `git merge --ff-only feat/engine && git push origin main`, then
  `gh run watch` the "Deploy to GitHub Pages" run (test+build+deploy jobs). Local preview: worktree `scratchpad/preview`
  (detach at a commit, `bun run build`, `bun run preview -- --port 4480` detached). Other sessions' Chrome cleanup can
  kill our preview/Chrome ("scratchpad" in the command line) — restart when it dies with exit 255.

### Lessons with interactive scenes, EN + RU, merged on feat/engine
Start here (why-this-course, how-it-connects — with DotUnit INTRO scenes), 0.G squares-and-plane (+3D tile card),
0.2 powers, 0.3 pythagoras, 0.4 unit-circle, 0.5 law-of-cosines, 1.1 vector, 1.2 vector-arithmetic, 1.3 length,
1.4 dot-product (+DotUnit teaser), 1.5 projection, 1.6 basis (orthonormal checkbox scene), 1.7 cosine-similarity +
embeddings in AI (real GloVe/arctic data, RAG, analogies, 1-head attention preview), 1.8 cross-product,
1.9 waves-dft (NEW: DFT as dot products + epicycles), **3.2 data-as-tensors, 3.3 tensor-memory, 3.4 broadcasting,
3.5 reductions** (merged, not independently reviewed), **3.1 tensor-road** (rank slider 0→6 in 3D, real data per
rank, memory strip + strides, transpose, DotUnit "axis that disappears"; einsum playground opens on bij,bjk->bik).
All Part 0/1 lessons passed the consolidated batch review + one fix round (`batch-review.md`, `fixround-report.md`).
3.1 not independently reviewed yet. Numbering: "Start here" unnumbered; titles/sidebar show CURRICULUM ids (0.2, 1.5,
Part 0/1/3); tensors has explicit `"part": 3` in course.json (matrices will be part 2).

### Services / engine
- Engine (6d Blender look, store/links, drag, N-panel, Timeline, Formula, NumTable/NumGrid, size modes ×2/full,
  bilingual i18n) — accepted. Holo/clay look WITHDRAWN by the user (scenes = 6d Blender grey; space only on covers).
- Pyodide cells (`PyCell`) in all lessons — accepted. Term graph + maps (`terms.json`, `LessonMap`, glossary
  genealogy) — merged, fix round done, final check in batch review.
- Tensor core `src/core` (shape/strides/views/broadcast/reductions/matmul, einsum plan + explain, fp32/pairwise order,
  typed errors, NumPy fixtures) — merged, not yet reviewed.
- DotUnit procedural system (`scene = f(einsum spec, values)`, two faces: projection sticks → column through row →
  tile stack equal to projection; presets dot/matvec(row+column)/matmul cube/batched/attention/layer; zoom ladder with
  dive-in) — merged; polish pass running (§3).

## 1. User requirements (source: task.md — read it)

Gentle, CG-anchored explanations for a self-described slow-at-math CG/VFX developer; mind maps everywhere ("X built
on Y" down to machine level); bilingual EN root + RU /ru/; GitHub Pages; Pyodide + Colab; React + r3f + react-flow;
Rust/wasm only by measurement; professional DCC (Blender) look; every scene window: enlarge 2× and full screen;
look = Task 6d Blender grey (user decision 2026-10-11 after trying holo: "вчерашний вид целиком"; no space/holo/glass/
floor/HUD/clay in scenes). Content asks: powers (fractional/negative), "square is an area" 2D+3D, systems of equations
+ determinants (Part 2), eigen after matrices, DFT/FFT as dot products + epicycles (1.9, A4), embeddings in AI (1.7),
a separate "Start here" section with a 3D animation that connects dot product → matrix → tensor and explains WHY to
learn this, the procedural DotUnit system (parameters editable). **Top priority now: the tensor line with the DotUnit
animation.**

## 2. Operating rules (learned the hard way)

- **Machine is shared** (operator order): max 3 agents at once; each agent one shell at a time; no polling loops;
  preview/Chrome only for final screenshot passes, killed right after. Do not spawn shells in bulk.
- Never `git reset`/checkout-reverts/stash; commit by explicit path; no attribution lines; chat with the user in Russian.
- Never remove/tone down a user-chosen element; ask on conflict. Reviewer = a different model than the implementer and
  must run gates/screenshots itself. Keep ledgers human-readable.
- Agent worktrees (`isolation: worktree`) start from `main`: tell agents to `git merge --no-edit feat/engine` first;
  their reports land inside their worktree (`.claude/worktrees/agent-*/.superpowers/...`) — copy if needed.
- Merge pattern (controller, main checkout): `git merge --no-edit <branch>`; recurring conflicts: `src/i18n/ui.ts`
  (keep all lesson namespaces; ensure commas), `src/i18n/untranslated.json` (valid JSON; list only pages with no RU
  file), MDX import lines (keep PyCell + LessonMap + lesson imports), RU `translatedFrom` stamps (`bun run i18n:stamp
  <ru file>`). Every lesson page ends with "## Term map" + `<LessonMap />` in EN and RU. Run gates one at a time.
- Usage limits hit often: agents die with 429; resume the SAME agent via SendMessage (its edits stay in its worktree).

## 3. In flight right now (agents; resume via SendMessage if they died)

| Work | Branch (worktree under .claude/worktrees/) | Report (inside its worktree) | Port |
|---|---|---|---|
| Lessons 2.1 linear-transformations + 2.2 matrix-columns (NEW module matrices, part 2, between vectors and tensors; incl. the 3D basis-projection scene with orthonormal checkbox) | `worktree-agent-a0ddbb0cc8e811414` | `batch-matrices-a-report.md` | 4527 |
| Lessons 3.6 einsum + 3.7 layer (DotUnit presets, S3 light box, S4 plugs-and-cables) | `worktree-agent-a8d573ce2786a80bd` | `batch-tensors-c-report.md` | 4528 |
| Lessons 2.3 transform-gallery + 2.4 matrix-multiplication | `worktree-agent-a8aae68300b1dfec5` | `batch-matrices-b-report.md` | 4529 |

All three were told at ~20:15 to STOP: kill processes, WIP-commit, write a resume note. Resume each via SendMessage to
the same agent (or a new agent pointed at the branch + its report). Merge a branch into feat/engine/main only if its
gates are green; otherwise push the branch and finish it first. Known small TODOs: link 3.4 "No copy: stride 0" to
`lesson:tensor-memory` (EN+RU); course.json ordering of matrices lessons (2.1, 2.2, then 2.3, 2.4).

Done today and merged (for reference): engine fix (Stroke `writeLine` in fatLine.ts, shared `src/viz/fmt.ts`), DotUnit
polish (Arrow opacity, fp32/pairwise, memory strip, LOD visuals, challenges), batch fix round (framing `<Room>`, label
placement, RU «вы»), engine-small (unnumbered Start, DotUnit `preset`, field contrast, `tests/js/frames.ts`).

## 4. Next steps, in order

1. Collect the three agents above; merge one at a time (`git merge --no-edit <branch>`; resolve ui.ts / course.json /
   terms.json / untranslated.json by keeping both sides; restamp RU; gates one at a time); publish.
2. Then 3.6 einsum/contraction + 3.7 layer y = xWᵀ + b (DotUnit presets), 2.3 gallery + 2.4 matmul (4 views incl. the
   cube) + 2.5 transpose, 2.6–2.8 (determinant, computing det, **systems of equations**), 2.9–2.11 (inverse, rank,
   eigen). ≤3 agents.
3. Independent review batch for 3.1–3.x and 2.x (one Fable reviewer, one preview, sequential screenshots), one fix
   round, publish.
4. Then Parts 4–10, advanced A1–A10 (A4 = complex DFT/FFT), branch Q1–Q12 per the completion map; notebooks/Colab
   (Task 10), size report + quiz-lint (Task 11, `scripts/size-report.ts` does not exist yet), tensor core review,
   covers.

## 5. Known open issues

- Scenes small at 360 px in several 3D cards; some label crowding (lesson reports list them).
- Font/KaTeX load-time layout shift (baseline C7/C9) still open.
- Lesson 2.9 / A4 referenced without links until those pages exist.
- Physical devices / other browsers / touch never tested (headless Chromium only).
