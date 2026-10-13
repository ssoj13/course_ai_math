# NEXT_SESSION.md — instructions for the next session/agent

## Where you are

`codec-simd` **v0.3.20** (private, GitHub: `ssoj13/codec-simd-rs`).

Current state — authoritative summary is **CLAUDE.md §Status**; roadmap is
**CODECS.md**; exact constants are **CONTRACTS.md**.  In short:

- **40 primitives** (46 public wrappers), **107 cross-tier tests**, bit-exact
  green at tiers 0/3/5/6/8/9; `aarch64` + `wasm32` cross-compile clean.
- **Layout modularized** (2026-07): `api/{family}.rs` + `prims_{isa}/{family}.rs`;
  `lib.rs` is just attrs + re-exports.  Feature-gating (Phase 3) deferred.
- **Consumer survey done** (see CODECS.md §Consumer reality): ffmpeg-rs is the
  strongest consumer; kvazaar-rs + heif-rs (incl. new AV1) also strong; apv-rs
  uses `wide`; jph-rs is a donor (own AVX-512 SIMD); jph-wgpu-rs is GPU (out).
- **Chosen next target: ffmpeg-rs H.264 + HEVC.**  Next primitives:
  `idct4x4_h264_add_u8` (`z2=(s1>>1)-s3`, `>>6`) + `idct8x8`, then HEVC
  IDCT 4/8/16/32 + DST-VII 4×4 (shift 7 / 20−bd), then deblock + SAO.
  Write the integration guide in THIS repo (e.g. `docs/integration/ffmpeg-rs.md`).
- **35 git tags** (v0.1.0 → v0.3.19; v0.3.20 tagged this session).

### What is DONE (do not redo)

Tier-1 (13) · Tier-2 encoder cost (sad/satd4x4/sse/variance/sad_x4) ·
MC: 1D u8 8/6/4/2-tap h+v, u8-intermediate 8-tap 2D, full
i16-intermediate 2D (8-tap HEVC/AV1 luma, 4-tap HEVC chroma, 6-tap
H.264 centre `j`) · dequant_block_i16 · weighted_avg_u8 · lincomb3_clip_u8.

### What REMAINS — needs codec-specific DESIGN, do NOT guess

