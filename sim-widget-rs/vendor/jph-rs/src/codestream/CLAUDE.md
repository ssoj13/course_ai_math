# CLAUDE.md — `jph-rs/src/codestream`

> Рабочая записная книжка. Парсинг/запись JPEG2000 codestream (маркеры, тайлы, пакеты, прогрессия).

## 🔑 РЕШАЮЩАЯ НАХОДКА (investigation)
**OpenJPH v0.26.3 САМ поддерживает только 1 quality layer** — идентичная ошибка в `ojph_codestream_local.cpp:794`
(`if (num_qlayers != 1) OJPH_ERROR(0x00030053, ...)`). jph-rs портировал это 1:1 ВЕРНО.
⇒ jph-rs НЕ отстаёт от своего референса — он точно ему соответствует. Multi-layer — это фича, которой
НЕТ в OpenJPH вообще. «До полного ISO-стандарта» = реализовать с нуля (референс — **OpenHTJ2K** osamu620,
582 conformance-теста), это НЕ порт-доводка, а новая фича сверх OpenJPH.
- SPP/MRP в блок-декодере jph-rs ЕСТЬ (порт OpenJPH), достижимы для single-layer multi-pass потоков.
- OpenJPH/jph-rs энкодеры — CUP-only by design; multi-pass/multi-layer генерит только Kakadu/OpenHTJ2K.
- Типовые `.jph` (от OpenJPH — де-факто стандарт HTJ2K) — SINGLE-LAYER. Multi-layer — нишевый interop.

## ✅ ГОТОВО — RGN (ROI max-shift, FF5E, bit-exact, сверх OpenJPH)
Нативный декод region-of-interest max-shift (OpenJPH скипает, OpenJPEG реджектит в HT).
- `ojph_params_local.rs::read_rgn` — парс Lrgn/Crgn/Srgn/SPrgn (Crgn 1б при csiz<257, иначе 2б);
  `Srgn≠0`→reject (только implicit max-shift). Потребляет ровно Lrgn байт (== skip_marker).
- Хранение: `LocalCodestream::roishift: Vec<u8>` (главный RGN) → копируется в `Tile::roishift: *mut Ui8`
  (per-comp, bump-alloc как nlt_type3) при finalize; **in-tile RGN** (idx 4 в first-tile-part loop)
  переопределяет тайл. Достать: `Resolution::get_roishift` (parent_comp→tile→comp_num).
- Применение: **`Codeblock::apply_roi`** — конвенция OpenHTJ2K `j2k_dequant_rev_scalar`
  (block_dequant.cpp): сдвиг ВВЕРХ *фоновых* коэф (`(mag & !mask)==0`, `mask=MAX>>(k_max+1)`) на
  сырой fixedpoint-магнитуде ДО `>>pLSB`. **Ловушка:** jph-rs k_max = M_b (полный бюджет, ВКЛ. ROIshift),
  поэтому НЕ наивный down-shift финального целого (как t1.c OpenJPEG) — это давало бы 0 срабатываний.
  Драйвится `Subband::pull_line` сразу после `block.decode()`. Reversible+irreversible (формула общая).
- **Ловушка (empty tile-parts):** guard `0x300B5` (multi-layer×tile-parts) ПОСЛЕ
  `data_left==0`-возврата в `parse_tile_header` → пустые trailing tile-parts (SOT+SOD, 0 пакетов;
  так Kakadu пишет RGN-потоки: 4 tile-parts, всё в части 0) декодятся.
- **✅ Multi-layer ACROSS multiple non-empty tile-parts (buffer-and-defer):** payload'ы tile-part'ов
  склеенные = байт-идентичны single-tile-part packet stream. Поэтому при `num_layers>1 &&
  TNsot!=1` `parse_tile_header` НЕ парсит пакеты сразу, а ВОЗВРАЩАЕТ сырые payload-байты
  (`Option<Vec<u8>>`); `read()` копит их в `LocalCodestream::ml_tp_buf` (Drop-managed, НЕ на arena
  Tile) и при достижении TNsot частей (или на EOC при TNsot==0) запускает sweep ОДИН раз над
  конкатенацией через `MemInfile`. Sweep вынесен в `Tile::parse_packets` (max_decomp + прогрессия,
  без seek; seek делает caller). POC поддержан: POC тайла клонируется из ПЕРВОЙ части и несётся в
  completion-sweep (p0_07: main-header POC, 8 слоёв, 256 тайлов). PPM/PPT+multipart, POC+multipart
  и POC+reduce тоже работают (заголовки PPM/PPT буферизуются вместе с телом, `Tile::defer_packets`); `0x300B5` — defensive backstop для TNsot==1+2-я часть.
  Валидация: conformance `p0_10` (4 тайла, 2 слоя, все split) и `p0_07` PORT-EXACT (maxd=0) vs oracle.
- Валидация: `ds0_ht_03_b11/b14` (256×256, 2×2 тайла, 8 слоёв+POC+SOP+RGN s=7) bit-exact vs oracle
  (`tests/rgn_probe.rs`, RGNVEC/RGNREF-gated; сравнение после level-shift+clamp как `main_dec.cpp`).
  jph-rs (как OpenJPH) отдаёт сырые сэмплы — clamp на стороне consumer.

