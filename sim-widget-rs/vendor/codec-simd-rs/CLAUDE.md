# CLAUDE.md — `codec-simd-rs`

## Where you are

Cross-arch SIMD primitives crate for software video codecs.  Tier-dispatched
across x86_64 (SSE2/SSSE3/AVX2/AVX-512), AArch64 (NEON), wasm32 (simd128).
Public API is **safe** (slice-based, length-asserted); all `unsafe` is
encapsulated in `src/prims_*.rs`.

## Status

**v0.3.35** — 77 append-only `Dispatch` fields (2 new), 193 cross-tier tests, bit-exact green at
forced tiers 0/3/5/6/9 on x86_64; `aarch64` + `wasm32` cross-compile clean.
v0.3.35 adds `prefix_sum_bias_u8_inplace` + `interleave_halves_u8` — the DECODE-side pair of
OpenEXR's ZIP/RLE first-difference byte predictor (`internal_zip.c`'s `reconstruct` +
`interleave`), requested by **exr-rs** (a Rust OpenEXR port), which had briefly hand-rolled its
own `unsafe` SSE4.1/SSE2 port of this exact algorithm directly in its codec crate before being
told to move it here instead, per this crate's "unsafe stays inside codec-simd" rule (exr-rs
measured the un-vectorized decode path at 52.6% of its ZIP codec's total decode time — more than
the zlib inflate itself). SSE4.1-tier (SSSE3 instructions, prefix-sum shift-and-add tree +
`pshufb` carry) / SSE2-tier (`unpacklo`/`unpackhi_epi8`) + NEON backends (`vextq_u8` prefix-sum,
`vzip1q_u8`/`vzip2q_u8` interleave), both 1:1 ports of OpenEXR's own C SIMD (the ENCODE-side
transform has no SIMD counterpart in OpenEXR either, and measured at only ~3-4% of total codec
time there, so it stays scalar-only, matching C exactly — no primitive added for it).

