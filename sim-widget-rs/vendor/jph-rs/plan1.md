# plan1.md - jph-rs bug hunt report (2026-09-30)

Status (updated 2026-09-30, end of the fix session): **approved by the user and executed on `main`** (answers to Q1-Q8: no API breaks,
CLI on `mem`, `Rounding` option, FORCE_LEVEL clamp + feature, BSD-3 kept + upstream notices, data wired into tests, work on main, push every 5 commits).
Fixed: H1 H2 H3 H4 H5 H6 H7 H8 H9, M1 M3 M4 M5 M6 M7 M8 M9 M10 M11 M13 M14 M15 (option) M16 (null derefs; `unsafe` flip deferred by policy) M17 M18 M19 M20 M21,
M12 (covered by M9), the LOW items (blanket allows, OnceLock env, div_ceil, stale TODO/docs), plus bugs found on the way: Ccap endianness / HT MIXED,
per-block ROI midpoint, Part-1 midpoint plane, Psot=0, PRCL, TLM table sized 0, irreversible-64 line element size (heap overflow on a hostile QCD).
Conformance: 73/73 vectors (tests/conf_ref.rs). Open items: see AGENTS.md "TODO ledger" (M2 `Quant` enum, `unsafe` flips, POC walk merge, a few reject guards).
The text below is the ORIGINAL audit report (kept for the file/line references; line numbers have moved).

Method: 5 read-only auditors (disjoint scopes) -> orchestrator spot-checks -> 2 independent adversarial verifiers
(30 claims re-read against code). `cargo check --all-targets --features cli,debug-extract` = 0 warnings;
`cargo clippy` = 3 trivial warnings (`src/mem.rs:714`, `examples/bench_inmemory.rs:35`, `src/bin/ojph_bench.rs:520`).
Nothing was executed (no repro runs): "confirmed" means verified by reading the code twice (auditor + verifier);
HIGH items were also read by the orchestrator. Paths are relative to the repo root (`src/` prefix omitted for brevity in tables).

Legend for V: **C** confirmed by verifier, **C+me** also read by orchestrator, **P** partly confirmed (nuance in text),
**S** single source, NOT double-checked (treat as a lead, re-verify before fixing).

## 0. Root causes (why bugs cluster)

1. **No single untrusted-input gate.** `mem::check_geometry` runs *after* `Codestream::create()`; `parse::enumerate` and the CLI have none;
   the ported core caps only tile count (65535). Header validation (`check_validity`) exists only on the *write* path.
2. **Two segment-length models and two tag-tree walkers** (single-layer `ojph_precinct.rs` vs multilayer `ojph_packet_ml.rs`) that drifted apart
   (EOF handling, caps, BYPASS).
3. **1:1-port raw pointers leak through `pub` safe fns** (`LineBuf::wrap_*`, `MemInfile::open`, `Codestream::pull_ref`), contradicting the
   "safe mem API" story in CLAUDE.md.
4. **Docs and tests drifted**: 3 different status snapshots in CONFORMANCE.md; env-gated tests pass silently.

## 1. Findings

### HIGH - memory safety / DoS / silent misdecode

