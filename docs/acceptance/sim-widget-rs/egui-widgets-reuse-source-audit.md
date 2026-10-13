# egui-widgets-rs — полный source audit переиспользования

Дата: 2026-10-12. **51 entries: 50 libraries + 1 gallery.**
[Acceptance index](README.md) · [Reuse plan](../../../plans/20261012-explainer-runtime/phase-03-reuse-audit.md)
· [Ledger](../../../sim-widget-rs/docs/implementation-status.md).
Source features independently supported; REUSE-R1 audit correction independently approved.
Build/runtime/performance или dependency adoption из этого не следуют.

## Snapshot и доказательства

SSH: `ssh://git@github.com/ssoj13/egui-widgets-rs.git`;
immutable rev `96fc02ff58f30f3ea8a39193c39f39004a7835b7`. Donor clean до/после read-only audit.
Реальный lock: egui0.36.2, wgpu30.0.1, glam0.33.12; workspace MSRV1.96.
Current render glam0.32 требует checked array/type adapter либо отдельно reviewed migration.
egui Context/input/fonts/renderer ещё требуют host integration. Новая donor HEAD не меняет
этот audited snapshot; обновлять dependency pin следует отдельным reviewed решением.

| Receipt | Bytes | SHA256 |
| --- | --- | --- |
| [Original catalog](../../../.superpowers/sdd/sim-widget-rs/reuse-egui-widgets-catalog-audit.json) | 282316 | `9f06dec3832f2bca05ee62910ea90e4ffdb8d24ce0a5d249de1bb6527d70d576` |
| [Current catalog fix1](../../../.superpowers/sdd/sim-widget-rs/reuse-egui-widgets-catalog-audit-fix1.json) | 285364 | `32fde23b2a22538aabc08abfad39e316959214a9ecdef98126bd271006688ab9` |
| [Подробный gizmo audit](../../../.superpowers/sdd/sim-widget-rs/task-03-egui-gizmo-reuse-audit.json) | 14907 | `8a25d3c43cee9eb6b6e182d416c9a89caebd575883a1aaff04541bfca9149749` |

Catalog audit прочитал109 source files, перечислил125 Rust paths и все51 workspace entries;
missing members нет. reference/_ref, target и gallery execution не принимались за proof.
JSON сохраняет точные paths/line symbols, manifests/features/licenses и53 source identities.
Conservative lock closure не является activated cargo-tree target/feature proof.
Build/tests/browser/GPU/accessibility/IME/gzip/memory/p95 здесь **NOT_RUN**.

## Independent cross-check — source supported, audit correction approved

[Source cross-check](../../../.superpowers/sdd/sim-widget-rs/reuse-egui-widgets-crosscheck-review.json)
returned FINDINGS, configured `gpt-6-astra high`;13,584 B, SHA256
`1bb7a3b23a6ad3fc6892061408d1317b9325ce960cbacb705b593d85e013fb73`.
Verified51-member inventory/direct normal dependencies,698 source citations and124 Rust source
hashes support the feature/gizmo conclusions. Current HEAD moved to docs-only a830c26;
original source/manifest identities still match audited96fc pin.

REUSE-R1, medium: three closure lists (display/viewport-toolbar/demo) treated `gpu-mem`
alias as package identity. Actual package is `gpu-info-rs0.1.0`, SSH commit
`5d345b3bc2a44e1ff2bcbbcfe2f9d89b9a66568c`; display/toolbar also omit pollster0.4.0.
Original282316 B audit is preserved. Current285364 B fix1 corrects these three lists,
retaining all51 source capability entries and correction history. Independent
[fix1 review](../../../.superpowers/sdd/sim-widget-rs/reuse-egui-widgets-catalog-audit-fix1-review.json)
APPROVED_FOR_REUSE_R1_CORRECTION closes REUSE-R1 for audit accuracy only: exactly7 JSON paths
(three lists, three closure_basis notes, correction history) changed; original9f receipt unchanged.
Fix review4,007 B SHA256 `40004443d6150b7f129dcdadefd1103e9ff50860243253c16c17ea865a2ba8e5`.
Original lists remain historical; activated target-feature/resource/runtime proof is still required.
This finding does not invalidate implemented feature inventory or authorize runtime adoption.