**v0.3.32** — 71 append-only `Dispatch` fields (3 new), 186 cross-tier tests, bit-exact green at
forced tiers 0/3/6/9 on x86_64; `aarch64` + `wasm32` cross-compile clean.
v0.3.32 adds the **HEVC in-loop deblocking-filter FILL kernels** on the `u16` plane (byte-exact port of
ffmpeg-rs `av-codec-h265/filter/deblock.rs` `luma_strong`/`luma_weak`/`chroma_weak`).
`hevc_deblock_luma_strong_u16(plane, base, xstride, ystride, tc, tc2, tc3, max)` = the 4-tap clipped
strong filter over 4 lines (delta-clip only, output proven ∈ `[0, max]` → no-clip `u16` store; `max`
unused, kept for symmetry); `hevc_deblock_luma_weak_u16(..., tc, nd_p, nd_q, max)` = the weak filter
with the **per-line** `|delta0| < 10*tc` gate (failing line keeps its bytes) + optional `p1`/`q1` side
filters (uniform `nd_p`/`nd_q > 1`); `hevc_deblock_chroma_weak_u16(..., size, tc, max)` = the single-tap
chroma delta, branchless. **Plane+stride ownership, ONE kernel per orientation** — the segment origin
`base` + the two orientation strides (vertical edge `xstride=1`/`ystride=stride`, horizontal
`xstride=stride`/`ystride=1`) exactly mirror the scalar reference; the vertical/horizontal transpose is
folded into the SIMD gather/scatter. **Hard invariant: i32 accumulation** — every tap sum accumulates in
i32 lanes (one lane per line; the strong intermediate `2*p3+3*p2+p1+p0+q0+4` reaches ~32764 at 12-bit,
within 3 of `i16::MAX`), the `u16` narrow is the only width reduction at the store. Scalar + **SSE2**
(i32×4, deblock coeffs via shifts+adds, signed min/max emulated with `cmpgt`+select) + **AVX2** (same
i32×4 with native `min/max_epi32`) — the kernel is 4-lane by nature (4 luma lines) so AVX2 adds no
parallelism, only native min/max. The weak per-line gate is a per-lane mask; failing lanes blend back to
their original bytes (unconditional scatter stays exact) and `p1`/`q1` are skipped unless `nd_p`/`nd_q >
1`; all reads are snapshotted before any store. NEON/wasm scalar-fallthrough (x86 SIMD modules
`#[cfg]`-gated). Safe wrappers assert the exact access window + strides `>= 1` (G3 bounded loads).
Independent naive golden (reimplements the deblock.rs math), both orientations (swapped strides) × bd
8/10/12 × tc/nd extremes × the gate boundary + gate-pass∧`nd_p==1` / gate-fail discriminators + a padded
plane pad-untouched assert + a size=4 Miri-ready bounds case. **NOTE: ffmpeg-rs integration deferred** —
deblock is memory-movement-bound, so per plan `_md/plan_simd_deblock.md` §7 the integration is
measure-gated (≥3% wall win) and these primitives may remain reusable-only, like the intra-angular ones.
v0.3.31 adds the **HEVC intra-prediction FILL kernels** on the `u16` plane (byte-exact port of
ffmpeg-rs `av-codec-h265/pred.rs` `pred_planar`/`pred_angular`). `hevc_intra_planar_u16(dst,
dst_stride, top, left, log2_size)` = the planar `POS(x,y)` weighted average; `hevc_intra_angular_u16(dst,
dst_stride, ref_buf, bias, size, angle, vertical)` = the angular 2-tap FIR `((32-fact)·ref[k+1] +
fact·ref[k+2] + 16)>>5` with `idx`/`fact` computed internally from `angle`. The CALLER builds `ref_buf`
(the C `ref` array incl. the `INV_ANGLE` back-projection) + passes `bias` (`ref_buf[k+bias] == C ref[k]`,
FULL slice so negative `k` stay in-bounds); the mode-26/10 boundary filter + `clip_pixel` are the
caller's, post-call. **Hard invariant: i32 accumulation** — both the planar sum and the angular MAC
accumulate in i32 (the numerators overflow i16 at 10/12-bit: planar ~262112, angular 126945); the `u16`
narrow is the only width reduction, at the store (output ∈ `[0, 2^bd)`, no clip). Horizontal family
(`mode<18`) reuses the vertical contiguous-row kernel into a `size×size` scratch then transpose-stores
(pure data move). Scalar + **SSE2** (`mullo`/`mulhi`/`unpack` i32 MAC, `packs_epi32` narrow) + **AVX2**
(native `mullo_epi32`, `packus_epi32`); both do 8-wide interior + scalar tail so a fixed load never
over-reads the caller-sized buffers (`size=4` runs entirely scalar — G3 bounded-load guard). NEON/wasm
scalar-fallthrough (x86 SIMD modules `#[cfg]`-gated). Independent naive golden (reimplements the pred.rs
math), sizes 4/8/16/32 × bd 8/10/12 × all angular modes 2..=34 × random/saturated/alt-0-max inputs,
`dst_stride = size+8` with pad columns asserted untouched, synthetic `fact ∈ {1,31}` + negative-`idx`
extremes (`hevc_intra_*`). Miri G3 check pending (miri component not installed on the build host; bounds
are structurally guaranteed by the 8-wide-only-when-`col+8<=size` chunking + the `ref_buf` length assert).
v0.3.30 adds the **HEVC SAO (Sample Adaptive Offset) band + edge filter** on the `u16` picture plane
(byte-exact port of ffmpeg-rs `av-codec-h265/filter/sao.rs` `sao_band`/`sao_edge` → the C
`h2656_sao_template.c`). `sao_band_u16(...)` = per-pixel `clip(src + offset_table[(src>>band_shift)&31],
0, max)` (caller-built 32-entry band LUT, `band_shift = bit_depth-5`; `src==dst` in-place safe).
`sao_edge_u16(...)` = per-pixel `cat = 2 + CMP(p,n0) + CMP(p,n1)` over the two `eo_class` neighbours
(`CMP=(a>b)-(a<b)`), `clip(src + offsets[edge_idx[cat]], 0, max)`, `edge_idx=[1,2,0,3,4]`; a pixel whose
neighbour is outside the `w×h` region keeps its value (the picture-boundary `sao_edge_restore_0` rule),
and `src` must not alias `dst` (snapshot semantics — the kernel reads neighbours). **Band is scalar on
every tier** (its 32-entry per-pixel LUT gather is not beaten by a SIMD scalar-gather; kept correct +
dispatched per CAVEMAN). **Edge gets SSE2 + AVX2 `i16`-lane backends**: HEVC samples are `≤12-bit`
(`≤4095`) so the whole classify+clip runs in `i16` lanes (8 / 16 per step) — signed `cmpgt_epi16` for
`CMP`, `cmpeq`+select for the category→offset LUT, `add`/`max_epi16(0)`/`min_epi16(max)` for the clip;
the interior rectangle is SIMD, boundary rows/columns + the column tail scalar. NEON/wasm
scalar-fallthrough (the SSE2/AVX2 sao modules are `#[cfg]`-gated to x86). Independent naive golden
(reimplements the sao.rs per-pixel math), bd 8/10, all 4 `eo_class`, saturation extremes, tiny-region
picture-boundary sweep (`hevc_sao_*`). Also this rev: the **HEVC inverse-transform API wrappers now
`debug_assert!(bit_depth ∈ 8..=12)`** (mirroring ffmpeg-rs `idct.rs`), and the transform cross-tier
tests sweep `bd=9` too (`HEVC_BDS=[8,9,10,12]`).
v0.3.29 completed the **HEVC inverse-transform family (Phase-4 Wave-B, sizes 8/16/32)** —
`hevc_idct8_i16_inplace` / `hevc_idct16_i16_inplace` / `hevc_idct32_i16_inplace(coeffs, col_limit,
bit_depth)`, the `idct_nxn` 8/16/32 paths (`tr_8/16/32_core` DCT partial even/odd butterfly + the
col/row passes; 32×32 basis matrix ported verbatim from `dsp.c`). Two-pass SCALE
`clip_int16((r+add)>>shift)` (col `>>7`, row `>>(20-bd)`), i32 accumulators (TR_32 sum ≈94M ≪ i32).
**`col_limit` odd-coeff pruning is FULLY replicated (option (a), byte-exact for ANY input):** the
scalar reference prunes inline (col `limit2=min(col_limit+4,H)` shrinking every 4th column, row
`limit=min(col_limit,H)`, tr_32 even at `end/2`); the SIMD backends **pre-zero** the pruned sources
(each source routes to one even/odd leaf, so zeroing == skipping) then run the FULL uniform butterfly.
**SIMD strategy: vectorize the partial butterfly ACROSS columns** (SSE2 4-col i32 via mullo/mulhi
widen of the i16 sources, AVX2 8-col native cvtepi16_epi32/mullo_epi32/min_max_epi32); the row pass is
the column butterfly on the TRANSPOSE of pass-1 (separable identity). NEON/wasm scalar-fallthrough
(the prune helpers are `#[cfg]`-gated to x86). Independent naive golden (reference tr_8/16/32_core +
idct_nxn with pruning, own basis-matrix copy) + ffmpeg-rs KAT vectors, bd 8/10/12, full-range i16
(fully-populated AND pruned col_limit) incl. saturation extremes.
v0.3.28 opened the family at **size 4** — `hevc_idct4_i16_inplace` (`idct_nxn` size-4 DCT-4 even/odd,
64/83/36), `hevc_dst4_i16_inplace` (`transform_4x4_luma` DST-VII, 29/55/74), and the size-generic
`hevc_idct_dc_i16_inplace` (`idct_NxN_dc`, sizes 4/8/16/32, `>>(14-bd)`). All operate **in-place on an
i16 coeff block** (add-to-plane is the separate `add_clip_to_pixel_*` step). Byte-exact port of
ffmpeg-rs `av-codec-h265/transform/idct.rs`.
v0.3.27 adds the **HEVC u16-source MC tap-filter** family — the u16 siblings of the i32-source luma
8-tap / chroma 4-tap filters, for the ffmpeg-rs MC path that gathers reference pixels as **u16**
(2 B) instead of i32 (4 B) so the SIMD load packs **2× the lanes/register**:
`mc_8tap_h_u16_to_i32` / `mc_8tap_v_u16_to_i32` / `mc_4tap_h_u16_to_i32` / `mc_4tap_v_u16_to_i32`
+ the hv 2D wrappers `mc_8tap_2d_hp_u16_to_i32` / `mc_4tap_2d_hp_u16_to_i32` (which reuse the
**SHARED** `mc_*tap_v_i16_to_i32` `>> 6` 2nd pass, not duplicated). Widen `u16 → i32` then the SAME
`Σ f[k]·src[k] >> shift`, RAW i32 (NO narrow/saturation/rounding) — **byte-identical to each
i32-source twin** given the same pixel values (proven by an equivalence test u16-vs-i32 at every
tier). Scalar + **SSE2** (direct 8×i16 load + `mullo/mulhi`, SSE2-tier parity with the i32 family)
+ **AVX2** (`cvtepu16_epi32` + `mullo_epi32`); NEON/wasm scalar-fallthrough.
v0.3.26 adds the **HEVC MC "final" clip/combine** family — the pixel-producing tail SHARED by luma
and chroma (element-wise over the i32 intermediate, tap-count independent):
`mc_uni_clip_i32_to_u16` (`clip((v+offset)>>shift)`), `mc_uni_w_clip_i32_to_u16`
(`clip(((v*wx+offset)>>shift)+ox)`), `mc_bi_clip_i32_to_u16` (`clip((v+src2+offset)>>shift)`, src2
i16), `mc_bi_w_clip_i32_to_u16` (`clip((v*wx1+src2*wx0+bias)>>rshift)`, src2 i16), and
`mc_store_i32_to_i16` (**WRAPPING** `as i16`, the bi src2 store — NOT saturating). Byte-exact port
of ffmpeg-rs `av-codec-h265/mc.rs` `put_luma_uni`/`_uni_w`/`_bi`/`_bi_w`/`put_luma`; `clip(x) =
x.clamp(0, max)`, `max=(1<<bd)-1`; all i32 math (widest product ~1.0e7 ≪ i32::MAX). Scalar +
**SSE4.1 + AVX2** (i32→u16 clamp+narrow uses `packus_epi32`/`mullo_epi32`/`min_epu16`, which are
SSE4.1 — same tier as the crate's `add_clip_to_pixel_u16`/`weighted_avg_u16`; SSE2/NEON/wasm
scalar-fallthrough). Tests sweep bd 8/10/12 × HEVC weight/offset/denom ranges × out-of-i16 `v`,
incl. a directed wrapping-store test past i16::MAX.

