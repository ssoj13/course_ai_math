# Changelog

## Unreleased — no crates.io `exr` (2026-10-04)

- The `cli` feature's `image` dependency no longer takes `image`'s default features (which pulled
  crates.io `exr`); it lists image's default format set minus `exr`. EXR is exr-rs's job, and the
  CLI cannot use exr-image because exr-core depends on jph-rs. `ojph` therefore no longer reads
  `.exr` input (it was quantised to 8 bit anyway); every other format is unchanged.

## 0.2.0 — breaking API cleanup (2026-10-01)

Migration for consumers (`exr-core`, `heif-rs`, `exv-codec`, `jph-wgpu-rs`, `vfx-io`):

- `mem::EncodeParams`: the interacting fields `reversible` / `quant_step` / `qfactor` are replaced by
  `quant: Quant` (`Quant::Lossless` (default) | `Quant::Step(f32)` | `Quant::Factor(u8)`), and
  `progression: &'static str` by `progression: Progression`.
  - `reversible: true` → omit (default) or `quant: Quant::Lossless`
  - `reversible: false, quant_step: d` → `quant: Quant::Step(d)`
  - `reversible: false, qfactor: Some(q)` → `quant: Quant::Factor(q)`
  - `progression: "RPCL"` → `Progression::Rpcl` (`"RPCL".parse::<Progression>()` also works)
  - Full struct literals without `..Default::default()` now need a `quant:` field.
- `MemInfile::open(&[u8])` borrows the slice (`MemInfile<'a>`): `infile.open(ptr, len)` →
  `infile.open(&bytes)`. The raw-pointer form is `unsafe MemInfile::open_raw`.
- `LineBuf::wrap_{i32,f32,i64}` are `unsafe fn`; `LineBuf` is no longer `Copy`/`Clone`.
- `mem::DecodeError` is `#[non_exhaustive]` (add a wildcard arm) and now `Clone + PartialEq + Eq`; `EncodeError` too.
- `try_decode`, `try_decode_with`, `try_encode`, `try_encode_with`, `Quant`, `Progression`, `Rounding`,
  `DecodeParams`, `EncodeOptions`, `DecodeError`, `EncodeError` are re-exported at the crate root.
- `ojph_arg` (unused C++ arg-parser port) removed.

Also in this release: all 73 ISO conformance vectors decode pixel-correct, Part-1 BYPASS/RESTART/ERTERM,
HT MIXED, PRCL, PPM/PPT with POC and multi-tile-part, reduced-resolution decode, `Rounding` option,
`ojph` CLI rewritten on the `mem` API, security hardening (see `plan1.md`).
