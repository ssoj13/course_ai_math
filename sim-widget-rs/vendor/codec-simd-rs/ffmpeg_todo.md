# ffmpeg-rs TODO for codec-simd-rs

Date: 2026-07-05
Consumer: `C:\projects\projects.rust.cg\cglibs\ffmpeg-rs`

This file is the `codec-simd-rs` side of the ffmpeg-rs speed plan. The immediate ffmpeg-rs problem is slow 4K HEVC decode/export:

- `IMG_9436.MOV`: HEVC Main 10, 3840x2160, `yuv420p10le`, 1926 frames.
- `aaa.MP4`: HEVC Main, 3840x2160, `yuv420p`, 10169 frames.

Existing u8 HEVC MC primitives help the 8-bit drone file. They do not cover the Main10 hot path, because ffmpeg-rs stores HEVC pixels as `u16` and passes runtime `bit_depth`.

## P0 - High-bit-depth HEVC MC

Needed for `ffmpeg-rs/crates/av-codec-h265/mc.rs`.

Current ffmpeg-rs contract:

- luma qpel filters: 8 taps, `QPEL_FILTERS[4][8]`.
- chroma epel filters: 4 taps, `EPEL_FILTERS[8][4]`.
- source pixels are `u16`, valid bit depths are 8/10/12.
- intermediate is signed 14-bit precision in `i16`.
- no-filter put intermediate: `src << (14 - bit_depth)`.
- h/v put intermediate: `sum(filter * src) >> (bit_depth - 8)`.
- hv put intermediate: horizontal pass `>> (bit_depth - 8)` into i16 temp, vertical pass `>> 6`.
- uni final: `clip((intermediate + round) >> (14 - bit_depth), 0..(1<<bit_depth)-1)`.
- bi final: `clip((intermediate + src2 + round) >> (15 - bit_depth), 0..max)`.
- weighted final follows ffmpeg-rs `put_luma_uni_w`, `put_luma_bi_w`, `put_chroma_uni_w`, `put_chroma_bi_w` math.

Preferred primitives, low-level and composable:

1. `mc_8tap_h_u16_to_i16`
   - luma horizontal qpel first pass.
   - `src: &[u16]`, `dst: &mut [i16]`, strides in elements.
   - params: `w`, `h`, `filter: &[i16; 8]`, `shift = bit_depth - 8`.

2. `mc_8tap_v_u16_to_i16`
   - luma vertical-only qpel first/final intermediate from `u16` source.
   - same output contract as ffmpeg-rs `luma_intermediate` for `pass.v && !pass.h`.

3. `mc_8tap_v_i16_to_i16`
   - luma hv second pass from horizontal i16 temp to i16 intermediate.
   - `shift = 6` for HEVC hv.

4. `mc_8tap_2d_hp_u16_to_i16`
   - convenience wrapper around 1 + 3.
   - Important: allow caller-provided scratch, or provide a no-allocation pair API. ffmpeg-rs already needs to reuse scratch per CTU/slice.

5. `mc_4tap_h_u16_to_i16`, `mc_4tap_v_u16_to_i16`, `mc_4tap_v_i16_to_i16`, `mc_4tap_2d_hp_u16_to_i16`
   - chroma epel siblings with 4 taps and the same precision model.

6. final clip/combine helpers for `i16` intermediate to `u16` pixels:
   - `mc_intermediate_to_u16`
   - `mc_intermediate_bi_to_u16`
   - `mc_intermediate_weighted_to_u16`
   - `mc_intermediate_bi_weighted_to_u16`

The final helpers are useful because ffmpeg-rs has modes that compute an intermediate from one list, then combine with another list's `src2` intermediate. If the API stays lower-level, make sure ffmpeg-rs can still avoid per-block allocation and keep the final scalar fallback small.

## P1 - HEVC 8-bit MC integration polish

Existing primitives look close for `aaa.MP4`:

- `mc_8tap_h_u8_to_i16`
- `mc_8tap_v_i16_to_u8`
- `mc_8tap_2d_hp_u8`
- `mc_4tap_h_u8_to_i16`
- `mc_4tap_v_i16_to_u8`
- `mc_4tap_2d_hp_u8`
- `weighted_avg_u8`

Before ffmpeg-rs can use them cleanly, confirm these match ffmpeg-rs/FFmpeg HEVC semantics exactly:

