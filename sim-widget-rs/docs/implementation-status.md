# sim-widget-rs — implementation ledger

Updated: 2026-10-13. State: historical tasks 01/02, core and scoped picker/input accepted; CONSUMER-R1/R2 bounded corrections and Chrome MMB/wheel scoped pass; whole Warpbro UI port planning/review underway, PICK-P1/budget/full acceptance open.
[README](../README.md) · [Work packages and task manifest](../../plans/20261012-explainer-runtime/phase-04-implementation.md)
· [Acceptance matrix](../../plans/20261012-explainer-runtime/phase-05-acceptance.md).

## Bootstrap and evidence

The coordinator created `sim-widget-rs` with `cargo new --lib` in `math_en` on `feat/engine`.
At this ledger's creation, `Cargo.toml` and the generated `add`/test scaffold existed.
The worker reports that task 01 replaced the scaffold with model/math/runtime and the facade;
foundation scope was independently accepted after correction round 1/5 below.

Coordinator preflight verified installed Rust/Cargo 1.99.0, wasm32 target, working SSH access,
cglibs origins/revisions and clean StoryTold donor baselines. Registry search reported
wgpu 30.0.1 and egui 0.36.2. Dependency and compatibility gates require manifests/lock/build evidence.
Worker-reported task 01 checks are recorded below. The old high-level PBR constructor probe failed as
recorded below; at preflight, successful GPU rendering was unverified. Later partial draw evidence
is recorded in the current Task 03/04 section; performance and full acceptance remain open.

Partial B evidence reported by the coordinator: `.superpowers/sdd/sim-widget-rs/renderer-probe`
resolves `render-engine-pbr` from the checked render-rs SSH rev and wgpu 30.0.1 with webgpu
and fragile WASM flags. `cargo check --locked --target wasm32-unknown-unknown` for that probe
exited 0 (initial 1m11s, cached 0.36s). This proves compile-time dependency resolution only;
GPU runtime, pixels, frame time, full-system memory and payload size remain unverified. The coordinator's native
`cargo check --locked` for the same actual render-rs/opensubdiv closure also exited 0 (48.09s).
The transitive opensubdiv branch/main resolved to lock commit prefix `59657511`; the complete
source/feature graph still requires audit. Probe dependency wasm-bindgen is 0.2.129;
matching wasm-bindgen CLI 0.2.129 was installed and its command version verified after
detecting installed CLI 0.2.127. The release probe exports an actual async Renderer3D constructor
and `hovered_id`; generated `pkg/sim_render_portability_probe_bg.wasm` under the probe directory
is 163,265 B raw and 55,810 B gzip (level 9, Bun 1.4.3), measured before the diagnostic panic hook.
This is a lower bound: full rendering, math, author UI and assets are not linked. It does not
close the 300,000 B renderer budget or any full-system performance gate.
The coordinator also reported 10,109 B gzip for the probe JS. These constructor-only artifacts
do not establish the complete payload, actual GPU behavior or final budget acceptance.

### B — browser portability probes

The coordinator's [browser constructor probe](../../.superpowers/sdd/sim-widget-rs/browser-constructor-probe.json)
returned `FAILED` on Chrome 154.0.0.0 / Windows at `2026-10-12T23:58:52.588Z`.
The localhost page was a secure context, WebGPU was available and its adapter was ready.
The diagnostic release WASM build and wasm-bindgen step both exited 0;
`console_error_panic_hook` 0.1.7 exposed the Rust runtime error.

`Renderer3D` construction panicked at `wgpu-30.0.1/src/backend/webgpu.rs:414:35`:
`FeaturesWGPU(POLYGON_MODE_LINE)` is not enabled for the WebGPU backend. The async panic
left the constructor Promise pending and the page at its construction stage.
A read-only source agent corroborated unconditional `PolygonMode::Line` at
`render-engine-pbr/src/pipelines.rs:470` in the pinned render-rs source.
The donor source and dependency cache were not modified.

Before GPU acceptance, review the portable wireframe implementation and explicit capability
contract, then repeat constructor and actual rendering/lifecycle scenarios. Preserve real
wireframe behavior and the scoped visual contract below. No pixels, picking, resize, loss/dispose,
frame time or memory were validated by this failed constructor trial. The earlier size lower
bound predates the diagnostic hook and remains outside full payload/budget acceptance.
Task 03 stays in progress while portability and integration preparation continue.

The [task 03 risk plan](../../.superpowers/sdd/sim-widget-rs/task-03-risk-plan.json) is ready
(39,632 B). Its read-only reconnaissance defines multiview GpuHost ownership, pass/lifecycle
contracts, portable picking, capability matrices and measurements; it is preparation evidence.
The final [architecture gate](../../.superpowers/sdd/sim-widget-rs/task-03-architecture-gate.json)
approved R03-ARCH-B1: a widget-owned shared GpuHost/pass graph and raster wrapper composed with
unchanged public `standard_surface_bsdf::wgsl_source()` at the checked SSH render-rs pin.
The wrapper uses at most four bind groups: frame, object, material and lighting resources.
Real Chrome `maxBindGroups=4` rejected the fixed-five-group standard-surface pipeline before
device creation. That clean capability rejection proves no draw or GPU acceptance.
No fixed-five-group pipeline, donor shader rewriting, second BSDF, private Renderer3D fork
or cache patch belongs to the selected architecture. Public material validation and an explicit
GPU packing ABI must preserve the BSDF contract; all per-stage resource limits still apply.
The failed constructor gate concerns the pinned high-level PBR path. GPU/platform feasibility
and successful rendering require the selected route's own evidence. Architecture is accepted;
actual draw, true wireframe/picking, lifecycle and budget gates are open.

The coordinator's [public BSDF browser probe](../../.superpowers/sdd/sim-widget-rs/browser-bsdf-probe.json)
returned `PASS_PIPELINE_CREATION_ONLY` on Chrome 154 / Windows with `maxBindGroups=4`.
Unchanged public BSDF WGSL plus owned entrypoints and four real uniform groups created
TriangleList and LineList pipelines; `ErrorScopeGuard.pop().await` reported no validation errors
and the console error list was empty. The operation took 216.6 ms; scratch release build took
23.89 s and wasm-bindgen exited 0. The emission entrypoint exercises a real public BSDF function.
This is a pipeline-creation result: no pixels, full material/light parity, stroke width/AA,
cameras, multiview, loss/dispose, complete payload or performance were demonstrated.
LineList creation does not establish the required pixel-width ribbon wireframe behavior.

The updated probe records 199,122 B raw / 65,614 B gzip for WASM and 45,375 B raw /
9,285 B gzip for JS (level 9, Bun 1.4.3). These are lower bounds for constructor-only
pipelines and the public WGSL emission entrypoint. Full renderer, per-lesson payload,
frame-time and memory acceptance remain open.

## States and closure rules

Use `pending`, `in progress`, `implemented awaiting review`, `accepted` or `blocked`.
An accepted task needs its implementation snapshot, outputs, applicable acceptance IDs,
implementer model, independent reviewer model, reviewer commands/results and closed findings.
The reviewer model must differ from the implementer model. Unrun gates remain explicitly open.

Each task below inherits its complete A–N checkbox scope and output/dependency contract from
the linked manifest. A wave closes its own scope only; the full release closes after task 18
and the user trial. Migration of existing lessons is a subsequent plan.

## Execution tasks

| ID | Task / A–N mapping | Depends on | Implementation | Independent review / evidence |
| --- | --- | --- | --- | --- |
| 01 | Foundation / A, C base | Preflight | accepted: foundation scope; fix round 1/5 closed | APPROVED: gpt-6-astra high; T01-R1 addressed, 0 open |
| 02 | Runtime authority, schema/history/session / C, F base | 01 | accepted runtime scope: sim_runtime_impl, gpt-6.1-sol high; fix round 2/5 closed | APPROVED: gpt-6-astra high; R1–R5 closed, 0 findings |
| 03 | Portability, scene and renderer / B, D | 01/02; accepted core prerequisite | original sim_renderer_impl gpt-6.1-sol high sole writer; scoped picker fix2 accepted | APPROVED gpt-6-astra high: R6 closed; PICK-P1/browser/full renderer open |
| 04 | Input, controls and UI quality / E | 02/03 | scoped core/InputCoordinator accepted; sole renderer consumer fix1 | Chrome MMB/wheel scoped PASS; consumer FINDINGS CONSUMER-R1 open; full Task 04 pending |
| 05 | Full time, branches and simulation / F | 02/03 | pending | pending |
| 06 | Math and linked data views / G | 02/03/05 | pending | pending |
| 07 | Full visual author editor / H | 04/05/06; 12 contract | pending | pending |
| 08 | Resolver and resource lifecycle / I base | 02/03 | pending | pending |
| 09 | GLB/glTF materials/animation/skinning/morph / I | 08 | pending | pending |
| 10 | Alembic samples/changing topology / I | 05/08 | pending | pending |
| 11 | USD layers/variants/references/payloads/matrix / I | 08/09 | pending | pending |
| 12 | Portable project, migrations/storage/recovery / J | 02/08 | pending | pending |
| 13 | PNG/SVG/frames/video/audio/captions / J | 03/05/12 | pending | pending |
| 14 | Learner/author web artifacts, native/embed / K, B verdict | 04/07/12/13 | pending | pending |
| 15 | Isolated EN/RU full cosines trial / L | 06/07/14 | pending | pending |
| 16 | Ten canonical scene roundtrips / M | 06/09/10/11/14 | pending | pending |
| 17 | All 98 course entries, extensions/matrices / M | 15/16 | pending | pending |
| 18 | Full independent release acceptance / N | All 01–17 | pending | pending |

