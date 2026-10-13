# CLAUDE.md — `src/transform`

> Working notebook: keep it current. Parent notes: `../../AGENTS.md`.

## What this is
1:1 OpenJPH ports of the wavelet (`ojph_transform*.rs`: 5/3 reversible + 9/7 irreversible lifting, ATK/DFS-driven general lifting)
and colour transforms (`ojph_colour*.rs`: RCT/ICT, NLT, signed/unsigned conversion, `rev_convert`/`irv_convert`) with one file per SIMD tier
(`_sse`, `_sse2`, `_avx`, `_avx2`, `_avx512`, `_wasm`; `_local` = shared scalar). Dispatch happens once (`Once`) from
`ojph_arch::get_cpu_ext_level()` (`OJPH_CPU_MAX_LEVEL` cap; `OJPH_CPU_FORCE_LEVEL` is clamped to the detected tier).

## Used by
`codestream/ojph_resolution.rs` (inverse/forward DWT per resolution), `codestream/ojph_tile.rs` (colour transforms, level shift).

## Traps
- Lines are `LineBuf`s of i32 / i64 (reversible, precision > 32) or **f32** (irreversible, always).
- Reduced-resolution decode returns the LL band as-is: reversible 5/3 low-pass can overshoot the sample range (consumers clamp).
- AVX-512 transform has never run on hardware (BUG1.md:1041); keep its dispatch gate equal to the fuzz-test gate (F+CD+BW+VL).

## TODO
- [ ] wasm dispatch lacks `cfg(target_feature = "simd128")`; dead non-wasm stubs.
