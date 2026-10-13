# Task 04 — atomic override core prerequisite

Recorded: 2026-10-12 (local). Verdict: **APPROVED after correction round 1/5**, scoped prerequisite only.
[Acceptance index](README.md) · [Ledger](../../../sim-widget-rs/docs/implementation-status.md).
The coordinator returned sole product ownership to the original renderer implementer.
Full Task 03 renderer, Task 04 input and Task 18 release remain open.

## Accepted boundary and authority

Implementer: `gpt-6.1-sol high`, original `/root/sim_runtime_impl`.
Independent reviewer: `gpt-6-astra high`, `/root/sim_atomic_patch_review`; configured model/effort
are established by coordinator dispatch. Final freeze: **2026-10-13 03:33:29 UTC**.
The earlier 03:25:20 freeze and pending worker statuses are historical, not the final verdict.

The [original review](../../../.superpowers/sdd/sim-widget-rs/task-04-atomic-patch-review.json)
retains its `FINDINGS` verdict. The
[fix1 review](../../../.superpowers/sdd/sim-widget-rs/task-04-atomic-patch-fix1-review.json)
is `APPROVED`: R1/R2 closed, no findings. This accepts the bounded mixed override command,
playback policy and session V2 migration at the exact snapshot below.
[Task 02](task-02-runtime.md) remains the historical accepted v1 runtime; its receipt was not rewritten.
No document schema, operator, product dependency, donor or course changes belong to this core patch.

## Public handoff

`CommandKind::PatchLiveOverrides { entries: Vec<(ParameterId, Option<Value>)>, playback: OverridePlaybackPolicy }`
and the matching `EventKind::PatchLiveOverrides` carry one atomic mixed patch.
`OverridePlaybackPolicy::{Pause, Preserve, SetIntent(bool)}` and
`Session::playback_intent(&self) -> bool` are the reviewed exports.

- `Some(value)` sets an override; `None` removes it. Validate all entries and evaluate the final
  combined candidate once at unchanged logical time. Keep normal identity/generation/epoch/revision checks.
- Reject duplicate IDs before map conversion. Learner set requires overrides permission; removal
  requires return-to-guide permission. Both require writable, exposed, unlocked parameters.
- Pause sets physical playing and desired intent false; Preserve retains both;
  SetIntent(v) sets both to v. SetIntent requires playback authority; Pause checks authority when bits change.
- Exact no-op preserves revision, sequence, author redo and playback. Equal-number removal is
  a source/provenance change. Successful cancellation is a new monotonic compensation event.
- Bound incoming capacities, serialized command/event/checkpoint size, current/rolling checkpoint,
  candidate old/new lifetimes and retained event growth before copying or publishing.

Capture both playback bits downstream. Already physically paused gestures use Preserve, including
imported `playing=false/intent=true`. An active first update uses Pause; owned compensation may
use SetIntent(true) only after conflict checks. Core provides no gesture history or whole-session rollback.
The bounded InputCoordinator still must implement touched-key capture, one gesture/one interaction Undo,
pointer cancellation and explicit permission/time/generation conflicts.

## Session protocol V2

`SESSION_VERSION=2`; document schema/operators remain unchanged.
Supported homogeneous V1 checkpoints and envelopes normalize at decode, paired initialization,
restore and replay boundaries. Known old events retain exact ticks, override absence, provenance,
epoch and revisions. Imports physically pause and preserve desired intent; trusted role remains constructor-owned.

Serializers emit V2 for supported V1/V2 typed payloads. Unsupported future/mixed versions and
a Patch falsely claimed as V1 reject. Older command decoders reject the unknown Patch tag.
Five real V1 fixtures were exported through accepted v1 public APIs before codec edits, frozen in PRE,
and independently matched byte-for-byte; full/evicted/zero event envelopes include extreme i64 times.
The generator SHA256 is `ffeaae454bc102b7fb0431fea6f160b2cfd59771bc35525e3083276adff3a67e`.

## Review and correction chronology

| Stage | Independent result |
| --- | --- |
| Initial review | 78 native tests and WASM compile passed. Independent mixed-patch/compensation control passed; allocation regression failed. R1/R2 remained blocking. |
| R1 (P2) | An 8,192-element vector with capacity 16,384 and event limit 192 B returned Limit atomically, but allocated a 65,536 B compact payload before admission. |
| R2 (P2) | Compilation proved no executed native/WASM parity for patch/playback/compensation/eviction/migration. |
| Fix round 1/5 | Only runtime lib.rs and atomic-override-patch.rs changed from the original patch freeze. Borrowed PatchEventView serialization and checked compact-event allocation precede clones/compaction. An unchanged independent allocator case now peaks at 192 B. |
| Scoped re-review | 79 native integration tests, allocator/control probes and executed native/WASM parity passed; R1/R2 closed, zero findings. |

The reviewer rebuilt native/WASM offline from actual core path dependencies, generated wasm-bindgen
NodeJS glue and executed WASM in Bun. Both deterministic reports are **157,831 B** with exact SHA256
`a45fd1e278ee68c465e3e6e399688823f709b276b7ac2089b0d4ebedc2535e6b`.
Cases cover retention 0/1/2/8, real V1 full/evicted/zero migrations and checkpoint,
exact source absence/provenance, monotonic compensation, atomic invalid cases, paused import/desired
intent and budget denial. This does not execute all 79 native tests in WASM.

## Independent commands and limits

All commands below exited 0; exact arguments/log paths are retained in the final review.