| ID | V | Where | Problem | Systematic fix |
|----|---|-------|---------|----------------|
| H1 | C+me | `mem.rs:203` vs `:228`; `codestream/ojph_codestream_local.rs:456-457`; `openjph/ojph_mem.rs:203,251-256`; `parse.rs:205-206` | `create()` allocates area-scaled memory (CodedCbHeader `ojph_subband.rs:107`, precincts `ojph_resolution.rs:310`, lines) **before** `check_geometry`; `enumerate()` has no cap at all; only cap is `num_tiles<=65535`. SIZ 2^31 x 2^31 with 1x1 precincts -> abort/overflow. | One `Budget` check (checked u64 math over tiles x comps x resolutions x precincts x blocks, plus absolute cap) executed right after `read_headers()` and before `create()`; shared by `try_decode`, `enumerate`, CLI. Allocator sums use `checked_*` and return `OjphResult`. |
| H2 | C+me | `mem.rs:208-225` | `Image.width/height` = raw `Xsiz/Ysiz`, planes use `Xsiz-XOsiz` (`ojph_params_local.rs:729,983`). With XOsiz>0 `Image.width != planes[0].width`; check_geometry also sees the wrong extent. | Derive extent once from `get_recon_size` of the reference grid (same helper as `parse.rs:233`). Add offset-image test. |
| H3 | C+me | `mem.rs:345-346` | Subsampling `dx = W / plane.w` is floor division: 33 wide + 17 wide plane gives dx=1 (needs 2). No validation that `ceil(W/dx)==plane.w`; `dx as Ui8` wraps >255. | Search `dx` in 1..=255 with `div_ceil(W,dx)==plane.w`, else `EncodeError`. Same for dy. Test with odd sizes. |
| H4 | C+me | `codestream/ojph_packet_ml.rs:200-204,258-265` | `read_bit` discards the `bool` of `bb_read_bit`; at EOF `bb_read` yields zero bits with `Ok(false)` (`ojph_bitbuffer_read.rs:42-47`), so `zbp_decode`'s `while !state` never ends (u16 wraps in release = hang; debug = overflow panic). Single-layer parser errors on EOF (`ojph_precinct.rs:824`). | `read_bit`/`read_n_bits` return `Err` on EOF exactly like the single-layer path; bound ZBP walk by kmax. Fuzz test: truncate ML packet headers at every byte. |
| H5 | C (math), reasoning shown | `ojph_packet_ml.rs:209-241,345-348` | `inc_decode` reads **one** bit per node per call; T.800 B.10.2 / OpenJPEG `tgt_decode` read until threshold. Only one skipped layer is patched by the layer>0 pre-pass. Example: block first included in layer 2, layer 1 packet empty (single 0 bit, tag tree untouched): stream `0 1`, decoder reads one bit, desyncs. Same code as OpenHTJ2K, so the oracle cannot expose it. | One `tt_walk(threshold)` shared by inclusion and ZBP (loop until resolved or `current>threshold`). Round-trip test against a tag-tree *encoder* with empty-layer gaps. **Not yet reproduced on a real stream (Kakadu multi-layer with empty packets) - decide on adding such a fixture.** |
| H6 | C+me | `ojph_bitbuffer_read.rs:88-109`; `ojph_precinct.rs:598-621,875-917`; `openjph/ojph_mem.rs:415,473-474` | Single-layer Part-1 header: `lblock` unbounded, length up to 2^32-1, `num_bytes + 24` unchecked u32, no `num_bytes<=bytes_left` guard (ML has one at `ojph_packet_ml.rs:624`). Release: tiny allocation then zero-fill ~4 GB = heap corruption; or ~4 GiB alloc from a few header bytes. `bb_read_bits` only `debug_assert!(num_bits<=32)`. | Guard in `bb_read_chunk` (single point): `num_bytes<=bytes_left`, `checked_add`; `get_buffer` takes `usize`, `checked_add`, returns error; cap `lblock`; `bb_read_bits` returns Err for >32. |
| H7 | C | `ojph_tile.rs:1469,1481-1490` | Multi-layer LRCP + reduced-resolution decode: loop `for l { for r in 0..=max_decomp-skipped }` never consumes skipped-resolution packets inside layer l, so layer l+1 headers are read at wrong offsets (only POC is guarded, 0x300BC). Silent misdecode. C++ has no multi-layer, so this is our own extension. | Iterate full resolution range, skip only block decode (or return explicit error). Add test: 2-layer LRCP + `restrict_input_resolution(1,1)`. |
| H8 | C+me | `ojph_params_local.rs:682-702,658-680` | `set_num_components(10)` then `(6)`: else-branch sets `cptr=store`, `comp_data_mut()` still returns `cptr` since `old_Csiz>4` -> writes 6 entries into 4-entry store; `destroy()` frees interior pointer. No in-crate caller triggers it, but it is reachable from safe pub API; `init()` also leaks the old heap block. | Replace raw ptr + inline store by `Vec<SizCompInfo>`. |
| H9 | C | `ojph_tile.rs:1406-1411` (0x300C5) | Stale guard rejects **all** single-layer PPM/PPT streams although `phdr` is threaded (`ojph_precinct.rs:559-583`, `ojph_tile_comp.rs:216-244`). Git: the guard survived a merge after `7165f9d` threaded `phdr`; CONFORMANCE.md claims p1_06 (PPT) passes -> claim is stale for HEAD. | Delete guard, add p1_06-style regression fixture in-repo (small synthetic PPT stream), fix CONFORMANCE.md. **Verify by running the conformance vector first** (my instruction: no guessing). |