**v0.3.23** — 46 primitives (3 new append-only `Dispatch` fields), 127 cross-tier tests,
bit-exact green at forced tiers 0/3/5/6/8/9 on x86_64; `aarch64` + `wasm32` cross-compile clean.
v0.3.23 adds the **HEVC chroma 4-tap MC tap-filter** family
(`mc_4tap_h_i32_to_i32` / `mc_4tap_v_i32_to_i32` / `mc_4tap_v_i16_to_i32` + the hv 2D wrapper
`mc_4tap_2d_hp_i32_to_i32`, const `MC_CHROMA_EXTRA = 3`): the 4-tap sibling of the luma family,
i32 pixel source → **RAW i32** intermediate, PURE `>> shift` (NO rounding offset, NO narrow, NO
saturation), byte-exact port of ffmpeg-rs `chroma_h_sample`/`chroma_filter_tmp`/`fill_chroma_htmp`
(`EPEL_FILTERS`). Chroma htmp ⊆ `[-2560, 18939]` (i16, truncation-narrowed byte-exact); the hv 2nd
pass is RAW i32 (adversarial test drives it to 43006 > i16::MAX). Scalar + SSE2 + AVX2 host-tested;
NEON/wasm scalar-fallthrough.
v0.3.22 adds the **HEVC luma 8-tap MC tap-filter** family
(`mc_8tap_h_i32_to_i32` / `mc_8tap_v_i32_to_i32` / `mc_8tap_v_i16_to_i32` + the hv 2D wrapper
`mc_8tap_2d_hp_i32_to_i32`, consts `MC_MAX_PB_SIZE`/`MC_LUMA_EXTRA`): i32 pixel source → **RAW
i32** intermediate, PURE `>> shift` (NO rounding offset, **NO narrow, NO saturation** — the
wrapping-i16 `put` store and uni/bi `clip_pixel` are separate downstream steps; the hv 2nd pass
can exceed i16, e.g. 33271), byte-exact port of ffmpeg-rs `av-codec-h265/mc.rs`. The 2D wrapper's
i16 `htmp` is truncation-narrowed (in-range `[-6142, 22522]`, byte-exact). Scalar + SSE2 + AVX2
host-tested (incl. a directed out-of-i16 adversarial test); NEON/wasm scalar-fallthrough (SIMD
backfill deferred). v0.3.21 added safe in-place wrappers `add_clip_to_pixel_u16_inplace`/`_u8_inplace`
(`pix` aliased as both pred and dst) over the existing element-local add-clip kernels.
Shipped: full MC family (u8 + u16 bipred), encoder cost fns
(SAD/SATD/SSE/variance/sad_x4), dequant, weighted-pred, residual-add
(`add_clip_to_pixel_u8`/`_u16`), colour (`lincomb3_clip_u8`, HTJ2K
`rct_fwd/bwd_i32`), `hadamard_4x4_i16`.

