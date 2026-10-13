# src/ — lcms-rs source (working notes)

Pure-Rust port of Little-CMS (lcms2 2.19). C reference: Little-CMS 2.19 @ `8f7f96b`, kept in the reference archive as `vfx.ref/Little-CMS/` (no longer a submodule here; the test profiles live in `../tests/fixtures/icc/`).
Roadmap: `../PORTING_PLAN.md` · deferral ledger: `../DEFERRED.md` (all rows closed).
Rule: no FFI, no `unsafe`, bit-exact where it matters (fixed-point, half tables, V2/V4
encodings, published constants).

## Status — ✅ COMPLETE
All 26 lcms2 C modules ported (~32k Rust LOC). **340 tests**, **clippy 0**, **0 `unsafe`,
0 FFI, 0 stubs**. Bit-exact vs the real lcms2 2.19: 1029 golden vectors
(`../tools/gen-vectors` → `../tests/golden.rs`) plus 7 real ICC profiles round-tripping
byte-for-byte (`../tests/real_icc.rs`).

## Layout (mirrors the lcms2 subsystems)
- `lcms.rs` — crate root (`[lib] path` points here, not `lib.rs`); `error.rs` — `cmsERROR_*`.
- `types/` — `num` (fixed-point + quick-floor/saturate), `color` (CIEXYZ/xyY/Lab/LCh…),
  `sig` (4CC `Signature`), `format` (`PixelFormat` bitfield, verified vs lcms2.h 713–738).
- `math/` — `mat3` (cmsmtrx), `half` (verbatim tables, feature-gated), `interp` (cmsintrp:
  1D/bilinear/trilinear/tetrahedral/eval-N + the `cmsInterpFunction` plugin seam).
- `io/` — `endian` (BE codecs), `handler` (Mem/File/Null IoHandler).
- `color/` — `pcs`, `gamma`, `wtpnt`, `cam02` (feature `cam02`).
- `named/` — `mlu`, `namedcolor`, `dict`.
- `pipeline/` — `stage` (Stage/StageData incl. NamedColor + Custom), `mod` (Pipeline eval,
  reverse-eval, `fast_eval` optimisation hook).
- `tags/` — `value`/`handler`/`descriptor` + `types/*` (36 type handlers, 3 MPE, 66 descriptors).
- `profile/` — `mod` (cmsio0: header, tag dict, read/write tag), `save` (two-pass + computeID),
  `read_hl` (cmsio1: tag→Pipeline, whitepoint/CHAD, profile info).
- `cmm/` — `pack/` (formatters), `optimize`, `intents`, `blackpoint`, `gamut`, `alpha`,
  `transform` (6 kernels, plugin pixel loop, parallel scheduler).
- `detect/gbd.rs` · `virt.rs` (built-in profiles) · `plugin.rs` (registry) ·
  `cgats/` (feature `cgats`) · `postscript.rs` (feature `postscript`) · `hash/md5.rs`.

## Changelog (compressed)
- **M0** foundation: types, context, endian, mat3, half (tables bit-identical to C), interp
  (all 6 tetrahedral branches line-verified).
- **M1** colour science: wtpnt (isotempdata + Bradford bit-identical), pcs (V2/V4 kept
  separate), gamma (10 parametric + inverses), cam02, named.
- **M2** pipeline/stages: eval16/float/reverse; V2↔V4 matrices verified.
- **M3** profile I/O: all tag types byte-idempotent; cmsio0 + cmsio1; MD5 ProfileID;
  validated on real ICC profiles.
- **M4** CMM: formatters, optimisers, intents, black point, gamut, alpha, transform —
  end-to-end `cmsDoTransform`.
- **M5/M6**: virtual profiles, detection (black point/TAC/gamut), CGATS, PostScript.
- **Closure passes**: every deferred bridge closed (`Profile: Clone`, black-point round-trip,
  K-preservation, gamut-check, transform→device-link, named-colour stage, `.cube` link);
  optimisers reworked to keep serialisable stages behind a `fast_eval` hook; all ten supported
  plugin categories implemented and consumed.

## Bug-hunt pass 1 (see `../.bughunt/plan1.md`)
A 14-agent line-by-line audit against `_ref/` found 6 HIGH + 5 bug *classes*; all fixed on
`bughunt/plan1-fixes`. The ones worth remembering as traps:
- **Two evaluation lanes.** `_cmsPipelineSetOptimizationParameters` sets `Eval16Fn` ONLY
  (`cmslut.c:1674`). `eval_float` must always walk the chain. The hook is now an `Eval16` trait
  so the type system enforces it - do not "unify" them.
- **`cmsMLUgetASCII` is Latin-1**, one byte per wide unit. `get_ascii_bytes` is the
  serialization path; `get_ascii` -> `String` is display only.
- **MLU/dict readers must bypass the setters.** `MluEntry.text` holds exactly C's MemPool units
  (`add_raw`); routing through `set_wide` truncates at the NUL and shifts every later offset.
- **C's `cmsmin`/`cmsmax` propagate NaN**; Rust's `f64::min`/`max` discard it. Use
  `math::cms_min`/`cms_max`.
- **The optimizer is an accelerator, not a precondition** - C discards its return value at all
  three call sites. Never `?` it.
- **A tag plugin with empty `supported_types` is legal** - it is the `_cmsAvoidTypeCheckOnTags`
  sig-0 sentinel. Reject at the use site, never at registration.
- **C is deliberately asymmetric**: LUT8 rejects an empty pipeline, LUT16 tolerates it; B2A
  rejects, A2B tolerates. Do not "tidy" these into one rule.
- Byte-stability tests cannot catch evaluation bugs (that is how H2 survived). Add an
  evaluation-equivalence test for anything on an eval path, and **mutation-check every gate** -
  three tests in this pass looked green while proving nothing.

## Conventions / traps
- Fixed-point rounding is load-bearing — transcribe C formulas verbatim, don't simplify.
- `PixelFormat.bytes() == 0` means a `double` (8 bytes), NOT zero.
- `Signature::new(b"RGB ")` derives the exact lcms2 enum value; prefer it over literals.
- Tests assert values re-derived from the C formula — never guessed.
- Plugins are held **per `Context`**. A context with no chunk of its own resolves to the
  process-global registry, which IS Context 0. The two-step is `_cmsContextGetClientChunk`: own
  chunk if present, **else** Context 0 — a local chunk SHADOWS the global, it never merges with
  it. Registration follows the same resolver, so `cmsPlugin` and `cmsPluginTHR(NULL, ..)` are
  literally one call here and cannot drift.
  - `Context::new()` means "I am Context 0"; only `with_plugin`/`dup` give a context its own
    chunk — exactly as `cmsCreateContext` is the only way to get one in C. And a new context
    starts EMPTY, it does not inherit Context 0 (`cmsgamma.c:117`); only `dup` copies.
  - Everything is empty by default, so with no plugin registered every subsystem takes its
    built-in path byte-for-byte. Registry-mutating tests live in
    `../tests/plugin_registry.rs` (separate binary, so Context 0 can't leak into unit tests).
- Memory + Mutex plugin categories are omitted by an explicit, ratified scope decision (Rust
  covers both natively) — see `../DEFERRED.md`. No other category is accepted-then-ignored.