### MEDIUM

| ID | V | Where | Problem | Fix |
|----|---|-------|---------|-----|
| M1 | C | `mem.rs:401-408,340,347` | `try_encode` "no panic" promise false: short `samples` slice panics, `planes.len()>u16::MAX` wraps `Csiz`, empty planes, `bit_depth` 0 underflows (`ojph_params_local.rs:716`), depth>31 not representable in `i32` planes; `get_i32` never checks precision flag. (Verifier: the claimed i64->i32 reinterpretation on decode is REFUTED - lines are `wrap_i32` at `ojph_codestream_local.rs:603`; the real effect is silent truncation of >32-bit samples.) | `Image::validate()` (checked math, 1..=16384 planes, depth 1..=31, non-zero dims) used by `try_encode`; decode returns typed `Unsupported` for precision >31. |
| M2 | S | `mem.rs:88-96,368,390` | Lossy default `quant_step=0.0`; NaN/neg unchecked; `qfactor` silently ignored when reversible; capacity hint `w*h*n` overflows and can pre-reserve 1 GiB. | `enum Quant{Lossless,Step(f32),Qfactor(u8)}`; `checked_mul`, cap hint. |
| M3 | C | `ojph_codestream_local.rs:1980,469`; `ojph_resolution.rs:159,376-378`; `ojph_tile.rs:1469,1536` | `restrict_input_resolution` validates skip only against main COD. With COC having fewer decompositions: underflow (debug panic; release wrap = hang-like loop / component not reduced); `1u32 << skipped` overflows at 32. | Validate `skip <= min over components` after headers; `checked_sub`. |
| M4 | C | `ojph_tile_comp.rs:145,223,245,275,305` | `num_decomps - res_num` with image-wide max res: debug/overflow-check panic on **valid** mixed-decomposition COC streams (release wraps benignly). 8 copies of the same walk vs bounds-safe `get_res` (`:165-178`). | Route all through `get_res` (also dedup). |
| M5 | C | `ojph_params_local.rs:1452`; `ojph_tile.rs:1694-1696`; `ojph_codestream_local.rs:941-944` | COD/QCD `check_validity` only on write; `prog_order>=5` -> release gives blank tile with `Ok`; `num_layers==0`, `mc_trans>1` unchecked. (SIZ IS validated on read, `:952`.) | Run validity on read; explicit `prog_order<=4`, `num_layers>=1`. |
| M6 | C | `ojph_codestream_local.rs:2069-2070`; `ojph_bitbuffer_read.rs:180` | `skip_marker`: `com_len-2` u16 underflow, seek result ignored (MemInfile::seek returns -1 without moving, `ojph_file.rs:445-455`). | `com_len<2` -> Err; check seek. |
| M7 | C | `ojph_params_local.rs:3762`; `ojph_tile.rs:1295-1316,1357` | `psot-12` underflows for 1..11 -> `data_left` ~4G -> `vec![0u8; data_left]` in buffered ML route. Psot==0 (legal: last part to EOC) rejected 0x300C2 / silently dropped in resilient mode. | Reject `0<psot<14`, clamp to remaining bytes, implement Psot=0. |
| M8 | C | `ojph_codestream_local.rs:1504-1533`; `ojph_params_local.rs:2445-2451,2956-3030` | In-tile QCC seeds `tile_qcd` via `copy_quant_from` which omits main QCC chain -> other components lose their main QCC (silent misdecode); in-tile QCC `comp_idx` unchecked. | Clone the QCC chain; bounds check. |
| M9 | C | `ojph_params_local.rs:2828,3260`; `ojph_subband.rs:185,429`; `ojph_codestream_gen.rs:114,134` | exponent 0 underflow; `1u32 << (31-k_max)` for k_max>31; `requantize` never re-checks 32/64-bit class after in-tile QCD. Also silent clamp to last signalled band (`:2740-2749`). | Guard like reversible branch, `checked_sub`, error on class change / short band list. |
| M10 | P | `ojph_codestream_local.rs:1481-1500,1584-1593` | In-tile COD/COC/NLT skipped with only a warning (wrong text "PPT marker") -> silent misdecode. | Error unless byte-identical to main. |
| M11 | P | `ojph_precinct.rs:907-917`; `ojph_packet_ml.rs:459-471`; `ojph_params_local.rs:1479-1489`; `ojph_block_decoder_part1.rs:370-381` | Part-1 BYPASS without RESTART: header parsers use single-segment length model (T.800 needs per-segment lengths after pass 10). COD accepts it (comment says rejected), header desyncs for >10 passes, decoder returns false (strict: 0x300A1; resilient: silent zero block). Over-strict: <=10 passes would decode. Pure ERTERM rejected on a probably wrong premise. | One shared `seg_plan(cmodes, pass)` (OpenJPEG `opj_t2_init_seg` style) used by both parsers; decoder gets `(pass_range,len)` per segment; ERTERM = no-op. Until then reject at COD parse with a specific message. Re-test p0_13 with OpenJPEG to find the real cause. |
| M12 | P | `ojph_params_local.rs:226` (SPrgn<32 checked), `ojph_codeblock.rs:682-714`, `ojph_subband.rs:426-429` | RGN itself is validated (two auditors disagreed; verifier: checked). Residual hole: in-tile QCD raises k_max to >=31 on a BUF32 block -> `Ui32::MAX >> (k_max+1)` overflow. Narrow, hostile only. | Covered by M9 fix. |
| M13 | C | `openjph/ojph_arch.rs:205-212`; `ojph_transform.rs:~108`; `ojph_codeblock_fun.rs:~221,261` | `OJPH_CPU_FORCE_LEVEL` unclamped -> illegal instruction in any consumer (exr-core, heif-rs); documented as intentional (README:128), bench relies on it. | `min(detected, forced)` by default; unclamped only behind an explicit cargo feature; bench uses that feature. |
| M14 | P | `ojph_codeblock_fun.rs:147,178,207,233,253`; `ojph_codeblock.rs:279-284`; `ojph_subband.rs:136-141` | Irreversible + 64-bit code-block buffer (Kmax>=31, tiny base_delta) has `tx_to_cb64=None`; release silently uses the *reversible* converter on an f32 line = OOB read. C++ leaves NULL too. Reachable only with extreme quant steps. | Reject in header validation (or add `gen_irv_tx_to_cb64`); replace `unwrap_or` fallbacks by hard error. |
| M15 | P | `ojph_codestream_gen.rs:96` vs `_sse2.rs:120`, `_avx2.rs:127,140` | Lossy quantization: generic truncates, SSE2/AVX2 `cvtps` rounds-to-nearest-even; same split in C++ v0.31. Differs only when rint crosses a multiple of 2^(31-Kmax) (~1e-5..1e-6 per coeff). README claims identical J2C across tiers (true only for lossless). | **Decision needed** (Q3). Add differential unit test for values with frac>=.5. |
| M16 | C | `openjph/ojph_mem.rs:27,73-121`; `openjph/ojph_file.rs:413`; `openjph/ojph_codestream.rs:154-160,207` | Safe pub fns create UB: `wrap_i32(0x10 as *mut,..).get_i32(..)`, `LineBuf: Copy` gives aliasing `&mut`, `MemInfile::open(*const u8)` has no lifetime, `pull_ref`/`fill_and_exchange` deref null after the last line (also `pull` over-pull loops forever `:1913-1929`). | `unsafe fn` for `wrap_*`/`open`; safe `MemInfile::from_slice(&'a [u8])`; drop `Copy`; return `Option/Err`. |
| M17 | C | `bin/ojph.rs:425-486,546-559,120-170` | CLI: `-skip_res` uses full-res `get_width(0)` (OOB read of reduced lines); 16-bit path turns negative samples white (`(sample as u32).min(max)`); every option parse indexes `args[i]` after `i+=1` (panic), typos silently become defaults; `--verify` compares via `to_rgb8` and can print BIT-EXACT for different images. | **Delete duplicate driver**: CLI converts to `mem::Image` and calls `try_encode/try_decode` (extend `EncodeParams`, add `DecodeParams` for tile size/TLM/COM/skip_res/resilient); one small `Result` arg parser (reuse or delete `openjph/ojph_arg.rs`). |
| M18 | S | `openjph/ojph_file.rs:352,89,376,79,197,274,536-541`; `openjph/ojph_mem.rs:282-289` | I/O swallows errors (`unwrap_or(0)`), unbuffered 2-4 byte writes, `debug_assert!(false)` on legit OOM in `ojph_aligned_malloc` (test panics in debug), unchecked `size+alignment+header`. | Buffered wrappers, `Result` returns, `checked_add`, debug-profile test run. |
| M19 | C | `others/debug_extract.rs:176-178`; `ojph_block_decoder64.rs:275`; `ojph_codeblock.rs:643-644` | `u_ext_fired` reset only inside the 64-bit step; stale `true` leaks into later 32-bit records (doc says false). `debug-extract` feature only. | Reset once per block in `Codeblock::decode`; drop guard for `capture()`. |
| M20 | C | `ojph_block_decoder64.rs:856,871` | No `missing_msbs` upper bound in 64-bit decoder (`62 - missing_msbs`), placeholder folding can push past 62: debug panic / release garbage. | Reject `missing_msbs > 60`. |
| M21 | S | `parse.rs`, `mem.rs:249-252` | `planes[c]`/slice indexing in `try_decode` can panic if the port yields unexpected component/extra line; no bit-flip fuzz test for `try_decode`/`enumerate`. | `.get()`+`Malformed`; smoke fuzz. |

