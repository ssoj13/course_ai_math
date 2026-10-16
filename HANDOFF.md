# HANDOFF — math_en ("Math for AI" interactive textbook)

## Current Rust widget handoff — 2026-10-13

This section supersedes the historical 2026-10-11 execution status below for the Rust
widget task only. Current source and evidence: [widget instructions](sim-widget-rs/AGENTS.md),
[implementation ledger](sim-widget-rs/docs/implementation-status.md),
[full 18-task plan](plans/20261012-explainer-runtime/phase-04-implementation.md).

**STOP checkpoint:** the user ordered HANDOFF update, push everything and stop.
Implementation is stopped. Root owns the requested commit/push and its outcome; this
note does not assert that a push has succeeded. Resume only on the user's instruction.

The actual Rust/WASM egui + wgpu startup GUI was observed in Chrome at
[editor.html](http://127.0.0.1:43874/editor.html), against the copied 87-package closure.
Startup freeze: 57 paths, SHA256
`bd019a2a92393ed7db898d0628dc1418db02eb9aacbe75e1dc9fca8d6a0bc20b`.
Completed independent startup review is **FINDINGS**, SHA256
`7b3b506f9e16a375c6b1f2841ca3e6fcef7feb6c6ea74bb725630152b8974438`:
EDITOR-R1 clean-focus pending ownership/rejected false draft and EDITOR-R2
Ctrl+Shift+Z Redo failure are confirmed. Tested Undo and B-drag Cancel pass.
O1 clean locale draft formatting and O2 publication coherence remain open.

GUI FIX1 is incomplete and **not accepted or promoted**:

- Edited `sim-editor-ui/src/{panels/inspector.rs,transport.rs,hotkeys.rs,shell.rs,catalog.rs}`
  and `tools/render-trial/src/editor_web.rs`; added `tests/editor_fix1.rs`.
- Native check PASS 2.72 s, WASM check PASS 20.01 s, release PASS 22.32 s,
  wasm-bindgen exit0. Release emits two unused snapshot/points warnings.
- Latest new native suite **10 cases: 4 PASS / 6 FAIL**; old seven-case headless suite
  was not reached after failure. Dirty transfer/range retry/play-invalid/locale/ABA/Escape
  and real hotkey focus routing require completion; do not weaken tests.
  Temporary TRACE eprintln diagnostics remain in the new test file.
- Inspector writes clean-focus handoff and feedback-based dirty reconciliation;
  redo-first and clean-locale refresh source are written but unverified.
- O2 second paint was removed after held-pointer state rejection. Current source
  prepares GUI/scene/picker from one pre-intent FrameSnapshot and publishes canonical
  writes on the next demand frame; feedback drains before the next UI pass.
  gui_revision/scene_revision/canonical_revision coherence still needs full verification.
- `editor-fix1.html/js/pkg-editor-fix1` rebuilt; worker boot-only browser observation,
  **no functional FIX1 browser proof or independent correction review**.
  Original editor artifacts and manual user tab 1696999236 remain unchanged.
- WP-1 paused: copied gizmo `neutral.rs` 11191 B and public module/native check only;
  kernel/lifecycle integration not implemented.
- Startup WASM **10,271,979 B raw / 3,273,482 B gzip — SIZE FAIL**, no waiver.
  Full WP/Task03/04/all18, PICK-P1/performance/lifecycle/IME/a11y/locale remain open.
  Original Session/math remains authority; no private panel history/core/schema replacement.

Portable evidence is copied under
[checkpoint-2026-10-13](docs/acceptance/sim-widget-rs/checkpoint-2026-10-13/):
startup report/review and FIX1 logs only; giant source snapshots excluded.
Historical progress notes below precede this stopped checkpoint.

### Resume commands (planned; do not execute while stopped)

From repository root, restart the loopback preview if absent (existing server was PID101556,
12-hour lifetime; do not assume it survives):

```powershell
bun sim-widget-rs/tools/render-trial.ts serve --port 43874
```

From `sim-widget-rs`, first finish defects and run retained/new tests:

```powershell
cargo test --locked --offline -p sim-editor-ui
```

From repository root, rebuild the isolated FIX1 artifact after source corrections:

```powershell
cargo build --locked --offline --manifest-path sim-widget-rs/tools/render-trial/Cargo.toml --lib --target wasm32-unknown-unknown --release --target-dir sim-widget-rs/target
wasm-bindgen sim-widget-rs/target/wasm32-unknown-unknown/release/sim_render_trial.wasm --target web --out-dir sim-widget-rs/tools/render-trial/web/pkg-editor-fix1
```

Then use a fresh own browser tab at `http://127.0.0.1:43874/editor-fix1.html`,
reproduce original R1/R2 witnesses plus locale/publication/input cases, freeze exact
sources/artifacts and obtain different-model review before promotion. Preserve manual tabs.

- Latest user correction, 2026-10-13: «мы КОПИРУЕМ крейты из warpbro и других проектов».
  Vendor the selected required crates and own-project closure from Warpbro, egui-widgets
  (gizmo/widgets), other cglibs and StoryTold into `sim-widget-rs`; preserve the whole
  Warpbro UI and our wgpu viewport. The old SSH-only and UI-only exception rulings are
  superseded. Record frozen source commit/hashes/licenses/NOTICE/assets/local patches;
  local path dependencies replace own-project Git dependencies. **Do not vendor crates.io**:
  normal registry dependencies remain pinned by Cargo.lock.
- [Full Warpbro UI port plan](plans/20261012-explainer-runtime/phase-06-warpbro-ui-port.md)
  is APPROVED_FOR_IMPLEMENTATION (architecture only). WP-0 preflight/implementation underway,
  original `/root/sim_renderer_impl` sole product writer. Before product copy obtain complete
  activated transitive file/resources/license provenance. Preflight is now APPROVED_FOR_COPY_PREFLIGHT:
  `.superpowers/sdd/sim-widget-rs/warpbro-ui-copy-transitive-preflight-review.json`,
  SHA256 `4646a50ba0c1abbe3247b9ce61be75b3458ad964bc251fa568fe8bcc18daf9c0`.
  First raw product copy DONE: executor 1703 entries / 38453205 B, 1655 required matching
  files + 46 UI slices + 2 Cargo references; root saw 17 new vendor roots plus existing StoryTold.
  Manifest normalization/portable UI code underway; native/WASM/browser not yet compiled.
  WP-0 acceptance requires frozen post-patch/hash/local Cargo resolution and independent review.
  Frozen Warpbro853590699cd220532a15063442fa94e8e3c49794/egui96fc02ff58f30f3ea8a39193c39f39004a7835b7;
  plan SHAfb870f581883058edcb76b7367184096e290b5cc9808bfea3a57902e13bc572e,
  review SHAf1969d65e7cf6492e0f03598d16dbf195ec4d9a49a1dab87704e566a90d95acc.
  Donor working tree concurrent foreign edits are excluded from frozen copy inputs.
- CONSUMER-R2 pending-first-success correction is independently APPROVED, retaining R1
  recovery; review `.superpowers/sdd/sim-widget-rs/task-03-input-consumer-fix2-review.json`,
  39,041 B SHA256 `8a7c771788e8c6b13605ae8dadd2740132a444414ac0738b2f1a91c50c371eb3`.
  Initial invalid/no-op proposal remains pending; a valid held-pointer continuation works;
  admitted writes retain compensation even if rendering fails.
- Native scoped checks and actual Chrome singular→valid→Escape plus rebuilt MMB/wheel
  smoke pass. Native OS autoscroll was not reproduced by CDP. User says the wheel cover
  no longer appears; its root cause remains unconfirmed. Existing manual tab is untouched.
  `cosines-input-fix2.html` is staged, not a canonical promotion.
- Full Tasks 03/04 and all 18 tasks remain open; PICK-P1 and renderer WASM **385,747 B
  gzip > 300,000 B** remain open without waiver. Current own translation gizmo is not
  final shared professional gizmo integration.
- Preserve one Session/evaluator, InputCoordinator, common picker, ViewState and GpuHost.
  Full professional UI, EN/RU, 11 CSS px compact typography and no Holo remain required.
  Continue through the reviewed full port contract; no donor/product edits by docs owner,
  no implicit commit/push, and preserve all other agents' work.

## Historical course handoff (2026-10-11)

Updated 2026-10-11 ~20:15 by controller session math-en (Claude). User ordered: stop agents, commit, merge into main. Read this, then [AGENTS.md](AGENTS.md) (working
notebook: contracts, traps, TODO), [task.md](task.md) (the user's own words, Russian), the recover plan/ledger
`.superpowers/sdd/2026-10-11-recover/plan.md` (gitignored, on disk only, append-only log of everything below), and
[docs/superpowers/plans/2026-10-11-course-completion.md](docs/superpowers/plans/2026-10-11-course-completion.md)
(full scope + acceptance per lesson). Trust `git log` and files over memory.

## 0. Status in one screen

- Repo `C:\projects\projects.rust.cg\courses\course_ai_math`, GitHub `ssoj13/course_ai_math` (public). Branch `feat/engine`
  HEAD `8aef269`; `main` = `8aef269` (pushed). Pages deploys verified green up to 5dc9fef; 8aef269 pushed after.
  Live: https://ssoj13.github.io/course_ai_math/ (RU: /course_ai_math/ru/).
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

**State after the stop (20:20):** 2.1+2.2 MERGED into feat/engine/main (gates green: 873 tests, 59 pages) but with a
KNOWN DEFECT: horizontal page scroll at 360 px on both pages (overflow 210/236 px, EN+RU; cause not found) and its
screenshots were never looked at — fix first. 3.6+3.7: WIP `7c1d4b4` (3.6 code + core ExtentError/typed spec
reasons) MERGED into feat/engine/main at 20:25 after the controller fixed one type error in its test (gates green);
the code is not mounted on any page yet — still to do: 3.6 EN/RU pages + course.json line + RU stamp, all of 3.7,
terms.json entries, screenshot pass (resume note: worktree-agent-a8d573ce2786a80bd/.superpowers/.../batch-tensors-c-report.md). 2.3+2.4: not started
(branch has only a resume note commit 840e7c9 that force-added a gitignored report; do not merge it).

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
