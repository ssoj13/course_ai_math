# KEYS - Nuke/Fusion-style keyframe tracks for curves-rs (design note)

Status: **approved and implemented** (decisions in section 8). Code: `src/keys.rs`, `src/fit.rs`, `src/legacy.rs`.
Date: 2026-10-06. Branch: `feat/keys-tangents`. Updated after the two reviews (section 9).

## 0. Scope and non-goals

New module `src/keys.rs` (re-exported from `curves.rs`, so `[lib] path` stays) adds an **animation track**
model: keys with per-side tangents, one cubic-Bezier evaluator, extrapolation, vector channels, a key-reduction
fitter and legacy migration.

The old value-remap `Curve`/`Knot`/`CurveKind`/`eval_segment`/`ease` model is **removed**: one evaluator, `Track`.
A ramp (colour or position over `[0,1]`) is a `Track`/`Channels` with `Tan::Smooth` keys (never overshoots). Only
the serde reader of old scenes remains (`pub mod legacy`: `CurveKind`, `Tangent`, `LegacyKey`; `Track::from_legacy`).

Non-goals: Nuke `Cubic` (global natural C2 spline: every edit changes every key, not local), AE-style
temporal/spatial influence, 3D motion paths, quaternion slerp.

## 1. References checked

- Nuke (AnimationKey / curve serialisation docs, learn.foundry.com): interpolation Constant, Linear ("slopes
  point directly at the next key"), Smooth ("same as Catmull-Rom but the slopes are clamped so the convex-hull
  property is preserved", the default), Catmull-Rom ("slope at key n = slope between n-1 and n+1"), Cubic.
  Extrapolation Constant (end slopes zero) and Linear (left slope of the first key = its right slope, and
  vice versa for the last).
- Fusion/Resolve spline editor manual: Smooth (auto Bezier), Linear, Step In/Out, per-key Bezier handles,
  Reverse / Loop / Ping-Pong strip. The manual's Step In/Out wording is ambiguous, so this design defines the
  semantics itself (3.3) instead of copying it.
- OpenUSD `pxr/base/ts` (`D:\Projects\vfx.ref\OpenUSD\pxr\base\ts`): every curved segment is a cubic Bezier in
  (time, value) (`eval.cpp`: `_Cubic::FromPoints`, time cubic solved for the parameter with Cardano, value cubic
  evaluated there); Hermite is the same with a fixed width; handles are width + slope; "AutoEase" automatic
  tangents; anti-regression (`regressionPreventer.cpp`) keeps x(u) monotone: Contain clamps each width to the
  segment, the regressive test is the ellipse `w1^2+w2^2-2(w1+w2)+w1*w2+1 > 0` on normalized widths;
  extrapolation Held / Linear / Sloped / LoopRepeat (= our CycleOffset) / LoopReset (= Cycle) / LoopOscillate
  (= PingPong). We adopt the segment formula, the width+slope handle encoding and Contain.
- Maya: tangent types Auto / Clamped (no overshoot) / Flat / Linear / Step / Spline, weighted and broken
  tangents, infinity modes constant / linear / cycle / cycle-with-offset / oscillate. Our `Smooth` = Maya
  Clamped = Nuke Smooth; our `CatmullRom` = Maya Spline/Auto (may overshoot).

## 2. Model

```rust
pub enum Tan { Smooth, Linear, Constant, Flat, CatmullRom, User }   // serde: lowercase strings

pub struct Side<T> {          // one side of a key
    pub kind: Tan,
    pub slope: T,             // value per time unit; used by User only (auto kinds derive it)
    pub weight: Option<T>,    // handle length in TIME units; None = segment span / 3; User only
}

pub struct Key<T> {
    t: T, v: T,               // private: edited through Track so derived data stays valid
    pub inn: Side<T>,         // shapes the segment that ARRIVES at this key
    pub out: Side<T>,         // shapes the segment that LEAVES this key
    pub broken: bool,         // sides independent; false => setters keep them coupled
}

pub enum Extrap { Constant, Linear, Cycle, CycleOffset, PingPong }

pub struct Track<T: Real = f64> {          // scalar animation channel
    pre: Extrap, post: Extrap,
    keys: Vec<Key<T>>,                     // authoritative, sorted, strictly increasing t
    /* derived, rebuilt on load */ times: Vec<T>, segs: Vec<Seg<T>>, pre_slope: T, post_slope: T,
}
pub struct Channels<T, const N: usize>([Track<T>; N]);   // Vec3 = Channels<T, 3>; eval -> [T; N]
```

Coupling when `broken == false` (enforced by the `Track` methods and on entry, never by eval): editing one side
makes the other follow (same kind, same `User` slope; weights stay independent, as in Maya). A `Constant` side is
exempt in both directions: it neither propagates nor is overwritten (3.3). To make the sides differ, break the key
first (`set_broken`) or use `set_seg`, which re-evaluates every key it touches: a key whose sides differ becomes
broken, a key whose sides match again becomes unbroken, so `set_seg(i, Linear)` followed by `set_seg(i, Smooth)`
restores the original track. The kind of a NEW key is an argument (`Track::add(t, v, tan)` / `Key::with_tan`); the
crate has no global default, hosts store "New key type". `Tan::ALL`/`Tan::label()` and
`Extrap::ALL`/`Extrap::label()` are the one list every UI uses.

Validation: ONE point, `Key::checked`, used by `from_keys`, `insert`, serde, legacy loading, the fitter and
`scale_time`/`shift`. Time and value must be finite, `-0.0` is read as `0.0` (otherwise `total_cmp` would keep two
keys at "the same" time), `User` slope finite, weight finite and `>= 0` (`KeysError::NotFinite` / `Weight`), and an
unbroken key gets its `in` side coupled to its `out` side. Times must be strictly increasing
(`KeysError::Order(i)` names the key). A stray slope/weight on an auto side is dropped, not an error. Legacy loading
is the only place that clamps (old scenes had out-of-range Bezier handles).

Time unit is opaque (seconds or frames, the host's choice); slopes and weights use the same unit.
`Track::scale_time(k)` multiplies times and weights and divides slopes, so re-timing or an fps change keeps the
curve shape (the result is validated; collapsing or overflowing times are an error and change nothing). Hosts that want frame-rate independence store seconds (or call `scale_time(new_fps/old_fps)`).

| Tan | handle slope m | notes |
|---|---|---|
| Flat | 0 | |
| Linear | secant of the segment on this side | out: toward next key; in: toward previous key |
| CatmullRom | `(v[i+1]-v[i-1]) / (t[i+1]-t[i-1])` | may overshoot; end key: one-sided secant |
| Smooth | CatmullRom slope, clamped (below) | default; Nuke Smooth / Maya Clamped |
| User | stored `slope` | optional `weight` |
| Constant | segment is a hold, see 3.3 | |

Smooth clamp (no overshoot, Fritsch-Carlson style): `sL`, `sR` are the adjacent secants. If `sL*sR <= 0` (local
extremum or a flat neighbour) then `m = 0`; else `m = sign(sL) * min(|m_cr|, 3*min(|sL|, |sR|))`. With the default
width `span/3` this keeps each handle's y inside `[min(v_i, v_nb), max(v_i, v_nb)]`, so every segment stays inside
its two keys' value range. End keys use the one-sided secant. A `User` handle is never clamped.

Auto slopes depend only on neighbour **positions and values**, never on neighbour tangent types, so an edit
invalidates only segments `i-2 ..= i+1`.

## 3. Evaluation

### 3.1 One formula (segment `[t0, t1]`, `s = t1 - t0`)

Handle widths: `w0 = clamp(weight_or(s/3), 0, s)` for the out side of the left key, `w1` likewise for the in side
of the right key. The clamp is USD "Contain": with normalized widths `a = w0/s`, `b = w1/s` in `[0,1]` the time
Bezier is monotone, so the parameter solve has a unique root.

```
P0 = (t0, v0)            P1 = (t0 + w0, v0 + m0*w0)
P3 = (t1, v1)            P2 = (t1 - w1, v1 - m1*w1)
```

With `tau = (t - t0)/s` solve `X(u) = tau`, where `X` is the Bezier with control values `(0, a, 1-b, 1)`, then
`v = Bezier(v0, P1.y, P2.y, v1)(u)`.

- Fast path: `a = b = 1/3` gives `X(u) = u` exactly, no solve (every auto kind and every unweighted User handle).
- Otherwise Newton from `u = tau` inside the bracket `[lo, hi]` (`rtsafe`: a step that leaves the bracket or
  converges too slowly is replaced by bisection), stop at `|X - tau| <= 16 eps`, at most 100 iterations,
  `X'(u) = 3[a(1-u)^2 + 2(1-a-b)u(1-u) + b u^2]`. It reaches the tolerance for every `a, b` in `[0,1]`, also the
  degenerate widths (0, 1, near 1/2) where a fixed 8 iterations left errors of 1e-4. No allocation, no recursion.
- Straight line (`m0 = m1 = secant`), smoothstep (`m = 0`) and Hermite (unweighted User) are all special cases:
  no per-kind code in the evaluator, only in the handle derivation when the `Seg` is built.

### 3.2 Continuity

C0 everywhere except holds. C1 where both sides have equal slope and width (unbroken auto/User keys). Corners:
Linear keys, broken keys, holds. Tests assert C0 at every key and C1 at unbroken Smooth keys.

### 3.3 Constant (hold) semantics, defined here

- `out == Constant` on key `a`: the segment is `v = a.v` for `t` in `[t0, t1)`, jumping to `b.v` at `t1`
  (Nuke Constant, Fusion Step Out, Maya Stepped).
- else `inn == Constant` on key `b`: the segment is `a.v` at `t0` and `b.v` for `t` in `(t0, t1]` (jump right
  after `a`, the reversed step).
- both: `out` wins. A hold is a branch on the `Seg` kind; its handles are unused.
- Constant belongs to the OUT side. `with_tan(Constant)`, `set_tan(i, Constant)`, `add(.., Constant)` and
  `set_side(i, Out, Constant)` set the out side only and leave the in side alone, so the segment arriving at the
  key is not turned into a reverse step. A step-in is explicit (`set_side(i, In, Constant)`, or `set_seg(i,
  Constant)` which sets both ends of one segment) and survives later edits of the out side.

### 3.4 Extrapolation (per track: `pre` before the first key, `post` after the last)

`T = t_last - t_first`. Cycle modes need `>= 2` keys and `T > 0`, otherwise they act as Constant.

| Mode | Value |
|---|---|
| Constant | end key value |
| Linear | `v_end + slope_end*(t - t_end)`; slope = the end key's outer side (first key: `out`, last key: `inn`), derived like an inner slope; a Constant side gives 0. A single key uses `out` before and `in` after it |
| Cycle | evaluate at `t0 + rem_euclid(t - t0, T)` |
| CycleOffset | Cycle plus `k*(v_last - v_first)`, `k = floor((t - t0)/T)` (USD LoopRepeat) |
| PingPong | reflect: even period forward, odd period `t_last - rem`, no offset |

The Nuke rule is kept as is: the last key's `inn` slope is used even when the arriving segment is a hold (the
slope is the key's, not the segment's). The first key's `in` side and the last key's `out` side are dead (no
segment uses them); `Track::handle(i, side)` returns `None` for them.

Non-finite times: NaN evaluates to NaN, `+-inf` to the end key's value for every mode (and slope 0). A finite `t`
so far out that the cycle count or remainder overflows (f32 near 3e38) also gives the end value; the index
arithmetic is saturating, so no input can panic.

Editor mapping: Nuke constant/linear = ours; Fusion Loop = Cycle, Ping-Pong = PingPong, Relative = CycleOffset;
Maya cycle / cycleRelative / oscillate = Cycle / CycleOffset / PingPong.

### 3.5 Vector channels and rotations

`Channels<T, N>` is `[Track; N]`: each component has its own keys, tangents and extrapolation (Nuke, Fusion and
Maya all work per component). `eval(t) -> [T; N]`. Rotation channels store **unwrapped** angles:
`Track::unwind(period)` / `unwind_slice(&mut [T], period)` makes consecutive values differ by at most half a
period (shortest path) at authoring/bake time, so the evaluator needs no wrap logic and tangents see a continuous
function. Both return `Result`: a period that is not finite and positive is `KeysError::Scale`, an overflow of the
running offset is `NotFinite`, and nothing is written on error (two passes, no extra buffer). Euler only;
quaternions are out of scope.

### 3.6 Performance

- `eval` allocates nothing. `times: Vec<T>` (SoA, contiguous) is binary-searched with `partition_point`;
  `Seg { t0, t1, inv_s, v0, v1, y1, y2, a, b, mode }` (9 scalars + a byte: 40 B at f32, 80 B at f64, so not one
  cache line at f64) is the only data touched afterwards.
- `eval_hint(t, &mut usize)` keeps the last segment index: sequential playback and the recorder hit the same or
  next segment in O(1) and fall back to binary search on a miss.
- Every edit is local: value/tangent edits and `move_key` within its neighbours refresh segments `i-2 ..= i+1`
  (O(1)); `insert`/`remove` splice `keys`, `times` and `segs` (a memmove, O(1) at the end) and refresh the same
  window; `move_key` past neighbours rotates the span it crosses. `from_keys`, `deserialize`, `scale_time`,
  `shift` and `unwind` rebuild in O(n). A test compares the result with a full rebuild after 1500 random edits.
  Measured (release, 10 000 keys, f64): appending keys one by one 1394 ms -> 4.3 ms, `move_key` in place
  275 us -> 0.2 us. Keys are private, so cache and keys cannot drift.
- `eval_many(t0, dt, out: &mut [T])` walks segments linearly (baking, LUTs), no search per sample.

### 3.7 Queries and bulk edits

- `find(t) -> Result<usize, usize>`: like `slice::binary_search` on the key times (`-0.0` is `0.0`).
- `set_at(t, v, tan)`: auto-key. The key at `t` keeps its tangents and only changes value; otherwise a key is
  inserted with `tan`.
- `shift(&[usize], dt)`: moves a selection in one step. The final state is validated as a whole (selected keys
  may pass unselected ones, which reorders and changes indices, so hosts track keys by time); landing on another
  key's time is `Order(i)` and changes nothing. Duplicate indices move once.
- `freeze(i, side)` makes an auto side `User` at its current effective slope (no jump); `set_handle(i, side,
  (dt, dv))` is a drag of the handle point: slope `dv/dt`, weight `|dt|`; dragged to or across the key it gets
  weight 0 and keeps its slope. Both go through `set_side`, so coupling applies. `handle(i, side)` is the inverse
  (what an editor draws).
- `slope_at(t)`: `dv/dt` from the same segment data (right-hand at a key, 0 on holds, extrapolated outside, negated
  on a reversed ping-pong pass). One evaluator: `eval`, `slope_at` and `integrate` share the segment lookup and the
  extrapolation mapping.
- `integrate(a, b)`: exact, extrapolation included. For a curved segment `int v dt = s * int_0^u* Y(w) X'(w) dw`,
  a polynomial of degree 5 (cubic times quadratic), so a 3-point Gauss-Legendre rule is exact for weighted and
  unweighted segments alike (one parameter solve for a partial segment); holds are `v * dt`; Linear extrapolation
  is a trapezoid; Cycle / CycleOffset / PingPong add whole periods in closed form (CycleOffset: `dv*T*k(k-1)/2 +
  k*dv*rem`; a reversed ping-pong pass: `total - cum(t_last - rem)`). Cost O(segments between the first key and
  the bound). `b < a` is negative, a non-finite bound gives NaN. Tested against a numeric reference over random
  tracks, all mode pairs and ranges crossing both ends.

### 3.8 Ramps and vector values

Hosts keep their stops (position, colour) and build ONE `Track` per channel with `Track::from_keys` on `[0,1]`,
`Smooth` keys (never overshoots); `Channels<T, N>` is for fixed small dimensions only, hosts with a runtime
dimension hold a `Vec<Track>`. A hard edge is a key with a `Constant` OUT side (no two keys at the same time), a
`Channels` of the wrong length is `KeysError::Count`.

## 4. Key-reduction fitter (`keys::fit`)

Input: samples `(t_i, v_i)` (WarpBro recorder bakes per frame), `tol` finite and `>= 0` (else
`KeysError::Scale`). Output: a `Track` with `|eval(t_i) - v_i| <= tol` for **every** sample (hard bound, tested
by re-evaluating the returned track).

Algorithm (Schneider, Graphics Gems I "An Algorithm for Automatically Fitting Digitized Curves", adapted to a
function `v(t)`):

1. A segment is a sample range `[i, j]` with fixed end keys. Initial parameterization by normalized time,
   `u_k = (t_k - t_i)/(t_j - t_i)` in `[0,1]` (both axes are scaled to the unit square for the least squares).
2. Slopes per sample: the central difference of the two neighbouring samples; the first and last sample use the
   one-sided secant of two samples. An interior split point shares its slope between both sides (C1, unbroken).
3. Solve the 2x2 least squares for the two handle **widths** (slopes fixed), clamp to `[0, s]` (Contain), fall
   back to `s/3` when ill-conditioned.
4. Error = `max |v_k - eval(t_k)|` measured with the real evaluator (vertical error: the output is a function of
   time). `<= tol`: accept. Else up to 4 Newton-Raphson reparameterization passes on `u_k`, retry.
5. Still above tol: split at the sample with max error, recurse on both halves.
6. Pieces become keys: a straight piece gives `Linear` sides, a fitted piece `User` sides (slope = the shared sample
   slope, weight = the MEASURED width, always written: an omitted weight would be evaluated by the `span/3` fast
   path instead of the measured curve). Ranges that already fit a straight line are tried first, so linear data
   collapses to two keys. User handles are kept and nothing is demoted to an auto kind (that would break the
   rigorous bound).

Termination and bound: step 4 measures the evaluator error on the actual samples; the recursion bottoms out at
2-sample ranges, where both keys use `Linear` sides (slope = secant) and the error is 0 at both samples. The
bound is then re-checked on the FINISHED track (rounding between the piece measurement and the built keys); a
miss refits with the inner tolerance halved and finally 0 (every sample a key, exact), so the returned track
always satisfies `tol` and `FitStats::max_err` is measured on it. Cost (release, noisy sine, tol 0.05,
noise 0.3, about 63% of samples kept as keys): 5k samples 34 ms, 10k 73 ms, 20k 223 ms, 40k 981 ms, i.e.
7 us per sample at 5k growing to 25 us at 40k: superlinear on data that keeps most samples as keys. A camera
recording is ~30-60 samples per second, so a minute (1.8k-3.6k samples) fits in well under 30 ms.

API: `Fitter::new()` owns the scratch buffers and refills the key/time/segment buffers of `out` in place (no
per-call allocation in steady state);
`Fitter::fit(&mut self, samples: &[(T, T)], tol: T, out: &mut Track<T>) -> Result<FitStats { keys, max_err }, KeysError>`;
`fit_uniform(t0, dt, &[T], ..)` for per-frame bakes; `out` keeps its extrapolation. Rotation samples must be
unwound by the caller first (`unwind_slice`).

## 5. Legacy CurveKind migration

Both hosts evaluate a segment with the **left key's** `interp` through `eval_segment` (Step = hold-left, constant
extrapolation, tangents in time units, outer neighbours taken one span away: uniform-neighbour assumption).
Migration therefore works per **segment** `i` (left key `i`, kind `K_i`): it sets `out_i` and `inn_{i+1}`. A key
whose sides are not `coupled` (different kind or slope, neither a Constant) gets `broken = true`. `pre = post = Constant`. The first key's `inn` and the
last key's `out` copy the adjacent side (unused).

