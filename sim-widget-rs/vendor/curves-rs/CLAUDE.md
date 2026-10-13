# curves-rs - pure-Rust keyframe tracks (Nuke / Fusion / Maya style)

Keep this file current: record anything non-obvious about this crate here (TODOs with [ ] marks, traps, decisions).

## What
`curves` is a tiny dependency-free library (serde optional) for **animated scalars**: a `Track` of `Key`s with per-side
tangent types, ONE cubic-Bezier evaluator for every segment, pre/post extrapolation, vector `Channels<T, N>`, a key
reduction `Fitter`, and a reader for old scenes. Design note with the maths and decisions: `docs/KEYS.md`.
No UI, no FFI, no unsafe. Generic over `Real` (f32 / f64, default f64).

## Layout
- `src/curves.rs` - lib root (`[lib] path`): `Real` trait + re-exports.
- `src/keys.rs` - `Tan`, `Side`, `Key`, `Extrap`, `Track`, `Channels`, `unwind_slice`; tests in `src/keys/tests.rs`.
- `src/fit.rs` - `Fitter` (Schneider fit adapted to v(t), hard error bound, measured on the finished track).
- `src/legacy.rs` - `pub mod legacy`: `CurveKind` / `Tangent` / `LegacyKey` + `Track::from_legacy` (loading only).
- `examples/edit_bench.rs` - edit cost (append 10k keys, `move_key` at 10k): `cargo run --release --example edit_bench`.
- `tests/eval_alloc.rs` - counting allocator proves `eval` is allocation-free.

## Model in one paragraph
Segment `i` = key `i` `out` side + key `i+1` `inn` side. Each side: `Tan` {Smooth, Linear, Constant, Flat, CatmullRom, User},
`User` carries slope + optional time weight (None = span/3). Handles: `P1 = (t0+w0, v0+m0*w0)`, `P2 = (t1-w1, v1-m1*w1)`,
widths clamped to `[0, span]` so x(u) is monotone; `a = b = 1/3` skips the solve (X(u) = u). Smooth = Catmull-Rom slope with
Fritsch-Carlson clamp (no overshoot). Constant out = hold left; Constant in on the right key = jump after the left key; Constant is the OUT side only
(setting it never touches the other side, and a Constant side is exempt from unbroken coupling).

## Traps
- Keys are private; every edit goes through a `Track` method, which splices/refreshes the derived `Seg` cache locally
  (segments `i-2..=i+1`). A test compares the local result with a full rebuild after 1500 random edits - keep it green
  when adding setters.
- ONE validation point: `Key::checked` (finite, `-0.0` -> `0.0`, slope finite, weight finite `>= 0`, unbroken coupling).
  Anything that puts keys into a track (from_keys, insert, serde, legacy, fitter, scale_time, shift) goes through it.
- ONE evaluator: `eval`, `slope_at`, `integrate` share `outside()` (extrapolation mapping) and the segment lookup.
  `integrate` is exact: `Y(u) X'(u)` is degree 5, so 3-point Gauss-Legendre needs no per-kind code.
- The fitter writes measured weights and re-checks the bound on the finished track (refit with tighter inner tol).
- Auto slopes depend only on neighbour times/values, never on neighbour tangent types (keeps invalidation local).
- Unbroken keys: setting one side makes the other follow; use `set_broken`/`set_seg` to make sides differ.
- `from_legacy` is semantic for CatmullRom/Monotone (real key times, one-sided ends), exact for the other kinds.
- The new-key tangent is NOT a crate global: hosts pass `Tan` to `Track::add` (decision 2026-10-06).
- Heredocs with apostrophes break the Bash tool here; write files with the Write tool.
- Every edit re-checks the derived `Seg` values (`Seg::finite`, 16x margin) and rolls back with `KeysError::NotFinite`
  (`edit`/`splice`/`unsplice`/`set_keys`); new mutators must go through those, never write `keys` directly.

## Tests
`cargo test --all-features`, `cargo clippy --all-features --all-targets -- -D warnings`, `cargo fmt`.

## Consumers (all by `git = "ssh://git@github.com/ssoj13/curves-rs.git", branch = "main"`)
Migration to `Track` pending in: egui-widgets-rs `egui-ramp`, ofx-rs `ofx-host` + `ofx-host-egui`, playa (`playa-engine`,
`playa-ui`), playa-menu, warpbro-rs, warpbro-diag. Rv-rs lists the dependency in `Cargo.toml` only. Exact list: see the
migration report of the `feat/keys-tangents` work.