These require a focused session with per-kernel design + verification
(the user's rule: нигде не гадать).  Each has codec-specific constants
or algorithms that must be confirmed, not invented:

- **Inverse transforms (v0.4)** — IDCT 4/8/16/32 have codec-specific
  butterfly constants (H.264 4×4 ≠ HEVC ≠ VP9/AV1).  CODEC-AGNOSTIC
  members that CAN be done without guessing: `hadamard_4x4`/`hadamard_8x8`
  (universal ±1 Walsh butterfly — the scalar ref already exists inside
  `satd_block_4x4_u8`; the SIMD needs the transpose lane-ordering done
  carefully, see the transpose bug note below) and `iwht_4x4` (H.264
  lossless, also a fixed butterfly).  The DCT/ADST families need the
  codec's exact constants + shift schedule.
- **Intra prediction (v0.5)** — DC/V/H/planar are simple; the 33-angle
  directional modes (HEVC) / TM (VP9) need the exact reference-sample
  projection.  Pick a codec.
- **Loop filters (v0.6)** — deblock strength decisions + SAO are
  intricate and codec-specific.
- **Colour (v0.7)** — the plane kernel `lincomb3_clip_u8` is shipped; an
  interleaved RGBA output + chroma-upsampling (420/422) wrapper remains
  (interleave shuffles; design the output layout first).
- **Quant (v0.9)** — `dequant_block_i16` shipped; encoder `quant` with
  sign-hiding needs care (i16::MIN abs edge + 32-bit multiply on SSE2).
- **u16 high-bit-depth** siblings of everything (the i32 intermediate is
  already wide enough; mostly u16 load/store + a `max` clip parameter).

Non-guessing additions that are still mechanical: none obvious remain.

## First action — read these in order

1. `CLAUDE.md` (this repo) — architecture rules + worked-example pointer.
2. `ROADMAP.md` — full v0.1.x → v1.0 plan.  Section "v0.2.x" tells you what
   primitives are still missing in this milestone.
3. `CHANGELOG.md` — every shipped version has its WHY documented (which
   caller asked, which ISA-specific bugs were caught, what the next
   primitive should be).
4. `src/prims_sse2.rs` and `src/prims_neon.rs` — read 2-3 functions to
   absorb the pattern.

## Per-version operating procedure (don't skip steps)

1. **Pick a primitive** from ROADMAP.md `v0.2.x` table that's still
   `📋 planned`.  Don't add primitives without an identified caller.
2. **Scalar reference first** in `src/prims_scalar.rs`.  Keep it
   obvious; never optimise.
3. **Per-backend impls** in `src/prims_{sse2,sse41,avx2,neon,wasm}.rs`.
   If an ISA doesn't have native support, dispatch can fall through to
   a lower tier (see how SSE2 falls back to scalar for
   `sat_pack_i32_to_u16` because `packusdw` needs SSE4.1).
4. **Wire `Dispatch`** in `src/dispatch.rs` — append field, install at
   correct tier, default in `scalar()`.  Append-only field order.
5. **Public safe API** in `src/lib.rs` — slice-based, asserts length
   contracts, dispatches.  No `unsafe` in the public signature.
6. **Cross-tier tests** in `tests/cross_tier.rs`.  Always include:
   - scalar-vs-naive reference test
   - detected-dispatch-vs-scalar test (sweep multiple N / sizes)
   - **boundary-value test** — this is non-optional, see below
7. **Bump `Cargo.toml`** patch version.
8. **`cargo test --release`** — green.
9. **Tier sweep**:
   ```bash
   for L in 0 3 5 6 8 9; do
     CODEC_SIMD_FORCE_LEVEL=$L cargo test --release --test cross_tier
   done
   ```
10. **Cross-compile sanity**:
    ```bash
    cargo check --target aarch64-unknown-linux-gnu --lib
    cargo check --target wasm32-unknown-unknown --lib
    ```
    Both must finish clean.  Without this, NEON/wasm impls go untested.
11. **CHANGELOG entry** under the new `[x.y.z]` heading.  Describe
    WHAT and WHY (which caller asked).  Document any ISA-specific
    semantic differences caught by boundary tests.
12. **Commit** with `feat:` prefix, **tag** `vX.Y.Z`, **push**:
    ```bash
    git tag vX.Y.Z -m "vX.Y.Z — <one-line summary>"
    git push origin main --tags
    ```

## Why boundary tests are non-optional

**5 SIMD bugs were caught by boundary tests during the last session.**
Each would have shipped silently into consumer codecs as bit-drift if
the tests had only used "random" inputs.  Specifically:

| Bug | Caught by |
|---|---|
| AVX2 `_mm256_abs_epi16(i16::MIN)` returns `0x8000`, not saturating | `abs_i16_boundary` (mixed `i16::MIN`/`i16::MAX` lanes) |
| wasm `i16x8_abs` same non-saturating bug | same test |
| x86 `_mm_mulhrs_epi16(MIN, MIN)` returns 0x8000 (not saturating); NEON/wasm do saturate | `mul_hi_round_boundary_min_squared` |
| Scalar `>> 15` arithmetic shift rounding mode (Rust signed-shift semantics) edge case | initial mac_widen test failure traced to wrapping vs saturating contract |
| SATD 4×4 transpose lane ordering | `transpose_4x4_double_yields_identity` |

Always include explicit `i16::MIN`/`i16::MAX`/`-1`/`u8::MAX`/`0` values
mixed into your test inputs.  Don't trust random-only.

## Next concrete tasks (pick one)

Ordered by impact:

### A) v0.2.5 — `variance_block_u8` ✅ SHIPPED

Returns `(sum: u32, sum_sq: u64)` over the block.  SSE2 psadbw + madd,
AVX2 256-bit, NEON vpadal + vmlal, wasm extadd_pairwise + extmul.
Done; see CHANGELOG `[0.2.5]`.

### B) v0.3.0 — `mc_8tap_h_u8` ✅ SHIPPED

Horizontal 8-tap MC, the decoder hot path.  Signature
`mc_8tap_h_u8(dst, dst_stride, src, src_stride, w, h, filter: &[i16; 8])`;
per pixel `dst[c] = clip_u8((Σ src[c+i]*filter[i] + 64) >> 7)`.
**Edge convention:** `src` pre-offset by −3, `w + 7` readable bytes per
row (libvpx/dav1d).  All 5 backends form the `u8 × i16` product at full
i32 width → bit-exact for any tap.  NEON uses `vqrshrun_n_s32::<7>` for
round-shift+clamp in one op.  Done; see CHANGELOG `[0.3.0]`.

### v0.3.1 — `mc_8tap_v_u8` ✅ SHIPPED  (vertical sibling)
### v0.3.2 — `mc_8tap_2d_u8` ✅ SHIPPED  (u8-intermediate, libvpx convention;
thin `lib.rs` composition of h∘v, no new backends)

