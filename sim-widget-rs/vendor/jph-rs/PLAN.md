# jph-rs Parity Plan

**Upstream:** [OpenJPH v0.26.3](https://github.com/aous72/OpenJPH/tree/v0.26.3/src/core)  
**Local C++ parity clone:** `OPENJPH_REF_DIR` (see [README.md](./README.md))

## Status

| Area | Verdict |
|------|---------|
| Core encode/decode (8-bit interleaved) | Bit-exact vs C++ |
| Lossy ICT | J2C bit-exact |
| 16-bit | J2C bit-exact (harness primary gate) |
| Multi-tile / decomp / block / progression order | J2C bit-exact |
| Cross-interop Rust ↔ C++ | OK |
| SIMD correctness | Identical J2C across tiers |
| Planar feed (library API) | Roundtrip tests in `tests/planar_roundtrip.rs` |
| AVX2 encode performance (4K lossless) | ~1.5× faster than generic (2026-05-22 fix) |
| Planar vs C++ (raw/YUV CLI) | Not in harness yet |
| CLI vs `ojph_compress` / `ojph_expand` | Partial |

## Verification

```bash
cargo test --release
cargo run --release --features cli --bin ojph_bench -- --compare-simd --assert-codestream
CONFDIR=<OpenHTJ2K conformance_data> cargo test --release --test conf_ref -- --ignored --nocapture   # 73/73
```

Harness notes: SIMD J2C gate in bench; port-target comments use GitHub blob URLs. (The former
`python bootstrap.py parity` harness no longer exists; see AGENTS.md "Build / verify".)

## Remaining work

1. **Planar C++ parity** — library roundtrip done; add harness via raw/YUV or `-planar` CLI.
2. **CLI flags** — `-profile`, `-precincts`, image offsets, PFM/YUV/raw (the CLI is now a thin driver over `mem`; see [ojph_compress.cpp](https://github.com/aous72/OpenJPH/blob/v0.26.3/src/apps/ojph_compress/ojph_compress.cpp)).
3. **Optional:** public `ParamQcd` wrappers aligned with `ojph_params.h`.

Tile-scoped QCD/QCC are applied (in-tile requantize); in-tile COD/COC/NLT are refused in strict mode (the C++ reference skips them silently).

## Done (acceptance)

- No committed `_ref/` paths; GitHub URLs for port targets
- `OPENJPH_REF_DIR` parity harness
- 16-bit J2C-primary harness, SIMD J2C assert in bench + harness
- Planar library roundtrip tests
- AVX2 encode performance regression fixed (`proc_pixel`: `srlv` → `srl_epi32`)
