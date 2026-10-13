# Phase 03 — аудит переиспользования StoryTold

[Подробный принятый план полного Warpbro UI port](phase-06-warpbro-ui-port.md):
архитектура APPROVED_FOR_IMPLEMENTATION, WP-0 preflight/исполнение underway; actual copies,
build/browser/full-feature acceptance требуют отдельных receipts.

Дата: 2026-10-12. Статус: DRAFT, аудит исходников и документации.
[План](plan.md) · [Featureset](phase-01-featureset.md) · [Пилот](phase-02-pilot-and-delivery.md).
Область: C:/projects/projects.rust.cg/artcraft/storytold. Интегрированная StoryTold сборка
и browser launch не подтверждены; частичное render-rs WASM compile evidence — в журнале.

## Вывод для архитектуры

StoryTold содержит кандидатов для профессиональной Rust-оболочки, анимационного runtime
и браузерного хранения. egui/eframe следует сравнить с DOM-гибридом как серьёзный вариант
full-window editor и embedded widget. Выбор UI пока открыт: нужны формулы, IME,
accessibility, scroll/touch и embedding в Astro на копии косинусов.

## Уровни переиспользования

| Уровень | Условие |
| --- | --- |
| Локальный vendored crate | Физическая копия самостоятельного crate с подходящими API, лицензией, features и target support |
| Extract/adapter | Полезная логика связана с app model; выделить контракт и преобразование типов |
| Reference | UI/сценарий показывает подход, но перенос потребует новой реализации или не доказан |

Перенос приложения целиком не является выбранным решением. Документы объяснения, semantic IDs,
математические operators, timeline и история должны остаться общими для author/learner.

## Подтверждённые кандидаты

| Проект | Что найдено | Предлагаемый reuse | Ограничение доказательств |
| --- | --- | --- | --- |
| DeckCraft anim | state_at(shape, para, time), state/state_with; trigger, step duration, repeat/reverse/rewind, easing/morph/path/show | Extract runtime primitives через адаптер | Типы связаны с deckcraft_model и slide model |
| DeckCraft web | eframe wgpu/accesskit, browser async file/download, WebGPU/WebGL2 пути | Shell/startup/reference | Source comments: preferences не persisted; target behavior не запускался |
| DeckCraft gaps | Animation pane thin, no timeline, alpha gaps | Список задач editor | Runtime animations не означают полный author timeline |
| LightCraft web | eframe glow/WebGL2, independent-memory workers | Reference web hosting/worker boundaries | Это другой backend, не доказательство wgpu integration |
| LightCraft storage | OPFS/IndexedDB, catalog journal/cache | Extract storage adapter | Known same-origin multi-tab writes: last wins; нужен concurrency policy |
| EffectCraft keyframe/time | evaluate(keys,t,spatial), Tick, Bezier и spatial interpolation | Локальный vendored crate candidate | Проверить units/time conversion, portability и feature graph |
| EffectCraft ui-egui | Реальный graph value/speed editor, handles/multi-select/command edits | Extract/adapter | UI связан с app model/commands |
| EffectCraft engine | `Arc<Project>` Undo/Redo, merge и branching history | Extract pattern/adapter | История привязана к EffectCraft Project |
| EffectCraft screenshots | AE-like timeline, curve и properties layout | Visual reference | Screenshot не доказывает плавность или готовность APIs |
| craft-ui | Generic docking, tabs/layout, buttons/icons/theme primitives | Локальный vendored crate candidate | Application commands/document semantics остаются приложению |
| craft-common-actions | CI workflows | Build/release reference | Это не runtime command system |

DeckCraft точка времени: crates/anim/src/timeline.rs, state_at на строке 433.
Source evidence: apps/deckcraft-web/src/main.rs и Cargo.toml; docs/gaps.md;
LightCraft apps/lightcraft-web и docs/web.md. Точные reuse package paths фиксируются
при выделении boundary; при выборе dependency читается полный feature graph.

