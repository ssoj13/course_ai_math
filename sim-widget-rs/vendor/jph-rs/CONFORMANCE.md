# jph-rs — аудит соответствия ISO 15444 (HTJ2K Part 15 / JPEG2000 Part 1)

> ## ТЕКУЩИЙ СТАТУС (2026-09-30) — читать первым; ниже — исторический журнал расследований
> `tests/conf_ref.rs` (`CONFDIR=… cargo test --release --test conf_ref -- --nocapture`) сверяет
> **все 73 вектора** попиксельно с `references/*.pgx` (или с выводом OpenHTJ2K для `ds*`/`u*`/`kodim*`
> через `CONFORACLE`): **73/73 OK**, `KNOWN_GAPS` пуст. Lossy-векторы держатся базовыми PAE
> (`PAE_BASELINE`); `p1_05` = 11/7/15 — ровно то же расхождение, что у самого OpenHTJ2K vs референс.
> Закрыто после записей ниже: BYPASS / BYPASS+ERTERM без RESTART (единая модель сегментов
> `seg_end`, `ojph_block_decoder_part1.rs`, общая для single-/multi-layer парсеров и декодера),
> чистый ERTERM (= один сегмент, `p0_13` bit-exact; прежний «мусор» был от ложной модели длин, не от
> MQ), PPM/PPT, HT MIXED, Part-2 CAP/PRCL, per-block J2K-диспетч, Ccap-endianness, ROI-midpoint
> per-block, `Psot==0`. `p0_08` сверяется при `DecodeParams{reduce:1}` (`mem::try_decode_with`).
> Записи «reject/⏳/🚧» про BYPASS/ERTERM/p0_13/p1_05 ниже **устарели**.

> Цель: довести jph-rs до полного стандарта. Референс полноты — **OpenHTJ2K** (osamu620,
> 582 conformance-теста ITU-T T.803). jph-rs сейчас = bit-exact порт **OpenJPH v0.26.3**,
> а OpenJPH сам неполон относительно ISO. Ниже — что отсутствует (источник: grep явных
> `"not supported yet"`/`OJPH_ERROR` в `vfx.ref/OpenJPH/src/core`).

## Найдено (investigation, 2026): чего нет в OpenJPH ⇒ нет в jph-rs

| # | Фича | Маркер/код | Где в OpenJPH | Приоритет |
|---|------|-----------|---------------|-----------|
| 1 | **Multiple quality layers** | COD num_layers, 0x00030053 | codestream_local.cpp:796 | ВЫСОКИЙ |
| 2 | **SPP/MRP refinement** (decode ЕСТЬ, не протестирован; encode нет) | num_passes>1 | block_decoder32 (есть) | ВЫСОКИЙ |
| 3 | **RGN** (Region of Interest) | 0xFF5E | codestream_local.cpp:834 (skip) | сред |
| 4 | **POC** (Progression Order Change) | 0xFF5F | :837 (skip), :962 (tile) | сред |
| 5 | **PPM** (Packed packet headers, main) | 0xFF60 | :840 (skip) | сред |
| 6 | **PPT** (Packed packet headers, tile) | 0xFF61 | :966, :1050 | сред |
| 7 | **In-tile параметр-маркеры** COD/COC/QCD/QCC/RGN/POC/PPT/NLT | — | :1026–1061 | сред |
| 8 | **Scalar derived quantization** (Sqcd&0x1F==1) | QCD/QCC | params.cpp:1640/1701 | низ |
| 9 | **NLT** не-type-3 варианты | 0xFF76 | params.cpp:1943 | низ |
| 10 | Часть профилей | PRF | codestream_local.cpp:1132 | низ |

Примечание: SIMD/мультипликации в SSE2/AVX — это внутренние оптимизации, не фичи стандарта.