### LOW (details in section 4 / auditor notes)

Stale TODO `ojph_bitbuffer_read.rs:115-119` (premise false: p1_06 has PPT, EPH is inside packed headers; C confirmed) -> delete; `CTX_MR` unused (decoder hardcodes `[14,15,16,16]` at `ojph_block_decoder_part1.rs:219`) -> use the LUT; `ojph_div_ceil` overflow (`ojph_defs.rs:68`) -> `div_ceil`; `Lsiz-38`/`XTsiz+XTOsiz`/`Lqcd-3` debug overflows; blanket `#![allow(unused_assignments, unused_mut)]` in 7 files (`coding/*` line 1) hide dead stores; env vars re-read per code-block (`ojph_arch.rs:38-44`, `ojph_block_encoder_avx2.rs:1263`) -> `OnceLock`; AVX-512 encoder/transform never executed (BUG1.md:1041) and its fuzz test gate (F+CD) differs from dispatcher (F+CD+BW+VL); wasm dispatch lacks `cfg(target_feature="simd128")`, dead non-wasm stubs; force-override ordering bugs (`ojph_codeblock_fun.rs:241-265`); `Format400` (`ojph_params_local.rs:2132`) is truly dead (also unused in C++) -> delete or keep documented; bench `--assert-codestream` passes with one surviving tier (`ojph_bench.rs:341-372`); tests/bench_simd.rs, tests/fuzz_simd_tiers.rs, scripts/bench_cpp_vs_rust.py and `bootstrap.py i` point to wrong paths / miss `--features cli`; `ojph_arg.rs` unused; default message handler prints to stdout (`ojph_message.rs:147`); error code 0x00030024 used twice.