### craft-ui — наиболее прямой кандидат UI

Путь внутри StoryTold: craft-libs/crates/ui. Manifest: egui 0.36, default features отсутствуют,
optional serde, Rust 1.95, MIT/Apache. Реальные exports: `Layout<P>`, `Node<P>`, `Action<P>`,
Permissions, DockError, DockArea/DockContent/DockStyle, PanelLimits, begin_drag/cancel_drag.
validate/apply/apply_with_permissions дают транзакционный путь layout operations:
split/tab/accordion stacks, move/float/close/activate/open/reposition/resize.
Layout/Floating поддерживают serde; DockArea snapshot возвращает actions для app dispatch,
DockContent задаёт labels; permissions включают PROTECTED.

Дополнительно Tab/fit, split_rect/SplitSize/ResizeHandle/fit_toolbar, IconButton,
SvgIconSet и WidgetPalette. Floating groups — egui-панели, не отдельные OS windows.
lib.rs оставляет commands, document semantics, theme/assets приложению.
Общего undo/command bus здесь не подтверждено; история semantic edits проектируется отдельно.
Это сильный local vendored crate candidate после проверки совместимости и сборки.

### EffectCraft — вычисления анимации и реальный editor

crates/time использует Tick: 254,016,000,000 единиц в секунду.
crates/keyframe/src/lib.rs содержит Keyframe/Ease/Interp и evaluate(keys,t,spatial):
per-dimension speed/influence Bezier, hold/linear, auto/continuous, spatial arc length,
roving и time_reverse. Dependencies time/geom/color/serde делают evaluator кандидатом
на отдельное переиспользование, но browser target ещё не собран.

crates/ui-egui/src/graph.rs реализует value/speed graph editor с handles, multi-select
и command edits. engine/src/lib.rs хранит `Arc<Project>` историю, Undo/Redo, merge и branching.
Для нашего документа требуется adapter и проверка транзакций; готовый общий command bus
из этого не следует.

docs/gaps.md описывает раннюю зрелость: playback/scrub stutter, отсутствие 3D gizmo,
accessibility не проверена. Оценка около 45% является оценкой документа, не измерением.
Поэтому actual code полезен, но polished motion/performance не подтверждены.

### EffectCraft — properties, timeline и web shell

project/src/props.rs:58 — Property со stable UID, static/keys/expr, ParamUi,
hold/spatial/shown_dims; value_at на строке 123. Это близко typed property contract,
но expression provenance и math shape/unit diagnostics требуют нашей модели.

ui-egui/src/timeline.rs содержит hierarchy, scrub, stopwatch, navigator, work area,
cache/ruler, bars и keys; graph.rs — value/speed handles.
dock_shared.rs использует craft_ui docking; workspace_state.rs реализует сохранение layout.
engine/src/lib.rs:660 — merge/history, :701 — Undo, :711 — Redo.

apps/effectcraft-web/src содержит WASM shell, async worker GPU jobs, browser audio,
VFS и disk cache. UI manifest: eframe 0.36 wgpu/accesskit/web-time, native syslocale
gated; workspace wgpu 30.0.1 с fragile-send-sync-non-atomic-wasm, Rust 1.95, MIT/Apache.
Это browser implementation evidence, а не подтверждение запуском. Проверяются worker
device/resource ownership, embed/input и семантика отмены задач.

### Общие web shells и fonts

WordCraft/GridCraft/launcher также используют eframe 0.36 wgpu/accesskit и web-time clocks.
Это дополнительные patterns запуска приложения; mathematical runtime из них не следует.
craft-fonts содержит комментарий о примерно 24 MB embedded fonts: такой пакет нельзя
автоматически включать в widget; оцениваются fetch/subset и native filesystem loading.

## Совместимость и бюджеты