## Стратегия порта (перфекционист-фундамент)
- **Энаблер:** собрать OpenHTJ2K → генерить conformance-данные + reference-декоды для ВСЕХ фич.
- Порядок: (1) multi-layer → (2) SPP/MRP (валидация) → (3) POC → (4) RGN → (5) PPM/PPT → (6) in-tile markers → (7) scalar-derived-quant → (8) NLT-варианты.
- Каждый порт сверять с OpenHTJ2K decode/reference в пределах ISO-tolerance (PAE/MSE), НЕ ломая bit-exact CUP/single-layer (все текущие тесты зелёные).
- Референс реализации: `vfx.ref/OpenHTJ2K/src` (полный) + `vfx.ref/OpenJPH/src` (для совпадающих частей).

## Часть 2: Part-1 (legacy non-HT) блоки — отдельный кодек
Для ПОЛНОГО ISO нужен и Part-1 энтропийный декодер: **MQ арифметический декодер + классический EBCOT**
(SigProp/MagRef/Cleanup на КАЖДОЙ битплоскости, до 3·N проходов). Это второй кодек целиком, нужен
только для legacy `.jp2/.j2k` и MIXED-режима (HT+legacy). У HT-блоков (`.jph`) по дизайну ≤3 прохода —
упрощённая OpenJPH-модель для них ПОЛНА. Порядок: сначала HT multi-layer (накопление lengths1/2 через
слои, эффективные проходы ≤3 + missing_msbs), затем — Part-1 MQ+EBCOT как отдельный крупный модуль.

## Статус (прогресс по порядку из §Стратегия)
- [x] Аудит пробелов (этот файл).
- [x] Сборка OpenHTJ2K (энаблер). + **Kakadu v8.4** (`C:\Program Files\Kakadu\Kakadu Demo-Apps`) —
      ЕДИНСТВЕННЫЙ доступный энкодер multi-layer/refinement HT (OpenHTJ2K CLI хардкодит 1 слой,
      reversible — только cleanup). Стенд: `kdu_compress Cmodes=HT … → jph_rs::mem::decode → сверка`.
- [x] **(1) Multi-layer HT декод — ГОТОВО, bit-exact** на всех 5 порядках (LRCP/RLCP/RPCL/PCRL/CPRL),
      2/4/6 слоёв, 2 размера (21 кейс). Реализация: `src/codestream/ojph_packet_ml.rs` + обвязка
      precinct/resolution/tile_comp/tile. Тест: `tests/ml_kakadu_validate.rs` (MLDIR-gated).
      Подробности — `src/codestream/CLAUDE.md`. Ловушка: lossless Kakadu даёт ТОЛЬКО cleanup-блоки
      (refinement распределяется не так) ⇒ multi-layer тесты SPP/MRP НЕ покрывают.
- [x] **(2) SPP/MRP refinement — ГОТОВО, провалидировано.** Refinement-пассы (`num_passes>1`) есть в
      lossy HT (Kakadu reversible даёт только cleanup). Найдены и исправлены ДВА латентных бага,
      не ловившихся т.к. ни один тест не задействовал refinement:
      1. **SIMD (AVX2+SSSE3) `rearrange_sigma`** обнулял guard-строку sigma ВНУТРИ цикла `while y<height`
         (затирая sigma каждой строки сразу после записи) — в C++ блок стоит ПОСЛЕ цикла (`y==height`).
         Симптом: PAE=68 на боевом AVX2; scalar был верен (PAE=1). Фикс: вынос блока за цикл
         (`ojph_block_decoder_{avx2,ssse3}.rs`).
      2. **scalar/64/wasm `sample_mask += sample_mask`** переполнял u32 на последней итерации
         (значение после не используется) → debug-паника overflow-check. Фикс: `<<= 1` (C++ uint wrap)
         в `ojph_block_decoder_{32,64,wasm}.rs`.
      Валидация: lossy `spp_lossy1`/`spp_lossy_ml` декодятся PAE=1 vs Kakadu на ВЫБРАННОМ CPU декодере.
      Постоянный тест: `tests/spp_probe.rs` (MLDIR-gated). Lossy cleanup-only = PAE 1 (lossy-путь был ок).