## 2. Unfinished / TODO map (what each one really is)

| Place | Reality | Action |
|-------|---------|--------|
| `ojph_bitbuffer_read.rs:115` TODO(part1-eph, MUST REVISIT) | Obsolete; strict EPH check is correct; p1_06 = PPT case | delete TODO + CONFORMANCE (e); regression test |
| `ojph_tile.rs:1395-1402` PPM/PPT x POC (0x300C3) | explicit Err, genuinely unfinished; trivial to thread `phdr` into `poc_parse`/`poc_emit_layer`/`Resolution::poc_parse_layer` (`ojph_resolution.rs:1594` hardcodes `&mut None`) | implement (Phase 3) |
| `ojph_tile.rs:1406` PPM/PPT single layer (0x300C5) | stale guard, see H9 | remove |
| `ojph_params_local.rs:1481-1483` BYPASS/ERTERM | see M11 | seg_plan |
| `ojph_packet_ml.rs:11-14` "items not yet wired" | stale; every item is wired (only `CTX_MR` in t1_luts is unused) | fix comment |
| `ojph_tile.rs` 0x300B5/0x300B7/0x300BC/0x300BB | explicit errors; gaps: TNsot not checked for consistency across parts, POC in later tile-part ignored on buffered route, 0x300BB falsely rejects "empty part 0, data in part 1" | Phase 3 |
| MIXED (HT+Part-1) streams, lossy multilayer, multi-component ML | ported, unvalidated (src/codestream/CLAUDE.md) | fixtures needed |
| `ojph_params_local.rs:2132` Format400 | dead | delete or doc |

