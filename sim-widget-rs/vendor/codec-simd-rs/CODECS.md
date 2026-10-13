# CODECS.md — consumer-codec integration plan for `codec-simd`

The sibling pure-Rust codec ports under `C:\projects\projects.rust.cg\cglibs\`
are the real consumers of this SIMD crate.  This is the plan to serve all
of them, derived from a source survey of each (exact constants/shifts in
**`CONTRACTS.md`**).

**Principle:** stay codec-agnostic where one parameterized primitive serves
many codecs (caller supplies the filter/matrix/scale/shift); add a distinct
primitive only where the *algorithm* differs.  Every primitive below has an
**identified caller** (the roadmap rule) — found by this survey.

---

## 1. The consumers (state)

| Crate | Codec | Role | Has own DSP? |
|---|---|---|---|
| `kvazaar-rs` | HEVC | encoder | yes — transforms, intra, deblock, SAD/SATD, quant |
| `ffmpeg-rs` | HEVC | decoder (pure Rust port of FFmpeg) | yes — IDCT, MC, add_residual, deblock, SAO |
| `h264-rs` | H.264 | decoder | yes — IDCT 4/8, DC transforms, 6-tap MC, deblock, weighted pred |
| `apv-rs` | APV | intra codec | yes — DCT-II, dequant (per-position matrix); **no MC** |
| `jph-rs` | HTJ2K | codec | yes — DWT 5/3 & 9/7, RCT/ICT colour, quant |
| `heif-rs` (AV1) | AV1/AVIF | decoder | yes — DCT/ADST/Identity, MC, deblock, loop-restore, film grain |
| `heif-rs` (HEVC) | HEVC | binding | **no** — delegates to `rust_h265` |

8/10/12-bit are real targets (ffmpeg-rs enforces `{8,9,10,12}`; APV/HTJ2K wider).

---

## 2. What codec-simd ALREADY covers (with now-confirmed callers)

| Primitive (shipped) | Confirmed caller(s) | Contract match |
|---|---|---|
| `sad_block_u8`, `sad_x4_block_u8` | kvazaar `cost::sad_nxn`, ME search | `Σ|a-b|` then `>>(bd-8)` (shift is caller's) ✅ |
| `satd_block_4x4_u8`, `hadamard_4x4_i16` | kvazaar `cost::hadamard_4x4` | 4×4 Hadamard + `(sum+1)>>1` ✅ |
| `satd_block_8x8_u8` | kvazaar `cost::satd_8x8_subblock` | 8×8 Hadamard + `(sum+2)>>2` ✅ (scalar+NEON; SSE2/AVX2/wasm scalar fallback) |
| `sse_block_u8` | kvazaar R-D | ✅ |
| `variance_block_u8` | adaptive quant | ✅ |
| `dequant_block_i16` | kvazaar **dequant** `(q·scale + (1<<(s-1)))>>s` ✅, APV dequant (uniform) | EXACT — caller passes `scale=INV_QUANT_SCALES[qp%6]<<(qp/6)`, `shift=20-14-tr_shift` ✅ |
| `add_clip_to_pixel_u16` | ffmpeg-rs HEVC **`add_residual`** `clip(pred+res,0,max)` | EXACT ✅ |
| `add_clip_to_pixel_u8` (v0.3.20) | ffmpeg-rs H.264 IDCT-store, HEVC `add_residual` (u8), ProRes, AV1 recon | EXACT `clip(pred+res,0,255)` ✅ |
| `mc_8tap_{h,v,2d}_u8`, `*_u8_to_i16`/`*_i16_to_u8`, `mc_8tap_2d_hp_u8` | ffmpeg-rs HEVC luma QPEL (8-tap, runtime shift); VP9/AV1 | filters `[-1,4,-10,58,17,-5,1,0]…` are caller-supplied; shifts `bd-8` / `6` / `14-bd` fit the runtime-shift API ✅ |
| `mc_4tap_{h,v}_u8` + `mc_4tap_2d_hp_u8` | ffmpeg-rs HEVC chroma EPEL `[-2,58,10,-2]…` | runtime shift ✅ |
| `mc_6tap_{h,v}_u8` + `mc_6tap_2d_hp_u8` | h264-rs luma `[1,-5,20,20,-5,1]`, `(.+16)>>5`, centre `(.+512)>>10` | `mc_6tap` shift=5; centre via `mc_6tap_2d_hp` shift1=0/shift2=10 ✅ |
| `mc_bilinear_{h,v}_u8` | h264-rs chroma (1D part); AVIF | ✅ |
| `mc_avg_u8` / `mc_avg_u16` | every bipred / 10-12-bit bipred; h264 qpel avg | `(a+b+1)>>1` ✅ |
| `weighted_avg_u8` / `weighted_avg_u16` | h264-rs explicit+implicit WP; HEVC WP | `(w0·a+w1·b+off)>>shift` clip — caller folds rounding ✅ |
| `lincomb3_clip_u8` | YUV→RGB output | BT.601/709 rows ✅ |
| `clamp_i32_to_i16`, `sat_pack_i16_to_u8`, `sat_pack_i32_to_u16`, `mac_widen…`, `transpose_4x4/8x8`, `sat_add/sub/abs_i16`, `mul_hi_round_i16`, `round_shift_narrow…` | transform/MC inner loops across all | building blocks ✅ |

**Conclusion:** the entire MC family, encoder cost functions, dequant,
weighted-pred and residual-add (u8 + u16) are already done and
contract-correct.

### Consumer reality (survey 2026-07-05 — the 6 sibling crates)

| Crate | Verdict |
|---|---|
| **ffmpeg-rs** | 🎯 strongest consumer.  H.264 + HEVC + ProRes DSP, **pure scalar, zero unsafe**.  H.264 lives in `crates/av-codec-h26x`, HEVC in `crates/av-codec-h265` — there is NO standalone `h264-rs`. |
| **kvazaar-rs** | 🎯 strong (HEVC encoder, scalar, `forbid unsafe`).  Wants SATD-8x8, DCT/DST, quant.  Risk: plans its own internal `kvz-simd` (M5), may not adopt. |
| **heif-rs** | 🎯 strong.  HEVC (vendored `rust_h265`, scalar) **plus a new AV1 decoder** (`heif-codec-av1`: dct/adst/identity 4..64, CDEF, loop-restoration) — a large new surface. |
| **apv-rs** | weak: already uses the safe portable-SIMD `wide` crate by default.  Only residual-add+clip-u16 is clearly ours. |
| **jph-rs** | **donor, not consumer** — 1:1 OpenJPH port with its OWN full per-ISA SIMD incl **AVX-512**.  We can lift its integer kernel math (RCT/DWT-5,3) into a `jph` family later, but it won't call us. |
| **jph-wgpu-rs** | **not a consumer** — GPU/WGSL; its CPU DSP is reference/legacy only.  Removed from the list. |

---

## 3. What REMAINS — now UNBLOCKED (exact constants known)

Ordered by value × number of callers.  All constants in `CONTRACTS.md`.

### Tier A — inverse transforms (decoder, ~15% of decode; 3 codecs)

1. **`idct4x4_add_u8` / `_u16`** — HEVC/APV DCT-II 4×4 *and* a separate
   **H.264 integer 4×4** (different butterfly).  Decision: **two primitives**
   (`idct4x4_dct_add` with caller-supplied core constants `64,83,36`, and
   `idct4x4_h264_add` — the shift-only integer transform `(32+res)>>6`),
   because the algorithms differ.  Both: 2D (vert shift1 → horiz shift2 →
   add pred → clip).  Callers: kvazaar, ffmpeg-rs, h264-rs, apv-rs.
2. **`idst4x4_add_u8`** — HEVC DST-VII (intra 4×4 luma) `[29,55,74,84]`.
   Callers: kvazaar, ffmpeg-rs.
3. **`idct8x8_add_u8`** — HEVC/APV DCT-II 8×8 (`TBL_TM8`/`KVZ_G_DCT_8`) and
   H.264 8×8 integer (separate).  Callers: kvazaar, ffmpeg-rs, h264-rs, apv-rs.
4. **`idct16x16` / `idct32x32`** — HEVC/APV (big, partial-butterfly).
5. **`hadamard_8x8_i16`** — 8×8 SATD (kvazaar `(sad+2)>>2`).  Constant-free,
   already half-built (4×4 done).  Caller: kvazaar 8×8/any-size SATD.
6. **`iwht4x4_add_u8`** — H.264 luma DC inverse Hadamard + dequant.
7. **`dc_only_idct`** — ffmpeg-rs DC fast path `((c0+1)>>1 + add) >> (14-bd)`.

Transform-core helper to add: a generic **`partial_butterfly`** isn't
worth it (each size/codec differs); implement per (size, codec) with the
verbatim constants from `CONTRACTS.md`, scalar + SSE2/AVX2 (host-tested),
NEON/wasm backfill — same discipline as `hadamard_4x4`.

### Tier B — loop filters (decoder, ~10%; HEVC + H.264)

8. **`deblock_luma_strong_u8` / `_weak_u8`** — HEVC (kvazaar/ffmpeg, same
   formulas) — TC/BETA tables are caller-supplied; the filter math is fixed.
9. **`deblock_h264_lt4 / _eq4`** — H.264 (different formulas + ALPHA/BETA/TC0).
10. **`sao_band_u8` / `sao_edge_u8`** — HEVC SAO (ffmpeg-rs).

### Tier C — colour / wavelet (HTJ2K + output)

11. **`rct_fwd_i32` / `rct_bwd_i32`** — SHIPPED v0.3.19.  HTJ2K reversible
    colour: fwd `Y=(R+2G+B)>>2, Cb=B-G, Cr=R-G`; bwd `G=Y-((Cb+Cr)>>2),
    R=Cr+G, B=Cb+G`.  ⚠️ NO live consumer — jph-rs already has its own
    SIMD (incl. AVX-512); formula matches (`gen_rct_forward/backward`) but it
    won't call us.  Kept as a correct building block.
12. **`dwt_53_horz_i32` / `_vert_i32`** — 5/3 reversible lifting
    `dst ∓= (b + s1 + s2) >> e` (step0 b=2,e=2; step1 b=1,e=1).  Caller: jph-rs.
13. **`dwt_97_horz_f32` / `_vert_f32`** — 9/7 irreversible (float lifting
    coeffs `0.4435, 0.8829, -0.0530, -1.5861`).  Caller: jph-rs.  (Float =
    new bit-exactness contract; gate behind the existing `dwt` feature.)
14. **`ict_f32`** (YUV↔RGB float, BT.601) — jph-rs ICT; complements the
    integer `lincomb3_clip_u8`.
15. **`yuv_to_rgb_interleaved`** — wrap `lincomb3` + an interleave for
    RGBA output (the display path).

### Tier D — high-bit-depth (u16) siblings  *(in progress)*

16. `mc_avg_u16` ✅, `weighted_avg_u16` ✅.  Remaining: `mc_8tap_*_u16`,
    `mc_4tap/6tap_*_u16`, `sad_block_u16`, `sse_block_u16`,
    `dequant_block_i16` already i16, `idct*_add_u16`.  Same kernels, u16
    load/store, `max` clip param.  Callers: 10/12-bit decode in every codec.

### Tier E — APV per-position dequant + AV1

17. **`dequant_block_matrix_i16`** — `coef[i]*q_matrix[i]` (per-position
    scaling matrix), the variant `dequant_block_i16` (uniform scale) does
    not cover.  Caller: apv-rs (and HEVC scaling lists).
18. **AV1 transforms** (`heif-rs` av1): DCT/ADST/Identity with the dav1d
    constants (`181,1567,3784,…`), i64 intermediate.  Large; lower priority
    until an AV1 consumer is prioritized.

---

## 4. Implementation order (next sessions)

- [x] **`rct_fwd/bwd_i32`** — DONE (v0.3.19).  Trivial, constant-free, HTJ2K
  caller; lossless round-trip verified; real SIMD on all backends.  First
  primitive shipped off the codec-survey contracts.
1. **`hadamard_8x8_i16`** — constant-free, caller (kvazaar 8×8 SATD), extends
   the v0.3.15/16 work.  Low risk.
2. **`idct4x4_h264_add_u8`** + **`idct4x4_dct_add_u8`** + **`idst4x4_add_u8`**
   — the 4×4 inverse-transform set (3 codecs).  Verbatim constants.
3. **`idct8x8_*`** (HEVC/APV/H.264).
4. **`deblock_*`** (HEVC then H.264).
5. **`dwt_53_*`**, then **`sao_*`**, then the u16 MC siblings, then 16/32 IDCT.
6. **`dwt_97_f32` / `ict_f32`** (float family, own contract).
7. AV1 transforms (when prioritized).

Each follows the established discipline: scalar ref (verbatim from the
codec) → SSE2/AVX2 (host-tested bit-exact) → NEON/wasm (backfill) →
cross-tier test → tier sweep → cross-compile → CHANGELOG → tag+push.

### Integration step (per codec, after primitives land)
Open a `simd-integration` branch in each consumer crate; replace the scalar
hot loop with the `codec-simd` call; verify bitstream-identical output at
all forced tiers (`CODEC_SIMD_FORCE_LEVEL`).  No consumer currently depends
on `codec-simd` (confirmed: zero references), so integration is greenfield.
