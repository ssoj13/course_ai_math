# Task 03 — isolated compiled AA preview

Recorded: 2026-10-12 (local). Verdict: **APPROVED_FOR_AA_PREVIEW**, bounded functional/provenance scope.
[Acceptance index](README.md) · [Current ledger](../../../sim-widget-rs/docs/implementation-status.md).
This accepts the exact compiled preview below; full Task 03/04/18 and visual-quality gates remain open.

## Authority and provenance

Coordinator explicitly allowed a scratch compiled AA trial while product ownership was frozen.
It uses a copy of accepted [Task 02 v1 core](task-02-runtime.md), not the then-unreviewed V2 patch.
The [source manifest](../../../.superpowers/sdd/sim-widget-rs/task-03-aa-preview/source-provenance.json)
records 36 baseline paths plus 24 renderer/trial additions. Of the baseline, 33 hashes match;
the three authorized exceptions are renderer workspace registration, frozen Cargo.lock dependencies
and model scene-module export. Scratch-only collapse/empty-index fixes do not accept production recovery.

Original renderer implementer: `gpt-6.1-sol high` (coordinator dispatch).
Independent reviewer: configured `gpt-6-astra high`, `/root/sim_atomic_patch_review`.
The [independent AA review](../../../.superpowers/sdd/sim-widget-rs/task-03-aa-preview-review.json)
SHA256 is `632c1da6ad4a729a3e764b4251d24ce0b7903c52175540b02cbd6567b9828bf7`.
It matched all **60 source hashes and three artifact hashes**, reviewed source and operated its
own prescribed CUA Chrome tab `1696998553`; it did not rebuild or alter product/other tabs.
The user separately received tab `1696998539`.

## Exact deliverable

[Open compiled AA preview](http://127.0.0.1:43874/cosines-aa.html), localhost PID118580.
The [artifact manifest](../../../.superpowers/sdd/sim-widget-rs/task-03-aa-preview/artifacts.json)
identifies the coherent HTML/WASM/JS set:

| Artifact | Raw / gzip bytes | SHA256 |
| --- | --- | --- |
| cosines-aa.html | 12,919 / 4,561 | `2dd9100b8c7923028c71d9c15960995fe07575be635750aa7f896015dd780fa8` |
| pkg-aa/sim_render_trial_bg.wasm | 1,042,729 / 323,366 | `45e1f62d0215b005f3cd5d0a0dbd279de1ef265934d5ada2f62df627b73d59b2` |
| pkg-aa/sim_render_trial.js | 75,004 / 13,639 | `2c5dafa47fc2991a3e4ba2a3aed0d74b49b497e99e481f78e117707d0847ff7b` |

Renderer validates X4 support for Bgra8UnormSrgb and Depth32Float. Relevant pipelines/color/depth
use sample4 with resolve to a single-sample surface. Owned capsule SDF coverage supplies rounded
stroke caps and zero-length points; public BSDF stays unchanged. Browser initialized, submitted
draws and returned frame receipts without application/GPU errors.

## Independent functional checks

All **13 actual browser cases passed**, with numeric tolerance 1e-10:

- Initial 3/4/60 and 90/120 presets produce displayed c²13/25/37.
- a=4 yields c²16, synchronized formula/labels/ranges; 2D→3D→2D preserves values/time/revision.
- Theta0 and theta180 produce c²1/49 and recover through reset to c²13.
- Playback starts and completes at theta120/c²37; actual slider Home/End seeks to 60/120
  with monotonic revisions. All statuses ready; no page/GPU errors.

These are real Session-derived preview receipts. Pure playback-intent changes may leave the last
rendered receipt revision; this receipt is not a complete audit stream.
The six unrelated Chrome-extension warnings were classified separately, not hidden as application errors.

## Remaining limits

WASM **323,366 B gzip exceeds 300,000 B by 23,366 B**; no waiver. This preview has no direct
manipulation/orbit handlers and incomplete global locale. Common picking/InputCoordinator,
touch/mobile/responsive layout, occlusion, performance and full renderer scope remain unaccepted.

The reviewer attempted one screenshot via aaTab.getScreenshot(); Page.captureScreenshot timed
out at 5 s, with no retry or policy workaround. No rendered/pixel UI-quality acceptance follows.
Actual browser MSAA edge readback, full DPR/fractional-motion/multiview/resize/loss and complete
preallocation resource union remain unverified.

Existing six-case native numeric AA evidence separately covers DPR1/1.25/2 × MSAA1/4.
The [native attestation](../../../.superpowers/sdd/sim-widget-rs/task-03-aa-native-attestation.json)
links receipt SHA256 `4dd7fef2c4a6ecfb1cbf285c2b95fda62aa78f23ac2f44198fdc7aae3cfff44b`.
The AA preview reviewer read that evidence but did not rerun it or promote it to browser pixel proof;
the prior numeric review lacked a verified model ID. Its different core linkage is explicit.

Static-reference browser policy stays unchanged: this exception is the actual compiled trial.
The accepted [atomic override prerequisite](task-04-atomic-override-prerequisite.md) and
shared picking design must still be integrated into a newly frozen coherent artifact before
direct manipulation is claimed. No product code, tests, builds or commits were changed by this
documentation handoff.