| Legacy kind | New (out_i, inn_{i+1}) | Exact? |
|---|---|---|
| Linear | Linear, Linear | exact (handles at span/3 on the chord give a straight line) |
| Smooth (smoothstep) | Flat, Flat | exact: Hermite with m = 0 is `3t^2 - 2t^3`, span/3 widths |
| Step | Constant, (unchanged) | exact (hold-left); `inn_{i+1}` is only set when nothing else gives it a type, else the key's out side (Smooth after a Step: a Constant in side would turn the previous segment into a reverse step) |
| Hermite | User, User; `slope = dy/dx` of the old handle (0 when `dx` ~ 0); `weight = None` | exact (old code used only the slope and span/3 spacing) |
| Bezier | User, User; `weight = clamp(abs(dx), 0, span)`, `slope = dy/weight` (sign per side) | exact, except old handles pointing the wrong way (`dx <= 0` out / `>= 0` in) or shorter than one epsilon (`0 < |dx| <= EPS`): the old code put the control x on the key and kept `dy` (a vertical handle); slope+weight cannot express that, so it becomes weight 0 and slope 0 |
| CatmullRom | CatmullRom, CatmullRom | exact only for uniformly spaced interior keys; **ends differ** (old code repeats the end value: half secant; new: full one-sided secant); irregular spacing differs because the new slope uses real times |
| Monotone | Smooth, Smooth | exact only for uniform spacing; the new Smooth uses the time-weighted Catmull-Rom slope + the same `3*min` clamp, the old one the arithmetic mean of secants; at ends and irregular spacing results differ slightly (both stay inside the value range) |

