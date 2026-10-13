# sim-widget-rs — implementation instructions

Read [root AGENTS](../AGENTS.md), the [full implementation plan](../plans/20261012-explainer-runtime/phase-04-implementation.md)
and the [ledger](docs/implementation-status.md) before editing. Root budgets,
formal Russian «вы», GitNexus workflow and failure-report destinations apply here.

## Scope and ownership

This directory is the isolated implementation area on `feat/engine`. Other agents share the
checkout: preserve their changes, use explicit file ownership, and never reset or stash their work.
Source donors are read-only. Existing lessons, manifest/sidebar and progress remain under their owners.
Commit only when explicitly requested.

Implement the full professional featureset. Internal waves follow dependencies; a foundation
or cosine pilot does not establish full-system completion. Create a child crate with working
behavior and meaningful acceptance; no empty packages, `todo!()`, `unimplemented!()`, panic
placeholders, fake mathematics or claimed gates that were not run.

## Visual authority

The user's 2026-10-12 instructions for the new widget and copied cosines pilot are:
«holo не надо», «нужна добротная и понятная, плавная визуализация», «с хорошим приятным UI».
These instructions supersede the stale root AGENTS requirement for Holo in this authorized scope.

Build a quiet professional Blender-like scene and pleasant controls. Preserve readable semantic
geometry, stable numbers/labels, smooth continuous presentation and direct drag/scrub response.
Use clear typography/spacing/focus states, inspector and timeline. Do not restore decorative
Holo, galaxies, glowing HUD instruments or their reference obligations from older notes.
All 18 tasks and professional capabilities remain in scope; user quality settings do not silently
hide teaching content. Existing lesson migration and covers are separate scope.

Typography authority, 2026-10-12: the user chose **11 CSS px** for the main desktop UI
font and ordinary scene labels. Scene label size stays fixed in CSS pixels through geometric
zoom. Keep comfortable hit areas and a restrained heading hierarchy. This choice supersedes
the earlier 14 px desktop minimum for the new widget/pilot. Positive static-reference feedback
does not close rendered review, runtime UI or tests; browser reference review remains `NOT_RUN`.

Density authority, 2026-10-12: reduce control heights, vertical padding and group gaps across
embedded, learner and author shells. Retain desktop 11 px typography. Pointer controls are
typically 24–28 CSS px high; declared touch/coarse-pointer mode keeps at least 44 CSS px
hit areas. Keep that input-mode policy explicit and preserve all content and control reachability.
User screenshot feedback does not establish independent rendered acceptance.

Read the [durable static reference](design/reference.html),
[UI contract](design/ui-visual-contract.json) and
[source handoff](../docs/acceptance/sim-widget-rs/ui-reference-source.md) before implementing UI.
Density iteration 2 passed scoped source guidance review only. Preserve its 36 px topbar,
28 px headers, 24–26 px rows, 24 px numeric envelope and 8 px groups/padding with coarse
hit areas ≥44 px. Runtime controls/animations and rendered/accessibility acceptance need
their own implementation and review. For this static-reference source review,
browser/rendered/mobile/interaction/performance remain `NOT_RUN`; later live trial evidence
is recorded separately in the ledger.

## Architectural contracts

- Compact DAG: `sim-model` → serde/thiserror; `sim-math` → model; `sim-runtime` → model/math
  and the checked keyframe adapter. Later: project → model; render → runtime/render-rs;
  author → runtime/project/craft-ui/egui. Root facade → runtime/project; web/native → facade.
- For new documents: tracks/layer blend → live overrides → constraints → one Rust evaluator
  → truth/provenance → presentation → renderer. Invalid commands make no partial writes.
- Author document revision/history and session revision/events are distinct. Selection,
  hover and camera are view state, not mathematical history.
- Public `TimeTicks` uses checked arithmetic/conversion and decimal wire strings. Validate
  nonfinite values, overflow and donor range before using EffectCraft time/keyframe APIs.
- Learner and author compile to separate WASM artifacts. Verify artifact contents and load paths.
  Embedded DOM/KaTeX versus egui author accessibility remains a measured shell decision.