## 3. Dedup / SSOT plan (no feature removed)

1. **Untrusted-input gate** (`limits`/`Budget`) - one function, three callers (mem, parse, CLI).
2. **`cmodes` module**: all coding-mode bit constants (`BYPASS/RESET/RESTART/CAUSAL/ERTERM/SEGSYM/HT/HT_MIXED/HT_PHLD`, masks `0x3F`, literal `0x004`).
3. **`seg_plan()`** + **`tt_walk()`** shared by single-layer and ML header parsers (`ojph_precinct.rs:864-917` / `ojph_packet_ml.rs:459-471`, `:209-270`); also unify `log2ceil/int_log2`.
4. **Progression iterator** `next_precinct(order, ..)` replacing 3 copies (`ojph_tile.rs:852-967, 1538-1693, 1079-1225`) and the 3 `if num_layers>1 {_ml} else {..}` blocks; `TileComp::get_res` replacing 8 walks (M4).
5. Codestream marker loops (first vs later tile-part ~150 lines) and PPT/PPM assembly (`ojph_codestream_local.rs:1417-1434`/`1637-1665`); `num_tileparts` (`ojph_tile.rs:175-201`/`326-351`); `Resolution::pre_alloc/finalize_alloc` child-rect geometry (shared `child_rects`); `ParamCod::read/read_coc`, `ParamQcd::read/read_qcc`.
6. SIMD: shared `ojph_ht_bits` (0.31 MEL/VLC/MS primitives), `simd_util` (emulated lzcnt, VLC/UVLC tables, once-warning flags); SSSE3 decoder reuses scalar `decode_step1_vlc_mel_32` (verbatim copy at `ojph_block_decoder_ssse3.rs:18-165`); Part-1 stripe-scan loops (3 copies in `ojph_block_decoder_part1.rs`).
7. CLI vs `mem` (one driver), bench `encode_once`/`encode_to_vec`, `compare_simd`/`assert_codestream_tiers`.
8. Tests: `tests/common/mod.rs` (PGM loader is copied 6x, level-shift+clamp 4x, synthetic image builder, `skip_unless_env`).

## 4. Test / CI / repo hygiene

- 8 test targets are env-gated and report **pass** when their env var is unset (`MLDIR`, `CONFFILE`, `CONFDIR`, `DQVEC`, `PPVEC`, `RGNVEC`; `conf_sweep` cannot fail; `parse_parity` is empty without `debug-extract`) -> `#[ignore = "needs X"]` or commit tiny fixtures; assert on counts.
- Missing tests: MQ known-answer (T.800/JBIG2 Annex H sequence), tag-tree encoder/decoder round trip with empty layers (H5), ML parser truncation fuzz (H4), odd subsampling / XOsiz>0 / signed / depth>8 through `mem`, bit-flip fuzz of `try_decode` and `enumerate`, Part-1 BYPASS, p1_06-like PPT, reduced-res multilayer LRCP.
- `Cargo.toml`: redundant `[[test]] planar_roundtrip`; `debug-extract` tests lack `required-features`; no `readme`/`exclude`/`[lints]`; MSRV 1.89 vs heif-rs 1.85; `usize::is_multiple_of` used (MSRV check needed - lead, unverified); `Cargo.lock` ignored although bins exist.
- **License**: `Cargo.toml` says BSD-3-Clause; upstream OpenJPH is BSD-2-Clause (Aous Naman, Kakadu, UNSW); no LICENSE file, no upstream notice in sources (C). A derived port must carry the notice.
- Tracked but unused: `_test_data/` (13 files), `test_corpus/` (24 files) (C). `.omh/` untracked and not ignored. `_ref/` is gitignored (build outputs not tracked - fine).
- `OPENJPH_REF_DIR`/`bootstrap.py parity` documented in README/PLAN/CLAUDE.md do not exist (bootstrap.py has b,t,c,i,m,x,cl only).

## 5. Documentation drift (fix in Phase 6)