- [x] **(3) POC (Progression Order Change, FF5F) — ГОТОВО, bit-exact.** Новая реализация
      (OpenJPH POC скипает) по T.800 §B.12: `ParamPoc`/`PocEntry` парс маркера (7/9 байт/entry) +
      обобщённый итератор пакетов (`ojph_tile.rs::poc_parse`/`poc_emit_layer`): POC-entry + трейлинг
      full-volume в COD-порядке, emit-once через per-precinct next-layer счётчик, спат-merge для
      RPCL/PCRL/CPRL. Компонуется с multi-layer (`parse_ml_layer`/`finalize_ml`). 5 фиксированных
      порядков (без POC) — свои быт-в-быт пути не тронуты. Громкий reject: POC через tile-parts и
      POC+res-reduction (нет фикстур). Валидация: 4 Kakadu POC-файла (single/multi-layer,
      multi-precinct, RPCL→PCRL / LRCP→RPCL / LRCP→CPRL) — PAE=0. Тест: `tests/poc_probe.rs` (MLDIR-gated).
- [x] **(4) RGN (Region of Interest max-shift, FF5E) — ГОТОВО, bit-exact.** Нативная реализация (OpenJPH
      скипает, OpenJPEG реджектит в HT). Парс маркера (main-header + in-tile, `read_rgn` в
      `ojph_params_local.rs`): Crgn/Srgn/SPrgn, `Srgn≠0`→громкий reject (только implicit max-shift).
      Хранение per-component на codestream (`roishift`) + per-tile массив (`Tile::roishift`,
      init из главного, in-tile override). Применение — `Codeblock::apply_roi`: по конвенции
      OpenHTJ2K `j2k_dequant_rev_scalar` сдвиг ВВЕРХ *фоновых* коэффициентов (`(mag & !mask)==0`)
      на сырой fixedpoint-магнитуде ДО `>>pLSB` (k_max=M_b — полный бюджет вкл. ROI; НЕ наивный
      down-shift финального целого). Драйвится в `Subband::pull_line` сразу после `decode()`.
      **Побочный фикс:** guard `0x300B5` (multi-layer через tile-parts) переставлен ПОСЛЕ
      `data_left==0`-возврата → пустые trailing tile-parts (SOT+SOD без пакетов) больше не реджектятся
      (валидный JPEG2000; непустые subsequent части всё ещё reject). Валидация: `ds0_ht_03_b11/b14`
      (256×256, 2×2 тайла, 8 слоёв + POC + SOP + RGN s=7, 1 tile-part данных + 3 пустых)
      **bit-exact vs oracle** (после level-shift+clamp как `main_dec.cpp`). Тест: `tests/rgn_probe.rs`
      (RGNVEC/RGNREF-gated). jph-rs, как OpenJPH, возвращает сырые сэмплы — clamp на стороне consumer.
- [x] **(7) Scalar derived quantization (QCD/QCC `Sqcd&0x1F==1`) — ГОТОВО, ISO-tolerance.** OpenJPH
      реджектил. Сигналится одна SPqcd (LL/NLLL), остальные выводятся (T.800 E.1.1 / OpenJPEG
      `j2k.c`): `expn_b = max(0, expn_0 − (b−1)/3)`, mantissa общая. `read` (QCD/QCC) читает одно
      SPqcd-слово (num_subbands=1); `derived_SPqcd(idx)` синтезирует слово по формуле; потребители
      `get_kmax`/`get_irrev_delta`/`get_largest_kmax`/`get_MAGB` получили ветку irrev==1. Валидация:
      `ds0_ht_05_b11/b12` (1024×1024, 4 комп., irreversible 9/7) **PAE≤1 vs oracle** (`tests/derived_quant_probe.rs`,
      DQVEC/DQREF-gated). PAE=1 = float-rounding jph-rs(OpenJPH) vs OpenHTJ2K, в пределах ISO 9/7 gate.
