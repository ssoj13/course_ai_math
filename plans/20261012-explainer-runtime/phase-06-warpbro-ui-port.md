# Phase 06 — перенос полного Warpbro UI и подключение нашего wgpu viewport

Дата: 2026-10-13. Статус: **архитектурный план APPROVED_FOR_IMPLEMENTATION**;
оригинальный `/root/sim_renderer_impl` начинает WP-0 как единственный product writer.
Первая raw product copy выполнена после approved preflight; normalized library,
работающий browser shell и полная версия ещё не приняты.

[Общий план](plan.md) · [Reuse](phase-03-reuse-audit.md) ·
[Все 18 задач](phase-04-implementation.md) · [Приёмка](phase-05-acceptance.md) ·
[Фактический прогресс](../../sim-widget-rs/docs/implementation-status.md).

## 1. Основание и границы принятого решения

Пользователь требует весь профессиональный UI Warpbro с нашим wgpu viewport.
Последняя sourcing-инструкция: «мы КОПИРУЕМ крейты из warpbro и других проектов».
Выбранные собственные crates и необходимая own-project dependency closure физически
копируются в `sim-widget-rs`, включая egui-widgets/gizmo и другие cglibs.
Прежние SSH-only и временное Warpbro-UI-only исключение отменены.
**crates.io не вендорится**: registry versions, features, Cargo.lock/checksums сохраняются.

Точный [машиночитаемый план](../../.superpowers/sdd/sim-widget-rs/warpbro-ui-port-plan.json):
156719 B, SHA256 `fb870f581883058edcb76b7367184096e290b5cc9808bfea3a57902e13bc572e`.
[Независимый review](../../.superpowers/sdd/sim-widget-rs/warpbro-ui-port-plan-review.json):
13041 B, SHA256 `f1969d65e7cf6492e0f03598d16dbf195ec4d9a49a1dab87704e566a90d95acc`,
configured `gpt-6-astra high`, APPROVED_FOR_IMPLEMENTATION, findings пусты.
Это принятие архитектуры и порядка работ с обязательным preflight перед копированием.
Оно не доказывает runtime, build новой копии, browser UI или выполнение всех 18 задач.

Полный featureset сохраняется: author editor, full-window lab, embedded learner,
curve/node tools, форматы и export, все учебные сцены. Косинусы — изолированный пилот;
действующие уроки и пользовательское состояние не мигрируются этой работой.

## 2. Frozen source и состав копии

| Источник | Точный snapshot | Что сохраняем |
| --- | --- | --- |
| Warpbro | `853590699cd220532a15063442fa94e8e3c49794` | 78 tracked files проверены как reference; 46 точных UI slices из 12 source files |
| egui-widgets-rs | `96fc02ff58f30f3ea8a39193c39f39004a7835b7` | Все 50 library crates, 226 inventoried files; gallery/demo не shipping entry |
| Warpbro прежний egui baseline | `25313cb886448f1fe626c28bd0b97eb3a45f2a1a` | Только compatibility reference; actual copied call sites проверяются против96fc |

Независимый reviewer подтвердил 78/78 Warpbro hashes, 226/226 egui hashes и
46/46 byte slices. [Source receipt](../../.superpowers/sdd/sim-widget-rs/warpbro-ui-port-plan-review-source-check.json)
SHA256 `9589e14645c9199ccce49dd2b84f87f909e777bdfec5de012608a4760685c853`.
29 исключённых tracked files находятся только в `egui-gizmo/_ref`: reference snapshots
и cache metadata, не executable dependencies. Их исключение не удаляет рабочий kernel.

Bytes получать через `git show <commit>:<path>`, не из движущегося working tree.
У Warpbro зафиксированы чужие concurrent dirty changes. Их hashes — отдельное наблюдение,
не copy inputs; не stash/reset/revert и не смешивать snapshots. Смена baseline требует
нового coherent inventory и review delta.

Срезы сохраняются как pristine provenance; их точные boundaries не являются обещанием,
что смешанный native `App` сам соберётся как portable crate. Compiled UI обобщается
на typed host projections и intents с сохранением каждого control/action.

### Полный реестр 46 source slices

Inclusive 1-indexed строки оригинальных bytes; hash каждого slice и whole source blob
находится в машиночитаемом плане. Пересечения interval, например OCIO declarations,
deduplicate в физическом receipt.

