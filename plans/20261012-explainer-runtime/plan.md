# Универсальный runtime анимированных объяснений

Дата: 2026-10-12. Статус: выполнение полной версии разрешено, bootstrap создан.
Рабочая библиотека: `sim-widget-rs`; готовность runtime и миграция ещё не подтверждены.
[Featureset](phase-01-featureset.md) · [Пилот](phase-02-pilot-and-delivery.md) · [Reuse](phase-03-reuse-audit.md).
[Подробная реализация A–N](phase-04-implementation.md) · [Матрица приёмки](phase-05-acceptance.md).
[Полный Warpbro UI port — accepted architecture](phase-06-warpbro-ui-port.md).
[Журнал выполнения](../../sim-widget-rs/docs/implementation-status.md).

## Цель

Общий движок для интерактивных математических и CG/VFX-объяснений: точные вычисления,
обратимый timeline, плавная презентация, профессиональное управление и связанные представления.
Применимость ко всему курсу проверяется десятью характерными сценами, а расширения добавляются через
контракты объектов, операторов и импортёров. Полнота произвольного DCC-приложения не обещается.

## Предлагаемая архитектура

Rust: модель документа, runtime, геометрия, picking, renderer на wgpu.
Браузер: WASM для Rust; DOM-гибрид и egui/eframe shell сравниваются на embedding/формулах/accessibility.
StoryTold даёт craft-ui и app/runtime кандидатов; выбранные crates требуют portability проверки.
Native: тот же Rust runtime, отдельная оболочка; WASM здесь не требуется.
Новые документы вычисляет один Rust evaluator. При будущей миграции существующий TS core
может обслуживать отдельные legacy-документы через явный bridge; authority выбирается на документ.

Оболочки V1: встроенная карточка урока, браузерная лаборатория на всё окно и визуальный авторский редактор.
Решение пользователя: полная версия сразу; лаборатория и редактор входят в общий scope.
Испытания идут на копии урока о косинусах; существующий урок сохраняется.
Различаются компоновка и разрешённые операции; документ и runtime общие.

Визуальное решение пользователя от 2026-10-12 для нового widget и копии пилота:
«holo не надо», «нужна добротная и понятная, плавная визуализация», «с хорошим приятным UI».
Спокойная Blender-like сцена, читаемые числа/подписи, плавное движение и приятный профессиональный
UI заменяют прежнее требование Holo в этом scope. Все 18 задач остаются обязательными;
миграция существующих уроков и covers планируется отдельно.

## Полная версия — обязательный scope

- Семантические ID объектов, значений, операций и терминов; связанные hover, selection и guide.
- Типизированные параметры, реальные вычисления и происхождение чисел.
- Детерминированное вычисление состояния на времени t, reversible scrub, шаги и главы.
- 2D/3D-геометрия, графики, поля, таблицы, тензорные срезы, изображения, вычислительные графы.
- Камеры и несколько views; drag, клавиатура, touch, Undo, reset, search, панели.
- Редактор: scene tree, bindings, multitrack/NLA, curve/graph tools, branches, compositions, learner preview.
- Загрузка/отмена/dispose, versioned portable package, asset manifest, embed API, восстановление GPU.
- Raster materials/lights/WGSL, formats GLB/ABC/USD matrix, export PNG/SVG/frames/video.
- Стили следуют принятому 2026-10-12 контракту: понятная плавная визуализация и приятный Blender-like UI без Holo.

## Источники зависимостей

Latest user authority2026-10-13: физически копировать selected required crates и own-project
closure Warpbro/egui-widgets/gizmo/cglibs/StoryTold в sim-widget-rs с frozen source commit,
hashes/licenses/assets/NOTICE и local patch provenance. Прежнее SSH-only решение отменено.
crates.io остаётся Cargo versions + Cargo.lock, без registry vendoring/source replacement.
StoryTold paths локальны; provenance/licenses/update protocol: [политика](phase-03-reuse-audit.md#источники-зависимостей).
Создана библиотека `sim-widget-rs`. Workspace, зависимости, donor copies и их приёмка
учитываются отдельно в журнале; создание библиотеки не закрывает ни одной feature group.

## Этапы и критерии перехода

1. Исследования риска: WASM зависимостей, размер загрузки, текст/линии, GPU capability,
   многосценовый canvas, контракт времени и ввода. Сформировать измерения и решения.
2. Вертикальный learner + author цикл на изолированной копии урока о законе косинусов:
   создать сцену, связать числа, анимировать, задать главы, preview, save и embed того же документа.
3. Расширить primitives и редактор: десять сцен курса, graph/curve tools, constraints, templates;
   подтвердить, что API не требует отдельного движка для каждого раздела.
4. Форматы: GLB, затем Ogawa Alembic, затем согласованное подмножество USD;
   каждая упаковка имеет отдельный lazy boundary и измеренный бюджет.
5. Завершить полную версию: лаборатория, редактор и весь согласованный featureset; проверить controls,
   touch, плавность, accessibility, save/export и embed. После этого планировать миграцию уроков.

Текущий renderer gate 300,000 B gzip остаётся ограничением. Новый gate требует измерений
и отдельного решения. Никаких автоматических отключений элементов по мощности устройства.

## Открытые решения

- Принят полный copied Warpbro egui shell с нашим single-GpuHost wgpu viewport;
  actual browser accessibility/IME/формулы и отдельный learner artifact требуют своих gates.
- Целевые браузеры, устройства и native-платформы; обязателен ли WebGL2 fallback?
- Подмножество USD, формат проекта и размещение внешних ресурсов.
- Показатели FPS/frame time, загрузки, памяти и scene complexity для целевых устройств.
- Проверка читаемости, плавности и UI на target devices по принятому контракту без Holo.

## Проверка предложения

[Подробный документ](phase-01-featureset.md) содержит контракты времени, перечень функций,
референсы интерфейсов, риски cglibs и проверочные сцены. Это план исследований и разработки;
результаты WASM, benchmark и миграции требуют отдельных evidence и независимой приёмки.
