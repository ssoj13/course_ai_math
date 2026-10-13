# src/tags/ — ICC tag-type serialization (port of `cmstypes.c`)

Bit-exact, pure-Rust port of Little-CMS `src/cmstypes.c` (`vfx.ref/Little-CMS`). This is the round-trip layer
profile I/O (cmsio0/cmsio1) will sit on top of.

## What this is
- `value.rs` — `TagValue`: the decoded payload of a tag (lcms's opaque `void*`). One variant
  per decoded *object*, so `curv`/`para` share `Curve(ToneCurve)`, the four text types share
  `Mlu(Mlu)`, all five LUT/MPE types share `Pipeline(Pipeline)`, pseq/psid share `Seq`.
- `handler.rs` — `TypeHandler` enum (34 distinct handlers = `SupportedTagTypes`, the 2 broken
  aliases reuse XYZ/Curve) + `get_handler(sig)`. Also the shared plumbing: type-base +
  alignment (`_cmsRead/WriteTypeBase`, `_cmsRead/WriteAlignment`, `_cmsALIGNLONG`), position
  tables (`Read/WritePositionTable`), `from_8_to_16`/`from_16_to_8`, `tell`/`seek_to`,
  `stream_len`, and the `type_sig`/`mpe_sig` constant modules.
- `descriptor.rs` — `TagDescriptor` + `get_descriptor(sig)` (66 tags = `SupportedTags`) with the
  DecideType callbacks (`DecideLUTtypeA2B/B2A`, `DecideXYZtype`, `DecideCurveType`,
  `DecideTextType`, `DecideTextDescType`) and the `tag_sig` constants.
- `types/` — per-family (de)serializers: `xyz`, `curve`, `lut`, `mpe`, `mlu`, `namedcolor`,
  `seq`, `dict`, `misc`.

## Dispatch shape (why an enum, not a `dyn` trait)
A `dyn TagTypeHandler` can't expose generic `io: &mut (impl Read+Seek)` (object safety), and the
reused `io::endian` primitives are generic over `Read`/`Write` (can't go through `&mut dyn`). So
the registry is the `TypeHandler` enum with generic `read`/`write` methods fanning out to
`types::*` — a faithful stand-in for lcms's function-pointer table. Writers take `Write+Seek`
because position-table types (`mAB `,`mBA `,`mpet`,`psid`,`dict`,`MHC2`) seek back to patch a
zero-filled directory.

## Non-obvious fidelity points (do not "fix")
- `write(&self, io, value, icc_version)` — `icc_version` mirrors `self->ICCVersion`; **only**
  pseq/psid consult it (via `SaveDescription`, `desc` for V2 vs `mluc` for V4).
- MLU serialization works in canonical `u16` units; the C `wchar_t`↔`u16` arithmetic collapses
  to identity. Offset = pool_bytes + 12*count + 16; header = 12*count + 8.
- MPE tone curves are ALWAYS segmented (seg types 0 sampled / 6,7,8 formula). A plain parametric
  curve is not writable inside `mpet` (that's `WriteSegmentedCurve`'s `Type-6 ∈ 0..2` gate).
- MHC2 write does NOT seek back to the end after patching its directory — matches C (lcms's mem
  handler tracks a high-water mark). `Cursor::into_inner()` still returns the full buffer.
- `desc` (`Type_Text_Description`) always pads to a 4-byte boundary, but the reader does not
  consume that padding and pseq adds no inter-record alignment. So **V2 pseq is byte-stable only
  when each embedded description has `strlen ≡ 1 (mod 4)`** (`len_text ≡ 2 mod 4`). This is an
  lcms property, reproduced faithfully — V4 pseq (mluc, no padding) is always byte-stable.
- Dictionary read prepends (`cmsDictAddEntry`), so one read reverses entry order; byte-identity
  holds over a *two*-cycle round trip (see the dict test).

## Testing gate
Each type has a `#[cfg(test)]` round-trip proving **serialization idempotency**:
`write(v)→b1`, `read(b1)→v'`, `write(v')→b2`, `b1 == b2`. `testutil` (in `mod.rs`) frames the
8-byte type base so position-table offsets are exercised. 40 tag tests, all green; clippy clean.

## Status
- [x] All 34 distinct type handlers + 2 broken aliases (XYZ/curve) + 3 MPE element handlers.
- [x] 66 tag descriptors + 5 DecideType fns.
- [x] Plug-in tag/type registration (`_cmsRegister*Plugin`) — `get_descriptor` and
      `get_handler` now consult the process-global `crate::plugin` registry first, then the
      built-ins. `TypeHandler::Plugin` carries a plugin tag-type's dynamic read/write (over
      `&mut dyn ReadSeek`/`WriteSeek`); a signature-0 tag flips `avoid_type_check` (DEFERRED #7).

## Integration note
`lcms.rs` needs `pub mod tags;` (added) for the module to exist in the crate.
