# Phase 05 — приёмка полной версии

Дата: 2026-10-12. Статус: план приёмки полной версии; scoped evidence записывается в журнале.
[План](plan.md) · [Пакеты реализации](phase-04-implementation.md) · [Featureset](phase-01-featureset.md).
Коды ниже связывают observable behavior с пакетами работ. Все gates остаются открытыми.
[Задачи исполнения](phase-04-implementation.md#задачи-исполнения) и
[журнал](../../sim-widget-rs/docs/implementation-status.md) связывают gates с outputs и review.
Reviewer запускает применимые checks самостоятельно, с моделью, отличной от implementer;
записывает snapshot, обе model IDs, команды, результаты и закрытые замечания.

Visual authority для нового widget/pilot, 2026-10-12: «holo не надо», «нужна добротная и
понятная, плавная визуализация», «с хорошим приятным UI». Проверять спокойную Blender-like
сцену и controls без декоративного Holo/HUD: stable numbers/labels, semantic readability,
плавную continuous presentation, direct drag, typography/spacing/focus, inspector/timeline.
Это scoped override старых root AGENTS; все 18 задач сохраняются. Legacy lessons/covers
требуют отдельной миграции.

Typography authority, 2026-10-12: пользователь выбрал 11 CSS px для основного desktop UI
и обычных scene labels. Labels сохраняют размер в CSS pixels независимо от geometric zoom;
hit areas остаются удобными, heading hierarchy — сдержанной. Выбор заменяет прежний
desktop minimum 14 px только для нового widget/pilot. Положительный отзыв о static reference
не закрывает rendered review, полноту runtime UI или tests; browser reference review — `NOT_RUN`.

Density authority, 2026-10-12: уменьшить heights, vertical padding и group gaps во всех
embedded/learner/author shells при сохранении desktop 11 px. Pointer controls обычно
24–28 CSS px; явный touch/coarse-pointer режим сохраняет hit areas минимум 44 CSS px.
Компактность не сокращает content/features или reachability. Пользовательский screenshot
остаётся reference feedback; rendered reviewer acceptance требуется отдельно.

## Проверить контракты ядра

| ID | Сценарий | Ожидаемый результат | Пакеты |
| --- | --- | --- | --- |
| CORE-01 | Изменить a=3,b=4,θ=60° | c²=13, provenance указывает реальные operands/операцию | C/G/L |
| CORE-02 | Matrix singular input | Typed error, сохранён документ, geometry/formula согласованы | C/G |
| CORE-03 | Invalid unit/shape/binding/cycle | Адресный diagnostic, нет partial writes | C/H |
| CORE-04 | Duplicate/missing semantic ID | Validation error, namespace document/instance различим | C |
| CORE-05 | TS→Rust backend comparison | Зафиксированные dtype/rounding/order fixtures совпадают | C/G |
| CORE-06 | Real backprop scene | Gradient создаёт actual operator graph, finite-diff comparison обоснован | C/G/M |
| CORE-07 | Неизвестный operator/schema | Явная unsupported capability, без fake result | C/I |
| CORE-08 | Длинный Tick timeline через WASM | Целые ticks точны, string/BigInt bridge без Number truncation | C/F/K |
| CORE-09 | Track и live override конфликтуют с constraint | Effective inputs проверяются до evaluator; typed error без partial writes и расхождения truth/provenance | C/F |
| CORE-10 | Time conversion overflow/NaN/division by zero | Checked TimeTicks выдаёт error до donor adapter; нет silent zero/saturating результата | C/F/K |
| CORE-11 | Selection/camera против author Undo и session replay | View state не создаёт math history; document/session revisions имеют явных владельцев | C/E/F/K |
| CORE-12 | Новый session protocol V2/mixed patch | Отдельный different-model review, реальные V1 fixture migrations, checkpoint/event replay и eviction; document schema/operators неизменны. Историческая Task 02 v1 receipt не принимает новый patch | C/E/F |

## Проверить время и взаимодействия

- [ ] TIME-01: seek t₀→t₁→t₀ и повторный t₁ дают одинаковую truth для authored tracks.
- [ ] TIME-02: event/state branch replay при том же log/seed совпадает; без history не обещать совпадение.
- [ ] TIME-03: stochastic simulation backward seek восстанавливает checkpoint/PRNG/fixed-step state.
- [ ] TIME-04: изменение FPS не меняет mathematical result и sample sequence.
- [ ] TIME-05: reader edit pauses playback; override виден, return-to-guide восстанавливает base.
- [ ] TIME-06: layer/clip/constraint conflicts следуют documented precedence.
- [ ] TIME-07: speed/loop/in-out/step/chapter gates работают без reader autoplay.
- [ ] TIME-08: springs изменяют presentation, derived formula сразу читает truth.
- [ ] INPUT-01: direct drag следует pointer; одна операция Undo, cancel возвращает checkpoint.
- [ ] INPUT-02: pointercancel/Esc не вызывает click; mouse/touch/keyboard semantic edits эквивалентны.
- [ ] INPUT-03: hover/selection/guide независимы; labels locale не меняют semantic bindings.
- [ ] INPUT-04: camera reset и parameter reset раздельны; wide/full retain document state.
- [ ] INPUT-05: focus, remapped shortcuts, clipboard/IME, touch scroll arbitration проверены.
- [ ] INPUT-06: constrained movement не создаёт jitter/скрытые contradictory clamps.
- [ ] INPUT-07: reduced motion сохраняет содержание, compute trace и разрешённые controls.
- [ ] INPUT-08: Reset values использует authored defaults как live overrides с pause; Return to guide samples tracks текущего времени; Author set редактирует defaults с history; denied/invalid reset атомарен (R02-10).
- [ ] INPUT-09: все points/arrows/manipulators имеют действующие endpoint/shaft/group/angle/axis/plane/rotation semantics. A-only сохраняет O/B, B-only O/A, O-only A/B; group move сохраняет shape; B не ограничен старой осью. Проверить translated/tilted 2D/3D и named residual tolerances через actual Session forward graph.
- [ ] INPUT-10: Space или Alt + LMB tumble/MMB pan/RMB dolly, Shift precision, fit-all/selection и embedded/focused wheel profiles. Navigation modifier до picking; input/contenteditable/IME и чужая карточка не теряют keys/scroll; полный ViewState не создаёт math history.
- [ ] INPUT-11: mixed Some/None override patch и playback policy атомарны; cancel восстанавливает owned override presence/source, компенсирует monotonic events/revisions и не переписывает unrelated state. One gesture/one interaction Undo, conflict/budget/permission rejection и source-bound native/browser proof; author Undo не подменяет interaction history.
- [ ] INPUT-12: typed zero/dependent authored basis rejection; допустимые theta 0/pi дают empty fill плюс real edges/points. Rejected render candidate сохраняет согласованный published frame/formula; stop/reset/retry остаются работоспособными после failure.

Пакеты: C/E/F/K. Seek precision и numeric tolerances определяются арифметическим контрактом:
exact integer/state matching там, где он обещан; floating tolerance явно названа для geometry.

## Проверить визуализацию и данные

| ID | Доказательство | Пакеты |
| --- | --- | --- |
| VIEW-01 | Pixel lines/text стабильны при zoom/DPR/resize; ordinary scene labels 11 CSS px независимо от geometric zoom, declutter и semantic links | B/D/G |
| VIEW-02 | Ortho/perspective, fit-selected/all, gizmo, transforms/pivots/up/units корректны | D/E |
| VIEW-03 | Multiple views не переносят camera/picking/render в чужую карточку | D/K |
| VIEW-04 | Fields/plots/contours/surfaces guard nonfinite и declared adaptive accuracy | D/G |
| VIEW-05 | Tables/heatmaps/tensor slices/strides/memory/dtype/bitfields связаны с evaluator | G/M |
| VIEW-06 | Image UV/channel sampling, code/graph/term links сохраняют ID | G |
| VIEW-07 | Clipping/section/A-B/instancing/transparency работают на capability matrix | D/I |
| VIEW-08 | Custom WGSL failure даёт diagnostic, не crash всей страницы | D/I |
| VIEW-09 | Quiet Blender-like scene/UI: desktop 11 CSS px, компактные heights/padding/gaps, pointer controls обычно 24–28 CSS px и explicit touch/coarse hit areas ≥44 CSS px; сдержанные headings, stable labels/direct drag, без Holo; content/reachability сохранены | A/B/D/E/K |
| VIEW-10 | User quality presets явны; учебная геометрия/числа/controls не скрываются автоматически | D/E |
| VIEW-11 | Actual supported MSAA4 у pipelines/color/depth/resolve + owned analytic/SDF edges, unchanged public BSDF; DPR1/1.25/2 и fractional-motion metrics, near clipping, CSS width/hit policy, checked concurrent target allocations/resize/loss | B/D/E/K |

Screenshots помогают visual audit, но не доказывают frame time или working input.
Motion проверяется реальным playback/scrub на выбранных устройствах.
Layering и label-to-geometry clearance проверять на переходных ширинах, не только на двух
desktop/mobile endpoints. На compact author layout library/resources/compositions и add-object
route остаются достижимыми. Исторический fix1 source review требует исправления D1/D5;
density iteration 2 не является rendered acceptance.

## Проверить авторский workflow

- [ ] AUTHOR-01: создать scene/palette objects, hierarchy/parent/pivot/lock/layer/isolation.
- [ ] AUTHOR-02: bind inspector values к math operators и linked formula/table/geometry.
- [ ] AUTHOR-03: multi-edit/expression/unit constraints, diagnostics и undo journal.
- [ ] AUTHOR-04: keys/clips/groups/markers/range snap/zoom/NLA/additive/relative.
- [ ] AUTHOR-05: value/speed curves, Bezier tangents, camera paths и retiming.
- [ ] AUTHOR-06: graph connections вычисляют actual values; units/shapes/cycles отображаются.
- [ ] AUTHOR-07: template/composition instances expose params и независимые instance IDs.
- [ ] AUTHOR-08: chapter/pause/hotspot/quiz/captions/audio editing и learner permissions.
- [ ] AUTHOR-09: preview использует тот же runtime, что embedded learner.
- [ ] AUTHOR-10: layout/docking/search/keymap сохраняются; floating panels не заявлять OS windows.
- [ ] AUTHOR-11: save→open→preview→embed без ручного переписывания project document.

Пакеты: C/E/F/G/H/J/K. History semantics проверяются против нашего документа, не только donor app.

## Проверить форматы и хранение

| ID | Сценарий | Ожидаемый результат |
| --- | --- | --- |
| ASSET-01 | GLB/glTF mesh/material/animation/skinning/morph | Поддержанные features перечислены и реально отображены |
| ASSET-02 | ABC time samples/changing topology | Seek воспроизводит нужную геометрию |
| ASSET-03 | USD layers/variants/refs/payload/material subset | Matrix соблюдена, missing/unsupported адресно указаны |
| ASSET-04 | Cancel import / change document | Late result не применяется, память освобождается |
| ASSET-05 | Relative assets под /math-course | Base-aware URLs, static fetch успешен |
| ASSET-06 | Corrupt/missing/oversize resources | Retry/diagnostics/budget policy без зависания runtime |
| SAVE-01 | Portable package другого host | Schema/capabilities/resources перечислены, donor paths отсутствуют |
| SAVE-02 | Migration старой схемы | IDs/state сохраняются либо даётся конкретный incompatibility error |
| SAVE-03 | Multi-tab writes / interrupted save | Явная conflict/recovery policy, без silently losing edits |
| SAVE-04 | Snapshots/state links/clipboard | Документ и session data восстанавливаются по declared semantics |
| EXPORT-01 | PNG/SVG | Supported views/export limitations перечислены |
| EXPORT-02 | Deterministic frames/video | Frames соответствуют logical t, codec/platform matrix проверена |
| AUDIO-01 | Pause/seek/speed/chapter | Declared audio/caption sync, no reader autoplay |

Пакеты: I/J/F/K; реальные fixtures подбираются с лицензией и provenance.

## Проверить delivery и lifecycle

- [ ] LIFE-01: mount/dispose/remount многократно, без растущих GPU/listener/worker resources.
- [ ] LIFE-02: GPU device loss/restoration, error/retry сохраняют recoverable document state.
- [ ] LIFE-03: hidden/visible/resize/fullscreen/scroll не ломают picking и focus.
- [ ] LIFE-04: lazy pages без сцены не загружают renderer/editor/all format packs.
- [ ] LIFE-05: Pyodide/ELK workers сосуществуют с Rust runtime без collisions.
- [ ] LIFE-06: learner и author имеют отдельные WASM artifacts; network/artifact audit подтверждает отсутствие author implementation в learner.
- [ ] LANG-01: EN/RU parity, formal «вы», accessible names и runtime dictionaries.
- [ ] LANG-02: shared extensible catalog/host locale и простой global switch обновляют весь text/hints/errors/ARIA/units/number formats и dynamic controls. IDs/TeX/core units/time/session/camera/focus сохраняются; locale switch не commits pending edit. Placeholder parity, third catalog registration и localized parse/format roundtrip проверены.
- [ ] A11Y-01: keyboard/focus/IME/screen-reader сценарий на выбранном shell доказан.
- [ ] SOURCE-01: cglibs SSH Git pinned rev; StoryTold local copies/closure; registry versions+lock.
- [ ] SOURCE-02: licences/NOTICE/provenance/patch journal и extension API complete.
- [ ] BUILD-01: locked native/browser checks и integrated Bun1.4.3 check/build/size.
- [ ] PERF-01: target matrix, reproducible workload, cold load/memory/frame-time/p95/input delay.
- [ ] SIZE-01: renderer 300,000 B gzip и остальные current gates соблюдены либо есть explicit ruling.
- [ ] PERF-02: 60 FPS initial target оценён измерением; screenshot labels не performance evidence.

### CURRENT TODO — обязательный interaction proof

Tasks 03/04/18 остаются открытыми. Требования последнего live-preview trial приоритетны внутри
полного plan: INPUT-09–12, CORE-12, LANG-02 и VIEW-11 требуют собственных reviewed snapshots
и actual native/browser evidence. Bounded core prerequisite принят после fix1; sole product ownership возвращён original renderer owner. Full input/renderer acceptance остаётся открытой.
Плановые сценарии см. в [interaction contract](../../.superpowers/sdd/sim-widget-rs/task-03-interaction-contract.json)
и [актуальном ownership/status](../../sim-widget-rs/docs/implementation-status.md#task-0304--interaction-priority-and-live-preview).
Partial native/browser material PASS, source shader validation и working fields/playback preview
не закрывают эти gates. Combined cosines WASM 320,584 B gzip превышает 300,000 B;
standalone raster 110,097 B gzip — отдельный partial artifact, не replacement full payload.

### Shared picking и AA proof — оставить полноту gate явной

- [ ] INPUT-13: один общий resolver для hover/press/drag start в 2D/3D ortho/perspective; physical/semantic identities, clipped visible geometry и complete runtime/scene/topology/view/layout/device/display stamps. Foreign/stale async result не применяется.
- [ ] INPUT-14: bounded BVH refit/rebuild/query с CSS slop broadphase и exact transformed narrowphase; world ray t при nonuniform scale, negative winding/backface, barycentrics и measured memory/query costs. Eligibility/opaque occlusion до priority; explicit masked/transparent/x-ray/selected-overlay policies; deterministic complete-ID ties.
- [ ] VIEW-12: production theta0/pi empty fill/real edges/points и atomic render candidate сохраняют coherent frame/formula; recovery controls работают. Isolated scratch AA fixes не принимают production path.
- [ ] VIEW-13: actual MSAA4 browser edge/DPR1/1.25/2/fractional-motion + resize/loss/multiview/resource-union proof. Current compiled AA receipt sample4 и native six numeric cases — scoped evidence; screenshot timeout и отсутствие full browser matrix не заменяются pass.

[Core prerequisite receipt](../../docs/acceptance/sim-widget-rs/task-04-atomic-override-prerequisite.md)
принимает bounded Patch/playback/V2 migration только; весь CORE-12 integration/input gate открыт.
[Picking design review](../../.superpowers/sdd/sim-widget-rs/task-03-shared-picking-design-review.json)
APPROVED_FOR_IMPLEMENTATION не закрывает INPUT-13/14.
[AA browser receipt](../../.superpowers/sdd/sim-widget-rs/task-03-aa-preview/browser-receipt.json)
подтверждает actual sample4 и controls/recovery на isolated accepted v1 core; unpublished
handles/navigation/catalog и WASM323,366 B gzip сохраняют failures/open gates.

## Принять косинусы и весь scope

Пилот: dev EN/RU-копии, самостоятельные document/store/card/timeline IDs.
LC overview/detail/proof, signed strips, shear, rigid turns и two-bone IK сохраняют смысл.
При a=3,b=4 ожидается c²=13/25/37 для 60°/90°/120°.
Оригинал и пилот одновременно работают без shared state; original manifest/progress не изменяются.

Canonical coverage: dot, matrix inverse/determinant, tensor strides/rounding, derivatives,
Jacobian, probability, real backprop, attention cell, IK/Fourier, quantization.
Каждая сцена проходит author→learner→package→embed, replay и provenance.
Mapping всех 98 course entries фиксирует требуемые capabilities и gaps до full completion.

Готовность полной версии требует всех feature groups, compatibility matrices и closed blockers.
Foundation, workspace, locked checks или отдельный пилот не закрывают full-version gate.
Только после принятия полной версии на копии косинусов начинается отдельное планирование миграции.