PLAN.md (v0.26.3 baseline, wrong verification commands, tile-scoped markers claim), README.md (heif-rs is a git dep already; 13 test files; nonexistent commands), CLAUDE.md (`crates/jph-rs` path, 15 vs 16 `#![allow]`, module map lacks parse/ml/part1/mq, `try_*` API + `OjphError` conversion not mentioned, no local CLAUDE.md in coding/transform/openjph), src/codestream/CLAUDE.md ("multi-tile-part ML not done" contradicts its own DONE section), CONFORMANCE.md (contradictory 39/34, 22, 21/23, 11+2 snapshots; stale BYPASS/RESTART/in-tile QCD statements), DIAGRAMS.md (47-file pie, unsourced severity pie, no mem/parse/Part-1/ML) -> **DIAGRAMS.md refreshed in this session**, docs/O0_1_IMPL_SPEC.md (finished task prompt; references removed `catch_unwind`/`OjphErrorPanic`) -> archive, BUG1.md header says round 2 (file has 10 rounds).

## 6. Proposed phases (each ends with cargo check/clippy + targeted tests; commit per phase)

- **P0 Gate & safety (H1,H2,H3,H4,H6,M1,M3-M9,M20)**: `limits` module, `Image::validate`, geometry from recon size, EOF/caps in bitreader, header validation on read, Siz Vec, skip_marker/psot, QCC chain, exponent guards. Tests first (fail -> pass).
- **P1 Multilayer correctness (H5,H7,H9)**: `tt_walk`, tag-tree round-trip test, LRCP reduced-res, remove stale PPM/PPT guard after running the conformance vector, PPM/PPT x POC.
- **P2 Part-1 (M11,M12)**: `cmodes`, `seg_plan`, BYPASS/ERTERM, `CTX_MR`, delete obsolete TODO, MQ known-answer test.
- **P3 Public API & soundness (M2,M16,M21)**: `unsafe` on raw-pointer fns, `MemInfile::from_slice`, `#[non_exhaustive]` + `Clone/PartialEq` errors with OjphError code, re-export `try_*`, `Quant` enum, progression enum.
- **P4 CLI (M17,M18)**: rebuild on `mem` API, arg parser, buffered IO.
- **P5 SIMD (M13,M14,M15,M19 + LOW)**: force-level clamp, irv64 error, rounding decision, `OnceLock` dispatch, drop blanket allows (read the warnings first), dedup into shared modules, wasm/AVX-512 CI notes.
- **P6 Tests/CI/docs/license (section 4-5)**.

## 7. Decisions needed from you

1. **Q1 API break**: OK to make `wrap_*`/`MemInfile::open` `unsafe`, drop `Copy` on `LineBuf`, add `#[non_exhaustive]` to public structs/errors (heif-rs, exr-core, jph-wgpu-rs consume this crate; I will grep them first)?
2. **Q2 CLI**: rewrite `ojph` on top of `mem` (extend EncodeParams/DecodeParams) - yes/no?
3. **Q3 lossy rounding**: keep C++-identical behaviour (AVX2/SSE2 round, generic truncates; document that lossy J2C is not bit-identical across tiers) or unify on truncation (changes AVX2 bitstreams vs C++ v0.31 / the exr-rs oracle)?
4. **Q4 `OJPH_CPU_FORCE_LEVEL`**: clamp by default, unsafe override behind cargo feature `unsafe-force-simd` - OK?
5. **Q5 license**: switch to BSD-2-Clause + add LICENSE with upstream notices?
6. **Q6 data**: delete `_test_data/` and `test_corpus/` or wire them into tests?
7. **Q7 fixtures**: may I generate small Part-1 (PPT, BYPASS) and Kakadu multi-layer fixtures (Kakadu 8.4 is installed) into `tests/fixtures/`?
8. **Q8 git**: work on a dev branch, one commit per phase, push every 5 commits (per /sysgo)?

## 8. Not verified (leads only)

M2, M18, M21, `is_multiple_of` MSRV, AVX-512 correctness (never run), SIMD dedup line ranges, A9 elastic-store alignment (`ojph_mem.rs:365-366`), bench/script path claims, `ojph_message.rs` stdout default. Re-verify before touching.

## 9. Recovery notes (survive compaction)

Auditor+verifier reports lived only in transcripts; every item above already carries file:line. Working notes: `AGENTS.md` (dataflow, traps), `DIAGRAMS.md` (mermaid). Memory MCP workspace `jph-rs` holds progress. Next step: **await approval of sections 6-7**, then start P0 on a dev branch with tests first.