## ✅ ГОТОВО — MULTI-QUALITY-LAYER ДЕКОД (сверх OpenJPH, все 5 порядков, bit-exact)
jph-rs декодит многослойный HTJ2K — фича, которой НЕТ в OpenJPH. Single-layer путь не тронут (bit-exact).
Валидация: bit-exact против Kakadu (`kdu_compress Cmodes=HT Clayers=N`) на всех 5 порядках,
2/4/6 слоёв, размеры 128×128 d3 и 200×130 d4 — 21 multi-layer кейс, все lossless round-trip
(тест `tests/ml_kakadu_validate.rs`, gated на `MLDIR`).

### Архитектура (новый код, single-layer не задет)
- `ojph_packet_ml.rs` — **весь multi-layer парсер** (safe Rust, owned-состояние):
  - `MlTree` — stateful tag-tree (value/current/state), переживает слои; `MlBand` (inc+zbp+blocks);
    `MlBlock{cmodes, num_zbp, pass_lengths[], this_layer_bytes, coded}`; `MlPrecinct{bands:[Option<MlBand>;4]}`.
  - `parse_subband_ml` — **точный порт** OpenHTJ2K `parse_packet_header` (coding_units.cpp:1678-2149):
    inclusion (layer-0 спец-случай при l>0) + ZBP walk + HT segment-длины (HT_PHLD/placeholder/
    segment_bytes==0/fast-skip). Хранит длины **по пассам** (`pass_lengths[]`, по одному ненулевому
    на сегмент в его последнем пассе). primary/secondary/fast_skip OpenHTJ2K — локальный throwaway,
    НЕ хранится (восстанавливается на декоде из ведущих нулей).
  - `read_subband_body_ml` — тело слоя: `this_layer_bytes` сырых байт в `MlBlock::coded` (zero-pad при truncation).
  - `finalize_block`+`finalize_band` — **порт** OpenHTJ2K `htj2k_decode` (ht_block_decoding.cpp:1110-1198):
    `P0=ведущие_нули/3`, `missing_msbs=num_ZBP+P0`, `num_passes=raw−3·P0`, `len_cup`=первый ненулевой пасс,
    `len_ref`=Σ остальных; буфер = непустые пассы подряд (cleanup‖ref) → один contiguous `next_coded`
    (декодер блока читает ОДИН буфер, цепочку `next_list` НЕ обходит — `build_coded_lists`).
- `ojph_precinct.rs` — `Precinct::parse_ml_layer` (1 слой пакета) + `finalize_ml` (после последнего слоя).
- `ojph_resolution.rs` — `parse_one_precinct_ml` (innermost: L пакетов подряд, локальный MlPrecinct);
  `parse_all_precincts_ml` (outermost: 1 слой всех precinct'ов, состояние в map).
- `ojph_tile_comp.rs` — `parse_one_precinct_ml` / `parse_precincts_ml` (навигация к resolution).
- `ojph_tile.rs` — кеш `num_layers`/`ccap15`; роутинг по `num_layers>1`:
  - **innermost** RPCL/PCRL/CPRL — слой ВНУТРИ precinct (пакеты подряд) → `MlPrecinct` локальный в вызове.
  - **outermost** LRCP (L→R→C→P) / RLCP (R→L→C→P) — precinct посещается по разу на слой не подряд →
    `HashMap<precinct_ptr, MlPrecinct>` локальный в `parse_tile_header` (без arena-владения, без утечек).
- `ojph_codestream_local.rs` — reject `num_layers!=1` **УБРАН**; `get_ccap15()` добавлен.

### Ловушки (подтверждённые)
- Декодер блока (`ojph_codeblock.rs:508`) читает блок из ОДНОГО непрерывного буфера `next_coded.buf+PREFIX`
  длиной `pass_length[0]+[1]`, цепочку `next_list` НЕ обходит → байты слоёв СКЛЕИВАТЬ, не чейнить.
- `get_buffer`/`bb_read_chunk` ЗАМЕНЯЮТ `*cur_coded_list` (single-layer = 1 узел), не аппендят.
- `CodedCbHeader.{already_included,lblock,num_passes,pass_length,missing_msbs}` стартуют 0 на fresh decode —
  ML-путь на это опирается (аккумуляция с нуля на слое 0).
- Прогрессия задаёт вложенность слоя; outermost требует персистентного per-precinct состояния.
- `segment_bytes==0` в HT_PHLD — читать доп. length-бит (иначе десинк битстрима — был баг).

### Оракулы
- **Kakadu** v8.4 (`C:\Program Files\Kakadu\Kakadu Demo-Apps`): `kdu_compress Cmodes=HT Clayers=N Corder=...`
  — ЕДИНСТВЕННЫЙ доступный энкодер multi-layer HT (OpenHTJ2K CLI хардкодит `number_of_layers=1`).
- OpenHTJ2K (`vfx.ref/OpenHTJ2K/build/bin/`) — источник истины для порта парсера/декода длин.

### Статус 2026-09-30 (см. AGENTS.md / CONFORMANCE.md)
- Multi-tile-part + multi-layer: ГОТОВО (buffer-and-defer, см. выше). MIXED-mode: ГОТОВО и валидировано (ds0_hm_*, per-block `orig_j2k`).
- Теперь общие модули: `coding/ojph_cmodes.rs` (флаги + `seg_end`), `tt_walk` (теги), `walk_pos` (позиционные прогрессии, writer+reader),
  `TileComp::get_res/with_res`, `Subband::set_quant`. PPM/PPT работают и вместе с POC (guard 0x300C3 удалён).
- Reject-список (0x300B7/BB/BC) закрыт; остаётся только 0x300B5 (malformed: TNsot==1, но есть вторая часть);
  POC (RPCL/PCRL/CPRL) теперь тоже идёт через `walk_pos` (`PocCursor`).
