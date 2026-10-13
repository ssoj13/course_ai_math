# vfx-core

The base crate of the oiio-rs stack: pixel and image types, image metadata, and the single
implementations of rules that several crates need (filters, quantisation, alpha, channel names,
ST coordinates, texture filtering, MIP building). Every other crate in the workspace depends on
it; it depends on no workspace crate.

## Features

| Feature | Default | What it does |
|---|---|---|
| `clap` | off | `clap::ValueEnum` on `FilterKind`, for CLIs (vfx-warp's `cli` feature turns it on). |
| `rayon` | on | rayon's pool is the default `runner::Runner` (the resampler and the CPU paths of vfx-compute / vfx-stmap). Off: `Serial` by default, no rayon linked; an OpenFX plug-in passes its host's MultiThread suite as a `Runner`. |

Always-on dependencies: `half`, `thiserror`; `rayon` with the `rayon` feature.

## Modules

| Module | Contents |
|---|---|
| `pixel` | `Rgb<C, T>`, `Rgba<C, T>`, `PixelFormat` (u8/u16/f16/f32), Rec.709 luma, and the float->integer conversions `quantize_u8/u16/bits/i32/scaled`, `u16_to_u8` (OIIO rounding) |
| `image` | `Image<C, T, N>`, `ImageView`, `ImageViewMut` (channel count is a const generic) |
| `colorspace` | colour-space marker types (`AcesCg`, `Srgb`, `Rec2020`, ...) |
| `spec`, `format`, `rect` | `ImageSpec`, `AttrValue`, `TypeDesc`/`DataFormat`, `Rect`, `Roi`, `Roi3D` (OIIO's `ROI`) |
| `filter`, `resample` | `FilterKind`/`FilterDesc` (OIIO's 13 filters plus `Nearest`), `filter_desc_from_name`, `resample_f32` |
| `alpha` | OIIO's premultiply and its two unpremultiply rules |
| `channels` | OIIO's alpha/Z/colour classification by channel name |
| `st` | ST-map decode (`StConvention`, `StDecode`, `st_to_px`/`px_to_st`) |
| `texture_filter` | footprint maths from OIIO `texturesys.cpp` (ellipse, blur, aspect, MIP level choice, probes, wrap, B-spline weights) |
| `texture_lookup` | the texture lookup itself over a `TexelSource` (used by vfx-io `TextureSystem` and vfx-warp EWA) |
| `mip` | the MIP builder, `maketx`'s box filter |
| `consts` | numeric constants shared by several crates (white points, LogLin, exposure/contrast, ACES) |
| `test_support` | `gpu_unavailable` / `cuda_unavailable` for other crates' GPU tests (hidden from docs) |

`ImageSpec::default_channel_names()` follows OIIO `formatspec.cpp`: 1 channel is `Y`, 2 is `R,G`,
3 is RGB, 4 is RGBA with `alpha_channel = 3`, 5+ is RGBA then `channel4`, `channel5`, ...

## Design

- The colour space is part of the pixel type: mixing ACEScg and sRGB pixels does not compile.
- `Image` has a compile-time channel count.
- Each rule listed above has one implementation here; other crates call it instead of keeping a
  copy.

## Tests

```
cargo test -p vfx-core
```

Several of this crate's functions are checked against real OIIO output from vfx-io's tests
(`alpha_oiio_parity.rs`, `texture_oiio_parity.rs`, `quantize_oiio_parity.rs`). Test tooling
(GPU policy, `cargo xtask parity`): [`docs/testing.md`](../../../docs/testing.md). Open work:
[`docs/ROADMAP.md`](../../../docs/ROADMAP.md).
