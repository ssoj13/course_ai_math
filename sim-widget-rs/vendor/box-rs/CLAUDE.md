# CLAUDE.md — `box-rs`

> Working notebook for this repo. Keep it current: structure, decisions, gotchas.

## What this is
A standalone, dependency-light **nested data container** (python-box parity):
dot/bracket/attr path access, typed getters/setters, smart merge, structural
diff/subtract, JSON interop. Extracted from `mermaid-rs/crates/rust-box` on
2026-06-19 into its own private repo for reuse across the `cglibs` projects.

## Naming gotcha (important)
- Package name: **`box-rs`**.
- Crate links as **`box_rs`** — bare `box` is a reserved Rust keyword, so the
  `[lib] name` is `box_rs`. Always `use box_rs::{RustBox, BoxValue, …};`.
- Consume it with: `box-rs = { git = "ssh://git@github.com/ssoj13/box-rs.git", branch = "main", package = "box-rs" }`.

## Structure
```
src/
  lib.rs        RustBox + Index<&str> (dot-aware via box_dots), get_path/get_str
  value.rs      BoxValue enum (typed: Vec3/Vec4/Mat3/Mat4/Color + scalars + Buffer),
                ValueKind, DType, BufData, Index<usize>, JSON conversion, hand-written serde
  container.rs  [feature safetensors] save_safetensors/load_safetensors — one .safetensors
                file = buffers as tensors + box tree JSON in __metadata__["box"]
  access.rs     BoxRef/BoxMut, get_at_ref/navigate_mut, fluent .attr()/.idx()
  path.rs       parse_path (`a.b[2].c`) -> SmallVec<Segment>
  config.rs     BoxConfig, RecastRule, safe-prefix policy
  merge.rs      MergeOptions + all policies (object/array/scalar/null/type conflict)
  diff.rs       DiffOptions, DiffReport, Change/ChangeKind, apply patch
  subtract.rs   SubtractOptions, structural subtraction
  convert.rs    From/Into helpers, JSON value bridging
  error.rs      BoxError / MergeError / DiffError / SubtractError (thiserror)
```

## Deps
serde, serde_json, thiserror, smallvec — base crate, zero internal/cglibs deps.
**Optional** `safetensors` (feature `safetensors`, OFF by default) for the binary
container backend. It is the value model of `nodes-core` (in nodes-rs).

## Gotchas
- `BoxValue` serde is **hand-written tagged** (not derive) — keep value.rs round-trip
  tests green when adding variants (`cargo test`). `Buffer` serializes as
  `{"$buffer":{dtype,shape,data:[u8]}}`; the DECODE path (`from_json_value` /
  `tagged_from_obj` / container `decode`) must handle every `$`-tag or it silently
  demotes a tagged value to a plain Object (a real bug the container test caught).
- **Adding a `BoxValue` variant is semver-breaking** for exhaustive consumer matches
  (no `_` arm) — see README Consumers. Update those before they bump the pinned rev.
- `BufData`'s `Debug` prints `[N bytes]` on purpose: error paths format values with
  `{:?}`, and dumping MB of buffer bytes into an error string is a footgun.
- Buffer bytes are `Arc<[u8]>` → clone/`deep_freeze_clone` SHARE (cheap), never copy.
- The `expect("box-rs: …")` panic strings are intentional invariants of the panicking
  index ops; prefer `get_path` for the `Option` form.

## Tests / build
`cargo test` (20 core) and `cargo test --features safetensors` (adds the container
round-trip). `cargo check`. See `SPEC.md` + `README.md`.