- HEVC luma 2D shifts: `shift1 = bit_depth - 8`, `shift2 = 20 - bit_depth`; for 8-bit, `0` and `12`.
- HEVC chroma 2D shifts: same precision model with 4 taps.
- edge convention: luma source pre-offset by `-3*stride - 3`; chroma by `-1*stride - 1`.
- output stride in elements, not bytes.
- no hidden allocation in the hot path, or a separate API with caller scratch.

If missing, add explicit `*_with_scratch` wrappers for 2D MC so ffmpeg-rs can reuse CTU/slice scratch.

## P2 - Residual add and reconstruction helpers

`ffmpeg-rs/crates/av-codec-h265/transform.rs:add_residual` is a low-risk first integration point.

Already useful:

- `add_clip_to_pixel_u16`
- `add_clip_to_pixel_u8`

Needed checks or wrappers:

1. Confirm `add_clip_to_pixel_u16` handles row-strided destination and contiguous residual rows without extra copies.
2. If not, add `add_clip_to_pixel_u16_strided(dst, dst_stride, residual, residual_stride, w, h, max)`.
3. Add tests using HEVC residual ranges for bit depths 8/10/12.

## P3 - HEVC inverse transforms

Needed after MC/residual basics, because ffmpeg-rs still runs scalar transform code.

Add in codec-simd-rs, not in ffmpeg-rs:

- HEVC inverse transform add for 4x4, 8x8, 16x16, 32x32.
- HEVC 4x4 DST-VII path where required.
- u16 destination variants for 8/10/12-bit.
- add-and-clip form preferred, because ffmpeg-rs reconstructs directly into picture planes.

Suggested API shape:

- `hevc_idct4_add_u16(dst, dst_stride, coeffs, bit_depth)`
- `hevc_idct8_add_u16(...)`
- `hevc_idct16_add_u16(...)`
- `hevc_idct32_add_u16(...)`
- `hevc_dst4_add_u16(...)`

Keep scalar reference first, then SIMD backends.

## P4 - HEVC deblock and SAO

Needed for later large speedups. ffmpeg-rs currently runs deblock and SAO as whole-picture scalar passes after reconstruction.

Targets:

- HEVC luma/chroma deblock edge filters for u16 bit-depth-generic planes.
- SAO band offset and edge offset kernels for u16 planes.
- APIs should be row/edge/block oriented so ffmpeg-rs can later parallelize rows/tiles safely.

Do this after MC and transform. Deblock/SAO has more boundary/state risk.

## P5 - Output colour conversion helpers

ffmpeg-rs JPEG/RGB export spends significant time rebuilding swscale context and converting/copying RGB. codec-simd-rs already has `lincomb3_clip_u8`, but ffmpeg-rs needs full wrappers eventually.

Useful wrappers:

- `yuv420p8_to_rgb24`
- `yuv420p8_to_rgba8`
- `yuv420p10le_to_rgb24`
- `yuv420p10le_to_rgba8`
- `yuv420p10le_to_rgb48le` if lossless-ish 16-bit RGB export remains important.

These must take explicit matrix/range parameters. Do not bake in BT.601/709/2020 defaults inside the primitive.

## Verification Requirements

Every primitive needed by ffmpeg-rs should ship with:

1. scalar implementation first;
2. safe public wrapper with slice length assertions;
3. cross-tier tests in `tests/cross_tier.rs`;
4. forced-tier sweep on x86_64:

```powershell
foreach ($L in 0,3,5,6,8,9) { $env:CODEC_SIMD_FORCE_LEVEL=$L; cargo test --release --test cross_tier }
```

5. `cargo test --release`;
6. aarch64 and wasm32 cross-compile checks;
7. fixture tests matching ffmpeg-rs scalar outputs for at least:
   - HEVC 8-bit qpel/epel;
   - HEVC 10-bit qpel/epel;
   - weighted and unweighted bi-pred;
   - residual add at bit depths 8/10/12.

## ffmpeg-rs Integration Order

Once codec-simd-rs exposes the pieces above, ffmpeg-rs should integrate in this order:

1. `add_residual` via `add_clip_to_pixel_u16`.
2. HEVC 8-bit MC for `aaa.MP4` path.
3. HEVC u16 high-bit-depth MC for `IMG_9436.MOV` path.
4. transform add primitives.
5. deblock/SAO.
6. RGB output wrappers.

The highest-priority missing item for ffmpeg-rs Main10 performance is P0: high-bit-depth HEVC MC u16 primitives with caller-reusable scratch.