Pure tasks 01/02 can proceed while B renderer/shell investigations run. AUTHOR-11 waits
for project/shell completion; a contract draft alone cannot close the save/open/embed workflow.

## Decision register

| ID | Accepted decision | Evidence / remaining gate |
| --- | --- | --- |
| D1 | Compact workspace + root facade; apps depend facade | Architect preflight; metadata/dependency graph review pending |
| D2 | Tracks→overrides→constraints→single Rust truth/provenance→presentation | Architect preflight; effective-input conflict/seek tests pending |
| D3 | Checked TimeTicks and decimal wire strings; donor Tick behind adapter | Donor behavior audit; overflow/nonfinite/rational/wire tests pending |
| D4 | Document/session/view owners distinct | Architect preflight; revision/Undo/event/view isolation tests pending |
| D5 | Learner and author separate WASM artifacts | Architect preflight; artifact/network/size checks pending |
| D6 | Pure foundation starts after A without waiting for GPU B | Architect preflight; full release still requires all B–N gates |
| D7 | Latest compatible versions with concrete evidence | Coordinator Rust/registry preflight; integrated compatibility pending |
| D8 | cglibs SSH rev + locked transitive graph; StoryTold local closure | Source audit; manifests/lock/provenance/closure checks pending |
| D9 | New isolated directory on feat/engine; explicit briefs/ledger; no implicit commits | Coordinator ruling; preserve shared-checkout ownership |

R1 shell DOM/egui/accessibility, R2 WASM byte ownership, target device/capability matrix,
worker headers, USD/export matrices, persistence conflict policy and measured visual quality
remain open until measured evidence closes the corresponding risk in the implementation plan.
Visual direction for the new widget/pilot is resolved by R03-VISUAL below; its measured
readability/motion/UI acceptance remains open.

### R03-VISUAL — user contract, 2026-10-12

Accepted user authority: «holo не надо», «нужна добротная и понятная, плавная визуализация»,
«с хорошим приятным UI». Scope: new sim-widget-rs widget and the copied cosines pilot.
This overrides the stale root AGENTS Holo requirement for that scope.

Use a quiet professional Blender-like scene and pleasant UI: readable semantic figures,
stable values/labels, smooth continuous presentation and direct drag/scrub response; clear
typography, spacing, focus, inspector and timeline. Decorative Holo/HUD styling is excluded.
All 18 tasks retain their full professional features. Existing lessons and covers have a
separate migration scope. The user direction is accepted; rendered quality still requires review.

#### R03-TYPE — user typography steering, 2026-10-12

After manually opening the static reference, the user said: «выглядит хорошо, шрифт меньше
сделай. там что-то типа 14 сейчас, надо 11». The main desktop UI font and ordinary scene
labels are **11 CSS px**. Scene labels keep that CSS pixel size independently of geometric
zoom. Preserve comfortable hit areas and a restrained heading hierarchy. This explicit choice
supersedes the earlier 14 px desktop minimum for the new widget and copied pilot;
existing lessons remain outside this typography scope.

The [durable static visual contract](../design/ui-visual-contract.json) and
[reference HTML](../design/reference.html) are
preparation artifacts. The designer's frozen fix1 source used 11 px; the independent UI
source reviewer uses this revised contract. Density iteration 2's source handoff is recorded below.
The user's positive response is reference feedback. Rendered reviewer acceptance, runtime UI
completeness and test passes are not established by it. Browser reference review remains
`NOT_RUN` because of the CUA file-protocol policy block; no workaround or rendered pass is claimed.

#### R03-DENSITY — user spacing steering, 2026-10-12

The user described the reference as near the desired result and asked: «вертикальные промежутки
в UI можно уменьшить, сделать всё компактнее?»; «огромные гапы по вертикали везде».
Reduce control heights, vertical padding and group gaps in embedded, learner and author shells.
Retain desktop 11 CSS px. Pointer controls are typically 24–28 CSS px high; explicit
touch/coarse-pointer mode keeps hit areas at least 44 CSS px. The policy preserves all
content/features and reachability, including narrow-layout library and add-object routes.

Before density iteration 2, the
[frozen fix1 source re-review](../../.superpowers/sdd/sim-widget-rs/ui-design-fix1-review.json)
returned `NEEDS_REVISION` from `gpt-6-astra high`, distinct from designer `gpt-6.1-sol high`.
It applies to the historical fix1 snapshot. D1 was partial:
source paint order predicts viewport chrome occlusion, and fixed label backings cross triangle
sides at transitional widths (721 px was reproduced analytically). D5 was partial:
library/resources/compositions and add-object content disappeared at ≤980 px without an equivalent
disclosure route. Density iteration 2 subsequently addressed these source findings. D2/D3/D4 and
11 px typography were resolved in source only; rendered accessibility/interaction remains unverified.

The user's screenshot is reference feedback. It does not establish independent rendered review,
runtime UI completeness, performance or test passes. Browser/rendered reference review remains
`NOT_RUN`; no UI acceptance record is published.

#### Density iteration 2 — durable source handoff

The [density source re-review](../../.superpowers/sdd/sim-widget-rs/ui-design-density-review.json)
returned `PASS_WITH_LIMITATIONS` for the frozen static reference and contract only.
Independent `gpt-6-astra high` closed F1–F3 in source and checked 1,067 source geometry cases;
designer `gpt-6.1-sol high` supplied the corrected source. No runtime controls, animation,
browser rendering or full UI acceptance was established.

The designer copied byte-identical artifacts into `sim-widget-rs/design/reference.html`
and `design/ui-visual-contract.json`. Documentation-owner SHA256 checks match the reviewed
hashes. The [durable source handoff](../../docs/acceptance/sim-widget-rs/ui-reference-source.md)
preserves both exact file hashes, models, chronology, dimensions and limits. Compact fine-pointer
specification: topbar 36 px, header 28 px, rows 24–26 px, numeric envelope 24 px, groups/padding
8 px; ordinary desktop UI/scene labels 11 CSS px, pointer commands 24–28 px, coarse/touch
hit areas ≥44 px. All content and library/add routes remain reachable in the source contract.

Browser/rendered desktop/mobile/screenshots/interaction/performance/runtime math are `NOT_RUN`
in this design review. User positive screenshot/reference feedback remains separate from
independent rendered evidence. Full UI acceptance remains open. Task 02's scoped runtime
acceptance below is a separate decision; copying the reference establishes no renderer/UI gate.

## Full release gates

- [ ] All feature groups and detailed checkboxes A–M implemented and independently accepted.
- [ ] All Phase 05 CORE/TIME/INPUT/VIEW/AUTHOR/ASSET/SAVE/EXPORT/AUDIO gates passed.
- [ ] Browser/native lifecycle, language/accessibility and separate artifact loading proven.
- [ ] Locked Rust gates and applicable integrated Bun 1.4.3 gates passed by reviewers.
- [ ] Reproducible cold-load/memory/frame-time/input-delay and current size budgets accepted.
- [ ] Licenses, StoryTold provenance/closure, SSH rev/lock graph and extension API verified.
- [ ] All ten canonical scenes and mapping of every one of the 98 course entries complete.
- [ ] Full-version trial on the isolated cosines copy accepted by the user.
- [ ] Task 18 accepted; full-system completion recorded with links to evidence.

## Review records

Task 01 foundation was accepted by independent scoped re-review after its requested correction.
Full-system acceptance remains open.
Add each task's immutable snapshot or reviewed diff, both model IDs, reviewer-owned gate outputs,
findings/fixes and acceptance report link here.
Acceptance reports follow root policy in `docs/acceptance/`; do not replace reports with checkboxes.

### Task 01 — foundation accepted after correction round 1/5

[Durable foundation acceptance](../../docs/acceptance/sim-widget-rs/task-01-foundation.md)
preserves the verdict, commands/results, correction and accepted snapshot independently of
the gitignored scratch JSON/logs.

Implementer: `gpt-6.1-sol`, reasoning `high`, recorded by the coordinator and the review package.
The worker report's inherited-model placeholder is not the authoritative model identification.
Independent reviewer: `gpt-6-astra`, reasoning `high`, agent `/root/sim_foundation_review`.
The initial review returned `CHANGES_REQUESTED`; the original implementer completed
correction round 1/5. Independent corrected-snapshot review returned `APPROVED` for foundation.
One finding was addressed, zero remain open; no commits were made.

- [Worker report](../../.superpowers/sdd/sim-widget-rs/task-01-report.json):
  `DONE_WITH_CONCERNS`, ready for independent review, not full-system acceptance.
- [Initial frozen review package](../../.superpowers/sdd/sim-widget-rs/task-01-review-package.json):
  48 file hashes and four review diffs; working-tree snapshot, no commit requested.
- [Independent review](../../.superpowers/sdd/sim-widget-rs/task-01-review.json):
  one confirmed P2, T01-R1; no full-system signoff.
- [Fix round 1 re-review](../../.superpowers/sdd/sim-widget-rs/task-01-fix1-review.json):
  `APPROVED`, T01-R1 `ADDRESSED`, no new findings.
- [Exact reviewed fix patch](../../.superpowers/sdd/sim-widget-rs/task-01-fix1-review.patch):
  constant-segment handling and two added regression tests.

The initial worker report recorded exit 0 for locked native/WASM workspace builds, WASM check,
workspace tests, format check and clippy with `-D warnings`. Its 18 integration tests passed: 8 foundation
and 10 boundary scenarios. The executed native cosines example reports c²=13/25/37 at
60°/90°/120°, using nine actual operations. These are implementer results; the reviewer-owned
checks of the initial snapshot are recorded separately below.

