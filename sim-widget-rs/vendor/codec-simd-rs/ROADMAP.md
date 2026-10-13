# ROADMAP.md — `codec-simd`

Full path from where we are today (v0.2.4) to **v1.0**, organized by
minor-version milestones.  Each milestone is small enough to land in 1-7
focused days; the whole thing is ~5-6 months of one-developer focused effort,
faster with parallel agents.

This is the **complete plan**.  It is not aspirational — every entry
below has a known caller in one of the existing pure-Rust codec ports
(`h264-rs`, `apv-rs`, `kvz-rs`, `ffmpeg-rs`, `vp9-rust`, `jph-rs`), or in
a planned consumer (AV1 native port, dav1d-rs, etc.).  Anything that
doesn't have an identified caller is **not** on the roadmap.

## Status legend

- ✅ shipped — primitive + 5 backends + cross-tier test + CHANGELOG entry
- 🚧 in progress — code being written
- 📋 planned — design known, callers identified
- 💭 deferred — wait for second consumer or upstream stabilization

---

## v0.1.x — Tier-1 universal primitives

Small building blocks that consumers compose into larger DSP kernels.
8-16 lanes per iteration, one or two intrinsics per lane.

| Version | Primitive | Status | Required by | Notes |
|---|---|---|---|---|
| 0.1.0 | `sat_pack_i16_to_u8` | ✅ | kvz-rs, vp9-rust, 8-bit decoders | 5 backends, AVX2 intra-lane gotcha handled |
| 0.1.1 | `sat_pack_i32_to_u16(max)` | ✅ | h264-rs, apv-rs, ffmpeg-rs h265, vp9-rust ≥10bpc | parametric `max = (1<<bd)-1`; SSE4.1 new tier |
| 0.1.2 | `add_clip_to_pixel_u16(pred, res, dst, max)` | ✅ | ffmpeg-rs `av-codec-h265::add_residual` (the IMG_9436 hot path) | fused i16 + u16 → u16 clipped; saves a buffer |
| 0.1.3 | `clamp_i32_to_i16(buf)` | ✅ | apv-rs `quant.rs:63` dequant inner loop | in-place clamp |
| 0.1.4 | `abs_diff_u8(a, b) → u8` | ✅ | every SAD/SATD building block | preamble for v0.2 |
| 0.1.5 | `mac_widen_i16_to_i32(a, b, acc)` | ✅ | every separable filter (MC), DCT inner | `vmlal_s16` / `pmaddwd` / mul-then-add fallback |
| 0.1.6 | `transpose_4x4_i16`, `transpose_8x8_i16` | ✅ | every transform family | classic `vtrn`/`unpcklwd` shuffle |
| 0.1.7 | `sat_add_i16x8`, `sat_sub_i16x8`, `abs_i16x8` | ✅ | deblock filter inner loops | trivial wrappers, build block for v0.6 |
| 0.1.8 | `mul_hi_i16x8` (rounding) | ✅ | MC filter coefficient multiply | `vqrdmulhq_s16` / `pmulhrsw` |
| 0.1.9 | `round_shift_narrow_i32x4_to_i16x8` | ✅ | DCT output stages | combined shift+round+pack |

**Exit criterion for v0.1.x:** every Tier-1 op the consumer ports
demonstrably need.  No further Tier-1 ops added without a 2nd consumer
asking for one.

## v0.2.x — Encoder kernels (SAD/SATD/SSE)

Tier-2: block-sized.  Motion estimation hot loops in encoders.

| Version | Kernel | Status | Caller |
|---|---|---|---|
| 0.2.0 | `sad_block_u8` (sizes 4×4 … 64×64) | ✅ | kvz-rs `inter_me::MeState::check_mv_cost`, `cost::sad_nxn` |
| 0.2.1 | `satd_block_4x4_u8` (Hadamard SAD 4×4) | ✅ | kvz-rs `cost::satd_*` |
| 0.2.2 | `satd_block_4x4_u8` NEON + wasm backends | ✅ | (backfill of v0.2.1) |
| 0.2.3 | `sse_block_u8` (rate-distortion cost) | ✅ | every R-D loop |
| 0.2.4 | `mc_avg_u8` (bipred rounding average) | ✅ | every bi-prediction (pulled forward from v0.3.6) |
| 0.2.5 | `variance_block_u8` → `(sum, sum_sq)` | 🚧 | HEVC adaptive quant (vbaq), VP9/AV1 partition cost |
| 0.2.6 | `sad_x4_block_u8` (4 candidates in parallel) | 📋 | hexagon / diamond search inner loop |