| Source | Строки | Поведение/символ |
| --- | --- | --- |
| `src/app.rs` | 31–231 | App/UI state declaration reference |
| `src/app.rs` | 349–536 | SettingsPage/Controls/Settings declarations |
| `src/app.rs` | 4410–4437 | ViewTool/nav defaults |
| `src/app.rs` | 537–542 | f_row |
| `src/app.rs` | 543–547 | b_row |
| `src/app.rs` | 548–589 | prefs_grid |
| `src/app.rs` | 829–1012 | settings_ui |
| `src/app.rs` | 1013–1082 | draw_overlays |
| `src/app.rs` | 1083–1247 | manipulate |
| `src/app.rs` | 1872–1891 | assign_menu |
| `src/app.rs` | 1900–2089 | materials_tab |
| `src/app.rs` | 2111–2206 | material_library |
| `src/app.rs` | 2236–2273 | snapshot_menu |
| `src/app.rs` | 2308–2328 | window_shot_menu |
| `src/app.rs` | 2764–2857 | export_ui |
| `src/app.rs` | 2858–2982 | fly_camera |
| `src/app.rs` | 2983–3063 | orbit_camera |
| `src/app.rs` | 3139–3318 | top_bar |
| `src/app.rs` | 3319–3424 | browser |
| `src/app.rs` | 3425–3465 | inspector |
| `src/app.rs` | 3466–3483 | status_bar |
| `src/app.rs` | 3484–3568 | resizable_status_bar |
| `src/app.rs` | 3569–3629 | fixed_status_bar |
| `src/app.rs` | 3630–3708 | viewport_click |
| `src/app.rs` | 3758–3901 | viewport |
| `src/app.rs` | 4233–4402 | ui |
| `src/dock.rs` | 1–740 | module non-test body |
| `src/world_ui.rs` | 1–3382 | module non-test body |
| `src/takes_ui.rs` | 1–847 | module non-test body |
| `src/render_profiles_ui.rs` | 1–965 | module non-test body |
| `src/hotkeys.rs` | 1–518 | module non-test body |
| `src/camera_slots.rs` | 1–110 | module non-test body |
| `src/export.rs` | 650–773 | ui |
| `src/export.rs` | 774–809 | fps_row |
| `src/export.rs` | 810–823 | quality_row |
| `src/export.rs` | 824–892 | output_transform_ui |
| `src/ocio.rs` | 54–72 | Sel UI DTO |
| `src/ocio.rs` | 982–1037 | ColourPreset/ColourPresets/PresetEdit declarations |
| `src/ocio.rs` | 841–882 | quick_view_ui |
| `src/ocio.rs` | 883–993 | ui |
| `src/ocio.rs` | 1063–1153 | ui |
| `src/ocio.rs` | 1154–1177 | config_combo |
| `src/ocio.rs` | 1178–1194 | combo |
| `src/window.rs` | 85–91 | style bootstrap |
| `src/material_gallery.rs` | 1–213 | module non-test body |
| `src/file_dialogs.rs` | 1–71 | module non-test body |

### Размещение

```text
sim-widget-rs/
  crates/sim-editor-ui/
    lib.rs, projection.rs, intents.rs, shell.rs, dock.rs, chrome.rs
    preferences.rs, color_ui.rs, camera_slots.rs, hotkeys.rs, accessibility.rs
    panels/{outliner,inspector,timeline,takes,catalog,materials,render_profiles,export}.rs
  vendor/<project>/crates/<package>/    # полные выбранные own crates + closure
  provenance/warpbro/
    manifest.json, NOTICE, source-slices/, patches/
  apps/sim-web/                        # browser entry/platform/input adapter
  crates/sim-render/                   # single GpuHost/pass/cache
  crates/sim-author/                   # author projection/controller, graph/curves
  crates/sim-project/                  # packages/storage/assets/export
```

Добавлять workspace member одновременно с работающим API/registry/rendering, без пустых
пакетов, `todo!`, panic placeholders и blank panel closures. Исходные mixed App/WorldUi
сохраняются как uncompiled reference; новые модули получают original/target hashes и patch log.
Own dependency identities нормализуются через local paths/root patch sections, без duplicate
версий одного собственного package.

### Все 50 библиотек toolkit

Физическая доступность всех50 не означает eager linking/loading всех50 в learner.
Активная feature closure и отдельные author/learner artifacts проверяются самостоятельно.

`egui-widgets-config`, `egui-value-editor`, `egui-timeline`, `egui-statusbar`, `egui-menubar`, `egui-splitter-table`, `egui-attr-grid`, `egui-colorpicker`, `egui-prefs`, `egui-prefs2`, `egui-progressbar`, `egui-help-overlay`, `egui-logbuffer`, `egui-drag-overlay`, `egui-gpu-view`, `egui-hdr-view`, `egui-display`, `egui-scopes`, `egui-camera-controls`, `egui-viewport`, `egui-viewport-overlays`, `egui-viewport-hud`, `egui-viewport-toolbar`, `egui-track-timeline`, `egui-pattern`, `egui-keyframes`, `egui-hotkeys`, `egui-nodes-view`, `egui-otio-timeline`, `egui-asset-browser`, `egui-outliner`, `egui-arbor`, `egui-layout-manager`, `egui-dock-layout`, `egui-preset`, `egui-jobs-table`, `egui-encode-dialog`, `egui-gizmo`, `egui-ramp`, `egui-slider`, `egui-file-field`, `egui-font-picker`, `egui-spinner`, `egui-toast`, `egui-image-clip`, `egui-titlebar`, `egui-charts`, `egui-command-palette`, `egui-tab-menu`, `egui-puffin`.

