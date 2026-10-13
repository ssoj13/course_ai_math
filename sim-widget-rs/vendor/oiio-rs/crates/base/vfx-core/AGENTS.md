# vfx-core: agent notes

User-facing overview: `README.md`. Open work: `docs/ROADMAP.md` (IDs below point there).

## Rule: this crate owns the shared rules; callers do not re-derive them

Each module below is the one implementation of its concept (ROADMAP FX-G). Before writing any of
this arithmetic in another crate, call the module; if it does not fit, change it here.

| Concept | Owner | Do not |
|---|---|---|
| float -> integer | `pixel.rs` `quantize_u8/u16/bits/i32/scaled`, `u16_to_u8` (G3) | write `(v * 255.0) as u8` or `v >> 8`: they truncate (0.5 -> 127, 51528 -> 201) |
| unpremultiply | `alpha.rs` (G4) | add a fifth rule; see below |
| alpha/Z/colour channel by name | `channels.rs` (G12) | compare names with `== "A"` or map channels by position |
| ST decode | `st.rs` (G5) | inline `st * size - 0.5`; WGSL/CUDA still hold it as text (G6) |
| texture footprint maths | `texture_filter.rs` (G17) | re-derive ellipse/blur/aspect/MIP choice in a caller |
| texture lookup | `texture_lookup.rs` | copy the drivers/samplers into vfx-io or vfx-warp |
| MIP chain | `mip.rs` | add a 2x2-box-of-level-0 builder |
| resampling filters | `filter.rs` + `resample.rs` | add a second filter enum or resample function |
| who runs parallel CPU jobs | `runner.rs` (`Runner`, `Serial`, `Rayon`, `default_runner`, `rows`) | call rayon directly in a library path an OpenFX plug-in can reach |

## Alpha (`alpha.rs`)

OIIO has two unpremultiply rules and both live here:

- `unpremult_value`/`unpremult_pixel`/`unassociate_alpha` = `ImageBufAlgo::unpremult`: alpha 0 or
  1 leaves the pixel, any other alpha (negative and denormal too) divides; Z is untouched.
- `color_op_alpha` = the colour ops' rule (`colorconvert_impl`): alpha < `FLT_MIN` counts as 1;
  RGB is divided before the transform and multiplied by the same original alpha after.

Divide, never multiply by `1/alpha`: the reciprocal of a denormal is `inf`. `associate_alpha` has
no zero-alpha case (OIIO `premult_impl`). `Rgba::unpremultiply`, vfx-ops `composite`, and vfx-io's
per-format association (`vfx-io/src/alpha.rs`) all call these. Gates: `alpha::tests::
unpremult_matches_oiio_rule`, vfx-io `tests/alpha_oiio_parity.rs` (oiiotool numbers).

## ST (`st.rs`)

Texel-centre convention: `px = st * size - 0.5` (`pixel_center_offset` drops the `- 0.5`).
`StDecode::from_px` is the exact inverse of `to_px`. A tiler that re-encodes a tile's ST into a
cropped source must use `from_px` of the same decode the kernel uses, otherwise tiled output drifts
from single pass (it did, by up to a quarter of the value range). `StConvention::NUKE` (the
`Default`) is vfx-stmap's bottom-up V; `StConvention::OIIO` is OIIO `st_warp` with no flips.

## Texture (`texture_filter.rs`, `texture_lookup.rs`, `mip.rs`)

- `texture_filter` reads no pixels. It includes ports of OIIO's `fast_sincos`/`fast_atan2`
  (MSVC-build `fast_rint`, unfused `madd`). Do not swap them for `f32::sin_cos`: that put
  cos(pi/2) at -4.4e-8, moved the minor axis across an integer in `2*aspect - 1` and changed the
  probe count at `sblur = 0.15` (the oracle caught it).
- `texture_lookup` is generic over `TexelSource: MipSource`. `aniso_footprint` is the ellipse
  path's decision without texel reads; a tiler sizes source regions from it plus `texel_span`, so
  region and lookup cannot disagree (gate `aniso_reads_only_what_the_footprint_and_span_allow`).
  The caller keeps file/UDIM/`firstchannel`/`flip_t`/`Wrap::Default` resolution, the >4-channel
  recursion and `gray_to_rgb`.
- `mip`: `maketx_halve` is `maketx`'s default `box` (`resize_block`): two-pass 2x2 box when both
  sides are even, clamped bilinear at destination centres otherwise. `maketx_halve_rect` over
  `maketx_source_rect` computes any window bit-identically to the whole level
  (`windows_equal_the_whole_level`). Bit-exact with real `maketx` float output (vfx-io
  `texture_oiio_parity.rs`).