**Layout modularized (2026-07).**  Public API split into
`src/api/{convert,arith,cost,mc,colour}.rs`; per-ISA kernels split into
`src/prims_{isa}/{family}.rs`.  `lib.rs` is now just attrs + `mod`/`pub use`.
Feature flags `mc/transform/lpf/dwt` are declared but NOT yet gated (Phase 3,
deferred as likely over-engineering for a &lt;4s-compile crate).

**Roadmap** lives in `CODECS.md` (consumer survey + integration plan) and
`CONTRACTS.md` (verbatim per-codec constants).  Next target: ffmpeg-rs
H.264+HEVC — `idct4x4_h264_add_u8`, HEVC IDCT/DST set, deblock, SAO.

## Read order

1. **`CAVEMAN.md`** — how to add a new primitive.  Canonical reference.
2. `src/dispatch.rs` — function-pointer table model.
3. `src/api/{family}.rs` — safe public wrappers (convert/arith/cost/mc/colour).
4. `src/prims_avx2/{family}.rs` — the SIMD pattern in code.
5. `tests/cross_tier.rs` — every primitive needs a cross-tier test before it
   ships.
6. `CODECS.md` / `CONTRACTS.md` — consumer survey, roadmap, exact constants.

## Architecture rules (do not break)

- **`unsafe` stays inside the crate.**  Public API takes slices, asserts
  sizes, then calls `unsafe`.  Consumers never write `unsafe`.