Manifest majors render-rs и просмотренных story UI: wgpu 30, egui 0.36.
Это номинальное совпадение, а не результат совместной сборки; проверяются features,
transitive dependencies, device ownership и integration API.
Manifest scan StoryTold не нашёл текущих references на cglibs render-rs/wgpu-widgets/nodes-rs/
curves-rs/graph-layout-rs: это отдельные экосистемы, не готовая интеграция.
Наличие test sources не является доказательством прохождения тестов.

В LightCraft docs приведён полный bundle около 4,583,465 B gzip с codecs и без wasm-opt.
Это измерение другого приложения, не прогноз размера нашего runtime.
Текущий renderer gate 300,000 B gzip остаётся ограничением.

Workers LightCraft используют независимую память; docs/web.md описывает отсутствие
необходимости COOP для текущего пути. Для нашего shared-memory/Rayon режима решение
нельзя наследовать: отдельная схема памяти требует собственной проверки headers.

На viewed EffectCraft screenshots есть значения порядка 129–176 ms; они не доказывают
frame time, измеренный FPS или возможную плавность будущего виджета.

## Источники зависимостей

Последнее решение пользователя, 2026-10-13: «мы КОПИРУЕМ крейты из warpbro и других проектов».
Выбранные нужные crates и внутренняя own-project closure Warpbro, egui-widgets-rs
(включая выбранные widgets/gizmo), остальных cglibs и StoryTold физически вендорятся
в `sim-widget-rs`. Перенос включает весь Warpbro UI с нашим wgpu viewport.
Это заменяет прежнее SSH-only решение и временное исключение только для Warpbro UI.
Исторические source audits и проверенные commits ниже сохраняются как evidence.

| Источник | Способ | Фиксация |
| --- | --- | --- |
| Warpbro, egui-widgets и остальные наши проекты/cglibs | Физические выбранные crates + необходимая own-project closure, local path dependencies | Frozen source commit, source/copied SHA256, licenses/NOTICE/assets, local patch journal |
| StoryTold | Физические repository-owned copies и внутренняя closure | Source commit/hash, local paths, licenses/NOTICE, patch journal |
| crates.io | Обычные registry packages, **без vendoring** | Cargo versions + Cargo.lock/checksums |

Целевая структура (каталоги появляются с нужной реализацией):

```text
sim-widget-rs/
  Cargo.toml / Cargo.lock      # local own-project paths + normal registry versions
  crates/                     # наш runtime/adapters/shell, copied Warpbro UI shell
  vendor/
    storytold/                # выбранная closure
    cglibs/<source-project>/  # выбранные widgets/gizmo/render/other own crates
  provenance/
    manifest.json             # repo/path/commit/license/features/source/copied hashes
    patches/                  # причины и diff каждой локальной адаптации
    NOTICE
```

Копировать из frozen committed snapshot, отдельно записывать обнаруженные dirty donor
differences; чужие рабочие edits не переносить молча и не менять source repos.
Сохранять workspace package/dependency/lint inheritance, manifests, licenses/attribution,
shaders, build resources и используемые fonts/media/assets. Внутренние own-project
path/Git references замыкаются на локальные copies; исходные donor directories не нужны
для сборки. Registry references остаются обычными Cargo dependencies.
Не копировать .git/target/cache или неиспользуемые bundles.

Update protocol: выбрать новый source snapshot → проверить diff/licenses/features/closure
→ обновить локальные копии и documented patches → обновить provenance/Cargo.lock →
проверить metadata, native/WASM builds и затронутые runtime gates.
SSH URL может храниться как source provenance/download origin; он не заменяет физическую
копию выбранного crate. `cargo vendor` crates.io и registry source replacement не применяются.

Приёмка: local own-project closure, no external donor paths, hashes/licenses/patches,
normal registry lock и реальные native/browser gates. Смена sourcing не закрывает
renderer/input/portability/performance/budget gates. Фактический прогресс — в
[журнале](../../sim-widget-rs/docs/implementation-status.md).

### Проверенные исходники для первого переноса

По preflight координатора от 2026-10-12 SSH authentication работает; проверенные remote HEAD:

