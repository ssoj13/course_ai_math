# AGENTS.md - jph-rs working notebook

> Keep this file current: every significant discovery about this crate goes here (TODOs with [ ] boxes, traps, dataflow).
> Companion files: `CLAUDE.md` (purpose/API summary), `DIAGRAMS.md` (mermaid), `plan1.md` (bug-hunt report + phase status),
> `CONFORMANCE.md` (ISO vectors; read its top status block first), `BUG1.md` (rounds 1-10 history), `src/*/CLAUDE.md` (per-directory notes).

## What this is
Pure-Rust port of OpenJPH (HTJ2K) that grew past it: Part-1 (MQ/EBCOT) decoder incl. BYPASS/RESTART/ERTERM, HT MIXED, Part-2 PRCL/CAP,
multi-quality-layer decode, RGN, PPM/PPT (also with POC), in-tile QCD/QCC, reduced-resolution decode, parse-only front-end for the GPU port.
Baseline v0.26.3 + 0.31.0 backports (AVX2 encoder/decoder rewrite, COM version, COC sizing, 64-bit irv).
Consumers: `exr-core` (per-line protocol), `heif-rs` (`mem` API, git dep), `jph-wgpu-rs` (`parse::enumerate`, `debug-extract` feature).
License: BSD-3-Clause + upstream BSD-2 (OpenJPH) / BSD-3 (OpenHTJ2K) notices in `LICENSE`.

## Layers
```
Layer 2 (safe, idiomatic)   mem.rs  try_decode / try_decode_with(DecodeParams) / try_encode / try_encode_with(EncodeOptions) / decode / encode
                            parse.rs  enumerate()
Layer 1 (1:1 port, unsafe)  openjph/ (Codestream, params, file, mem, arch)   codestream/  coding/  transform/
CLI (feature `cli`)         bin/ojph.rs  = thin driver over mem (encode|e decode|d convert|c verify|v);  bin/ojph_bench.rs = SIMD bench
```
New code (mem, parse, packet_ml, part1, mq, cmodes, ojph.rs) is idiomatic and may be refactored; the 1:1 port keeps C++ structure on purpose.

## Decode dataflow (mem::try_decode_with)
```
bytes -> MemInfile::open(ptr,len)
      -> [resilient] Codestream::enable_resilience
      -> Codestream::read_headers            SOC SIZ CAP COD/COC QCD/QCC RGN POC PPM TLM COM NLT ATK DFS   (params_local.rs; COD validity checked on read)
      -> [reduce>0] restrict_input_resolution (validated vs the SMALLEST per-component decomposition count)
      -> mem::check_siz                      budget gate (MAX_IMAGE_PIXELS) BEFORE any geometry-sized allocation
      -> Codestream::create                  pre_alloc/finalize_alloc: bump arena (ojph_mem.rs)
      -> loop pull_ref(&mut comp) per line:  (Err, not null, past the last line)
           Tile::parse_tile_header (first pull): SOT/SOD, tile-part loop, then Tile::parse_packets
              progression: LRCP/RLCP (resolution/layer loops)  |  RPCL/PCRL/PRCL/CPRL via walk_pos (shared with the writer)  |  POC via poc_parse
              single-layer: Precinct::parse  -> parse_header (HT: 2 segment lengths; Part-1: cmodes::seg_end segment lengths)
              multi-layer : ojph_packet_ml.rs  (MlTree + tt_walk, MlBlock{style}, finalize_band); outermost LRCP/RLCP keep HashMap<precinct,MlPrecinct>
              multi-tile-part + ML: payloads buffered in LocalCodestream::ml_tp_buf, swept once at TNsot parts / EOC
              skipped (dropped) resolutions: headers parsed, bodies stepped over (Resolution::skipped_res_for_read)
           Subband::pull_line -> Codeblock::decode -> [HT: block_decoder{32,64,ssse3,avx2,wasm} | Part-1/MIXED-J2K: block_decoder_part1 + mq_decoder]
              -> Codeblock::apply_roi (RGN) -> tx_from_cb (dequant) -> Resolution inverse DWT -> colour inverse
      -> Plane.samples[row]   (raw samples: NO clamp for lossy / reduced overshoot; consumers clamp)
```
## Encode dataflow (mem::try_encode_with)
```
Image -> check_image (sizes, depth<=31, ceil subsampling) -> SIZ (tile_size option) / COD / QCD -> [tlm][comment][tilepart divisions]
      -> Codestream::write_headers (validity incl. irreversible >32-bit refusal) -> exchange/fill_and_exchange per line
      -> colour fwd -> DWT fwd -> tx_to_cb (quantize; Rounding::Cpp = tier dependent, Rounding::Trunc = scalar truncation on every tier)
      -> Codeblock::encode (HT block_encoder{,_avx2,_avx512}; CUP-only) -> Tile::flush (walk_pos for RPCL/PCRL/CPRL) -> MemOutfile -> Vec<u8>
```
## Parse-only front-end (parse::enumerate)
`read_headers -> check_siz -> create -> walk tiles/packets/tag-trees -> ParsedImage{CodecConfig, CompInfo, CodeblockPlan}` (no entropy decode, no DWT).
Byte-exact vs debug_extract oracle (tests/parse_parity.rs, feature debug-extract).

