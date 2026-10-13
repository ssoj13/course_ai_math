# jph-rs

Pure-Rust **1:1 port** of [OpenJPH](https://github.com/aous72/OpenJPH) v0.26.3 core for
HTJ2K / JPH codestream processing, **grown past upstream** toward fuller ISO conformance. Zero
library dependencies — the codec is pure `std`; `image` is an optional, CLI-only dep. MSRV **1.89**
(uses std x86 SIMD intrinsics stabilised in 1.89).

The 0.26.3 baseline carries a handful of targeted backports from OpenJPH v0.31.0 (the version
real C++ OpenEXR 3.4.15 links against, via vcpkg) — the embedded build-version comment marker,
COC-aware precinct scratch sizing, and the missing 64-bit irreversible codeblock conversion. See
[`BUG1.md`](BUG1.md) for the full delta triage against v0.31.0 (ported / skipped / follow-up).

## Features

- **HTJ2K (JPEG 2000 Part 15)** encode + decode — bit-exact with the C++ reference.
- **Legacy JPEG 2000 Part 1** (MQ / EBCOT) decode, alongside the HT path.
- **Parse-only front-end** — `jph_rs::parse::enumerate(&[u8]) -> ParsedImage`: walks markers /
  packets / precincts / tag-trees down to per-codeblock coded bytes + geometry **without**
  entropy-decoding (byte-exact vs the internal `debug_extract` oracle). Consumed by GPU decoders
  (`jph-wgpu-rs`) that run entropy on-device.
- **ISO conformance**: all 73 vectors of the OpenHTJ2K conformance set decode pixel-correct
  (`tests/conf_ref.rs`): Part-1 incl. BYPASS/RESTART/ERTERM, HT MIXED, Part-2 PRCL/CAP, multi-layer,
  multi-tile-part, POC, PPM/PPT (also with POC), RGN, in-tile QCD/QCC, reduced-resolution decode.
- **High-level whole-image `mem` API** (`decode` / `encode`, `try_*` variants, `DecodeParams`
  `{reduce, resilient}`, `EncodeOptions` `{rounding, tile_size, tlm, comment, tilepart_*}`; safe — no
  `unsafe` at call sites).
- **SIMD tiers** (SSE/AVX2/AVX-512/WASM): lossless J2C is identical across tiers; lossy output is too
  with `Rounding::Trunc` (the default `Rounding::Cpp` keeps OpenJPH's tier-dependent rounding).

## Reference

- Upstream: <https://github.com/aous72/OpenJPH>
- Port baseline tag: `v0.26.3` (`src/core`)
- Parity plan: [PLAN.md](./PLAN.md); architecture notes: [AGENTS.md](./AGENTS.md), [DIAGRAMS.md](./DIAGRAMS.md)
- License: BSD-3-Clause with the upstream OpenJPH (BSD-2) / OpenHTJ2K (BSD-3) notices, see [LICENSE](./LICENSE)

## Usage

Standalone crate, consumed via git-ref:

```toml
[dependencies]
jph-rs = { git = "ssh://git@github.com/ssoj13/jph-rs.git", branch = "main" }
```

Consumed by `exr-rs` (`exr-core` — the EXR `HT256` HTJ2K codec), `heif-rs` (HTJ2K-in-HEIF) and
`jph-wgpu-rs` (`parse::enumerate`). The high-level whole-image API is in `src/mem.rs`
(`decode` / `encode`, safe, no `unsafe` at call sites); all `unsafe` stays encapsulated in the 1:1
port underneath.

## Build & test

```bash
python bootstrap.py b   # build
python bootstrap.py t   # test
python bootstrap.py c   # check / clippy
cargo test --release    # everything; env-gated conformance probes are #[ignore]d
```

Conformance (needs the OpenHTJ2K conformance set, not redistributed):

```bash
CONFDIR=<dir with .j2k/.j2c + references/*.pgx> cargo test --release --test conf_ref -- --ignored --nocapture
```

Committed fixtures (`tests/data/`): a PNM corpus, C++-encoded streams and Kakadu multi-layer / POC /
PPM / PPT streams, exercised by `tests/corpus.rs`; `tests/bitflip_fuzz.rs` is a hostile-input smoke fuzz.

The two CLI binaries — `ojph` (converter) and `ojph_bench` (SIMD bench) — are gated behind
the `cli` feature (which pulls in `image`):

```bash
cargo build --release --features cli --bin ojph
cargo build --release --features cli --bin ojph_bench
ojph                                   # help; `ojph <command>` shows that command's options
ojph e photo.png photo.j2c -Q 90       # encode (lossy, quality factor 90); `ojph d` decodes, `ojph v` compares
ojph d photo.j2c half.png -s 1         # decode at half resolution
```

## Parity with the C++ reference

Verification against OpenJPH / OpenHTJ2K / Kakadu is test-driven: `tests/conf_ref.rs` (73 ISO vectors
vs reference pixels or the OpenHTJ2K decoder output), `tests/ml_kakadu_validate.rs` and the other
`#[ignore]`d probes (Kakadu-encoded multi-layer / POC / PPM / PPT / RGN streams, bit-exact), plus the
committed fixtures in `tests/data`. Quick Rust vs C++ encode timing (needs a built `ojph_compress`
under `OPENJPH_REF_DIR/build`): `python scripts/bench_cpp_vs_rust.py`.

## SIMD benchmark

Per-tier caps run in **fresh worker processes** so CPU dispatch is initialized once per level.

```bash
cargo run --release --features cli --bin ojph_bench -- --compare-simd
cargo run --release --features cli --bin ojph_bench -- --compare-simd --assert-codestream
cargo run --release --features cli --bin ojph_bench -- --compare-simd --lossy -q 0.05
```

Single tier (example: force AVX2):

```powershell
$env:OJPH_CPU_MAX_LEVEL = "9"
cargo run --release --features cli --bin ojph_bench -- --worker
```

On a typical Zen 3 desktop (Ryzen 5900X), 3840×2160 RGB **lossless** median throughput is roughly:

| Tier    | Relative to generic | Notes                          |
|---------|---------------------|--------------------------------|
| generic | 1.00×               | scalar encode path             |
| avx2    | ~1.5× faster        | block encoder SIMD (see below) |
| avx512  | needs HW            | skipped if no AVX-512F+CD      |

All successful tiers produce **identical J2C** with `--assert-codestream`.

### AVX2 encode

OpenJPH C++ compiles `ojph_block_encoder_avx2.cpp` with `/arch:AVX2` or `-mavx2`.
This crate uses `#[target_feature(enable = "avx2")]` on hot helpers plus a direct
dispatch from `Codeblock::encode` into the AVX2 encoder (no fn-pointer per codeblock).

**2026-05 fix:** `proc_pixel` must use constant shift (`_mm256_srl_epi32`), not
`_mm256_srlv_epi32`. The variable-shift intrinsic was a port bug that made AVX2
encode slower than scalar despite correct codestream output.

### Environment variables

| Variable | Purpose |
|----------|---------|
| `OJPH_CPU_MAX_LEVEL` | Cap SIMD tier for bench workers (`0` = generic … `9` = AVX2, `11` = AVX-512). |
| `OJPH_CPU_FORCE_LEVEL` | Force a tier, clamped to the detected one (feature `unsafe-force-simd` lifts the clamp for bench / crash testing). |
| `OJPH_FORCE_ENCODE=generic` | Keep wavelet/tx SIMD but use scalar block encode (isolates encoder). |
| `OJPH_FORCE_TX=sse2` | Force SSE2 quantize/transform path. |
| `OJPH_FORCE_WAVELET_REV=sse2` | Force SSE2 reversible wavelet. |
| `OJPH_AVX2_LZCNT=emulated\|hw\|auto` | LZCNT in AVX2 `proc_pixel` (default: `emulated`, matches C++). |
| `OJPH_ENCODE_PROFILE=1` | Print per-codeblock encode timing breakdown to stderr (debug only; slows encode). |

### Encode profile breakdown

When `OJPH_ENCODE_PROFILE=1`, each codeblock logs:

`loads | proc_pixel | cal_tuple(gather) | cq_pipeline | mel | ms | vlc`

**Corrected 2026-09-22 (BUG1.md Round 9) — the line below was stale/wrong.** Re-measured
(3840x2160 RGB, 512x512 worker, `avx2` tier, averaged over 1470 codeblocks,
`OJPH_ENCODE_PROFILE=1 ./target/release/ojph_bench --worker --width 512 --height 512`):
**`ms` ~35-39%, `vlc` ~17-18%, `mel` ~6-9% (`ms`+`vlc`+`mel` ~60-70% combined)**, `loads` ~18%,
`proc_pixel` ~8%, `gather`/`cq_pipeline` ~5% each — `proc_pixel` does NOT dominate; `ms`/`vlc`/`mel`
(the HT entropy-coding bit-packing stages) do. Use this table to spot regressions; do not leave
`OJPH_ENCODE_PROFILE` enabled for production benches (`ojph_bench --compare-simd` clears it in
child workers).
