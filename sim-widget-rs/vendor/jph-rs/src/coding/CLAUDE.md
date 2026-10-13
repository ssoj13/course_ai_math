# CLAUDE.md — `src/coding`

> Working notebook: keep it current (what/why/traps/TODO). Parent notes: `../../AGENTS.md`.

## What this is
The entropy coders. Two families sharing one code-block buffer layout (sign @ bit 31/63, magnitude on absolute
bit-planes), so the transform/dequant stages are identical for both:
- **HT (Part 15)** — 1:1 OpenJPH port: `ojph_block_decoder{32,64,_ssse3,_avx2,_wasm}.rs`, `ojph_block_encoder{,_avx2,_avx512}.rs`,
  shared tables/primitives `ojph_block_common.rs`, `table0.rs`, `table1.rs`; scalar `ojph_block_decoder.rs` = dispatch shims.
- **Part-1 (MQ/EBCOT)** — NOT in OpenJPH; ported from OpenHTJ2K: `ojph_mq_decoder.rs` (MQ arithmetic decoder),
  `ojph_t1_luts.rs` (context tables), `ojph_block_decoder_part1.rs` (sigprop/magref/cleanup, RAW bypass passes, SEGMARK, midpoint dequant).
- **`ojph_cmodes.rs`** — SSOT for the code-block style flags (BYPASS/RESET/RESTART/CAUSAL/ERTERM/SEGMARK/HT/HT_MIXED/HT_PHLD),
  `style_ok`, `part1()`, `SEG_MODES` and `seg_end()` (BYPASS/RESTART codeword-segment model used by BOTH packet parsers and the Part-1 decoder).

## Traps
- Part-1 per-pass lengths (`seg_lengths`): one entry per pass, a contribution's bytes on its LAST pass; the decoder sums per `seg_end` segment.
- ERTERM needs no decoder support (encoder-side termination only). BYPASS: passes >= 10 that are SigProp/MagRef are RAW.
- Irreversible reconstruction midpoint is per sample (`SHIFT_P` state) except under ROI max-shift (fixed offset after the up-shift, per BLOCK).
- 64-bit decoder guards `missing_msbs <= 62`; 32-bit refuses > 30.
- SIMD tiers must produce identical codestreams (lossless) — `tests/fuzz_simd_tiers.rs`, `ojph_bench --assert-codestream`. AVX-512 encoder has
  never run on hardware (BUG1.md:1041).
- The 1:1 port files are deliberately C++-shaped (index loops, many args); idiomatic rewrites go in the non-1:1 files only.

## TODO
- [x] SSSE3 step1 reuses the scalar `decode_step1_vlc_mel_32`.
- [ ] MQ known-answer test from T.800 / JBIG2 Annex H.