| Источник | SSH origin | Проверенный commit |
| --- | --- | --- |
| render-rs | `git@github.com:ssoj13/render-rs.git` | `0d1af8c796a8cb188fde5ec7458054c1a6f84517` |
| curves-rs | `git@github.com:ssoj13/curves-rs.git` | `ada2822fe7eae5a7badeb5622be4f8db2a6a3d7a` |
| nodes-rs | `git@github.com:ssoj13/nodes-rs.git` | `9a92d90561c3ddb323ac27ffec94dad1b2d66df0` |

Это historical baseline для выбора source snapshot; provenance подтверждает source commit,
а manifest/metadata — local vendored dependency под latest2026-10-13 copy policy.
Локальные donor trees clean: craft-libs `1aa81a773aa9198a0e4c0daeda954d622475af38`,
EffectCraft `67434e90388b6dca4417555346fa6850b6f96033`.
Первый selected closure: EffectCraft time/geom/color/keyframe и craft-ui/common;
`serde_json` keyframe остаётся registry dependency. Project engine/Undo и donor app целиком не копируются.

EffectCraft Tick не является публичным wire type: donor использует i64 JSON number,
NaN→0, zero-division fallback и saturating arithmetic. Наш checked `TimeTicks` сериализуется
десятичной строкой; рациональные conversions и range проверяются до вызова donor adapter.
Ошибки не заменяются нулём, единицей или saturating результатом.

## cglibs — выбранные публичные boundaries и fast/light contract

Пользователь явно запросил быстрый лёгкий wgpu и указал nodes-rs, wgpu-fx-rs,
wgpu-tile-atlas, wgpu-widgets-rs. [Durable source audit](../../docs/acceptance/sim-widget-rs/cglibs-reuse-source-audit.md)
фиксирует checked pins, API evidence и ограничения. Это read-only source audit:
build/browser/performance acceptance отсутствует; сейчас зависимости не добавляются.

| Кандидат | Выбранный boundary | Ограничение |
| --- | --- | --- |
| nodes-rs `9a92d90561c3ddb323ac27ffec94dad1b2d66df0` | Editor projection: ports/types/layout/nodes-view из Session truth | Не второй cook/document/history; циклы валидирует sim-math; GraphRenderer encoder/submit/sync poll не подходит browser host |
| wgpu-fx-rs `9b838175005e20f250f610c5bd2012f0ab5378ef` | Scene SoA buffers/HUD SDF и pure wgpu-cam math | Не generic post-FX catalog; sample1 text, discarded errors, frame allocations и fit performance требуют proof/public-API решения |
| wgpu-widgets-rs `386303241deaa785adb67af56fb0223eed0667db` | InputRouter external hover + public shared-device upload/draw; text — будущий measured candidate | Catalog arboard3.6.1 WASM blocker по source; core Instant, sample1 Depth32Float, glam0.33.2 vs наш0.32 не проверены integration |
| wgpu-tile-atlas `8e138c6672810da59a196ecfe7b8764d9b553c7b` | Budgeted raster-tile residency для реального будущего tile workload | Не geometry/BVH/font cache; visible protection может превысить budget, host admission обязателен |

Один Rust Session остаётся authority для truth/provenance/history; camera — одна ViewState authority.
Общий displayed-geometry picker и InputCoordinator продолжаются без ожидания новых зависимостей.
Публичный draw использует host Device/Queue/RenderPass; native convenience submission,
sync readback/poll, private cache patches и upstream edits не допускаются.

Fast/light подтверждается demand redraw, dirty-only refit, coalesced moves и reused buffers,
затем p95 frame/input/query, peak resident CPU/GPU и coherent WASM measurements.
AA preview323366 B gzip всё ещё превышает300000 B, без waiver. Registry packages остаются
Cargo dependencies; selected cglibs/egui/Warpbro/StoryTold crates physically copied under
latest 2026-10-13 authority, with frozen source and local closure/provenance audit. Source findings не закрывают полный renderer/input/release.