- [x] **(5) PPM/PPT (packed packet headers, FF60/FF61) — ГОТОВО (layer-innermost), ISO-tolerance.**
      OpenJPH скипает оба. Заголовки пакетов читаются из маркер-буфера (PPM: глобальный поток
      `[Nppm][headers]` по tile-part; PPT: per-tile), тела — из тайл-потока. `PacketHeaderReader`
      (`ojph_packet_ml.rs`) — ПЕРСИСТЕНТНЫЙ bit-reader (tmp/avail_bits/unstuff переживают границы
      пакетов, как buf_chain OpenHTJ2K). `parse_ml_layer` разделён: header-bb (PPM/PPT) + body-bb (тайл);
      `phdr` протянут tile→tile_comp→resolution→precinct. Валидация: `ds1_ht_03` (PPM single-tile),
      `ds1_ht_05` (PPM multi-tile) **PAE≤1 vs oracle** (`tests/packed_headers_probe.rs`). Ловушка:
      без EPH заголовки — непрерывный bit-поток, fresh-bb-per-packet десинхронит (нужен персистентный).
      Работает для ВСЕХ порядков (вкл. LRCP/RLCP `ds1_ht_02` PAE≤1 после finalize-fix ниже).
- [x] **LRCP/RLCP finalize-fix (pre-existing баг, вскрыт PPT+LRCP):** если payload исчерпан ДО
      последнего слоя (энкодер выделил пустой последний слой), guard `if data_left>0` пропускал
      вызов последнего слоя → `parse_all_precincts_ml` finalize НЕ вызывался → все блоки не
      материализованы (flat). Фикс: `if data_left>0 || last` (`ojph_tile.rs` LRCP+RLCP).
- [ ] (6) in-tile markers · (8) NLT-варианты.
- [ ] multi-tile-part ML/POC (непустые subsequent части, `ds0_ht_07`) — известное ограничение (guard 0x300B5/BB).
- [ ] Part-1 MQ+EBCOT декодер (legacy, отдельный кодек, §Часть 2).

## 🔬 CONFORMANCE SWEEP (data-driven карта пробелов, заменяет угадывание)
**Энаблер:** `vfx.ref/OpenHTJ2K/conformance_data/` — 73 ISO 15444 вектора + `references/*.pgx` +
оракул `open_htj2k_dec`. Снимает зависимость от Kakadu (который запрещает RGN/PPM/PPT в HT-профиле).
**Стенд:** `tests/conf_sweep.rs` (env `CONFDIR`) — декодит всё, catch_unwind, PASS/FAIL.

**Результат (после …+RGN+derived-quant+PPM/PPT+LRCP-finalize): 39 pass / 34 fail из 73.** Причины падений — НЕ баги
в готовых фичах (multi-layer/POC работают; guard’ы ловят неподдержанное громко), а MISSING features:

| Группа | Векторы (примеры) | Причина / пробел |
|---|---|---|
| **Part-1 (MQ+EBCOT)** | `p0_*`, `p1_*`, `ds0_hm_*`, `hifi_p1_*`, `u10_part1_*` (~26) | legacy-энтропия — отдельный кодек (§Часть 2) |
| ~~HT multi-set (>3 пасса)~~ | ~~`ds0_ht_02`, `ds1_ht_01`, `ds1_ht_07`~~ | **НЕ фича — был баг COC+SOP/EPH (ИСПРАВЛЕН).** «36 пассов»/«EPH not found» — симптомы десинка; ≤3-pass модель корректна |
| ~~RGN~~ | ~~`ds0_ht_03`~~ | **ГОТОВО bit-exact** (b11/b14). `ds0_ht_08` ложно помечен — RGN там нет (data-матч FF5E); его падение по иной причине |
| ~~PPM/PPT~~ | ~~`ds1_ht_02/03/05`~~ | **ГОТОВО ВСЕ порядки (PAE≤1)** вкл. PPT+LRCP (после finalize-fix) |
| ~~derived-quant~~ | ~~`ds0_ht_05`~~ | **ГОТОВО** (b11/b12 PAE≤1). `ds1_ht_03` также требует PPM (derived-часть уже работает) |
| **EPH edge** | `ds1_ht_07` | "should find EPH" — граничный случай packet-парса |
| **CAP опции** | `4KuF_H0L1_arri` | "Pcap CAP options not supported" — расширить CAP-парс |
| **ATK/DFS DWT** | `ATK_DFS_REV/IRV` | произвольное ядро трансформа + downsampling-style — варианты DWT |
| **multi-tilepart ML** | `ds0_ht_07` | мой guard `0x300B5` — multi-layer через tile-parts (известное ограничение) |