## Приоритет reuse и архитектура

Author inspector: attr-grid/slider/colorpicker/value-editor; timeline/keyframes/track-timeline/
ramp; outliner; command-palette/tab-menu/menubar; config/splitter/status/titlebar/layout/toast/help.
«Кандидат» означает пригодный source boundary после gates, не немедленную dependency adoption.

Widgets — projection/proposals: Session остаётся единственным evaluator/truth,
InputCoordinator — gesture/history, общий stamped picker — hover/press, ViewState — camera.
Actions/snapshot edits проходят typed checked commands. CPU widgets требуют настоящего
egui Ui/Context, GPU widgets — shared host Device/Queue/renderer/encoder/pass graph.
Новый eframe window/GPU owner не добавляется.

Выбирать packages по SSH pin и consumer transitive lock; наши source не копировать.
Author/learner WASM closures измерять отдельно, gallery/workspace целиком не загружать.
DOM/KaTeX accessibility shell остаётся measured выбором. Internal English menus/hints/errors
требуют полного EN/RU public string/host adapter. 11 CSS px, compact density, touch≥44
и no-Holo сохраняются; generic HUD stats не означают decorative Holo.

## Все 51 entries

«Кандидат» — узкий reusable source; «Adapter» — host model/time/input/commands;
«Public seam» — текущего API недостаточно для общей архитектуры.
Optional/dev/reference entries не сокращают полный professional scope.

