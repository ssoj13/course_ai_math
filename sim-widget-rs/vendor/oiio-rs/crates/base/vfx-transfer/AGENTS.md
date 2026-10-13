# vfx-transfer agent notes

What the crate is and its public surface: `README.md`. Open work: `docs/ROADMAP.md` (see the
last section here). References: vendor white papers and ITU/SMPTE/AMPAS specs, and OCIO's
`transforms/builtins/*.cpp` in `C:/projects/projects.rust.cg/vfx.ref/OpenColorIO/`.

## Conventions

- **`LogCameraTransform` has two constructors; picking the wrong one silently changes the
  curve.** `build` derives the linear toe from C1 continuity with the log branch.
  `build_with_lin_ext` takes a vendor-published toe verbatim (OCIO's explicit `linearSlope`). If
  the vendor publishes a linear segment, check whether it equals the log branch's tangent at the
  break: V-Log's does, so it uses `build`; D-Log's (`6.025x + 0.0929`) does not, so it uses
  `build_with_lin_ext`, and a test pins its non-C1 character.
- **Log poles clamp, they do not return 0.** When a log argument goes non-positive, clamp it to
  `f32::MIN_POSITIVE` as OCIO does (`LogOpCPU.cpp`). For curves like REDLogFilm, 0.0 is a mid-scale
  code value, so returning 0 made deep negatives brighter than black.
- **Follow every branch of the spec.** ACEScc decode has three branches in S-2014-003, including
  the clamp to HALF_MAX at the top.
- Pin curves to published anchor values (the vendor's 18% grey and black code values), not to
  values computed by our own implementation.

## Traps

- **"Doesn't match the vendor" has more than one cause.** S-Log2 was once S-Log1's numbers under
  S-Log2's name (fix: the right curve). D-Log came from a white paper whose linear toe is not C1
  with its own log branch (fix: a policy decision to carry the published toe). Find out which case
  you have before changing constants.
- **vfx-ocio has a second set of camera curves.** OCIO's `BuiltinTransform` camera styles are
  implemented in `vfx-ocio/src/builtin_transforms/cameras.rs` with OCIO's constants, not with this
  crate. vfx-ocio's `BuiltinTransfer` path still calls these modules. A fix here does not reach
  the builtins, and the other way round (ROADMAP P3, "Duplicated constants").

## Trap: heif-transfer is a numerically identical copy of six of these modules

heif-rs's `heif-transfer` crate carries a copy of `pq`, `hlg`, `bt2020`, `srgb`, `rec709` and
`gamma`. The copy is deliberate: heif-rs is standalone (owner decision 2026-09-24) and an upstream
dependency of oiio-rs. It is not byte-for-byte: the `_fast` variants and the OCIO-only entry
points (`ocio_lin_to_pq`, ...) live only here. `tests/heif_transfer_parity.rs` calls both crates
and asserts bit-for-bit agreement at the breakpoints and in the negative-mirror regions, so a
change to one side fails that test. If you change any of the six modules, make the same change in
heif-rs. The camera log curves are not duplicated there.

`gamma.rs` holds THE moncurve of the stack (2026-09-23, owner decision): `moncurve_params` /
`moncurve_eval` are the port of OCIO's `GammaOpUtils.cpp` (f64, narrowed once); vfx-ocio's
`ExponentWithLinearTransform` (CPU apply, GPU emitters) imports them, and heif-rs's copy is
verbatim. `moncurve_fwd/rev` take negatives on the linear segment (OCIO LINEAR), the
`moncurve_mirror_*` pair mirrors. Gate `gamma::tests::moncurve_matches_pyocio`.

## Open items

Tracked in `docs/ROADMAP.md`: COLOR-11 (the `log_camera.rs` module table lists S-Log3; its row
has not been re-audited) and the P3 "Duplicated constants" item. G8 is done (2026-09-24): PQ
has one encode and one decode rational (`oetf_ratio`, `eotf_ratio`); the ST 2084 functions clamp
to the spec domain, the `ocio_*` ones do not (like OCIO); vfx-ocio's GPU PQ reads the same
constants through `gpu/pq.rs`.