## Single sources of truth (do not re-grow copies)
- `coding/ojph_cmodes.rs`: style flags, `style_ok`, `part1()`, `SEG_MODES`, `seg_end()` (BYPASS/RESTART segment model: parsers AND decoder).
- `ojph_packet_ml.rs::tt_walk`: the only tag-tree decode (inclusion + ZBP).
- `ojph_tile.rs::walk_pos/pick_pos` + `PrecinctCursor` (`TileCursor` / `PocCursor`): the only position-driven packet iterator (writer, reader, POC).
- `TileComp::get_res/with_res`: the only resolution lookup. `Subband::set_quant`: the only k_max/delta derivation (alloc + in-tile requantize).
- `mem::check_siz`: the only untrusted-SIZ gate. `ParamCod::block_style_ok` -> `cmodes::style_ok`.

## Traps (verified)
- Block decoder reads ONE contiguous buffer per block (`next_coded`): multilayer bytes are concatenated, not chained.
- `bb_read` at EOF returns zero bits with `Ok(false)`; callers MUST check the bool.
- Irreversible 64-bit code-blocks feed an **f32** line (`gen_irv_tx_from_cb64` writes floats): element size 4, not 8.
- Part-1 per-pass lengths: a contribution's bytes sit on its LAST pass (zeros elsewhere); decoder sums per `seg_end` segment.
- Ccap is big-endian in the stream (`get_ccap_part(15)` index = popcount of Pcap bits above); MIXED detection depends on it.
- ROI max-shift: the decision "reconstruct with midpoint" is per BLOCK (HT-MIXED may mix J2K and HT blocks), not per component.
- `OJPH_CPU_FORCE_LEVEL` is clamped to the detected tier unless built with `--features unsafe-force-simd`.
- Default `cargo test` has overflow checks: valid-stream arithmetic must not wrap (the bit-flip fuzz runs there too).
- Duplicate error codes inherited from the C++ port (e.g. 0x300B7/0x300C3 IMF/broadcast vs ours) - cosmetic, see plan1 LOW.
- `MAX_IMAGE_PIXELS` (268M samples) still lets one hostile header request ~1 GB of planes; lower it if a consumer needs tighter bounds.

## Build / verify
`cargo test --release` (all green; env-gated probes are `#[ignore]`d and panic without their env).
Conformance: `CONFDIR=D:/Projects/vfx.ref/OpenHTJ2K/conformance_data [CONFORACLE=<oracle pgx dir>] cargo test --release --test conf_ref -- --ignored --nocapture` -> 73/73 OK.
Oracle outputs: `open_htj2k_dec.exe -i X.j2k -o dir/X.pgx [-reduce n]` (D:/Projects/vfx.ref/OpenHTJ2K/build/bin). Kakadu 8.4 at `C:\Program Files\Kakadu\Kakadu Demo-Apps`.
Clippy: `cargo clippy -p jph-rs --lib` = 0 warnings; the 7 blanket `allow(unused_*)` in coding/ are gone.
CLI: `cargo run --release --features cli --bin ojph -- --help`. Bench: `--bin ojph_bench -- --compare-simd --assert-codestream`.

## TODO ledger (see plan1.md for IDs)
- [x] P0 gate/safety: H1-H4, H6, H8, M1, M3-M10, M13, M14, M16 (pull_ref/exchange), M18, M19-M21, TLM sizing bug (found while testing options)
- [x] P1 multilayer: H5 tt_walk + encoder round-trip test, H7 reduced-res (Kakadu regression), H9, PPM/PPT x POC (Kakadu fixtures)
- [x] P2 Part-1: cmodes SSOT, seg_end (BYPASS), ERTERM, p0_13/p1_05/p0_08 conformance (73/73)
- [x] P4 CLI on mem; Rounding option (Q3); force-level clamp (Q4); LICENSE (Q5); data wired into tests (Q6); work on main, push every 5 commits (Q8)
- [x] dedup: position-driven iterator, get_res, set_quant, cmodes; `OJPH_FORCE_*` read once
- [x] API break done (0.2.0, see CHANGELOG.md): `Quant`, `Progression`, `MemInfile<'a>`, `unsafe wrap_*`, `LineBuf` !Copy, `DecodeError` non_exhaustive; consumers migrated
- [x] POC positional entries (RPCL/PCRL/CPRL) now run through `walk_pos` with a `PocCursor` (LRCP/RLCP POC loops stay separate)
- [x] POC + reduce (guard removed, bit-exact vs oracle and kdu_expand)
- [x] POC across tile-parts (deferred single sweep, Kakadu fixture)
- [x] multi-layer / POC multi-tile-part with PPM/PPT (headers buffered and concatenated; Kakadu fixtures)
- [x] Part-1 stripe scan unified (`scan`); [x] SSSE3 step1 now reuses the scalar one (verified at OJPH_CPU_MAX_LEVEL=5, 73/73)
- [ ] AVX-512 encoder/transform never executed on hardware (BUG1.md:1041); wasm dispatch cfg; `ojph_arg.rs` (unused C++ arg-parser port) removed
- [x] tests/common (read_pnm, pgm, synth_*); [ ] docs/O0_1_IMPL_SPEC.md archive
- [x] perf: `examples/tile_bench.rs` (+ `scripts/tile_bench.cpp` C++ twin), lzcnt env latch, `VecOutfile` (CLAUDE.md "Performance")
- [ ] perf: re-arm a codestream for an identical-geometry tile without rebuilding the tile tree (`read()` ~4-5% of tile decode)
- [ ] perf: HT block decoder (69% of decode) vs C++ OpenJPH codegen gap; `walk_pos` cands Vec per resolution (6 allocs/tile)
- [ ] CAP Ccap15: OpenJPH 0.31 writes 0x0202 where we write 0x0002 for these exv streams (1 byte) - check which is right
