# CLAUDE.md — `src/openjph`

> Working notebook: keep it current. Parent notes: `../../AGENTS.md`.

## What this is
The public-facing OpenJPH layer (1:1 `ojph_*.rs`): `Codestream` wrapper over `codestream::LocalCodestream`, parameter wrappers
(`ojph_params.rs`), files (`ojph_file.rs`: `MemInfile/MemOutfile` raw-pointer based, `J2cInfile/J2cOutfile` buffered, read errors recorded),
bump/elastic allocators and `LineBuf` (`ojph_mem.rs`), CPU detection + env overrides (`ojph_arch.rs`), messages/errors (`ojph_message.rs`,
`ojph_error!` -> `OjphError` with a numeric code), base types (`ojph_base.rs`, `ojph_defs.rs`), embedded version COM (`ojph_version.rs`).

## Traps
- `Codestream::pull_ref` / `exchange_first` / `fill_and_exchange` return `Err` (not null deref) past the last line.
- `MemInfile::open(*const u8, len)` borrows without a lifetime and `LineBuf::wrap_*` take raw pointers; they stay safe-signature for API
  compatibility (policy: no API breaks). `mem.rs` is the safe layer — prefer it.
- Env knobs are read ONCE (`OnceLock`): `OJPH_CPU_MAX_LEVEL`, `OJPH_CPU_FORCE_LEVEL` (clamped unless feature `unsafe-force-simd`), `OJPH_FORCE_*`.
- `ojph_arg.rs` (unused C++ arg-parser port) was removed; the CLI has its own `Result`-based parser in `bin/ojph.rs`.
- Error codes inherited from C++ collide in a few places (e.g. 0x300B7); ours live in 0x000300E0+.

## TODO
- [ ] `unsafe` on `wrap_*` / `MemInfile::open` + safe `from_slice` (needs the API-break decision to be revisited).
