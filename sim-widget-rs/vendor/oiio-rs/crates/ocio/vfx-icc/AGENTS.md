# vfx-icc agent notes

What the crate is and its public surface: `README.md`. The engine is `lcms-rs` (workspace
`Cargo.toml`, git dependency), a pure-Rust port of Little CMS 2 validated byte-identically
against Little CMS 2.19. The crate-level rustdoc in `src/lib.rs` carries the same background.

## Rules

- **No FFI, no `unsafe`.** `lib.rs` has `#![forbid(unsafe_code)]`. Do not bring back the C
  `lcms2`/`lcms2-sys` binding.
- **The pinned test values are the proof of correctness.** Tests pin sRGB EOTF `0.5 -> 0.214041`,
  the sRGB->ACEScg Bradford column `[0.6130974, 0.07019372, 0.02061559]`, sRGB->Display P3
  `(0.472011, 0.309269, 0.218316)`, `0.8 -> 0.603827`, and the `"XYZData"`/`"LabData"`/
  `"GrayData"` colour-space strings. They held unchanged across the move from the C library to
  `lcms-rs`. If one fails, the mapping onto `lcms-rs` is wrong; fix the code, never the expected
  value.
- **Every fallible `lcms-rs` call becomes a typed `IccError`.** No `.unwrap()`/`.expect()` on an
  `lcms-rs` result reachable from the public API, and no substitute profile on failure.
  `IccError` (`src/error.rs`) has one variant per failure mode; add a variant rather than reuse an
  unrelated one or add a catch-all.
- Only `vfx-ocio` uses this crate, through `Profile::from_file` in `config.rs`
  (`instantiate_display_from_monitor_name`, `instantiate_display_from_icc_profile`). Check that
  call site before changing a public signature.

## Traps

- **A fallback profile is silent wrong colour.** `unwrap_or_else(|_| new_srgb())` on a failed
  standard-profile build does not panic; it returns sRGB labelled as whatever was requested.
  Standard-profile failures propagate as `IccError::StandardProfileFailed`.
- **`Profile::gray` must validate `gamma` itself.** `lcms-rs`'s `ToneCurve::build_gamma` rejects
  unknown curve types, not bad values, so a zero, negative, NaN or infinite gamma would still
  produce a curve. The up-front check returning `IccError::InvalidGamma` stays
  (`test_gray_invalid_gamma`).
- **The internal clones are required.** `lcms-rs` decodes tags lazily and writes them back into
  the profile, so `create_transform` and tag reads need `&mut Profile`, while this crate's API
  takes `&Profile`/`&self`. `Profile::info`, `Profile::to_icc` and `Transform::build` clone the
  inner profile. A clone copies only the tag dictionary and shares one `Arc<Mutex<..>>` I/O
  backing store, so no file bytes are copied. Removing the clone breaks the `&self` signatures.
- **`do_transform` is not in place.** `Transform::apply`/`apply_pixel`/`apply_buffer` stage the
  source in a scratch buffer. `Transform::run` is the one place that casts `[[f32; 3]]` to bytes
  with `bytemuck`. The length checks that return `IccError::InvalidBuffer` stay, because
  `bytemuck::cast_slice_mut` panics on a length that does not divide evenly.
- `description()`/`manufacturer()`/`model()`/`copyright()` return `""` for an absent tag on
  purpose: real profiles often lack them. This is not a swallowed error.
- `StandardProfile::to_profile`'s failure paths (`StandardCurveFailed`, `StandardProfileFailed`)
  use fixed internal coefficients and cannot be reached through the public API. They guard
  against an `lcms-rs` regression. Do not write a test that pretends to trigger them.
- `Profile::srgb()` goes through `StandardProfile::Srgb.to_profile()`, so it cannot drift from
  `Profile::from_standard(StandardProfile::Srgb)`. `Profile::srgb()` and `Profile::xyz()` are
  fallible because `lcms-rs` builds them.

## Layout

`error.rs` (`IccError`, `IccResult`), `profile.rs` (`Profile`), `standard.rs` (`StandardProfile`:
primaries, whites and TRCs as compile-time constants), `transform.rs` (`Transform`, `RGB_FLT`
only; `convert_rgb`), `lib.rs` (`Intent`).