## 3. Обязательный WP-0 preflight — до product copy

1. Для **каждого activated own transitive crate** рекурсивно разобрать Cargo manifests:
   workspace inheritance, optional/target/dev dependencies, root patches и build scripts.
2. Зафиксировать immutable source identity и **каждый файл/resource/NOTICE/license**,
   в том числе `include_*` и assets за пределами crate root. Имеющийся список Git refs
   является conservative union, а не complete per-file closure.
3. Проверить полный source/resource hash inventory до product mutations; записать
   source path/commit/blob SHA256, package/version/features, target destination и license.
4. Спланировать local-path normalization. Cargo metadata/tree после копирования доказывают
   одну own package identity и отсутствие reachable own Git source в product Cargo.lock.
   Registry packages не копируются и не заменяются source replacement.
5. Сохранить лицензии и upstream attribution. Warpbro committed root не имеет LICENSE и
   Cargo license declaration; provenance честно фиксирует отсутствие, не наследует автоматически
   math_en MIT/Apache. У egui root также нет LICENSE; arbor license texts и gizmo NOTICE
   сохранены, недостающие applicable texts закрываются в SOURCE-02 перед distribution acceptance.
   Пользователь уже разрешил внутреннее копирование; новой permission pause не требуется.
6. Уточнить реальные compatibility fixes в локальных copies: WASM RNG configuration,
   jph removed SIMD intrinsics, clipboard/clock/sample/depth services. Никакого cache surgery.
7. Зафиксировать preflight receipt; затем копировать original bytes, проверять exact hashes
   и документировать каждый изменённый manifest/source patch.

Compiled Warpbro раньше остановился на uuid browser RNG; этот working-tree check не
доказывает committed snapshot. Toolkit default native matrix50/50 и WASM48/50 — historical
compile evidence: nodes-view RNG и image-clip EXR/jph SIMD требуют исправлений. LDR variant
собирался, но он не заменяет обязательный EXR. Новый copied entry запускается отдельно.

### Первые own source groups

| Project | Frozen source revision | Зачем |
| --- | --- | --- |
| egui-phosphor | `21aeabe699eea6f8dee740580097c4e920df1702` | Config icon font resource; copy complete actual crate/fonts/notices. |
| curves-rs | `169ded7299f85d1f2b2472c5f579679993c9c222` | Audited toolkit dependency; reconcile committed WarpBro curves2d82329 vs audited169ded through public checked adapters; no second timeline authority. |
| nodes-rs | `8b4b9e8399ce1474b058762598014aca0e02c4c4` | Copied camera helpers and nodes UI closure; graph projections consume Session, no nodes AutoCook/second graph truth. |
| render-rs | `0d1af8c796a8cb188fde5ec7458054c1a6f84517` | Existing sim-render standard-surface-bsdf dependency now converted to physically copied same-source crate; public WGSL byte identity retained. |
| egui-file-dialog | `865964380b02525bcf4c7d145e37f820b72486fc` | Native dialog UI retained with PlatformServices; browser file picker is host adapter, not native filesystem emulation. |
| oiio-rs | `3e5b2c9fe7d71529412fac2e70f94f3010eaf837` | Toolkit ICC/LUT/OCIO processing closure; explicit byte/config resolution and injected same-GPU service. |
| wgpu-widgets-rs | `386303241deaa785adb67af56fb0223eed0667db` | Toolkit GPU-view/text/theme/flex geometry closure, clean shared host/lifecycle services. Bug4 public clipboard/sample/depth/clock defects must be fixed in copied owned API. |
| gpu-info-rs | `5d345b3bc2a44e1ff2bcbbcfe2f9d89b9a66568c` | Toolkit display/toolbar host capabilities; no request-global/private-device in shell. |
| exr-rs | `fb1765f99b4a8732c8716f5bd81e8e01b4fc4ba6` | Full mandatory EXR/ImageClip closure; copied public jph-rs WASM intrinsic fix required, not blanket LDR substitution. |
| jph-rs | `a1bf2dbbe6c3e000d67063ce2bb5aff4bd6f1b13` | Repair removed WASM intrinsics in owned copy preserving correct lane semantics and tests; affects EXR, then recheck complete closure. |
| otio-rs | `229dfb41b446dfe0ae7e959504aace3892a37749` | Optional professional media/timeline adapters retained for full feature scope; its document/playback never substitutes Session. |

