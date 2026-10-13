# vfx-math

Small f32 math crate for colour and image work. No Cargo features.

## Contents

- `Mat3`, `Vec3`: row-major storage, column vectors (`result = matrix * vector`); `to_glam` /
  `from_glam` convert to glam's column-major types.
- `layout`: matrices whose row/column-major convention is part of the type (`Mat4<RowMajor>`,
  `to_col_major()`); used where a matrix is uploaded to a GPU uniform. Not glob-re-exported.
- Chromatic adaptation: `BRADFORD`, `CAT02`, `VON_KRIES`, `XYZ_SCALING` (and inverses),
  `adapt_matrix`, `adapt_matrix_inv`. White points are not tabulated here; build them from
  chromaticities with `vfx_primaries::white_xyz` (see ROADMAP G16).
- Interpolation and shaping: `lerp`, `inverse_lerp`, `remap`, `smoothstep`, `smootherstep`, `bias`,
  `gain`, ...
- `bit_range_convert` (integer bit-depth rescale).
- `simd`: `wide`-based batch helpers (`batch_mul_add`, `batch_mat3_mul_vec3`, `batch_lut1d`, ...).

```rust
use vfx_math::{Mat3, Vec3, lerp};

let rgb_to_xyz = Mat3::from_rows([
    [0.4124564, 0.3575761, 0.1804375],
    [0.2126729, 0.7151522, 0.0721750],
    [0.0193339, 0.1191920, 0.9503041],
]);
let xyz = rgb_to_xyz * Vec3::new(1.0, 0.5, 0.25);
let t = lerp(0.0, 1.0, 0.5);
```

Depends on `vfx-core`, `glam`, `wide`. Used by vfx-io, vfx-ops, vfx-compute, vfx-ocio,
vfx-primaries, vfx-color and vfx-warp.

## Tests

```
cargo test -p vfx-math
```

Unit tests only. Test tooling: [`docs/testing.md`](../../../docs/testing.md). Open work:
[`docs/ROADMAP.md`](../../../docs/ROADMAP.md).