| № | Package | Назначение | Реально реализованный reuse | Browser/host ограничения |
| --- | --- | --- | --- | --- |
| 1 | `egui-widgets-config` | Tokens/icons/segmented controls | AttrMetrics/layout/reset/state helpers | Кандидат; font один раз, Phosphor SSH/cache budget |
| 2 | `egui-value-editor` | Vector/quaternion/matrix/text editor | Working copy→Apply result | Adapter; checked narrowing; arrays read-only, не tensor/code editor |
| 3 | `egui-timeline` | Indexed scrubber | Seek/ticks UI | Adapter usize→TimeTicks; playback у Session |
| 4 | `egui-statusbar` | Status sections | Resizable clipped closures | Кандидат; widths — view state, host locale |
| 5 | `egui-menubar` | Menus | Opaque IDs/enabled/shortcuts/labels | Кандидат; общий command registry |
| 6 | `egui-splitter-table` | Resizable table | Virtualized visible-row body | Кандидат; large inspector с row caps |
| 7 | `egui-attr-grid` | Property inspector | Vec/Mat/lists/reset/random/preset/animation intents | Adapter; f32 conversion, snapshot/commands; не ndarray/strides |
| 8 | `egui-colorpicker` | HDR colorpicker | RGBA/HSV/hex/history/eyedropper hook | Adapter; host linear/display/color truth |
| 9 | `egui-prefs` | Staged preferences | Apply/OK/Cancel, closure registry | Adapter; prefs2/box-rs/attr-grid closure |
| 10 | `egui-prefs2` | Schema preferences | RustBox pane/state callbacks | Adapter; UI view model, не Session/project/persistence |
| 11 | `egui-progressbar` | Progress | Painted current/total | Кандидат; host task state |
| 12 | `egui-help-overlay` | Shortcut help | Display sections/entries | Кандидат; localized strings/bindings |
| 13 | `egui-logbuffer` | Logs/tracing | Bounded store/panel | Adapter; cap lines, snapshot clones/mutex |
| 14 | `egui-drag-overlay` | Drag ghost | Typed payload/cross-panel overlay | Adapter; drop/cancel/generation у host |
| 15 | `egui-gpu-view` | GPU texture view | Shared RenderState/device/renderer bridge | Adapter; invalidate забывает ID без free; retirement/resize proof |
| 16 | `egui-hdr-view` | HDR presenter | RGBA formats/host color transform | Adapter; exposure/gamma/tonemap fields — inert stubs |
| 17 | `egui-display` | Display/capture/export | Shared PresentPass/backdrop/capture UI | Adapter; native ffmpeg/process/fs отдельно от browser display |
| 18 | `egui-scopes` | GPU color scopes | Histogram/waveform/vector/parade/false-color | Optional author tool; WebGPU formats/shaders/resources NOT_RUN |
| 19 | `egui-camera-controls` | Camera intents | Orbit/fly controls | Adapter в ViewState; Houdini arbitration/wheel policy |
| 20 | `egui-viewport` | 2D image viewport | Pan/zoom/fit/channel/readout | Adapter; host texture, без второго camera/picker |
| 21 | `egui-viewport-overlays` | Geometry overlays | Axes/grid/bbox/frustum/reticle CPU painter | Adapter; f64 row-vector matrices→наш reversed-Z |
| 22 | `egui-viewport-hud` | Viewport stats | Corner panels/HudExtents | Кандидат; semantic stats/locale, без decorative Holo |
| 23 | `egui-viewport-toolbar` | Viewport toolbar | Collapsible controls/claimed rect | Adapter; pointer occupancy; egui-display/export closure |
| 24 | `egui-track-timeline` | Multi-track timeline | Clips/keys move/trim/slide actions | Adapter; host model/history/time, pattern cache cap |
| 25 | `egui-pattern` | Pattern fills | Raster tile→quad, три primitives | Кандидат; axis≤1024, per-context texture/DPR cap |
| 26 | `egui-keyframes` | Inline keyframes | Time/value/add/move/delete intents | Adapter; f32 tracks/diamonds; tangent graph даёт ramp |
| 27 | `egui-hotkeys` | Scoped hotkeys | Resolve/consume/copy-paste routing | Adapter; InputCoordinator priority, browser clipboard bridge |
| 28 | `egui-nodes-view` | Node graph | nodes-egui/SubnetHost snapshot | Public seam; projection-only/shared pick/pass, heavy/native closure |
| 29 | `egui-otio-timeline` | OTIO NLE | otio_edit/timeline re-exports | Video niche; чужие undo/model не заменяют Session |
| 30 | `egui-asset-browser` | Asset browser | List/thumbs→Open/Create/Select/Rename/Delete/Drag | Adapter; thumbnails/import/pickers у host |
| 31 | `egui-outliner` | Outliner | Multicolumn tree→selection/context/open/sort/visibility | Adapter; depth guard, обычная projection tree |
| 32 | `egui-arbor` | Arbor tree | Trait-based DnD/actions | Adapter; event_log SystemTime отдельно, не blanket tree ban |
| 33 | `egui-layout-manager` | Layout manager | Named opaque layout blobs | Кандидат; view-state store/restore у host |
| 34 | `egui-dock-layout` | Dock layout | egui_dock persistence/toolbar | Adapter egui_dock0.21; clear NaN rects→JSON; wrapper |
| 35 | `egui-preset` | Presets | Generic store/Recall(weight) | Adapter; host capture/apply/blend checked commands |
| 36 | `egui-jobs-table` | Jobs table | Sortable/filterable borrowed rows→actions | Кандидат; не запускает jobs; queue/row caps |
| 37 | `egui-encode-dialog` | Encode dialog | Schema/settings→Start/Browse | Adapter; нет встроенных codecs/process/rfd; browser download у host |
| 38 | `egui-gizmo` | Transform gizmo | T/R/S/Universal/Box/Shear/CornerPin/arcball | Public neutral pick/draw/cancel/clip seam; готовые tools reuse |
| 39 | `egui-ramp` | Curve/gradient editor | f32/f64 Tracks/tangents/EditPhase | Adapter в authored tracks; curves-rs SSH, не второй evaluator |
| 40 | `egui-slider` | Numeric slider | egui Slider/reset/soft range/value box | Кандидат; changed→checked command/accessibility |
| 41 | `egui-file-field` | File field | Browse request/remembered directory | Adapter; PathBuf UI не открывает fs, browser identity у host |
| 42 | `egui-font-picker` | Font picker | Host family list/generic names | Кандидат; не OS enumeration; actual renderer fonts |
| 43 | `egui-spinner` | Spinner | Busy-time repaint UI | Кандидат; demand redraw когда нужен |
| 44 | `egui-toast` | Toasts | Caller stack/frame TTL | Кандидат; cap queue/localized text |
| 45 | `egui-image-clip` | Image clip | Native persistent image store/picker | Public storage/clock adapter; default fs/SystemTime/EXR; не OS clipboard |
| 46 | `egui-titlebar` | Titlebar | Collapsible tinted section/closures | Кандидат; view-only open state |
| 47 | `egui-charts` | Charts | Stacked OHLC mip/line/histogram | Adapter для samples; LOD cap; не tensor/function editor |
| 48 | `egui-command-palette` | Command palette | Fuzzy query→opaque command ID | Кандидат; host actions/locale, ranking caps/p95 |
| 49 | `egui-tab-menu` | Houdini Tab menu | Cascade/search/history/favorites/usage | Кандидат; host action tree, Phosphor closure |
| 50 | `egui-puffin` | Puffin profiler | Flamegraph UI | Dev-only; web_time есть, не blanket native blocker |
| 51 | `egui-widgets-demo` | Native gallery | Examples всех widgets | Reference only; eframe own window/GPU/full closure |

