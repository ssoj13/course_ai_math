# egui-value-editor — notebook

## Что это
Generic модальный deep-editor ОДНОГО типизированного значения. Извлечён из
vfx-view `panels/attr_editor.rs` (USD attribute value editor), расцеплён от USD.

## API
- `EditValue` — носитель значения в широких типах: `Bool/Int(i64)/Float(f64)/
  Vec(Vec<f64>)/Quat([f64;4] x,y,z,w)/Mat4([f64;16] row-major)/Text/Multiline/
  ArrayView(String ro)/ReadOnly(String ro)`. Хост сужает на apply (i64→u32,
  f64→f32/half, арность Vec → точный тип).
- `ValueEditor` — состояние (open/title/type_label/буферы/dirty). `open(title,
  type_label, EditValue)` seed'ит буферы; `show(ctx) -> ValueEditorResult`;
  `close()`/`is_open()`.
- `ValueEditorResult::{ Closed, Open, Apply(EditValue) }`. Apply пересобирает
  EditValue из буферов и оставляет диалог открытым (хост сам решает close).

## Связь с egui-attr-grid
`egui-attr-grid` — плоский `[label|value]` grid. Этот — модалка для ОДНОГО
сложного значения (Mat4/Quat/Array). В будущем grid может запускать эту модалку
для сложных ячеек. Пока крейты независимы.

## Особенности / ловушки
- Ints в буфере хранятся как f64 (DragValue round), наружу `Int(i64)`. Mat4
  редактируется как 16 строковых ячеек (parse f64, unwrap_or 0.0 на мусоре).
- Read-only kinds (Array/ReadOnly) никогда не dirty → Apply недоступен, rebuild→None.
- Токены из `egui-widgets-config` (`semantic(ui).weak` для type-label). Без
  хардкод-цветов/шрифтов. Body-текст из `ui.visuals()`.
- Hermetic: egui + egui-widgets-config. Никаких usd_*/pxr/app-типов.