Policy (decision 1): **semantic only**: CatmullRom/Monotone become the auto kinds; no exact-bake mode, no report.
Tests compare an oracle of the old `eval_segment` with `Track::eval` on random non-uniform curves for the five exact
kinds. Bezier handles longer than the span are exact too: the slope uses the clamped width, so `dy` is kept whole as
the old code did; only wrong-way handles collapse to weight 0.

Serde. New on-disk shape (defaults omitted):
`{"pre":"constant","post":"constant","keys":[{"t":0.0,"v":1.0,"in":{"kind":"smooth"},"out":{"kind":"user","slope":0.5,"weight":0.2},"broken":true}]}`.
The hosts' old key structs differ in field names, so curves-rs provides a plain serde
`LegacyKey<T> { t, v, interp: CurveKind, tan_in: Tangent, tan_out: Tangent }` and
`Track::from_legacy(impl IntoIterator<Item = LegacyKey<T>>) -> Result<Track<T>, KeysError>`. A host maps
its own field names onto `LegacyKey` (serde `alias`, or an untagged old/new enum in the host); the maths lives only
in the crate. `CurveKind` keeps its serde derive (the reader needs it) but leaves the animation API.

## 6. Public API (what hosts adopt)

- Types: `Real`, `Tan` (`ALL`, `label`), `Side`, `SideId`, `Key`, `Extrap` (`ALL`, `label`), `Track`,
  `Channels<T, N>`, `KeysError` (`NotFinite`, `Order(i)`, `Index`, `Scale`, `Weight`, `Count`), `unwind_slice`,
  `Fitter`, `FitStats`; legacy reader under `curves::legacy`: `CurveKind`, `Tangent`, `LegacyKey`
  (and `Track::from_legacy`).