**Exit criterion for v0.2.x:** kvz-rs encoder bitstream-identical at all
forced tiers, with measurable SIMD speedup on motion estimation
(target: 3-4× AVX2 vs scalar on SAD8x8).

## v0.3.x — Motion compensation kernels (decoder, hottest)

~30-40% of decode time for any modern codec.  This is the jewel.

| Version | Kernel | Status | Caller |
|---|---|---|---|
| 0.3.0 | `mc_8tap_h_u8` (W×H sizes) | 📋 | VP9/AV1/HEVC luma |
| 0.3.1 | `mc_8tap_v_u8` | 📋 | same |
| 0.3.2 | `mc_8tap_2d_u8` (h-then-v with intermediate) | 📋 | same |
| 0.3.3 | `mc_4tap_h/v/2d_u8` | 📋 | HEVC chroma |
| 0.3.4 | `mc_6tap_h/v/2d_u8` | 📋 | H.264 luma 6-tap (different coeffs from HEVC) |
| 0.3.5 | `mc_bilinear_h/v_u8` | 📋 | H.264 chroma, AVIF |
| 0.3.6 | `mc_avg_u8` (P → B-frame averaging) | ✅ | shipped early in v0.2.4 |
| 0.3.7 | `mc_8tap_*_u16` (high-bit-depth siblings) | 📋 | 10-bit decode |

**Block-size generics:** all kernels are `<const W: usize, const H: usize>`
generic.  Inner-most body lives once; `seq_macro!` or `paste!` expands
to monomorphizations for every (W, H) pair the codec needs (~13 pairs for
VP9, ~17 for HEVC).

**Exit criterion for v0.3.x:** ffmpeg-rs h265 / vp9-rust MC achieves
≥75% of FFmpeg-asm performance on a representative frame.

## v0.4.x — Inverse transforms

~15% of decode time.  Each transform size is its own kernel; the
butterfly stages don't share much.

| Version | Kernel | Status | Caller |
|---|---|---|---|
| 0.4.0 | `idct_4x4_add_u8` | 📋 | every video codec since H.264 |
| 0.4.1 | `idct_8x8_add_u8` | 📋 | same |
| 0.4.2 | `idct_16x16_add_u8` | 📋 | HEVC/VP9/AV1 |
| 0.4.3 | `idct_32x32_add_u8` | 📋 | HEVC/VP9/AV1 — **3-4 days alone**, biggest single primitive |
| 0.4.4 | `iadst_4x4`, `iadst_8x8`, `iadst_16x16` | 📋 | VP9/AV1 asymmetric |
| 0.4.5 | `iwht_4x4_add_u8` | 📋 | H.264 lossless |
| 0.4.6 | `hadamard_4x4`, `hadamard_8x8` | 📋 | inside SATD; v0.2.1 consumer |
| 0.4.7 | `idct_*_add_u16` | 📋 | high-bit-depth siblings |

**Exit criterion for v0.4.x:** all decoder consumers can call into
codec-simd for every common transform size at both 8/10 bpc.

## v0.5.x — Intra prediction

~8% of decode time but huge variety (35 modes × block sizes for HEVC).

| Version | Kernel | Status | Caller |
|---|---|---|---|
| 0.5.0 | `intra_dc_predict_u8` (4/8/16/32 blocks) | 📋 | every codec |
| 0.5.1 | `intra_v_predict_u8`, `intra_h_predict_u8` | 📋 | every codec |
| 0.5.2 | `intra_planar_predict_u8` | 📋 | HEVC |
| 0.5.3 | `intra_tm_predict_u8` (true-motion) | 📋 | VP9 |
| 0.5.4 | `intra_angular_predict_u8` (33 dirs) | 📋 | HEVC |
| 0.5.5 | `intra_directional_predict_u8` (8 dirs) | 📋 | VP9 |
| 0.5.6 | high-bit-depth siblings of all above | 📋 | 10-bit codecs |

## v0.6.x — Loop filters

~10% of decode time.  HEVC has the longest deblock kernels.

| Version | Kernel | Status | Caller |
|---|---|---|---|
| 0.6.0 | `deblock_filter_4tap_u8` (AVC normal) | 📋 | h264-rs |
| 0.6.1 | `deblock_filter_normal_u8` (HEVC normal-strength) | 📋 | ffmpeg-rs h265, kvz-rs |
| 0.6.2 | `deblock_filter_strong_u8` (HEVC strong) | 📋 | same |
| 0.6.3 | `sao_band_offset_u8`, `sao_edge_offset_u8` | 📋 | HEVC SAO |
| 0.6.4 | high-bit-depth siblings | 📋 | 10-bit codecs |

## v0.7.x — Colour conversions

Runs on every output pixel of every codec.

