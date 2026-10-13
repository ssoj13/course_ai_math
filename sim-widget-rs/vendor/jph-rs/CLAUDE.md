# CLAUDE.md — `jph-rs`

## Purpose
First-party pure-Rust 1:1 port of the OpenJPH core (HTJ2K / JPH codestream), **now grown past
OpenJPH** toward fuller ISO conformance (NOT a vendored copy — we extend it).
Used by `exr-core` for the HTJ2K codec (EXR's `HT256` compression).
**Ports:** OpenJPH v0.26.3 baseline — <https://github.com/aous72/OpenJPH/tree/v0.26.3/src/core> —
plus targeted fixes backported from v0.31.0 (the version the real C++ OpenEXR 3.4.15 oracle
links, per vcpkg): the embedded build-version `COM` comment (`ojph_version.rs`), the
COC-aware precinct scratch buffer sizing, and the missing 64-bit irreversible
`tx_from_cb64` conversion. See `BUG1.md` for the full 0.26.3->0.31.0 delta triage
(what was ported, what was skipped and why, what remains as follow-up work).

## Who uses it
- `exr-core` — the HTJ2K encode/decode paths call into this crate directly (per-line protocol).
- `heif-rs` (`heif-codec-j2k`) — HTJ2K-in-HEIF, via the **`mem`** high-level API (path dep `../jph-rs`).

## High-level `mem` API (Layer 2)
`src/mem.rs` exposes whole-image, **safe** (no raw pointers, no `unsafe` at call sites) helpers for
consumers that have an image in memory: `decode(&[u8]) -> Image`, `encode(&Image, &EncodeParams) ->
Vec<u8>` (panicking) and the fallible `try_decode` / `try_decode_with(&DecodeParams{reduce,resilient})` /
`try_encode` / `try_encode_with(&EncodeOptions{rounding,tile_size,tlm,comment,tilepart_*})`, with `Image { width, height, planes: Vec<Plane{width,height,bit_depth,signed,samples:Vec<i32>}> }`.
Built on the existing safe Layer-1 primitives (`LineBuf::get_i32[_mut]`, `Codestream::pull_ref`/
`fill_and_exchange`) — all `unsafe` stays encapsulated in the 1:1 port. Re-exported from the crate
root. Added `ParamSiz` usage note: `get_num_components()` returns `u16` (raw `Csiz`).

## Parse front-end + Part-1 / conformance (on `main`, commit e05e88a)
- **`jph_rs::parse::enumerate(&[u8]) -> Result<ParsedImage>`** — parse-only per-codeblock
  enumeration front-end: walks markers / packets / precincts / tag-trees down to per-block coded
  bytes + geometry, **without** entropy-decoding. **Byte-exact vs the `debug_extract` oracle.**
  This is the geometry/coded-bytes tap consumed by `jph-wgpu-rs` (its `parse` module wraps this).
- **Full Part-1 (legacy J2K, MQ/EBCOT) decoder** alongside the HT (Part-15) path.
- **ISO conformance: 73/73 vectors** (`tests/conf_ref.rs`, `--ignored` + `CONFDIR`): BYPASS/RESTART/ERTERM,
  HT MIXED, PRCL/CAP, multi-layer, POC, PPM/PPT (+POC), RGN, in-tile QCD/QCC, reduced resolution.

## Module map
- `src/{openjph,codestream,coding,transform,others}/` — 1:1 `ojph_*.rs` modules mirroring C++ structure
  (new non-1:1 modules inside: `codestream/ojph_packet_ml.rs` multi-layer parser, `coding/ojph_block_decoder_part1.rs`
  + `ojph_mq_decoder.rs` + `ojph_t1_luts.rs` Part-1, `coding/ojph_cmodes.rs` style flags + segment model)
- `src/mem.rs` — high-level whole-image safe API (Layer 2; NOT a 1:1 port — idiomatic safe Rust)
- `src/parse.rs` — parse-only front-end (`enumerate`)
- `src/bin/ojph.rs` — CLI over `mem` (`encode|e decode|d convert|c verify|v`); `src/bin/ojph_bench.rs` — SIMD bench
- `AGENTS.md` (dataflows, SSOT list, traps, TODO ledger), `DIAGRAMS.md`, `plan1.md`, `CONFORMANCE.md`, `PLAN.md`, `README.md`
- `tests/data/{corpus,ict,kakadu}` — committed fixtures used by `tests/corpus.rs`

## How it works
- SIMD tiers (sse/avx2/avx512/wasm) produce identical LOSSLESS J2C; lossy is identical too with `Rounding::Trunc`
  (default `Rounding::Cpp` = OpenJPH behaviour: scalar truncates, SSE2/AVX2 round).
- `OJPH_CPU_FORCE_LEVEL` is clamped to the detected tier (feature `unsafe-force-simd` lifts it). See `README.md`.

## MSRV
Per-crate `rust-version = "1.89"` (std x86 SIMD intrinsics stabilised in 1.89).
The workspace floor is 1.85; the override is scoped here so the rest of the workspace
stays at 1.85 and avoids tripping unrelated MSRV-gated lints elsewhere.

## Clippy policy
`cargo clippy -p jph-rs --lib` = 0 warnings. Two categories suppressed intentionally:
- **`incompatible_msrv`** — resolved by the per-crate MSRV override above, not by `allow`.
- **1:1-port lints** (`not_unsafe_ptr_arg_deref`, `missing_safety_doc`, `identity_op`,
  `too_many_arguments`, `excessive_precision`, C-style index loops, explicit casts, …) —
  16 crate-level `#![allow]`s in `src/lib.rs`. Fixing these to idiomatic Rust would deviate
  from the C++ structure and break 1:1 fidelity. They are clippy-only; `cargo build` is clean.

## Performance (exv-like tiles; `examples/tile_bench.rs`, Ryzen 9 5900X AVX2, 1 thread, lto=thin cgu=1)
Bench drives jph exactly like exv-rs (pooled `Codestream` + `restart()`, rev 5/3, 5 levels, 64x64 HT,
NLT on float bit patterns). `--check` = bit-exactness gate (fingerprint `27fb650354386405`, 90 cases);
`scripts/tile_bench.cpp` = the same protocol against C++ OpenJPH 0.31 (vcpkg static lib).
- Baseline MB/s per thread (dec/enc): 256x5 F16 synth ~330/250, 512x5 F16 ~320/260, 256x3 U8 ~195/120,
  U16 ~350/240, U32/F32 noise ~290/155. Rust vs C++ 0.31 on the same streams: parity to ~25% slower
  (C++ best-of faster on 16-bit/NLT cases); the machine was loaded, so treat as +-15%.
- Decode stage split (256x5 F16, samply): HT block decode 69% (incl. destuff/MEL/memset), inverse DWT 10%
  (horz_syn 6, vert_step 3), dequant `rev_tx_from_cb32` 6%, line/subband/tile pull plumbing 8%,
  `read()`/`create` + packet parse 5%, coded-byte copy (`MemInfile::read`) 2%, NLT 2%.
- Encode: HT block encoder (+ms_drain/vlc_encode) ~70%.
- Allocation churn per tile (warm pooled codestream): decode 7 allocs / ~0 KB (6 are `walk_pos` cands Vecs);
  encode 133 allocs / 976 KB -> 7 / 0 KB after (a) `OJPH_AVX2_LZCNT` read once (was `env::var` + env lock per
  code-block) and (b) `VecOutfile` (append into the caller's reused Vec; no MemOutfile buffer, no copy-out).
  A COLD codestream allocates ~4.2 MB for a 256x5 tile (1 MiB elastic + ~3.1 MB fixed arena) - consumers
  must keep one codestream per worker thread alive (exv-rs does).
- Ideas not done (measured share above): re-arm without rebuilding the tile tree (`read()` ~4-5%), fused
  typed/strided output in `pull` (line copy ~3% consumer side), block decoder vs C++ codegen gap.

## Conventions
→ repo-root `CLAUDE.md` (full list).

## Update rule
Record integration facts (re-exports used by exr-core, version bumps) in THIS file.