**PASS (22):** `ds0_ht_01/04/06/09–16`, `ds1_ht_04/06`, `hifi_ht1_02`, `kodim23_*`, `u04_small`,
`u10_part15_lossless` (4K HT lossless 3840×2160 c=3!) — включая multi-layer/POC-файлы.

**Приоритеты (data-driven; все — реальные фичи, не однострочники):** PPM/PPT → derived-quant →
RGN (ROIshift во всех 4 SIMD) → HT multi-set (>3 пасса, полный цикл) → CAP/EPH edge → ATK/DFS →
Part-1 MQ+EBCOT (крупнейшее). Каждый — с готовым вектором + oracle-reference; каждый = focused-сессия.

### ⚡ VALIDATION WORKFLOW (оракул разгадан)
`open_htj2k_dec -i <vec>.j2k -o out.pgm` пишет **`out_<NN>.pgm`** (суффикс по номеру компоненты!).
Для reversible-вектора: oracle-вывод = эталон, jph-rs decode должен быть bit-exact. Стенд:
`tests/conf_sweep.rs` (PASS/FAIL), пофайлово — `tests/poc_probe.rs`-style (env SPPFILE/SPPREF, PAE).
Маппинг вектор→reference также в `OpenHTJ2K/build/CTestTestfile.cmake`. **Компаундность:** большинство
векторов задействуют НЕСКОЛЬКО фич — искать single-feature цели через профиль маркеров.

### СЛЕДУЮЩАЯ ЦЕЛЬ ПОЛНОСТЬЮ ПОДГОТОВЛЕНА: HT multi-set (`ds0_ht_02_b11`)
- **Чистая single-feature цель:** 6 слоёв (multi-layer — поддерживается) + LRCP + 1 tile, без RGN/PPM/POC.
  Единственный пробел — ">3 coding passes (36)". Reference: `ds0ht02_00.pgm` (64×126, 8-bit, reversible).
- **Корень:** блок-декодер OpenJPH (`ojph_block_decoder_{32,avx2,ssse3,wasm}.rs`) декодит ОДИН HT-set
  (≤3 пасса: cleanup+SigProp+MagRef). OpenJPH-энкодер cleanup-only → >3 никогда не встречал. Глубокий
  refinement (много битплоскостей через слои) даёт multi-set → реджект на `avx2.rs:1326`.
- **ДИАГНОСТИКА (инструментация ml-finalize):** блоки этого файла дают `pass_lengths` с **13 ненулевыми
  сегментами** (np=57, lead0=21→P0=7, но nonzero=13: pl=[0×21, 32,0,0,2,0,15,7,0,9,0,0,8,...]).
  **НО** HT-декодер OpenHTJ2K сам реджектит >4 сегментов (`htj2k_decode:1157`), а этот файл он декодит →
  значит **мой multi-layer парсер выдаёт НЕВЕРНУЮ структуру** (13 сегментов вместо ≤4), ЛИБО это Part-1/MQ
  блоки. **Приоритет #1 пересмотрен:** это может быть **латентный баг в моём закоммиченном
  multi-layer-коде** (Kakadu-фикстуры были cleanup-only → не покрывали real-refinement multi-layer).