- Build/edit: `Track::{new, from_keys, add, insert (upsert), set_at, remove, set_value, move_key, shift, set_side,
  set_tan, set_user, set_handle, freeze, set_broken, set_seg, set_extrap, scale_time, unwind}`, read: `keys, len,
  range, pre, post, find, handle(i, side)`.
- Evaluate: `eval`, `eval_hint(t, &mut hint)`, `eval_many(t0, dt, &mut [T])`, `slope_at`, `integrate(a, b)`;
  `Channels::{eval, eval_hint}`.
- Reduce: `Fitter::{fit, fit_uniform}`.
- serde (feature `serde`): `Track` as `{pre, post, keys:[{t, v, in, out, broken}]}`, defaults omitted, validated on load.

## 7. Test plan (implementation phase)

Continuity (C0 at keys, C1 at unbroken Smooth), clamping (Smooth stays inside neighbours' range on random data;
CatmullRom does overshoot), regression guard (any weights: eval monotone in t for monotone data), hold semantics
(3.3, Constant out-only), every extrapolation mode incl. single key and `T = 0`, `+-inf` and overflowing times for
every mode pair, `-0.0`, non-finite and negative handles, `unwind`/`scale_time` validation and rollback, vector
channels, serde round trip and validation on load, legacy mapping (exact kinds), fitter error bound re-measured on
the returned track (noisy walk and sine, tolerances 1e-6..0.5) plus key-count reduction, allocation-free eval
(counting allocator in a test), `scale_time` invariance, Newton solver on corner weights (0, 1e-9, 1/3, 1) vs the
tolerance, local splice equal to a full rebuild over 1500 random insert/remove/move/shift/set_at/tangent edits,
`integrate` vs a numeric reference and closed forms, `slope_at` vs finite differences.

