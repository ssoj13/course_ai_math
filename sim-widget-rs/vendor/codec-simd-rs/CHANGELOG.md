# Changelog

All notable changes to `codec-simd` are listed here.  Format loosely follows
[Keep a Changelog](https://keepachangelog.com/).  Adding fields to the
`Dispatch` struct is non-breaking (append-only); removing or reordering
fields is a major bump.


## [Unreleased]

### Added

- **`fdct_faan`: FFmpeg `ff_faandct` (`-dct faan`), the float AAN forward DCT on an `int16_t` block.** `float` butterflies with the `double` constants of `faandct.c` (every operand they touch is computed in double and rounded back, as C does), `postscale` built from double products, `lrintf` (ties to even) into `int16_t`. 80-block KAT (`tests/fdct_kat.rs`) against the unmodified `faandct.c` (clang, `tests/c_oracle`).
- **`fdct8`: 8x8 integer forward DCT kernels, byte-exact vs the C references.** One generic
  `fdct_islow::<CONST_BITS, PASS1_BITS, OUT_SHIFT, E>` (IJG LL&M, `E` = `i16`/`i32` work array) with the named
  instances `fdct_islow_ff8` (`ff_jpeg_fdct_islow_8`), `fdct_islow_ff10` (`_10`, also the ProRes FDCT),
  `fdct_islow_tj8` (libjpeg-turbo `jpeg_fdct_islow`, `DCTELEM = short`) and `fdct_islow_tj12` (12-bit, `JLONG`),
  plus `fdct_ifast` (`ff_fdct_ifast`, the AAN kernel of `-dct fastint`). KAT: `tests/fdct_kat.rs` replays 200 C
  outputs (`tests/data/fdct_kat.txt`) produced by `tests/c_oracle/gen_fdct_kat.ps1` from the unmodified FFmpeg 9.0.1
  and libjpeg-turbo sources. No separate multiply form: FFmpeg's `(int)(v * (unsigned)k)` and libjpeg's `v * k`
  are the same wrapping 32-bit product.

### Decided (no code change)

- **The float-kernel family stays on the anti-roadmap**, re-examined against a real caller
  (playa's CPU colour path) rather than in the abstract. Three concrete findings, recorded in
  `ROADMAP.md`: Rust never auto-contracts, so any `_mm256_fmadd_ps` diverges from the scalar
  reference and the contract would have to be "no FMA, ever" (halving the win, and the mirror fix
  breaks wasm); reshaping the caller's loop into passes over an L1-resident tile took it from
  177 ms to 117 ms per 4K frame with no dependency and no `unsafe`, which is more than the
  FMA-banned SIMD ceiling; and two of the three proposed kernels had no caller that fit
  (`blend_rgba_f32` assumes premultiplied alpha, transfer curves are not bit-exactly vectorisable
  and were solved in `vfx-transfer::fast`). The consumers had also already picked other tools —
  `wide::f32x4` in `vfx-lut`, rayon in `vfx-ocio` — and playa does not depend on this crate at all.
  The precondition for revisiting is now written down: a caller this crate's plane-batch shape
  fits, plus a bit-exactness contract that answers FMA, ±0, NaN and denormals up front.


## [0.3.35] — 2026-09-24

### Added

- **`prefix_sum_bias_u8_inplace`** — in-place biased running prefix sum over a
  `u8` buffer (`buf[i] = buf[i-1] + buf[i] - 128`, wrapping, `buf[0]`
  unchanged). The DECODE side of OpenEXR's ZIP/RLE first-difference byte
  predictor (`reconstruct`, `OpenEXRCore/internal_zip.c:49-157`). SSE4.1-tier
  dispatch (SSSE3 instructions only, same tier choice as `mul_hi_round_i16`)
  via a classic shift-and-add prefix-sum tree carrying the running delta
  between 16-byte blocks with `pshufb`; NEON backend using `vextq_u8` in
  place of `_mm_slli_si128`. Scalar reference is a straight port of C's own
  scalar fallback. Requested by exr-rs (the OpenEXR port): its own
  hand-rolled `unsafe` SSE4.1 port of this exact algorithm was moved here per
  the "unsafe stays inside codec-simd" project rule, after being measured at
  taking 52.6% of ZIP's total decode time while fully scalar (more than the
  zlib inflate itself).
- **`interleave_halves_u8`** — re-interleave two halves into one buffer
  (`out[2k] = t1[k]`, `out[2k+1] = t2[k]`). The DECODE side of the SAME
  predictor's byte-reorder step (`interleave`,
  `OpenEXRCore/internal_zip.c:161-244`), paired with
  `prefix_sum_bias_u8_inplace`. SSE2-tier dispatch (`unpacklo`/`unpackhi_epi8`,
  32 bytes/iteration); NEON backend (`vzip1q_u8`/`vzip2q_u8`). The matching
  ENCODE-side operation (deinterleave + forward difference) has no SIMD
  counterpart in OpenEXR itself, so it is intentionally NOT added here —
  it measured at only ~3-4% of total codec time (no gap to close).
- Cross-tier tests in `tests/cross_tier.rs`: scalar-vs-naive-reference,
  detected-dispatch-vs-scalar (0/1/2/15/16/17/... boundary lengths incl. 0),
  and a round trip through the safe public API together.


## [0.3.34] — 2026-07-15

### Added

- **`sse_block_u16`** — the u16 sibling of `sse_block_u8` (sum of squared errors
  over a `w×h` block, `u64`), completing the u16 encoder-cost trio
  (SAD/SATD/SSE) begun in 0.3.33. `i32` diff, `u64` accumulator; byte-identical
  to `sse_block_u8` on u8-range inputs (equivalence-tested). Scalar on every tier
  (append-only `Dispatch` field; `detect()` untouched — SIMD backfill deferred).
  Consumed by the kvz-rs bit-depth-generic RD-cost path (`calc_ssd`).


## [0.3.33] — 2026-07-15

### Added

- **HEVC encoder-cost primitives on the `u16` plane** (Tier-D scaffold) — the
  10/12/16-bit siblings of the u8 SAD/SATD cost kernels: `sad_block_u16`,
  `satd_block_4x4_u16`, `satd_block_8x8_u16`. Element (u16) strides; `i32`
  intermediates (a u16 diff and the Hadamard ×16/×64 gain overflow i16), so
  byte-identical to the u8 kernels on u8-range inputs (equivalence-tested).
  Scalar on every tier for now (`detect()` untouched): a hand-tuned SIMD
  backfill is deferred rather than shipped untested, per the crate's documented
  scalar-fallthrough policy. Three new append-only `Dispatch` fields
  (non-breaking). Motivates the kvz-rs single-binary 8/10-bit encoder, whose
  bit-depth-generic cost path picks the u8 or u16 kernel at runtime.


## [0.3.32] — 2026-07-11

### Added

- **HEVC in-loop deblocking-filter primitives on the `u16` plane** — the three
  DSP inner kernels (`hevc_deblock_luma_strong_u16` / `hevc_deblock_luma_weak_u16`
  / `hevc_deblock_chroma_weak_u16`), a byte-exact port of ffmpeg-rs
  `av-codec-h265/filter/deblock.rs` (`luma_strong` / `luma_weak` / `chroma_weak`
  → the C `h2656_deblock_template.c`). These are the filter loops only; the
  caller owns the boundary-strength grid, the `d0+d3 < beta` decision, the
  strong-vs-weak selection, and the derivation of `tc`/`nd_p`/`nd_q`/`size`.
  - `hevc_deblock_luma_strong_u16(plane, base, xstride, ystride, tc, tc2, tc3,
    max)` — the 4-tap clipped strong filter over 4 lines; 6 outputs per line
    `x + clip(tap - x, -tcN, tcN)`. Delta-clip only (NO `clip_pixel`): the result
    is provably in `[0, max]`, so the `u16` store needs no clamp (`max` is
    accepted for signature symmetry, unused).
  - `hevc_deblock_luma_weak_u16(plane, base, xstride, ystride, tc, nd_p, nd_q,
    max)` — the weak filter over 4 lines; the **per-line** gate
    `|delta0| < 10*tc` (a failing line keeps its input bytes) + `p0`/`q0`
    (`clip_pixel`) + the optional `p1`/`q1` side filters (uniform `nd_p`/`nd_q > 1`).
  - `hevc_deblock_chroma_weak_u16(plane, base, xstride, ystride, size, tc, max)`
    — the single-tap chroma delta `clip(((q0-p0)*4 + p1 - q1 + 4) >> 3, -tc, tc)`
    over `size` lines, `clip_pixel`-clamped. Branchless.
  - **Plane + stride ownership, ONE kernel per orientation.** Each primitive
    takes the plane slice, the segment origin `base`, and the two orientation
    strides in samples (vertical edge: `xstride=1`, `ystride=stride`; horizontal:
    `xstride=stride`, `ystride=1`), exactly like the scalar reference. The SAME
    kernel serves both edges — the vertical/horizontal difference is folded into
    the SIMD backends' internal gather/scatter (the transpose).
  - **i32 accumulation (hard invariant).** Every tap sum accumulates in **i32**
    lanes (one lane per line): the strong intermediate `2*p3 + 3*p2 + p1 + p0 +
    q0 + 4` reaches ~32764 at 12-bit (within 3 of `i16::MAX`), so a packed-i16
    MAC would overflow. The `u16` narrow is the only width reduction, at the store.
  - **Scalar** reference + **SSE2** (i32×4, one lane per line; deblock
    coefficients formed with shifts+adds, signed `min/max` emulated with
    `cmpgt`+select) + **AVX2** (same i32×4 with native `min/max_epi32`). The
    deblock kernel is 4-lane by nature (4 luma lines), so AVX2's 256-bit lanes
    add no parallelism here — the win over SSE2 is only native min/max. The weak
    filter's per-line gate is a per-lane mask; failing lanes are blended back to
    their original bytes (so an unconditional scatter is exact); `p1`/`q1` stores
    are skipped entirely unless `nd_p`/`nd_q > 1`. All reads are snapshotted
    before any store (no in-place read-after-write hazard). NEON/wasm
    scalar-fallthrough (the x86 SIMD modules are `#[cfg]`-gated).
  - **Bounded loads (G3).** The safe wrappers assert the exact access window
    (`base - 4*xstride .. base + 3*ystride + 3*xstride` for strong, etc.) is
    in-bounds and both strides `>= 1`, so no gather/scatter can over-read the
    caller-sized plane.
  - Independent naive golden (reimplements the deblock.rs kernel math, NOT the
    primitive calls): both edge orientations (swapped `xstride`/`ystride`), bd
    8/10/12, tc/nd extremes, the `|delta0| < 10*tc` gate boundary, the
    gate-pass∧`nd_p==1` / gate-fail discriminating cases, a padded plane with
    pad-untouched assert, a strong-output-in-range assert, and a size=4
    Miri-ready bounds case. Byte-exact at forced tiers 0/3/6/9.

## [0.3.31] — 2026-07-10

### Added

- **HEVC intra-prediction primitives on the `u16` plane — planar + angular
  FILL kernels** (`hevc_intra_planar_u16` / `hevc_intra_angular_u16`), a
  byte-exact port of ffmpeg-rs `av-codec-h265/pred.rs` (`pred_planar` ~201,
  `pred_angular` ~264, spec 8.4.4.2.4 / 8.4.4.2.6). These are the DSP fill
  kernels only:
  - `hevc_intra_planar_u16(dst, dst_stride, top, left, log2_size)` — per pixel
    `((size-1-x)·left[y] + (x+1)·top[size] + (size-1-y)·top[x] + (y+1)·left[size]
    + size) >> (log2_size+1)`, `size = 1<<log2_size` (2..=5).
  - `hevc_intra_angular_u16(dst, dst_stride, ref_buf, bias, size, angle,
    vertical)` — the 2-tap FIR `((32-fact)·ref[k+1] + fact·ref[k+2] + 16) >> 5`
    with per-projection `idx = ((r+1)·angle)>>5`, `fact = ((r+1)·angle)&31`
    computed internally. The **caller** builds `ref_buf` (the C `ref` array incl.
    the negative-angle `INV_ANGLE` back-projection) and passes `bias`
    (`ref_buf[k+bias] == C ref[k]`, so negative `k` stay in-bounds — the FULL
    slice + `bias` is passed, never `&ref_buf[bias..]`). `vertical == mode>=18`.
    The horizontal family (`mode<18`) is computed with the SAME contiguous-row
    kernel into an internal `size×size` scratch, then transpose-stored (a pure
    data move — byte-exact by construction, no per-column strided scatter). The
    mode-26/10 boundary filter + `clip_pixel` remain the caller's, post-call.
  - **i32 accumulation (hard invariant).** Both the planar weighted sum and the
    angular 2-tap MAC accumulate in **i32** lanes — the numerators overflow i16
    at 10/12-bit (planar ~262112; angular 12-bit 31·4095 = 126945). The `u16`
    narrow is the only width reduction, at the store; outputs are provably in
    `[0, 2^bd)` so no clip is needed there.
  - **Scalar** reference + **SSE2** (i16-widening `mullo`/`mulhi`/`unpack` MAC,
    `packs_epi32` narrow) + **AVX2** (native `mullo_epi32`, `cvtepu16_epi32`,
    `packus_epi32` narrow). Both SIMD backends process the inner index in 8-lane
    chunks (`col+8<=size`) + a scalar tail, so a fixed-width load never
    over-reads the caller-sized buffers (`size=4` runs entirely scalar — G3
    bounded-load guard). NEON/wasm scalar-fallthrough (the x86 SIMD modules are
    `#[cfg]`-gated). Two new append-only `Dispatch` fields.
  - Independent naive golden (reimplements the pred.rs planar `POS` + the
    angular fact==0/!=0 fill) across sizes 4/8/16/32 × bd 8/10/12 × all angular
    modes 2..=34 × pseudo-random / saturated / alternating-0-max inputs;
    `dst_stride = size+8` with the pad columns asserted untouched (transpose /
    stride bug guard); synthetic `fact ∈ {1,31}` + negative-`idx` extremes; a
    `size=4` bounded-load test. Bit-exact green at forced tiers 0/3/6/9.