- **✅ ИСПРАВЛЕНО (не multi-set, а COC+SOP/EPH).** Dual-parser трассировка (инструментировал
  OpenHTJ2K `parse_packet_header` + мой `parse_subband_ml` per-(layer,block)) показала: первый блок
  (LL 1×1) оракул `np=2 zbp=4`, я `np=24 zbp=0` — десинк С ПЕРВОГО пакета. Причина: `ds0_ht_02`
  имеет **COC** (FF53), а `ParamCod::packets_may_use_sop/eph` для COC возвращали `false` — SOP/EPH
  глобальны (Scod COD, COC их НЕ несёт) ⇒ SOP-маркер не скипался ⇒ байты FF91 читались как
  биты заголовка. **PRE-EXISTING баг** (общий precinct-alloc, не multi-layer-специфичный; Kakadu-
  фикстуры без COC не вскрывали). **Фикс:** precinct берёт SOP/EPH из главного COD
  (`ojph_resolution.rs:617`), не из COC. **Результат: `ds0_ht_02` bit-exact vs oracle (ndiff=0); sweep 22→30.**
  Multi-layer/POC остаются bit-exact (21+4 кейсов).

## 🏁🏁 ИТОГ Part-1: 21/23 ISO-векторов faithful (14 bit-exact + 7 PAE≤1), 0 DIVERGE
Полный sweep (сверка с OpenHTJ2K oracle):
- ✅ **PORT-EXACT (14):** p0_01/02/03/07/08/09/10/11/12/14/15/16, p1_01, p1_07.
- ✅ **PORT-PAE≤1 (7, 9/7 float-tolerance):** p0_04/05/06, p1_02/03/04/06.
- 🚫 **REJECT (2, громкий, самые сложные):**
  - `p0_13` — чистый ERTERM: **сам оракул OpenHTJ2K СЕГФОЛТИТСЯ** → невалидируем.
  - `p1_05` — PPM + BYPASS-без-RESTART + ERTERM: нужны ML-PPM-EPH + bypass-segment парсер (3 фичи).

Покрытые моды/фичи: multi-layer, multi-tile-part, POC, RGN (rev+irrev), in-tile QCD/QCC, derived-quant,
RESET, CAUSAL, SEGMARK, RESTART (single+multi-layer), ERTERM+RESTART, BYPASS+RESTART, PPT (single-layer).

## 🏁 Part-1 (legacy MQ + EBCOT) ДЕКОДЕР — РЕАЛИЗОВАН И ПРОВЕРЕН
Второй энтропийный кодек целиком, портирован с OpenHTJ2K (OpenJPH HT-only). Фазы:
- **MQ арифметический декодер** (`src/coding/ojph_mq_decoder.rs`) — shifted-representation порт
  `mq_decoder.cpp` (Areg_min=1<<23, активный байт C на битах 8..24, packed 47-state таблица).
- **EBCOT context-таблицы** (`src/coding/ojph_t1_luts.rs`) — `SIG_LUT[4][256]`/`SIGN_LUT[2][256]` +
  RL/UNI/MR контексты, авто-конвертация из `EBCOTtables.hpp` (без ручной транскрипции).
- **Три bit-plane прохода** (`src/coding/ojph_block_decoder_part1.rs`) — sigprop/magref/cleanup
  (+run-length) + MQ sign-coding + драйвер плоскостей. Textbook per-sample σ-сетка (без packed
  block_contexts/broadcast), прямой индекс в LUT. Вывод в раскладке HT-декодера (знак@31, магнитуда
  на абсолютных битплоскостях) ⇒ `tx_from_cb32` + iDWT/RCT переиспользуются БЕЗ изменений.
  Ловушка (поймана ревью): проходы ОБЯЗАНЫ идти в 4-строчном **stripe**-порядке, не столбцом.
- **Интеграция**: relax SIZ Rsiz 0x4000 + COD/COC block_style (Part-1 допускает RESET/CAUSAL/SEGMARK,
  reject BYPASS/RESTART/ERTERM — мульти-сегментные, нужны per-pass длины); ветка модели длин/проходов
  в single-layer (`ojph_precinct.rs`) И multi-layer (`ojph_packet_ml.rs`, `is_ht` через `MlBlock`):
  Part-1 = один MQ-сегмент на все проходы, missing_msbs только из tag-tree, без HT placeholder-folding.
  Диспетч `Codeblock::decode` → `decode_codeblock_part1` для non-HT 32-бит блоков (orientation из band).

