# Phase 04 — подробный план реализации

[Подробный принятый план полного Warpbro UI port](phase-06-warpbro-ui-port.md):
архитектура APPROVED_FOR_IMPLEMENTATION, WP-0 preflight/исполнение underway; actual copies,
build/browser/full-feature acceptance требуют отдельных receipts.

Дата: 2026-10-12; sourcing обновлён 2026-10-13. Статус: полная реализация разрешена, bootstrap `sim-widget-rs` создан.
[Общий план](plan.md) · [Featureset](phase-01-featureset.md) · [Пилот](phase-02-pilot-and-delivery.md) ·
[Reuse](phase-03-reuse-audit.md) · [Приёмка](phase-05-acceptance.md).
[Задачи и фактический прогресс](../../sim-widget-rs/docs/implementation-status.md).

## Зафиксировать результат проекта

Полная версия включает author editor, full-window лабораторию и embedded learner.
Документ, вычисления и время общие. Пользователь собирает объяснение визуально, проверяет
reader controls и встраивает сохранённый документ в урок.
Этапы ниже задают зависимости работ; они не сокращают итоговый featureset.
Основная площадка испытаний — отдельная EN/RU-копия закона косинусов.
Миграция действующих уроков начинается после полной приёмки и принятия пилота.

User authority, 2026-10-12: «holo не надо», «нужна добротная и понятная, плавная визуализация»,
«с хорошим приятным UI». Новый widget и copied pilot используют спокойную Blender-like сцену,
стабильные читаемые числа/подписи, плавную continuous presentation и direct drag/scrub;
приятные профессиональные typography/spacing/focus, inspector и timeline. Декоративного Holo/HUD нет.
Этот scoped контракт заменяет stale root AGENTS Holo requirement. Все 18 задач и capabilities
сохраняются; legacy lesson/covers migration остаётся отдельной.

Работа выполняется в новом каталоге на существующей `feat/engine`; чужие изменения сохраняются.
Пути с пометкой «план» создаются с работающей реализацией. Bootstrap не означает готовности функций.
Source URL/rev проверены отдельно; device matrix и измерения требуют своих evidence.

## Содержание