## [0.3.30] — 2026-07-10

### Added

- **HEVC SAO (Sample Adaptive Offset) band + edge filter primitives on the
  `u16` plane** — byte-exact port of ffmpeg-rs `av-codec-h265/filter/sao.rs`
  (`sao_band` / `sao_edge`), itself a port of
  `_ref/FFmpeg/libavcodec/h26x/h2656_sao_template.c`. 2 new append-only
  `Dispatch` fields. Strides are in `u16` elements; `max = (1 << bit_depth) - 1`.
  - `sao_band_u16(dst, dst_stride, src, src_stride, w, h, offset_table, band_shift, max)`
    — per pixel `dst = clip(src + offset_table[(src >> band_shift) & 31], 0, max)`.
    `offset_table` is the caller-built 32-entry band LUT (`band_shift =
    bit_depth - 5`; ≤4 non-zero window entries); reads only its own pixel, so
    `src == dst` in-place is safe.
  - `sao_edge_u16(dst, dst_stride, src, src_stride, w, h, eo_class, offsets, max)`
    — per pixel `category = 2 + CMP(p,n0) + CMP(p,n1)` over the two `eo_class`
    neighbours (`CMP(a,b) = (a>b) - (a<b)`), then `dst = clip(src +
    offsets[edge_idx[category]], 0, max)` with `edge_idx = [1,2,0,3,4]`. A pixel
    whose neighbour is outside the `w×h` region keeps its value (the picture-
    boundary `sao_edge_restore_0` rule); `src` must not alias `dst` (the kernel
    reads neighbours — snapshot semantics, matching the C).
  - **Band is scalar on every tier** (the 32-entry per-pixel LUT gather is not
    beaten by a SIMD scalar-gather; kept correct + dispatched, per CAVEMAN's
    scalar allowance). **Edge gets SSE2 + AVX2** `i16`-lane backends: samples
    are `≤ 12-bit` (`≤ 4095`) so the whole classify+clip runs in `i16` lanes
    (8 / 16 per step) — signed `cmpgt_epi16` for `CMP`, `cmpeq`+select for the
    category→offset LUT, `add`/`max_epi16(0)`/`min_epi16(max)` for the clip; the
    interior rectangle is SIMD, boundary rows/columns + the column tail scalar.
    NEON / wasm fall through to scalar. Byte-exact across all tiers (levels
    0/3/5/6/8/9) vs an independent naive golden + saturation + picture-boundary
    tests (`hevc_sao_*`).

### Changed

- **HEVC inverse-transform API wrappers** now `debug_assert!` that
  `bit_depth ∈ 8..=12` (`hevc_idct4/8/16/32_i16_inplace`, `hevc_dst4_i16_inplace`,
  `hevc_idct_dc_i16_inplace`), mirroring the ffmpeg-rs `idct.rs` contract; the
  transform cross-tier tests now also sweep `bit_depth = 9` (`HEVC_BDS =
  [8,9,10,12]`).


## [0.3.29] — 2026-07-10

### Added

- **HEVC inverse-transform family — sizes 8 / 16 / 32 (Phase-4 Wave-B)** — the
  continuation of the size-4 family (v0.3.28), a byte-exact port of the
  ffmpeg-rs `av-codec-h265/transform/idct.rs` `idct_nxn` 8/16/32 paths
  (`tr_8_core` / `tr_16_core` / `tr_32_core` + the col/row IDCT passes). 3 new
  append-only `Dispatch` fields; all in-place on the `i16` `size×size` block.
  - `hevc_idct8_i16_inplace(coeffs, col_limit, bit_depth)` /
    `hevc_idct16_i16_inplace(...)` / `hevc_idct32_i16_inplace(...)` — two passes
    (columns `>> 7`, rows `>> (20 - bit_depth)`) of the DCT partial (even/odd)
    butterfly with per-element `clip_int16` SCALE. Each `tr_N_core` = the even
    part `tr_{N/2}` on the even-index inputs + the odd part
    `o[i] = Σ_{j odd, j<end} TRANSFORM[N-row][i]·src[j]`, combined `e[i]±o[i]`;
    the 32×32 DCT-II basis matrix is ported verbatim from `dsp.c`.
  - **`col_limit` odd-coefficient pruning is fully replicated (option (a) — the
    faithful standalone primitive, byte-exact for ANY input, not just the
    zeros-beyond-limit HEVC contract).** The scalar reference prunes inline
    (`limit2 = min(col_limit+4, H)` shrinking by 4 every 4th column on the
    column pass, `limit = min(col_limit, H)` on the row pass, `tr_32`'s even
    part at `end/2`). The SIMD backends achieve the identical result by
    **pre-zeroing** the pruned source coefficients (each source routes to
    exactly one leaf of the even/odd recursion, so zeroing == skipping) then
    running the FULL uniform butterfly across columns.
  - **scalar** port + **SSE2** (4-column `i32` butterfly, `mullo/mulhi` widen of
    the i16 sources) + **AVX2** (8-column, native `cvtepi16_epi32` /
    `mullo_epi32` / `min_max_epi32`); NEON / wasm fall through to scalar.
    **SIMD strategy: vectorize the partial butterfly ACROSS columns**; the row
    (2nd) pass is the same column butterfly on the TRANSPOSE of the pass-1
    result (separable-transform identity). Products accumulate in `i32` (a
    `TR_32` sum ≤ 32·90·32767 ≈ 94M ≪ `i32::MAX`, matching the reference `int`).
  - `tests/cross_tier.rs`: an independent naive golden reimplementing the EXACT
    `tr_8/16/32_core` + `idct_nxn` col/row passes (WITH pruning) over its own
    copy of the basis matrix, across bd 8/10/12, random full-range i16 blocks,
    many `col_limit` (fully populated AND pruned), saturation extremes, and the
    ffmpeg-rs `transform/tests.rs` KAT vectors (`idct_8x8_known_pattern`,
    `idct_16x16_full_butterfly`, `idct_32x32_full_butterfly`). Byte-exact green
    at forced tiers 0/3/5/6/8/9; `aarch64` + `wasm32` cross-compile clean.


## [0.3.28] — 2026-07-10

### Added

- **HEVC inverse-transform family — size 4 (Phase-4 Wave-A)** — the first of the
  HEVC IDCT/DST primitives, byte-exact port of ffmpeg-rs
  `av-codec-h265/transform/idct.rs` (itself a port of `dsp_template.c`). All
  three operate **in-place on an `i16` coefficient block** (the add-to-plane is
  the separate, already-SIMD `add_clip_to_pixel_*` step). 3 new append-only
  `Dispatch` fields.
  - `hevc_idct4_i16_inplace(coeffs, bit_depth)` — the `idct_nxn` size-4 path:
    two passes (columns `>> 7`, rows `>> (20 - bit_depth)`) of the DCT-4 even/odd
    butterfly (constants 64/83/36) with per-element `clip_int16` SCALE. TR_4 has
    no odd-loop pruning, so no `col_limit` is threaded.
  - `hevc_dst4_i16_inplace(coeffs, bit_depth)` — `transform_4x4_luma` (DST-VII,
    constants 29/55/74), same two-pass SCALE model.
  - `hevc_idct_dc_i16_inplace(coeffs, log2_size, bit_depth)` — the DC-only fast
    path `idct_NxN_dc`: fills the whole `size×size` block (sizes 4/8/16/32) with
    `clip_int16((((coeffs[0]+1)>>1) + add) >> (14 - bit_depth))`.
  - **scalar** reference + **SSE2** + **AVX2**; NEON / wasm fall through to
    scalar. **SIMD strategy: vectorize ACROSS the 4 columns** — all 4 columns
    run the identical 4-point butterfly, so one register (4 × i32 lanes) does the
    whole butterfly; the row (2nd) pass is the column pass applied to the
    TRANSPOSE of the 1st-pass result (separable-transform identity; the SCALE
    clip commutes with transposition), reusing one butterfly. SSE2 uses the
    `mullo_epi16`/`mulhi_epi16` widen (source coeffs are i16) and expresses the
    DST core in the collapsed direct-basis matrix (algebraically identical to the
    reference intermediates); AVX2 uses native `cvtepi16_epi32` +
    `mullo_epi32` + `min/max_epi32` (i32-domain transpose). Products accumulate
    in i32 (max ~11M ≪ i32::MAX); `clip_int16` = signed-saturate after `>> shift`.
  - `tests/cross_tier.rs`: an independent naive golden reimplementing the EXACT
    reference `tr_4_core` / `tr_4x4_luma_core` (reference c0/c1/c2/c3
    intermediates) / `idct_dc` math, over bit-depth 8/10/12 and random full-range
    i16 blocks (incl. saturation extremes on both ends), plus fixed KAT vectors
    extracted from the ffmpeg-rs `transform/tests.rs`. Byte-exact green at forced
    tiers 0/3/5/6/8/9.


## [0.3.27] — 2026-07-10

### Added

- **HEVC u16-source MC tap-filter family** — the u16 siblings of the i32-source
  luma 8-tap / chroma 4-tap tap filters (v0.3.22 / v0.3.23), for the ffmpeg-rs MC
  path that gathers reference pixels as **u16** (2 bytes) instead of i32 (4 bytes)
  so the SIMD load packs **2× the lanes per register**. The math is IDENTICAL — a
  u16 pixel (`0..(1<<bit_depth)` ⊆ i16) is widened to i32 then fed to the SAME
  8-tap/4-tap MAC + `>> shift` — so every kernel is **byte-identical to its
  i32-source twin** given the same pixel values.
  - `mc_8tap_h_u16_to_i32` / `mc_8tap_v_u16_to_i32` (luma) + `mc_4tap_h_u16_to_i32`
    / `mc_4tap_v_u16_to_i32` (chroma) — RAW i32 output (NO narrow, NO saturation,
    NO rounding offset), the source-reading passes.
  - `mc_8tap_2d_hp_u16_to_i32` / `mc_4tap_2d_hp_u16_to_i32` — hv 2-pass wrappers:
    the u16 h-pass fills the in-range i16 `htmp` (`[-6142, 22522]` luma /
    `[-2560, 18939]` chroma ⊆ i16, truncation-narrowed byte-exact), then **reuse
    the SHARED** `mc_8tap_v_i16_to_i32` / `mc_4tap_v_i16_to_i32` `>> 6` second pass
    (not duplicated). The i16-htmp 2nd pass and the final clip/combine kernels are
    unchanged.
  - **scalar** reference + **SSE2** (u16 loaded directly as 8 i16 lanes in a single
    128-bit load — half the i32 source's load bytes — then the same
    `mullo_epi16`+`mulhi_epi16`+`unpack` widening MAC as the i32/i16 SSE2 kernels)
    + **AVX2** (`_mm256_cvtepu16_epi32` zero-extend — the mirror of the i16 v-pass's
    `cvtepi16_epi32` — then native `mullo_epi32`). SSE2-tier parity with the
    i32-source family; NEON / wasm fall through to the scalar reference
    (append-only `Dispatch` default; 4 new fields).
  - `tests/cross_tier.rs`: the strongest check is an **equivalence** test —
    `mc_*tap_*_u16_to_i32(u16 pixels)` vs the existing `mc_*tap_*_i32_to_i32(the
    same values as i32)`, asserted byte-identical — plus an independent naive
    golden, over bit-depth 8/10/12, all QPEL/EPEL filters, ten block geometries,
    and both hv wrappers; the shared i16 2nd pass keeps its out-of-i16 adversarial
    tests. Byte-exact green at forced tiers 0/3/5/6/8/9 (u16-vs-i32 equivalence
    holds at SSE2 / SSE4.1 / AVX2).


## [0.3.26] — 2026-07-10

### Added