**Валидация (ISO 15444-4 conformance, сверка с собственным выводом OpenHTJ2K = верность порта):**
- ✅ **PORT-EXACT (11, reversible bit-exact, вкл. 12-бит и multi-tile-part):** `p0_01/03/07/08/09/10/11/14(3c)/15/16`, `p1_07(2c)`.
  `p0_03` = 8 слоёв signed-4bit; `p0_08` = 12-бит signed 5/3; **`p0_10` = 4 тайла × 2 слоя × multi-tile-part;
  `p0_07` = 2048², 256 тайлов × 8 слоёв + POC, split tile-part** (multi-tilepart-ML buffer-and-defer, ниже).
- ✅ **PORT-PAE≤1 (2, 8-бит 9/7, float-tolerance — ожидаемо для irreversible):** `p0_05`, `p1_02`.
- 🚧 **Легитимный reject (6):** `p0_02/04/12/13`, `p1_01/03/05` — BYPASS/RESTART/ERTERM (мульти-сегмент, MVP-defer).
- ✅ **irreversible + RGN — ИСПРАВЛЕНО:** `p0_06` теперь PORT-PAE≤1 (был maxd 590). Под ROI
  reconstruction-offset фиксирован (`1<<(pLSB−1)`, N_b=M_b) и добавляется в `apply_roi` после up-shift
  для ВСЕХ ненулевых (gated `!reversible && !is_ht`); per-sample bake подавлён при roishift≠0.
- ⏳ **Остаётся:** (a) ~~irreversible+RGN~~ (выше, исправлено)
  с ROIshift=11. ПОД ROI формула реконструкции OpenHTJ2K (`block_dequant.cpp`) переключается на
  ФИКСИРОВАННЫЙ offset `1<<(pLSB−1)` (`N_b=30−pLSB+1`), добавляемый ПОСЛЕ ROI up-shift фона;
  мой Part-1 бакает per-sample offset ДО `Codeblock::apply_roi` ⇒ неверно при ROIshift. Фикс =
  прокинуть ROIshift+k_max в декодер + упорядочить offset↔apply_roi (системная задача, общая с HT).
  (b) **`p1_04` ROOT-CAUSE ИСПРАВЛЕН (был мисдиагноз multi-tilepart):** p1_04 = 64 ОДНО-tile-part тайла
  («296 SOT» = ложные FF90 в данных). Тайл 0 верный, 1-63 — константная per-tile DC-ошибка, потому что
  тайлы 1-63 несут **in-tile QCD** (per-tile квантование), а jph-rs его **ПРОПУСКАЕТ** (codestream_local.rs
  skip_marker «QCD … not supported yet»). `finalize_alloc` ставит k_max/delta из main-QCD ОДНОКРАТНО
  (read:1206→536) до тайлов. ФИКС = читать in-tile QCD/QCC/COD/COC (сейчас все skip) + per-tile recompute
  квантования субполос тайла. (known gap «in-tile параметр-маркеры».) (c) BYPASS/RESTART/ERTERM (6 векторов, мульти-сегмент, MVP-defer).
- ✅ **multi-tilepart-ML — ГОТОВО (buffer-and-defer):** ML-тайл с пакетами через несколько tile-part’ов:
  пайлоады буферизуются на `LocalCodestream` (Drop-managed, НЕ на arena-`Tile`), при завершении
  (TNsot или EOC) конкатенация прогоняется ЧЕРЕЗ валидированный one-shot свип (`Tile::parse_packets`)
  один раз (конкатенация = байт-идентична single-tile-part потоку). POC через tile-parts теперь работает.
  Reject: PPM/PPT+ML+multipart (0x300B7), POC+reduced-res (0x300BC). Нуль риска для 21 ML-кейса.
  **(e) ⚠ p1_06 EPH/empty-packet десинк — TODO В КОДЕ (`ojph_precinct.rs::parse`, `bb_skip_eph`), ОБЯЗАТЕЛЬНО ВЕРНУТЬСЯ.**
  Крошечный multi-tile SOP+EPH-поток, первый пакет читается как empty (leading bit 0), но EPH в потоке
  нет (пустые пакеты = 0 байт между SOP). Ленивый EPH проходит EPH, но дальше падает декод блока
  (0x300A1) — корень в модели заголовка пакета, не в EPH. Нужна инструментация OpenHTJ2K read_packet.