| Independent command | Working directory | Result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | `sim-widget-rs` | exit 0 |
| `cargo test --workspace --locked` | `sim-widget-rs` | exit 0; 18 integration tests passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | `sim-widget-rs` | exit 0 |
| `cargo check --workspace --locked --target wasm32-unknown-unknown` | `sim-widget-rs` | exit 0; compilation only |
| `cargo run --locked --example cosines` | `sim-widget-rs` | exit 0; c²=13/25/37, nine actual operations per frame |
| `cargo test --offline -- --nocapture` | `.superpowers/sdd/sim-widget-rs/review-scratch` | exit 101; two constant-track regressions failed, one large-time test passed |

Reviewer provenance verification compared the initial 48 snapshot files and all 26 copied/source
records with SHA256: zero mismatches; donor commit/status, licenses, NOTICE and documented
manifest adaptations matched. The review JSON contains per-command log paths and reproduction.

Initial T01-R1: a constant EasyEase segment 1→1 returned 0.9999999999999999 at 1% progress.
A valid parameter range [1,1] or Equal constraint then rejected seek with a Range/constraint diagnostic.
The requested correction preserves equal endpoints exactly before donor interpolation and adds
constant-sampling/constrained-seek regressions; final validation remains strict, with no epsilon
clamping of arbitrary truth values. Assigned correction scope: `track.rs`, `boundaries.rs` and
the implementer report. The initial passing suite did not close this reproduced finding.

The accepted correction returns exactly equal authored endpoints before donor interpolation;
nonconstant segments keep their original path. Final ranges/constraints remain strict.
Regressions cover exact constant bits, forward/back constrained seeks and Track provenance,
unchanged author history, and atomic rejection of a genuinely out-of-range override.

| Independent re-review command | Working directory | Result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | `sim-widget-rs` | exit 0 |
| `cargo test --workspace --locked` | `sim-widget-rs` | exit 0; 20 integration tests passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | `sim-widget-rs` | exit 0 |
| `cargo check --workspace --locked --target wasm32-unknown-unknown` | `sim-widget-rs` | exit 0; compilation only |
| `cargo test --offline --locked -- --nocapture` | `.superpowers/sdd/sim-widget-rs/review-scratch` | exit 0; all 3 original reviewer tests passed |

The reviewer verified all 48 corrected snapshot hashes with zero mismatches and found no new
issues in the exact fix diff. T01-R1 is addressed. Re-review scope covers foundation and the
correction; browser execution, GPU/native UI, payload budgets and full A–N gates remain open.

Implemented scope reported: typed bounded document validation, checked IDs/time/value/unit ports,
scalar/vector operators and provenance, effective-input constraints, atomic revisioned commands,
transaction cancel/Undo and bounded session history, checked donor scalar tracks, presentation
separate from truth, local EffectCraft time/geom/color/keyframe closure and provenance.
craft-ui is deferred until its author consumer exists.

Foundation acceptance excludes browser WASM execution and GPU acceptance; donor upstream tests are outside the
root test set; host wire decoding, full schema/migrations/permissions, tensor/autodiff, full timeline,
renderer/editor/assets/project/export and remaining course coverage belong to later tasks.
Task 01 acceptance closes foundation scope. Every full release gate remains open.

### Task 02 — runtime accepted after correction round 2/5

[Durable Task 02 runtime acceptance](../../docs/acceptance/sim-widget-rs/task-02-runtime.md)
records the final scoped verdict, exact snapshot, models, commands, correction history and limits.
R1–R4 closed at fix1; R5-F1 and parent R5 closed at fix2. No findings remain open in Task 02.
The original report/review chronology below is retained; full release remains open.

The coordinator dispatched `/root/sim_runtime_impl` with `gpt-6.1-sol`, reasoning `high`,
from the [concrete task 02 brief](../../.superpowers/sdd/sim-widget-rs/task-02-brief.json).
The [source baseline](../../.superpowers/sdd/sim-widget-rs/task-02-baseline/) contains 21 files.
The [worker report](../../.superpowers/sdd/sim-widget-rs/task-02-report.json) returned
`DONE_WITH_CONCERNS` and froze source at `2026-10-13 00:31:59 UTC`.
The [review package](../../.superpowers/sdd/sim-widget-rs/task-02-review-package.json) records
36 owned source/config/fixture files and 27 unchanged donor/provenance files, with no build caches.
The coordinator retained the [36-file after snapshot](../../.superpowers/sdd/sim-widget-rs/task-02-after/),
[full patch](../../.superpowers/sdd/sim-widget-rs/task-02-review-full.patch) and six ordered
diff shards listed in that package. The shards must be concatenated in order.

Independent reviewer `/root/sim_runtime_review` was dispatched with fresh `gpt-6-astra`,
reasoning `high`, distinct from implementer `gpt-6.1-sol high`. The
[independent review](../../.superpowers/sdd/sim-widget-rs/task-02-review.json) returned
`CHANGES_REQUESTED`: one P1 and four P2 findings below. The coordinator dispatched all five
findings together to the original implementer for correction round 1/5. The reviewed snapshot
and original report are retained. At that boundary, Task 03 source release/integration waited
for clean re-review. Final fix2 acceptance below released the scoped runtime source.
The renderer brief and B1 architecture/probe preparation do not imply renderer acceptance.

Reported implementation adds schema v2 with explicit v1 migration, eight parameter kinds and
checked metadata/display units, typed property bindings, immutable authority and trusted roles,
bounded atomic document edits/history, incremental truth/provenance/property frames, stateless
snapshot/event-suffix replay and independent ViewState. Original 20 foundation regressions remain
unchanged. Worker-reported RED→GREEN covers v1 migration, rejection of active-session counter
rewind, and retained-capacity storage budget enforcement; other new tests did not each observe RED.

All nine final worker gates below exited 0 on the frozen snapshot. These are implementer
results; independent review is required before accepting Task 02.

| Worker command | Directory | Recorded result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | `sim-widget-rs` | exit 0 |
| `cargo test --workspace --locked` | `sim-widget-rs` | exit 0; 49 integration tests |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | `sim-widget-rs` | exit 0; no warnings |
| `cargo build --workspace --locked` | `sim-widget-rs` | exit 0; native build |
| `cargo check --workspace --locked --target wasm32-unknown-unknown` | `sim-widget-rs` | exit 0; compilation only |
| `cargo clippy --workspace --all-targets --locked --target wasm32-unknown-unknown -- -D warnings` | `sim-widget-rs` | exit 0; no warnings |
| `cargo build --workspace --locked --target wasm32-unknown-unknown` | `sim-widget-rs` | exit 0; WASM build only |
| `bun sim-widget-rs/tools/runtime-parity.ts --check` | repository root | exit 0; six real TS fp64 sequential dot fixtures |
| `cargo run --locked --example cosines` | `sim-widget-rs` | exit 0; c²=13/25/37, nine actual operations per frame |

The 49 tests comprise inherited 20 plus schema 6, commands 6, frames 5, session 9 and parity 3.
The report retains command log paths. Registry versions were unchanged; Cargo.lock added three
local dependency edges: sim-math→serde, sim-runtime→serde and sim-runtime→serde_json.
Final GitNexus incremental reindex reported `ok` for 26 source files; communities/processes/routes
still require full analysis. The worker reports zero mismatches for the 27 donor/provenance files.

The worker describes conservative storage accounting, including retained capacities and shared
Arc ownership. The initial independent review found gaps in current checkpoint, newest history
entry and registry/cache accounting; see the original T02-R2/R3/R5 findings below.
Fix1 closed R2/R3 and the registry admission case; fix2 closed the R5 transition lifetime below.
Allocator/RSS, latency and GPU budgets were not measured.
The parity evidence covers six fp64 sequential vector-dot cases only. Matrix/tensor/autodiff,
fp32/pairwise/backend parity, extended time/simulation/springs, browser command boundaries,
renderer/editor/assets/project/export/audio and host disposal/callback cancellation remain
downstream gates. Session replay here is stateless and requires a new pristine runtime;
author journal persistence and in-place rollback are outside this package.
These initial worker results did not authorize Task 02 acceptance or source release.
The final independent closure is recorded below. No source commits were made.

#### Independent review — CHANGES_REQUESTED

Reviewer `gpt-6-astra high` ran all nine standard commands listed above independently:
each exited 0, including 49 integration tests, native/WASM fmt/clippy/build/check,
the actual TS parity checker and cosines. Logs are `gate-1.log` through `gate-9.log` in
[task 02 review scratch](../../.superpowers/sdd/sim-widget-rs/task-02-review-scratch/);
the review JSON records each command/directory/result.

The reviewer also ran `cargo test --offline --locked -- --nocapture` in that scratch directory.
It exited 101: six expected-contract regressions failed across five findings; the control
`ordinary_snapshot_roundtrip_still_works` passed. The two cumulative checkpoint cases share
T02-R2. Exact reproductions live in
[regressions.rs](../../.superpowers/sdd/sim-widget-rs/task-02-review-scratch/regressions.rs), with
[stdout](../../.superpowers/sdd/sim-widget-rs/task-02-review-scratch/regressions-final.stdout.log)
and [stderr](../../.superpowers/sdd/sim-widget-rs/task-02-review-scratch/regressions-final.stderr.log).