Остальная activated closure: box, codec-simd, graph-layout, imath/deflate, LCMS,
murmur и другие packages из точного plan vendor list. Их per-file inventories ещё предстоит
получить в preflight; hashes не выдумывать. Будущие glTF/ABC/USD/FX/tile-atlas packs
получают тот же source/resource/patch protocol в соответствующих задачах.

Committed `src` не содержит runtime `include_bytes!/include_str!` assets.
Fonts/icon fonts приходят из dependency resources. Gallery/material thumbnails генерируются
из настоящего document/material state; `docs/*.jpg` — reference, не поддельные thumbnails.
Colour configs/HDR/templates/bookmarks/imports разрешаются как runtime package/URI assets.

## 4. Все панели, settings и chrome

Каждая donor panel доступна через menu/docking/reopen; selection, DnD, draft и action
семантика сохраняются и подключаются к нашему document/controller. Ни одна панель
не считается реализованной по одному видимому заголовку.

| Panel | Destination | Сохраняемые возможности | Владелец данных/действий |
| --- | --- | --- | --- |
| Viewport (`viewport`) | `shell viewport + sim-render controller` | resolved scene texture; select/move/rotate/scale tools; local/global/custom/parent spaces as reviewed public capabilities; camera slots; exposure/display/view/channel/gamma controls; show/selectability per kind, extras; fit/home/projection/grid/snap/precision; flight and Houdini orbit/pan/dolly; preview/cache/progress | ViewState camera/tool/layout; qualified shared picker; InputCoordinator transforms; colour/GPU services; Session playback |
| Gallery (`gallery`) | `panels/catalog.rs` | adaptive card grid/search; template/category library; thumbnail work queue/retry/stale rejection; create/open/load scene; context actions | sim-project resource/template service; creation typed author command; real render thumbnails |
| Bookmarks (`bookmarks`) | `panels/catalog.rs` | save current snapshot/bookmark; name/open/delete; independent IDs and preview; persistence across runs | Portable project/session snapshot services; no donor load-bookmarks PathBuf/fs |
| Materials (`materials`) | `panels/materials.rs` | create from preset; select/edit/rename/duplicate/delete; context assignment to selected and grouped objects; remembered consumers; thumbnail refresh/error retry | Author material graph/object binding command, sim-render public BSDF; property view reflects actual document |
| Attribute Editor (`inspector`) | `panels/inspector.rs` | schema sections/search/filter; custom sliders/exact fields/soft vs hard ranges; integer/f64/vector/colour/file/reference/choice/bool editors; units/expression/constraint/errors; component expansion and splitter origins shared with timeline; key/auto-key/timeline reveal/reset/context menus; metadata dirty drafts and multi-edit | Read-only typed PropertyProjection and draft editor; f64 canonical parses; transactional typed edits through coordinator/author command authority |
| Settings (`settings`) | `preferences.rs + color_ui.rs + profiles` | Display; Color; Controls; Fonts; Animation; Render & Viewport; fonts and UI scale; dense metrics/status layout; camera inertia/gizmo preferences; slot button mapping; key interpolation/defaults | View/preferences state only for appearance/nav; authored colour/output profile commands distinguished; PlatformCapabilities controls available backends |
| Render / Encode (`export`) | `panels/export.rs` | name/resolution/sample quality/denoise; frame range/current/timeline; PNG/EXR/video formats; encoding/HDR peak/display transform; FPS rational presets/quality/encoder policy; start/progress/cancel/error | ExportPlan typed service freezes immutable document/version/time range; deterministic sim-runtime samples; browser/native encoder capability matrix |
| Timeline (`timeline`) | `panels/timeline.rs` | layer hierarchy/names/shy/selection; keys and clips/lanes/component expansion; DnD ordering/key move/copy/delete; work area/frame/FPS/range/loop; time markers/digit slots/fit/zoom/snap; preview caching/pause/selection playback; Linear/Smooth interpolation; Simplify/Smooth/Euler filter parameters | TimeTicks and authoritative tracks; widget projection/cache never advances its own authoritative playhead; all mutations typed atomic author intents |
| Outliner (`outliner`) | `panels/outliner.rs` | tree hierarchy/search/reveal; selection/range/multiselect; context/add/create kinds; reparent/order DnD; visibility/selectability/lock/isolate; stable identifiers and caches | Qualified object/instance IDs; ViewState selection; structural author intents validated against actual document |
| Material Library (`material-library`) | `panels/materials.rs` | read-only categories/presets/search/cards; explicit add-to-work-area; independent material copies; assignment separate from selection | Public material/preset service actual descriptors/real thumbnails; presets not fake fractal scene numbers |
| Takes (`takes`) | `panels/takes.rs` | record switch; selection/range list; play/bind/unbind; trim/speed/split/derive/effects non-destructive copies; camera/object capture; bake channels; copy/paste/import/export; target compatibility and errors | Session-derived sampled take data; explicit author recording transaction, not automatically mutating a reader camera into document; project service for packages |