- **HEVC MC "final" clip/combine family** — the pixel-producing tail of HEVC inter
  prediction, SHARED by luma and chroma (element-wise over the i32 intermediate `v`
  produced by the `mc_*tap_*_i32_to_i32` kernels, so it is independent of the tap
  count). Byte-exact port of ffmpeg-rs `av-codec-h265/mc.rs`
  `put_luma_uni`/`put_luma_uni_w`/`put_luma_bi`/`put_luma_bi_w` (+ `put_luma`'s
  `int16_t` bi-src2 store). `clip_pixel(x) = x.clamp(0, max)` with
  `max = (1<<bit_depth)-1`; the caller pre-derives `offset`/`shift`/`ox`/weights/`bias`
  exactly as the reference computes them from `bit_depth`/`denom`. All arithmetic is
  i32 to match the reference `int` math (the widest product
  `v*wx1 + src2*wx0 + bias` reaches ~1.0e7 ≪ `i32::MAX`; the `v` reaching these
  kernels is the post-shift intermediate, bounded ~±34k).
  - `mc_uni_clip_i32_to_u16` — `dst = clip((v + offset) >> shift)`.
  - `mc_uni_w_clip_i32_to_u16` — `dst = clip(((v*wx + offset) >> shift) + ox)`.
  - `mc_bi_clip_i32_to_u16` — `dst = clip((v + src2 + offset) >> shift)`, `src2` i16.
  - `mc_bi_w_clip_i32_to_u16` — `dst = clip((v*wx1 + src2*wx0 + bias) >> rshift)`, `src2` i16.
  - `mc_store_i32_to_i16` — **WRAPPING** narrow `v as i16` (low 16 bits, NOT
    saturating), the bi first-prediction (src2) store.
  - **scalar** reference (ground truth) + **SSE4.1** (8 lanes/iter) + **AVX2** (16
    lanes/iter). The i32→u16 clamp+narrow reuses the crate's proven idiom
    (`packus_epi32` folds the `≥0` clamp into the narrow, `min_epu16`/`min_epi32`
    applies `≤max`; weights via `mullo_epi32`, `src2` sign-widened via
    `cvtepi16_epi32`) — same SSE4.1 tier as `add_clip_to_pixel_u16` /
    `weighted_avg_u16`, since `packus_epi32`/`mullo_epi32` require SSE4.1. SSE2 /
    NEON / wasm fall through to the scalar reference (append-only `Dispatch`
    default). The wrapping store masks each lane to its low 16 bits then
    `packus_epi32` (exact, in-range).
  - `tests/cross_tier.rs`: independent goldens (the exact reference formulae, never
    calling the primitive) swept over bit-depth 8/10/12, HEVC weight/offset/denom
    ranges, ten block geometries, and `v` planes spanning out-of-i16 magnitudes;
    scalar-vs-golden + detected-vs-scalar at every forced tier, plus a directed
    `mc_store_i16_wraps_not_saturates` test driving values past `i16::MAX`.
    Byte-exact green at forced tiers 0/3/5/6/8/9.


## [0.3.25] — 2026-07-07

### Fixed