Task 02's [accepted runtime handoff](../docs/acceptance/sim-widget-rs/task-02-runtime.md) is the
source boundary for downstream implementation. For saved state use paired
`Session::from_checkpoint(doc, registry, config, identity, epoch, checkpoint)` or
`Session::from_events(doc, registry, config, identity, epoch, envelope)` before first evaluation;
do not bootstrap at zero-time defaults when validity depends on saved time/overrides.
Trusted constructor config owns role. Import is one-time/paused; replacement creates a new
GenerationId and the host cancels old callbacks. Do not rewind an active Session.
`FrameSnapshot::objects(&self) -> &[SceneObject]` owns the publication-specific object contract;
ChangeSet compares against the previous publication, including preview/cancel transitions.
Candidate ceilings include old/new/temporary author transition lifetimes, not just final state.

## Interaction priority and coordinated core patch

After the live preview, the user required draggable **all points, arrows and manipulators**,
real 3D orbit/pan/zoom like Houdini, an easy global language switch and smooth antialiased edges.
This prioritizes an interaction proof inside Tasks 03/04; all 18 tasks retain their full scope.
Read the [interaction contract](../.superpowers/sdd/sim-widget-rs/task-03-interaction-contract.json)
with the coordinator's current ownership ruling in the ledger. Its older pending-approval
fields do not supersede the subsequent bounded patch authorization.

- Geometry: authored `origin`, `a/b/theta` and raw `u_raw/v_raw` generators feed declared
  Gram–Schmidt nodes in the existing evaluator. Normalized basis outputs have real provenance.
  The inverse Rust solver proposes one atomic patch; it does not become a forward authority.
  A-only keeps O/B fixed; B-only keeps O/A fixed; O-only keeps A/B fixed. Group translation
  moves all three and preserves shape. B is not artificially locked to its original axis.
  Expose actual endpoint, shaft/group, angle and axis/plane/rotation semantics with stable IDs.
  Reject invalid zero/dependent authored basis with typed errors. Valid theta 0/pi collapse
  produces empty fill and real edges/points; it must not poison stop/reset/recovery controls.