Settings содержит **шесть** страниц: Display, Color, Controls, Fonts, Animation,
Render & Viewport. Fonts/UI scale, camera inertia, gizmo, camera slots и interpolation
не теряются. Appearance/navigation — ViewState/preferences; authored color/output changes
идут отдельными typed document commands. Native/browser capabilities показаны явно.

Chrome сохраняет File/Edit/View/Render/Window menus, dynamic translated items,
dock splits/tabs/close/reopen/floating, named layouts save/restore/update/reset,
status/progress/error/GPU metrics, toolbar placement/collapse/camera slots,
scoped hotkeys/command routing, font chooser/icons/dense attributes/solid scrollbars.
Floating egui panels остаются окнами внутри canvas, не OS/browser windows;
restore clamped в видимую work area, layout schema versioned и recoverable.

**Отсутствующие у Warpbro, но обязательные функции:** полноценный Bezier value/speed/tangent
editor, настоящий computation graph с ports/types/units/cycle diagnostics,
tensor/table/formula/code linked views, palette/assets/compositions, chapters/branches,
quizzes/hotspots/captions/audio/learner permissions. Они добавляются в Tasks05–17;
donor timeline interpolation не выдаётся за готовый Graph Editor.

## 5. Typed UI boundary и единственная authority

```text
browser/native input + service results
                  |
       EditorProjection (readonly)
                  |
   EditorUi.draw → UiOutput/UiIntent
          |             |
      ViewState      controller/admission
                        |
             author transaction / InputCoordinator
                        |
                 Session → sim-math
                        |
            FrameSnapshot truth/provenance
                        |
          GpuHost scene + egui compositor
```

Portable public boundary:

```rust
EditorUi::draw(
    ctx: &egui::Context,
    projection: &EditorProjection,
    view: &mut EditorViewState,
    capabilities: &PlatformCapabilities,
) -> UiOutput
```

UI не получает mutable Session, native WorldEditor, Queue submit или filesystem handles.
Projection содержит typed IDs/revisions/GenerationId, readonly truth/provenance и
author schema metadata. Semantic labels отделены от IDs; canonical f64/tensor данные
не превращаются в f32 widget truth. Dirty caches stamped document/session/schema/layout.

UiIntent разделяет View, Gesture, Author и Service. Gesture несёт GestureId/PointerId,
phase, parameter patch и DisplayStamp. Structural author journal имеет одного approved
owner; interaction Undo остаётся у InputCoordinator. Global Undo маршрутизируется
по виду authoritative transaction, не по приватному panel history. Camera/selection/layout
не создают mathematical history. Donor WorldEditor/evaluator/undo не копируются в authority.

Transport переводит rational display frames/FPS в checked decimal TimeTicks.
UI не вызывает отдельный WorldUi::advance и не хранит независимый authoritative playhead.
Keys сохраняют stable IDs и точное время после сортировки/retiming.

### Сохранение принятого gesture protocol

- Press сначала pending. Proposal решается/валидируется до первого owned write.
- Первый Err/Ok(false) освобождает только вновь acquired **unwritten** ownership; press
  остаётся pending и rebases по текущему playback frame.
- Ok(true) устанавливает started **до compile**: render failure сохраняет компенсируемую запись.
- Cancel восстанавливает точные Some/None overrides и intent через monotonic compensation.
- External revision/ABA/event gaps/permission conflicts fail closed; terminal nonwriting
  abandon освобождает ownership без восстановления чужих значений.
- Foreign pointer не завершает владельца. No-op не очищает redo и не останавливает playback.
  Принятые CONSUMER-R1/R2 regressions и исторические snapshots сохраняются.

## 6. Один GpuHost и pass graph

Ровно один Instance/Adapter/Device/Queue и один encoder/submit owner на mount/device generation.
Scene RasterRenderer только encodes; egui renderer получает тот же Device/Queue.
Native Warpbro device/shell и private OCIO submit не переносятся.

Frame order:

1. Нормализовать input; принять только generation/revision-valid async results.
2. Egui читает coherent projection, отдаёт intents/view rectangles/texture deltas.
   После применённого intent переснять truth, dirty scene и camera/display stamps.
3. Проверить integer backing sizes/DPR/device features и **union old/new/temporary**
   resource budget до allocation.
4. Каждый scene view рисуется в собственные MSAA4 color/depth и single-sample resolve.
   Scene RH reversed-Z: Depth32Float clear0; viewport/scissor local.
5. Resolved texture регистрируется в egui на том же device с declared format/alpha/UV.
6. Font/image deltas и UI buffers загружаются; shell compositor использует отдельный
   host surface pass с явными sample/format, без scene depth.
7. Callback command buffers входят в общий host submit batch; один submit/present.
   Не request_device/submit в callbacks и не sync readback/poll в browser.
