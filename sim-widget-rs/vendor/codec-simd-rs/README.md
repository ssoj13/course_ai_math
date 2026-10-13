# codec-simd

Cross-architecture SIMD primitives and DSP kernels for software video codecs.

Tier-dispatched at runtime across:
- **x86_64**: scalar / SSE2 / SSSE3 / SSE4.1 / AVX / AVX2 / AVX-512
- **AArch64**: scalar / NEON  (SVE/SVE2 planned)
- **wasm32**: scalar / simd128

The crate exists to **stop every codec port from re-implementing the same SIMD
helpers from scratch**.  Consumers (e.g. `vp9-rust`, `h264-rs`, `apv-rs`,
`kvz-rs`, `jph-rs`) call a stable safe API; `codec-simd` picks the best
implementation for the CPU it's running on.

## Status

**v0.1 — skeleton.** One worked primitive (`sat_pack_i16_to_u8`) across all
five backends, demonstrating the full dispatch pattern.  Future primitives
and Tier-2 kernels (MC convolutions, DCT/ADST butterflies, loop filter, DWT
lifting) are added by copying that pattern.

See [`CAVEMAN.md`](CAVEMAN.md) for how to plug this crate into a codec or
how to add a new primitive.

## Quick example

```rust
use codec_simd::sat_pack_i16_to_u8;

let src: Vec<i16> = (-256..256).collect();      // some values outside [0,255]
let mut dst = vec![0u8; src.len()];
sat_pack_i16_to_u8(&src, &mut dst);             // saturating clip to u8 lane-by-lane
assert_eq!(dst[0], 0);
assert_eq!(dst[511], 255);
```

The call above runs scalar code on a generic CPU, NEON on Apple Silicon,
AVX2 on a modern Intel/AMD desktop, and wasm `i8x16_narrow_i16x8` in a
browser.  No `unsafe` at the call site.

## Environment overrides (testing / benchmarking)

| Variable | Purpose |
|---|---|
| `CODEC_SIMD_FORCE_LEVEL=<int>` | Pin dispatch to a specific tier regardless of CPU (must be supported by hardware). Useful for bit-exact conformance runs across all available implementations. |
| `CODEC_SIMD_MAX_LEVEL=<int>` | Cap dispatch at this tier. `min(detected, cap)`. Useful when bisecting which tier introduced a regression. |

Tier integers follow the x86 ladder by convention (Generic=0, SSE2=3, SSSE3=5,
SSE4.1=6, SSE4.2=7, AVX=8, AVX2=9, AVX-512=11).  See [`src/arch.rs`](src/arch.rs)
for AArch64 / wasm values.

## License

**Dual: GPL-3.0-or-later OR commercial.**

- Open-source / GPL-compatible projects use it for free under the GPL — see
  [`LICENSE`](LICENSE).
- Closed-source / proprietary products buy a commercial license to waive the
  GPL copyleft — see [`LICENSE-COMMERCIAL.md`](LICENSE-COMMERCIAL.md) for
  terms and how to acquire one.

If you are not sure which applies to you, the safe default is "commercial" —
contact the maintainer.
