# CLAUDE.md — `crates/otio-time`

> Dev conventions (Alex Joss, joss13@gmail.com): see `../../CONVENTIONS.md`.

## What
Pure-Rust port of OTIO's `opentime` module (Phase 1 of otio-rs, see `../../PLAN.md`).
Three `Copy` value types + SMPTE timecode. No deps but `thiserror`. Independently
usable (timeline widgets can depend on this crate alone).

- `RationalTime { value, rate }` — `rational_time.rs`
- `TimeRange { start_time, duration }` — `time_range.rs` (Allen's 13 relations)
- `TimeTransform { offset, scale, rate }` — `time_transform.rs`
- `TimeError` — `error.rs` (replaces OTIO `ErrorStatus` + invalid-sentinel)

Lib root: `src/otio_time.rs` (`[lib] name = "otio_time"`).

## Ported from
`_ref/OpenTimelineIO/src/opentime/{rationalTime.h+cpp, timeRange.h, timeTransform.h,
errorStatus.h}`. Timecode/drop-frame/parse logic lives in `rationalTime.cpp` (589 LOC)
— all reproduced.

## Mapping decisions (C++ → Rust)
- Overloaded methods get distinct names: `contains` (RationalTime) vs
  `contains_range` (TimeRange); `*_time` suffix = the RationalTime-arg variant
  (`before_time`, `begins_time`, `finishes_time`, `overlaps_time`).
  `applied_to_{time,range,transform}` for `TimeTransform::applied_to`.
- `rescaled_to(f64)` vs `rescaled_to_time(RationalTime)`; same for `value_rescaled_to*`.
- `from_seconds(s)` (rate 1) vs `from_seconds_with_rate(s, rate)`.
- Default-arg methods take explicit params (no Rust defaults). `epsilon_s` is an
  explicit arg; `DEFAULT_EPSILON_S` is exported. Timecode default → `to_timecode_default`.
- `PartialEq` for `RationalTime` **rescales** (matches C++ `operator==`); use
  `strictly_equal` for exact field compare. `PartialOrd` compares `value/rate`.
  `TimeRange`/`TimeTransform` `PartialEq` mirror their C++ `operator==` (epsilon /
  field-exact respectively).

## Gotchas / deliberate deviations
- **SMPTE rate table**: C++ declares `array<double,11>` but lists 10 values, leaving
  a stray `0.0`. We use 10 (no `0.0`) — see comment in `rational_time.rs`. If a future
  parity test exercises rate `0.0`, revisit.
- `to_time_string` reproduces `printf("%.7g", frac)` via a hand-rolled `fmt_g`
  (incl. `%e` branch) + leading-'0' strip + 7-char cap + ".0" fallback. The C++
  `resize(7,'\0')` quirk maps to our `truncate(7)`.
- `from_time_string` is a clean split-on-`:` reimplementation (≤3 fields; minutes/
  seconds `<60`; only leftmost field signed) — equivalent to the reference's
  right-to-left pointer scan for all well-formed inputs. Per-field sign semantics
  preserved (a negative leftmost field negates only that field, like OTIO).
- `from_timecode` parses fixed offsets 0/3/6/9 (HH:MM:SS<sep>FF), requires len ≥ 11.

## Status
- [x] All three types + timecode + time strings ported.
- [x] `cargo test -p otio-time` → 14 pass. `cargo clippy` → clean.
- [x] **Phase-1 gate GREEN**: differential vs Python `opentimelineio` 0.18.1 →
      543 cases, 0 mismatches. Harness: `examples/oracle.rs` (tab line-protocol)
      + `tests/diff_opentime.py`. Re-run:
      `cargo build -p otio-time --example oracle; python crates/otio-time/tests/diff_opentime.py`.

### Found via the differential (parity fixes)
- `from_time_string` rejects **any** sign (`-`/`+`) in **every** field — even
  `-90` and `+01:00:00.0` → ERR. The reference's backward scan trips on the sign
  char before any signed parse. (`to_time_string` still *emits* `-` for negatives;
  it is intentionally one-way.) Fixed via `is_plain_decimal` per-field validation.

## Next (Phase 2)
`otio-core` (arena + Metadata via box-rs) + `otio-serde` + `otio-schema`. This crate
is a dependency of all of them.