- [Структура workspace](#структура-workspace)
- [Контракты и поток данных](#контракты-и-поток-данных)
- [Зависимости пакетов работ](#зависимости-пакетов-работ)
- [Задачи исполнения](#задачи-исполнения)
- [A — подготовка и источники](#a--подготовка-и-источники)
- [B — исследования риска](#b--исследования-риска)
- [C — документ, команды и вычисления](#c--документ-команды-и-вычисления)
- [D — сцена и renderer](#d--сцена-и-renderer)
- [E — управление и качество UI](#e--управление-и-качество-ui)
- [F — время и интерактивные ветки](#f--время-и-интерактивные-ветки)
- [G — математика и данные](#g--математика-и-данные)
- [H — визуальный редактор](#h--визуальный-редактор)
- [I — assets и форматы](#i--assets-и-форматы)
- [J — хранение, экспорт и звук](#j--хранение-экспорт-и-звук)
- [K — оболочки и embedding](#k--оболочки-и-embedding)
- [L — копия косинусов](#l--копия-косинусов)
- [M — покрытие курса](#m--покрытие-курса)
- [N — завершение полной версии](#n--завершение-полной-версии)
- [Решения и риски](#решения-и-риски)
- [Правила исполнения](#правила-исполнения)

## Актуальная политика исходников — 2026-10-13

User authority: «мы КОПИРУЕМ крейты из warpbro и других проектов».
Физически копировать выбранные необходимые crates и own-project dependency closure
Warpbro, egui-widgets-rs (widgets/gizmo), остальных cglibs и StoryTold в `sim-widget-rs`.
Весь Warpbro UI переносится и подключается к нашему wgpu viewport. Прежние SSH-only
и Warpbro-UI-only exception rulings superseded; исторические audits/receipts сохраняются.
Own-project зависимости замыкаются на local vendored paths. Frozen source commits,
source/copied SHA256, licenses/NOTICE/assets/features и local patches обязательны.
**crates.io не вендорить**: normal registry packages + Cargo.lock/checksums.
Donor repos/cache не изменять, чужие dirty edits сохранять. Копирование не закрывает
full 18-task, native/browser, single-authority, performance или budget gates.

## Структура workspace

Preflight архитектура принята до первой реализации. Root package — публичный facade;
workspace children появляются с реальной функциональностью. A–N — пакеты работ, а не 14 пустых crates.

```text
sim-widget-rs/
  Cargo.toml / Cargo.lock / rust-toolchain.toml
  src/lib.rs                  # facade: sim-runtime; sim-project после реализации
  crates/
    sim-model/src/             # document, IDs, parameters, diagnostics, migrations
    sim-math/src/              # operators, tensor, autodiff, provenance; depends model
    sim-runtime/src/           # session, time, commands/history, presentation; model/math
    sim-project/src/           # план: package/assets/persistence/export; depends model
    sim-render/src/            # план: scene/input/GPU views; runtime + render-rs
    sim-author/src/            # план: editors; runtime/project + craft-ui/egui
  apps/
    sim-web/src/               # план: depends facade; browser lifecycle/bindings
    sim-native/src/            # план: depends facade; native lifecycle
  vendor/storytold/             # физические copies выбранных donor crates
  vendor/cglibs/<source>/       # selected Warpbro/egui/other own crates + closure
  provenance/{manifest.json,NOTICE,patches/}
  fixtures/{cosines,canonical,assets}/
  tools/                       # плановые build/package команды
src/viz/rust/                  # Astro/React/DOM host adapter, если выбран
src/components/RustExplainer.astro
src/content/docs/dev/law-of-cosines-rust.mdx
src/content/docs/ru/dev/law-of-cosines-rust.mdx
public/sim-widget-rs/          # план: generated WASM/JS/resources, base-aware URLs
```

RU source placement проверяется при реализации по существующим маршрутам.
Нельзя переносить current src/core в Rust путём создания второго независимого evaluator.
Если legacy-документу нужны TS операции, host предоставляет явно выбранный bridge.
Новые документы используют Rust authority. Root facade не зависит от web/native приложений.
Embedded learner и author получают отдельные WASM artifacts: dynamic import не разделяет
монолитный WASM binary, Cargo features не доказывают lazy loading без проверки artifacts.

R03-ARCH-B1 принят в [architecture gate](../../.superpowers/sdd/sim-widget-rs/task-03-architecture-gate.json):
widget-owned shared GpuHost/pass graph + неизменённый public `standard_surface_bsdf::wgsl_source()`
и собственный raster wrapper с <= 4 bind groups (frame/object/material/lighting).
Chrome maxBindGroups=4 исключает fixed5 standard-surface pipeline. Donor shader не переписывается,
второй BSDF/private Renderer3D fork/cache patch не создаются. Реальные GPU/draw/wireframe/picking,
lifecycle, material parity и бюджеты ещё проходят приёмку; схема интегрируется после принятия task 02.

## Контракты и поток данных

```text
pointer / keyboard / author edit / host set
                    |
                Command
                    |
          validate -> transaction -> history
                    |
        document + logical time + session events/seed
                    |
       authored tracks -> clip/layer blend -> live overrides
                    |
       validate effective inputs / final constraints
                    |
       single Rust evaluator -> FrameState truth/provenance
                    |
      presentation spring / labels / camera
                    |
          renderer + linked DOM/GPU UI
                    |
       hover / selection / guide by semantic ID
```

Logical time управляет состоянием объяснения. Wall clock нужен playback scheduler.
Presentation clock управляет сглаживанием отображения. Прямой drag не проходит через spring.
Derived numbers и проверки задач всегда читают truth, а не промежуточную presentation value.

Предложенный декларативный документ; это schema sketch, не runnable API:

```text
Document {
  schema_version, document_id, locale_refs,
  objects[], parameters[], operators[], bindings[],
  tracks[], clips[], compositions[], chapters[],
  state_machine?, asset_manifest[], learner_permissions
}
Parameter { id, type, unit, range, step, default, lock, readonly }
Binding { source_value_id, target_property_id, transform_operator_id? }
Command { transaction_id, expected_revision, kind, target_ids, payload }
Event { session_id, sequence, logical_time, kind, payload }
FrameState { revision, time, truth_values, scene_objects, provenance, diagnostics }
```

Документ хранит данные и зарегистрированные operator IDs.
Загруженный проект не выполняет произвольный Rust/JS из выражений.
Plugins устанавливаются отдельно и объявляют supported schema/operator capabilities.
Подмена отсутствующего operator фиктивным результатом запрещена.

IDs namespaced document + composition instance; object/term/value/operation IDs раздельны.
Hover передаёт semantic ID, не отображаемый label. EffectCraft Tick=254,016,000,000/sec
не является публичным time type. Checked `TimeTicks` через JS boundary сериализуется
десятичной string/BigInt, не Number с потерей целочисленной точности. Rational conversion,
overflow/nonfinite/division errors проверяются до donor adapter; silent repair запрещён.
Artist expressions EffectCraft не заменяют typed math evaluator/provenance.

Public boundary, план:

- load(bytes, resolver) → validated document либо адресные diagnostics.
- dispatch(command) → revision/change set либо validation error, без частичной записи.
- seek(time, session_history) → FrameState; для stateless tracks history не требуется.
- setLiveOverride(parameter_id, value) → pause playback + validated override.
- returnToGuide(ids) → снять overrides по объявленной политике и пересчитать.
- select/hover/guide(ids) → независимые состояния UI; не изменить вычисления.
- savePackage/exportSnapshot() → versioned bytes/manifest.
- dispose() → отменить задачи и освободить resources; поздние callbacks игнорируются.

При WASM crossing передавать команды и изменения пакетами.
Большие GPU assets и постоянные arrays остаются в runtime; dirty ranges измеряются.
Для новых документов используется Rust evaluator; legacy bridge выбирается один раз на документ.
Author document revision/history и session revision/event log имеют разных владельцев.
Hover/selection/camera относятся к view state и не создают mathematical Undo operations.

R02-10 (coordinator, 2026-10-12): Reset values записывает текущие authored defaults как live
overrides и ставит playback на pause; permissions/ranges/final constraints проверяются атомарно.
Return to guide снимает overrides и samples tracks на текущем logical time. Author set редактирует
defaults документа с history; отдельной immutable factory baseline нет. Ошибка reset не меняет state.

### Обязательный interaction preview — актуальный приоритет Tasks 03/04

После работающего preview пользователь потребовал draggable все точки/стрелки/manipulators,
реальную 3D orbit/pan/zoom как Houdini, простую глобальную смену языка и сглаженные края.
Это меняет порядок обязательного proof, сохраняя полный scope 03/04 и всех 18 задач.
Реальный renderer использует wgpu 30/WebGPU; наличие backend не закрывает input/AA/UI gates.
[Interaction contract](../../.superpowers/sdd/sim-widget-rs/task-03-interaction-contract.json)
содержит solver/input/locale/AA сценарии; последующее coordinator authorization ниже заменяет
его старые pending-approval поля, не переписывая исторический JSON.

| Работа | Зависимость / output | Обязательное доказательство и независимый review |
| --- | --- | --- |
| Geometry/handles | Existing scalar authority `origin+a/b/theta`, raw `u_raw/v_raw` → declared Gram–Schmidt graph → unit basis; reusable Rust inverse proposal, atomic patch | A-only сохраняет O/B; B-only O/A; O-only A/B; group translation сохраняет shape. Все points/arrow tips/shafts и axis/plane/rotation/angle tools имеют заявленную semantic operation. Нет B-axis lock; named residual tolerances и native/browser cases проверяет math/input reviewer. |
| Gesture state | Reviewed mixed override patch + bounded Task 04 InputCoordinator; explicit playback ownership | One gesture/one interaction Undo; Some/None восстанавливает точное наличие override/source, cancel добавляет compensation с monotonic events/revisions. Bounds/permissions/conflicts не дают partial writes; author Undo и stale whole-session restore не заменяют input history. |
| Camera | Единый complete ViewState; активный embedded/focused-lab profile | Space или Alt + LMB tumble/MMB pan/RMB dolly, Shift precision, wheel policy и view-fit. Modifiers до picking; fields/contenteditable/IME не перехватываются, camera не меняет math/history. Проверяет input/browser reviewer. |
| Locale | Shared extensible catalog + host locale/global switch; EN/RU first | Все text/hints/errors/ARIA/units/number formats и dynamic controls меняются вместе; IDs/core/radian units неизменны. Third catalog registration, placeholder parity и switch без math mutation проверяет locale/accessibility reviewer. |
| AA/resources | Supported MSAA4 pipelines/color/depth/resolve + owned analytic/SDF stroke/point coverage | Инструментированные sample counts, allocation union, DPR1/1.25/2, fractional-motion edges и resize/loss; public BSDF unchanged. Native/browser image/resource reviewer; screenshots и Naga validation недостаточны. |

Raw generators не являются готовым orthonormal basis: нулевой/зависимый authored input
выдаёт typed error без silent repair. При допустимых theta 0/pi вырожденная polygon fill
пуста, реальные edges/points остаются; stop/reset/recovery должны работать после failure.
Forward geometry/formula/provenance всегда пересчитывает Session, JS только переносит gestures/DOM.
Picking, labels и geometry используют один опубликованный shown/prepared frame; direct drag
cut affected presentation. Explicit ray/plane/axis constraints задают глубину 3D движения.

Navigation source: [SideFX viewing controls](https://www.sidefx.com/docs/houdini/basics/view.html)
и [hotkey reference](https://www.sidefx.com/docs/houdini/start/hotkeys.html).
Widget profile поддерживает Space/Alt формы; focused H/F доступны только viewport, embedded wheel
сохраняет page scroll. Suppress browser defaults только для consumed gestures; clear keys on blur.

**Исторический ownership gate (до fix1 acceptance):** renderer product writes FROZEN в
[pre-core manifest](../../.superpowers/sdd/sim-widget-rs/task-03-interaction-pre-core-freeze.json).
Original runtime implementer — sole product writer bounded `PatchLiveOverrides` с `Some/None`
и playback policy. Разрешена `SESSION_VERSION=2` с migration реальных V1 session fixtures,
без document schema/operator changes. Историческая Task 02 acceptance относится к прежнему
snapshot/v1 wire. Новый patch требует отдельного different-model review до возврата renderer ownership.
Конкретные exported names и acceptance записываются после review, не объявляются работающими заранее.

### CURRENT TODO — interaction proof перед полной renderer приёмкой

- [x] Scoped atomic override/V2 prerequisite принят после fix1; original renderer owner снова sole product writer. Full Task 04 не закрыта.
- [ ] Подключить все manipulators и Houdini navigation к реальным browser gestures с bounded cancel/Undo.
- [ ] Исправить theta 0/pi fill collapse и recoverable stop/reset без stale frame/formula.
- [ ] Подключить global catalog/host locale без inline EN/RU пары и непереведённых ARIA/errors/units.
- [ ] Доказать actual MSAA4/analytic AA, DPR/resource lifetime и unchanged BSDF.
- [ ] Повторить source/artifact-bound browser/native proof и full payload budget; все 03/04/18 gates открыты.

### Принятый core prerequisite и общий picking contract

[Durable atomic override acceptance](../../docs/acceptance/sim-widget-rs/task-04-atomic-override-prerequisite.md)
закрывает только bounded core patch после fix1: 79 native tests, allocator peak192 B вместо65536 B
при event ceiling192 B и executed native/WASM reports157,831 B с одинаковым hash.
Исходный FINDINGS R1/R2 и историческая Task 02 v1 приёмка сохранены отдельно.
SESSION_VERSION2 с реальными V1 migrations принят; document schema/operators не менялись.
Original runtime writer завершил работу; original renderer owner получил sole product ownership.

[Shared picking design](../../.superpowers/sdd/sim-widget-rs/task-03-shared-picking-design.json)
и [review](../../.superpowers/sdd/sim-widget-rs/task-03-shared-picking-design-review.json)
имеют APPROVED_FOR_IMPLEMENTATION, не implemented acceptance. Один typed resolver visible
shown/prepared geometry обслуживает hover/press/drag start в 2D/3D ortho/perspective.
Полный stamp включает runtime generation/document/session/epoch/revisions, scene/topology,
view/layout/device generation и displayed sample; stale/foreign results отвергаются.
Qualified physical object/subpart/instance IDs отделены от semantic ID. Clip/filter/occlusion
идут перед priority, затем CSS distance/world depth/stable ties; x-ray/transparent/masked
и selected overlay — явные typed policies.
Bounded BVH refits/rebuilds actual transformed bounds; CSS slop broadphase не пропускает thin
hits, exact local narrowphase сохраняет world ray t/nonuniform scale/negative winding и
barycentrics. Нельзя каждый pointermove сканировать весь imported mesh.
Near-plane crossing, occluded handles и deterministic subpart ties — конкретные prototype
findings, которые нужно закрыть source-bound native/browser regressions, включая multiview,
async stamps, capture/compensation conflicts и measured index/query/resource bounds.

Coordinator разрешил отдельное compiled AA artifact из scratch copy accepted Task 02 v1 core
во время ownership freeze, без unreviewed V2 API/product writes. URL
[cosines-aa.html](http://127.0.0.1:43874/cosines-aa.html) подтверждает sample4, canonical outputs,
2D/3D, fields/play/seek и 0/pi reset recovery. Scratch fixes не принимают production collapse
path. Direct manipulation/navigation и весь catalog ещё не опубликованы; WASM323,366 B gzip
выше300,000 B. Native numeric AA6cases отдельно; browser visual/DPR/resource/lifecycle gates
остаются открытыми. Static-reference policy block не обходился: trial — actual compiled renderer.

### Reuse prerequisites внутри Tasks 03/04 — egui-gizmo и author widgets

[Полный catalog/source audit](../../docs/acceptance/sim-widget-rs/egui-widgets-reuse-source-audit.md)
и accepted audit correction не являются runtime adoption. Manifest сохраняет18 последовательных
задач; prerequisites ниже входят в03/04 и последующий07-author, без scope/budget waiver.

- [ ] Для полного manipulator scope переиспользовать implemented `egui-gizmo` kernel через
  additive neutral public API: stable typed handles/geometry/draw data, external shared-pick
  candidates, checked f64/ray/clip boundary и begin/update/end/cancel/abandon. Сохранить
  compatible egui facade над тем же kernel; не писать новый rotate/scale engine ради private API.
- [ ] До local vendored edits координатор задаёт отдельный реальный task/owner для egui-gizmo public
  seam и точных files; local vendored writer, consumer writer и independent reviewer имеют явные
  ownership boundaries. Required gates: существующая facade regression, public neutral host,
  navigation/picker/cancel conflicts, native/WASM targets и reviewed snapshot. Здесь local patch
  ещё не создан; latest copy authority разрешает vendored source и reviewed local patches,
  но не private calls/cache edits или изменение donor working tree.
- [ ] После independent different-model review local vendored изменения фиксируются source
  baseline/patch hashes/provenance; consumer adoption проверяет public exports, local closure,
  registry lock, shared host pass,
  clip/depth/DPR/gesture cancellation и реальные bytes/peak/p95. Audited96fc source snapshot
  остаётся историческим baseline, не подменяется новой HEAD.
- [ ] Для07-author выбирать нужные packages: attr-grid/slider/colorpicker/value-editor,
  timeline/keyframes/track-timeline/ramp, outliner, command-palette/tab-menu/menubar и chrome/
  layout/status/toast/help. Каждая выбранная closure проходит target/resource/locale proof;
  все50 libraries и demo не становятся обязательными dependencies.
- [ ] Session author truth/history, InputCoordinator gesture history, ViewState camera и
  stamped picker/GpuHost остаются единственными owners. Optional OTIO/scopes/video encoding
  сохраняются будущими professional author options; текущий cosines priority их не отменяет.

Tasks03/04 закрываются только после полного manipulator/input/browser acceptance, а не по
catalog source proof. Task07/14 отдельно доказывают author UI и learner artifact isolation.

## Зависимости пакетов работ

```text
A sources/toolchain -> C document/commands/evaluator
        |                  |
        +-> B risk spikes -+-> renderer/shell contract
                                       |          |
                                       v          v
                                  D renderer     F time/states
                                       |          |
                                       v          v
                                  E input        G math/data
                                       +----+-----+
                                            v
                                       H author UI
                                            |
                      I assets ----+--------+---- J package/export/audio
                                   |                 |
                                   +--------+--------+
                                            v
                                    K web/native/embed
                                            |
                                    L copied cosines
                                            |
                                    M canonical coverage
                                            |
                                    N full acceptance
                                            |
                                   migration planning
```

C pure foundation начинается после A; renderer исследования B не блокируют headless core.
I базовый resolver можно начать после C; format adapters требуют D/G data contracts.
J package/schema начинается после C, frame export ждёт F/D.
K lifecycle/input prototypes выполняются в B, окончательная оболочка зависит от H/J.
L создаётся ранним вертикальным примером и расширяется до полного испытания при N.

## Задачи исполнения

Один full release; внутренние волны закрывают зависимости. Ни foundation, ни pilot не являются MVP
или завершением системы. Все checkbox A–N сохраняются как детальные критерии ниже.
Каждая задача имеет отдельный brief: owner/files, dependency interfaces, outputs, commands и review scope.
При отсутствии skill template используются эти briefs и журнал, без выдуманных путей scripts.

Reviewer каждой строки — независимый агент другой модели относительно implementer.
До закрытия задачи записать обе model IDs, snapshot/revision, собственные команды reviewer,
результаты и исправленные замечания. Самопроверка не закрывает колонку review.
Некоторые acceptance IDs проходят через несколько задач: задача закрывает только свою явно
указанную часть, а весь gate остаётся открытым до проверки полного сценария. Foundation wire
tests не закрывают browser CORE-08 или полный SOURCE-02/extension API.

| Задача | A–N | Зависимости | Выход и полный scope задачи | Приёмка / независимый reviewer |
| --- | --- | --- | --- | --- |
| 01-foundation | A, C base | AGENTS, source audit, preflight | Real model/math/runtime children, facade, checked IDs/time, scalar graph/provenance, atomic commands; StoryTold closure/provenance and lock | CORE-01/03/04/07/08, SOURCE-01/02; architecture + math reviewer |
| 02-runtime | C, F base | 01 | Typed parameters/units/shapes, validation/migrations, revision/history, single authority, override/constraint ordering, seek/events/session boundaries | CORE-02/03/05/09/10/11, TIME-01/05/08; runtime reviewer |
| 03-render | B, D | 01, FrameState 02; reviewed egui-gizmo public seam before full gizmo adoption | Browser/native capability trial; hierarchy/cameras/primitives, pixel text/lines, multiview/materials/lights/WGSL/effects, optional compute/pathtrace module with declared matrix; prioritized interaction/AA proof above | VIEW-01/02/03/07/08/09/10/11/12/13, LIFE-02, PERF/SIZE; render + visual reviewer |
| 04-input | E | 02/03; reviewed core patch + egui-gizmo public seam before adoption | Picking/all manipulators/drag/gizmos/snap, bounded override cancel/one interaction Undo, Houdini profiles, keyboard/touch/IME/focus, exact fields, resets, menus/search/keymap, adaptive docking/theme; shared locale service | INPUT-01–14, CORE-12, LANG-02, A11Y-01; input + accessibility reviewer |
| 05-time | F | 02, presentation 03 | Tracks/Bezier/value-speed curves, camera paths, clips/NLA/additive layers, chapters/branches/replay, fixed-step PRNG/checkpoints, bounded seek/springs | TIME-01–08, CORE-08; time + determinism reviewer |
| 06-math-views | G | 02/03/05 | Tensors/strides/rounding/bitfields, matrix ops, actual autodiff, plots/fields/integrals/Jacobian, probability/attention, images/UV, formula/code/term links | CORE-05/06, VIEW-04/05/06; numerical + linked views reviewer |
| 07-author | H | 04/05/06; project API 12 | Scene palette/tree/inspector, graph + curve + multitrack editors, templates/instances/permissions, chapters/quizzes/hotspots/captions, preview/search/layout/history | AUTHOR-01–11, INPUT-01–07; editor workflow reviewer |
| 08-assets | I resolver | 02/03 | Async relative/package/file resolver, cache/progress/retry/budgets, cancellation/generation IDs, units/up-axis and dependency diagnostics | ASSET-04/05/06, LIFE-01; resource lifecycle reviewer |
| 09-gltf | I GLB/glTF | 08 | Materials/textures/animation/skinning/morph with licensed samples, stable IDs and format lazy boundary | ASSET-01, VIEW-07; format reviewer |
| 10-alembic | I ABC | 08/05 | Ogawa byte importer, sample interpolation/changing topology and reversible geometry seek | ASSET-02, TIME-01; cache/time reviewer |
| 11-usd | I USD | 08/09 | Layers/variants/refs/payloads/schema/material matrix, browser resolver/package textures, explicit unsupported diagnostics | ASSET-03/05/06; USD compatibility reviewer |
| 12-project | J persistence | 02/08 | Portable versioned archive, migrations, relative assets, clipboard/duplicates/state links/snapshots, OPFS/IndexedDB/native, conflict/recovery journal | SAVE-01–04, SOURCE-02; persistence reviewer |
| 13-export-audio | J export/audio | 03/05/12 | PNG/SVG declared views, deterministic frames/video with codec matrix, audio/captions resources and seek/pause/speed sync | EXPORT-01/02, AUDIO-01; export/time reviewer |
| 14-shells | K, B shell verdict | 04/07/12/13 | Separate learner/author WASM, full lab/native, host API/lifecycle, lazy SSR Astro/base paths, worker coexistence, accessibility/global host locale/dispose | LIFE-01–06, CORE-08/10, LANG-01/02, A11Y-01, BUILD-01; browser/native reviewer |
| 15-cosines | L | 06/07/14 | Isolated EN/RU dev copy, independent IDs/history, LC overview/detail/proof/shear/turn/IK, full author→save/open→embed workflow | CORE-01, all pilot criteria; math + visual workflow reviewer |
| 16-canonical | M scenes | 06/09/10/11/14 | All ten canonical scenes, actual math and provenance, author/learner/embed/replay roundtrip | Canonical coverage + relevant CORE/VIEW/TIME; course reviewer |
| 17-coverage | M map | 15/16 | Mapping every one of 98 entries to supported capabilities and closed concrete gaps; extension API/examples and all format/platform matrices | Complete feature rows A–M; completeness reviewer |
| 18-full-acceptance | N | All 01–17 | Independent locked native/WASM/browser and integrated course gates, performance/memory/size evidence, visual/input trial, license audit, user full-version trial | Every Phase 05 gate closed; release reviewer of different model |

01/02 могут идти до окончания B measurements; 03/14 требуют результатов B для GPU/shell.
07 использует согласованный project contract, но AUTHOR-11 закрывается только после 12/14.
Evidence хранится в root docs/acceptance; [журнал](../../sim-widget-rs/docs/implementation-status.md)
показывает фактическое состояние каждой задачи и её независимого review.

## A — подготовка и источники

Зависимости: текущие планы, AGENTS, reuse audit. Выход: воспроизводимые origins и workspace.

- [ ] Прочитать полные AGENTS/HANDOFF и принятый visual contract 2026-10-12 без Holo для нового widget/pilot.
- [ ] Проверить git/worktree state; определить ownership новых файлов и source boundaries.
- [ ] Зафиксировать source repository/path/immutable commit выбранных Warpbro/egui/cglibs crates.
- [ ] Выбрать required Warpbro/egui/cglibs/StoryTold crates по API; включить own-project closure.
- [ ] Физически копировать selected own crates, весь Warpbro UI и required manifests/licenses/NOTICE/shaders/assets/build resources.
- [ ] Адаптировать workspace inheritance и local paths, не оставляя external donor references.
- [ ] Записать provenance manifest: source commit/hash, licences, copied hashes, local patches.
- [ ] Оставить crates.io обычными versions; создать Cargo.lock без registry vendoring.
- [ ] Зафиксировать Rust toolchain/target/wasm CLI; matching wasm-bindgen crate/CLI при его выборе.
- [ ] Подключить planned Rust build/package к Bun/Astro static pipeline; проверить package scripts.
- [ ] Описать update protocol frozen snapshots/local copies/patches; source repos не редактировать.

Готово: metadata указывает локальную own-project closure, provenance содержит source commits/hashes;
registry resolution ожидаем без crates.io vendoring.
Блокеры: неизвестные URLs/licenses, incompatible toolchains, ownership conflicts.

## B — исследования риска

Зависимости: A. Выход: записи measurements/decisions, выбранные integration contracts.

- [ ] Собрать минимальный browser/native renderer с выбранными cglibs features.
- [ ] Проверить wgpu capability и WebGL2 subset; не считать fallback compute-equivalent.
- [ ] Измерить WASM+JS/fonts/shaders cold load, gzip, memory и frame-time distribution.
- [ ] Сравнить DOM+viewport и egui/eframe+craft-ui на одинаковом author/learner экранe.
- [ ] Проверить формулу KaTeX/GPU text alternative, русский текст, IME, clipboard, screen reader.
- [ ] Проверить несколько карточек, clipping/scissor, scroll/resize/DPR/fullscreen.
- [ ] Прототипировать pixel-stable lines/labels, спокойную читаемую сцену, плавность и UI по контракту без Holo.
- [ ] Проверить worker policy independent/shared memory, SSH/static host constraints и headers.
- [ ] Прототипировать reversed seek/event history/checkpoint без renderer dependency.
- [ ] Зафиксировать target device/browser matrix, условия измерения и критерии качества.

Исходный целевой ориентир движения — 60 FPS на выбранной матрице, пока не доказан.
Определить frame budget/p95/input delay и допустимую scene complexity после измерений.
300,000 B gzip renderer gate не повышать молча; WASM byte ownership требует явного решения.

## C — документ, команды и вычисления

Зависимости: A и preflight core contracts; B нужен для GPU/shell boundary. Владение: model/math/runtime.

- [ ] Создать schema_version, стабильные semantic IDs и references между scene/math/UI.
- [ ] Реализовать typed parameters/ranges/units/enum/matrix/file, defaults и readonly/lock.
- [ ] Проверить uniqueness/missing refs, допустимые cycles, units/shapes и asset references.
- [ ] Создать atomic commands, revisions, transactions, undo journal, cancel checkpoints.
- [ ] Реализовать dirty propagation и derived values; invalid command не оставляет partial state.
- [ ] Создать operator registry, input/output contracts, typed diagnostics и provenance.
- [ ] Rust evaluator — authority новых документов; legacy TS bridge сохраняет текущий арифметический контракт.
- [ ] Планировать Rust tensor/einsum/dtype эквивалент через exact fixtures перед сменой authority.
- [ ] Добавить реальный autodiff отдельным операторным пакетом; сейчас src/core autodiff не содержит.
- [ ] Ввести schema migration tests и неизвестные capabilities без silent repair.

Готово: один числовой результат объясним до operand/operation; Undo возвращает модель.
Ошибки содержат parameter/object/operator ID и location для inspector/graph.

## D — сцена и renderer

Зависимости: C FrameState, B device contract. Владение: scene/render.

- [ ] Scene hierarchy: local/world transforms, pivots, parenting, units и up-axis.
- [ ] Multi-view cameras: ortho/perspective, home/fit-selected, presets, user camera overrides.
- [ ] Primitives: points/segments/arrows/arcs/polygons/grids/meshes/trajectories.
- [ ] Plot/field/surface buffers с deterministic adaptive sampling и nonfinite diagnostics.
- [ ] Materials/lights/textures/environment/instancing/transparency/custom WGSL registry.
- [ ] Pixel lines/text/labels, declutter, focus outlines, clipping/section/A-B views.
- [ ] Renderer resource ownership, device restore и stale-frame discard.
- [ ] Lazy optional pathtrace/compute capability module; budgets и subset явно объявить.

Готово: сцена воспроизводит FrameState, resize не меняет математику; visual reference принят.
Качество меняется только через явные controls/presets, без hardware-driven omissions.

## E — управление и качество UI

Зависимости: C/D. Владение: input/shell interaction; shell выбор определяет widgets.

- [ ] Picking по semantic IDs, отдельно hover/selection/guide; multiple selection и sets.
- [ ] Drag gizmos/constraints/snap/modifiers, CSS pixel hit sizes и direct response.
- [ ] Pointer capture/cancel/Esc rollback, одна drag transaction, no accidental cancel-click.
- [ ] Camera orbit/pan/zoom с согласованными shortcuts и scroll/touch arbitration.
- [ ] Typed fields: scrub/exact expression/unit/step, dependent bounds, multi-edit и reset.
- [ ] Keyboard focus/ARIA или tested accessibility bridge, remappable keymap.
- [ ] Context menu/tooltips/search palette; команды используют общий dispatch/history.
- [ ] Docking/layout presets/restore/themes; adaptive phone composition и reduced motion.
- [ ] Раздельные reset view/reset values; retain state при wide/full transitions.

Готово: мышь, клавиатура и touch выполняют одинаковые semantic edits.
Противоречащие constraints дают адресную ошибку, не дрожание или скрытый clamp.

## F — время и интерактивные ветки

Зависимости: C, presentation hooks D. Владение: runtime time/tracks/states/simulation.

- [ ] Logical scheduler: play/pause/step/back/speed/loop/in-out/chapters, no reader autoplay.
- [ ] Tracks/keyframes/Bezier/hold/ease; camera paths и value/speed evaluation.
- [ ] Clips/groups/NLA, relative/additive layers и документированный precedence.
- [ ] Constraint/override order; reader edit pauses, return-to-guide снимает заданные overrides.
- [ ] State machine/events/branches, recorded input log, replay и snapshots.
- [ ] Seeded stochastic ops; integrator fixed step/checkpoints/PRNG state/seek cost limits.
- [ ] Presentation springs independent of truth, stable retarget и reduced motion.
- [ ] Optional audio clock alignment с chapter/pause/speed/seek policy.

Готово: повторный forward/back seek совпадает по truth; interactive replay требует history.
Перестройка UI и FPS не меняют sample sequence или derivative values.

## G — математика и данные

Зависимости: C/F/D. Владение: math + linked view contracts.

- [ ] Semantic formula/derivation reveal/morph, number boxes и geometry links.
- [ ] Tables/heatmaps/selected cell, matrix transforms/determinant/inverse diagnostics.
- [ ] Tensor slices/strides/flat memory/broadcast/reshape, dtype rounding/bitfields trace.
- [ ] Curves/tangent/integral areas, scalar/vector fields/contours/Jacobian surfaces.
- [ ] Probability samples/histograms/statistics с seed and observation provenance.
- [ ] Реальный autodiff graph, chain-rule stages, gradients и update step.
- [ ] Attention graph/table/cell mapping и real softmax intermediate values.
- [ ] Images/UV/channels/sample links, code line links и term genealogy.
- [ ] Layout APIs для large graph/data views, measured LOD с явным user control.

Готово: hover числа находит объект/operator/code; данные совпадают с chosen evaluator.
Читатель видит причины singular/undefined cases без вымышленных результатов.

## H — визуальный редактор

Зависимости: C–G, выбранный shell B. Владение: author.

- [ ] Palette/presets/scene tree, hierarchy/group/duplicate/lock/isolate/layers.
- [ ] Inspector schema-generated controls, bindings/expressions/constraints, errors.
- [ ] Multitrack timeline keys/clips/groups/markers/ranges/zoom/snap/layers.
- [ ] Value/speed curve editor, tangents, interpolation, retiming и camera paths.
- [ ] Computation node editor connects actual operators; units/shapes/cycles diagnostics.
- [ ] Compositions/templates/instances, exposed parameters и reusable choreography.
- [ ] Chapter/step editor, guidance/hotspots/quizzes/captions/optional audio tracks.
- [ ] Learner permissions and preview; автор проверяет ровно тот же runtime.
- [ ] Workspace/layout persistence, search, commands, Undo/cancel и history inspection.
- [ ] Export/embed action validates dependencies and produces same project document.

Reuse: craft-ui local copies; EffectCraft keyframe/time local copies;
EffectCraft editors/history adapted к нашему document вместо fork всего app.

## I — assets и форматы

Зависимости: C/D, resolver используется J. Владение: assets.

- [ ] Async resolver: relative URL, embedded package, browser file, native file, dependency graph.
- [ ] Cache/progress/cancel/retry/budgets; смена документа invalidates task results.
- [ ] GLB/glTF materials/textures/meshes, animation/skinning/morph targets.
- [ ] Ogawa ABC from bytes, time samples, topology changes, units/up conversion.
- [ ] USD compatibility matrix: layers/references/payloads/variants/schemas/material subset.
- [ ] USD browser resolver/textures/package dependencies; unsupported feature diagnostics.
- [ ] Formats отдельно lazy; fetch uses Astro base-aware URLs, а не root absolute paths.
- [ ] Предусмотреть missing resources/corruption/oversize и explicit user load policy.

Готово: импорт даёт scene model со stable IDs, реальными time samples и asset attribution.
Неподдержанный shader/schema не считается полной поддержкой формата.

## J — хранение, экспорт и звук

Зависимости: C/I; frames require D/F. Владение: project.

- [ ] Versioned portable archive, schema/capability/assets manifest и relative references.
- [ ] Save/load/duplicate/clipboard/snapshots/state links; migrations preserve IDs.
- [ ] Browser persistence OPFS/IndexedDB после quota/concurrency проверки; native local storage.
- [ ] Multi-tab write policy, recovery journal и incomplete-write detection.
- [ ] PNG snapshot; SVG supported plot/vector views с явной supported-feature matrix.
- [ ] Deterministic frame sequence; browser/native video paths и codec matrix.
- [ ] Audio/captions resources, optional synchronization and license attribution.
- [ ] Package excludes editor implementation but preserves document semantics/permissions.

Готово: portable project открывается без donor paths; missing external assets перечислены.
Экспорт Frames не зависит от скорости real-time playback.

## K — оболочки и embedding

Зависимости: H/J, B shell verdict. Владение: shell/web/native/learner.

- [ ] Embedded lifecycle, load/unload/cancel/error/retry, hot/selection/time events.
- [ ] Full-window lab и author workspace; один документ, разные capabilities.
- [ ] Native shell использует Rust напрямую; browser shell использует WASM.
- [ ] Astro wrapper SSR sizing/skeleton/caption/accessibility до lazy runtime hydration.
- [ ] Respect base /math-course для WASM/worker/assets; static Pages без backend.
- [ ] Pyodide/ELK workers coexist без eager engine import на страницах без сцен.
- [ ] Host API commands/events, model authority bridge и serializable inputs.
- [ ] Resize/visibility/fullscreen/focus/scroll, multiple instance ownership.
- [ ] Runtime dispose освобождает GPU/listeners/workers и отменяет async work.

Готово: одинаковый документ author preview/learner/embed; editor не входит в ordinary lesson payload.

## L — копия косинусов

Зависимости: vertical C–K subset, завершение при N. Владение: pilot fixture/MDX sandbox.

- [ ] Скопировать EN/RU source только в dev routes; проверить реальную структуру RU файлов.
- [ ] Новые document/store/card/timeline IDs и отдельные assets; main sidebar/progress не изменять.
- [ ] Перенести LC-0 overview/detail и их совместное управление объяснением.
- [ ] LC proof: signed strips, shear area preservation, rigid turn и linked formula cancellation.
- [ ] LC 0/1/2/3 включая two-bone IK reachable/unreachable и degenerate cases.
- [ ] Числовые примеры a=3,b=4: 60°→13, 90°→25, 120°→37 для c².
- [ ] Author build/bind/animate/chapter/preview/save/open/embed full workflow.
- [ ] Side-by-side оригинал/копия: независимые state и history, same mathematical meaning.
- [ ] EN/RU parity, formal «вы», keyboard/touch/reduced motion и visual reference check.

Главный trial пользователя — этот урок; canonical fixtures дополняют его, не заменяют.

## M — покрытие курса

Зависимости: полный C–K. Владение: fixtures + acceptance map.

- [ ] Dot/cosines; matrix determinant/inverse; tensor strides/rounding.
- [ ] Derivatives/finite differences; Jacobian surface; seeded probability.
- [ ] Real backprop; attention cell; two-bone IK/Fourier; quantization bitfields.
- [ ] Для каждой сцены author/learner/embedded save/open/replay и provenance.
- [ ] Сопоставить 98 entries completion map с primitives/operators/views/guidance requirements.
- [ ] Зарегистрировать недостающее как конкретный work item; не заменить fake illustration.
- [ ] Проверить все feature groups по [приёмке](phase-05-acceptance.md).

## N — завершение полной версии

Зависимости: все пакеты A–M и closed blockers.

- [ ] Complete feature/acceptance matrix, explicit format/platform compatibility.
- [ ] Пройти scene, input, seek/replay, author, package/export и lifecycle acceptance.
- [ ] Подтвердить budgets/frame-time/visual/accessibility/locale на выбранных устройствах.
- [ ] Проверить licenses/provenance/Git rev/lock и documented extension API.
- [ ] Исправить failures и перепроверить только затронутые gates.
- [ ] Дать пользователю full-version trial на копии косинусов.
- [ ] После принятия составить отдельный migration plan действующих уроков.

## Решения и риски

| ID | Решение/риск | Разрешить | Влияние |
| --- | --- | --- | --- |
| R1 | DOM vs egui shell | B identical scenario comparison | H/K widgets, formulas/accessibility |
| R2 | Renderer 300 kB gate vs WASM | B ownership measurements + user decision if needed | Pipeline/lazy structure |
| R3 | GPU/browser matrix | B selected devices/features | Fallback/effects/exports |
| R4 | Worker memory/headers | B host and shared-memory spike | CPU simulation/import paths |
| R5 | Mathematical authority | C bridge decision + exact fixtures | G values/provenance |
| R6 | Interactive time precedence | C/F specification | Replay/override/Undo semantics |
| R7 | Imported formats completeness | I explicit matrix | Assets diagnostics and expectations |
| R8 | StoryTold app coupling | A/H adapters | Avoid duplicate Project/history authorities |
| R9 | Persistence multi-tab | J conflict policy | Save safety |
| R10 | Video codec/browser limits | J tested export paths | Format compatibility |
| R11 | New widget/pilot visual contract | Resolved by user 2026-10-12: no Holo; quality measurements pending | Readability, smooth motion and pleasant Blender-like UI; full features retained |
| R12 | Local vendored closure/source commits/hashes/licenses | A actual source records | Build reproducibility |
| R13 | Native toolchain prerequisites | A/B platform build records | Packaging |
| R14 | Label/text accessibility quality | B/E/G measured trial | Usability of every scene |

### Принятые preflight решения

| ID | Решение от 2026-10-12 | Обязательство реализации |
| --- | --- | --- |
| D1 | Compact DAG: model→serde/thiserror; math→model; runtime→model/math/keyframe adapter; project→model; render→runtime/render-rs; author→runtime/project/craft-ui/egui | Root facade→runtime/project; web/native→facade, обратных зависимостей нет; crates создаются с работающими API |
| D2 | Effective inputs до evaluator | Tracks/layer blend→live overrides→constraints→Rust truth/provenance→presentation→render; failed input атомарен |
| D3 | Checked TimeTicks | Decimal wire strings, checked rational conversion/overflow/nonfinite; donor Tick только за adapter boundary |
| D4 | Document/session/view ownership | Author revision/Undo отдельно от session revision/events; hover/selection/camera не переписывают math history |
| D5 | Separate WASM artifacts | Learner artifact не содержит author implementation; проверить generated files и network imports |
| D6 | Pure foundation не ждёт GPU | A→C core параллельно исследованиям B; full release всё равно требует B–N |
| D7 | Последние совместимые инструменты с evidence | Coordinator cargo search: wgpu 30.0.1, egui 0.36.2; installed Rust/Cargo 1.99.0 + wasm32; manifests/lock проверяют совместимость |
| D8 | Dependency closure audit, updated2026-10-13 | Physically copied selected own-project closure + source/local hashes/licenses/patches; normal registry Cargo.lock, no crates.io vendoring |
| D9 | Shared checkout и skill workflow | `feat/engine` + новый isolated directory; manual briefs/ledger допустимы при несовпадении skill scripts; commit только по явному запросу |

R11 для нового widget/pilot разрешён user authority 2026-10-12: без Holo, понятная плавная
визуализация и приятный Blender-like UI. Проверки readable geometry/values/labels и motion/UI
остаются открытыми. Старое Holo-требование root AGENTS не восстанавливается в этом scope.

## Правила исполнения

Перед symbol edits проверять GitNexus graph_status, выполнять impact и сообщать HIGH/CRITICAL.
После изменений reanalyze; detect_changes не обновляет graph.
Соблюдать existing AGENTS scope и budget rules, не менять чужие edits.

Reproducible skill/reference failures записываются в oh-my-harness/BUG3.md;
filesystem server defects — в filesystem-mcp-rs/BUG_CDX.md. Existing entries сохраняются.
Expected missing file и server defect различаются. Полные reproduction/impact/fallback обязательны.

Tests/builds ниже — плановые gates полного workspace; факт исполнения подтверждается только evidence:

```text
# После создания workspace и чтения его manifests:
cargo check --manifest-path sim-widget-rs/Cargo.toml --workspace --locked
cargo check --manifest-path sim-widget-rs/Cargo.toml --target wasm32-unknown-unknown --workspace --locked
cargo test --manifest-path sim-widget-rs/Cargo.toml --workspace --locked
# WASM package command выбирается в A/B, tool versions фиксируются.
# После static integration, с требуемым Bun 1.4.3:
bun run check
bun run build
bun run size
# Existing JS suite: bun test tests/js (scope по затронутым contracts).
```

Команды package-specific после feature selection уточняются, а не копируются вслепую.
Test fixtures должны проверять observable math/time/input contracts, а не повторять implementation.
Internal waves используют scoped Rust gates и independent review. Полные Bun gates выполняются
при затронутой integration и перед явно запрошенным commit; foundation не объявляет их пройденными.
Запрещены stub crates, `todo!()`, `unimplemented!()`, panic placeholders, fake math и фиктивные pass reports.
Acceptance evidence помещать по проектным правилам в docs/acceptance; source plans остаются здесь.

## Состояние

- [x] Записаны scope, references, source reuse audit и dependency policy.
- [x] Разложены work packages и приёмочные контракты.
- [x] Создан bootstrap `sim-widget-rs` через `cargo new --lib`; это только начало исполнения.
- [ ] Все implementation packages A–N.
- [ ] Source copies/local-path closure/provenance manifests/workspace и подтверждённый dependency graph.
- [ ] Builds/tests/measurements и full-version acceptance.
- [ ] Миграция действующих уроков после принятия полной версии.
