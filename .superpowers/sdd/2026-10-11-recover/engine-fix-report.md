# Engine fix: growing Stroke lines + SSR/client number formatting

## Resume note
- Done: merge feat/engine (2cd33b9); commits 60b8680 (Stroke growth), abd77d8 (shared formatter), ee61e9a (empty Stroke placeholder NaN fix).
  Gates after ee61e9a: bun test 729 pass / 0 fail; check 0/0/0. Build + renderer size measured on abd77d8 (45 pages, 0 warnings, renderer 269,302 gz).
- Done after ee61e9a: build 45 pages / 0 warnings, renderer chunk (ViewportCard) 269,348 gz / 1,005,876 raw (<= 300,000); largest other NPanel 86,989; elk worker 432,138 (lazy, <= 600,000). Browser pass done (below).
- Next: nothing pending in this task.
- Uncommitted: nothing (this report and evidence folder only).

## Bug 1: Stroke kept its first segment count
Root cause: three's WebGL renderer caches `geometry._maxInstanceCount` of an `InstancedBufferGeometry` the first time it
binds it (WebGLBindingStates, only when undefined) and draws `min(instanceCount, _maxInstanceCount)`; only the
geometry's `dispose` event deletes it (WebGLGeometries). three-stdlib `LineGeometry.setPositions` replaces the
attributes with a fresh exact-size buffer but leaves the cache, so growth is cut back to the first count and shrinking
draws past the new buffer. It also allocated a Float32Array + interleaved buffer per call (per frame for beams).
Fix: `src/viz/scene/fatLine.ts` `writeLine(geo, flat, pairs)`: one instance buffer per line with power-of-two
capacity, writes in place, sets `instanceCount` to the real segment count, updates dash distances like
`computeLineDistances`, fills the unused tail with copies of the last segment (bounds/raycasts read the whole
buffer), refreshes bounds; grows (with `geo.dispose()` to drop the cache) only when the count exceeds capacity.
`Stroke` (view.tsx) uses it for its handle; empty `points` are supported (placeholder pair, emptied).
Test: `tests/js/fat-line.test.ts` emulates the renderer's cache rule; first case shows upstream setPositions stuck at 1
segment after growing to 10, the others show writeLine drawing exactly n-1 for 1..513 points, grow/shrink sequences,
no reallocation within capacity, tail bounds, dash distances, segment pairs, empty lists.
Workaround removed from waves-dft.impl.tsx (STICK/CIRCLE/TRACE/LIVE caps, `padded`, ZERO_* arrays, 1200-point limit).

### Audit of lines whose point count changes
Imperative (handle) writers, the only ones affected by the bug: Arrow shaft (2 pts), Segment (2 pts), 1.9 sticks /
orbit circles / trace / freehand stroke: all now via writeLine.
Declarative `points` (drei rebuilds a fresh geometry whenever the array identity changes, so a new count was always
drawn fully): unit-circle Curve (0.4 graph traces), FnCurve, Circle, AngleArc, RightAngle, Parallelogram, Poly edge,
basis, waves-dft lanes/target, Grid axes/ticks (SLine).
Not fat lines: vector-arithmetic paint stamps (Points + drawRange, preallocated 4096 ring), TileGrid (InstancedMesh
count + LineSegments drawRange, MAX_TILES), DotUnit Boxes (InstancedMesh count <= cap), Poly fill (useDynGeo keyed by
vertex count). None depends on `_maxInstanceCount`.

## Bug 2: hydration #418 from last-bit trig differences
Root: no shared formatter; every lesson had its own `toFixed` helper, and `toFixed`/`Math.round` of a value on a
display boundary (0.425 from cos sums) differ when JSC and V8 differ in the last bit.
Fix: `src/viz/fmt.ts` (`settle`, `fixed`, `trim`, `int`): settle on an absolute grid 6 decimals finer than the display,
then the exact `toFixed`/round; never prints -0. Routed through it: tex.showVal (Formula default), scrub.fmtField
(N-panel fields/chips), Num default, Ledger, tiles totals, demo rows, and all lesson formatters (basis, cosine-
similarity, cross-product, dot-product, length, projection, vector, vector-arithmetic, law-of-cosines, powers,
pythagoras, squares-and-plane, unit-circle, waves-dft, dotunit model). The 1.9 `fmt2` workaround is now the shared one.
Test: `tests/js/fmt.test.ts` (ulp-perturbed boundary values print identically; plain toFixed shown to differ).
Behaviour note: exact negative halves (-0.125 at 2 dp) now round away from zero like toFixed (old showVal used
Math.round toward +inf). NumTable's `toPrecision(8)` default already snaps and was left as is.

## Browser pass (one, headless Chrome/SwiftShader over CDP, production dist, port 4521)
Evidence: `engine-fix-evidence/` (verify.ts, serve.ts, run.log, report.json, PNGs).
- All 44 lesson/dev/start pages EN + RU (plus root): no exceptions, no React hydration error (#418), no console error
  besides the known favicon.svg 404 and THREE.Clock deprecation warning.
- 1.9 WD-4 epicycles: sticks S = 1, 11, 21, 63 (N-panel PageUp/PageDown) at t = 0.25 / 0.6 / 0.98: the trace grows
  with time and closes the full heart at S = 63; chain and orbit circles drawn for every S.
- 0.4 unit-circle: all 4 cards, cos/sin graph traces drawn up to the current angle.
- First run found a NaN bounding sphere: the one-point placeholder for an empty segments Stroke (orbit circles) gave
  LineSegmentsGeometry half an instance; fixed in ee61e9a (placeholder pair, emptied on mount).
- `astro preview` exited with code 255 twice under headless Chrome load (cause not investigated; unrelated to this
  change); the pass used a minimal Bun static server for dist (serve.ts).

## Open items
- astro preview (bun) exit 255 under load: unknown cause, worth a separate look.
- Residual risk by construction: two engine results straddling a grid point 1e-(d+6) (probability ~1e-8 per value).
