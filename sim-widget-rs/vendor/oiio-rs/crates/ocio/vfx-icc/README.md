# vfx-icc

ICC colour profiles and profile-to-profile transforms, on `lcms-rs`, a pure-Rust port of
Little CMS 2 (no C library, no FFI, `#![forbid(unsafe_code)]`).

## What it does

- `Profile`: load ICC v2/v4 profiles from a file or bytes (`from_file`, `from_icc`), build
  standard ones (`srgb`, `linear_srgb`, `adobe_rgb`, `display_p3`, `dci_p3`, `aces_ap0`,
  `aces_ap1`, `rec709`, `rec2020`, `gray(gamma)`, `xyz`, `lab`, or
  `from_standard(StandardProfile::..)`), read the info tags (`description`, `manufacturer`,
  `model`, `copyright`, `color_space`) and serialize (`to_icc`).
- `Transform`: RGB float transform between two profiles with an `Intent` (perceptual, relative
  colorimetric, saturation, absolute colorimetric). `apply`/`apply_pixel` work in place,
  `transform` writes to a separate buffer, `apply_buffer` takes interleaved `f32` RGB.
  `new_uncached` builds a transform without the CMM cache, for sharing between threads.
  `convert_rgb` is a one-call helper.
- Matrix/TRC and LUT-based (A2B/B2A) profiles, as far as `lcms-rs` supports them.

## Example

```rust
use std::path::Path;
use vfx_icc::{Intent, Profile, Transform};

let camera = Profile::from_file(Path::new("camera.icc"))?;
let aces = Profile::aces_ap0()?;
let transform = Transform::new(&camera, &aces, Intent::Perceptual)?;
let mut pixels = [[0.5_f32, 0.3, 0.2]];
transform.apply(&mut pixels);
```

Every constructor returns `IccResult`; failures are typed `IccError` variants.

## Features and tests

No Cargo features. Unit tests pin reference colour values (sRGB EOTF, sRGB->ACEScg Bradford,
sRGB->Display P3):

```
cargo test -p vfx-icc
```

Test tooling is described in [`docs/testing.md`](../../../docs/testing.md); open work is in
[`docs/ROADMAP.md`](../../../docs/ROADMAP.md).

## Dependencies

Uses `lcms-rs`, `bytemuck` and `thiserror`. Used by `vfx-ocio` (monitor and display
ICC profiles).
