# Phase 02 — полная версия и пилот на копии урока

Дата: 2026-10-12. Статус: разрешённая полная реализация. [План](plan.md) · [Полный featureset](phase-01-featureset.md).

## Решение пользователя

Сначала сделать полную версию лаборатории и визуального редактора. Пробовать её на копии
урока о косинусах. Полный scope планируется сейчас; этапы задают порядок зависимостей.
Создан bootstrap `sim-widget-rs`; исполнение и evidence ведутся в
[журнале](../../sim-widget-rs/docs/implementation-status.md). Полная приёмка остаётся открытой.

Визуальное решение пользователя от 2026-10-12: «holo не надо», «нужна добротная и понятная,
плавная визуализация», «с хорошим приятным UI». Новый widget и копия косинусов используют
спокойную Blender-like сцену и профессиональные controls: читаемые стабильные числа/подписи,
плавное непрерывное движение и прямой отклик drag/scrub. Декоративный Holo/HUD исключён;
это scoped override старых root AGENTS. Все 18 задач сохраняют полный featureset.
Изменения существующих уроков и covers относятся к отдельной миграции.

## Изолированный пилот

Источник: src/content/docs/ground/law-of-cosines.mdx и соответствующие
law-of-cosines.tsx, .rows, .math, .impl. Предлагается отдельная dev/sandbox route,
EN/RU-копии и project document нового runtime. Точные новые пути фиксируются при реализации.

Копия получает собственные document/store/card/timeline IDs: одновременный запуск оригинала
и пилота не должен смешивать значения, guide focus и историю. Main manifest, sidebar,
progress и исходные уроки сохраняются. Пилот не заменяет существующий маршрут.

## Содержание пилота

- LC-0: overview недостающих прямоугольников и связанная detail-сцена. Общее управление
  шагами overview/detail; обзор не получает независимую математическую последовательность.
- LC proof: c² = a² + b² − 2ab cos θ, signed area strips, shear с сохранением площади,
  rigid turns, подсветка и сокращение членов формулы.
- LC scene 0/1/2/3, включая two-bone IK: reachable/unreachable boundary и корректные
  вырожденные состояния без скрытой подмены данных.
- Связанные geometry, labels, typed inspector, derivation, table и code; scene model
  вычисляет реальные промежуточные числа для всех представлений.
- Author workflow: собрать и связать сцену, отредактировать tracks/curves, задать главы,
  preview learner permissions, сохранить portable project, загрузить и embed того же документа.

Числовые ориентиры: a=3, b=4; θ=60° даёт c²=13, θ=90° даёт 25, θ=120° даёт 37.
Проверяются плавный playback, прямой drag, reversible seek, override/return-to-guide,
Undo/cancel, reset, native/full-window/browser embed и локализация EN/RU.

## Порядок разработки полной версии

| Этап | Результат | Условие перехода |
| --- | --- | --- |
| A. Portability / vendoring | StoryTold copies + cglibs SSH Git pinned rev, browser/native map, GPU/text/lines/multiview, бюджеты | Provenance/licenses/lock и browser/native dependency resolution |
| B. Вертикальный пилот | Копия косинусов: author → learner → save/open → embed | Один документ, реальные числа, сохранённый смысл исходного объяснения |
| C. Полный runtime/editor | Весь geometry/data/input/timeline/graph/composition featureset | Acceptance по каждой группе, без исключения авторских tools |
| D. Formats/assets/export | GLB/glTF, ABC, USD matrix, resolver, archive, image/frames/video | Диагностика зависимостей и заявленная browser/native compatibility |
| E. Полная приёмка | Пилот + десять сцен архитектурного покрытия | Controls/motion/accessibility/locale/lifecycle/budgets приняты |
| F. Миграция | Отдельный план переноса уроков курса | Полная версия готова и пилот принят пользователем |

Stages B/C могут итеративно использовать sandbox; сокращённый пилот не становится финальным scope.

## Карта приёмки полного featureset

| Группа | Доказательство |
| --- | --- |
| Данные и graph | Связанные значения/provenance, shapes/units, реальные operators/autodiff, diagnostic cycles |
| Timeline и states | Multitrack/clip/layer curves, branches, seed/event history/checkpoint, repeat seek |
| Geometry и rendering | 2D/3D, поля/графики/срезы, clipping, A/B, materials/lights/WGSL, текст/линии |
| Author UI | Scene tree/palette, inspector/bindings, docking/search/keymap, Undo, templates/instances |
| Learner | Разрешённые controls, step/chapter gates, quizzes/hotspots/captions/audio policy, no autoplay |
| Assets | Dependency manifest, cancel/cache/progress, GLB animation/morph, ABC topology, USD matrix |
| Persistence/export | Version migrations, relative resources, archive/state links/snapshots, PNG/SVG/frames/video |
| Platform | Browser/native capability, GPU recovery/dispose, multiview, accessibility, touch, EN/RU |
| Расширения | Документированные operators/renderers/importers, portable schema validation |
| Курс | Косинусы + десять canonical scenes; затем mapping всех 98 записей completion map |

Полная версия завершается по этой карте, а не только по готовности косинусов.
PNG/SVG/frames/video и native/browser путям назначается compatibility matrix:
поддержка не считается доказанной до соответствующей проверки.

## Источники зависимостей пилота

cglibs/наши библиотеки: SSH Git dependencies с pinned rev, исходники не копируются.
StoryTold: физические copies используемых crates и внутренней closure в sim-widget-rs/vendor/storytold.
crates.io: обычные versions + Cargo.lock; registry vendoring/source replacement отсутствуют.
StoryTold workspace/path adaptations локальны; licenses/NOTICE/provenance/patch journal сохраняются.
Git/registry downloads и SSH доступ разрешены; offline build опционален при заполненных caches.
[Полная политика](phase-03-reuse-audit.md#источники-зависимостей).
Наличие bootstrap не доказывает готовность copies, workspace и зависимостей; они проходят пакет A.

## Ограничения текущего решения

Renderer gate 300,000 B gzip и остальные проектные gates сохраняются. При невозможности
уложиться нужны измерения и явное решение пользователя; нельзя молча увеличить бюджет.
Принятый контракт без Holo проверяется на понятности, плавности и приятном UI;
учебные объекты, числа и controls не отключаются автоматически.
Планирование и отдельная compile probe не подтверждают runtime готовность WASM библиотек;
интегрированные сборки, browser launch и измерения ещё предстоят.
