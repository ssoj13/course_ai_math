# Phase 01 — featureset runtime объяснений

Дата: 2026-10-12. Статус: контракт разрешённой полной реализации. [Общий план](plan.md).
Scope: полная версия лаборатории и визуального редактора со всеми перечисленными возможностями.
Порядок разработки задаёт зависимости, а не сокращение состава продукта.
[Пилот на копии урока и поставка](phase-02-pilot-and-delivery.md).
Bootstrap `sim-widget-rs` создан; [журнал](../../sim-widget-rs/docs/implementation-status.md) отделяет его от готовых функций.

## Содержание

- [Продукт и оболочки](#продукт-и-оболочки)
- [Данные и вычисления](#данные-и-вычисления)
- [Время и плавность](#время-и-плавность)
- [Визуализация V1](#визуализация-v1)
- [Управление и интерфейс](#управление-и-интерфейс)
- [Визуальный редактор V1](#визуальный-редактор-v1)
- [Полный набор профессиональных возможностей](#полный-набор-профессиональных-возможностей)
- [Runtime и интеграция](#runtime-и-интеграция)
- [Источники зависимостей](#источники-зависимостей)
- [Assets и cglibs](#assets-и-cglibs)
- [Референсы интерфейсов](#референсы-интерфейсов)
- [Проверочные сцены](#проверочные-сцены)
- [Риски и решения](#риски-и-решения)

## Продукт и оболочки

Движок должен объяснять вычисления через анимацию и взаимодействие. Автор задаёт модель,
связи и последовательность раскрытия; читатель двигает объекты, меняет числа, возвращается
на любой шаг и видит их последствия во всех представлениях.

| Оболочка | Назначение | Компоновка |
| --- | --- | --- |
| Embedded | Сцена внутри MDX-урока | Viewport, компактные параметры, timeline, связанные блоки курса |
| Full window learner lab | Лаборатория на всё окно браузера | Несколько views, inspector, дерево, граф, таблицы и timeline |
| Author editor, обязательный V1 | Создание объяснений | Scene tree, bindings, chapters, keyframes, curve editor, preview |

Решение пользователя: сначала полная версия; лаборатория и визуальный редактор разрабатываются
вместе. Испытания проводятся на изолированной копии урока о косинусах до миграции существующих уроков.

Full window не требует native-приложения. Native-поставка может использовать тот же Rust
runtime и wgpu; там Rust запускается напрямую. WASM является способом доставки Rust в браузер.
[Web-платформы wgpu](https://wgpu.rs/doc/wgpu/documentation/platforms/web/index.html);
[eframe как пример оболочки для web/native](https://docs.rs/eframe/latest/eframe/).

UI shell пока открыт: DOM + Rust viewport и egui/eframe + craft-ui являются серьёзными кандидатами.
[Аудит StoryTold](phase-03-reuse-audit.md) добавляет готовые docking/layout widgets и web app patterns.
DOM сохраняет KaTeX и структуру учебника; egui reuse может сократить author UI работу.
Оба пути проверяются на формулах, IME, clipboard, focus, доступности, scroll/touch и Astro embedding.

```text
                    один документ объяснения
                параметры / операторы / tracks / ID
                                |
       +------------------------+-----------------------+
       |                        |                       |
  embedded shell       full window learner       author editor
  DOM + canvas          DOM + canvas              author UI V1
       |                        |                       |
       +------------------------+-----------------------+
                                |
                    Rust runtime / evaluator
                    сцена / picking / timeline
                                |
                         renderer: wgpu
                                |
                     browser WASM | native Rust
```

Для новых документов один Rust evaluator владеет истинными значениями и provenance.
При будущей миграции legacy-документ может явно выбрать TS bridge; два evaluator не
вычисляют truth одного документа. Author preview и learner используют одинаковую authority.

## Данные и вычисления

Обязательное ядро V1:

- Стабильные object ID, term ID, value ID и operation ID. ID связывают сцену, формулу,
  таблицу, код и граф; не меняются при переводе и перестройке UI.
- Hover, selection и guide focus имеют разные состояния. Приоритет подсветки явный,
  уход указателя не снимает selection и восстанавливает guide.
- Схема параметров: scalar, integer, bool, vector, matrix, enum, color, file.
  Метаданные: диапазон, единицы, шаг, точность, default, readonly, lock и validation error.
  Derived values вычисляются моделью. Зависимые границы и геометрические constraints входят в контракт.
- Опциональные выражения ограничены зарегистрированными операторами. Формат документа
  не предполагает выполнение произвольного кода из загруженного файла.
- Реальные операции, промежуточные числа, происхождение результата и связи operand/result.
  Вычислительный граф рисуется из выполняемого вычисления; autodiff нельзя подменять иллюстрацией.
- Отслеживание shape, strides, offset, dtype, порядка суммирования и округления для машинного слоя курса.
- Расширение через зарегистрированные типы объектов, операторов и импортёров.
  Поддержка произвольного ограниченного дерева сцены не обещает полный набор функций DCC.

## Время и плавность

Предлагаемый основной контракт: evaluate(document, parameters, seed, time) возвращает
математическое состояние для авторских tracks. Повторный seek на одно время даёт один результат.
Интерактивные ветвления/state machines дополнительно требуют input event log и snapshots:
документ, параметры и время без истории событий не определяют произвольную сессию.
Ввод читателя по умолчанию ставит playback на паузу и устанавливает контролируемый override.

| Область | Обязательное поведение |
| --- | --- |
| Timeline | Play/pause, предыдущий/следующий шаг, scrub, главы, скорость, loop, диапазон |
| Запуск | Playback начинается действием пользователя; autoplay не нужен |
| Seek | Вперёд и назад, повторяемое состояние, отсутствие зависимости от частоты кадров |
| Стохастика | Явный seed; повторение и сравнение запусков |
| Интеграторы | Фиксированный шаг, checkpoints, восстановление PRNG/state, ограниченная стоимость seek |
| Overrides | Guide задаёт base; live override читателя имеет явный приоритет до Return to guide |
| Возврат | Return to guide снимает override и приводит сцену к состоянию выбранного времени |
| Reset values | Текущие authored defaults записываются как live overrides с pause; permissions/ranges/constraints проверяются атомарно |
| Author set | Defaults документа редактируются с author revision/history; отдельной immutable factory baseline нет |
| Презентация | Ease, springs, переходы камеры, draw-in и выделение задаются независимо от truth |
| Прямой ввод | Drag/scrub следует указателю напрямую; spring допустима после дискретного ввода |
| Reduced motion | Доступный режим без лишней анимации, с теми же вычислениями и содержанием |

Spring не меняет истинное значение и не превращается в численный интегратор модели.
При scrub фиксируются значения и история; pointercancel/Esc восстанавливают checkpoint.
Seek не должен неожиданно сбрасывать правку читателя: base/override и reset имеют ясный UI.

В V1 автор редактирует keyframes, interpolation, hold, Bezier tangents, value/speed curves
и state transitions. Runtime и визуальный редактор используют один временной контракт.

## Визуализация V1

Пользователь 2026-10-12: «holo не надо», «нужна добротная и понятная, плавная визуализация»,
«с хорошим приятным UI». Для нового widget и скопированного пилота принят спокойный
профессиональный Blender-like вид. Геометрия, числа и подписи читаемы и стабильны;
непрерывное движение плавно, drag/scrub следует вводу напрямую. Типографика, spacing,
focus, inspector и timeline создают приятный точный UI. Декоративного Holo/HUD нет.
Это решение имеет приоритет над старым Holo-требованием root AGENTS в данном scope;
все 18 задач сохраняются, миграция существующих уроков и covers остаётся отдельной.

| Группа | Требуемые возможности |
| --- | --- |
| Геометрия | Points, segments, arrows, arcs, polygons, meshes, transformed grids, trajectories |
| Аналитика | Адаптивные function plots, scalar/vector fields, contours, surfaces, нормали |
| Числа | Связанные таблицы, heatmaps, векторные компоненты, накопление суммы, error display |
| Тензоры | Срезы, reshape/broadcast, strides и flat memory, dtype/rounding trace, bitfields |
| Изображения | Texture/UV, каналы, выбор sample и связь с формулой; bounded image inputs |
| Графы | Реальный computation graph, ancestry/term genealogy, связанные узлы; read-only для читателя |
| Подписи | Размер в CSS pixels, стабильность при zoom, declutter, доступные DOM-эквиваленты |
| Камеры | Ortho/perspective, fit all/selected, gizmo, presets, несколько views с выбираемой синхронизацией |
| Пространства | Local/world/view, clipping planes, axis/up conversion при импорте, корректные normals |
| Сравнение | A/B или wipe для исходного/преобразованного результата, если нужен уроку |

Renderer V1 — GPU raster. Anti-aliasing, пиксельные линии и текст оцениваются на прототипе.
Materials/lights/textures и визуальные возможности реализуются по полному featureset;
новый визуальный контракт требует понятной сцены и приятного UI без декоративного Holo.
В полном roadmap предусмотрен optional path tracing/compute module с отдельной capability matrix.
Его стоимость и browser/native поддержка измеряются отдельно от обязательного raster renderer.

Учебные объекты, значения, подписи и controls не скрываются автоматически по мощности устройства.
Визуальная приёмка нового widget/pilot проверяет принятый контракт; изменения существующих
изображений и legacy-уроков относятся к отдельной миграции.

## Управление и интерфейс

Профессиональный интерфейс определяется точностью поведения и последовательностью операций:

- Поля со scrub, вводом точного числа, единицами, ranges, lock/read-only и понятными ошибками.
- Pointer picking, drag handles, translate/rotate/scale gizmos, snapping, modifiers.
- Keyboard navigation и focus; действия доступны без мыши, подсказки показывают shortcuts.
- Pointer capture/cancel, Esc rollback, Undo/Redo как транзакции, reset view и reset values раздельны.
- Context menu, delayed tooltips, searchable command palette, remappable keymap и presets;
  команды ограничены текущей задачей.
- Inspector с разделами и disclosure, sliders и exact expressions, multi-edit, reset defaults,
  searchable properties; layout presets, resize, docking, сохранение рабочих пространств.
- Touch: hit targets, отмена жеста, конфликт drag/scroll, адаптивное расположение панелей.
- Согласованные hover, selection, guide focus; число связано с объектом и своим вычислением.
- Спокойная типографика, одинаковые расстояния, иерархия и состояния контролов.
  Темы, contrast/focus states, accessibility и локализация; формальный русский регистр «вы».

Читателю не нужны все author tools одновременно. Shell задаёт доступные команды и панели.
Docking для desktop lab не должен вытеснять нормальную мобильную компоновку.

## Визуальный редактор V1

Основной workflow: создать сцену → связать числа и математику → анимировать → поставить главы
и остановки → проверить learner preview → сохранить и встроить тот же документ в урок.

| Инструмент | Обязательное поведение V1 |
| --- | --- |
| Scene tree / palette | Primitives/presets, hierarchy, names, multi-selection, pivots, parenting, axes/units, layers, visibility/lock/isolation, duplicate/group |
| Inspector | Typed parameters, units/ranges, bindings, derived values и constraints |
| Timeline | Multitrack, keys, clips/groups, markers, chapters, pause gates, range/zoom/snap, NLA, relative/additive layers |
| Curve editor | Bezier tangents, hold/linear/ease, value/speed views, camera paths, constraints и retiming |
| Computation graph editor | Соединения реальных операторов/evaluators; inspect operands/results и provenance |
| Learner preview | Тот же runtime и документ; выбор разрешённых reader controls, locks и reset policy |
| Project assets | Import, dependency manifest, portable versioned package, save/open/export |
| Templates / compositions | Сцены/подграфы, instances, exposed parameters, стили, choreographies, библиотека presets |
| History / validation | Undo transactions, cancel, ошибки references/cycles/units/shapes с адресом источника |
| Extensions | Зарегистрированные scene/schema/operator/importer plugins через устойчивые контракты |

Редактор показывает ошибки до preview/export: потерянные ссылки, недопустимые циклы,
несогласованные units/shapes и отсутствующие assets. Визуальная связь вычисляет результат
тем же evaluator, который обслуживает learner runtime. Изменения bindings, объектов и keys
входят в транзакции истории; drag создаёт одну отменяемую операцию.

Portable package содержит версию схемы, scene document, tracks, разрешённые controls,
manifest зависимостей и assets либо явные ссылки на них. Отсутствующие внешние assets
показываются как ошибки зависимости. Формат архива и политика external resources ещё проектируются.
Авторские инструменты подключаются отдельным модулем: embedded lesson не загружает весь editor.

Первый прототип проходит цикл создания и публикации одного объяснения. Затем V1 расширяет
palette, graph/curve tools и templates до покрытия курса. Это последовательность реализации
одного релиза; лаборатория и визуальный редактор поставляются вместе.

## Полный набор профессиональных возможностей

| Область | Полный scope |
| --- | --- |
| Время и события | State machine, interactive branches, event triggers, recorded inputs, seed, checkpoints, session replay |
| Guidance | Chapters/steps, pause gates, hotspots, captions, quizzes/challenges, reader permissions, optional audio sync без autoplay |
| Формулы | Semantic terms и derivation steps, morph/reveal с привязкой к геометрии, таблицам, code и evaluator |
| Scene editing | Transform hierarchy, pivot/parenting, local/world axes, units, selection sets, layers, isolation, clipping/section views, A/B |
| Renderer | Materials, lights, textures, environment, custom WGSL, instancing, AA, transparency, pixel-stable lines/labels |
| Quality | Явные пользовательские presets, capability/budget diagnostics; учебная геометрия/числа/controls сохраняются без автоматического сокрытия |
| Документ | Versioning/migrations, journal/Undo, snapshots/history, state links, clipboard/duplicate, reusable templates/instances |
| Экспорт | Portable archive, PNG, SVG supported views, deterministic frame sequence/video; browser/native пути проверяются отдельно |
| Расширения | Документированный API Rust/operators/renderers/importers/schemas, portable declarative format, validation; без untrusted eval |

Все строки входят в план полной версии. Семантика clips/layers, branches и constraints должна
иметь явный порядок разрешения конфликтов: base tracks → согласованный layer blend → learner overrides
→ constraints → единый evaluator. Constraints проверяют эффективные входы; конфликт отклоняется атомарно.
Derived values вычисляет evaluator; presentation
springs не изменяют truth. Clip composition не допускает скрытого переписывания исходного документа.

Параметры, визуальные связи, quizzes и audio/captions используют те же time/term/value IDs.
Audio управляется отдельной optional дорожкой; seek/pause/speed policy документируется и проверяется.
Читательские permissions определяют, какие параметры и действия разрешены при embed.

## Runtime и интеграция

- Document lifecycle: load, validate, mount, pause, dispose; освобождение GPU, listeners и workers.
- Cancel загрузок и задач; stale result после смены документа не применяется.
- Один renderer обслуживает несколько областей canvas; сверка scroll/resize/visibility,
  fullscreen, devicePixelRatio и restoration GPU context/device.
- Worker boundary и cache policy; CPU-модель может вынестись в worker после измерений.
  Shared-memory threading требует отдельной проверки host headers.
- Версионированная сериализация, миграции, параметры/seed/time, snapshots, history/journal,
  session event log, shareable state links и portable archives с relative resource URLs.
- Embed API: load document, set parameters, seek, events selection/value/error, dispose.
  Подробные signatures проектируются после вертикального примера.
- Lazy modules для renderer, графов и форматов; данные урока не тянут все импортёры.
- Offline save документа и локальных assets; offline runtime требует cache/package policy.
- Export PNG/SVG supported views и frame/video export входят в полный scope.
  Browser/native implementation paths проверяются после time model и asset packaging;
  browser codec/capture restrictions отражаются в явной compatibility matrix.

Текущий лимит renderer chunk: 300,000 B gzip. Его нельзя молча заменить более широким.
Нужны измерения WASM + JS + ресурсов и согласование их принадлежности существующим gates.
Также фиксируются memory budget, cold load, frame time и поддерживаемая сложность сцены.

## Источники зависимостей

Последнее решение пользователя имеет приоритет:
cglibs и наши библиотеки берутся через SSH Git dependencies с pinned rev; StoryTold копируется.
crates.io используются обычными Cargo versions + Cargo.lock.

StoryTold source crates с внутренней transitive closure физически размещаются в
sim-widget-rs/vendor/storytold. Workspace/path references адаптируются к локальным копиям.
Для cglibs сохраняются Git dependency origins; reproducibility обеспечивают immutable rev
и Cargo.lock. vendor/cglibs и vendor/registry не создаются; cargo vendor crates.io и
registry source replacement не используются.

StoryTold provenance manifest хранит source repo/path/commit, versions, hashes, licenses/NOTICE,
attribution и patches; update protocol — явный snapshot import + diff review.
Для cglibs фиксируются SSH repository URL и rev; изменение rev — явное обновление зависимости.
[Подробная политика](phase-03-reuse-audit.md#источники-зависимостей).

Плановая приёмка: StoryTold local dependency closure, lock consistency и browser/native build.
Git/registry downloads разрешены; Git authentication/SSH доступ — build prerequisites.
Offline build опционален при заполненных Git/Cargo caches, без zero-download гарантии.
Состояние copies, workspace, Git dependencies и сборок фиксируется в журнале выполнения;
эта политика сама по себе не подтверждает готовность зависимостей.

## Assets и cglibs

Это наблюдения о кандидатах на переиспользование. Отдельная render-rs compile probe прошла;
runtime и интегрированная WASM-совместимость всех библиотек ещё требуют проверки в журнале.

| Кандидат | Польза | Риск/проверка |
| --- | --- | --- |
| render-rs | Cameras, scene, PBR, picking, outlines, curves | Browser features, texture/text pipeline, размер renderer |
| wgpu-widgets-rs | HUD, SDF controls, text, parameter panels | winit/arboard, webgpu feature, accessibility all-GPU UI |
| curves-rs | Keys и interpolation | Совместимость временного контракта runtime |
| nodes-rs | nodes-core headless evaluator, nodes-view wgpu, nodes-interact winit, chrome/keymap, egui bridge | wgpu 30 без webgpu при default-features=false, arboard/native границы; browser port не доказан |
| graph-layout-rs | Standalone layout без dependencies | Проверить API, качество и browser переносимость |
| gltf-rs | GLB/glTF parsing | Fetch внешних ресурсов, materials, animation и skinning |
| alembic-rs | Ogawa from_bytes | mmap/Rayon boundaries, changing topology, time samples |
| usd-rs | Composition, schemas, imaging/wgpu bridge | usd-sdf с mmap/OpenVDB, browser resolver и размер зависимостей |

Порядок реализации: GLB/glTF static mesh/PBR, animations/skinning/morph targets,
Ogawa Alembic geometry caches с time samples/changing topology, затем USD по заявленной
compatibility matrix. Все эти группы предусмотрены в полной версии.
References, payloads, sublayers и textures требуют browser asset resolver.
Чтение формата не гарантирует воспроизведение всех его материалов и схем.
USD matrix перечисляет variants, references, payloads, layers, поддержанные schemas/materials,
textures и packaging; неподдержанное выдаёт адресную диагностику. Async asset pipeline включает
resolver, dependency/cache budgets, cancellation, progress и diagnostics.

## Референсы интерфейсов

Ссылки проверены по документации; это список идей для прототипирования, а не сравнительный
визуальный аудит screenshots. Заимствуются взаимодействия, затем проверяются в учебном сценарии.

| Пример | Что изучить и применить |
| --- | --- |
| [Blender editors](https://docs.blender.org/manual/en/latest/editors/index.html) | Editor regions, inspector, outliner, viewport/navigation, timeline |
| [Maya Graph Editor](https://help.autodesk.com/cloudhelp/2024/ENU/Maya-Animation/files/GUID-34D3F53B-BDB2-4E42-B39D-F965B4204E1B.htm) | Curves, keys и tangents для авторского редактирования движения |
| [Houdini panes](https://www.sidefx.com/docs/houdini/basics/panes.html), [playbar](https://www.sidefx.com/docs/houdini/anim/playbar.html) | Panes, linked parameter/node views, time/ranges |
| [Cinema 4D](https://help.maxon.net/c4d/2026/en-us/Content/html/5821.html) | Layout, attribute workflow, компактные контролы |
| [TouchDesigner Pane](https://docs.derivative.ca/Pane) | Разные представления одного procedural проекта |
| [After Effects speed](https://helpx.adobe.com/after-effects/desktop/animate-in-after-effects/speed-between-keyframes/speed.html) | Value/speed curves и управление ритмом движения |
| [Nuke Viewer](https://learn.foundry.com/nuke/content/reference_guide/image_nodes/viewer.html) | Viewer controls и сравнение изображений A/B |
| [Rive state machine](https://github.com/rive-app/help-center/blob/master/editor/state-machine.md) | Inputs, states и transitions как авторская модель интерактивности |
| [Figma easing/springs](https://help.figma.com/hc/en-us/articles/360051748654-Prototype-easing-and-spring-animations) | Контролируемые переходы и параметры spring |
| [Desmos sliders/points](https://help.desmos.com/hc/en-us/articles/202529069-Sliders-and-Movable-Points-in-a-Graph) | Прямое управление математикой, зависимые значения и ограничения |
| [GeoGebra Graphics View](https://geogebra.github.io/docs/manual/en/Graphics_View/) | Связь геометрии и объектов с параметрами |
| [Transformer Explainer](https://poloclub.github.io/transformer-explainer/) | Связанные числовые представления реального computation |

## Проверочные сцены

Универсальность проверяется покрытием разных механизмов, а не количеством классов renderer.
Это предложенные будущие acceptance examples; они не запускались в рамках планирования.

| Сцена | Что должна доказать |
| --- | --- |
| Dot product / law of cosines | Drag, angles, linked numbers/formula/code, reversible derivation |
| Matrix determinant / inverse | Transform, signed area, singular state и таблица |
| Tensor strides / rounding | Flat memory, slices, transposes, dtype, exact accumulation trace |
| Derivatives / finite differences | Adaptive plot, tangent, scrub параметров, error convergence |
| Jacobian / transformed surface | 3D surface, local frame, normals, grid deformation |
| Probability / sampling | Seed, histogram, reproducibility, checkpoints и seek |
| Real backprop | Реальный autodiff graph, chain rule, gradients и шаг обновления |
| Attention → cell | Tensor/table/graph links, softmax, значение выбранной ячейки |
| Two-bone IK / Fourier | Constraints, coordinate spaces; epicycles/trails и time evaluation |
| Quantization | Bitfields, dtype transforms, quantization error и числовая provenance |

Сценарии проходят embedded и full window shells. Проверяются pointer, keyboard, touch,
cancel/undo/reset, seed и повторный seek, сохранение/восстановление документа.
Основной pilot — изолированная копия law-of-cosines: MDX sandbox и project document;
сейчас запланированы, но ещё не созданы. Для авторской оболочки проверяется цикл: создать, связать, анимировать, задать главы,
preview, portable save/open и embed без ручного переписывания документа.
Полное покрытие 98 записей курса требует отдельного mapping: эта выборка проверяет
архитектурные риски и не заменяет итоговый course completion map.

## Риски и решения

До реализации требуется определить target devices/browsers, обязательность WebGL2 fallback,
host/offline mode, строгий all-GPU UI либо hybrid и подмножество USD.
Порядок поставки author editor определён: вместе с лабораторией в V1.
WebGL2 fallback нельзя считать функционально равным WebGPU без отдельной проверки.

Первые исследования: переносимость features/dependencies; фактический размер загрузки;
линии/подписи и несколько views; deterministic seek; стоимость симуляции;
стабильность UI на телефоне и соответствие visual references.

Результат планирования: достаточно богатый общий контракт V1 и список измеримых рисков.
Полная версия включает лабораторию, редактор и все строки featureset. Этапы реализации:
portability spike → copied cosines pilot → полный editor/runtime → assets/export → acceptance.
Миграция существующих уроков планируется после готовности полной версии и принятия пилота.