## 8. Decisions (operator, 2026-10-06)

1. Legacy CatmullRom / Monotone migrate to the semantic auto kinds; no exact bake, no compatibility layer beyond loading.
2. The default tangent of a new key is a host setting (Settings > Animation > New key type: Linear | Smooth, default
   Smooth); the crate takes the `Tan` as an argument of `Track::add` / `Key::with_tan`, no global.
3. Auto slopes ignore neighbour tangent types (Smooth next to a Constant segment stays local).
4. Slope + weight handles, no vertical handles.
5. The old `Curve`/`Knot`/`CurveKind` model is dropped: one evaluator, every consumer moves to `Track`.
6. (Review, 2026-10-06) Constant is the OUT side only (Nuke / Maya step); a step-in is explicit. `set_seg` toggles:
   keys whose sides match again are unbroken.
7. The fitter keeps `User` handles and writes measured weights (hard bound on the finished track); no automatic
   demotion to auto kinds.
8. Linear extrapolation keeps the Nuke rule (first key out / last key in slope); dead sides are hidden by `handle`.
9. One validation point (`Key::checked`) and one evaluator (`eval`, `slope_at`, `integrate` share the lookup and
   the extrapolation mapping); the crate owns `integrate` and `slope_at` so hosts never re-derive segment modes.

## 9. Open points

- `Linear` slopes use the neighbours' secants even across a hold (auto slopes ignore neighbour tangent kinds, by
  design, section 2).
- Derived-value overflow is closed (no silent infs): after every edit the refreshed segments must have finite
  derived values (`y1`/`y2`, `1/span`, normalized widths, with a 16x margin so every Bezier/slope/integral
  intermediate stays finite; hold segments only need finite end values). Otherwise the edit that caused it
  (`insert`, `remove`, `set_value`, `move_key`, `shift`, `set_side`/`set_user`/`set_handle`/`freeze`, `set_tan`,
  `set_broken`, `set_seg`, `scale_time`, `unwind`, `from_keys`, serde, legacy, fitter) returns
  `KeysError::NotFinite` and the track is unchanged. Consequence: key values must stay below `f64::MAX / 16`.
  Evaluation of a valid track never yields inf at a finite time: a Linear extrapolation that would overflow gives
  the end value, like a cycle overflow. `integrate` can still exceed the range for absurd magnitudes (value x time).