| Finding | Severity / regression | Confirmed reproduction and required correction |
| --- | --- | --- |
| T02-R1 | P1; `valid_saved_author_state_can_reconstruct_pristine_runtime` | Start with a=3 and NonZero(a), override a=5, then author-set default a=0. Both edits and export succeed; `new_with_epoch` evaluates t=0 with empty overrides and fails Range at constraints/0 before restore. Construct/import document and saved effective state atomically before first evaluation, preserving trusted policy, paused playback and fresh generation; also cover valid saved nonzero time. |
| T02-R2 | P2; `cumulative_overrides_stay_exportable_or_reject_atomically`, `default_budget_exported_snapshot_is_accepted_by_restore` | Four 98-character scalar IDs under max_command_bytes=512 accept individual overrides but retain a 1,024 B checkpoint that cannot export. Under default limits, four len-one vectors with capacity 40,000 accept commands and export a 1,280,640 B checkpoint; identical-limit restore rejects it against 1,048,576 B. Bound cumulative current checkpoint and rolling baseline, retained and serialized storage, consistently across commands/export/import; reject the crossing command atomically. |
| T02-R3 | P2; `oversized_single_history_entry_rejects_instead_of_losing_undo` | Cosines with history_capacity=128 and max_history_bytes=10,000 accepts author a=5, then reports history_len=0: its sole newest retained before/after Undo group is evicted. Reject a newest single entry that cannot fit before truncating redo/appending; evict only older entries once the new group fits. |
| T02-R4 | P2; `transaction_cancel_reports_restored_object_metadata` | Preview triangle label change, then cancel. Committed label returns but changed_objects/removed_objects and other dirties are empty (structural=true), because comparison uses the committed graph rather than the previous published draft. Diff the previous publication through preview/update/commit/cancel, including object creation/deletion and metadata restoration. |
| T02-R5 | P2; `compiled_registry_is_charged_to_candidate_budget` | Empty document plus trusted frozen registry of 200 descriptors × 4,096 description characters constructs under max_candidate_bytes=4,096 despite 819,200 description bytes and owned registry/graph copies. Include checked registry/compiled cache/candidate retained storage before cloning, with explicit shared ownership policy. This is configuration budget correctness, not remote registry injection. |

At correction round 1/5 dispatch, all five findings remained open. The assigned re-review
required the original seven scratch cases, new regressions and applicable standard gates;
fix1 results and remaining source-release condition are recorded below.
The reviewer matched all 36 product hashes before/after checks and all 36 after-snapshot hashes;
the 27 donor/provenance files matched the Task 01 references. It read all six shards in order;
their full patch agrees after trailing-terminator normalization only. No product files or commits
were changed by the reviewer. This initial review did not publish a Task 02 acceptance record.

#### Fix round 1/5 — worker results

The worker's [report fix_rounds[0]](../../.superpowers/sdd/sim-widget-rs/task-02-report.json)
returned `DONE_WITH_CONCERNS` and froze the correction at `2026-10-13 01:03:37 UTC`.
The [scoped fix1 review package](../../.superpowers/sdd/sim-widget-rs/task-02-fix1-review-package.json)
retains the [corrected after snapshot](../../.superpowers/sdd/sim-widget-rs/task-02-fix1-after/),
[full fix patch](../../.superpowers/sdd/sim-widget-rs/task-02-fix1-review-full.patch) and two
ordered diff shards. Scope is T02-R1–R5 and defects introduced by their corrections.

The worker reports 60 integration tests (49 retained + 11 fix regressions) and all nine scoped
standard commands above exiting 0. WASM gates are compilation/clippy/build only. Its unchanged
original reviewer harness still had one R1 failure using strict `new_with_epoch` before restore;
the other six cases passed. The correction introduces paired `Session::from_checkpoint` and
`Session::from_events` initialization for documents valid only at saved time/overrides.
Scoped independent re-review must exercise that load contract while retaining a strict legacy
constructor control; the worker result is not independent closure of R1 or the other findings.

The scoped independent re-review below supersedes the pending-verdict state for this snapshot.

#### Fix1 independent review — R1–R4 closed, R5-F1 open

The [fix1 independent review](../../.superpowers/sdd/sim-widget-rs/task-02-fix1-review.json)
returned `CHANGES_REQUESTED`. Reviewer `gpt-6-astra high` is the same independent reviewer
thread, distinct from original implementer `gpt-6.1-sol high`. Scope was T02-R1–R5 and the
consequences of their corrections; no unrelated broad review or downstream signoff occurred.

All nine reviewer-run standard gates exited 0, including 60 integration tests. Logs are
`fix1-gate-1.log` through `fix1-gate-9.log` in the existing review scratch directory.
`cargo test --offline --locked -- --nocapture` also exited 0: all seven original independent
regressions passed. Only the R1 bootstrap calls were adapted to `from_checkpoint`; a strict
legacy `new_with_epoch` expected-Range control was retained, with saved-state equality,
fresh generation, paused playback and repeated-restore rejection checks.

| Finding | Independent fix1 disposition |
| --- | --- |
| T02-R1 | CLOSED: paired saved-state construction validates the real saved time/overrides before first publication, with trusted policy and fresh generation |
| T02-R2 | CLOSED: cumulative checkpoint guards reject crossing commands atomically; export/restore agree, including spare-capacity normalization and eviction |
| T02-R3 | CLOSED: oversized newest Undo group rejects before redo truncation; accepted newest group remains undoable, with explicit capacity-zero support |
| T02-R4 | CLOSED: exact object dirties compare against the prior published frame through previews, cancellation and commit |
| T02-R5 | PARTIALLY_FIXED: original registry admission case rejects correctly; T02-R5-F1 transition lifetime remains open |

T02-R5-F1 (P2) was independently reproduced by
`cargo run --offline --locked --bin fix1-peak` in
[review scratch](../../.superpowers/sdd/sim-widget-rs/task-02-review-scratch/), exit 101.
The [probe source](../../.superpowers/sdd/sim-widget-rs/task-02-review-scratch/fix1-peak.rs),
[stdout](../../.superpowers/sdd/sim-widget-rs/task-02-review-scratch/fix1-peak.stdout.log) and
[stderr](../../.superpowers/sdd/sim-widget-rs/task-02-review-scratch/fix1-peak.stderr.log) are retained.
Fixture: 25 valid SceneObjects with distinct IDs and 4,000-byte labels, no math/bindings/assets;
history/event retention both zero, candidate ceiling **150,000 B**. Replacing one label with
another 4,000-byte string was accepted. During that dispatch alone, the peak simultaneously
live newly allocated 4,000-byte label buffers was **396,000 B**: a lower bound already 2.64×
the ceiling. This is live allocation minus deallocation, not cumulative allocation volume.

The meter starts after fixture, Session, replacement object and Command preparation and stops
before printing/assertion. It excludes existing buffers, every other allocation size and allocator
overhead; subtracting preactivation-buffer frees can only lower the bound. Evidence is native
debug only, with no RSS/release/WASM runtime measurement claim. Product source was untouched.
The remaining accounting gap at fix1 was the union of old Session graph/frame, prospective candidate
contracts and temporary author/history/audit document copies during transition, even with
history/events disabled. Admission must bound those known simultaneous lifetimes before
expensive copies, or prevent the copies from coexisting; increasing the test ceiling or redefining
the contract as final-state-only does not close this finding.

The coordinator dispatched the original implementer for correction round **2/5**, scoped to
T02-R5-F1 and directly introduced regressions. Ordinary edit and transaction commit must be
checked with retention disabled and enabled, preserving atomicity and R1–R4 guarantees.
Task 02 acceptance and Task 03 source release/integration were held until the clean scoped
fix2 re-review below. All 18 tasks and every full release gate retain their existing scope.

#### Fix2 independent review — APPROVED, scoped Task 02 accepted

The [final fix2 review](../../.superpowers/sdd/sim-widget-rs/task-02-fix2-review.json) returned
`APPROVED` at `2026-10-13 01:31:40 UTC`. Independent `gpt-6-astra high` closed T02-R5-F1
and parent T02-R5, preserved historical R1–R4 closures and reported no findings.
Scope was R5-F1 and directly introduced breakage; the coordinator accepted aggregate Task 02
runtime scope from this review plus its prior closures. Implementer remained `gpt-6.1-sol high`.

All nine reviewer standard gates passed with 63 integration tests; seven prior independent
regressions passed unchanged in round 2. The unchanged original allocator probe now rejects
at the same 150,000 B ceiling with new label-buffer peak 0 B, rather than accepting at 396,000 B.
Eight independent allocation edit/commit controls and four enabled-history Undo/Redo transitions
passed. Source lifetime review checked old/new/temporary admission before candidate creation,
including target history evaluator reserves. Measurement is native debug label buffers only,
not a total-allocation/RSS, release, browser or WASM execution performance claim.

Accepted freeze: `2026-10-13 01:26:30 UTC`. The final report's `fix_rounds[1]` retains 36 owned
files: only runtime `lib.rs` and `runtime_commands.rs` changed versus fix1; 34 files and 27
donor/provenance files remained unchanged. The report SHA256 is
`349bdb870b18399051abae6c5e73c2b052609bc86400b6018f503e58448f7fa3`.
Documentation readback matched all 36 byte/hash records in fix2-after and Task 03 baseline,
confirmed the two-file diff and all 27 donor/provenance hashes. Exact identities/commands/limits
are retained in the durable record. Admission remains conservative; no configured ceiling increased.
No commit was made. Task 02 acceptance releases its scoped runtime source for Task 03 only;
all full release gates remain open.

#### R02-10 — reset semantics

Coordinator ruling, 2026-10-12; normal reset/guide paths passed initial independent checks;
Task 02 runtime acceptance is complete; full UI/playback integration retains later gates.

| Operation | Required state change |
| --- | --- |
| Reset values | Write the current authored parameter defaults as live overrides and pause playback. Check reader permissions and final constraints/ranges atomically before publishing. |
| Return to guide | Remove the selected live overrides and sample authored tracks at the current logical time; recompute truth/provenance through final validation. |
| Author set | Edit document defaults through author revision/transaction/history. Defaults are current authored values; no separate immutable factory baseline is introduced. |

A rejected reset preserves document, session, history and playback. Reset values does not
substitute a track sample for the authored default; Return to guide restores that time's guide.

#### R02-11 — restore/replay and replacement identity