- Примечание: irreversible reconstruction теперь per-sample (SHIFT_P в state, `1<<(plane−1)` по последней
  декодированной плоскости каждого сэмпла) — точный порт `j2k_dequant` для non-ROI случая.

### Частично ГОТОВО / TODO: Part-1 multi-segment моды (BYPASS/RESTART/ERTERM)
- ✅ **RESTART single-layer — ГОТОВО (`p0_12` PORT-EXACT bit-exact).** Каждый проход = свой
  codeword-сегмент; парсер читает `num_passes` длин по `Lblock` бит, хранит в `CodedCbHeader.seg_lengths`
  (raw-указатель на elastic-массив, Copy-safe); декодер `mq.init(seg_pos,len,false)` на каждый
  проход (states только при z==0/RESET). Multi-layer RESTART отклоняется (0x300C9).
- ⏳ **Остаётся:** BYPASS (raw-проходы `get_raw_symbol`, p1_03/05), multi-layer RESTART (p0_04, 20 слоёв —
  нужна ML-ветка seg-длин), ERTERM (p0_13 — нужна MQ predictable-termination; оракул сам сегфолтит).
- Правила кодирования длин (OpenHTJ2K `coding_units.cpp:1667-2149`, `block_decoding.cpp:500-553`):
- **RESTART (0x04) — реализовано:** ОДИН сегмент на проход; длина = `Lblock` бит; N длин на блок.
- **BYPASS (0x01):** порог=10. Проходы 0..9 — MQ (один сегм, `Lblock+floor(log2(passes))`). z≥10 RAW:
  SigProp/MagRef парами (2 прохода, `is_bypass`, `get_raw_symbol`), Cleanup один (MQ). Длина raw-сегм
  = `Lblock+floor(log2(seg_passes))` (SigProp/MagRef) или `Lblock` (Cleanup).
- **ERTERM (0x10):** НЕ меняет число длин в заголовке, НО predictable termination рвёт MQ байт-поток
  per-pass — нужна поддержка терминации в MQ (`mq_decoder.cpp:302` TODO). **Проверено: single-segment passthrough
  ДАЁТ МУСОР (1×1 c=257), и сам оракул OpenHTJ2K СЕГФОЛТИТСЯ на p0_13** — ERTERM не реализован и там.
- Хранение: per-PASS массив (длина на ПОСЛЕДНем проходе сегмента, ранее — 0); декодер суммирует по сегменту.
- **БЛОКЕР АРХИТЕКТУРЫ:** `CodedCbHeader.pass_length` = `[Ui32; 2]` и структура `#[derive(Copy)]` — не
  вмещает переменное число per-pass длин (Vec ломает Copy). Нужно либо side-storage на LocalCodestream
  (HashMap, как `ml_tp_buf`), либо расширение хранилища. Декодер-драйвер (segment-loop) и raw-проходы
  (`get_raw_symbol`) уже понятны — осталось хранилище + парсер per-mode. **ОБЯЗАТЕЛЬНО ВЕРНУТЬСЯ.**

Итог: **реверсивный Part-1 путь полностью bit-exact** (11 ISO-векторов, вкл. multi-layer, multi-tile-part и 12-бит),
8-бит irreversible — в пределах tolerance. Остаток — узкий 12-бит-irreversible + multi-tilepart-ML.
HT-путь байт-идентичен (вся регрессия зелёная). Триаж: `tests/conf_one.rs` (CONFFILE/CONFDUMP, дамп
≤8бит=u8 / >8бит=u16-LE для сверки с .pgx/oracle).