| Version | Kernel | Status | Caller |
|---|---|---|---|
| 0.7.0 | `yuv420_to_rgb_8bit` | 📋 | every video output path |
| 0.7.1 | `yuv422 / yuv444 to_rgb_8bit` | 📋 | professional codecs |
| 0.7.2 | `yuv420 / 422 / 444 _to_rgb_10bit` | 📋 | HDR, professional |
| 0.7.3 | `colour_rct_forward / backward_i32` | 📋 | extract from jph-rs (jph-rs `ojph_colour_*`); HEVC RExt, VVC lossless, JPEG XS, AVIF |

## v0.8.x — Wavelet (DWT)

For HTJ2K and any future wavelet codec.  Extracted from jph-rs once a
second wavelet consumer appears.

| Version | Kernel | Status | Caller |
|---|---|---|---|
| 0.8.0 | `dwt_lift_53_horz_i32`, `dwt_lift_53_vert_i32` | 💭 | jph-rs (already implements; extract when 2nd consumer arrives) |
| 0.8.1 | `dwt_lift_97_horz_f32`, `dwt_lift_97_vert_f32` | 💭 | same |

## v0.9.x — Quantization

Mostly trivial SIMD; needed for completeness.

| Version | Kernel | Status | Caller |
|---|---|---|---|
| 0.9.0 | `quant_block_i16` | 📋 | every encoder |
| 0.9.1 | `dequant_block_i16` / `_i32` | 📋 | every decoder |
| 0.9.2 | `quant_block_with_sign_hiding` | 📋 | HEVC |

## v1.0 — Stabilization

- All public API marked `#[non_exhaustive]` where appropriate.
- Architecture coverage:
  - x86: SSE2 / SSSE3 / SSE4.1 / AVX / AVX2 / AVX2+FMA / AVX-512 (F+BW+VL)
  - ARM: NEON / SVE / SVE2
  - wasm: simd128 (relaxed-simd opt-in feature)
  - RISC-V: V extension (when toolchain support stabilizes)
- All six consumer codecs integrated with `cargo build --release` clean.
- Benchmark suite (criterion) vs FFmpeg `--enable-asm` baseline.
- Open-sourced (move from "private commercial-license-only development"
  phase to dual GPL+commercial public).
- Docs: full mdBook published at https://codec-simd.dev/ (or
  github.io/codec-simd).
- `cargo audit` clean.  MSRV pinned and CI'd.

---

## Operating procedure per version

Same dispatch discipline for every primitive added.  No shortcuts.

1. **Identify caller.**  Pick a Tier-1 primitive from the table that has
   ≥1 identified Rust-codec caller (the table tracks this; don't add
   primitives without a caller).
2. **Write scalar reference first.**  In `src/prims_scalar.rs`.  Keep
   it obvious; do not optimise.
3. **Write per-backend impls.**  In `src/prims_{sse2,sse41,avx2,neon,wasm}.rs`.
   Each backend file holds **all** primitives at that ISA.  Same
   convention as jph-rs.  Some primitives skip lower tiers (e.g.
   `sat_pack_i32_to_u16` skips SSE2 because `packusdw` is SSE4.1+); in
   that case, scalar fallback is automatic via tier overrides in
   `Dispatch::detect()`.
4. **Wire dispatch.**  `Dispatch` gets one new `pub fn` pointer field
   (append-only; never reorder).  `Dispatch::scalar()` returns the
   scalar variant; `Dispatch::detect()` overwrites with higher tiers.
5. **Public safe API.**  Add a `pub fn` in `src/lib.rs` that asserts
   slice lengths then dispatches.  No `unsafe` in the public signature.
6. **Cross-tier test.**  In `tests/cross_tier.rs`.  Three tests per
   primitive:
   - scalar vs naive reference
   - detected dispatch vs scalar
   - boundary-value test (`i32::MIN/MAX`, `max-1`, `max`, `max+1`)
7. **CHANGELOG entry.**  Under the new `[x.y.z]` heading.  Describe
   WHAT and WHY (which caller asked for this).
8. **Bump `Cargo.toml` version.**  Patch bump for additions; only
   minor-bump when stabilizing a milestone.
9. **`cargo test --release`** — green.
10. **Tier sweep:**
    ```bash
    for L in 0 3 5 6 8 9 11 ; do
        CODEC_SIMD_FORCE_LEVEL=$L cargo test --release --test cross_tier
    done
    ```
    All available tiers must pass.  Tiers the host doesn't have are
    skipped automatically.
11. **`git commit`** with conventional prefix (`feat:` for new primitive,
    `fix:` for bug, `refactor:` for restructure, `chore:` for tooling).
