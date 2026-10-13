# sim-widget-rs

Rust runtime for interactive mathematical and CG/VFX explanations, with a shared document
for an embedded learner, full-window laboratory and visual author editor.

The full implementation is authorized. Task 01 foundation and Task 02 runtime have scoped
independent acceptance; renderer, editor, importers and delivery retain later implementation gates.
See the [implementation ledger](docs/implementation-status.md) for actual progress.

## Development

Work from the `math_en` repository root. The coordinator verified Rust/Cargo 1.99.0 and
the `wasm32-unknown-unknown` target on 2026-10-12. Dependency versions and compatibility
must also be recorded in manifests, lockfile and acceptance evidence.

The following are implementation gates after workspace/dependency resolution, not recorded passes:

```powershell
cargo check --manifest-path sim-widget-rs/Cargo.toml --workspace --locked
cargo check --manifest-path sim-widget-rs/Cargo.toml --workspace --target wasm32-unknown-unknown --locked
cargo test --manifest-path sim-widget-rs/Cargo.toml --workspace --locked
```

Use scoped package gates during implementation. An independent reviewer of a different
model runs the applicable gates and records evidence before a task is closed.
The integrated Bun/browser/performance gates remain required for full release.

## Architecture contract

The root package is a facade. Working workspace children own model, math and runtime;
project, renderer and author crates are added with their implementations.
Web/native applications depend on the facade. The facade does not depend on applications.

Authored tracks and clip layers produce base inputs. Live overrides are applied, constraints
validate the effective inputs, then one Rust evaluator produces truth and provenance.
Presentation springs and rendering consume that frame. New documents use Rust authority.

Author document history, session event history and view state have separate ownership.
Checked `TimeTicks` crosses the browser boundary as a decimal string; donor time types stay
behind checked adapters. Learner and author have separate WASM artifacts.

Saved-state load uses `Session::from_checkpoint` or `Session::from_events` to validate the exact
document and saved effective inputs before first evaluation. Constructor policy stays trusted,
playback starts paused and each replacement gets a fresh generation. Immutable
`FrameSnapshot::objects()` accompanies truth/provenance and publication-relative object dirties.
See the [accepted runtime handoff](../docs/acceptance/sim-widget-rs/task-02-runtime.md).

## Visual contract

User authority, 2026-10-12: «holo не надо»; «нужна добротная и понятная, плавная визуализация»;
«с хорошим приятным UI». This applies to the new widget and copied cosines pilot.

Use a quiet professional Blender-like scene and UI: readable semantic geometry, stable numbers
and labels, smooth continuous presentation and direct pointer response. Typography, spacing,
focus, inspector and timeline must support precise, comfortable use. Decorative Holo/HUD styling
is excluded. This user ruling supersedes the root AGENTS Holo requirement for this scope.
All 18 implementation tasks retain their full functional scope. Migration of existing lessons
and covers remains a separate task.

The user's typography ruling sets the main desktop UI font and ordinary scene labels to
**11 CSS px**. Labels keep their CSS pixel size through geometric zoom; controls retain
comfortable hit areas and headings keep a restrained hierarchy. This scoped choice replaces
the earlier 14 px desktop minimum. Static-reference feedback does not establish UI acceptance.

Use compact control heights, vertical padding and group gaps across all three shells.
Desktop pointer controls are typically 24–28 CSS px high; explicit touch/coarse-pointer mode
keeps hit areas at least 44 CSS px. Density changes retain 11 px typography and every
feature/content route, including library and add-object controls.

[Static UI reference](design/reference.html) and [visual contract](design/ui-visual-contract.json)
are durable design guidance. The [source handoff](../docs/acceptance/sim-widget-rs/ui-reference-source.md)
records exact hashes and the limited source review. Working controls, animation and independently
rendered UI acceptance remain open.

The live-preview requirements also include draggable all points/arrows/manipulators, actual
Houdini-style 3D navigation, an extensible global EN/RU locale catalog and smooth MSAA4 plus
analytic edge coverage. These are mandatory interaction gates within the full plan.
See the [interaction priority and current preview limits](docs/implementation-status.md#task-0304--interaction-priority-and-live-preview).
The old localhost cosines preview demonstrates real Rust fields/formula/playback and 2D/3D
rendering, but does not yet demonstrate mouse navigation, handles, complete localization or AA.
Its WASM is 320,584 B gzip, above the unchanged 300,000 B gate. No full renderer/UI acceptance
or budget waiver follows from that preview or the smaller standalone raster probe.

The current [compiled AA preview](http://127.0.0.1:43874/cosines-aa.html) is a separate isolated
artifact based on accepted v1 core. It demonstrates MSAA4 and recovery at theta 0/pi; its
WASM is 323,366 B gzip and still exceeds the budget. Direct manipulation, navigation and
complete locale remain unpublished. The [atomic core prerequisite](../docs/acceptance/sim-widget-rs/task-04-atomic-override-prerequisite.md)
is separately accepted; shared picking has design approval and still needs implementation.

## Dependency sources

- cglibs: SSH Git dependencies with immutable root revisions and an audited locked transitive graph.
- StoryTold: selected crates and internal dependency closure copied into `vendor/storytold`,
  retaining licenses, source commits, hashes, attribution and a patch journal.
- crates.io: ordinary Cargo dependencies and `Cargo.lock`.

No registry source replacement or cglibs source copies are part of this policy.
The [source audit](../plans/20261012-explainer-runtime/phase-03-reuse-audit.md)
contains checked origins and donor baselines; those records do not replace manifest/lock verification.

## Contracts and acceptance

- [Full featureset](../plans/20261012-explainer-runtime/phase-01-featureset.md)
- [Work packages A–N and execution tasks 01–18](../plans/20261012-explainer-runtime/phase-04-implementation.md)
- [Acceptance matrix](../plans/20261012-explainer-runtime/phase-05-acceptance.md)
- [Contributor instructions](AGENTS.md)

The trial uses an isolated English/Russian copy of the law-of-cosines lesson. Full acceptance
covers all planned feature groups and canonical scenes before existing lessons are migrated.

## License and provenance

Retain each copied dependency's original license/NOTICE and provenance. First-party licensing
must be recorded before distribution; dependency licenses do not establish the facade's license.