Coordinator ruling, 2026-10-12; fix1 closed R1 and fix2 closed R5-F1. Task 02 is accepted within scope.

Restore/replay runs once into a pristine fresh Session, bound to the exact document baseline
revision and epoch. A matching nonzero author epoch requires an explicit constructor.
Restored playback starts paused. Imported role or capabilities cannot replace the trusted
constructor policy. Restore never rewinds counters in place.

Tasks 12/14 replace the runtime with a new `GenerationId` and cancel stale callbacks.
Command identity includes `SessionId`, `GenerationId`, `DocumentId`, document epoch and
the applicable revisions. An old command must fail after replacement even when the new
runtime uses the same `SessionId`.

The implementer reports RED→GREEN checks for these boundaries. Initial independent checks
confirmed normal generation/epoch/role/paused-import cases, but T02-R1 blocks reconstruction
of a valid saved effective state in the initial snapshot. Independent fix1 review closed R1:
paired initialization validates document/checkpoint together before the first evaluation.
Independent fix2 review closed the remaining transition-budget finding. Host replacement and
callback cancellation still require Task 12/14 integration gates.

### Task 03 — full renderer implementation in progress

The coordinator dispatched fresh `/root/sim_renderer_impl` with `gpt-6.1-sol high`, fork none,
from the [accepted-gate renderer brief](../../.superpowers/sdd/sim-widget-rs/task-03-brief.json).
The [accepted source baseline](../../.superpowers/sdd/sim-widget-rs/task-03-baseline/) contains
exactly the 36 final Task 02 files, verified by byte counts/SHA256. Ownership covers new renderer,
model scene contracts, manifests and tools; original course files remain outside this task.
Use B1 public BSDF/shared GPU host architecture, no Holo, 11 px and the compact durable static
reference. Implement the full renderer scope; source guidance and prior pipeline probes do not
close actual draw, picking, lifecycle, payload or independent renderer acceptance.
Implementation is in progress; its independent reviewer/results and Task 03 acceptance are pending.
The subsequent interaction-priority ruling temporarily freezes renderer writes as recorded below.

### Task 03/04 — interaction priority and live preview

After using the actual preview, the user required draggable **all points, arrows and manipulators**,
real 3D orbit/pan/zoom like Houdini, an easy global language choice and smooth antialiased edges.
The coordinator confirmed actual wgpu 30/WebGPU. These mandatory interaction proofs take priority
within the complete Tasks 03/04 plan; Tasks 03/04/18 and all 18 feature scopes remain open.
The [interaction contract](../../.superpowers/sdd/sim-widget-rs/task-03-interaction-contract.json)
is planning/source guidance. Its older pending-approval text predates the current authorization
below; it is not an implementation or acceptance report.

#### Approved geometry and interaction semantics

The coordinator approved existing scalar authority: `origin`, `a/b/theta` and raw `u_raw/v_raw`
feed declared Gram–Schmidt operations into normalized unit basis and O/A/B. The displayed
forward positions, cosines result and provenance come from the existing Session evaluator.
The Rust inverse solver proposes one bounded atomic patch; JS transports gestures/DOM/locale.

| Handle | Required world-space behavior |
| --- | --- |
| A / OA tip | Move A only; O and B remain fixed |
| B / OB tip | Move B only; O and A remain fixed; no artificial authored-axis restriction |
| O | Move O only; A and B remain fixed; recompute scalar shape/plane |
| Group/body | Translate all three by one delta; preserve a/b/theta/c² and basis |
| Shaft/angle/axis/plane/rotation | Explicit labelled tool/binding semantics and real constrained motion; no inert advertised manipulator |

Raw generators are not prevalidated orthonormal parameters. Zero/dependent authored basis
rejects with a typed error; invalid proposals publish nothing. Valid theta 0/pi has empty fill
and real edges/points. Preserve the captured valid plane for inverse collinearity; no NaN,
arbitrary flips, fake geometry or failure that disables stop/reset/recovery. Picking, visible
geometry/labels/focus share one shown/prepared frame; direct drag cuts affected presentation.
Ray/plane/axis constraints explicitly resolve 3D depth, with named reconstruction tolerances.