## Filters (`filter.rs`, `resample.rs`)

- `FilterKind` is the only filter vocabulary. GPU kernel indices are a partial, per-backend
  property: `compute_index`/`warp_index` return `Option<u32>`, and most kinds have none. A caller
  must error on `None`, not remap to a neighbouring filter. The discriminants are a GPU ABI; do not
  renumber them.
- `FilterKind::Nearest` is a workspace extension, not one of OIIO's 13. OIIO's `"box"` is a
  closed-interval box; `Nearest` is an open-interval point sampler with an exact-integer path in
  `resample_f32` (CPU/GPU ulp parity gate). `"nearest"` resolves to `Nearest`, `"box"` to `Box`
  (`nearest_and_box_disagree_only_at_the_boundary`).
- `"bicubic"` resolves to `CatmullRom`, not `Cubic { a: 0.0 }` (a flatter curve,
  `plain_cubic_has_a_zero_outer_lobe_unlike_catmull_rom`).
- Scale trap: OIIO's `catrom1d` (`FilterKind::CatmullRom`) is exactly twice `catmull_rom_weight`
  (`cubic_bc(x, 0, 0.5)`); `Cubic { a: -0.5 }` ("keys") equals `catmull_rom_weight`. Harmless while
  every caller normalises by the weight sum; not harmless for raw kernel values
  (`keys_cubic_equals_catmull_rom_weight`, `catmull_rom_matches_catrom1d_exactly`).
- `cubic_bc`, `catmull_rom_weight`, `mitchell_weight`, `lanczos_weight`, `lanczos3_weight` and
  `warp_kernel_weight` are public free functions. No Rust caller outside this crate uses them now
  (vfx-compute's warp reads a host-built `FilterLut`); the WGSL/CUDA resize kernels in vfx-compute
  mirror their formulas as text.
- `resample_f32` is the one resampler. `axis_taps_x`/`axis_taps_y` are separate so an anisotropic
  `FilterDesc` resamples each axis with its own support. It returns `None` for non-separable
  filters (`disk`, `radial-lanczos3`); vfx-io `ImageBufAlgo::resize` has the 2D path for those.

## `Roi3D` (`rect.rs`)

The "all" ROI uses `i32::MIN`/`i32::MAX` sentinels, and `is_all` keys on `xbegin` alone (OIIO
`defined()`). `width`/`height`/`depth`/`nchannels` saturate on an "all" ROI; `new_2d` sets
`chend = Roi3D::CHEND_ALL` (10000), not `i32::MAX`, so `chend - chbegin` cannot overflow; `roi_union`
with an undefined operand returns the defined one. These interact: a change to one sentinel
behaviour must be checked against the others. `contains(x, y, z)` has no channel test on purpose
(`contains_with_channel` has it; ROADMAP BUF-29, decision).

## `ImageSpec` (`spec.rs`)

- Size accessors (`image_bytes`, `image_pixels`, `scanline_bytes`, `tile_bytes`, `tile_pixels`,
  `auto_stride`) saturate like OIIO's `clamped_mult64`; a hostile header must not wrap to a small
  size. `size_t_safe()` uses `checked_mul`, because a saturated `u64::MAX` equals `usize::MAX` on
  64-bit targets.
- `default_channel_names` never infers `z_channel`; alpha/Z from names goes through `channels`
  (`set_alpha_z_from_names`).

## Other notes

- `consts` is not glob-re-exported; write `consts::loglin::M`. `consts::white` has no users and
  there are three chromaticity tables in the workspace (ROADMAP G16, P3).
- The crate-level rustdoc in `lib.rs` lists `serde`/`rayon` features that do not exist (ROADMAP
  P3); the real feature list is in `Cargo.toml`.
- `test_support` is `#[doc(hidden)]` and meant only for tests. See `docs/testing.md`.

## Runner (`runner.rs`)

`Runner::run(jobs, job)` runs independent jobs; `rows(data, row_len, runner, f)` is the one
row-per-job loop (`resample_f32_with`, vfx-stmap `generate_with`, vfx-compute's CPU backend). Why:
an OpenFX plug-in must not start threads of its own (a lazily started rayon pool leaves threads and
thread-locals in the plug-in image the host may unload); it passes the host's MultiThread suite as a
`Runner`. Applications keep rayon: feature `rayon` (default) makes `Rayon` the `default_runner`,
without it the default is `Serial` and rayon is not linked. The workspace takes vfx-core with
`default-features = false`; vfx-compute (`rayon`) and vfx-stmap (`parallel`) turn it on. Jobs write
disjoint output, so every runner gives the same bits (gates `runner::tests`,
`resample::tests::every_runner_gives_the_same_samples`).
