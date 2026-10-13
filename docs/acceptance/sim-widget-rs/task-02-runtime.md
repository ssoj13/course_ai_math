# Task 02 — sim-widget-rs runtime acceptance

Recorded: 2026-10-12. **ACCEPTED for Task 02 runtime scope after correction round 2/5**.
Final independent review: `APPROVED`, `2026-10-13 01:31:40 UTC`; no findings remain open.
[Archive](README.md) · [Ledger](../../../sim-widget-rs/docs/implementation-status.md)
· [Task manifest](../../../plans/20261012-explainer-runtime/phase-04-implementation.md).

## Contents

- [Scope and review identity](#scope-and-review-identity)
- [Correction history](#correction-history)
- [Independent final checks](#independent-final-checks)
- [Accepted API handoff](#accepted-api-handoff)
- [Snapshot and provenance](#snapshot-and-provenance)
- [Limits and later work](#limits-and-later-work)
- [Accepted snapshot appendix](#accepted-snapshot-appendix)

## Scope and review identity

| Role | Agent | Model / reasoning |
| --- | --- | --- |
| Original implementer, both corrections | `/root/sim_runtime_impl` | `gpt-6.1-sol high` |
| Independent reviewer, original/fix1/fix2 | `/root/sim_runtime_review` | `gpt-6-astra high` |

The coordinator confirmed these dispatch identities. Task acceptance combines the initial
runtime review and scoped corrections; fix2 reviewed T02-R5-F1 and directly introduced breakage
only. It preserved fix1 closures and did not start a new broad review. No source commit was made.

Accepted runtime scope includes bounded schema v2/v1 migration, eight parameter kinds and checked
metadata/units/bindings, trusted roles and immutable math authority, atomic document commands and
Undo transactions, incremental immutable truth/provenance/property/object frames, checked final
effective-input constraints, stateless checkpoint/event-suffix replay, and separate ViewState.
Rust fp64 sequential vector-dot parity covers six actual TS fixtures. Full time, tensor/autodiff,
browser crossing, GPU, author UI, assets and portable project delivery retain later task gates.

## Correction history

| Review boundary | Observed result / disposition |
| --- | --- |
| Initial | CHANGES_REQUESTED: 1 P1 + 4 P2. All nine standard gates and 49 integration tests passed; six independent regressions failed, one ordinary snapshot control passed. |
| Fix1 | CHANGES_REQUESTED: R1–R4 closed, original registry admission fixed; R5-F1 transient author-transition accounting remained open. All nine gates, 60 integration tests and seven adapted original regression cases passed. |
| Fix2 | APPROVED: R5-F1 and parent R5 closed; R1–R4 historical closures preserved; no new findings. All final checks below passed. |

R1 corrected saved-state bootstrap: authored defaults may be invalid while the saved time/override
is valid, so paired load evaluates the saved effective state before the first publication.
R2 aligned cumulative checkpoint/export/restore budgets. R3 rejects a newest Undo group that cannot
fit before redo/history mutation. R4 compares object dirties against the previous published draft.
Those four corrections were independently closed at fix1.

R5-F1 reproduced with 25 SceneObjects having 4,000-byte labels, history/events disabled and a
**150,000 B candidate ceiling**. An ordinary label edit was accepted while **396,000 B** of newly
allocated label buffers were simultaneously live during dispatch. The unchanged independent
probe now rejects that edit before copying labels: peak **0 B**, with the same ceiling.
The worker also recorded focused assertion RED→GREEN for three added admission regressions.

Fix2 admits the checked union of old committed/draft state, prospective compiled/evaluated state
and known validation/history/audit temporary lifetimes before creating the atomic candidate.
It borrows comparisons, skips unused history/audit document payloads when retention is disabled,
reserves the actual Undo/Redo target evaluator, and deduplicates only identical shared immutable
contracts. Final candidate validation remains in place. Limits and mathematical constraints were
not increased or relaxed; rejected commands preserve the full state envelope and active draft.

## Independent final checks

All commands were run by the independent reviewer. This documentation transfer did not rerun them.
Workspace directory: `sim-widget-rs`; parity command directory: repository root.

| Command | Final result |
| --- | --- |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo test --workspace --locked` | exit 0; 63 integration tests |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| `cargo build --workspace --locked` | exit 0; native build |
| `cargo check --workspace --locked --target wasm32-unknown-unknown` | exit 0; compilation only |
| `cargo clippy --workspace --all-targets --locked --target wasm32-unknown-unknown -- -D warnings` | exit 0 |
| `cargo build --workspace --locked --target wasm32-unknown-unknown` | exit 0; WASM build only |
| `bun sim-widget-rs/tools/runtime-parity.ts --check` | exit 0; six fp64 sequential dot fixtures |
| `cargo run --locked --example cosines` | exit 0; c²=13/25/37 at 60°/90°/120°, nine real operations per frame |

The 63 integration tests comprise boundaries 12, foundation 8, runtime commands 13, frames 7,
parity 3, schema 6 and session 14. Unit/doc sets with zero tests are not included in this count.
In `.superpowers/sdd/sim-widget-rs/task-02-review-scratch`, the reviewer also ran:

| Command | Result |
| --- | --- |
| `cargo test --offline --locked -- --nocapture` | exit 0; seven prior independent regressions, unchanged in round 2 |
| `cargo run --offline --locked --bin fix1-peak` | exit 0; original 150,000 B case rejects at peak 0 |
| `cargo run --offline --locked --bin fix2-controls` | exit 0; eight independent edit/commit cases and four Undo/Redo transitions |

The original R1 scratch calls were adapted at fix1 to `from_checkpoint`; a strict legacy
`new_with_epoch` expected-Range control and trusted identity/paused/import checks remain.

| Allocation controls at the same 150,000 B ceiling | Retention 0 / 4 result and new label-buffer peak |
| --- | --- |
| 25-object ordinary edit | Both rejected atomically; 0 / 0 B |
| 9-object transaction commit | Both rejected; active draft preserved; 0 / 0 B |
| 1-object ordinary edit | Both accepted; 4,000 / 12,000 B |
| 1-object transaction commit | Both accepted; 4,000 / 20,000 B |

Enabled-history Undo/Redo peaks were 20,000/24,000 B after ordinary edit and 24,000/28,000 B
after transaction commit; correct labels/history were restored. The single-threaded native debug
meter counts allocation minus deallocation for 4,000-byte blocks only while dispatch runs.
Fixture/command preparation, existing buffers, other allocation sizes, allocator overhead and
printing are excluded. These are scoped live-buffer observations, not total-allocation or RSS
upper bounds; the reviewer separately audited checked source lifetime accounting.

## Accepted API handoff

| API | Contract |
| --- | --- |
| `Session::from_checkpoint(doc: ValidatedDocument, registry: OperatorRegistry, config: SessionConfig, identity: SessionId, epoch: u64, checkpoint: SessionCheckpoint) -> Result<Session>` | Construct a fresh runtime from exact saved document and checkpoint together before first evaluation |
| `Session::from_events(doc: ValidatedDocument, registry: OperatorRegistry, config: SessionConfig, identity: SessionId, epoch: u64, envelope: SessionEnvelope) -> Result<Session>` | Construct from the exact saved document baseline and validated event suffix |
| `FrameSnapshot::objects(&self) -> &[SceneObject]` | Immutable publication-specific object contract; dirties compare against the previous publication |

Tasks 12/14 load the matching document, use a paired constructor and replace the host runtime
generation while cancelling old callbacks. Constructor config owns trusted role; import payloads
cannot grant authority. Playback starts paused. Commands include SessionId/GenerationId/DocumentId,
epoch and revisions; an old command fails even if a replacement reuses its SessionId.
One-time import and strict final constraints remain enforced; no in-place counter rewind.
Legacy zero-time constructors remain valid when zero-time defaults satisfy the document.
Render/input consumers use immutable frame truth/provenance/properties/objects and ChangeSet;
view selection/camera state does not enter mathematical author history.

## Snapshot and provenance

Final freeze: **2026-10-13 01:26:30 UTC**, superseding the earlier 01:23 freeze.
Historical base HEAD: `e37479f`; acceptance covers the working-tree snapshot, not a source commit.
The final `task-02-report.json.fix_rounds[1].snapshot` contains 36 owned files. Only two changed
against fix1; 34 product files and all 27 donor/provenance files remained unchanged.

| Final round-2 changed path, relative to sim-widget-rs | Bytes | SHA256 |
| --- | --- | --- |
| `crates/sim-runtime/src/lib.rs` | 69,701 | `7929e934766e10c6c0df69661acc26913f046fa57499d1eb0bd7c63420dd1427` |
| `tests/runtime_commands.rs` | 21,645 | `4e101598f8d82e9ae1391073fa730afc8524993fe4f4eaa2d941de7b6680400f` |

The reviewer verified current/after/package hashes, read the 22,122 B correction patch and checked
the retained donor records. Documentation readback independently matched all 36 final file byte
counts/hashes in fix2-after and Task 03 baseline, confirmed only the two changes versus fix1,
and checked all 27 donor/provenance hashes. Donor closure/licensing remains as recorded in
[Task 01](task-01-foundation.md); manifests/Cargo.lock/dependency policy did not change in fix2.

Final worker report SHA256:
`349bdb870b18399051abae6c5e73c2b052609bc86400b6018f503e58448f7fa3`.
Final independent review SHA256:
`f0d854fe35c24675427c58d8eefd8734c4975279098f9927c4e51716aeff244e`.
The accepted 36-file identities are retained in the appendix below without command-log duplication.

Supplementary local records are gitignored. This document retains their scoped verdict, history,
commands/results, API contracts and snapshot identities:

- [Original task report, final fix_rounds[1]](../../../.superpowers/sdd/sim-widget-rs/task-02-report.json).
- [Original independent review](../../../.superpowers/sdd/sim-widget-rs/task-02-review.json).
- [Fix1 independent review](../../../.superpowers/sdd/sim-widget-rs/task-02-fix1-review.json).
- [Final fix2 independent review](../../../.superpowers/sdd/sim-widget-rs/task-02-fix2-review.json).
- [Final review package](../../../.superpowers/sdd/sim-widget-rs/task-02-fix2-review-package.json).
- [Exact final correction patch](../../../.superpowers/sdd/sim-widget-rs/task-02-fix2-review.patch).
- [Final frozen after snapshot](../../../.superpowers/sdd/sim-widget-rs/task-02-fix2-after/).

## Limits and later work

Admission is conservative and can reject a transition even when each document fits separately.
This preserves a transition ceiling; it is not a final-state-only or measured RSS/performance limit.
WASM check/clippy/build passed; no WASM execution, browser crossing, rendered UI, GPU or
release-profile performance is established here. Donor upstream property tests remain outside
the root suite. Advanced time/branches/simulation, tensors/autodiff, assets, project storage and
host callback disposal require later acceptance. The [static UI source handoff](ui-reference-source.md)
has its own limited source verdict and NOT_RUN rendered gates.

Task 02 acceptance releases its scoped source for Task 03 implementation. The coordinator
dispatched fresh `/root/sim_renderer_impl`, `gpt-6.1-sol high`, from the exact 36-file accepted
baseline. Renderer implementation and independent acceptance remain pending. All 18 tasks and
every full-release gate retain their original scope; no existing course was edited or commit made.

## Accepted snapshot appendix

Paths are relative to `sim-widget-rs`; hashes identify this historical accepted snapshot.
Later Task 03 edits need their own review.

| Path | Bytes | SHA256 |
| --- | --- | --- |
| `.gitignore` | 9 | `306fd52e74fca6746e12acc750f232de15e71da917756a1270d74c91f5eb7368` |
| `Cargo.lock` | 3684 | `8711051f4198af18a8ba401916795ea24abfbc838358a894e8635a1a6ac316b1` |
| `Cargo.toml` | 1061 | `83722ab7b951b7e2d9390b98194e35eee84a80f3a414a302a558eef5a9361dba` |
| `rust-toolchain.toml` | 103 | `6225fcc94ef493fa5d10b39e0adb3ad6fea6c3c70d19746296370046d8866f9f` |
| `tools/runtime-parity.ts` | 1392 | `332d817cd107b7348ebf8bd7e82be36b1ac4b9112a17f4e8f9b29c7576a888d4` |
| `tests/boundaries.rs` | 14235 | `e2ac79ab6a77d9d82a0250ab8fd4f916158cac066c324a16f16ffbd3990c537a` |
| `tests/foundation.rs` | 9529 | `0e86c1989f6c4b8624d9fb332fb97060d3ac55cfa912ad53df84fc10ef0fb0c2` |
| `tests/runtime_commands.rs` | 21645 | `4e101598f8d82e9ae1391073fa730afc8524993fe4f4eaa2d941de7b6680400f` |
| `tests/runtime_frames.rs` | 10410 | `5c55c9fb52c3bb141a603dcc49c2546299a6087dac63f8263d9b1bdc97b04e7e` |
| `tests/runtime_parity.rs` | 6871 | `092268ed33e5339bf8cfd37c4dfa9f01cda037b617694033ed29ad3dc230d08e` |
| `tests/runtime_schema.rs` | 11380 | `e56bde8313469d8a05ae02a2a3f859a968317e2ef359255b7692cf9fa503b231` |
| `tests/runtime_session.rs` | 22998 | `07f867bf75d5d3620f150a70b681fd1b7ae26087fbf23bf929811dc7c2f19c53` |
| `tests/fixtures/cosines.json` | 5468 | `622d26e310feadc9e4f38691092668aab9b7e2f912a098e688bee1ebcd6b048c` |
| `tests/fixtures/runtime/cosines-v2.json` | 6724 | `9ccacf8866c8306d43605c46624c4043637065c5682132ef71b4926dde1d28e5` |
| `tests/fixtures/runtime/dot-fp64.json` | 3112 | `3443a4f357d5e792f26623e717b4e8b033c49f7e44259f68bde30088d34a322e` |
| `tests/fixtures/runtime/support.rs` | 1155 | `f22842cc1865517d12052988b4a2b5276b0dbcee1f5db675231a20fedfd37ff5` |
| `src/lib.rs` | 549 | `d8d69dd7d42d90741d8c76502b84fa90821b08a4de66b34ab01f756b3ed3285e` |
| `examples/cosines.rs` | 1036 | `b7160a2980c2f815f7358c77bb13a2b56613e9a2792925d201d463bd860c2e43` |
| `crates/sim-runtime/Cargo.toml` | 448 | `8657a7ab2d5b77bd24fd71f0ded7c12fee2a2015dd84e2eeb89635f6823c94a0` |
| `crates/sim-runtime/src/commands.rs` | 11315 | `c06def0fb37577237edb5de9bbd3ee5265e87af92214d365657161c57939b895` |
| `crates/sim-runtime/src/history.rs` | 680 | `70b0b2c8b9a97c65b4757239ccb2424cfdc0e87d06a357c68fa8006af815dd25` |
| `crates/sim-runtime/src/lib.rs` | 69701 | `7929e934766e10c6c0df69661acc26913f046fa57499d1eb0bd7c63420dd1427` |
| `crates/sim-runtime/src/presentation.rs` | 2911 | `a68f9a21bf3af252182c532e38d813864cbb88aee856c29f58f90b0004df1933` |
| `crates/sim-runtime/src/session_state.rs` | 21402 | `3f82111462bc44d7a0498e574c626f38fc9f17ea2aad948350a1744a04a42ac3` |
| `crates/sim-runtime/src/track.rs` | 3321 | `22d8c234ed69630c54776db4dc780c8d6c64223217f97fea29e54f335d9e2caf` |
| `crates/sim-runtime/src/view_state.rs` | 3643 | `4a99332e235c31e88684ea0fa834231d106d3a7b9b8d9b4dcfbb57e7535ba77d` |
| `crates/sim-model/Cargo.toml` | 227 | `0c83dc60ca8cb85869568242f322b361292ac9c7ee4306d42bf37915de01b267` |
| `crates/sim-model/src/binding.rs` | 5010 | `f342c1e709b58cc8f735ec73ec650aae5a25b03415f451fdf0e427b21c356a15` |
| `crates/sim-model/src/diagnostic.rs` | 1493 | `4822ea57692dd6fe09db35fff6a9ca47892522b4e1201e2c01e41324ef812e8a` |
| `crates/sim-model/src/document.rs` | 41126 | `46eeeba65cdd86a89eea46ae3284fce7be392452e7d8b6dda740039f1e8372bb` |
| `crates/sim-model/src/lib.rs` | 224 | `c9e8d7d4f33ecad39c29adeb3e85baea49c6d0759d8b0551dbc786d5ea3758e1` |
| `crates/sim-model/src/migration.rs` | 6799 | `d7f40e2c63b7ffe025dea8362263dba7ce98cd6e4878446aa28ff5ba79e6d2ad` |
| `crates/sim-model/src/time.rs` | 3636 | `04a6fde0a66d31b248ec1b28141063514ad03fb2bee000d5f392f62f7b3dd5d5` |
| `crates/sim-model/src/value.rs` | 12493 | `b5616a6015d6180a9e6e50c1f892a2c50f331e5871af1263c3ad0331f959f703` |
| `crates/sim-math/Cargo.toml` | 238 | `0f1604f52b187e0893ac5cfe679a44e78dfc64526ffbc55807c3de2db2cb2804` |
| `crates/sim-math/src/lib.rs` | 20819 | `e3919c6f9625caabe52bb7f24d99602e04635b26a7152d4998abfb725c2cfa8e` |