Navigation source: [SideFX viewing controls](https://www.sidefx.com/docs/houdini/basics/view.html)
and [hotkey reference](https://www.sidefx.com/docs/houdini/start/hotkeys.html).
Space or Alt + LMB tumble/MMB pan/RMB dolly; Shift precision; view-fit forms and scoped H/F.
Focused lab wheel dollies, embedded plain wheel scrolls the page. Navigation modifiers win
before picking; editable fields/contenteditable/IME retain keys. Complete camera state lives
in ViewState; it neither edits mathematical parameters nor enters author/session math history.

Shared extensible catalog and host-level locale own the global language switch, EN/RU first.
All labels, menus, hints, statuses, errors, ARIA, dynamic controls, units and number formats
update together. Stable IDs/TeX/canonical core units remain unchanged; switch preserves
session/time/camera/focus and does not commit an unfinished field edit. Placeholder parity,
third-locale registration and localized input roundtrip remain required proof.

AA requires actual supported MSAA4 pipelines, matching color/depth attachments and resolve,
plus analytic/SDF stroke/point coverage in owned shaders. Public BSDF stays unchanged.
Account simultaneous backing/resolve/multiview allocations before allocation; prove actual
sample counts, DPR1/1.25/2 and fractional-motion edges. Current shader validation is source
evidence only; no MSAA/DPR/AA visual acceptance or resource/size waiver is claimed.

#### Historical coordinated core freeze — superseded by scoped acceptance below

This subsection records the pre-review boundary; current ownership and verdict are recorded below.

Renderer product writes are FROZEN at the
[pre-core manifest](../../.superpowers/sdd/sim-widget-rs/task-03-interaction-pre-core-freeze.json).
Original `/root/sim_runtime_impl` is the sole product writer for the bounded atomic mixed
override patch: `Some(Value)` sets and `None` removes, with explicit playback policy.
Authorization includes `SESSION_VERSION=2` and real V1 session fixture migration, without
document schema/operator changes. Names/signatures remain subject to the delivered reviewed API.
The historical Task 02 accepted snapshot/v1 wire facts and receipt remain unchanged; they do
not accept this new extension. A different-model scoped review must close before renderer
ownership returns. At that historical boundary no patch acceptance had been recorded.

The bounded Task 04 InputCoordinator owns gesture capture and interaction history separately
from author Undo. One successful gesture creates one entry. Cancel restores exact owned override
presence/source and playback ownership by compensating atomic command, retaining monotonic
revisions/events. Never rewind Session, replace it mid-gesture or overwrite unrelated state
from a stale snapshot. Reserve cancellation/history capacity before mutation; reject conflicts,
permissions, duplicate IDs and storage/evaluator failures without partial publication.

The freeze manifest reports native pose 3, cosines 1, camera 4 and shader 2 checks, without
browser gesture handlers. These are worker preparation results; core patch/full input are
not independently accepted. Renderer collapse/recovery fixes wait for product ownership return.

#### Actual live preview — limited working artifact

The coordinator opened [old cosines preview](http://127.0.0.1:43874/cosines.html) for the user
in Chrome tab `1696998488`. The
[preview snapshot](../../.superpowers/sdd/sim-widget-rs/task-03-cosines-preview-snapshot.json)
records `PARTIAL_WORKING_PREVIEW_NOT_TASK03_ACCEPTANCE`: real Rust fields/formula, canonical
c²=13/25/37, play/seek and 2D/3D rendering. It does not yet provide mouse orbit/handles,
actual AA or complete global catalog. WASM SHA256 is
`cc60e0715502e131698cfcc4720f52ec9b62b3f2b8b62f5e0ebf0f957c721351`.
Combined cosines WASM is **320,584 B gzip**, above the unchanged 300,000 B gate; no waiver.

Separate [registered native material trial](../../.superpowers/sdd/sim-widget-rs/task-03-portable-native-registered.json)
and [registered browser trial](../../.superpowers/sdd/sim-widget-rs/task-03-portable-registered-browser.json)
record partial actual draw PASS for five material cases, four groups and max pixel error
0.0017261207. Browser records `PARTIAL_PORTABLE_SHADER_PASS`; invalid shader candidates preserve
valid pixels. Its standalone optimized raster WASM is **110,097 B gzip**, a separate partial
artifact, not the complete cosines payload. Full renderer/material matrix, lifecycle, UI visual,
input, AA, performance and final payload acceptance remain open.

The coordinator's [staged V2 recoverability trial](../../.superpowers/sdd/sim-widget-rs/task-03-cosines-v2-recoverability-check.json)
is `FAIL_PARTIAL_STAGED_PREVIEW_NOT_ACCEPTANCE`. Theta=0 publishes core c²=1 but polygon compile
fails as collinear and leaves former geometry/formula; stop recompilation prevents recovery/seek.
EN also leaves Russian ARIA/title/units/receipt text. Fix empty fill, atomic render publication,
playback-only recovery and complete catalog after renderer ownership returns. The staged tab
was reloaded; the user's old deliverable tab remained untouched. No acceptance follows from it.

The [service renewal](../../.superpowers/sdd/sim-widget-rs/task-03-service-renewal.json) records
`PASS_SERVICE_PRESERVATION_ONLY`: same localhost URL, new owned PID **118580**, 12-hour lifetime
from `2026-10-13T02:50:01.194Z`; known HTML/WASM bytes unchanged and user tab/state preserved.
Browser observed HTTP200 for HTML/JS/WASM and canonical 3/4/60, c²=13. This is service availability
evidence, not interaction/AA/full-renderer acceptance. The older snapshot PID is historical.

#### Core prerequisite accepted — product ownership returned

The [durable Task 04 prerequisite](../../docs/acceptance/sim-widget-rs/task-04-atomic-override-prerequisite.md)
records the bounded Patch/playback/session V2 approval. Initial
[review](../../.superpowers/sdd/sim-widget-rs/task-04-atomic-patch-review.json) is retained as
FINDINGS: R1 event clone/compaction preceded budget admission; R2 executed WASM parity was absent.
Original implementer `gpt-6.1-sol high` corrected both in round 1/5. Independent
`gpt-6-astra high` [fix1 review](../../.superpowers/sdd/sim-widget-rs/task-04-atomic-patch-fix1-review.json)
is APPROVED, R1/R2 closed, zero findings. Review SHA256:
`c3b2c1a5589362383cffc9238e019f423e88ef3a1c1407d36141644a2b2d60a2`.

Final freeze `2026-10-13 03:33:29 UTC` has 17 scoped identities. Only runtime lib.rs and
atomic-override-patch.rs changed from the original patch freeze. Reviewer-run 79 integration tests,
the unchanged allocator case (192 B largest allocation after fix, originally 65,536 B under
192 B event budget), semantic compensation control and actual native/WASM scenario execution passed.
Both native/WASM reports are 157,831 B with SHA256
`a45fd1e278ee68c465e3e6e399688823f709b276b7ac2089b0d4ebedc2535e6b`.
This executes deterministic patch/migration/retention scenarios, not all 79 tests in WASM.

Reviewed API: `CommandKind::PatchLiveOverrides { entries, playback }`, matching EventKind,
`OverridePlaybackPolicy::{Pause, Preserve, SetIntent(bool)}`, and `Session::playback_intent()`.
Session V2 and real homogeneous V1 fixture migration are accepted; document schema/operators
unchanged. The durable receipt records exact permission, no-op, provenance, physical/desired
playback bits and admission contracts. Core provides no gesture Undo or whole-session rollback.
Original runtime implementer completed writing; coordinator returned sole product ownership
to original `/root/sim_renderer_impl`. Historical Task 02 v1 receipt remains unchanged.
Full Task 03/04/18, InputCoordinator and browser direct manipulation remain open.

#### Shared advanced 2D/3D picking — design approved only

The [shared picking design](../../.superpowers/sdd/sim-widget-rs/task-03-shared-picking-design.json)
has SHA256 `9c1c36ddba2d88615bd30028ea7de87a1b30fc2f60ebe6e54232a356e4568f87`.
Independent `gpt-6-astra high`
[review](../../.superpowers/sdd/sim-widget-rs/task-03-shared-picking-design-review.json)
SHA256 `e0f342f040e334cc1ff0a1f8b8704ab9422e79136a0ed33d84022241d65e0992`
returned APPROVED_FOR_IMPLEMENTATION with no design findings. This does not accept implemented input.

One typed resolver uses displayed geometry for hover/press/drag start across 2D/3D ortho/perspective.
Complete stamps bind runtime generation, document/session/epoch/revisions, scene/topology,
view/layout/device generation and displayed sample. Qualified object/subpart/instance physical IDs
are distinct from semantic IDs. Clipping/filtering/opaque occlusion precede priority; masked/
transparent/x-ray/selected-overlay behavior and full-ID ties are explicit.

Bounded BVH indexes real transformed bounds with refit/rebuild lifetimes and measured query/memory
cost. Broadphase includes CSS hit slop; exact local narrowphase preserves world ray t under
nonuniform transforms, negative-determinant winding/backface and triangle barycentrics.
Compare unbiased geometry depth with explicit precision/depth-bias epsilon. Shared capture
compensates via reviewed core patch; a midgesture permission/time/touched-key conflict preserves
current valid state and cannot claim restored after rejected compensation.

The [bounded AA/input review](../../.superpowers/sdd/sim-widget-rs/task-03-input-aa-review.json)
identified prototype PICK-R1 near-plane segment clipping mismatch, PICK-R2 priority preceding
opaque occlusion and PICK-R3 semantic-only unstable ties. The approved design closes the
architecture clarifications; source fixes/native/browser regressions remain required.
That numerical AA reviewer did not have a verified model ID, so no distinct-model Task 03/04
acceptance is claimed from that review.

#### Frozen shared picker — initial review and correction round 1/5

The original renderer implementer `gpt-6.1-sol high` froze the common picker with 10 native
tests. Separate input implementation reports nine tests and six cosine/pose checks passing;
these are worker results, not input acceptance. Browser gesture transport is still pending.
The coordinator keeps the original renderer as sole product writer.

Independent configured `gpt-6-astra high`
[picking review](../../.superpowers/sdd/sim-widget-rs/task-03-picking-review.json)
returned FINDINGS. Receipt SHA256:
`b19d7e48c7e0fcad8fa10ef7862bbbb114d898670b311e248ca7ce835fd63df2`
(16,793 B). The review targets the
[frozen picker](../../.superpowers/sdd/sim-widget-rs/task-03-picking-freeze.json),
not mutable input/bindings/trial consumers. It compiles actual frozen picking.rs by path,
checks source identities and executes 10 retained plus six independent semantic cases:
15 pass, one fails; a separate allocator probe fails. Both probe runs exit101.

| Finding | Independent reproduction and required correction |
| --- | --- |
| PICK-R4, medium | Mutating public camera inverse to identity while VP/CSS stay fixed bypasses stamp validation: admitted ray distance1 instead of9.99. Seal or validate the complete coherent camera mapping and invalidate every mapping state used by query. |
| PICK-R5, high | One Point asset reports resident341 B but retains509 B; measured peak837 B exceeds admitted341+181=522 B. Arc/object/alignment and small BTreeSet/root temporary capacity/lifetimes are missing. Admit conservative complete resident/temporary ceilings before allocation, including aggregate distinct/shared assets, refit and query. |

PICK-R1/R2/R3 close for the executed native clipping, occlusion-before-priority and stable-ID
ordering cases. This is not browser draw/pick pixel parity. The 10k-triangle single debug
run queried four narrowphase candidates and recorded build89,959µs/query23µs/refit9µs;
these diagnostic timings exclude occlusion work and do not establish p95/worst-case performance.

Coordinator authorized original-worker correction round 1/5 while other input work continues.
Preserve the original freeze/review, rerun unchanged probes, freeze native/WASM correction
evidence and obtain scoped Astra review. R4/R5 remain open until that review closes.
No full Task03/04/18, complete memory ceiling, browser input or visual acceptance follows.

#### Picker fix2 and input fix1 — historical findings before scoped closure

Configured independent `gpt-6-astra high`
[picker fix1 review](../../.superpowers/sdd/sim-widget-rs/task-03-picking-fix1-review.json)
returned FINDINGS; SHA256
`1b7333276c536d5f714d3c11234ce6fb4c812f67815e2063dfa53159b6cc7640`
(28,593 B). PICK-R4 closes for the sealed coherent CameraMatrices boundary; the original
PICK-R5 asset-allocation witness is fixed (reported/measured525 B), but aggregate resource
acceptance remains open under PICK-R6.

PICK-R6, medium: empty index reports/adopts resident477 B, then refit accepts larger valid
stamp IDs and retains960 B while counters still report477. Admit checked old/new retained
stamp/index totals before cloning and publish correct counters atomically; test growth,
shrink, repeat and rejection preservation. Reviewer-owned 12 frozen product tests pass;
semantic15pass/1fail and a separate stamp-growth allocator fail. WASM check passes only
compilation, not executed picker/browser parity. Coordinator authorized sole renderer
bounded correction round2/5, preserving prior freeze/reviews and unchanged probes. No waiver.

The [independent input review](../../.superpowers/sdd/sim-widget-rs/task-03-input-review.json)
also returned FINDINGS, configured `gpt-6-astra high` versus original `gpt-6.1-sol high`;
SHA256 `2be81fd1230d274a2400615f1c678e9bee1ec891b765933e72836d568b306aad`
(17,971 B). Scope is frozen InputCoordinator/input tests, not pending press, bindings or browser
transport. Its nine retained plus ten independent native cases give14pass/5fail;
executed allocation probes pass within their stated bounds, not exhaustive core/host peaks.

- INPUT-R1, high: Undo replaces the adjacent older history guard with current context, letting
  older Undo cross external time/playback/document boundaries. Preserve original contextual
  guards and test Undo/Redo boundaries without rebinding them.
- INPUT-R2, medium: external touched-key/time/playback write-away-and-back transitions evade
  final-state equality checks. They must conflict even when the final state matches.

Coordinator authorized original renderer input correction round1/5 using the existing ordered
`Session::events()` interval: distinguish owned/allowed unrelated events from forbidden
transitions; retention gaps fail closed. Preserve contextual history guards; no core API change
is authorized. Earlier accepted core/Task02 receipts stay unchanged. Browser drag is not ready.
PICK-R6 and INPUT-R1/R2 remain open until scoped independent correction reviews close;
full Tasks03/04/18, performance, payload and visual acceptance remain open.

#### Scoped picker/input approvals — browser transport remains open

The coordinator independently verified the final receipt hashes; documentation readback
matched both. Configured independent reviewer remains `gpt-6-astra high`, distinct from
original implementer `gpt-6.1-sol high`. Original FINDINGS reviews and earlier freezes remain.

- [Input fix1 review](../../.superpowers/sdd/sim-widget-rs/task-03-input-fix1-review.json):
  APPROVED, 18,939 B; SHA256
  `ff75ee35492944504fcb176512b98d75587af759c2e6bee52e7beb51a7eb237e`.
  INPUT-R1/R2 close only for frozen InputCoordinator scope. Adjacent context guards remain
  original; borrowed ordered Session event intervals detect touched/global ABA transitions
  and fail closed on continuity/retention gaps. Own unchanged19 semantic,12 product and6 new
  independent native cases pass; allocator probes pass. WASM check is compilation only.
- [Picker fix2 review](../../.superpowers/sdd/sim-widget-rs/task-03-picking-fix2-review.json):
  APPROVED, 27,661 B; SHA256
  `d485311e14947b86f0cb253246fdfb9e7837c4f63f42b15846a958f27d1edc26`.
  PICK-R6 closes within frozen six-file correctness/resource scope; R4/R5 closures retained.
  Own16 semantic/13 product cases, unchanged allocation witnesses/matrix, repeated aggregate
  grow/shrink and atomic rejection cases pass. Final stamp-growth actual/reported968 B
  match; old485 B resident admission rejects unchanged. WASM check is compilation only.

These approvals do not accept bindings/pending press/DOM transport, executed WASM interaction
parity, full renderer/input or browser visual/performance. PICK-P1 remains open:
16 coincident hits cumulatively allocate180,366 B; scoped resource correctness is not
fast/light acceptance. Full Tasks03/04/18 and all release gates remain open; no budget waiver.

The worker has actual browser interaction examples, but the user observed Chrome MMB
autoscroll and ordinary-wheel behavior failing the intended viewport navigation.
Transport correction and root-cause verification are ongoing. Wheel mask/overlay causality
is unresolved; do not claim it as established. Keep input handling viewport-scoped without
KeyboardLock/PointerLock. Existing browser examples do not establish corrected transport
or independent browser acceptance. Accepted native scopes and historical core receipts
remain intact while the original renderer continues as sole product writer.

#### Input consumer review — scoped transport pass, terminal recovery finding

Configured independent `gpt-6-astra high`
[consumer review](../../.superpowers/sdd/sim-widget-rs/task-03-input-consumer-review.json)
returned FINDINGS; SHA256
`a6f1a1846789debcb253f9f7daa5648d38a1d7b24ba9bb374a64d6036d2b7aee`.
All20 frozen source/artifact identities remained unchanged. Actual prescribed Chrome
MMB/wheel cases pass: viewport wheel without focus, outside page scroll, prevented MMB
down/auxclick, AltMMB capture/pan/release, wheel after MMB and Escape release.
Native OS autoscroll was not reproduced through CDP; no claim of universal OS/input coverage.
Four actual screenshots were inspected without a tested wheel cover; no pixel/AA benchmark.

CONSUMER-R1, high: actual public CosinesModel/CosinesInput sequence begins/updates A,
then external field set a=3.7. Both terminal commit and cancel reject correctly, preserving
external truth/revision, but leave consumer drag and coordinator active ownership.
Next begin reports already captured; repeated cancel conflicts. This is independent native
public-consumer evidence; a browser cross-control conflict sequence was not executed.

Coordinator authorized original sole renderer consumer fix1: explicit non-writing abandon/
release for both Rust owners plus terminal finally cleanup, without restoring external
values, rewinding audit, weakening context/history guards or changing prior scoped verdicts.
Correction/independent closure remains pending. Tested normal consumer/browser sequences
do not close terminal recovery or complete input. Current consumer WASM385,547 B gzip exceeds
300,000 B; PICK-P1 and full Tasks03/04/18 remain open without waiver.

#### Full egui-widgets source reuse — source supported, audit correction approved

The [readable51-entry catalog](../../docs/acceptance/sim-widget-rs/egui-widgets-reuse-source-audit.md)
covers50libraries+gallery and the preferred public gizmo boundary at SSH pin
`96fc02ff58f30f3ea8a39193c39f39004a7835b7`.
Actual egui0.36.2/wgpu30.0.1/glam0.33.12/MSRV1.96; author inspector/timeline/ramp/tree/
chrome/actions are candidates, not dependency adoption. Dedicated tensor/code/material
editors are absent; required capabilities remain in scope.

Full catalog receipt282,316 B SHA256
`9f06dec3832f2bca05ee62910ea90e4ffdb8d24ce0a5d249de1bb6527d70d576`;
gizmo receipt14,907 B SHA256
`8a25d3c43cee9eb6b6e182d416c9a89caebd575883a1aaff04541bfca9149749`.
Independent [cross-check](../../.superpowers/sdd/sim-widget-rs/reuse-egui-widgets-crosscheck-review.json)
returned FINDINGS REUSE-R1 only:51inventory/698citations/124sourcehashes supported;
alias gpu-mem must resolve to gpu-info-rs0.1.0 and display/toolbar pollster0.4 closure is missing.
Original receipt/pin preserved. [Current catalog fix1](../../.superpowers/sdd/sim-widget-rs/reuse-egui-widgets-catalog-audit-fix1.json)
285,364 B SHA256 `32fde23b2a22538aabc08abfad39e316959214a9ecdef98126bd271006688ab9`
corrects three closure lists/three basis notes with all51 source entries intact.
[Independent fix1 review](../../.superpowers/sdd/sim-widget-rs/reuse-egui-widgets-catalog-audit-fix1-review.json)
SHA256 `40004443d6150b7f129dcdadefd1103e9ff50860243253c16c17ea865a2ba8e5`
is APPROVED_FOR_REUSE_R1_CORRECTION: audit finding closed, original9f/source features preserved;
no runtime/dependency/performance approval. Consumer fix1 browser commit/Escape recovery is worker-reported only:
external truth retained/fresh gesture works; independent correction review still pending.
Source reads/lock graph are not build,
WASM/runtime/shared-pass/locale/IME/memory/p95/gzip acceptance. One Session/InputCoordinator/
picker/ViewState/GpuHost remains; gizmo private pick/cancel/draw/clip/f32 seams require reviewed
public API integration rather than private calls/source copy/new rotate-scale implementation.
No donor/product changes or feature/budget waiver follow; newer HEAD does not refresh this pin.

#### cglibs source reuse and fast/light requirement

The user explicitly requested fast/light wgpu and named nodes-rs, wgpu-fx-rs,
wgpu-widgets-rs and wgpu-tile-atlas. The
[durable source audit](../../docs/acceptance/sim-widget-rs/cglibs-reuse-source-audit.md)
and updated [reuse plan](../../plans/20261012-explainer-runtime/phase-03-reuse-audit.md)
record public seams, checked pins and limits. Source reports match SHA256/bytes:
nodes/FX `6edc2abdc9d3c044944a1892e394e0362f2f813be87adc76d265b4a6b4b675e4`,
15,760 B; widgets/atlas `7dfe60d4e2a68a51f832a86127a8bbe8647724e45415a63ca8cefb9361e824c1`,
20,023 B. Both inspect clean donors; fresh nodes/FX remote probes and coordinator-reported
widgets/atlas remote verification are distinct from the latter report's local-origin checks.

Keep one Session truth/history: nodes ports/layout/view are editor projection, not another
cook/document. FX supplies SoA/HUD/camera candidates, not generic post-FX; sample1 text,
discarded errors and frame allocations need proof/public API resolution. Widget external hover
and public shared-pass draw fit the common picker, but full catalog arboard WASM support,
core clock/sample/depth and glam boundary remain source risks. Text is a future measured
candidate; TileAtlas is only for an actual raster-tile workload. No dependency adoption is forced.

Demand redraw, dirty refit, coalesced moves and reused buffers need p95 frame/input/query,
peak resident CPU/GPU and coherent WASM measurements. No browser sync readback/poll,
native host submission, upstream/private-cache patch or cglibs source copy is authorized.
SSH immutable refs/consumer transitive lock remain required; StoryTold alone is copied.
Read-only source audits ran no build/test/performance gates and grant no budget waiver.
Current AA323,366 B gzip still exceeds300,000 B; full renderer/input/release remain open.

#### Current isolated AA preview — sample4 and recovery observed

An explicit coordinator exception permitted an isolated compiled AA preview while product
renderer ownership was frozen. Its scratch copy uses accepted Task 02 **v1** core source,
not the then-unreviewed V2 patch. Frozen renderer additions/registration and scratch-only
collinear empty-fill/empty-index fixes are recorded in
[source provenance](../../.superpowers/sdd/sim-widget-rs/task-03-aa-preview/source-provenance.json).
This exception concerned the actual compiled trial; the static design reference's browser
policy block was not bypassed. Scratch fixes do not accept the production collapse/recovery path.

Current user page: [cosines-aa.html](http://127.0.0.1:43874/cosines-aa.html), PID118580.
The [artifact receipt](../../.superpowers/sdd/sim-widget-rs/task-03-aa-preview/artifacts.json)
records these coherent files; coordinator independently verified all three hashes.

| Artifact | Raw / gzip bytes | SHA256 |
| --- | --- | --- |
| cosines-aa.html | 12,919 / 4,561 | `2dd9100b8c7923028c71d9c15960995fe07575be635750aa7f896015dd780fa8` |
| AA WASM | 1,042,729 / 323,366 | `45e1f62d0215b005f3cd5d0a0dbd279de1ef265934d5ada2f62df627b73d59b2` |
| AA JS | 75,004 / 13,639 | `2c5dafa47fc2991a3e4ba2a3aed0d74b49b497e99e481f78e117707d0847ff7b` |

The [browser receipt](../../.superpowers/sdd/sim-widget-rs/task-03-aa-preview/browser-receipt.json)
is PASS_COHERENT_AA_PREVIEW_CONTROLS_NOT_FULL_TASK03. Actual sampleCount4, capsule SDF wrapper,
canonical c²≈13/25/37, field edit, 2D/3D, playback/scrub, theta0→c²1 and pi→c²49 with reset
recovery were observed; GPU console errors empty. Receipt tab1696998536; coordinator separately
opened user tab1696998539. Recorded DPR≈0.9/backing808×505 is not the full browser DPR matrix.
Initial screenshot capture timed out: NOT_CAPTURED_TOOL_TIMEOUT, no visual acceptance.
The subsequent [independent AA review](../../.superpowers/sdd/sim-widget-rs/task-03-aa-preview-review.json)
is APPROVED_FOR_AA_PREVIEW, configured gpt-6-astra high: 60 source/three artifact hashes matched,
13 actual browser cases passed, no page/GPU errors. The
[durable scoped AA receipt](../../docs/acceptance/sim-widget-rs/task-03-aa-preview.md) retains
hashes/provenance/cases and limits. Reviewer screenshot timed out once at 5 s; no browser
pixel/UI-quality acceptance, native six numeric cases remain separate proof. Full input/locale/
size/Task 03/04/18 gates remain open; this bounded approval does not accept those tasks.

Native [AA attestation](../../.superpowers/sdd/sim-widget-rs/task-03-aa-native-attestation.json)
and independent numeric rerun cover six MSAA1/4 cases at DPR1/1.25/2, capsule/disc error below
0.01; MSAA4 triangle fringe pixels254/320/551 versus MSAA1 zero. These static numeric checks
do not prove full browser edge/motion, resize/loss/multiview or preallocation memory union.
Current AA WASM **323,366 B gzip** exceeds 300,000 B. Drag/orbit handlers and complete global
catalog remain unpublished; reviewed core and picking must be integrated into a new artifact.
The old cosines page and staged failure remain historical evidence; full renderer/input/UI release open.

#### Global log recovery receipt

An AA review logging write rebuilt BUG_CDX from a 30,000-character truncated read and reduced
it to 31,598 B. This was a caller logging error, distinct from the missing-file stack disclosure.
Global writes stopped; documentation owner reconstructed only confirmed successful operations,
removed the rejected write's uncommitted suffix and preserved the damaged raw backup.
Prior 283,814 raw bytes are restored exactly, SHA256
`a7b4f5f3d44cbfbe8421c61140bce6bef5dde87f3d5c141cba754e28630b9960`.
Both historical raw-prefix hashes, first30,000 characters and retained last-tail capture matched.
Final288,286 B includes the retained defect and explicit incident correction; SHA256
`0a7db16bb7f785f97c74379e4b76677829bfb53355ef5b56b9bb2d458601a864`.
[Recovery receipt](../../.superpowers/sdd/sim-widget-rs/log-recovery/restoration-receipt.json)
records byte equality/readback. No product source or commit changed in recovery.

#### Consumer fix2 — independently approved bounded correction (2026-10-13)

[Independent fix2 review](../../.superpowers/sdd/sim-widget-rs/task-03-input-consumer-fix2-review.json)
is APPROVED; SHA256 `8a7c771788e8c6b13605ae8dadd2740132a444414ac0738b2f1a91c50c371eb3`,
39,041 B. Configured reviewer `gpt-6-astra high` is distinct from implementer
`gpt-6.1-sol high`. All six frozen files remained unchanged; prior CONSUMER-R1
terminal recovery and the seven unchanged prior files remain supported.

CONSUMER-R2 closes only for the pending-first-success boundary: inverse solving precedes
new ownership; rejected/no-op initial admission releases newly unwritten ownership and
keeps the press pending for current-time rebase. Successful admission owns compensation
before render compilation; an already-started failure retains its real gesture.
No Session replacement, audit rollback or weakening of contextual history guards occurred.

Independent original R2 and terminal-recovery witnesses, 19 unchanged semantic cases,
15 coordinator/16 consumer cases, allocator checks and WASM compilation pass.
Actual Chrome singular A→O during playback followed by a valid move on the same held
pointer succeeds; Escape clears overrides, restores playback intent and releases capture.
Rebuilt Alt+MMB pan and ordinary wheel change camera while retaining exact mathematical
vertices/revision; screenshot is clear. Native OS autoscroll was not reproduced by CDP.
Touch/pen, full DPR, physical blur/visibility and full visual/performance gates are not
claimed. The user's previously reported wheel cover no longer appears; its cause is
unconfirmed. The staged page is `cosines-input-fix2.html`; no canonical promotion follows.

Full Tasks 03/04 and all 18 tasks remain open. PICK-P1 query allocation/performance remains
open; the renderer-containing WASM is **385,747 B gzip > 300,000 B**, without waiver.
The current own translation gizmo is not final professional shared-gizmo acceptance.

#### Current steering — copy the whole Warpbro UI (2026-10-13)

Latest correction, 2026-10-13: «мы КОПИРУЕМ крейты из warpbro и других проектов».
The whole Warpbro UI must be copied and attached to our wgpu viewport; selected required
crates and own-project dependency closure from Warpbro, egui-widgets/gizmo, other cglibs
and StoryTold must be physically vendored into `sim-widget-rs`. The earlier SSH-only policy
and temporary UI-only exception are superseded. Existing immutable source-audit receipts
stay historical evidence; they do not require retaining Git dependencies.
Record frozen source commit/SHA256, copied hashes, licenses/NOTICE/assets and local patches;
resolve own-project dependencies to local vendored paths. **crates.io is not vendored**:
normal registry dependencies and Cargo.lock/checksums remain. No physical copy or completed
port is claimed from this policy update.

The [detailed full UI port plan](../../plans/20261012-explainer-runtime/phase-06-warpbro-ui-port.md)
is materialized from `warpbro-ui-port-plan.json`, SHA256
`fb870f581883058edcb76b7367184096e290b5cc9808bfea3a57902e13bc572e`.
Independent `warpbro-ui-port-plan-review.json`, SHA256
`f1969d65e7cf6492e0f03598d16dbf195ec4d9a49a1dab87704e566a90d95acc`,
is APPROVED_FOR_IMPLEMENTATION for architecture only, with required before-copy preflight.
Warpbro853590699cd220532a15063442fa94e8e3c49794 and egui96fc02ff58f30f3ea8a39193c39f39004a7835b7
freeze11 panels/six settings/46 UI slices and all50 toolkit libraries/226 files.
All78 Warpbro references/226 egui files/46 slices independently match hashes.
Original `/root/sim_renderer_impl` is sole product writer; WP-0 preflight/implementation
is underway. Exact activated transitive file/resources/licenses inventory precedes product copy.
The first raw product copy is now performed as recorded below; normalized library,
new build/browser UI parity and full port acceptance are not claimed. Preserve one
Session/evaluator, InputCoordinator, common picker,
ViewState and GpuHost; donor native execution/domain bindings need explicit adapters.
The source snapshot must be immutable: concurrent donor working-tree edits are foreign.
The observed original WarpBro WASM check failed first in uuid's browser RNG configuration;
that does not certify later application compilation or browser readiness. Full UI,
EN/RU, compact 11 CSS px, no Holo, all 18 tasks and existing budgets remain required.


#### WP-0 first raw product copy — performed, acceptance pending (2026-10-13)

[Независимый copy preflight](../../.superpowers/sdd/sim-widget-rs/warpbro-ui-copy-transitive-preflight-review.json)
APPROVED_FOR_COPY_PREFLIGHT, SHA256
`4646a50ba0c1abbe3247b9ce61be75b3458ad964bc251fa568fe8bcc18daf9c0`.
Проверены immutable inventory/closure и source-copy admission; это не Cargo/runtime acceptance.
Executor выполнил первую raw product copy после этого gate: 1703 extraction entries,
38453205 B, включая 1655 required files с matching hashes, 46 UI slices и 2 raw Cargo references.
Root подтвердил наличие 17 новых vendor roots (плюс прежний StoryTold) и
`provenance/warpbro/{source-reference,source-slices,closure-source.json,source-slices.json}`.

Manifest normalization и portable UI library code сейчас underway у sole product writer.
Новая библиотека/native/WASM/browser ещё не compiled/accepted. WP-0 не принят:
нужны frozen post-patch copy manifest/hash delta, полное local metadata/tree/lock resolution,
actual native/WASM checks и independent post-patch review. Все runtime/UI/performance/budget
и full18 gates остаются открытыми. Raw copy — конкретное исполнение, не завершённый port.


#### Actual editor startup — live GUI, input findings open (2026-10-13)

The actual Rust/WASM **egui + wgpu** editor is running at
[editor.html](http://127.0.0.1:43874/editor.html). Root observed actual Chrome startup
and screenshot of the live GUI, not a PNG/static-reference substitute.
EditorUi is compiled against the 87-package copied closure. Source/artifact freeze:
[wp2-editor-startup-freeze/report.json](../../.superpowers/sdd/sim-widget-rs/wp2-editor-startup-freeze/report.json),
57 paths, SHA256 `bd019a2a92393ed7db898d0628dc1418db02eb9aacbe75e1dc9fca8d6a0bc20b`.
Worker reports real Play, numeric a=4 and B drag; these are reported execution evidence,
not full independent UI acceptance.

Independent startup review is ongoing: EDITOR-R1 is observed — clean field focus leaves
pending/busy ownership, and dirty state clears before command admission. Undo and B-drag
Cancel pass within tested scope; suspected Redo R2 remains unconfirmed. Fixes and their
receipts are pending; frozen source is retained. Latest priority is live input correctness
before further gizmo integration. WP-1 f64 neutral API module (~11 kB) has native check
evidence only; complete kernel/lifecycle integration is incomplete and paused for UI defects.

Actual editor WASM is **10,271,979 B raw / 3,273,482 B gzip**: SIZE FAIL against the
300,000 B renderer-containing artifact gate, no waiver. Native/WASM compilation and live
startup are concrete progress, not WP-0/WP-1/WP-2/full Task03/04/all18 acceptance.
PICK-P1, memory/performance/lifecycle/IME/accessibility/locale/full visual and feature gates
remain open. Original Session/math remains the authority; no private panel history or
core/schema replacement is authorized. Existing manual user state is preserved.