## egui-widgets-rs — весь catalog и gizmo reuse

По запросу пользователя проверены все51 workspace entries:50libraries+nativegallery.
[Durable full table](../../docs/acceptance/sim-widget-rs/egui-widgets-reuse-source-audit.md)
фиксирует назначения, implemented reuse, adapters/stubs/browser limits и source receipts.
Pin `96fc02ff58f30f3ea8a39193c39f39004a7835b7`, SSH egui-widgets-rs;
lock egui0.36.2/wgpu30.0.1/glam0.33.12, MSRV1.96.
Новая docs-only donor HEAD не переписывает audited snapshot. Independent cross-check
подтвердила51inventory/698citations/124sourcehashes; REUSE-R1 closure alias gpu-mem→
gpu-info-rs0.1.0 и pollster0.4 исправлены в separate current fix1 report;
[285364 B fix1](../../.superpowers/sdd/sim-widget-rs/reuse-egui-widgets-catalog-audit-fix1.json)
сохраняет original/source entries; [independent fix1 review](../../.superpowers/sdd/sim-widget-rs/reuse-egui-widgets-catalog-audit-fix1-review.json)
APPROVED_FOR_REUSE_R1_CORRECTION закрывает audit finding только, SHA256
`40004443d6150b7f129dcdadefd1103e9ff50860243253c16c17ea865a2ba8e5`.
Runtime/adoption/performance не принимаются этим review.

Author inspector/timeline/keyframes/ramp/outliner/chrome/commands — реальные кандидаты.
Dedicated arbitrary tensor, syntax-highlight code/formula и BSDF material editor не найдены:
обязательные teaching capabilities остаются собственными компонентами полного плана.
Public gizmo T/R/S/Box/Shear/CornerPin/arcball предпочтительно reuse через authorized public
neutral handle/pick/draw/cancel seam; сейчас private picker/draw data, rawMMB, NDC/f32 boundary
не соответствуют shared stamped picker/Houdini/reversed-Z без adapter/public API evolution.
Это historical source ruling, не реализованный seam. Latest copy authority разрешает
локальную vendored copy с reviewed patches; donor/cache edits не разрешены.

Сохранить один Session/InputCoordinator/picker/ViewState/GpuHost; egui host pass/fonts/IME/
locale/resources и отдельные author/learner closures требуют target/runtime/gzip/p95 proof.
Не включать всю gallery/eframe или native export closure в learner ради catalog.
Source builds/runtime/performance NOT_RUN; full gates и budgets без waiver.

Перед adoption в Tasks03/04 требуется отдельный owned local vendored task для additive neutral
egui-gizmo API в локальной vendored copy, compatible facade regression, different-model
review и recorded source baseline/local patch hashes;
consumer затем доказывает общий pick/cancel/clip/GPU pass, bytes/peak/p95.
Не реализовывать свой rotate/scale вместо reuse public kernel. Author packages выбираются
по реальной потребности и отдельной closure proof, не все50 сразу.
Optional OTIO/scopes/video encoding остаются future full professional author options.
[Execution prerequisites](phase-04-implementation.md#reuse-prerequisites-внутри-tasks-0304--egui-gizmo-и-author-widgets)
сохраняют18-task manifest, одного Session/history/ViewState/GpuHost owner и все gates.

## Предлагаемый следующий этап

1. Сопоставить конкретные reusable crates с полным featureset; прочитать licenses/features.
2. Сравнить DOM + Rust viewport и egui/eframe shell на формуле/IME/accessibility/embed.
3. Адаптировать timeline без зависимости от slide model: time/seed/history/overrides.
4. Проверить storage portable package и multi-tab конфликт на отдельной копии проекта.
5. Измерить browser/native portability, frame time, память и bundle до выбора stack.

Пилот остаётся копией закона косинусов; existing lessons и manifests не меняются.
Аудит не подтверждает готовность математического evaluator, полноценного NLA,
декларативной provenance или курса: эти контракты проверяются отдельно.