- Navigation follows [SideFX viewing controls](https://www.sidefx.com/docs/houdini/basics/view.html):
  Space or Alt + LMB tumble, MMB pan, RMB dolly; Shift gives precision. Support view-fit forms
  and scoped H/F shortcuts. Focused lab wheel dollies; embedded plain wheel scrolls the page.
  Navigation modifiers win before picking. Inputs/contenteditable/IME retain their keys;
  complete camera state belongs to ViewState and never creates mathematical history.
- Locale is a shared extensible catalog and host-level state, initially EN/RU, with an explicit
  global switch. Translate every label/hint/error/ARIA name, dynamic control, unit and number
  format together. Keep IDs, TeX symbols and canonical core units unchanged. Language switches
  preserve session/time/camera/focus and do not silently commit field edits.
- AA requires actual supported MSAA4 color/depth/pipelines and resolve plus analytic/SDF stroke
  and point coverage in owned shaders. Public BSDF WGSL stays unchanged. Account all simultaneous
  backing/resolve/multiview allocations before allocation; prove sample counts and DPR1/1.25/2.
  Current preview and source validation do not establish that proof or waive size budgets.

The [bounded core prerequisite](../docs/acceptance/sim-widget-rs/task-04-atomic-override-prerequisite.md)
is now independently APPROVED after fix1. Reviewed exports are `CommandKind::PatchLiveOverrides`
with Some/None entries, `OverridePlaybackPolicy::{Pause, Preserve, SetIntent(bool)}`, matching
EventKind and `Session::playback_intent()`. Session V2 and real V1 migrations are accepted within
that scope; document schema/operators are unchanged. Preserve historical Task 02 v1 receipts.
Original runtime implementer finished writing; product ownership returned to original
`/root/sim_renderer_impl` as sole product writer. Full Task 04 input is not accepted.
Task 04's bounded InputCoordinator then owns one gesture/one interaction Undo, exact override
absence and cancel compensation with monotonic revisions/events. Do not substitute author Undo,
replace Session mid-gesture, rewind counters or overwrite unrelated state from a stale snapshot.

## Shared 2D/3D picking and current AA boundary

Read the [shared picking design](../.superpowers/sdd/sim-widget-rs/task-03-shared-picking-design.json)
and [design review](../.superpowers/sdd/sim-widget-rs/task-03-shared-picking-design-review.json).
APPROVED_FOR_IMPLEMENTATION is design permission, not implemented picking acceptance.
Use one displayed-geometry resolver for hover/press/drag start across 2D/3D ortho/perspective,
with complete runtime/document/session/scene/topology/view/layout/device/display stamps.
Qualified physical object/subpart/instance identity is separate from semantic IDs.
Apply clipping, eligibility and explicit opaque/masked/transparent/x-ray/overlay occlusion
before priority; break ties by complete stable IDs. BVH refit/rebuild/query memory and cost are bounded.
Conservatively include CSS hit slop in broadphase; exact local narrowphase retains world ray
parameterization under nonuniform transforms, negative-determinant winding and unbiased depth.
Prototype near-plane, occlusion and tie findings require actual implementation/regressions.
No lesson-name branches or per-move full mesh scans replace the shared picker.

An explicit coordinator exception allowed an isolated compiled AA trial on a copy of accepted
Task 02 v1 core while product ownership was frozen. Only scratch collapse/empty-index handling
changed there; no unreviewed V2 core API was used. The actual page is
[cosines-aa.html](http://127.0.0.1:43874/cosines-aa.html). It demonstrates MSAA4, fields/play/seek,
2D/3D and 0/pi reset recovery; mouse manipulation/navigation and complete locale remain unpublished.
Its 323,366 B gzip WASM exceeds 300,000 B; full AA/DPR/lifecycle/resource/visual/size gates remain.
Static design-reference browser policy remains unchanged; the exception concerns the actual
compiled product trial. Integrate the accepted patch and shared picker through reviewed source
and a new coherent artifact snapshot before claiming direct manipulation.

## Dependencies and gates

User authority, latest 2026-10-13: «мы КОПИРУЕМ крейты из warpbro и других проектов».
Physically vendor the selected required crates and their own-project dependency closure
from Warpbro, egui-widgets-rs (including the selected gizmo/widgets), other cglibs and
StoryTold into `sim-widget-rs`. This supersedes the earlier SSH-only ruling and the
temporary Warpbro-UI-only exception. Copy the whole Warpbro UI with our wgpu viewport.
Record source repository/path, frozen committed revision, source/copied SHA256, licenses,
NOTICE, assets, features and local patch provenance. Resolve own-project references to
local vendored paths; preserve foreign donor edits. **Do not vendor crates.io**:
registry packages remain normal Cargo dependencies pinned by Cargo.lock/checksums.
The [full Warpbro UI port plan](../plans/20261012-explainer-runtime/phase-06-warpbro-ui-port.md)
is independently APPROVED_FOR_IMPLEMENTATION for architecture only. Exact activated
transitive file/resource/license preflight is required before product copy; original renderer
remains sole product writer in WP-0. Actual copy/build/browser acceptance needs separate receipts.
Preserve one
Session/evaluator, InputCoordinator, common picker, ViewState and GpuHost, all 18 tasks
and budgets; sourcing authorization does not establish implemented UI parity.
Selected StoryTold crates and their internal closure live in `vendor/storytold`; retain licenses,
source commits, hashes, resources and local patch provenance. Registry crates use normal Cargo
resolution. Choose the latest compatible proven tool versions and record verification evidence.

User fast/light wgpu requirement is explicit. Read the
[cglibs source audit](../docs/acceptance/sim-widget-rs/cglibs-reuse-source-audit.md):
nodes is a Session-derived editor projection, not a second cook/document/history.
FX SoA/HUD/camera, widget external hover/public shared-pass draw and future text/tile cache
are measured candidates; vendor the selected required closure under the current copy policy. Keep host GPU ownership,
common stamped picking and typed Session intents. Demand redraw, dirty refit, coalesced moves
and reused buffers require p95 frame/input/query, peak CPU/GPU resident memory and coherent
WASM receipts. Browser sync readback/poll and private cache/upstream patches are excluded.
Source-only risks and native diagnostic timing grant no performance or payload waiver.

Before symbol edits check graph freshness/impact; report HIGH/CRITICAL and reanalyze afterward.
Use scoped Rust gates while implementing. Independent reviewers run their own applicable checks
and must use a different model from the implementer. Full integration and pre-commit gates follow
root instructions. Never mark full A–N acceptance complete from foundation checks.

Update the ledger with task state, concrete outputs, unresolved blockers and review evidence.
When skill templates/scripts do not match this workspace, use explicit task briefs and the ledger;
log reproducible skill/reference failures in the prescribed BUG3 report. Do not invent evidence.
