# sim-widget-rs — static UI reference source handoff

Recorded: 2026-10-12. Density iteration 2 verdict: **PASS_WITH_LIMITATIONS for frozen source guidance**.
This handoff covers the static reference and its design contract. Working runtime controls,
animation and independently rendered UI acceptance remain unverified.
[Widget README](../../../sim-widget-rs/README.md) · [Execution ledger](../../../sim-widget-rs/docs/implementation-status.md).

## Contents

- [Durable artifacts](#durable-artifacts)
- [User authority and dimensions](#user-authority-and-dimensions)
- [Independent source review](#independent-source-review)
- [Runtime implementation handoff](#runtime-implementation-handoff)

## Durable artifacts

The designer copied the reviewed snapshot into the widget directory. The documentation owner
read each SHA256 with `Get-FileHash -Algorithm SHA256` after the copy was confirmed.
Both match the frozen density review exactly.

| Artifact | Bytes | SHA256 |
| --- | --- | --- |
| [Static reference HTML](../../../sim-widget-rs/design/reference.html) | 66,550 | `65cc407cae90a233e3e648600aa80a70305a73291011bf77be1669b119d31a01` |
| [UI visual contract JSON](../../../sim-widget-rs/design/ui-visual-contract.json) | 28,632 | `243c011861116e56394d15381272cd86af8bf7690aff04ad191341e8d17c1365` |

These file identities define the reviewed handoff. Later changes require their own scoped review.
The HTML and JSON were not modified by the documentation owner.

## User authority and dimensions

Scope: the new widget and copied cosines pilot; existing lessons/covers retain their separate
migration scope. The user requested no Holo, clear smooth visualization and pleasant UI.
After manually viewing the reference, the user said: «выглядит хорошо, шрифт меньше сделай.
там что-то типа 14 сейчас, надо 11», then requested smaller vertical gaps and compactness.
The positive screenshot/reference feedback records user direction; it does not establish
independent rendered review, runtime completeness or a test pass.

Keep **11 CSS px** for the main desktop UI and ordinary scene labels. Labels retain CSS pixel
size through geometric zoom. Preserve semantic readability, restrained heading hierarchy and
all content/routes across embedded, learner and author shells.

| Fine-pointer layout part | Compact source specification |
| --- | --- |
| Topbar | 36 px minimum; grows naturally when wrapping |
| Pane header | 28 px minimum |
| Tree/property/timeline rows | 24 px; numeric rows 26 px |
| Numeric input envelope | 24 px; native input inside is 22 px |
| Pane padding / group gaps | 8 px / 8 px; first section heading has no extra top margin |
| Pointer commands | Typically 24–28 px; reference uses 26 px |
| Touch/coarse-pointer commands | Hit areas at least 44 px; numeric envelope/row 46 px around 44 px input |

Touch/coarse-pointer policy enlarges control boxes independently of desktop glyph sizes.
The reference also applies its coarse/mobile tokens at widths ≤720 px. Runtime shells must
declare the input policy explicitly and preserve focus, scrolling and every command's reachability.
No library, resources, compositions or add-object route may disappear as layout becomes compact.

## Independent source review

Designer: `gpt-6.1-sol high`. Independent reviewer: `gpt-6-astra high`.
Exact dispatch identities were coordinator-supplied; the reviewer did not introspect model identity.

| Historical review | Verdict and scope |
| --- | --- |
| Initial reference | NEEDS_REVISION; D1–D5 requested changes |
| Frozen fix1 | NEEDS_REVISION; D1 layering/label clearance and D5 library/add reachability remained partial |
| Frozen density iteration 2 | PASS_WITH_LIMITATIONS; F1–F3 and compact-density source specifications resolved, no new findings |

Density review independently recomputed snapshot/baseline hashes, read the full 78,039-character
two-file patch and reconstructed CSS/SVG geometry with independent arithmetic.
Its **1,067 source geometry cases** found no checked plate/side/edge collisions, including
the former 721 px author/lab failures and the actual 500 px container-query transition.
Minimum conservative side-stroke clearance was about 2.648 px; figure-edge inset about 6.36 px.
These are source calculations for four principal value plates, triangle strokes and figure edges.
They do not prove browser font metrics, arbitrary values, every label collision or runtime fitting.

| Finding | Resolved source contract |
| --- | --- |
| F1 / D1 layering | View controls occupy a normal-flow row before the figure; explicit figure/label/handle/chrome layering preserves that reservation |
| F2 / D1 label clearance | Actual scene container ≤500 px selects the narrow composition; fixed CSS-pixel labels and revised anchors clear checked triangle sides across transitions |
| F3 / D5 reachability | Compact Objects disclosure contains Library/Resources/Compositions/Add as well as the object rows |

Aligned timeline columns retain a shared 22 px ruler and 6 px top padding: row pitch is 24 px
fine or 44 px coarse. Their centers were checked by source box arithmetic. Responsive reflow,
font wrapping, focus visibility and perceptual smoothness require rendered evidence.

## Runtime implementation handoff

Use the durable reference and contract for renderer/input/shell implementation. Preserve separate
toolbar reservations, label clearance by actual figure width, compact library/add routes,
11 px desktop labels and explicit pointer/coarse density. Implement real controls, animation,
runtime bindings, diagnostics, lifecycle and accessibility through the planned task owners.
The static cosine figure keeps a=3, b=4, θ=60°, c²=13; this source review ran no runtime evaluator.

Browser, rendered desktop/mobile, screenshots, interaction, performance and runtime math checks
are **NOT_RUN** in the independent design review. The binding CUA file-protocol block was
preserved; no alternative rendering or browser workaround was used. User screenshots from
an earlier iteration were not reviewer evidence for this frozen density snapshot.
Full UI/18-task acceptance remains open. Runtime correction round 2 likewise awaits independent
acceptance; this source handoff does not release gated runtime integration.

Supplementary local records are gitignored; the identities, verdicts and limitations above remain
durable project documentation:

- [Initial source review](../../../.superpowers/sdd/sim-widget-rs/ui-design-review.json).
- [Frozen fix1 source review](../../../.superpowers/sdd/sim-widget-rs/ui-design-fix1-review.json).
- [Density source review](../../../.superpowers/sdd/sim-widget-rs/ui-design-density-review.json).