### v0.3.3 — `mc_4tap_h_u8` / `mc_4tap_v_u8` ✅ SHIPPED  (chroma 4-tap,
runtime `shift` so HEVC chroma `>>6` and sum-128 `>>7` both work).  The
runtime-shift finalize (`_mm*_sra_epi32` / `vshlq_s32`+`vqmovun_s32` /
`i32x4_shr`+dual-narrow) is reusable for any future runtime-shift kernel.

### v0.3.4 — `mc_6tap_h_u8` / `mc_6tap_v_u8` ✅ SHIPPED  (H.264 luma 6-tap;
1D half-pel is a single `>>5` shift, so it reuses the v0.3.3 runtime-shift
finalize with a 6-tap loop).

### v0.3.5 — `mc_bilinear_h_u8` / `mc_bilinear_v_u8` ✅ SHIPPED  (2-tap;
closes the 1D u8 MC family).

### i16-intermediate MC — IN PROGRESS (one brick per version)

Goal: codec-exact high-precision 2D MC (HEVC/AV1 luma, HEVC chroma,
H.264 centre half-pel) by keeping the first pass at i16 precision
instead of clipping to u8.  All bricks are codec-agnostic — the CALLER
supplies the per-pass shifts (HEVC luma 2D: `shift1=BD-8, shift2=20-BD`;
H.264 centre: `shift1=0, shift2=10`).  Plan:

- v0.3.6 `mc_8tap_h_u8_to_i16` ✅ SHIPPED — first pass, u8→i16.
- v0.3.7 `mc_8tap_v_i16_to_u8` ✅ SHIPPED — second pass, i16→u8, clip.
  (The two compose into HEVC-8bit 2D — proven by the
  `mc_8tap_2d_high_precision_via_bricks` integration test.)
- v0.3.8 `mc_8tap_2d_hp_u8` ✅ SHIPPED — high-precision 2D luma (i16
  intermediate), thin wrapper, caller picks `shift1`/`shift2`.
- v0.3.9 4-tap i16 bricks + `mc_4tap_2d_hp_u8` (HEVC chroma 2D) ✅ SHIPPED.
- v0.3.10 6-tap i16 bricks + `mc_6tap_2d_hp_u8` (H.264 centre `j`) ✅
  SHIPPED — the i16-intermediate MC family is COMPLETE.
- **NEXT options** (the MC family is done; pick a new direction):
  - u16 high-bit-depth MC (10/12-bit) — same kernels over u16; the
    intermediate is already i32-safe, so it mostly means u16 load/store
    + a u16 clip parameter.  Large (doubles the family).
  - **v0.4.0 inverse transforms** (`idct_4x4_add_u8`, …) — ~15% of
    decode; codec-specific (H.264 4×4 ≠ HEVC), so pick a codec or
    parameterize the butterfly constants.
  - v0.2.6 `sad_x4_block_u8` ✅ SHIPPED (v0.3.11).

### Remaining roadmap families (each large + codec-specific)

- **v0.4.x inverse transforms** (IDCT 4/8/16/32, IADST, IWHT, Hadamard) —
  ~15% of decode.  Codec-specific butterfly constants — parameterize or
  pick a codec.
- **v0.5.x intra prediction** (DC/V/H/planar/angular).
- **v0.6.x loop filters** (deblock, SAO).
- **v0.7.x colour conversion** (yuv→rgb, BT.601/709 — parameterize the
  3×3 fixed-point matrix; well-defined math, good clean target).
- **v0.9.x quantization** (quant/dequant block — mostly trivial SIMD).
- **u16 high-bit-depth** siblings of the MC/transform families.

All follow the same dispatch discipline: scalar ref first, 5 backends,
cross-tier test, CHANGELOG, tier sweep + cross-compile, tag+push.

  Note: the i16 v-kernel reads i16 directly (`vld1q_s16`/`loadu_si128`/
  `v128_load`); the product stays i16×i16→i32 so the same
  `mullo`+`mulhi` / `mullo_epi32` / `vmlal_s16` / `extmul` apply.

### Other branches (independent):

- **i16-intermediate MC** (the architectural gap; ~a milestone of work):
  add i16-OUTPUT variants of the h/v kernels (the horizontal/first pass
  does NOT clip to u8 — it keeps the i32 accumulator narrowed to i16 with
  a *smaller* intermediate shift), plus an i16-INPUT vertical/second pass
  that applies the larger final shift and clips to u8.  Design the
  intermediate precision/shift contract first (it differs per codec:
  HEVC luma 2D = h-pass `>> (BD-8)` then v-pass `>> (20-BD)`; H.264
  centre `j` = unrounded sums then `>> 10`).  This single mechanism
  unlocks: codec-exact 2D for HEVC/AV1, the deferred 4-tap chroma 2D,
  the H.264 centre half-pel, AND is the foundation for the u16
  high-bit-depth family.  **Surface the contract choice before coding.**