## Gizmo — готовые tools через публичный seam

`Gizmo::interact(&Ui, &[Transform])` реализует Move/Rotate/Scale/Universal/Box2D/3D,
Shear, CornerPin2D/arcball, axes/planes/screen/free, snap и multi-target math.
Global/Local открыты; Parent/custom basis/pivot из prose sketch не являются implemented facade.
Shear/corner-pin требуют host storage. Переиспользовать готовый kernel предпочтительнее
повторного написания rotate/scale ради обхода private API.

Сейчас private kernel/draw data/subgizmo picker; нет external typed handle selection/filter
или begin/end/cancel seam. clear_hover не очищает active gesture; raw MMB translation
конкурирует с Houdini navigation. Donor NDC near=-1/far=+1 отличается от нашего RH reversed-Z
near1/far0. f32 facade требует checked narrowing из canonical f64. TRS decomposition может
потерять affine shear при rotated nonuniform scale.

Предпочтительный **планируемый** public boundary: stable handles/geometry/draw data,
host coherent ray/viewport/DPR/display stamp, external candidates/activation и bounded
start/update/end/cancel. Egui facade и DOM host используют один donor kernel.
Нужны отдельный authorized donor task, independent review и новый immutable pin:
этот audit не реализует seam, не разрешает private access/cache patch/source copy.
Shared egui-wgpu pass trial также требует реального input/clip/cancel/resource proof.

## Что не найдено и что проверить

Dedicated arbitrary tensor/shape/strides inspector не найден: fixed matrices/list UI его не заменяют.
Нет dedicated syntax-highlight code/language-service/formula editor или BSDF material editor;
generic attr/nodes/color дают часть chrome. Required teaching capabilities остаются в scope.

Source stubs указаны в таблице; public field не доказывает эффект. Fonts/egui atlas/pattern
textures/chart pyramids/log clones/action strings/HDR/scopes/MSAA/resolve/offscreen resources
требуют caps, measured peaks и device-generation retirement. Нужны selected-package target/
feature proof, GPU lifecycle, EN/RU/IME/accessibility/touch/DPR1/1.25/2, p95 frame/input/query,
peak CPU/GPU и coherent gzip. Renderer≤300000 B, other chunks≤100000 B,
per-task increment≤18000 B остаются gates. Source audit не даёт waiver и не закрывает Tasks03/04/18.
