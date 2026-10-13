# vfx-lut

1D and 3D LUT types, LUT file readers and writers, and the ASC CDL formula, for the OCIO stack.

## What it does

- `Lut1D` (mono or per-channel, with a per-channel input domain) and `Lut3D` (a `size^3` RGB
  cube with a domain and an `Interpolation`). Both can build identities, apply to pixels and
  compute an approximate inverse (`invert`, intended for monotonic LUTs).
- `interpolate_lattice`: the one 3D-LUT interpolation kernel (trilinear, tetrahedral, nearest)
  for the whole stack. `Lut3D::apply` and vfx-ocio's processor both call it.
- File formats, each in its own module with `read_*`/`parse_*` and, where supported, `write_*`:
  `.cube` (`cube`), `.3dl` (`threedl`), `.csp` (`csp`), `.spi1d`/`.spi3d` (`spi`), `.spimtx`
  (`spi_mtx`), `.clf`/`.ctf` (`clf`), `.cc`/`.ccc`/`.cdl` (`cdl`), `.itx` (`iridas_itx`), `.look`
  (`iridas_look`), `.mga` (`pandora`), `.vf` (`nuke_vf`), `.hdl` (`hdl`), `.cub` Truelight
  (`truelight`), `.1dl` Discreet (`discreet1dl`), `.icc`/`.icm`/`.pf` Matrix/TRC profiles (`icc`, read only).
- `cdl`: THE ASC CDL of the stack: the four OCIO styles (`apply_cdl_clamped`,
  `apply_cdl_unclamped`, `apply_cdl_inverse_*`, bit-exact with OpenColorIO's CPU),
  `validate_cdl_params` (OCIO's ranges) and the `.cc`/`.ccc`/`.cdl` reader and writer. Used by
  the vfx-ocio processor, vfx-color and vfx-compute.

## Memory order

`Lut3D::data` is Blue-fastest: index `b + size * (g + size * r)`, as in OCIO's `Lut3DOpData`.
`.cube` files are Red-fastest on disk; the `.cube` reader and writer reorder. To move data between
the two orders, use `OrderedLut3dData` (or `swap_red_blue_fastest_order`), never hand-written index
arithmetic.

## Example

```rust
let cube = vfx_lut::read_cube("grade.cube")?;
let lut = cube.lut3d.ok_or("no 3D LUT")?;
let out = lut.apply([0.5, 0.3, 0.2]);
```

`Lut3D::from_data(data, size)` rejects `size < 2`. The `.cube` and `.spi1d`/`.spi3d` readers cap
sizes at OCIO's limits (1D: 300000 entries, 3D: 129 per axis); the other readers do not yet.

## Features and tests

No Cargo features. Tests are unit tests in each module:

```
cargo test -p vfx-lut
```

Test tooling is described in [`docs/testing.md`](../../../docs/testing.md); open work is in
[`docs/ROADMAP.md`](../../../docs/ROADMAP.md).

## Dependencies

Uses `vfx-core`, `quick-xml` (CLF/CTF/CDL) and `wide` (SIMD). Feature `rayon` (default): the
`Lut3D::*_par` helpers run on rayon's pool (`vfx_core::runner`'s default runner); without it they
are serial and rayon is not linked. Used by `vfx-ocio`,
`vfx-color` and `vfx-compute`.