| Check | Directory / evidence |
| --- | --- |
| `cargo test --locked -p sim-widget-rs --tests` | sim-widget-rs; 79 integration tests |
| Compile unchanged task-04-review-probe.rs with rustc --edition=2024 --test | Exact --extern/-L/output paths in review receipt |
| Run event_limit_must_precede_large_payload_compaction, exact/single-threaded | Native allocator regression; largest allocation 192 B |
| Run independent_mixed_clear_set_roundtrip_and_compensation, exact/single-threaded | Independent semantic control |
| Run task04-native-parity with cargo run --locked --offline | Scratch parity harness; exact output argument in review receipt |
| `cargo build --lib --locked --offline --target wasm32-unknown-unknown` | Scratch parity harness; actual core dependencies |
| `wasm-bindgen target/wasm32-unknown-unknown/debug/task04_core_parity.wasm --target nodejs --out-dir pkg` | Scratch parity harness |
| `bun run-parity.mjs` | Executes WASM and compares exact report bytes |

The implementer additionally records nine scoped fmt/native/WASM clippy/build/check/cosines/TS parity
gates passing; they are worker results, not additional reviewer-run commands.
The scratch harness retains the product's 20 registry versions; Rust 1.99.0, Bun 1.4.3 and
wasm-bindgen crate/CLI 0.2.129 are recorded. Bound module SHA256:
`6bd7d8361d9e1a193c8f1a7616cf94ffc0980c2d21805f2642e680b8ed8e9508`.

Allocator evidence is a targeted native debug regression, not total RSS/release/browser memory measurement.
Core approval excludes renderer/GPU/browser controls, full BVH/picking, InputCoordinator,
visual/DPR/lifecycle/payload/performance and all remaining A–N gates. No product tests were rerun by
the documentation owner; no source commit was made.

## Durable snapshot and receipt identity

The [fix1 worker report](../../../.superpowers/sdd/sim-widget-rs/task-04-atomic-patch-fix1-report.json)
SHA256 is `a9d866cf949dc326c39ecde05e073759d185c107d91d8c231eaf3199e3aa3b05`.
Final review SHA256: `c3b2c1a5589362383cffc9238e019f423e88ef3a1c1407d36141644a2b2d60a2`.
The frozen [fix1-after snapshot](../../../.superpowers/sdd/sim-widget-rs/task-04-atomic-patch-fix1-after/)
retains these 17 identities. The facade, six prior test files and five V1 fixtures are unchanged
through the fix; runtime_session changes unsupported-version literals to SESSION_VERSION+1.
The original report/review remain separate evidence, including their initial limitations.

| Frozen path | Bytes | SHA256 |
| --- | --- | --- |
| sim-widget-rs/crates/sim-runtime/src/commands.rs | 15596 | `6d454c7bf45a740c520a597f6751775fc625115971b583fa3846daf5510a460a` |
| sim-widget-rs/crates/sim-runtime/src/lib.rs | 80241 | `afcbb0ffbd1a0d8358bcf0ce6b2b8fd7d5b5dd691aa59250ee22eec06a741510` |
| sim-widget-rs/crates/sim-runtime/src/session_state.rs | 28576 | `0c9a7c9d727df2a96d2f7e06a3c544939b38a8b1704de0960808b7e41cbe2e2e` |
| sim-widget-rs/src/lib.rs | 549 | `d8d69dd7d42d90741d8c76502b84fa90821b08a4de66b34ab01f756b3ed3285e` |
| sim-widget-rs/tests/boundaries.rs | 14235 | `e2ac79ab6a77d9d82a0250ab8fd4f916158cac066c324a16f16ffbd3990c537a` |
| sim-widget-rs/tests/foundation.rs | 9529 | `0e86c1989f6c4b8624d9fb332fb97060d3ac55cfa912ad53df84fc10ef0fb0c2` |
| sim-widget-rs/tests/runtime_commands.rs | 21645 | `4e101598f8d82e9ae1391073fa730afc8524993fe4f4eaa2d941de7b6680400f` |
| sim-widget-rs/tests/runtime_frames.rs | 10410 | `5c55c9fb52c3bb141a603dcc49c2546299a6087dac63f8263d9b1bdc97b04e7e` |
| sim-widget-rs/tests/runtime_parity.rs | 6871 | `092268ed33e5339bf8cfd37c4dfa9f01cda037b617694033ed29ad3dc230d08e` |
| sim-widget-rs/tests/runtime_schema.rs | 11380 | `e56bde8313469d8a05ae02a2a3f859a968317e2ef359255b7692cf9fa503b231` |
| sim-widget-rs/tests/runtime_session.rs | 23117 | `b10983aac1a6dcef44ccd41b94a9511369ce933bd82f9ce885a4ba90859fe21c` |
| sim-widget-rs/tests/atomic-override-patch.rs | 32844 | `d7a52c874c038a06abf33ac2607584812e33b3e361797a2c9f418140c71475a6` |
| sim-widget-rs/tests/fixtures/runtime/session-v1-document.json | 3609 | `27e83d73731004764b5ecf9aef3bde9881084e59acf89b6f1273c3cb3197fe36` |
| sim-widget-rs/tests/fixtures/runtime/session-v1-checkpoint.json | 222 | `207b557f7a7d6f2f73cb0c9bd3624dc5a61b21e0dfb5f3f0ffdb6340086fb6a9` |
| sim-widget-rs/tests/fixtures/runtime/session-v1-events.json | 1705 | `46bbc1dea70172b5a604e44a96fdb7b6f57944845dfa49f8fc2ccceec4224063` |
| sim-widget-rs/tests/fixtures/runtime/session-v1-evicted-events.json | 699 | `8a3bffa2ebc15efa0bfa10f3a08fdc1658a9afeee4d9455d4efa679f08226846` |
| sim-widget-rs/tests/fixtures/runtime/session-v1-zero-events.json | 286 | `78274c59ceb638db3b69ed5b9c2f5343042dffbc72315330ac528b6c02efd0df` |