12. **`git tag v0.x.y && git push origin v0.x.y`** so consumers can pin.
13. **Update consumer TODO_SIMD.md.**  Whichever codec asked for this
    primitive — open an issue / append a "ready" line saying "v0.x.y
    unblocks this".

## Anti-roadmap (deliberately NOT doing)

- **Floating-point DSP.**  `f32`/`f64` kernels have different bit-exactness
  semantics (rounding-mode, NaN handling).  Will be a separate family
  with its own contract.  Not before v0.8+.

  *Re-examined 2026-08-06 for a real caller (playa's CPU colour path).  Still
  no, and now for measured reasons rather than principle:*
  - **FMA halves the win before it starts.**  The ladder already detects
    `X86_AVX2_FMA`, but Rust never auto-contracts, so `_mm256_fmadd_ps`
    diverges from the scalar reference.  The contract would have to be "no
    FMA, ever" — and the mirror fix (`f32::mul_add` everywhere) breaks wasm,
    whose simd128 has no FMA and whose relaxed-simd is spec-nondeterministic.
  - **Reshaping the caller's loop beat hand-SIMD.**  That loop was 177 ms per
    4K frame.  Splitting it into passes over an L1-resident tile — no new
    dependency, no `unsafe` — took it to 117 ms, and it is still
    single-threaded.  Autovectorising a loop with the right SHAPE beats
    hand-vectorising one without it.
  - **Two of the three proposed kernels had no caller.**  A premultiplied
    `blend_rgba_f32` does not fit playa (straight alpha, mode dispatched
    inside the per-channel body, fused with a bilinear gather).  Transfer
    curves are not bit-exactly vectorisable at all, and were solved in
    `vfx-transfer::fast` — in the colour crate, where they belong.
  - **The consumers already chose other tools.**  `oiio-rs` uses `wide::f32x4`
    in `vfx-lut` and rayon in `vfx-ocio`; playa does not depend on this crate
    at all.  A third mechanism in one dependency graph is the duplicate
    mechanism those repos' own rules forbid.

  The precondition for revisiting is unchanged and now explicit: a float
  caller that this crate's plane-batch shape actually fits, and a written
  bit-exactness contract that answers FMA, ±0, NaN and denormals first.
- **Audio codecs (AAC, MP3, Vorbis, Opus).**  Need MDCT, polyphase
  filters, completely different shape.  Out of scope until video is done.
- **Encoder rate-distortion / RDO loops.**  Sequential by nature; SIMD
  doesn't apply.  R-D *cost computation* (SAD/SATD/SSE) is in scope;
  the search algorithm itself is not.
- **GPU offload paths.**  WGPU bindings for some kernels might be useful
  long-term but they're a different crate (probably `codec-gpu`).
- **Architecture-specific kernels that other arches can't match
  bit-exactly.**  Every primitive must be reproducible across all
  supported tiers.  No "AVX-512-only fast-rounding tricks" that
  diverge from scalar.
- **Inline assembly.**  Pure-Rust intrinsics via `core::arch::*`.  If
  a primitive needs an instruction Rust hasn't stabilized, file an
  issue against `rust-lang/stdarch` and use the scalar fallback in the
  meantime.
- **`pulp` / `std::simd` dependency.**  Both are good crates for other
  use cases.  codec-simd is hand-tuned per-ISA on purpose.

## Calendar estimate (one focused developer)

| Phase | Duration | Notes |
|---|---|---|
| v0.1.x complete (8 more primitives) | 2-3 weeks | Tier-1 building blocks |
| v0.2.x (SAD/SATD) | 2 weeks | unblocks kvz-rs encoder |
| v0.3.x (MC, 7 versions) | 4 weeks | the highest-impact decoder work |
| v0.4.x (IDCT, 8 versions) | 6 weeks | 32×32 IDCT alone is 3-4 days |
| v0.5.x (intra prediction) | 3 weeks | high variety, low per-kernel cost |
| v0.6.x (loop filter) | 3 weeks | HEVC strong filter is the harder one |
| v0.7.x (colour) | 2 weeks | high pixel throughput, simple math |
| v0.8.x (DWT) | 1 week | extracted from jph-rs |
| v0.9.x (quant) | 1 week | trivial SIMD |
| v1.0 stabilization | 2-3 weeks | tag stable, docs, OSS |

**Total: ~5-6 months of focused single-developer effort.**

Realistic parallelization with agents: 2× speedup is achievable for
independent kernels (Tier-1 is parallel-friendly), 1.2× for cross-cutting
work (changes to dispatch / arch / public API).  So with disciplined
agent orchestration: **~3-4 months calendar time** for the same scope.
