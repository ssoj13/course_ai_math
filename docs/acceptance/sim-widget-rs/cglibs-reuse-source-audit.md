# cglibs reuse — source audit and integration boundary

Recorded: 2026-10-12. **Source audit only: integration/build/performance acceptance open.**
[Acceptance archive](README.md) · [Reuse plan](../../../plans/20261012-explainer-runtime/phase-03-reuse-audit.md)
· [Current ledger](../../../sim-widget-rs/docs/implementation-status.md).

The user explicitly requested fast, lightweight wgpu and named nodes-rs, wgpu-fx-rs,
wgpu-tile-atlas and wgpu-widgets-rs. The coordinator selected narrow reuse candidates below.
No dependency is added by this ruling; continue the shared picker/InputCoordinator work.

## Source identity and evidence

| Library | SSH origin | Audited immutable commit |
| --- | --- | --- |
| nodes-rs | `ssh://git@github.com/ssoj13/nodes-rs.git` | `9a92d90561c3ddb323ac27ffec94dad1b2d66df0` |
| wgpu-fx-rs | `ssh://git@github.com/ssoj13/wgpu-fx-rs.git` | `9b838175005e20f250f610c5bd2012f0ab5378ef` |
| wgpu-widgets-rs | `git@github.com:ssoj13/wgpu-widgets-rs.git` | `386303241deaa785adb67af56fb0223eed0667db` |
| wgpu-tile-atlas | `git@github.com:ssoj13/wgpu-tile-atlas.git` | `8e138c6672810da59a196ecfe7b8764d9b553c7b` |

The read-only reports inspected actual source/manifests and clean donor worktrees.
The nodes/FX audit verified fresh local/remote HEAD equality. The widgets/atlas report
checked local origin refs without a fresh network probe; the coordinator separately reports
SSH remote HEAD verification. These evidence scopes stay distinct.

| Report | Bytes | SHA256 |
| --- | --- | --- |
| [nodes/FX audit](../../../.superpowers/sdd/sim-widget-rs/reuse-nodes-fx-audit.json) | 15760 | `6edc2abdc9d3c044944a1892e394e0362f2f813be87adc76d265b4a6b4b675e4` |
| [widgets/atlas audit](../../../.superpowers/sdd/sim-widget-rs/reuse-widgets-atlas-audit.json) | 20023 | `7dfe60d4e2a68a51f832a86127a8bbe8647724e45415a63ca8cefb9361e824c1` |

Audits ran no builds, tests or performance measurements. GitNexus reported missing donor
indexes; exact-source fallback was used without index mutations. API/module presence does
not establish browser readiness or production quality.

## Selected seams and limits

| Candidate | Permitted boundary | Proof still required |
| --- | --- | --- |
| nodes-rs | Ports/types, layout and nodes-view as an editor projection of Session truth | Browser target, dependency size and public shared-pass embedding |
| wgpu-fx-rs Scene | Fixed-capacity GPU SoA buffers when a real workload needs them | Validate limits/counts before allocation; full resource/loss accounting |
| wgpu-fx-rs HUD | Public SDF shapes and caller RenderPass encoding | MSAA/text pass compatibility, propagated errors, per-frame allocation |
| wgpu-cam | Pure camera math, fit/ray/pose APIs behind one ViewState authority | Target compatibility and actual fit/input performance |
| wgpu-widgets-core | External hover/event synthesis plus public upload/draw on shared device | Clock, cancellation/touch/IME, MSAA/depth and typed Session binding |
| wgpu-text | Future screen labels through public font supply and host pass | EN/RU shaping, 11 CSS px labels, sample policy, font/atlas/payload budgets |
| wgpu-tile-atlas | Future budgeted raster-tile residency for an actual tile workload | Strict visible-workload limits and measured host/GPU memory |

**One Rust Session owns mathematical truth.** Do not run nodes cook beside sim-math or import
a second editor document/history. nodes connect does not reject cycles; its topological sort
appends cyclic leftovers. Retain our typed cycle validation. GraphRenderer convenience APIs
create/submit their own encoder and synchronous pick poll; this path is unsuitable for the
browser host's shared encoder and asynchronous picking contract.

FX contains Scene/HUD/Camera, not a general postprocessing catalog. Its HUD text renderer
uses sample1 while shapes accept configurable samples, prepare allocates scratch/label copies,
and text errors are discarded. Prove or resolve these seams through public APIs before reuse.
Scene default allocations, saturating curve capacity and release count assignment need host
admission. No gallery effect bypasses the no-Holo visual contract.

Widget InputRouter external hover fits the common stamped scene picker; keep that picker.
Public SDF/Text upload/draw APIs share host Device/Queue/RenderPass. Avoid WidgetRenderer's
own encoder/submission. Full catalog unconditionally pulls arboard3.6.1: inspected platform
exports lack browser WASM Clipboard. This is source-supported inference, not a cargo-check
failure. Core also uses std::time::Instant; its SDF pipeline is sample1/Depth32Float.
Donor glam0.33.2 and current render glam0.32 require explicit conversion or a separately
reviewed version migration. Standalone text is a future measured candidate, not adopted UI.

TileAtlas caches externally supplied raster tiles; it is not font packing, geometry/BVH or
a renderer. Visible/current-frame protection can exceed nominal budgets, and zero means
unlimited. Host admission must bound the actual workload and late/epoch invalidation.

## Fast/light contract and dependency policy

Demand redraw, dirty-only work/BVH refit, coalesced pointer moves and reusable buffers must
be implemented and measured. No synchronous GPU readback or poll enters the browser input
path. Measure p95 frame/input/query latency, peak resident CPU/GPU memory and the coherent
WASM payload, including transient/cache/font/resolve allocations.

The AA preview remains **323366 B gzip > 300000 B**; source audits grant no waiver.
No speed, FPS or final memory claim follows from retained storage, native tests or a gallery.
Choose a package only after its public seam, browser build and measured incremental budget
satisfy the applicable full gates.

Our libraries use SSH Git immutable refs and audited consumer Cargo.lock transitive commits.
Only selected StoryTold closure is physically copied. Do not copy cglibs source, patch a
private cache, edit upstream or use a native host's submission path. Source license metadata/
text gaps recorded by the audits remain to resolve before distribution.
Full Tasks03/04/18 remain open.