- **`satd_block_4x4_u8` unaligned row loads (NEON / SSE2 / wasm)** — the 4-byte
  per-row load did a plain `*(ptr as *const u32/i32)` deref, which is UB (and
  aborts under debug's misaligned-pointer check) when the block sits at an
  arbitrary stride/offset (the common encoder case: sub-blocks inside a padded
  plane). Switched to `read_unaligned`. Bit-identical result; enables callers to
  price 4×4 SATD at any stride. New guard test `satd_4x4_unaligned_stride_matches_scalar`
  (odd strides 7/13/15/17 × base offsets 0/1/3).


## [0.3.24] — 2026-07-07

### Added

- **`satd_block_8x8_u8(a, a_stride, b, b_stride) -> u32`** — Hadamard-transformed
  SAD on an 8×8 `u8` block with the HEVC/HM normalisation `(sum + 2) >> 2` (the
  kvazaar `satd_8x8_subblock` convention). The 8×8 SATD is the dominant
  rough-search / sub-pel-refinement cost in HEVC encoders (blocks ≥ 8 tile into
  8×8 sub-blocks), the missing sibling of the v0.2.1 `satd_block_4x4_u8`.
  - **scalar** reference: two-pass fast Walsh-Hadamard (`wht8`, radix-2 3-stage)
    with an `i32` accumulator; cross-checked in `tests/cross_tier.rs` against an
    INDEPENDENT full 8×8 Sylvester Hadamard **matrix multiply** `H·D·H`
    (`H[i][j] = (-1)^popcount(i&j)`), not a butterfly mirror.
  - **NEON** (AArch64): 8 rows in `int16x8_t`, WHT down the columns, register
    8×8 transpose (trn at 16/32/64), WHT again, `vabsq_s16` + `vpadalq_s16`
    reduction. Validated bit-identical to scalar by
    `satd_8x8_detected_dispatch_matches_scalar` (the WHT is orthogonal so the
    abs-sum is invariant to butterfly order). SSE2/AVX2/wasm fall back to the
    scalar reference via the append-only `Dispatch` default (SIMD tiers TODO).
  - Append-only `Dispatch` field ⇒ non-breaking.


## [0.3.23] — 2026-07-05

### Added

- **HEVC chroma 4-tap MC tap-filter family** — `mc_4tap_h_i32_to_i32`,
  `mc_4tap_v_i32_to_i32`, `mc_4tap_v_i16_to_i32`, and the 2D convenience
  wrapper `mc_4tap_2d_hp_i32_to_i32` (+ public const `MC_CHROMA_EXTRA = 3`).
  The 4-tap sibling of the luma 8-tap i32 family from v0.3.22 — byte-exact port
  of ffmpeg-rs's scalar reference `av-codec-h265/mc.rs` (`chroma_h_sample` /
  `chroma_filter` / `chroma_filter_tmp` / `fill_chroma_htmp`, `EPEL_FILTERS`).
  Same **RAW i32** contract (NO narrow, NO saturation, NO rounding offset — the
  wrapping-i16 `put_chroma` store and the uni/bi `clip_pixel` are separate
  downstream steps):
  - source is **i32** (u16 pixels pre-widened, in `0..(1<<bit_depth)`);
  - h/v-only pass is `Σ_{k=0..4} f[k]·src[base+(k-1)·stride] >> (bit_depth-8)`
    (4 taps, i8 `EPEL_FILTERS`; caller pre-offsets `src` by `EPEL_EXTRA_BEFORE
    = 1` so tap 0 aligns with output column 0);
  - the hv two-pass fills an **i16** `htmp` (caller-provided scratch, `h+3`
    rows via `MC_CHROMA_EXTRA`, `MC_MAX_PB_SIZE`-strided, top-context row 0 =
    source row −1): the horizontal tap filter is narrowed into `htmp` by
    **truncation** (`as i16`, byte-exact since the chroma value ⊆ `[-2560,
    18939]`), then the vertical pass is `Σ f[k]·htmp[k] >> 6` — emitted as raw
    i32 (NOT saturated; a directed adversarial test drives full-range i16
    `htmp` past `i16::MAX` to 43006 and asserts the exact unsaturated value).
  3 append-only `Dispatch` fields; scalar + SSE2 (source-only i32→i16 narrow +
  `mullo`/`mulhi`, raw i32 store) + AVX2 (native `mullo_epi32`, raw i32 store)
  backends, cross-tier green at forced tiers 0/3/5/6/8/9.  NEON/wasm fall
  through to the bit-exact scalar reference (SIMD backfill deferred, per
  CAVEMAN's documented scalar-fallthrough allowance); `aarch64` + `wasm32`
  cross-compile clean.


## [0.3.22] — 2026-07-05

### Added

- **HEVC luma 8-tap MC tap-filter family** — `mc_8tap_h_i32_to_i32`,
  `mc_8tap_v_i32_to_i32`, `mc_8tap_v_i16_to_i32`, and the 2D convenience
  wrapper `mc_8tap_2d_hp_i32_to_i32` (+ public consts `MC_MAX_PB_SIZE = 64`,
  `MC_LUMA_EXTRA = 7`).  Byte-exact port of ffmpeg-rs's scalar reference
  `av-codec-h265/mc.rs` (`luma_h_sample` / `luma_filter` / `luma_filter_tmp` /
  `fill_luma_htmp`).  These are the tap filters that emit the **RAW i32** MC
  intermediate BEFORE any pixel narrow — downstream `put_luma` truncates it to
  `int16_t` (WRAPPING) while `put_luma_uni/_bi/_uni_w/_bi_w` keep the full i32
  and `clip_pixel`:
  - source is **i32** (u16 pixels pre-widened, in `0..(1<<bit_depth)`);
  - h/v-only pass is `Σ f[k]·src[k] >> (bit_depth-8)` — a **pure arithmetic
    right shift with NO rounding offset, NO narrow, NO saturation** (unlike the
    VP9/AV1 `mc_8tap_*_u8` `+64 >> 7`);
  - the hv two-pass fills an **i16** `htmp` (caller-provided scratch, `h+7`
    rows, `MC_MAX_PB_SIZE`-strided, top-context row 0 = source row −3): the
    horizontal tap filter is narrowed into `htmp` by **truncation** (`as i16`,
    byte-exact since the value ⊆ `[-6142, 22522]`), then the vertical pass is
    `Σ f[k]·htmp[k] >> 6` — whose output CAN exceed i16 (e.g. 33271) and so is
    emitted as raw i32 (NOT saturated);
  - coefficients are the i8 `QPEL_FILTERS`.
  3 append-only `Dispatch` fields; scalar + SSE2 (source-only i32→i16 narrow +
  `mullo`/`mulhi`, raw i32 store) + AVX2 (native `mullo_epi32`, raw i32 store)
  backends, cross-tier green at forced tiers 0/3/5/6/8/9 (incl. a directed
  adversarial test proving the hv output is NOT saturated at SSE2/AVX2).
  NEON/wasm fall through to the bit-exact scalar reference (a hand-tuned SIMD
  kernel for those ISAs is deferred rather than shipped untested, per CAVEMAN's
  documented scalar-fallthrough allowance); `aarch64` + `wasm32` cross-compile
  clean.
  - **Fix (post-initial-landing):** the tap filters originally narrowed their
    FINAL output to i16 with **saturation** (`_mm_packs_epi32` / `.clamp(i16)`).
    That is wrong for the hv second pass, whose value can exceed i16 — ffmpeg-rs
    stores it either wrapping (`put`, mc.rs:474) or as full i32 (uni/bi), never
    saturated.  The primitives now output the raw i32 intermediate (the i16
    narrow / pixel clip are separate downstream steps), renamed `*_to_i32`.

## [0.3.21] — 2026-07-05

### Added

- **`add_clip_to_pixel_u16_inplace(pix, res, max)`** and
  **`add_clip_to_pixel_u8_inplace(pix, res)`** — safe in-place siblings of
  `add_clip_to_pixel_u16`/`_u8` where `pix` is both prediction and
  destination: `pix[i] = clamp(pix[i] + res[i], 0, max)`.  This is the exact
  shape decoder reconstruction needs (HEVC/H.264 `add_residual` accumulates
  the residual straight onto the prediction buffer), which the out-of-place
  API cannot express in safe Rust — `pix` would have to be borrowed as both
  `&[_]` and `&mut [_]` at once, forcing callers to copy into a scratch
  buffer.  Pure safe wrappers over the existing `Dispatch` pointers — **no
  new kernel, no new `Dispatch` field**, so append-only rules are untouched.

### Soundness

Aliasing `pred == dst` is sound because every `add_clip_to_pixel_*` kernel
(scalar + SSE4.1 + AVX2 + NEON + wasm) is strictly *element-local*: output
index `i` reads only `pred[i]`/`res[i]` and writes only `dst[i]`, and each
SIMD block loads its whole `[i..i+N)` window before storing back to it, so no
lane reads an index a prior store overwrote.  A cross-tier test forced across
levels 0/3/5/6/8/9 proves the in-place output is byte-identical to the
out-of-place path at every SIMD tier.

## [0.3.20] — 2026-07-05

### Added

- **`add_clip_to_pixel_u8(pred, res, dst)`** — fused add-residual + clip to
  8-bit pixels: `dst[i] = (pred[i] + res[i]).clamp(0, 255)` (pass `dst==pred`
  for in-place reconstruction).  The u8 sibling of `add_clip_to_pixel_u16`.
  Pervasive decoder reconstruction step — confirmed callers: ffmpeg-rs H.264
  IDCT store, HEVC `add_residual`, ProRes, AV1 recon.

### Backends

Accumulates in **i32 lanes** (255 + i16::MAX overflows i16): SSE4.1/AVX2
(`cvtepu8_epi32`/`cvtepi16_epi32` → add → `max/min_epi32` clamp →
`packus_epi32`+`packus_epi16`), NEON (`vmovl`→add→`vmaxq/vminq`→`vqmovun`),
wasm (`i32x4` widen/add/clamp/narrow).  SSE2/SSSE3 fall back to scalar
(mirrors `add_clip_to_pixel_u16`'s SSE4.1-native tiering).

### Refactor (unversioned, between 0.3.19 and 0.3.20)

- **Layout modularized.**  `lib.rs` (1442→~35 lines) split: public API →
  `src/api/{convert,arith,cost,mc,colour}.rs`; each per-ISA kernel monolith
  `prims_{isa}.rs` → `src/prims_{isa}/{family}.rs` (re-exported via
  `pub(crate) use`, so `dispatch.rs` paths are unchanged).  Zero behaviour
  change; public API byte-identical.  Feature flags `mc/transform/lpf/dwt`
  declared but gating deferred.

## [0.3.19] — 2026-06-29

### Added

- **`rct_fwd_i32(r, g, b, y, cb, cr, n)`** and
  **`rct_bwd_i32(y, cb, cr, r, g, b, n)`** — HTJ2K reversible colour
  transform on `n` i32 samples (separate planes).  Forward (RGB→YCbCr):
  `Y=(R+2G+B)>>2; Cb=B-G; Cr=R-G`.  Inverse (YCbCr→RGB):
  `G=Y-((Cb+Cr)>>2); R=Cr+G; B=Cb+G`.  Lossless / bit-exact; `bwd∘fwd = id`.
  Constants verified against `jph-rs` (`CONTRACTS.md` §HTJ2K RCT).  First
  primitive shipped off the codec-survey contracts (`CODECS.md` order #2).

### Backends

Constant-free integer plane op, so a real SIMD path exists on **every**
backend (no scalar fallback): SSE2/AVX2 (`srai_epi32`/`add`/`sub`/`slli`),
NEON (`vshrq_n_s32`/`vaddq_s32`/`vsubq_s32`), wasm (`i32x4_shr`/`_add`/`_sub`).
Arithmetic (signed) shift throughout.

### Test coverage

Detected-vs-scalar for fwd and bwd over 13 sizes (every scalar-tail residue
1..=3 plus SIMD multiples), and a 257-sample lossless round-trip
(`bwd(fwd(rgb)) == rgb`).

## [0.3.18] — 2026-06-29

### Added

- **`weighted_avg_u16(a, a_stride, b, b_stride, dst, dst_stride, w, h, w0, w1, offset, shift, max)`**
  — explicit weighted bi-prediction on u16 blocks:
  `dst = clamp((w0*a + w1*b + offset) >> shift, 0, max)`.  The
  high-bit-depth sibling of `weighted_avg_u8`; `max = (1<<bd)-1`.

### Backends

The u16 weighted multiply needs a 32-bit product, so SIMD is SSE4.1+
(`cvtepu16_epi32` + `mullo_epi32` + `packus_epi32` + `min_epu16`),
host-verified via detected dispatch.  SSE2 / NEON / wasm use the scalar
fallback (as `sat_pack_i32_to_u16` does).

### Test coverage

Detected-vs-scalar over 8 sizes × 5 parameter sets (10/12-bit ceilings,
negative weight); plus a clamp-to-max check.

## [0.3.17] — 2026-06-29

**First high-bit-depth (u16) primitive** — the start of the 10/12-bit
sibling family.

### Added

- **`mc_avg_u16(a, a_stride, b, b_stride, dst, dst_stride, w, h)`** —
  rounding halving add of two `u16` blocks, `dst = (a + b + 1) >> 1`.
  The high-bit-depth sibling of `mc_avg_u8` (B-frame bi-prediction on
  u16 sample planes).  Strides are in u16 elements.

### Backends

- SSE2 `_mm_avg_epu16` / AVX2 `_mm256_avg_epu16` — single instruction.
- NEON `vrhaddq_u16` — single instruction.
- wasm `simd128` — no native u16 rounding average; emulated overflow-free
  via `(a | b) - ((a ^ b) >> 1)`.

### Test coverage

Detected-vs-scalar over 8 sizes on full-range u16 planes; plus a
rounding check at the u16 extremes (`(65535+0+1)>>1 = 32768`).

## [0.3.16] — 2026-06-29

### Added

- AArch64 NEON and wasm `simd128` implementations of
  **`hadamard_4x4_i16`** — completes the v0.3.15 primitive across all
  five backends (it previously fell back to scalar on NEON/wasm).

### Notes

NEON uses `vtrn`-based transpose, wasm uses `i16x8_shuffle`/`i32x4_shuffle`
— the same proven 4×4 transposes as the SATD kernels, with a second
transpose added to restore row-major order.  Both use the identical
algorithm and transpose orientation as the host-verified SSE2 path, so
they are correct by construction (cross-compile clean on
`aarch64-unknown-linux-gnu` / `wasm32-unknown-unknown`; the existing
`hadamard_4x4_*` cross-tier tests cover them when run on those targets).

## [0.3.15] — 2026-06-29

### Added

- **`hadamard_4x4_i16(src: &[i16; 16], dst: &mut [i16; 16])`** — the 2D
  Walsh-Hadamard transform `Y = H·X·H` (the ±1 matrix, no constants).
  Codec-agnostic; the building block inside 8×8 SATD and a
  transform-domain analysis tool.  The first member of the
  inverse-transform family that needs no codec-specific constants.

### Backends

- scalar — row pass then column pass.
- SSE2 (also AVX2) — 4 rows in the low 64 bits of 4 `__m128i`; the
  column butterfly is register add/sub, the row butterfly is the same
  after the classic in-register `unpack` 4×4 transpose, with a final
  transpose to restore row-major order.  Verified bit-exact against
  scalar.
- AArch64 NEON / wasm — scalar fallback for now; a SIMD backfill is
  roadmapped (mirrors the v0.2.2 SATD NEON/wasm backfill).

### Test coverage

Detected-vs-scalar over 40 random blocks; plus the algebraic property
that applying the transform twice yields `16·X` (`H·H = 4I`).

### Note on the remaining transform family

DCT/IDCT and ADST need codec-specific butterfly constants and shift
schedules (H.264 4×4 ≠ HEVC ≠ VP9/AV1) and must be designed per codec,
not guessed; see `NEXT_SESSION.md`.  `hadamard` and (future) `iwht_4x4`
are the constant-free members that can be added directly.

## [0.3.14] — 2026-06-29

### Added

- **`lincomb3_clip_u8(p0, p1, p2, dst, c0, c1, c2, offset, shift)`** —
  `dst[i] = clip_u8((c0*p0[i] + c1*p1[i] + c2*p2[i] + offset) >> shift)`.
  The **YUV→RGB output-plane** building block (call once per channel
  with that channel's matrix row, biases folded into `offset`) and a
  general 3-input planar linear blend.  Standard-agnostic — the caller
  supplies the BT.601/709/2020 coefficients.

### Backends

Widen u8→i16, signed widening multiply by each coefficient, sum +
offset, round-shift, clip to u8.  SSE2 `mullo`+`mulhi`, AVX2
`mullo_epi32` (16/iter), NEON `vmull_n_s16`, wasm `extmul`.  Planar in /
planar out (no interleave shuffles).

### Test coverage

Detected-vs-scalar over 10 lengths × 5 coefficient sets (incl. real
BT.601 red/green rows with negative coefficients); plus a passthrough
check (`1,0,0 / 0 / 0`) and a BT.601 uniform-pixel sanity value.

## [0.3.13] — 2026-06-29

### Added

- **`weighted_avg_u8(a, a_stride, b, b_stride, dst, dst_stride, w, h, w0, w1, offset, shift)`**
  — explicit weighted bi-prediction blend:
  `dst = clip_u8((w0*a + w1*b + offset) >> shift)`.  The generic affine
  blend behind H.264/HEVC explicit weighted prediction; the caller maps
  its slice weighted-pred parameters onto `w0,w1,offset,shift` (nothing
  hard-coded).  Unit weights (`1,1,1,1`) reduce to `mc_avg_u8`.

### Backends

Same full-width product + round-shift + clip-u8 shape as the MC kernels,
with two weighted inputs: SSE2 `mullo`+`mulhi`, AVX2 `mullo_epi32`
(16 cols), NEON `vmull_n_s16`, wasm `extmul`.

### Test coverage

Detected-vs-scalar over 8 sizes × 5 (w0,w1,offset,shift) combinations
(incl. negative weight); plus a property test that unit weights equal
`mc_avg_u8`.

## [0.3.12] — 2026-06-29

### Added

- **`dequant_block_i16(buf: &mut [i16], scale: i16, shift: u32)`** —
  in-place uniform dequantization: `buf[i] = sat_i16((buf[i]*scale +
  round) >> shift)`.  The decoder dequant inner loop with a uniform
  scale; parameterized so no codec is hard-coded (per-position scaling
  matrices are applied by the caller per run of equal scale).

### Backends

Flat per-element: signed widening multiply by the broadcast `i16` scale
(`mullo`+`mulhi` SSE2, `mullo_epi16`/`mulhi_epi16` AVX2 with cancelling
per-128 unpack/pack, `vmull_n_s16` NEON, `extmul` wasm), round-shift,
saturating narrow to i16.  SSE2/NEON/wasm 8/iter, AVX2 16/iter.

### Test coverage

Detected-vs-scalar over 9 lengths × 6 scales (incl. negative) × 5 shifts
(incl. `0`); plus known-value checks (scale 64/shift 6 identity; scale
4/shift 0 saturation).

## [0.3.11] — 2026-06-29

### Added

- **`sad_x4_block_u8(a, a_stride, refs: &[&[u8]; 4], b_stride, w, h) -> [u32; 4]`**
  — sum of absolute differences of one `a` block against four candidate
  blocks at once (the roadmap's v0.2.6 item).  The encoder motion-search
  inner loop: each `a` row is loaded once and run against all four
  candidates.

### Backends

- SSE2/AVX2 — `_mm_sad_epu8` / `_mm256_sad_epu8` per candidate, four
  64-bit accumulators.
- NEON — `vabdq_u8` + `vpadalq_u8` per candidate.
- wasm — emulated SAD (`max-min` + pairwise widen) per candidate.

### Test coverage

`sad_x4` (detected and scalar) vs four independent `sad_block_u8` calls
over 8 sizes and four distinct candidate offsets; plus a public-API
smoke test.

## [0.3.10] — 2026-06-29

**i16-intermediate MC family complete.**

### Added

- **`mc_6tap_h_u8_to_i16`** / **`mc_6tap_v_i16_to_u8`** — 6-tap i16 bricks.
- **`mc_6tap_2d_hp_u8`** — high-precision 2D 6-tap luma: the **H.264
  centre half-pel `j`** position.  With `filter = [1,-5,20,20,-5,1]`,
  `shift1 = 0` (unrounded i16 intermediate), `shift2 = 10`, this is the
  exact H.264 `j` formula `Clip((Σ fv·(Σ fh·src) + 512) >> 10)`.

### Backends / contract

Identical to the 8-tap i16 bricks with a 6-tap loop, `w+5`/`h+5`
padding.  The 2D wrapper is a pure composition.

### Test coverage

Both bricks detected-vs-scalar (5 filters × 3 shifts × 7 sizes); the 2D
wrapper against the real H.264 centre-`j` two-pass reference
(`shift1=0, shift2=10`) over 6 sizes.

### MC status

The MC family is now broadly complete for 8-bit: 1D u8 (8/6/4/2-tap
h/v), u8-intermediate 2D, and the full i16-intermediate 2D family
(8-tap HEVC/AV1 luma, 4-tap HEVC chroma, 6-tap H.264 centre).  Next:
u16 high-bit-depth, then the inverse-transform family (v0.4.x).

## [0.3.9] — 2026-06-29

### Added

- **`mc_4tap_h_u8_to_i16`** / **`mc_4tap_v_i16_to_u8`** — the 4-tap i16
  bricks (HEVC chroma 2D passes), 4-tap siblings of the v0.3.6/0.3.7
  8-tap bricks.
- **`mc_4tap_2d_hp_u8(dst, dst_stride, src, src_stride, w, h, filter_h, filter_v, shift1, shift2)`**
  — high-precision separable 2D 4-tap chroma MC (HEVC chroma) with an
  i16 intermediate; thin wrapper composing the two bricks through an
  i16 scratch `(h+3)×w`.

### Backends / contract

Identical shape to the 8-tap i16 bricks with a 4-tap loop and `w+3`
(h) / `h+3` (v) padding.  Runtime `shift` (0..=14), caller picks the
per-pass shifts.  No new dispatch for the 2D wrapper (pure composition).

### Test coverage

Both bricks detected-vs-scalar (4 filters × 3 shifts {0,6,12} × 7 sizes,
i16 output and i16 input planes incl. negatives); the 2D wrapper vs an
independent two-pass reference (3 shift pairs × 6 sizes).

## [0.3.8] — 2026-06-29

### Added

- **`mc_8tap_2d_hp_u8(dst, dst_stride, src, src_stride, w, h, filter_h, filter_v, shift1, shift2)`**
  — high-precision separable 2D 8-tap motion compensation (HEVC/AV1
  convention) with an **i16 intermediate**.  Horizontal pass at i16
  precision (`shift1`), vertical pass clips to u8 (`shift2`).

### Why a composition, not new backend code

Like `mc_8tap_2d_u8`, this is a thin `lib.rs` wrapper — it runs
`mc_8tap_h_u8_to_i16` (v0.3.6) into an i16 scratch `(h+7)×w`, then
`mc_8tap_v_i16_to_u8` (v0.3.7).  No new `Dispatch` field, no per-ISA
code.  The difference from v0.3.2's `mc_8tap_2d_u8` is the **i16** (not
u8) intermediate, so HEVC/AV1 2D is bit-exact instead of precision-lossy.
The caller supplies both shifts (HEVC luma: `shift1=BD-8`,
`shift2=20-BD`), so no codec is hard-coded.

### Test coverage

Three shift pairs (`(0,12)` = HEVC 8-bit, `(2,10)`, `(6,6)`) × 7 block
sizes, the public wrapper vs an independent two-pass i16-intermediate
reference.

### MC milestone

High-precision 2D luma MC is now complete.  Remaining i16 work: the
4-tap (HEVC chroma) and 6-tap (H.264 centre) 2D siblings, then the u16
high-bit-depth family.

## [0.3.7] — 2026-06-29

### Added

- **`mc_8tap_v_i16_to_u8(dst: &[u8], dst_stride, src: &[i16], src_stride, w, h, filter: &[i16; 8], shift: u32)`**
  — second pass of a high-precision 2D filter: vertical 8-tap over an
  i16 intermediate, clipped back to u8.
  `dst[r][c] = clip_u8((Σ src[r+i][c]·filter[i] + round) >> shift)`,
  `round = if shift==0 {0} else {1<<(shift-1)}`.  `src_stride` in i16
  elements.

### Why

The partner of v0.3.6's `mc_8tap_h_u8_to_i16`.  Together they form a
full high-precision 2D MC — the horizontal pass keeps i16 precision,
the vertical pass applies the final rounding/clip.  The caller supplies
both shifts (HEVC luma 2D: `shift1=BD-8`, `shift2=20-BD`; H.264 centre:
`0` then `10`), so no codec contract is hard-coded.

### Backends

The vertical u8 kernel with the input load changed from u8 (`vld1_u8` +
widen) to i16 read directly (`loadu_si128` / `_mm256_loadu_si256` /
`vld1q_s16` / `v128_load`); the product is i16×i16→i32 (same
`mullo`+`mulhi` / `mullo_epi32` / `vmlal_s16` / `extmul`), finalize is
the runtime-shift clip-to-u8 from the 4/6-tap kernels.  SSE2/NEON/wasm
8 cols/iter, AVX2 16.  Vertical padding `h + 7` i16 rows.

### Test coverage

5 filters × 4 shifts (`0, 4, 7, 12` — 12 is HEVC's `20-BD` second pass)
× ~8 sizes, scalar-vs-naive and detected-vs-scalar over an i16 source
plane that includes negatives and above-pixel magnitudes; an
identity-clamps-intermediate check; **and an integration test** that
composes the v0.3.6 + v0.3.7 bricks into an HEVC-8bit-style 2D
(`shift1=0, shift2=12`) and verifies it against an independent two-pass
i16-intermediate reference — a preview of the v0.3.8 `mc_8tap_2d_hp_u8`
wrapper.

## [0.3.6] — 2026-06-29

**Start of the i16-intermediate MC path** — the high-precision 2D
foundation.  Built incrementally, one primitive per version.

### Added

- **`mc_8tap_h_u8_to_i16(dst: &[i16], dst_stride, src: &[u8], src_stride, w, h, filter: &[i16; 8], shift: u32)`**
  — first pass of a high-precision 2D filter: horizontal 8-tap, u8 →
  i16, **not** clipped to pixel range.
  `dst[r][c] = sat_i16((Σ src[r][c+i]·filter[i] + round) >> shift)`,
  `round = if shift==0 {0} else {1<<(shift-1)}`.  `dst_stride` is in i16
  elements.

### Why an i16 intermediate (vs. the v0.3.2 u8 one)

`mc_8tap_2d_u8` clips its horizontal pass to u8 (libvpx convention),
losing precision.  HEVC/AV1 2D and the H.264 centre half-pel keep the
horizontal result at full i16 precision and apply the rounding only in
the vertical pass.  This primitive is that i16-producing first pass.
The `shift` is a runtime parameter (now also allowing `shift = 0`, the
H.264-centre / HEVC-8bit case) so the **caller picks the codec contract**
— nothing is hard-coded.

### Design (incremental plan)

The i16 path is built as codec-agnostic bricks, one per version:
v0.3.6 this horizontal pass; v0.3.7 the i16-input vertical pass
(`mc_8tap_v_i16_to_u8`); v0.3.8 the 2D composition taking `shift1`/
`shift2`; then the 4-tap (HEVC chroma 2D) and 6-tap (H.264 centre)
siblings.

### Backends

Same full-width `u8 × i16` product as the u8 8-tap kernel, but the
finalize narrows i32 → i16 (saturating: `packs_epi32` / `vqmovn_s32` /
`i16x8_narrow_i32x4`) and stores i16 instead of clipping to u8.  Runtime
shift via `sra_epi32` / `vshlq_s32` / `i32x4_shr`.  SSE2/NEON/wasm 8
cols/iter, AVX2 16.

### Test coverage

5 filters × 4 shifts (`0, 4, 6, 7`, incl. the no-round `0` case) × ~8
sizes, scalar-vs-naive and detected-vs-scalar; identity check and a
shift-0 high-pass case asserting the output is genuinely unclipped
(produces negative i16) and still bit-exact across tiers.

## [0.3.5] — 2026-06-29

### Added

- **`mc_bilinear_h_u8` / `mc_bilinear_v_u8(dst, dst_stride, src, src_stride, w, h, filter: &[i16; 2], shift: u32)`**
  — horizontal and vertical 2-tap bilinear motion compensation, u8 → u8,
  with a runtime `shift`.  For H.264 chroma the caller passes
  `[8-frac, frac]` and `shift = 3`.

### Why

The smallest MC kernel — H.264 chroma and AVIF use bilinear sub-pel.
It is the same runtime-shift shape as the 4/6-tap kernels with a 2-tap
loop, so it closes the 1D MC family (8/6/4/2-tap) with no new patterns.
Horizontal needs `w + 1` columns; vertical needs `h + 1` rows.

### Test coverage

5 filters (frac-0 identity, three H.264 chroma fractions, adversarial)
× 3 shifts (`1, 3, 6`) × ~8 sizes, scalar-vs-naive and
detected-vs-scalar, h and v; plus a frac-0 passthrough identity check.

### MC family status after v0.3.5

1D separable MC is complete for u8: 8-tap luma (`>>7`), 6-tap H.264 luma,
4-tap HEVC chroma, 2-tap bilinear (all runtime-shift except 8-tap), plus
8-tap 2D (u8 intermediate).  All share one inner shape: full-width
`u8 × i16` product → round/shift → saturating narrow to u8.  Remaining
MC work is the **i16-intermediate** path (codec-exact 2D for HEVC/AV1,
the H.264 centre half-pel, and the u16 high-bit-depth family).

## [0.3.4] — 2026-06-29

### Added

- **`mc_6tap_h_u8` / `mc_6tap_v_u8(dst, dst_stride, src, src_stride, w, h, filter: &[i16; 6], shift: u32)`**
  — horizontal and vertical 6-tap luma motion compensation, u8 → u8,
  with a runtime `shift`.  For H.264 luma half-pel the caller passes
  `[1, -5, 20, 20, -5, 1]` and `shift = 5`.

### Why (H.264 luma half-pel is a single shift)

The H.264 6-tap Wiener half-pel sample is
`Clip1((Σ [1,-5,20,20,-5,1]·src + 16) >> 5)` — a single normalization
shift, exactly the runtime-shift shape introduced for chroma in v0.3.3.
Reusing that infrastructure, `mc_6tap_{h,v}` are the 4-tap kernels with
a 6-tap inner loop.  (The 2D centre half-pel `j` position uses an
*unrounded* i16 intermediate and `>> 10` — that is the deferred
i16-intermediate work; quarter-pel positions are integer/half-pel
averages handled by `mc_avg_u8`.)

### Backends

Identical to the 4-tap kernels (full-width `u8 × i16` product, runtime
bias-add + register/`vshlq`/`i32x4_shr` shift + saturating narrow),
with a 6-tap loop.  SSE2/NEON/wasm do 8 cols/iter, AVX2 16.  Horizontal
needs `w + 5` columns; vertical needs `h + 5` rows.

### Test coverage

5 filters (identity, the real H.264 `[1,-5,20,20,-5,1]`, 2× H.264, box,
adversarial) × 4 shifts (`1, 5, 6, 7`) × ~8 sizes, scalar-vs-naive and
detected-vs-scalar, h and v; plus an identity spatial check and a
property test that the H.264 half-pel filter reproduces a constant
plane (taps sum to 2^5, shift 5).

## [0.3.3] — 2026-06-29

### Added

- **`mc_4tap_h_u8` / `mc_4tap_v_u8(dst, dst_stride, src, src_stride, w, h, filter: &[i16; 4], shift: u32)`**
  — horizontal and vertical 4-tap chroma motion compensation, u8 → u8,
  with a **runtime normalization `shift`**.  Per pixel:
  `clip_u8((Σ_{i=0..4} src[..] * filter[i] + (1<<(shift-1))) >> shift)`.

### Why a runtime `shift` (unlike the 8-tap luma kernels)

The luma 8-tap kernels hard-code `>> 7` (VP9/AV1 normalize to 128).
Chroma normalization is codec-dependent: **HEVC chroma uses `shift = 6`**
(its 4-tap filters sum to 64), other configs use 7.  Rather than guess a
single codec's contract or ship a separate kernel per shift, `shift` is
a parameter (`1..=14`) — one general, reusable chroma filter.  The round
bias is `1 << (shift-1)`.

### Backends

Same full-width `u8 × i16` product as the 8-tap kernels (bit-exact for
any tap), with a runtime-shift finalize instead of the const
`vqrshrun_n`:

- SSE2 / AVX2 — manual round-bias add + `_mm_sra_epi32` /
  `_mm256_sra_epi32` with a register shift count; AVX2 does 16 cols/iter
  (`mullo_epi32`) with 8-col + scalar tails.
- NEON — `vmlal_s16` MAC, then bias-add + `vshlq_s32` (arithmetic right
  by `-shift`) + `vqmovun_s32` (clamp ≥0) + `vqmovn_u16` (clamp ≤255).
- wasm `simd128` — `i32x4_extmul_*` product, bias-add + `i32x4_shr`
  (runtime amount) + signed/unsigned dual-narrow.

Horizontal needs `w + 3` readable columns per row; vertical needs
`h + 3` rows (vs. `w+7` / `h+7` for the 8-tap kernels).

### Test coverage

5 filters (identity, two HEVC chroma phases, box, adversarial) × 4
shifts (`1, 6, 7, 11`) × ~8 block sizes for scalar-vs-naive and
detected-vs-scalar, on both h and v; plus identity (`shift 6`) and
clamp-high/low saturation checks.  The shift sweep exercises the
runtime register-shift path on every backend.

### Notes

- 4-tap 2D chroma is deferred: HEVC chroma 2D uses an i16 (not u8)
  intermediate, so it cannot reuse the u8-composition trick of
  `mc_8tap_2d_u8`; it belongs with the future i16-intermediate work.

## [0.3.2] — 2026-06-29

### Added

- **`mc_8tap_2d_u8(dst, dst_stride, src, src_stride, w, h, filter_h, filter_v)`**
  — separable 2D 8-tap motion compensation with a **u8 intermediate**
  (the libvpx `convolve8` convention).  Horizontal-filters `h + 7` rows
  into a u8 scratch plane, then vertical-filters that into the `w × h`
  output.

### Why this is a composition, not a new backend

The u8-intermediate 2D filter is *exactly* `mc_8tap_v_u8 ∘ mc_8tap_h_u8`,
so it is implemented as a thin wrapper in `lib.rs` that calls the two
existing dispatched kernels through a scratch buffer.  No new
`Dispatch` field, no new per-ISA code — the SIMD lives entirely in the
h/v kernels (single point of responsibility).  Bit-identical to libvpx
`vpx_convolve8_c`.

### Contract

`src` is pre-offset by `-3*src_stride - 3`; the horizontal pass needs
`w + 7` columns and feeds 8 vertical taps, so the source must expose
`h + 7` rows × `w + 7` columns.  Both passes round `+64`, shift `>> 7`,
and clip to `[0,255]` (the intermediate is clipped to u8 — the defining
property of this convention).

The higher-precision **i16-intermediate** 2D filter (AV1/HEVC/dav1d,
where the horizontal pass is *not* clipped to u8) is a deliberately
separate future primitive with its own rounding contract; it needs
i16-output kernels and is not bit-compatible with this one.

### Test coverage

- Full filter cross-product (5 × 5 h/v kernels) × ~10 block sizes
  against an independent two-pass naive reference; runs under the tier
  sweep so every ISA's h/v path is exercised through the 2D wrapper.
- Separable-identity check: identity taps on both axes →
  `out[r][c] == src[(r+3)*stride + (c+3)]`.

### Notes

- A per-call scratch `Vec` is allocated; callers on a zero-alloc hot
  path should compose `mc_8tap_h_u8` + `mc_8tap_v_u8` with their own
  reusable buffer (which is all this wrapper does).  A scratch-taking
  variant can be added if a consumer needs it.

## [0.3.1] — 2026-06-29

### Added

- **`mc_8tap_v_u8(dst, dst_stride, src, src_stride, w, h, filter: &[i16; 8])`**
  — vertical 8-tap separable motion-compensation filter, u8 → u8.  The
  vertical sibling of v0.3.0's horizontal kernel.  Per pixel:
  `dst[r][c] = clip_u8((Σ_{i=0..8} src[r+i][c] * filter[i] + 64) >> 7)`.

### Why

The other half of separable luma motion compensation.  With
`mc_8tap_h_u8` this completes the H/V pair; the 2D combiner
(`mc_8tap_2d_u8`, v0.3.2) will chain them through an i16 intermediate.
Vertical + horizontal MC together are ≈30-40% of decode time.

### Edge convention (caller contract)

`src` is pre-offset so tap 0 aligns with output row 0 (caller passes
`src_origin - 3*src_stride`).  Output row `r` reads source rows
`r..r+8`, so the plane must have `h + 7` readable rows from the pointer
(columns need only `w`).  Fixed `>> 7` matches the VP9/AV1 8-tap
normalization.

### Backends

Structurally the horizontal kernel with the tap axis rotated: each tap
loads a *whole source row* (`src[(row+i)*src_stride + col ..]`) instead
of a shifted slice, so there are no in-row shuffles — just 8 row loads
times the broadcast taps.  All five form the `u8 × i16` product at full
i32 width (bit-exact for any tap):

- scalar — reference triple loop.
- SSE2 — 8 cols/iter; `mullo`+`mulhi`+`unpack` widening multiply.
- AVX2 — 16 cols/iter via `_mm256_mullo_epi32`; 8-col + scalar tails.
- NEON — 8 cols/iter; `vmlal_s16` + `vqrshrun_n_s32::<7>` + `vqmovn_u16`.
- wasm `simd128` — 8 cols/iter; `i32x4_extmul_*_i16x8` + dual-narrow.

Column over-read is bounded by `w` (no horizontal padding needed); only
the row padding (`h+7`) differs from the horizontal kernel.

### Test coverage

Mirrors v0.3.0: 5 filters (identity, two real kernels, box blur,
adversarial `[255,-255,…]`) × ~10 block sizes for scalar-vs-naive and
detected-vs-scalar; vertical identity check (`dst[r][c] ==
src[(r+3)*stride + c]`), clamp-high/low saturation, extreme-tap
conformance at odd widths.

## [0.3.0] — 2026-06-29

**First Tier-3 milestone — motion compensation.**  The decoder hot path
begins here.

### Added

- **`mc_8tap_h_u8(dst, dst_stride, src, src_stride, w, h, filter: &[i16; 8])`**
  — horizontal 8-tap separable motion-compensation filter, u8 → u8.
  Per pixel: `dst[c] = clip_u8((Σ_{i=0..8} src[c+i] * filter[i] + 64) >> 7)`.

### Why this primitive matters

Motion compensation is ≈30-40% of decode time in VP9/AV1/HEVC — the
single highest-impact kernel in the whole roadmap.  This is the
horizontal half of the separable luma filter; the vertical sibling
(`mc_8tap_v_u8`) and the 2D combiner follow in v0.3.1/0.3.2.

### Edge convention (caller contract)

`src` is pre-offset so tap 0 aligns with output column 0 (caller passes
`src_origin - 3`).  Each output column `c` reads `src[c..c+8)`, so every
source row must have `w + 7` readable bytes from the pointer.  Padding
the reference plane is the caller's responsibility — the standard
libvpx/dav1d MC convention.  Fixed `>> 7` matches the VP9/AV1 8-tap
normalization (taps sum to 128).

### Backends

All five form the `u8 × i16` product at **full i32 width**, so the
result is bit-exact for any i16 filter (no `|tap| ≤ 128` restriction):

- scalar — reference triple loop.
- SSE2 — 8 cols/iter; signed widening multiply (`mullo` + `mulhi` +
  `unpack` → i32) into two i32x4 lanes, then `(acc+64)>>7`,
  `packs_epi32` (i32→i16) + `packus_epi16` (i16→u8 = clamp [0,255]).
- AVX2 — 16 cols/iter via `_mm256_mullo_epi32` on i32-widened source
  (exact); 8-col remainder uses the SSE2 path; scalar tail.
- AArch64 NEON — 8 cols/iter; `vmlal_s16` widening MAC with the
  broadcast tap, then **`vqrshrun_n_s32::<7>`** (round-shift + clamp-≥0
  in one instruction) + `vqmovn_u16` (clamp ≤255).
- wasm `simd128` — 8 cols/iter; `i32x4_extmul_{low,high}_i16x8` for the
  exact widening product, `i32x4_shr` by 7, `i16x8_narrow_i32x4` +
  `u8x16_narrow_i16x8` for the clamp.  Loads via `v128_load64_zero`
  (8 bytes) to honour the `w+7` padding bound.

### Test coverage

- 5 filters (identity, two real VP9/HEVC kernels, box blur, and an
  adversarial `[255,-255,…]` kernel that overflows a 16-bit product —
  the test that proves the full-width multiply is necessary) × ~10
  block sizes, for scalar-vs-naive and detected-vs-scalar.
- Identity-filter spatial check (`dst[c] == src[c+3]`), clamp-high
  (→255) and clamp-low (→0) saturation, and extreme-tap conformance at
  odd widths exercising the scalar tail.

### Notes

- The `mc` Cargo feature stays a no-op placeholder; this kernel is
  always compiled, matching the v0.1.x/v0.2.x primitives.  Feature-
  gating the MC family can come when the set is larger.
- `Dispatch` grew one field (append-only); minor version bump because
  this opens the v0.3.x milestone.

## [0.2.5] — 2026-06-29

### Added

- **`variance_block_u8(src, stride, w, h) -> (sum: u32, sum_sq: u64)`** —
  fused single-pass sum and sum-of-squares over a `w × h` u8 block.
  `sum = Σ src[i]`, `sum_sq = Σ src[i]²`.

### Why this primitive matters

The encoder-side activity metric.  Block variance
(`sum_sq - sum²/(w*h)`) drives:
- HEVC adaptive quantization (`vbaq`) — higher-variance blocks get a
  coarser quantizer.
- VP9/AV1 partition-cost and mode decisions.
- PSNR / MSE measurement.

Fusing both reductions into one SIMD loop means the encoder scans each
block once instead of twice — this op sits in the per-CTU/superblock
analysis pass, called for every block at every partition depth.

### Backends

- scalar — reference double loop.
- SSE2 — `_mm_sad_epu8(v, zero)` for the byte sum (the dedicated psadbw
  path, whole-block u64 accumulation is safe at 128×128×255), widen
  u8→i16 + `_mm_madd_epi16(v, v)` for Σv² (drained per row to u64).
  16-byte chunk, 8-byte tail, scalar tail.
- AVX2 — `_mm256_sad_epu8` + `_mm256_madd_epi16` at 32 bytes/iter,
  falling through to the SSE2 16/8-byte paths for remainders.
- AArch64 NEON — `vpadalq_u16(acc, vpaddlq_u8(v))` for the sum,
  `vmlal_s16` / `vmlal_high_s16` widening MAC of `v²` for Σv².
- wasm `simd128` — `u16x8`/`u32x4` `extadd_pairwise` cascade for the
  sum, `i32x4_extmul_*_i16x8(v, v)` for Σv².  16 bytes/iter + scalar
  tail.

### Test coverage

- 16 block sizes (13 HEVC standard + 3 non-power-of-2) for scalar.
- 8 sizes for detected-vs-scalar.
- Boundary: all-zero → `(0, 0)`; all-255 16×16 → exactly
  `(65280, 16646400)`; constant-block zero-variance identity
  (`sum_sq == sum²/N`); mixed 0/255 lanes at odd widths to stress the
  widen/madd square path and the scalar tail.

### Notes

- The `sum` accumulator is kept whole-block (psadbw into u64 lanes is
  exact up to ~4.18M).  The `sum_sq` i32 accumulator is drained to u64
  once per row — a single row of 128 px tops out near 2.1M per i32
  lane, well inside `i32::MAX`, so the per-row drain keeps any block
  height safe.
- ROADMAP renumbered: `variance_block_u8` is v0.2.5 (was tentatively
  v0.2.4 “deferred”); v0.2.4 shipped `mc_avg_u8` instead.  Status
  column in ROADMAP.md brought in line with what actually shipped
  through v0.2.x.

## [0.2.4] — 2026-06-29

### Added

- **`mc_avg_u8(a, a_stride, b, b_stride, dst, dst_stride, w, h)`** —
  rounding halving add of two u8 blocks.  Per pixel:
  `dst[i] = ((a[i] + b[i] + 1) >> 1) as u8`.

### Why this primitive matters

The primary B-frame bi-prediction primitive in every video codec since
H.261.  After forward and backward motion compensation produce two
reference blocks, this fn averages them into the bipred output.
Single-instruction on every supported ISA.

### Backends

Single-instruction across all five:
- x86 SSE2 — `_mm_avg_epu8` (16 bytes/iter).  Falls through SSE4.1.
- x86 AVX2 — `_mm256_avg_epu8` (32 bytes/iter).
- AArch64 NEON — `vrhaddq_u8` (16 bytes/iter).
- wasm `simd128` — `u8x16_avgr_u` (16 bytes/iter).
- scalar reference.

### Tests

- Standard block sizes (4×4 to 64×64 + non-power-of-2).
- Rounding-semantics edge case: `avg(255, 0) = 128` (round-to-nearest,
  ties away from zero — matches `pavgb` semantics).

### Process note

This release also adds `NEXT_SESSION.md` — a self-contained briefing
for the next session/agent picking up codec-simd.  Documents the
operating procedure (10-step per-release checklist), records the
5 ISA-specific SIMD bugs caught by boundary tests last session
(rationale for why boundary tests are non-optional), and lists the
next concrete tasks (v0.2.5 variance, v0.3.0 mc_8tap_h, consumer-repo
notifications).

## [0.2.3] — 2026-06-29

### Added

- **`sse_block_u8(a, a_stride, b, b_stride, w, h) -> u64`** — sum of
  squared errors across a `w × h` u8 block.  Per-pixel:
  `(a - b) * (a - b)`.  Returns `u64`.

### Why this primitive matters

The rate-distortion cost function in every encoder.  Used in:
- HEVC encoder Lagrangian RDO: `RD = SSE + λ·R`
- VP9/AV1 partition selection
- Quality measurement (PSNR computation per block)

Strictly larger than SAD as a quality measure: penalizes outliers
quadratically.  Used at the inner-most level of rate-distortion
optimization which iterates over many candidates per block.

### Backends

- scalar — reference double loop.
- SSE2 — `_mm_madd_epi16(d, d)` does i16² pair-sums into i32 lanes,
  the canonical SSE2 idiom for sum-of-squares.  16 bytes/row chunk,
  8-byte tail, scalar tail.  i32 accumulator drained to u64 per row.
- AVX2 — `_mm256_madd_epi16` for 32-byte chunks; falls through to
  SSE2 16/8-byte paths for remainder.
- AArch64 NEON — `vmlal_s16(acc, d, d)` widening multiply-accumulate
  directly produces `acc + d²` in one instruction.  Particularly
  clean on NEON.
- wasm `simd128` — `i32x4_extmul_low/high_i16x8(d, d)` does widening
  i16² to i32, accumulated in i32x4.

### Test coverage

- 16 block sizes (13 HEVC standard + 3 non-power-of-2) for scalar.
- 8 block sizes for detected-vs-scalar.
- Zero-diff returns 0.
- Max-diff (0 vs 255 over 16×16) returns exactly `16²·255²`.

### Notes

- i32 accumulator drained per row to u64 — safe for blocks up to
  128×128 (the realistic codec maximum).  Larger blocks would
  overflow the per-row i32 but per-row drain keeps the total safe.
- The `madd_epi16` instruction is essentially built for this primitive
  — it was designed specifically for sum-of-products in the original
  SSE2 spec (1999).

## [0.2.2] — 2026-06-29

### Added

- NEON impl of `satd_block_4x4_u8` — 4 i16 rows in `int16x4_t`
  D-registers; butterflies via `vadd_s16`/`vsub_s16`; in-register
  transpose via `vtrn` at widths 16/32; abs via `vabs_s16` (input
  range `[-4080, 4080]` is safe from saturation); reduction via
  `vpaddlq_s16` widening pairwise add + `vaddvq_s32`.
- wasm `simd128` impl of `satd_block_4x4_u8` — 4 rows × 4 i16 in low
  half of `v128` (upper half zero throughout); shuffle-based 4×4
  transpose using compile-time constant lane indices via
  `i16x8_shuffle` / `i32x4_shuffle` / `i64x2_shuffle`; `i16x8_abs`
  + `i32x4_extadd_pairwise_i16x8` reduction.

### Why

v0.2.1 shipped SATD 4×4 with scalar fallback on NEON and wasm — this
release closes that gap.  Now all 5 backends have a real SIMD impl
for SATD 4×4.

Cross-compile verified via `cargo check --target aarch64-unknown-linux-gnu`
and `cargo check --target wasm32-unknown-unknown` (both clean).  Existing
cross-tier tests cover the new impls when run on those targets; CI
matrix expansion to actually execute on aarch64/wasm32 is roadmapped.

### Process note: cross-compile in the discipline

Up through v0.2.1, NEON and wasm impls had been written but never
verified to compile (cfg-gated to non-x86 targets).  v0.2.2 introduces
the rule that every release must `cargo check --target` clean on all
supported ISA targets, not just the host.  Added 2 cross-compile
targets (`aarch64-unknown-linux-gnu`, `wasm32-unknown-unknown`) to the
local toolchain for this purpose.

## [0.2.1] — 2026-06-28

### Added

- **`satd_block_4x4_u8(a, a_stride, b, b_stride) -> u32`** — Hadamard-
  transformed sum of absolute differences on a 4×4 u8 block.  Computes
  `sum |H × (a - b) × H'|` followed by `(sum + 1) >> 1` (HEVC/ffmpeg
  `satd_4x4_8_c` convention).

### Why this primitive matters

SATD is the standard alternative to SAD when transform-domain quality
matters more than pure pixel diff — it captures spatial frequency
information that pure SAD misses.  Used in:
- HEVC encoder rate-distortion cost (kvz-rs `cost::satd_4x4`)
- H.264 encoder mode decision
- VP9/AV1 RDO at sub-block partition selection

The 4×4 version is the smallest and most-called variant; 8×8 SATD
will arrive in a later release (it's a meaningfully different
algorithm — won't naively fall through to 4× the 4×4 cost).

### Backends

- scalar — reference: 2 stages of horizontal Hadamard, transpose
  implicitly via index permutation, 2 stages of vertical, abs+sum.
- SSE2 — vector-parallel: 4 rows live in 4 `__m128i` with valid
  data in the low 4 lanes; stages A+B (vertical Hadamard) are 8 SIMD
  add/sub; in-register 4×4 transpose via `unpack_epi16/epi32` +
  `shuffle_epi32`; stages A+B again for horizontal Hadamard; abs
  via `max(v, sat_neg(v))`; sum reduce via two-stage horizontal add.
- AVX2 — inherits SSE2 (no 256-bit advantage at 4×4 size).
- AArch64 NEON — falls back to scalar in v0.2.1.  NEON impl is
  feasible and roadmapped for a follow-up; the algorithm is similar
  to SSE2 but with NEON-flavored shuffles.
- wasm `simd128` — falls back to scalar in v0.2.1.  Same scoping.

### Test coverage

- 25 random `(seed_a, seed_b)` pairs for scalar-vs-naive
- 49 pairs for detected-vs-scalar (sweeps stride and seed combinations)
- Zero-diff identity case

### Notes

- 4×4 SATD is the smallest block size; SIMD throughput advantage is
  modest (~2-3× over scalar on x86) because the block barely
  saturates one SIMD register.  The real wins come at 8×8 / 16×16
  / 32×32 SATD where the SIMD pipeline stays full for many cycles.
- The `(sum + 1) >> 1` normalization is the ffmpeg/HEVC convention;
  some encoders skip it.  Caller's responsibility to multiply by 2
  if the unnormalized cost is wanted.

## [0.2.0] — 2026-06-28

**First Tier-2 milestone.**  Introduces block-shaped kernels —
primitives whose signature involves block dimensions `(w, h)` and
byte strides rather than just a flat lane count.  These compose
multiple Tier-1 primitives to deliver codec-shaped operations.

### Added

- **`sad_block_u8(a: &[u8], a_stride: isize, b: &[u8], b_stride: isize,
  w: usize, h: usize) -> u32`** — sum of absolute differences across
  a `w × h` u8 block, returning the total cost.  The classic motion-
  estimation cost function used in every hexagon/diamond/EPZS search.

- Backends:
  - scalar — reference double loop.
  - SSE2 — `_mm_sad_epu8` (the dedicated psadbw instruction) on
    16-byte chunks, 8-byte tail, scalar tail.  Accumulator is
    `__m128i` of two u64 partial sums, horizontally added at the end.
  - AVX2 — `_mm256_sad_epu8` on 32-byte chunks, falling through to
    SSE2 16/8-byte paths for remainders.  Accumulator is `__m256i`
    of four u64 partial sums.
  - AArch64 NEON — `vabdq_u8` + `vpadalq_u8` to accumulate into
    `uint16x8_t`, horizontally summed via `vaddlvq_u16` at the end.
    8-byte tail via `vabd_u8` + `vpaddl_u8`.
  - wasm `simd128` — no native SAD; emulated via
    `u8x16_max - u8x16_min` (per-byte abs diff) then
    `u16x8_extadd_pairwise_u8x16` (widen-and-pairwise-add to u16)
    then `u32x4_extadd_pairwise_u16x8` (to u32), accumulated, then
    horizontal sum.

- Four cross-tier tests:
  - All 13 HEVC block sizes (4×4 to 64×64) with mismatched stride
  - Non-power-of-2 widths (12, 24, 40) exercising tail paths
  - Zero-diff returns 0
  - Max-diff (all 0 vs all 255) returns w*h*255

### Why this primitive matters

kvz-rs encoder's motion estimation calls SAD thousands of times per
block (hexagon search visits ~50 candidates per refinement; this fn
is called for each).  Even a 2× speedup over scalar shows up as a
double-digit % improvement in whole-encode wall clock.

`_mm_sad_epu8` (psadbw) was introduced in SSE2 for exactly this use
case — it's literally one instruction per 16-byte row.  At AVX2 width
(32 bytes/instruction), modern Intel/AMD CPUs hit ~16 bytes per cycle
for SAD, which is enough that the encoder is no longer dominated by
this op.

NEON has no single-instruction SAD, but the abdiff+padal pair is
half-throughput per byte and still vastly faster than scalar.

wasm has no SAD primitive at all; the 3-instruction emulation is
~5× slower than x86/NEON dedicated paths but still fundamentally
SIMD-parallel.

### Architectural notes

- **Block-shaped primitives** (Tier-2) have a different API shape:
  `(a, a_stride, b, b_stride, w, h)` rather than `(a, b, dst, n)`.
  This is the standard codec DSP signature shape (FFmpeg, libvpx,
  openh264 all use it).
- Strides are `isize` (signed) to support reverse-scan and odd
  pitch arrangements — codecs sometimes need to mirror across a
  predicted MV.
- Caller is responsible for ensuring slice lengths cover the block
  geometry; we do a minimal bounds assert in the public wrapper.
- All cross-tier tests use `stride > w` to exercise the stride
  parameter (vs. tightly-packed input).

## [0.1.9] — 2026-06-28

### Added

- **`round_shift_narrow_i32_to_i16(src: &[i32], dst: &mut [i16], shift: u32)`** —
  combined "add rounding bias, arithmetic right shift, saturating narrow
  to i16".  Per lane:
  `dst[i] = saturating((src[i] + (1 << (shift-1))) >> shift) as i16`.
  `shift` is in `1..=31`.

### Why this primitive matters

The standard final stage of every DCT/IDCT and every fixed-point
filter that produces i16-range coefficients from an i32 accumulator.
Typical shifts: 6 (4×4 IDCT final), 12 (8×8 IDCT), variable per
codec.  Fuses three operations into one — saves two buffer passes.

### Backends and the wasm limitation

- scalar — reference.
- SSE2 — `_mm_add_epi32` + `_mm_sra_epi32` (runtime shift via
  register) + `_mm_packs_epi32` (signed saturating narrow).
  Natively saturates.  8 lanes/iter.
- AVX2 — same shape at 256-bit, plus the intra-lane
  `_mm256_permute4x64_epi64::<0b1101_1000>` fixup.  16 lanes/iter.
- AArch64 NEON — `vshlq_s32(_, neg_shift)` for variable arithmetic
  right shift + `vqmovn_s32` saturating narrow.  8 lanes/iter.
- wasm `simd128` — **scalar fallback only.**  wasm SIMD has no
  runtime-variable shift on `i32x4`; the `i32x4_shr<N>` intrinsic
  takes a compile-time-constant `N`.  A const-generic sibling
  `round_shift_narrow_i32_to_i16::<const SHIFT: u32>` that gets full
  SIMD on wasm is roadmapped for v0.2.x.

Three cross-tier tests including saturation stress with i32::MIN/MAX.

## [0.1.x] — Tier-1 milestone complete

With v0.1.9, the Tier-1 primitive surface envisioned in `ROADMAP.md`
is complete:

| # | Primitive | Shipped |
|---|---|---|
| 1 | `sat_pack_i16_to_u8` | v0.1.0 |
| 2 | `sat_pack_i32_to_u16(max)` | v0.1.1 |
| 3 | `add_clip_to_pixel_u16` | v0.1.2 |
| 4 | `clamp_i32_to_i16` | v0.1.3 |
| 5 | `abs_diff_u8` | v0.1.4 |
| 6 | `mac_widen_i16_to_i32` | v0.1.5 |
| 7-8 | `transpose_4x4_i16` + `transpose_8x8_i16` | v0.1.6 |
| 9-11 | `sat_add_i16` + `sat_sub_i16` + `abs_i16` | v0.1.7 |
| 12 | `mul_hi_round_i16` | v0.1.8 |
| 13 | `round_shift_narrow_i32_to_i16` | v0.1.9 |

**13 primitives.  5 backends each.  35 cross-tier tests.**  Every
SIMD impl bit-exact against the scalar reference on tier sweep
`CODEC_SIMD_FORCE_LEVEL=0/3/5/6/8/9`.

Next: v0.2.x — block-shaped Tier-2 kernels (SAD/SATD/SSE for
encoder hot paths).

## [0.1.8] — 2026-06-28

### Added

- **`mul_hi_round_i16(a: &[i16], b: &[i16], dst: &mut [i16])`** —
  Q15 fixed-point multiply with rounding.  Per lane:
  `dst[i] = saturating(((a[i] as i32 * b[i] as i32) + (1 << 14)) >> 15)`.

### Why this primitive matters

The fundamental operation of every fixed-point separable filter:
MC tap × sample, polyphase resampling stages, IIR coefficient
applications, DCT twiddle multiplies.  Combined with `mac_widen_i16`
this covers ~all integer DSP arithmetic patterns.

### Notes (the i16::MIN × i16::MIN saturation trap)

The "obvious" SIMD instructions for this op have **different
overflow semantics across ISAs**:

| ISA | Instruction | i16::MIN × i16::MIN behaviour |
|---|---|---|
| x86 SSSE3+ | `_mm_mulhrs_epi16` | wraps → returns 0x8000 (i16::MIN) |
| x86 AVX2 | `_mm256_mulhrs_epi16` | wraps → returns 0x8000 |
| AArch64 NEON | `vqrdmulhq_s16` | saturates → returns 0x7FFF (i16::MAX) |
| wasm | `i16x8_q15mulr_sat` | saturates → returns 0x7FFF |
| scalar | manual clamp | saturates → returns 0x7FFF |

To unify under the saturating semantic, x86 implementations apply an
**XOR fixup** after mulhrs: a mask of `(a == MIN) AND (b == MIN)` is
computed (= 0xFFFF in affected lanes, 0 elsewhere), then XOR'd into
the result.  This flips 0x8000 → 0x7FFF only where needed, no-op
elsewhere — adds 4 cheap SSE2/AVX2 instructions to the inner loop.

SSE2 fallback uses a 5-instruction emulation (`pmullw` + `pmulhw` +
`unpack` + `srai` + `packs_epi32`) which **naturally saturates** via
the final signed-pack instruction — no fixup needed.

This was caught by a `boundary_min_squared` test asserting the
expected i16::MAX output across all backends.  Without it the bug
would have shipped and shown up as half-precision drift in MC filter
output at extreme inputs.

## [0.1.7] — 2026-06-28

### Added

- **`sat_add_i16(a: &[i16], b: &[i16], dst: &mut [i16])`** —
  saturating addition of i16 lanes.  `paddsw` / `vqaddq_s16` /
  `i16x8_add_sat`, 8-16 lanes/iter.
- **`sat_sub_i16(a: &[i16], b: &[i16], dst: &mut [i16])`** —
  saturating subtraction.  `psubsw` / `vqsubq_s16` / `i16x8_sub_sat`.
- **`abs_i16(src: &[i16], dst: &mut [i16])`** — absolute value with
  saturating `abs(i16::MIN) = i16::MAX`.

### Notes

The naive `_mm256_abs_epi16` / `i16x8_abs` / `vqabsq_s16` instructions
have **different semantics** for the `i16::MIN` input:
- x86 `_mm256_abs_epi16(0x8000)` returns `0x8000` (unsigned-bit-pattern
  view — does NOT saturate).
- wasm `i16x8_abs(0x8000)` returns `0x8000` (same — does NOT saturate).
- NEON `vqabsq_s16(0x8000)` returns `0x7FFF` (saturating Q-variant).

To match the scalar reference (which yields `i16::MAX`), x86 and wasm
emulate via `max(v, sat_negate(0, v))` — saturating-negate produces
`i16::MAX` for `i16::MIN` input, then signed-max picks the saturated
positive.  This is also why SSE2 uses the same trick (no SSSE3
`pabsw` available at SSE2 tier).

Caught by a deliberate boundary-value mix in the test data
(`i16::MIN` / `i16::MAX` / `-1` interspersed every 5-7 lanes).
Without that the test would have passed and the bug shipped.

### Why these primitives matter

Deblock filter inner loops compute deltas like `tc = sat_sub(p1, q1)`
and `delta = sat_add(p0, sat_sub(q0, p0))`.  abs feeds into strength
tests like `abs(p0 - q0) < α`.  These three together cover almost the
entire deblock-filter inner loop arithmetic.

## [0.1.6] — 2026-06-28

### Added

- **`transpose_4x4_i16(src: &[i16], dst: &mut [i16])`** — row-major
  4×4 transpose of an `i16` block.  Used as the cross-over between
  vertical and horizontal stages of every 2D inverse transform
  (DCT/IDCT/ADST/IADST 4×4 in H.264, VP9, AV1).
- **`transpose_8x8_i16(src: &[i16], dst: &mut [i16])`** — row-major
  8×8 transpose of an `i16` block.  Classic 3-stage in-register
  transpose.  Used in 8×8 DCT/IDCT in every codec since H.264, and in
  8×8 SATD inside encoders.

Both shipped together because every 2D-transform kernel needs both.
Caller chooses based on transform size.

- Backends:
  - scalar — naive double loop.
  - SSE2 — 4×4 via `unpacklo_epi16` + `unpacklo_epi32` (4 stages, 4
    stores).  8×8 via classic 3-stage `unpack_epi{16,32,64}` shape
    (16 unpack instructions, 8 stores).
  - AVX2 — falls through to SSE2.  4×4 and 8×8 in-register transposes
    don't benefit meaningfully from 256-bit width.
  - AArch64 NEON — `vtrn1/vtrn2` at widths 16/32/64.  3 stages.
  - wasm `simd128` — no dedicated transpose intrinsics; uses
    `i16x8_shuffle` / `i32x4_shuffle` / `i64x2_shuffle` with const
    lane indices in the same 3-stage shape.

### Why these primitives matter

Every 2D transform (every codec) does:
  1. Inverse transform vertically (column-by-column)
  2. **Transpose** — switch row/column roles
  3. Inverse transform horizontally (same kernel, transposed input)
  4. Transpose back, store

The transpose has to be fast or it dominates the wall-clock at small
block sizes.  Both 4×4 and 8×8 are in-register: zero memory traffic
beyond load/store, and the inner shuffles are constant-time.

A double-transpose-equals-identity property test verifies the
permutation is correct (each backend's lane order matches scalar).

### Notes

- 4 cross-tier tests per primitive (scalar-vs-naive, detected-vs-scalar,
  double-transpose-identity) — 6 tests total for v0.1.6.
- The transpose primitives use a different signature shape from prior
  Tier-1 primitives — block-fixed (no `n` length parameter; size implied
  by the function name).  This is the introduction of Tier-1.5
  "mini-block" primitives that bridge into Tier-2 kernels.

## [0.1.5] — 2026-06-28

### Added

- **`mac_widen_i16_to_i32(a: &[i16], b: &[i16], acc: &mut [i32])`** —
  lane-wise widening multiply-accumulate.  Per lane:
  `acc[i] = acc[i].wrapping_add(a[i] as i32 * b[i] as i32)`.  The basis
  of every separable MC convolution (8-tap luma, 4-tap chroma) and
  every DCT inner product.
- Backends:
  - scalar — reference.
  - SSE4.1 — `cvtepi16_epi32` widen + `pmulld` (i32×i32) + add (8 lanes).
    SSE2 falls back to scalar — `pmulld` needs SSE4.1.
  - AVX2 — same shape with 256-bit (16 lanes/iter).
  - AArch64 NEON — `vmlal_s16` + `vmlal_high_s16`: **single instruction
    per half** does widening MAC.  8 lanes/iter in 2 instructions for
    the MAC step.  This is why NEON is fast at codec inner products.
  - wasm `simd128` — `extend_low/high_i16x8` + `i32x4_mul` + `i32x4_add`
    (8 lanes/iter).
- Three cross-tier tests including boundary stress
  (`i16::MIN × i16::MIN`, `i16::MIN × i16::MAX`).

### Why this primitive matters

This is the dominant inner-loop operation of every separable filter
and every transform butterfly stage.  An 8-tap luma MC kernel issues
`mac_widen` 8 times per output sample × tens of millions of samples per
second.  Same shape for DCT/IDCT inner products at 4/8/16/32 block
sizes.

Once Tier-2 kernels arrive in v0.3.x (MC) and v0.4.x (IDCT), they will
all consume this primitive as their fundamental building block.

NEON note: `vmlal_s16` does the widen+mul+add in **one instruction**.
This is the load-bearing reason NEON-equipped Apple Silicon Macs match
hand-tuned AVX2 desktop performance on video decode — the codec inner
product is half the work on NEON.

## [0.1.4] — 2026-06-28

### Added

- **`abs_diff_u8(a: &[u8], b: &[u8], dst: &mut [u8])`** — lane-wise
  absolute difference between two `u8` buffers.  Per lane:
  `dst[i] = (a[i] as i32 - b[i] as i32).unsigned_abs() as u8`.
  Always fits in `u8`.
- Backends:
  - scalar — branchless `max(a,b) - min(a,b)`.
  - SSE2 — `_mm_max_epu8` + `_mm_min_epu8` + `_mm_sub_epi8` (16 lanes/iter).
    First primitive at SSE2 tier beyond `sat_pack_i16_to_u8`.
  - AVX2 — same pattern with 256-bit (32 lanes/iter).
  - AArch64 NEON — single instruction `vabdq_u8` (16 lanes/iter).
  - wasm `simd128` — `u8x16_max` + `u8x16_min` + `u8x16_sub` (16 lanes/iter).
- Three cross-tier tests including boundary stress (0/255 pairs).

### Why this primitive matters

The building block of every SAD/SATD/SSE family that will land in v0.2.x.
Standalone uses: image diff visualisation, simple motion-detection
heuristics, pre-pass cost estimates.

x86 has dedicated `psadbw` (sum of absolute differences) that fuses the
per-lane `|a-b|` with the horizontal-sum reduction.  Our `abs_diff_u8`
emits only the per-lane part — the horizontal-sum primitive will arrive
as `sad_row_u8` in v0.2.0 specifically using `psadbw` for the fused path.

## [0.1.3] — 2026-06-28

### Added

- **`clamp_i32_to_i16(buf: &mut [i32])`** — in-place clamp of every `i32`
  lane into the `i16` range `[i16::MIN, i16::MAX]`.  Output stays as
  `i32` (the value just fits in i16).  Used at the apex of dequant —
  apv-rs `quant.rs:63`, HEVC RExt extended-precision dequant, any codec
  whose i32 accumulator must feed an i16-input inverse transform.
- Backends:
  - scalar — reference impl.
  - SSE4.1 — `pmaxsd` + `pminsd` (4 lanes/iter).
  - AVX2 — `_mm256_max_epi32` + `_mm256_min_epi32` (8 lanes/iter).
  - AArch64 NEON — `vmaxq_s32` + `vminq_s32` (4 lanes/iter).
  - wasm `simd128` — `i32x4_max` + `i32x4_min` (4 lanes/iter).
- Three cross-tier tests (scalar-vs-naive, detected-vs-scalar, boundary
  values around i16::MIN±1 / i16::MAX±1).

### Why this primitive matters

apv-rs (and HEVC RExt) accumulate dequantized coefficients into `i32`,
but the inverse transform takes `i16` inputs.  Without this primitive
the codec writes a manual scalar clamp loop in the hot path.
In-place to save a buffer pass — caller already has the i32 buffer
ready to feed into IDCT.


## [0.1.2] — 2026-06-28

### Added

- **`add_clip_to_pixel_u16(pred: &[u16], res: &[i16], dst: &mut [u16],
  max: u16)`** — fused residual-add then clip.  Per lane:
  `dst[i] = clamp(pred[i] as i32 + res[i] as i32, 0, max) as u16`.
  This is the high-bit-depth equivalent of FFmpeg's `add_residual`
  widened for 8/10/12-bit support in a single unified path.
- Backends:
  - scalar reference.
  - SSE4.1 — `cvtepu16_epi32` + `cvtepi16_epi32` widen, `pmaxsd` +
    `pminsd` clamp, `packusdw` narrow (8 lanes/iter).
  - AVX2 — uses `_mm256_castsi256_si128` + `_mm256_extracti128_si256`
    to split 256-bit into 128-bit halves, then `_mm256_cvtepX16_epi32`
    to widen each, add+clamp, `packus_epi32` + `permute4x64_epi64`
    (16 lanes/iter).
  - AArch64 NEON — `vmovl_u16`/`vmovl_high_u16` zero-extend pred,
    `vmovl_s16`/`vmovl_high_s16` sign-extend res, `vaddq_s32` +
    `vmaxq_s32` + `vminq_s32` + `vqmovun_s32` + `vcombine_u16`
    (8 lanes/iter).
  - wasm `simd128` — `i32x4_extend_low/high_u16x8` zero-extend pred,
    `i32x4_extend_low/high_i16x8` sign-extend res, `i32x4_add` + min/max
    + `u16x8_narrow_i32x4` (8 lanes/iter).
- Three cross-tier tests for the new primitive: scalar-vs-naive,
  detected-vs-scalar, boundary-values (pred near max, res near
  i16::MIN/MAX).

### Why this primitive matters

ffmpeg-rs h265 integration was blocked on this primitive specifically
(see `crates/av-codec-h265/transform.rs::add_residual` —
`dst: &mut [u16], stride, res: &[i16], size, bit_depth: u8`).  Without
a fused primitive, callers would have to materialise an `i32`
intermediate buffer, call `sat_pack_i32_to_u16` on it — wasting one
full buffer pass at every residual-add point.  Fusing saves a buffer
pass per 4×4 / 8×8 / 16×16 / 32×32 block, on every transform unit, on
every frame.

This is the most-called primitive at decode time in any high-bit-depth
codec.

### Notes

- No breaking changes vs 0.1.1.
- `Dispatch` grew one field; append-only ordering preserved.
- Like `sat_pack_i32_to_u16`, this primitive requires SSE4.1+ on x86
  (uses `packusdw`).  Pre-SSE4.1 CPUs run the scalar fallback for
  just this primitive.

## [0.1.1] — 2026-06-28

### Added

- **`sat_pack_i32_to_u16(src: &[i32], dst: &mut [u16], max: u16)`** —
  the high-bit-depth sibling of `sat_pack_i16_to_u8`.  Clamps each
  `i32` lane to `[0, max]` and narrows to `u16`.  `max = (1 <<
  bit_depth) - 1` at the call site.  Implemented across all 5 backends:
  - scalar — reference impl.
  - SSE4.1 — `pmaxsd` + `pminsd` + `packusdw` (8 lanes/iter).  NEW
    backend file `src/prims_sse41.rs`.  SSE2 falls through to scalar
    for this primitive because `packusdw` requires SSE4.1.
  - AVX2 — `_mm256_max_epi32` + `_mm256_min_epi32` +
    `_mm256_packus_epi32` + `_mm256_permute4x64_epi64` (16 lanes/iter).
  - AArch64 NEON — `vmaxq_s32` + `vminq_s32` + `vqmovun_s32` +
    `vcombine_u16` (8 lanes/iter).
  - wasm `simd128` — `i32x4_max` + `i32x4_min` + `u16x8_narrow_i32x4`
    (8 lanes/iter).
- New cross-tier test set in `tests/cross_tier.rs`:
  - `sat_pack_i32_scalar_matches_naive_reference`
  - `sat_pack_i32_detected_dispatch_matches_scalar`
  - `sat_pack_i32_boundary_values_match_scalar`
- `CHANGELOG.md` (this file).
- `ROADMAP.md` — complete plan from v0.1.x to v1.0.

### Why this primitive matters

The first integration sweep (six codec ports — h264-rs / apv-rs /
kvz-rs / ffmpeg-rs / vp9-rust / jph-rs) revealed that v0.1.0's only
primitive (`sat_pack_i16_to_u8`) **only fits 8-bit-locked decoders**.
Every modern pure-Rust port has been widened to `u16` sample planes +
`i32` residual intermediates for 8/9/10/12/14-bit support from day
one — so the actual pixel-clip primitive they need is `i32 → u16`
with parametric `max`.

### Notes

- No breaking changes vs 0.1.0.
- `Dispatch` struct grew one field; field order is append-only.
- SSE2 dispatch does NOT install `sat_pack_i32_to_u16` — uses scalar.

## [0.1.0] — 2026-06-28

### Added

- Initial skeleton crate.
- One worked primitive: `sat_pack_i16_to_u8`.
- Backends: scalar / SSE2 / AVX2 / AArch64 NEON / wasm32 simd128.
- Tier-dispatch via `Dispatch::detect()` with `OnceLock` caching.
- Env-var overrides `CODEC_SIMD_FORCE_LEVEL` / `CODEC_SIMD_MAX_LEVEL`.
- `tests/cross_tier.rs` enforces SIMD == scalar bit-equality.
- Dual GPL-3.0-or-later / commercial licensing model.