8. Retire texture IDs/targets безопасно по host generation/frame; ledger считает
   одновременно живые physical resources.

**Текущий encode_view делает Clear всей attachment до scissor.** Поэтому первый перенос
использует отдельные per-view targets. Нельзя наивно вызывать его несколько раз на surface:
он сотрёт соседний viewport/chrome. Direct-surface optimization требует отдельного
reviewed Load/Clear contract, не scissor workaround.

Sample4 поддержка проверяется реально у color/depth/pipelines/resolve.
Linear/sRGB/premultiplied alpha и один color-management transform проверяются pixel tests;
UI SDR text не меняется из-за scene HDR. HDR/float filtering только по actual capabilities.

Grow/reuse COPY_DST buffers/bindgroups, dirty-only scene/refit, bounded pick scratch и
font/texture caches; retire views при close/resize/loss/dispose. PICK-P1 и нынешние per-render
allocations остаются исправляемыми blockers. Доказательства: два одновременно движущихся
views без clear wipe, DPR1/1.25/2, AA fractional motion, format/device logs, resize/dock/fullscreen,
loss/recovery и repeated mount/dispose CPU/GPU peaks.

## 7. Настоящая скопированная gizmo

Копировать egui-gizmo целиком из audited toolkit; clean public API расширять в локальной
копии, сохраняя совместимый egui interact wrapper над тем же kernel. Новый отдельный
самодельный rotate/scale engine не нужен.

Public seam: checked viewport CSS rect/DPR/ViewId/camera convention, caller **f64 ray**,
stable GizmoHandleId/TargetId, neutral candidate geometry и DrawData,
Begin/Change/Commit/Cancel с настоящим active-state cleanup.
clear_hover не заменяет Cancel.

Common qualified BLAS/TLAS picker решает eligibility/occlusion/hit slop
14 CSS px mouse/22 touch. Gizmo private picker не запускает второго владельца.
DisplayStamp един для pick/draw. NDC-1..+1 donor и наш reversed-Z1..0 адаптируются явно.
Canonical matrix/parameter f64 сохраняется; f32 допустим только для visual representation.
Lossy affine/shear/TRS conversion отклоняется, не скрыто decomposes.

Полный tools набор: Move/Rotate/Scale/Universal; axis/plane/screen move;
axis/arcball rotation; uniform/axis scale; 2D/3D box face/corner grips; shear/corner pin;
multiselect, snapping/precision/keyboard equivalents. Parent/custom pivot — новый explicit
public config, не заявленная существующая facade. Все tools идут через один
InputCoordinator transaction; navigation modifiers/profile выигрывают до activation.

## 8. Browser/platform/input/locale/accessibility

Async ServiceRequest несёт request_id/generation/document_revision/limits;
Result/Progress/Error/Cancelled отбрасывает stale responses и освобождает resources.
Capabilities: Available, Unavailable с reasonKey, RequiresUserGesture.
Temporary explicit unsupported service не считается полной working feature.

Services: package/template/bookmark open/save, OPFS/IndexedDB/native atomic IO/conflicts;
browser file picker + стандартный file-input fallback/native dialog; bytes/URI/drop resolver;
clipboard text/package/PNG с user gesture/errors; download Blob lifecycle;
monotonic RAF clock отдельно от logical Session; same-GPU color/thumbnail;
workers/jobs cancellation; deterministic frames/PNG/EXR/SVG/video codec matrix;
audio/caption seek/pause/speed без reader autoplay.
Native FFmpeg/Vulkan/CUDA/OIDN implementation не входит в portable dependencies.

Browser entry `apps/sim-web`: async mount(canvas,config), resize/input/locale/services,
serialize/recover/dispose. Author и learner имеют раздельные compiled artifacts,
а не один монолит с dynamic import.

Routing: focused text/IME → egui chrome/modal → hit viewport navigation/tool.
Reserved browser chords сохраняются. Canvas early MMB pointerdown/mousedown предотвращает
autoscroll только для owned gesture; nonpassive wheel нормализует deltaMode/cancelability.
Lab wheel dollies; embedded plain wheel scrolls page, Ctrl/Cmd+wheel/pinch zooms.
Outside canvas defaults сохраняются. Pointerup flushes last move; cancel/lostcapture/Esc/
blur/hidden/dispose finally освобождают capture/modifiers. Нет invisible fullscreen
capture layer, PointerLock или KeyboardLock по умолчанию.

Shared EN/RU catalog охватывает все11panels, six settings, menus/dialogs/hints/errors/ARIA,
units и number parsing; formal «вы», IDs/action keys не переводятся.
Language switch сохраняет draft/focus/time/camera/session и не commit поле.
Third catalog registration/parity/roundtrip проверяются как extensibility.