- **One file per (family × ISA):** realized as `src/prims_{isa}/{family}.rs`
  (e.g. `prims_avx2/mc.rs`).  Each `prims_{isa}/mod.rs` re-exports its family
  submodules via `pub(crate) use`, so `crate::prims_{isa}::<fn>` paths (used
  by `dispatch.rs`) stay stable.  Public wrappers live in `src/api/{family}.rs`.
- **Scalar is always present.**  Every primitive has a scalar reference;
  `Dispatch::scalar()` is a complete usable impl.
- **Bit-exact across tiers.**  Integer-only.  Floats come later as a
  separate family with their own bit-exactness contract.
- **No `target_feature` in public signatures.**  Backend `#[target_feature]`
  fns are `pub(crate)`; the dispatch pointer hides them.

## Most common next task

Adding a new primitive (e.g. `mac_widen_i16_to_i32`):

1. Add field to `Dispatch` (`src/dispatch.rs`).
2. Add scalar impl (`src/prims_scalar/{family}.rs`) — always first, it's the
   correctness ground truth.
3. Add per-backend impls (`src/prims_{sse2,sse41,avx2,neon,wasm}/{family}.rs`).
4. Wire `Dispatch::detect()` to install the right pointer per tier.
5. Add safe public wrapper in `src/api/{family}.rs` (re-exported at crate
   root via `pub use api::*`).
6. Add cross-tier test in `tests/cross_tier.rs`.  **Without this, do not
   ship.**
7. `cargo test --release` then sweep: `for L in 0 3 5 6 8 9; do
   CODEC_SIMD_FORCE_LEVEL=$L cargo test --release --test cross_tier; done`;
   then cross-compile `aarch64-unknown-linux-gnu` + `wasm32-unknown-unknown`.

## Licensing

**BSD-3-Clause** (permissive; see `LICENSE`).  Relicensed from the former dual
GPL-3.0-or-later/commercial model on 2026-07-05 (maintainer decision) so the
sibling consumers — notably **ffmpeg-rs (LGPL-2.1-or-later)** — can link it with
no copyleft contamination.  `LICENSE-COMMERCIAL.md` is now superseded (legacy).

## Don't do

- Don't add audio / float DSP into v0.1.  Different bit-exactness model.
- Don't introduce `pulp` / `std::simd` dependencies.  This crate is
  hand-tuned per-ISA; that's the entire point.
- Don't relax `cargo test --release` requirement.  Conformance is the
  whole product.
