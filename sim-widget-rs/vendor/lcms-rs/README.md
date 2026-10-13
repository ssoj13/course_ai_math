# lcms-rs

A complete, from-scratch port of **Little-CMS (lcms2 2.19)** — the ICC colour-management
engine — to idiomatic **pure Rust**.

**No FFI. No `unsafe`. Bit-exact with the C library.**

```toml
[dependencies]
lcms-rs = { git = "https://github.com/ssoj13/lcms-rs" }
```

## Status

All 26 C modules ported (~38 000 lines of C → ~32 000 lines of Rust).

| | |
|---|---|
| Tests | **340** passing (unit + integration + doc, `--all-features`) |
| Clippy | 0 warnings (all-features, default and no-default, `--all-targets`) |
| `unsafe` blocks | **0** |
| FFI / `extern` | **0** |
| Unimplemented stubs | **0** |
| Open parity holes | **0** — all 382 `lcms2.def` exports accounted for (`.bughunt/api_census.md`) |

### Verified bit-exact against the real lcms2

The port is not merely "passes its own tests": `tools/gen-vectors/gen.c` links the genuine
lcms2 2.19 and emits **1029 golden vectors**, which `tests/golden.rs` reproduces and asserts
**byte-identical**. Coverage: sRGB→sRGB across all four rendering intents and with black-point
compensation, sRGB→Lab16, sRGB→XYZ16, sRGB→CMYK8 through a real printer profile's CLUT, Lab/XYZ
encodings, tone-curve evaluation and ΔE2000.

Additionally, seven real ICC profiles are loaded, fully tag-decoded and re-saved
**byte-for-byte identically** (`tests/real_icc.rs`), and a fuzzer-corpus profile is handled
without panics or out-of-bounds reads.

## What it does

- **Profile I/O** — open/save ICC v2 & v4 from file or memory; all 36 tag types, 3 MPE element
  types and 66 tag descriptors; byte-exact round-trip; MD5 `ProfileID`.
- **Transforms** — `cmsDoTransform` equivalent for RGB / CMYK / Lab / Gray, 8-bit, 16-bit and
  float, all four rendering intents, with or without black-point compensation; multi-profile
  chains; proofing with gamut-check alarm codes.
- **Colour science** — XYZ / Lab / LCh with separate V2 and V4 encodings, tone curves (10
  parametric families plus their analytic inverses), white-point and Bradford chromatic
  adaptation, CIECAM02, four ΔE metrics.
- **Virtual profiles** — sRGB, RGB from primaries, gray, Lab v2/v4, XYZ, OkLab, BCHSW abstract,
  NULL, linearisation and ink-limiting device links.
- **Detection & analysis** — black-point (including the Adobe destination algorithm), total ink
  coverage, gamut boundary descriptor, RGB profile gamma estimation.
- **Advanced CMM** — pipeline optimisers that keep *serialisable* stages behind a swappable
  fast evaluator, black-preserving K-only / K-plane intents, transform → device-link baking.
- **Peripheral** — PostScript CSA/CRD generation, CGATS.17 / IT8.7 / `.cube` parsing.
- **Plugins** — ten categories, held per `Context` with Context-0 fallback exactly as lcms does
  it, and consulted before the built-ins (parametric curves, tag, tag type, MPE, formatters,
  intents, optimisers, interpolation, transform, parallelisation).

## Feature flags

| Flag | Default | Effect |
|---|---|---|
| `std` | ✅ | Always on today; a seam for a future `no_std` core |
| `half-tables` | ✅ | lcms's verbatim half-float lookup tables (bit-exact) |
| `cam02` | — | CIECAM02 appearance model |
| `cgats` | — | CGATS.17 / IT8.7 / `.cube` parser |
| `postscript` | — | PostScript Level 2 CSA/CRD generation |

## Design notes

The port follows the C **behaviour** exactly while using Rust idiom for **structure**:
function-pointer tables become enums and traits, the `void*` tag payload becomes a typed
`TagValue`, parallel arrays become one `Vec<TagEntry>`, and C's manual allocator, mutex and
`Dup`/`Free` machinery dissolve into ownership, `Sync`, `Clone` and `Drop`.

Where rounding is load-bearing the C is transcribed verbatim — the Sakamoto fixed-point
interpolation (which relies on signed wraparound, reproduced with `wrapping_*`), the half-float
tables, the V2/V4 encoding split, and every published constant. Those places are commented
"do not clean up".

Two of lcms's twelve plugin categories, **Memory** and **Mutex**, are deliberately omitted:
they exist in C to patch language gaps that Rust closes natively, so there is no indirection
for a plugin to displace. This is a recorded scope decision — every other category is fully
implemented, never accepted-and-ignored.

## Repository layout

```
src/
  types/     numeric + colour types, ICC signatures, pixel-format bitfield
  math/      matrices, half-float, N-D interpolation kernels
  io/        endian-aware ICC readers/writers, IoHandler backends
  color/     PCS conversions, tone curves, white point, CIECAM02
  named/     MLU, named-colour lists, dictionaries
  pipeline/  Stage / Pipeline evaluation with a fast-eval hook
  tags/      tag-type serialisers and descriptors
  profile/   profile object, header, two-pass save, high-level readers
  cmm/       formatters, optimisers, intents, black point, gamut, transform
  detect/    gamut boundary descriptor
  virt.rs    built-in/virtual profiles      plugin.rs  plugin registry
  cgats/     IT8 parser                     postscript.rs  PS generation
tools/gen-vectors/   C harness that emits the golden vectors
tests/               real-ICC, golden-vector, plugin-registry suites
```

The C reference the port is checked against is Little-CMS 2.19 at commit `8f7f96b`
(<https://github.com/mm2/Little-CMS>); the shared reference archive keeps it as
`vfx.ref/Little-CMS`. Nothing in the build or the tests needs it: the real ICC profiles the
tests open are copied into `tests/fixtures/icc/` (see its README).

## Regenerating the golden vectors

Requires the C lcms2 (e.g. `vcpkg install lcms`) and MSVC. Edit the paths in
`tools/gen-vectors/build.bat` if yours differ, then:

```sh
tools/gen-vectors/build.bat
tools/gen-vectors/gen.exe tests/fixtures/golden/golden.txt tests/fixtures/icc
```

`lcms2-2.dll` must be on `PATH`. The generated `golden.txt` is committed, so the test suite
never needs a C toolchain.

## Documentation

- [`PORTING_PLAN.md`](PORTING_PLAN.md) — the phased roadmap and per-module verification method
- [`DEFERRED.md`](DEFERRED.md) — the ledger that tracked every temporarily-deferred bridge to
  closure (all rows closed)

## Licence

MIT, matching Little-CMS. The C original is © Marti Maria Saguer; see
[Little-CMS](https://github.com/mm2/Little-CMS).