Визуал: calm professional dark UI, без Holo; ordinary font/scene labels **11 CSS px**,
фиксированы через scene zoom. Dense36px topbar/28 headers/24–26 rows/24 numeric envelope/
8 group padding адаптируются с сохранением reachability; explicit coarse mode hit areas≥44px.
Solid scrollbars, narrow docking/reopen, все controls доступны.

Egui painting само по себе не accessibility proof. UiOutput stable semantic tree
создаёт настоящий DOM/AccessKit-compatible host bridge roles/labels/values/ranges/actions/focus.
IME hidden editor только у текущего поля, не заслоняет viewport. Реально проверить tab,
arrows, shortcuts, editing и assistive reader без второго DOM mathematical store.

## 9. Последовательность WP-0–WP-5

Все пакеты — этапы зависимостей полной версии, не сокращение featureset.

### WP-0 — Tasks 03, 04, 07, 14

**Ownership:** original /root/sim_renderer_impl sole product writer after root release; independent reviewer separate model.

**Paths:** `provenance/warpbro`, `vendor/egui-widgets-rs and complete activated own crate/resource closure`, `crates/sim-editor-ui`, `workspace manifest/lock only necessary changes`.

**Работа:** First concrete bounded unit: freeze/copy exact WarpBro UI slices + all50 toolkit library source/resources/notices and recursively required selected owned crate closure; generate path-normalized manifests/provenance. In same unit provide working portable sim-editor-ui library with all11 real registered panels/chrome/sixpages, typed EditorProjection/ViewState/UiIntent boundary and real egui context render using authoritative cosine Session projection for existing controls. Native execution bodies absent from portable dependency graph. Every menu/item either produces valid typed existing intent or explicit capability/service request with scheduled gate; no blank placeholders or fake evaluator. Preserve source behavior traceability for all controls, do not claim full binding completion.

**Приёмка:** Per-file copy/source hash equality and target patch hashes; exact no-loss panel/action registry; cargo check --locked --offline -p sim-editor-ui native + wasm; real native headless egui all11 panels, menus/docking/drafts; explicit borrowed projection API prohibits Session/GpuHost/native service mutation; self checks then different-model independent review. This unit is source/library boundary acceptance only, not full browser/editor or full18 completion.

### WP-1 — Tasks 03, 04

**Ownership:** same sole writer, public owned-vendor API symbols scoped after impact.

**Paths:** `vendor/egui-widgets-rs/crates/egui-gizmo`, `sim-render picking/interaction adapter`, `sim-editor-ui tool/config panels`.

**Работа:** Implement reviewed neutral public gizmo seam and attach all tool presentation/candidates to current common picker/f64 solver/coordinator; keep original wrapper source compatibility. Fix copied default WASM RNG config and jph SIMD public API correctness independently, no dependency-cache surgery.

**Приёмка:** Independent gizmo lifecycle/ray/clip/f64/affine/tool/multitarget cancellation/Undo and old consumer wrapper checks; native+WASM selected closures; no second picker/history.

### WP-2 — Tasks 03, 04, 14

**Ownership:** same writer; no concurrent app/GPU writer.

**Paths:** `sim-render GpuHost/pass/cache`, `apps/sim-web`, `sim-editor-ui semantic bridge`.

**Работа:** Single host integrates scene offscreenAA4 resolved textures into full egui shell; browser mount/input/IME/locale/services; preserve exact R1/R2terminal recovery, no user manual tab overwrite. Fix PICK-P1/reused GPU buffers with measured bounded scratch/caches before perf closure.

**Приёмка:** Actual copied cosine author page all11panels, real existing parameter/timeline/nav/gizmo interactions; two-view pass proof; EN/RU/density/browser wheel/MMB/escape/IME/keyboard/a11y; repeated lifecycle/loss/memory; payload measured. Preview staging separate historical URL.

### WP-3 — Tasks 05, 06, 07, 12

**Ownership:** task-owned sole executor waves with independent reviews.

**Paths:** `sim-author`, `sim-runtime approved extensions`, `sim-project`, `sim-editor-ui graph/curves`.

**Работа:** Complete donor takes/key/clip/material/profile/catalog actions against actual authoritative document and accepted time/project APIs; dedicated missing graph and value/speed curve/tangent editors; tensors/math linked views; permissions/chapter/quiz/caption/audio tooling. Domain command extension handoff if accepted core lacks action, not UI schema workaround.

**Приёмка:** Full AUTHOR01–11/TIME01–08/CORE05–12 with author→preview/save/open and one actual runtime.

### WP-4 — Tasks 08, 09, 10, 11, 13, 14

**Ownership:** scoped format/platform owner under root coordination, no overlapping mutations.

**Paths:** `sim-project asset/import/export/audio`, `vendor full format/native/browser required own crates`, `apps/native/web`.

