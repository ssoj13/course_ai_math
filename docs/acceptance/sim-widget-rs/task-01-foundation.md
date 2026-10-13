# Task 01 — sim-widget-rs foundation acceptance

Accepted: 2026-10-12, after correction round 1/5. Verdict: **APPROVED for foundation scope**.
T01-R1 is addressed; zero findings remain open in this scoped review. No commit was created.
[Acceptance index](README.md) · [Execution ledger](../../../sim-widget-rs/docs/implementation-status.md)
· [Task manifest](../../../plans/20261012-explainer-runtime/phase-04-implementation.md).

This record preserves the decision and measured results independently of the gitignored scratch
JSON/logs. It records a historical working-tree snapshot; later task edits need their own review.

## Contents

- [Review identity and scope](#review-identity-and-scope)
- [T01-R1 correction](#t01-r1--reproduced-corrected-independently-rechecked)
- [Recorded commands and results](#recorded-commands-and-results)
- [Source provenance and accepted snapshot](#source-provenance-and-accepted-snapshot)
- [Limitations and gates still open](#limitations-and-gates-still-open)
- [Local evidence references](#local-evidence-references)
- [Accepted snapshot appendix](#accepted-snapshot-appendix)

## Review identity and scope

| Role | Agent | Model | Reasoning |
| --- | --- | --- | --- |
| Implementer | `/root/sim_foundation_impl` | `gpt-6.1-sol` | high |
| Independent reviewer | `/root/sim_foundation_review` | `gpt-6-astra` | high |

The coordinator confirmed both dispatch identities. The original worker report's inherited-model
placeholder and pending-review text are historical; the final independent re-review supplies
the acceptance verdict. The reviewer ran its own commands and did not modify product source.

Task 01 replaces the generated Cargo scaffold with a root facade and three working Rust crates:
`sim-model`, `sim-math`, `sim-runtime`. Toolchain: Rust/Cargo 1.99.0, edition 2024, resolver 3;
native Windows execution and `wasm32-unknown-unknown` compilation were checked.

| Accepted foundation area | Reviewed behavior |
| --- | --- |
| Model | Bounded document decoding; typed IDs, finite values, shapes/units/ranges; explicit schema/capability/reference/cycle diagnostics; checked decimal-string TimeTicks and rational arithmetic |
| Math | Frozen operator descriptors; typed scalar/vector graph evaluation; real operands/results and provenance; singular/nonfinite/type/unit failures |
| Session authority | Tracks then live overrides, final constraints/ranges then one evaluator; atomic revision checks; separate author/session history; transaction commit/cancel and bounded Undo/Redo |
| Time/presentation | Checked local donor scalar-track adapter, rebased segments and explicit span limits; presentation reads immutable truth without changing mathematics |
| Facade/example | Actual API reexports; headless cosines fixture computes c²=13/25/37 at 60°/90°/120°, with nine real operations per frame |

Scalar/vector operators were implemented. Validated matrix/integer/bool storage does not establish
matrix/tensor operators or autodiff acceptance. GPU, browser bindings and UI were outside this package.

## T01-R1 — reproduced, corrected, independently rechecked

The initial independent review returned `CHANGES_REQUESTED` for one P2. An EasyEase segment
whose endpoints were both exactly 1.0 passed through donor cubic interpolation. At 1% progress
it returned 0.9999999999999999: f64 bits 4607182418800017407 instead of 4607182418800017408.
A valid theta range [1,1] then failed seek with `Range` at `parameters/theta`; an Equal constraint
against a fixed 1.0 parameter also failed.

The reviewer used the cosines fixture with theta default/range/key values set to 1 and EasyEase,
then sought to `TICKS_PER_SECOND / 100`. Its unchanged scratch command
`cargo test --offline -- --nocapture` exited 101: two constant-track regressions failed,
while the large-absolute-time/mode test passed.

The original implementer added two regressions and recorded assertion RED before the fix:
`cargo test --locked --test boundaries constant_easy_ease -- --nocapture` exited 101.
The same command exited 0 after the fix. Production change: `ScalarTrackAdapter::sample`
returns the exact authored value when `a.value == b.value`, before donor interpolation.
Nonconstant segments keep their existing interpolation. Strict ranges and equality validation
remain in place; no epsilon clamp or donor-source change was introduced.

The new regressions cover exact f64 bits for 1, -1 and 1e200 at 1%, 1/3 and 99%; repeated
forward/backward constrained Session seeks; Track provenance; unchanged author history;
and atomic rejection of a genuinely invalid `1 + f64::EPSILON` override.

The independent reviewer retained and reran all three original scratch tests:
`constant_easy_ease_preserves_exact_value`,
`constant_easy_ease_with_exact_range_remains_seekable`, and `modes_and_extreme_absolute_times`.
All passed. The exact production fix and two added regressions received `APPROVED`;
T01-R1 received `ADDRESSED`, with no new findings.

## Recorded commands and results

Commands below are recorded observations from the implementer/reviewer reports. This documentation
transfer did not rerun Rust gates. Workspace command directory: `sim-widget-rs`.

| Owner / snapshot | Command | Result |
| --- | --- | --- |
| Implementer, initial | `cargo metadata --format-version 1 --no-deps` | exit 0; actual four-package workspace |
| Implementer, initial | `cargo build --workspace --locked` | exit 0; native build |
| Implementer, initial | `cargo build --workspace --locked --target wasm32-unknown-unknown` | exit 0; WASM build only |
| Reviewer, initial | `cargo run --locked --example cosines` | exit 0; c²=13/25/37; nine actual operations per frame |
| Reviewer, initial | `cargo test --workspace --locked` | exit 0; 18 integration tests |
| Reviewer, initial | `cargo fmt --all -- --check` | exit 0 |
| Reviewer, initial | `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| Reviewer, initial | `cargo check --workspace --locked --target wasm32-unknown-unknown` | exit 0; compilation only |
| Reviewer, initial scratch | `cargo test --offline -- --nocapture` | exit 101; two failures and one pass, T01-R1 |
| Reviewer, corrected | `cargo fmt --all -- --check` | exit 0 |
| Reviewer, corrected | `cargo test --workspace --locked` | exit 0; 20 integration tests: 8 foundation + 12 boundary |
| Reviewer, corrected | `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| Reviewer, corrected | `cargo check --workspace --locked --target wasm32-unknown-unknown` | exit 0; compilation only |
| Reviewer, unchanged scratch | `cargo test --offline --locked -- --nocapture` | exit 0; all 3 original reviewer tests |

Scratch directory: `.superpowers/sdd/sim-widget-rs/review-scratch`. Unit/doc test sets with
zero tests are not included in the 20 integration count. The cosines execution belongs to the
initial review; the corrected re-review checked the exact fix and regressions.
Initial new-API RED was a compile failure for absent APIs. Two worker self-review regressions
and T01-R1 have observed assertion RED→GREEN; no claim is made for every test individually.

## Source provenance and accepted snapshot

The reviewer compared all 48 initial snapshot hashes and both source/copied files in all 26
provenance records using SHA256: zero mismatches. The EffectCraft donor at
`67434e90388b6dca4417555346fa6850b6f96033` had a clean worktree. Copied leaves were
time, geom, color and keyframe, with licenses/NOTICE, source hashes and original manifests retained.
Four local crate manifests add `package.workspace='../..'`; a minimal four-member local donor
workspace replaces the aggregate manifest. These adaptations were explicitly reviewed.
The adapter physically uses the copied keyframe evaluator. Registry packages resolve normally
through Cargo.lock. No registry/cglibs vendoring or unused UI/GPU dependency was added;
craft-ui waits for its author UI consumer.

The final reviewer verified all 48 corrected snapshot hashes, with zero mismatches. The original
package/report snapshot keeps the pre-fix hashes; the final record overrides exactly two entries:

| Path relative to sim-widget-rs | Accepted bytes | Accepted SHA256 |
| --- | --- | --- |
| `crates/sim-runtime/src/track.rs` | 2,837 | `12b325c99b147cfb84cbb101b54370b067f271b7906fddef3e2170d148404678` |
| `tests/boundaries.rs` | 14,235 | `e2ac79ab6a77d9d82a0250ab8fd4f916158cac066c324a16f16ffbd3990c537a` |

The snapshot appendix below preserves all accepted file identities. Paths may have changed in
later tasks; these hashes identify this acceptance boundary, not the current checkout.

## Limitations and gates still open

Foundation acceptance closes Task 01 only. Browser WASM execution, GPU/native UI, renderer
pixels/picking/lifecycle, payload/latency/memory budgets and full A–N acceptance remain open.
The donor's upstream property tests were not executed as part of the root workspace suite.
Hostile command-wire decoding, schema migrations/permissions, full timeline/simulation/replay,
matrix/tensor/autodiff, editor, assets, project/export/audio, and course coverage need later gates.
Task 02 awaits independent acceptance; renderer probes are preparation evidence.
No unrelated Bun gates or pre-commit gates were run for this pure foundation review.

## Local evidence references

These supplementary files are gitignored and available only on the development machine.
Their verdicts, commands, correction and snapshot identities are preserved above/below.

- [Task brief](../../../.superpowers/sdd/sim-widget-rs/task-01-brief.json).
- [Worker report, including fix_rounds[0]](../../../.superpowers/sdd/sim-widget-rs/task-01-report.json).
- [Initial package and four diff shards](../../../.superpowers/sdd/sim-widget-rs/task-01-review-package.json).
- [Initial independent review](../../../.superpowers/sdd/sim-widget-rs/task-01-review.json).
- [Final independent re-review](../../../.superpowers/sdd/sim-widget-rs/task-01-fix1-review.json).
- [Exact reviewed fix patch](../../../.superpowers/sdd/sim-widget-rs/task-01-fix1-review.patch).

Final review JSON SHA256: `3b66f909bbbd0f7e09b624130631b775350646b120a8b1490e94b8018491023c`.
Reviewed fix patch SHA256: `234d1cc20968b2f8b60d24757192abc31b756761fd3e8257863fd14b915e60b5`.

## Accepted snapshot appendix

All paths below are relative to `sim-widget-rs`. Values are the initial 48-file manifest with
the final independent review's two corrected entries applied; source file bytes were not copied here.

| Path | Bytes | SHA256 |
| --- | --- | --- |
| `.gitignore` | 9 | `306fd52e74fca6746e12acc750f232de15e71da917756a1270d74c91f5eb7368` |
| `Cargo.lock` | 3649 | `51c147d3042797af6a4cdddd863c7336bd869ca2d6d0499f9a774bb9005fe697` |
| `Cargo.toml` | 1061 | `83722ab7b951b7e2d9390b98194e35eee84a80f3a414a302a558eef5a9361dba` |
| `crates/sim-math/Cargo.toml` | 215 | `fb9014c49d2de4fbf96e6e1aa946f8733918bdaee9269746c68c860f379eeb72` |
| `crates/sim-math/src/lib.rs` | 15223 | `b2f3367f3f484204db9454eb64975567cb55bcdcb72fff7f17c6861e58f0370c` |
| `crates/sim-model/Cargo.toml` | 227 | `0c83dc60ca8cb85869568242f322b361292ac9c7ee4306d42bf37915de01b267` |
| `crates/sim-model/src/diagnostic.rs` | 1233 | `2cd366109670397c45114eefbb524f51a9f02a2c1a2ab7281b791bc805b43924` |
| `crates/sim-model/src/document.rs` | 22562 | `b19ba8685538dab7ee373054c5f7e38c0d38f8e27c6452a4b4db62cdb5f0130f` |
| `crates/sim-model/src/lib.rs` | 154 | `d192354eda51ef5620f73ab3fe6809694b4711efb879e297cd1b00f2521e519b` |
| `crates/sim-model/src/time.rs` | 3636 | `04a6fde0a66d31b248ec1b28141063514ad03fb2bee000d5f392f62f7b3dd5d5` |
| `crates/sim-model/src/value.rs` | 8513 | `a76b0059afbbaab10cec1fb6b3bd434ee18410800b15a2b03573456902190f5d` |
| `crates/sim-runtime/Cargo.toml` | 397 | `3e29dd0554d63b686ca3577d9afebe9c342b79d846fbf3e88be5607062b66833` |
| `crates/sim-runtime/src/lib.rs` | 20642 | `c5393a113b1e05a2f9d51014c79589c66291baf262e0532a4cbd71cb3553c041` |
| `crates/sim-runtime/src/presentation.rs` | 2810 | `c97a27c3ae645a785ccdd4e30bb7e7cd058cff9c7c41f3792066caa0c18ec738` |
| `crates/sim-runtime/src/track.rs` | 2837 | `12b325c99b147cfb84cbb101b54370b067f271b7906fddef3e2170d148404678` |
| `examples/cosines.rs` | 1036 | `b7160a2980c2f815f7358c77bb13a2b56613e9a2792925d201d463bd860c2e43` |
| `provenance/NOTICE` | 924 | `a94e36238939b0b8176f207472dd8e8f68555ebcfc9947951d66971ef8c4fe1a` |
| `provenance/manifest.json` | 13060 | `09e71c3877114cf6d083abe205a6c5a92c3926f45639aaf9fb35a3aa4b319d46` |
| `provenance/original-manifests/color-Cargo.toml` | 280 | `523e20a4a6aa543b034e0ab5754eebd29e7d19b28ebb5dd6afe7468e02e6eada` |
| `provenance/original-manifests/effectcraft-Cargo.toml` | 8344 | `98ec8c61fe9067e2b7f5be880e60f16964de52742871b2f18b735984d2a855be` |
| `provenance/original-manifests/geom-Cargo.toml` | 279 | `271aca34119ce37c42b173ccac5d98ed4d89dc7a568317668b5f08a69b5d318d` |
| `provenance/original-manifests/keyframe-Cargo.toml` | 580 | `9b5099da1689f626e053d5ab3153272fb3303f250dd74932a808037abe4950f1` |
| `provenance/original-manifests/time-Cargo.toml` | 279 | `e37680a8ed8f7d74388880f2898490a3691d766ad87bacae0f461aa2ef92d1a8` |
| `rust-toolchain.toml` | 103 | `6225fcc94ef493fa5d10b39e0adb3ad6fea6c3c70d19746296370046d8866f9f` |
| `src/lib.rs` | 283 | `84060e2cda525fa21942b123409de692f754dd4ba9393390c6f4784d28785b50` |
| `tests/boundaries.rs` | 14235 | `e2ac79ab6a77d9d82a0250ab8fd4f916158cac066c324a16f16ffbd3990c537a` |
| `tests/fixtures/cosines.json` | 5468 | `622d26e310feadc9e4f38691092668aab9b7e2f912a098e688bee1ebcd6b048c` |
| `tests/foundation.rs` | 9529 | `0e86c1989f6c4b8624d9fb332fb97060d3ac55cfa912ad53df84fc10ef0fb0c2` |
| `vendor/storytold/effectcraft/Cargo.toml` | 751 | `ff659fb19a275c4f61e23f3871cef491af18a24d287e2b6278017b305d2c6e21` |
| `vendor/storytold/effectcraft/LICENSE-APACHE` | 10351 | `eb3d7b5485466acbd81f2b496f595ab637d2792e268206b27d99e793bdb67549` |
| `vendor/storytold/effectcraft/LICENSE-MIT` | 1124 | `2da01426cc958cdac55d837acf12ea2966c5b022897be58fca2f09bcc749a563` |
| `vendor/storytold/effectcraft/NOTICE` | 924 | `a94e36238939b0b8176f207472dd8e8f68555ebcfc9947951d66971ef8c4fe1a` |
| `vendor/storytold/effectcraft/crates/color/Cargo.toml` | 301 | `ed9e0ccb266b83a65dd053e7122cbc5be7cd8bc0af9e4f00e21a997f7ac3de96` |
| `vendor/storytold/effectcraft/crates/color/src/blend.rs` | 19801 | `7f4117d14794bffc83f27c1ec6bbdab3cd9e97a6040e72d5611b5b45c32c24ac` |
| `vendor/storytold/effectcraft/crates/color/src/icc.rs` | 44784 | `01ee1fc5cee9211b1c1cd7e010a33a291ea56568c58466a686201f6f185c9db1` |
| `vendor/storytold/effectcraft/crates/color/src/lib.rs` | 8987 | `49c1375c0d3b2ca15bea84e782822eb7c375ef80fbe198dc9d164a127357d148` |
| `vendor/storytold/effectcraft/crates/color/src/space.rs` | 20160 | `5885524a683e7cfd21f36633848e6b46ecd3d15b8444317dd933a524963e9662` |
| `vendor/storytold/effectcraft/crates/geom/Cargo.toml` | 300 | `d07ca4104cb611c61183ece33effbb74ae2609fe668e52c0d829206db5327878` |
| `vendor/storytold/effectcraft/crates/geom/src/lib.rs` | 19838 | `8455b489e56684dbcc5c7bde5d49328250bc090ee3efdd9017e141b5a34a0b43` |
| `vendor/storytold/effectcraft/crates/keyframe/Cargo.toml` | 601 | `234fb19d86f5ed2716d54aa84dc87ce1a3a8b9e7ffbeefa0edcb7bc2ba5a3586` |
| `vendor/storytold/effectcraft/crates/keyframe/examples/ae_ease_samples.rs` | 1872 | `5157b15041e1a48dab6dde39f06f4dc3372621a49aac29695e4d661f0927c34a` |
| `vendor/storytold/effectcraft/crates/keyframe/src/lib.rs` | 43093 | `0959cb8496768381d1af4773ae6de4dde91712df7d3e5cea9b1a462d227c0dfc` |
| `vendor/storytold/effectcraft/crates/keyframe/src/text_doc.rs` | 55067 | `cf3993eff203ebe185bb4df25d69d3819d84cbd925d21b972f433dcae6d03101` |
| `vendor/storytold/effectcraft/crates/keyframe/src/value.rs` | 18281 | `e3b372b4bc0984cf7871df9af9f58e0dfffccc7f422b619c6f53f30a366fc961` |
| `vendor/storytold/effectcraft/crates/time/Cargo.toml` | 300 | `d72406acdf5564a34623e31281249a82bcea77fbef4520b5fbb97dbcd6617b14` |
| `vendor/storytold/effectcraft/crates/time/src/lib.rs` | 33035 | `2969c02c6588e75783c55b9d768bf9535b7058212a75f817a8a92d8740e59fe6` |
| `vendor/storytold/effectcraft/crates/time/tests/relative_min_cancellation.rs` | 1576 | `b1c3d8ae46766ca1994c590f3299ce0969c35d9010ac81ee1d645039ba91333c` |
| `vendor/storytold/effectcraft/rustfmt.toml` | 47 | `6d777b994e78fcdb76b2d5968807eff5c740eab00d4193b243c8836e32ef333f` |