- **v0.2.6 `sad_x4_block_u8`** — 4 MV candidates in parallel; a clean
  encoder-side increment, independent of the MC family.
- **v0.4.0 `idct_4x4_add_u8`** — start the inverse-transform family
  (~15% of decode time).

### Possible cleanup (not urgent)

The five 1D MC kernels (8/6/4/2-tap × h/v) are near-identical — only the
tap count and the const-vs-runtime shift differ.  A macro that generates
the per-(taps, h|v, backend) bodies would dedup ~2000 lines.  Deferred
deliberately (don't refactor shipped+tagged code without a reason); note
it if touching this area broadly.

### MC family shape (reference)

Every 1D kernel: load taps as broadcast i16; full-width `u8 × i16`
product (SSE2 `mullo`+`mulhi`, AVX2 `mullo_epi32`, NEON `vmlal_s16`,
wasm `extmul`) into i32 lanes; finalize = add round bias, arithmetic
right shift (const `>>7` for 8-tap via `vqrshrun_n`; runtime via
`sra_epi32`/`vshlq_s32`+`vqmovun`/`i32x4_shr` for 2/4/6-tap), saturating
narrow to u8.  h-kernels shift within the row (`src[col+i]`), v-kernels
stride rows (`src[(row+i)*stride+col]`).

### Design note for the MC family

`mc_8tap_h_u8` and `mc_8tap_v_u8` are near-identical: the only
difference is the per-tap load address (in-row shift vs. row stride) and
the horizontal `w+7` vs. vertical `h+7` padding.  Both form the
`u8 × i16` product at FULL i32 width (`mullo`+`mulhi` on SSE2,
`mullo_epi32` on AVX2, `vmlal_s16` on NEON, `extmul` on wasm), so they
are bit-exact for any i16 tap — reuse this exact shape for the 4/6-tap
and bilinear kernels (just change the tap count and, for 6-tap, confirm
the codec's shift).

### D) v0.3.x bonus — Tier-1.5 `mc_avg_u8_8x8`, `mc_avg_u8_16x16`, etc.

Const-generic block-size specializations of v0.2.4 `mc_avg_u8`.
Saves the per-row loop overhead in the hot path.  Tiny perf win
(~5-10%) but trivial to write.  Low priority.

### E) Notify consumer codec repos that v0.1.x/v0.2.x is in

Each repo `ssoj13/{h264-rs,apv-rs,kvz-rs,ffmpeg-rs,jph-rs,vp9-rust}`
has a `simd-integration` branch with a `TODO_SIMD.md` describing what
they want from codec-simd.  Now that v0.2.4 is shipped, those repos
should be notified — either by appending a status section to their
TODO_SIMD.md (commit on `simd-integration` branch), or via GitHub
issues.  This is what unblocks consumer integration.

## Quick start for the next session

```bash
# Set context.
cd /home/joss/projects/_ports/codec-simd-rs
git log --oneline -8  # see what shipped
cat ROADMAP.md | grep -A1 "📋"  # see what's planned

# Verify state.
cargo test --release  # should report 49 passed
cargo check --target aarch64-unknown-linux-gnu --lib  # should be clean
cargo check --target wasm32-unknown-unknown --lib     # should be clean

# Pick a primitive, follow the per-version procedure above.
```

## Do NOT do

- Add a primitive whose only SIMD impl is scalar fallback in 4/5
  backends.  At minimum, write the SSE2 (or whichever ISA the caller
  is currently on) impl.
- Skip the boundary-value test.  Random inputs alone caught **0** of the
  5 SIMD bugs in the last session; boundary inputs caught **all 5**.
- Skip the cross-compile sanity step.  NEON or wasm typos compile
  fine on x86 but error on the target ISA.
- Change the field order in `Dispatch`.  Append-only.  Adding fields
  is a minor bump; reordering/removing is a major bump.
- Add `unsafe` to the public API in `src/lib.rs`.  All unsafe stays
  in `src/prims_*.rs`.
- Add `pulp` or `std::simd` dependencies.  Hand-tuned per-ISA is the
  entire point of this crate.

## Handoff

Read this file plus `CLAUDE.md` plus `ROADMAP.md` plus the last 2-3
CHANGELOG entries.  That's the full context.  Then pick task A, B, or
E above and execute.