**Работа:** Full assets/formats, glTF/ABC/USD compatibility, EXR/color/thumbnail/exports/video/audio services, browser/native capabilities and generation/limits; retain controls and genuine documented platform matrices. Physically copied sources use same provenance/clean API policy; registry normal.

**Приёмка:** ASSET01–06/SAVE01–04/EXPORT01–02/AUDIO01/LIFE01–06 independent actual files/time/lifecycle/target receipts.

### WP-5 — Tasks 15, 16, 17, 18

**Ownership:** root orchestration, independent course/visual/release reviewers.

**Paths:** `dev EN/RU copied lessons`, `canonical fixtures`, `coverage and acceptance records`.

**Работа:** Full copied cosine overview/detail/proof/shear/rigidturn/IK, ten canonical and all98 course entries mapped and gaps closed; final fullfeature/compatibility/accessibility/perf/size gates, user review. Original lessons/state remain untouched until later authorized migration.

**Приёмка:** All Phase05 boxes and full18 scope closed with immutable implementation/check/review receipts; no single-shot claim from initial shell.

## 10. Трассировка всех 18 задач

| Task | Обязательный результат |
| --- | --- |
| 01 | Typed foundation/provenance; новая local source closure. |
| 02 | Единственная Session/math/time authority; без donor WorldEditor. |
| 03 | Same-GPU shell, material/AA/multiview/lifecycle/performance. |
| 04 | Public gizmo, все tools/common picker, IME/locale, один coordinator. |
| 05 | Authoritative keys/clips/NLA/Bezier/takes/retiming/chapters/replay. |
| 06 | Реальные tensor/autodiff/matrix/probability/attention views. |
| 07 | Все 11 панелей и graph/curves/resources/permissions/chapter/quiz workflow. |
| 08 | Generation-guarded asset bytes/URI resolver и import jobs. |
| 09 | GLB/glTF material/animation/skinning/morph pipeline. |
| 10 | Ogawa ABC: changing topology/time samples/reversible seek. |
| 11 | USD layers/variants/references/payloads/schema/material matrix. |
| 12 | Portable package/save/open/clipboard/bookmarks/takes; conflict/recovery. |
| 13 | Детерминированные frames/PNG/EXR/SVG/video и audio/captions/codecs/sync. |
| 14 | Раздельные author/learner WASM; full-window/native/embed/lifecycle/a11y. |
| 15 | Изолированная EN/RU полная копия косинусов с отдельными IDs/history. |
| 16 | Все десять canonical scenes: author→package→embed/replay/provenance. |
| 17 | Все 98 entries курса: mapping и закрытые concrete missing features. |
| 18 | Полная independent feature/perf/lifecycle/license/payload/user acceptance. |

Подробные acceptance IDs определены [Phase05](phase-05-acceptance.md).
Все изменения проходят own scoped checks и independent review другой моделью.
Один product writer; каждый следующий scope явно передаётся после frozen receipt.
Не редактировать чужие donor/product изменения, не commit/push без запроса.

## 11. Gates и текущие ограничения

- SOURCE: before-copy complete transitive per-file/resources/licenses inventory; after-copy
  exact pristine hashes и reviewed patch hashes; metadata/tree local closure; no own Git source.
- ARCH: WP-C1–13 приняты только как plan, с обязательным WP-0 preflight.
- BUILD/RUNTIME: новая copied shell actual native/WASM compilation, public wrapper compatibility,
  actual browser all panels/docking/fields/menu/services и lifecycle acceptance ещё впереди.
- VISUAL/INPUT: EN/RU, 11 CSS px/touch44, real gizmo/tools, MMB/wheel/Escape/IME,
  semantic a11y, AA/DPR/two-view/device-loss evidence.
- PERFORMANCE: cold/warm p95 frame/input/query, demand redraw и peak physical CPU/GPU.
  PICK-P1/current GPU allocations остаются открытыми.
- SIZE: renderer-containing physical artifact≤300000 B gzip; otherchunks≤100000,
  taskincrement≤18000, lazy graph gate сохранён. Current renderer **385747>300000**.
  Author fonts/UI могут увеличить payload: измерять physical author/learner/network closure
  рано, оптимизировать реальный код/resources/caches. Не relabel bytes и не удалять required UI
  ради формального pass. Waiver не дан.
- FULL: полные Tasks03/04, все18, десять canonical scenes и98 course entries остаются открытыми.

Сохранять original/fix1/fix2 trials и все accepted core/picker/input/AA receipts.
Новая staging URL появляется только с coherent build/browser review;
existing user/manual tab не reset/reload. Full completion требует всех Phase05 gates и
review полной копии косинусов, а не только screenshot или начальный shell.

## 12. Текущее исполнение

WP-0 underway: исходный renderer executor — sole product writer.

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

Дальнейшие source/library/browser receipts записываются в
[ledger](../../sim-widget-rs/docs/implementation-status.md).
