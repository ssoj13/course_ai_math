# vfx-transfer

Transfer functions (OETF/EOTF and camera log curves) as plain per-value functions.

## What it does

- Display and broadcast curves: sRGB (`srgb`), Rec.709 (`rec709`), BT.2020 (`bt2020`), PQ /
  SMPTE ST 2084 (`pq`), HLG / BT.2100 (`hlg`), pure gamma 2.2/2.4/2.6 and OCIO's moncurve
  (`gamma`).
- Camera log curves: ARRI LogC3 and LogC4 (`log_c`, `log_c4`), Sony S-Log2 and S-Log3 (`s_log2`,
  `s_log3`), Panasonic V-Log (`v_log`), Canon Log/Log2/Log3 (`canon_log`), DJI D-Log (`d_log`),
  Apple Log (`apple_log`), RED REDLogFilm and Log3G10 (`red_log`), Blackmagic Film Gen5
  (`bmd_film`), DaVinci Intermediate (`davinci_intermediate`), ACEScc and ACEScct (`acescc`,
  `acescct`).
- `log_camera::LogCameraTransform`: the shared log-with-linear-toe model that most camera curves
  are built on.
- `pq::ocio_lin_to_pq`/`ocio_pq_to_lin`: PQ in OCIO's FixedFunction convention (1.0 = 100 nits,
  mirrored, unclamped), used by vfx-ocio on CPU and GPU.
- `fast`: branch-free `f32` transcendentals for pixel loops; the `*_fast` curve variants (for
  example `srgb::eotf_fast`, `pq::eotf_fast`) are built on them.

## Example

```rust
use vfx_transfer::{hlg, pq, srgb};

let linear = srgb::eotf(0.5);
let encoded = srgb::oetf(linear);
let nits = pq::eotf(0.5);      // absolute cd/m2
let scene = hlg::eotf(0.5);
```

## Features and tests

No Cargo features.

```
cargo test -p vfx-transfer
```

Besides the unit tests, `tests/` holds published-value checks (`red_gate_camera_curves.rs`,
`aces_validation.rs`, `ocio_parity.rs`), a bound on how far the `*_fast` variants may differ from
the exact curves (`fast_curve_gate.rs`), and `heif_transfer_parity.rs`, which compares six
modules with their copies in heif-rs's `heif-transfer` crate. Test tooling is described in
[`docs/testing.md`](../../../docs/testing.md); open work is in
[`docs/ROADMAP.md`](../../../docs/ROADMAP.md).

## Dependencies

Uses `vfx-core` only. Used by `vfx-color`, `vfx-ocio`, `vfx-io` and `vfx-ops`.
