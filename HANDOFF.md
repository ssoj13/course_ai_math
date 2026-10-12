# HANDOFF — math_en ("Math for AI" interactive textbook)

Updated 2026-10-11 ~18:00 by controller session math-en (Claude). Read this, then [AGENTS.md](AGENTS.md) (working
notebook: contracts, traps, TODO), [task.md](task.md) (the user's own words, Russian), the recover plan/ledger
`.superpowers/sdd/2026-10-11-recover/plan.md` (gitignored, on disk only, append-only log of everything below), and
[docs/superpowers/plans/2026-10-11-course-completion.md](docs/superpowers/plans/2026-10-11-course-completion.md)
(full scope + acceptance per lesson). Trust `git log` and files over memory.

## 0. Status in one screen

- Repo `C:\projects\projects.rust.cg\cgprojs\math_en`, GitHub `ssoj13/math-course` (public). Branch `feat/engine`
  HEAD `2cd33b9` (pushed). Live site https://ssoj13.github.io/math-course/ = `main` = `72f7de8` (older: 3 lessons + 0.G +
  Pyodide). Everything since is pushed to `feat/engine` but NOT published yet (user chose: build a batch, review once,
  then publish).
- Gates on `2cd33b9`: `bun test tests/js` 717 pass / 0 fail; `bun run check` 0/0/0; `bun run build` 45 pages, 0 warnings;
  renderer chunk ~272 kB gzip (limit 300,000).
- Local preview: worktree `scratchpad/preview` detached at `2cd33b9`, served on http://localhost:4480/math-course/
  (restart: `bun run preview -- --port 4480` in that worktree, detached). `scratchpad/mainwt` = `main` worktree used to
  publish (`git merge --ff-only feat/engine && git push origin main`, then watch the Pages run with `gh run`).

### Lessons with interactive scenes, EN + RU, merged on feat/engine
Start here (why-this-course, how-it-connects — with DotUnit INTRO scenes), 0.G squares-and-plane (+3D tile card),
0.2 powers, 0.3 pythagoras, 0.4 unit-circle, 0.5 law-of-cosines, 1.1 vector, 1.2 vector-arithmetic, 1.3 length,
1.4 dot-product (+DotUnit teaser), 1.5 projection, 1.6 basis (orthonormal checkbox scene), 1.7 cosine-similarity +
embeddings in AI (real GloVe/arctic data, RAG, analogies, 1-head attention preview), 1.8 cross-product,
1.9 waves-dft (NEW: DFT as dot products + epicycles). Accepted by independent review: LC, DP, 0.G only; the rest is
under the consolidated batch review (see §3).

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

| Work | Worktree / branch | Report | State |
|---|---|---|---|
| Consolidated batch review of all lessons + live-fix wave + term maps (Fable) | own scratch `scratchpad/batch-review-wt` | `.superpowers/sdd/2026-10-11-recover/batch-review.md`, shots `batch-review-shots/` | ~80 % done (up to 1.8; 1.9 next) |
| DotUnit polish: cube readability, Arrow opacity, fp32/pairwise readout, memory strip, LOD-2/3 visuals, code-line follow, challenges, browser checks | `worktree-agent-afd8236f6bd0608dc` | its worktree `.superpowers/sdd/2026-10-11-recover/dotunit-report.md` | running |
| Engine fix: `Stroke` keeps first-draw segment count (growing lines truncated); SSR vs browser trig formatting → React #418; remove 1.9 workaround; audit growing-line scenes | `worktree-agent-a964c4141e1a85cb3` | its worktree `engine-fix-report.md` | running |

## 4. Next steps, in order

1. Collect the three results above; merge DotUnit polish + engine fix (gates one at a time).
2. One fix round for the batch-review must-fix list (≤3 agents, by lesson groups), quick re-check, then **publish**
   (`main` ff + push, watch Pages, check live URLs EN/RU).
3. Small engine task: "Start here" module unnumbered so Ground stays Part 0 (manifest field); add 1.7 terms
   (embedding table, nearest neighbour, semantic search/RAG, softmax) to `terms.json`; review the tensor core.
4. **Priority line:** Part 3 tensor lessons on the core + DotUnit (3.1 tensor road rank 0→N with real data and
   memory strides, 3.2–3.5 views/broadcast/reductions, 3.6 einsum/contraction, 3.7 layer), then Part 2 matrices
   (2.1–2.11 incl. determinants, systems of equations, inverse, eigen), using the DotUnit presets.
5. Then Parts 4–10, advanced A1–A10, branch Q1–Q12 per the completion map; notebooks/Colab (Task 10), size report +
   quiz-lint (Task 11), covers.

## 5. Known open issues

- Scenes small at 360 px in several 3D cards; some label crowding (lesson reports list them).
- Font/KaTeX load-time layout shift (baseline C7/C9) still open.
- Lesson 2.9 / A4 referenced without links until those pages exist.
- Physical devices / other browsers / touch never tested (headless Chromium only).
