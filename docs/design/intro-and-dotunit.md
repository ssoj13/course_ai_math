# Intro module and the DotUnit system: build spec

Status: design for implementation (2026-10-11). Owner pages: `start/why-this-course`, `start/how-it-connects`
(module "Start here", first in the sidebar), with reuse in 1.4, 2.2, 2.4, 3.6/3.7, 10.x and the home page.
Builds on `docs/design/visual-language.md` (Task 6d Blender look: grey viewport, flat fills, hatch for negatives,
orange hot outline; **no space/holo/glass/floor/HUD**: those are cover art only since 2026-10-11),
`docs/design/lessons-law-of-cosines-dot-product.md` (DP-1..4 conventions, ids, `fmt`, drag/snap/magnet rules), the engine
contracts in `AGENTS.md` (one shared Canvas + drei Views, SSR-safe entry → lazy impl, `lessonStore` vals/shown/hot/step,
Formula `\t{id}{}` / `\s{key}`, NumTable ids `a0…/b0…/p0…/sum`, Timeline `step[id]`/phases, palette roles, Ruler/Dimension),
and the course completion map (P5 "tensor core").
Every number drawn by these scenes comes from one TS tensor core; the formulas, tables and code lines read the same values.

Contents: 1 Core: a procedural einsum scene · 2 The DotUnit asset (the dot preset) · 3 Compositions (presets) ·
4 The zoom ladder (intro overview) · 5 Mounting in the course · 6 Engine needs · 7 Acceptance checks · 8 Open questions.

## 1. Core: `scene = f(spec, values)`, a procedural einsum scene

The DotUnit is not an illustration. It is one renderer for one kind of computation, *multiply matching numbers, add
them up*, driven by a spec the reader can edit. Every "composition" in section 3 is a preset of this renderer; the
intro's choreography (section 4) is a fixed timeline played over the same procedural content.

### 1.1 Spec

```ts
interface EinsumSpec {
  /** Einsum operation with exactly two operands, e.g. "i,i->", "ij,j->i", "ij,jk->ik", "bij,bjk->bik", "bhqd,bhkd->bhqk". */
  op: string;
  /** Extent of every index letter that appears in `op`; editable, 1..8 by default (see LOD for larger). */
  shapes: Record<string, number>;
  /** Values of both operands, row-major in the operand's own index order. */
  values: { a: Float64Array; b: Float64Array } | { seed: number; range: [number, number]; step: number } | { preset: string };
  /** Arithmetic the readouts imitate; the scene always draws fp64 geometry, dtype changes the printed numbers only. */
  dtype: 'fp64' | 'fp32' | 'fp16';
  /** Order in which the summed index is accumulated: left-to-right (the drawn order) or pairwise (what BLAS/GPU do). */
  order: 'sequential' | 'pairwise';
  /** Optional memory-layout view: draws the flat buffer strip and the strides of each operand under the viewport. */
  layout?: boolean;
}
```

Rules on the spec (all enforced by `parseEinsum`, which throws a typed error with the bad token; no silent repair):

- Exactly two operands and one output, letters `a–z`, each output letter must appear in an operand, no letter repeated
  inside one operand (no traces), no ellipsis. Everything the course needs (dot, matvec, matmul, batched, attention,
  einsum lessons) fits; traces/diagonals are a 3.7 side note drawn with code only.
- **Kept indices** = the output letters. **Summed indices** = letters in the operands but not in the output.
  At least one summed index (a pure outer product has nothing to pass; 3.6 draws it as the degenerate case of a shaft of
  length 1, i.e. `op: "i,j->ij"` is allowed and each shaft has one tile).
- `shapes` must list every letter; the same letter in both operands means the same extent (that is the contraction).
- Values: explicit arrays (editable cells), a seeded generator (`mulberry32(seed)`, uniform on `range` quantized to
  `step`, so the demo numbers stay short: default `range [-3, 3]`, `step 1`), or a named preset (section 3). The
  seed/preset is part of the lesson store, so "Reset" reproduces the same numbers.

### 1.2 Index-to-axis assignment (the layout rule)

World axes are the engine's: x right, y up, z toward the viewer. Scene units: one tile = 1 unit, gap 0.25 between tiles,
gap 1.5 between stacked cubes (batch), 2.5 between groups (heads).

1. **Role of a kept index** comes from its position in the *first* operand where it appears, counted from the end:
   last letter = *column role* → **x**; second-last = *row role* → **y** (rows go down the screen, so row index 0 is the
   topmost row: the y coordinate is `-(i + 0.5)` relative to the block top); any earlier letter = *batch role*.
   A rank-1 operand's only letter has column role.
2. **Conflicts** (two kept letters want the same axis, e.g. `bhqd,bhkd->bhqk`: `q` and `k` are both row-role) are
   resolved in output order: the first letter keeps its axis, the next takes the first free axis of `[x, y, z]`.
3. **Summed indices** take the first free axis of `[x, y, z]` after the kept ones. That axis is the **shaft**: the
   direction along which the column passes the row and the tiles are laid before they stack. Two or more summed
   letters (`ijk,jk->i`) are flattened into ONE shaft in row-major order (last letter innermost) with a 0.5 tick gap
   between outer groups; the N-panel shows the flattened length.
4. **Batch-role letters** (output letters beyond the two spatial ones): the first becomes a **stack** (copies of the
   block along y, gap 1.5, index 0 at the bottom: the "stack of cubes"); the second a **row of stacks** along x
   (gap 2.5: "heads are rows of stacks"); a third and beyond are **slice selectors** (an N-panel field picks the index
   drawn, default 0; the caption says "slice b=0 of 4"). Nothing is drawn as a 4-D grid.
5. Output cells are placed at the kept coordinates; the stack of tiles of a cell grows along the cell's own normal:
   **+z** when the shaft is z (tiles fall from the far end of the shaft onto the C face), **+y** when the shaft is x
   (the dot preset: tiles drop into a bar next to the projection ruler), **+x** when the shaft is y.

Worked assignments (the five lesson presets):

| op | kept → axis | summed (shaft) | batch | picture |
|---|---|---|---|---|
| `i,i->` | none | i → x | – | one row (a), one sliding column (b), one stack: the DotUnit |
| `ij,j->i` | i → y | j → x | – | row view: the column x passes each row of W; outputs stack down the right |
| `ij,jk->ik` | i → y, k → x | j → z | – | the matmul cube; shafts run from the A face (left) through to the B face (top) and sums fall on the C face (front) |
| `bij,bjk->bik` | i → y, k → x | j → z | b → stack along y | stack of cubes, one per batch element |
| `bhqd,bhkd->bhqk` | q → y, k → x (conflict rule) | d → z | b → stack, h → row of stacks | attention scores: each (b, h) cube compares every query with every key |

The same function `layoutEinsum(spec) → Layout` returns, for every output cell: its world position, the ordered list of
`(a-index, b-index)` tile pairs with their shaft positions, and the stack direction. It is pure, three-free and
unit-tested against the table above (cell counts, shaft lengths, axis letters).

### 1.3 Values, numbers, provenance

- One small TS tensor core (`src/core/tensor.ts`, completion map P5): `Tensor { shape, strides, offset, data: Float64Array }`,
  `reshape/view/transpose/contiguous`, `einsum(op, a, b)` over strides (no materialized transposes), plus `dot`, `matmul`
  as thin wrappers. It records nothing about rendering. Tests: fixtures generated by NumPy
  (`tests/fixtures/einsum/*.json`: op, shapes, seed, expected output to 1e-12) for the five presets and ten random
  specs; `strides` tested against `np.ndarray.strides / itemsize`.
- The scene asks the core for `einsum(spec)` **and** for the per-tile products (`products(spec) → Float64Array` in
  shaft order), so the tile areas, the partial sums the reader sees while scrubbing, the NumTable cells, the Formula
  numbers and the CodeView line values are the same floats. No scene-side arithmetic beyond `fmt`.
- `dtype` rounds the printed numbers only: `fp32` through `Math.fround`, `fp16` through `numfmt.ts` (Branch Q reuses
  the vllm course helper). The N-panel shows `exact − shown` as a read-only row `err` once dtype ≠ fp64, so Q1 can
  show rounding without a new scene. Geometry (tile areas, stack heights) is always the fp64 value: a rounding error
  is a number in the panel, not a visibly wrong height.
- `order` changes the running-sum readout and the drop order of tiles: `sequential` drops tile 0, 1, 2…;
  `pairwise` drops pairs (0+1), (2+3)… into intermediate half-height stacks that then merge (the "GPU/BLAS" picture,
  used by the summation-order depth note in 1.4 and Q2). Both end at the same height; the readout shows where the
  fp16 result differs when it does.

### 1.4 LOD and the readability contract

| extent per drawn axis | tiles | numbers | shafts |
|---|---|---|---|
| ≤ 8 per axis, ≤ 512 output cells | individual tiles, hatch, drop animation | printed on every tile and cell | one shaft per cell, individually hoverable |
| ≤ 32 per axis, ≤ 4096 cells | tiles without numbers; a cell is a flat bar whose height is its value | on hover / N-panel for the hovered cell | shafts collapse to one thin line per cell |
| ≤ 64 per axis, ≤ 20 000 cells | heatmap faces (`vecC` for positive, hatched `vecB` for negative, intensity = \|value\| / max) | on hover only | none drawn; the pass is a sweeping plane |
| beyond | refused: the slider stops at 64 and the status bar says "Up to 64 per index on this device"; a preset above the cap loads clamped with the same message | – | – |

The level is chosen from the spec, not from the device: the reader always gets the same picture for the same spec
(no `hardwareConcurrency` branching, AGENTS rule). The hard cap keeps the instance count under the perf budget below.

### 1.5 Instancing and performance budget

- Tiles, legs and stack slabs are **one `InstancedMesh` per role color** (vecA legs, vecB legs, positive tiles,
  negative tiles, hot outline copies), so a scene draws ≤ 6 draw calls plus lines. Per-instance attributes:
  `progress` (0..1 of the tile's own drop/flash), `value` (for height/intensity in the shader), `hot` (0/1).
  The timeline writes `progress` for all instances in one buffer update per frame; nothing re-creates geometry during a
  scrub. Hatch for negatives is in the fragment shader (45° stripes in CSS px via `usePxScale`, same look as `Quad`).
- Budget: 60 fps at ~20 000 instances on a laptop iGPU (measured on the dev page with the 64×64 heatmap preset;
  acceptance in section 7). Demand frameloop stays: the timeline invalidates per frame while playing, drags per move.
- Numbers on tiles are DOM `Label`s only at LOD 1 (≤ 512 cells); beyond, hover shows one label.
- Memory: operand buffers are `Float64Array` of ≤ 64×64×64 (2 MB); nothing allocates per frame (module-level
  temporaries, `Vec::clear` discipline as in `view.tsx`).

### 1.6 Memory layout view (`layout: true`)

Under the viewport, a `MemoryStrip` (planned in Stage 3) shows each operand as a flat strip of cells in buffer order with
its `shape` and `strides` from the core; hovering a tile lights its two source cells on the strips and the strip cells
light the tile. Transposing an operand in the N-panel (`aᵀ` checkbox) swaps strides without moving data, which the
strip shows (same cells, different arrows). This is the 3.3 "strides over flat memory" depth level; it ships with 3.3,
not with the intro.

### 1.7 What stays illustrative

- The **camera choreography** of the intro (section 4) is a fixed timeline over the procedural content: the spec,
  values and LOD can change, the shot list cannot.
- The transition between the two faces of the DotUnit (section 2.3) is authored motion (where legs go, when the stack
  rises). It is parametric in `n` and in the values, but its phases are fixed.
- Draw-in and spring timings are the engine's (`DRAW_MS` 600, `SPRING_MS`, `VIEW_MS` 250).

## 2. The DotUnit asset = the `i,i->` preset with two faces

Default values (every composition below starts from them so the reader recognizes the numbers):
`a = (3, 1, 2)`, `b = (1, 2, -1)`, so `a·b = 3·1 + 1·2 + 2·(−1) = 3 + 2 − 2 = 3`, `|a| = √14 ≈ 3.742`,
shadow of b on a = `a·b / |a| = 3 / 3.742 ≈ 0.802`, and `|a| · shadow = 3` (the same 3).

### 2.1 Store and ids (lesson store `dotunit`; the mounting lesson may alias it)

| key / id | kind | meaning |
|---|---|---|
| `a`, `b` | vec3, draggable tips (snap 0.1, range ±4 per component) | the two arrows; N-panel vector rows |
| `a:x` `a:y` `a:z`, `b:x` … | derived legs (Arrow `legSegs`) | axis-colored components; hover lights the matching table column |
| `len_a` | ro | \|a\| |
| `shadow` | ro | b·â (signed length of the shadow of b on a's line) |
| `dot` | ro | a·b = Σ aᵢbᵢ, the number both faces end on |
| `p0`, `p1`, `p2` | tiles (NumTable product ids) | aᵢ·bᵢ as areas; negative = hatched |
| `sum` | ro / stack | Σ pᵢ; the stack's height |
| `face` | timeline `unit` steps 0..9 | which face / phase is shown (section 2.3) |
| `n` | int 2..8 (N-panel field) | dimension; 2 draws in the plane, 3 in 3D, >3 draws the legs/tiles only (face 2) and the projection face shows the first three components with a "+ n−3 more" caption |

Formula (under the card, `Formula` with `\t{}` ids): `\t{dot}{a\cdot b} = \t{len_a}{|a|}\,\t{shadow}{\text{shadow}} = \t{p0}{a_1 b_1} + \t{p1}{a_2 b_2} + \t{p2}{a_3 b_3}` with the live numbers on a second line (`\s{}` on `a`, `b` components is left to 1.4; the intro's formula is read-only). NumTable `op: 'product'` rows `a`, `b` gives the cells `a0…`, `b0…`, `p0…`, `sum`. CodeView (python) lines: `1: s = 0.0`, `2: for i in range(n):`, `3:     s += a[i] * b[i]`, linked ids `3 → p{current}` while the pass runs, `1 → sum` at the end.

### 2.2 Face 1: projection with sticks (3D)

- Viewport: 3D ViewportCard, Persp, home camera three-quarter (engine `fitDistance`), extent 4.5, grid on, axes on.
- `a` as vecA arrow, `b` as vecB arrow, both from O. A thin `muted` dashed line extends a's direction both ways
  (the "ruler line"). From b's tip a dashed `muted` perpendicular drops to that line (`Segment`, dashPx 6); the foot is
  a `Point` (size 5, vecC). The **shadow** is a `Segment` from O to the foot in vecC, 2.5 px core, drawn ON the line.
  A `RightAngle` sits at the foot. An `AngleArc` between a and b shows θ (label `θ = 63.6°` for the defaults).
- The **ruler**: a `Ruler` primitive along a's line, ticks every 0.5 unit from O, numbers at integers, so the shadow is
  read as a length (0.80).
- Read-only `Dimension` from O to a's tip labelled `|a| = 3.74` offset 14 px to the other side of the line.
- The stack bar (face 2) is drawn in face 1 too, but collapsed (height 0) at the foot's position, so that in the
  transition it rises in place.
- Overlays popover: `ruler` (default on), `angle` (on), `grid` (on), `legs` (off in face 1, on in face 2).
- Hover: `a` / `b` / `shadow` / `len_a` light their arrow or segment with the hot outline and the formula term; status bar
  hint `Drag the tip of b: the shadow follows; a·b = |a| × shadow`.

### 2.3 Face 2: legs, row, column, tiles, stack

Front orthographic view (camera at +z looking at the xy plane; y-up). Layout for `n = 3`:

- **Row** of a's legs along x at y = 0: `a:x` (axisX red, length 3), `a:y` (axisY green, length 1), `a:z` (axisZ blue,
  length 2), laid head-to-tail from x = −3 with 0.25 gaps, each a 2.5 px beam with a small head.
- **Column** of b's legs along y, same axis colors, standing at x = −4.5 (left of the row), head-to-tail from y = 0.5:
  lengths 1, 2, −1 (the negative leg points DOWN, hatched halo so the sign is not color-only).
- The column **slides right** along the row. When the leg of index i of the column is above the leg of index i of the
  row, the pair **flashes** (both legs hot for 120 ms) and a **tile** `p{i}` appears: a rectangle of width `|a_i|` and
  height `|b_i|`, positioned with its bottom-left at the row leg's start, filled with the owning axis color at the
  engine's 18 % fill and 1.5 px edge (`Quad` recipe), **hatched** when `a_i·b_i < 0`, number `aᵢ·bᵢ` centered as a
  `Label` (mono, `ink`). Area = |product|; the sign is the hatch.
- After the three tiles exist, each tile in turn **drops** into the **stack** at x = 4 (right of the row): a vertical
  bar of width 1 whose segments are the tiles re-shaped to width 1 and height `a_i·b_i` (positive segments stack up
  from y = 0, a negative segment hangs down from the current top as a hatched band with a down arrow: the LC-1 Ledger
  convention). The running sum label `sum` sits at the stack's top edge and updates on every landing.
- Next to the stack, the **projection ruler** from face 1 stands upright: a vecC bar of height `|a| × shadow`
  (the same vecC as the shadow), with its `Dimension` label `|a|·shadow = 3.00`. The stack's top and the bar's top
  are joined by a dashed `hot` horizontal tick when their heights agree (always, to `fmt` precision): this is the
  "aha" frame, and the status bar reads `Same height: the sum of the tiles is the shadow times |a|`.

### 2.4 Timeline `unit` (steps, durations, what is hot)

| step | text (EN) | duration / motion | hot / marks |
|---|---|---|---|
| 0 | Two arrows. | draw-in 600 ms (beams grow from tails) | – |
| 1 | Drop a perpendicular from b to a's line: that is b's shadow on a. | foot point and perpendicular ease in, 250 ms | `shadow` |
| 2 | Read the shadow on the ruler: 0.80. Multiply by \|a\| = 3.74: 3.00. That number is a·b. | ruler ticks fade in; `len_a` then `dot` light | `len_a` → `shadow` → `dot` (120 ms each) |
| 3 | Now forget the picture: write each arrow as three numbers. | camera smooth-views to front ortho (`VIEW_MS` 250); a and b split into their legs (legs ease out of the arrows along their own axes, 250 ms; the arrows fade to 35 %) | `a:x` `a:y` `a:z`, then `b:*` |
| 4 | a's legs lie down in a row; b's legs stand up in a column. | legs translate to the row/column positions (spring, 250 ms, staggered 60 ms per leg) | – |
| 5 | The column passes the row. Where matching legs meet, they multiply: a tile whose area is aᵢ·bᵢ. | column slides from x = −4.5 to x = +3 over 900 ms (`phases: 3`: one per pair; scrubbing the step moves the column) | `p0` → `p1` → `p2` at the flashes; table cells light with them |
| 6 | A negative times a positive is a negative tile: hatched, it will pull the stack down. | 300 ms hatch wipe on the hatched tile | the hatched tile |
| 7 | Add them up: the tiles drop into a stack. | tiles drop one by one, 220 ms each, 80 ms apart (`phases: 3`) | `sum` after each landing |
| 8 | The stack is exactly as tall as \|a\| × shadow. One computation, two faces. | projection bar rises at the stack's side (250 ms), the dashed tick appears, both labels show 3.00 | `sum` and `dot` together |
| 9 | Drag b. Everything above is recomputed from the same two arrows. | nothing animates; handles enabled (they are enabled at every step, a drag never resets the timeline) | – |

Steps 3–8 own `phases` so the Timeline's range slider scrubs inside them; `step['unit:max']` feeds the lesson's
challenges (1.4: "make the stack negative", "make the shadow zero").

### 2.5 Reduced motion

`prefers-reduced-motion: reduce`: every spring/tween is a cut (engine rule); the slide of step 5 becomes three
discrete positions, the drop of step 7 three cuts; the flash is a static hot outline on the current pair; nothing
blinks. All content stays.

### 2.6 Mobile layout

< 720 px: the ViewportCard is 16:10 full width; the N-panel becomes the wrapped chip row (rows: `a` vec3, `b` vec3,
`len_a`, `shadow`, `dot`, `sum`, `n`: chip model computes the reserved height; `SceneIsland rows` must receive the same
rows); the Timeline bar under the chips; the Formula uses `texNarrow` (two lines: `a·b = |a|·shadow` / `= Σ aᵢbᵢ = 3.00`).
Face 2 is laid out for a 16:10 box: the column starts at x = −4.5 and the stack is at x = 4 with extent 5.5; at
360 px the labels on tiles are 11 px mono (same as the mobile ruler). 3D face 1 orbits on horizontal swipes, vertical
swipes scroll (engine rule).

## 3. Compositions: presets of the same renderer

Each preset is a `{ spec, values, timeline }` triple in `src/viz/dotunit/presets.ts`. The DotUnit mechanics (row,
column, flash, tile, drop, stack) are reused unchanged; only the layout from section 1.2 and the camera differ. Numbers
below are the defaults, all derived from `a`, `b` above so the reader sees the same 3 again.

### 3.1 Matrix × vector, row view (`ij,j->i`, lesson 2.2 scene 2.2-2)

`W = [[3, 1, 2], [1, 0, 2]]`, `x = (1, 2, −1)`, `y = Wx = (3, −1)`.
Layout: rows of W along x (row 0 on top), x's legs as the column that passes **each row in turn** (phase per row; the
column stays one object, it passes row 0 then moves down to row 1); tiles per row; each row's stack drops into the
output cell `y_i` at the right (stack direction +x per section 1.2 rule 5 is overridden for the rank-1 output: the
cell draws its stack as a horizontal bar whose length is y_i, pointing right for positive, hatched left for negative,
so the output reads as a vector of numbers). Timeline `mv`: 0 "W is two rows: two dot products waiting for x" ·
1 "x passes row 0 → y₀ = 3" (phases 3) · 2 "x passes row 1 → y₁ = −1" (phases 3) · 3 "y = (3, −1): one pass per row".
Ids: `w00…w12` (NumTable matrix cells), `x0..x2`, `y0`, `y1`, tiles `p{i}{j}`.

### 3.2 Matrix × vector, column view (same spec, lesson 2.2 scene 2.2-3)

Same numbers, other grouping: `y = 1·(3, 1) + 2·(1, 0) + (−1)·(2, 2)`. The scene draws W's **columns** as vecC arrows
in the output plane, scales each by `x_j` (the arrow stretches, flips when negative, 250 ms), then lays them
head-to-tail; the sum arrow lands on `(3, −1)`, the same point the row view produced, and a `Dimension` reads its
components. The DotUnit tiles are not drawn here; instead the scene keeps the row-view stack bars visible at 35 % so
the reader sees both views agree (`y0`, `y1` hot in both). Timeline `cv`: 0 "the columns of W" · 1 "scale each column
by its x" (phases 3) · 2 "head to tail" (phases 3) · 3 "same y".

### 3.3 Matrix × matrix cube (`ij,jk->ik`, lesson 2.4 scene 2.4-2)

`A = W` (2×3), `B = [[1, 0], [2, 1], [−1, 2]]` (3×2), `C = AB = [[3, 5], [−1, 4]]`.
The cube: A on the left face (i down, j into depth), B on the top face (j into depth, k right), C on the front face
(i down, k right). Every (i, k) cell owns a **shaft** along z: the row `A[i, :]` and the column `B[:, k]` meet inside
the cube, tile by tile along j, and the tiles fall toward the viewer onto the C face, where the stack is drawn as a
square slab whose thickness is `C[i, k]` (hatched when negative). Camera: three-quarter from the front-right-top, Persp,
extent = max(shape) + 1.5. Timeline `mm`: 0 "A on the left, B on top, C in front" · 1 "one shaft = one dot product:
row 0 of A meets column 0 of B" (phases 3) · 2 "all shafts" (all four shafts run in parallel, 900 ms, phases 3) ·
3 "C is the front face" (camera smooth-views to front ortho; the cube's C face reads as a matrix of numbers) ·
4 "swap the operands: BA is 3×3, a different cube" (the preset swaps the operand buffers under the same op
`ij,jk->ik`, now with shapes i = 3, j = 2, k = 3; the layout function relays out and the camera refits). Ids: `a{i}{j}`, `b{j}{k}`, `c{i}{k}`, tiles
`p{i}{k}{j}`.

### 3.4 Tensors: stacks of cubes (`bij,bjk->bik`, lesson 3.6 scene 3.6-1)

Batch of 2: `A[0] = W`, `A[1] = W` with the signs flipped, `B` shared (broadcast: the spec is `bij,jk->bik`, which
section 1.1 allows; the N-panel shows `B` without a batch axis and the status bar says "B is reused for every batch
element"). Two cubes stacked along y with gap 1.5. Timeline `batch`: 0 "one cube per batch element" · 1 "the same
shafts run in every cube at once" (all shafts, 900 ms) · 2 "the batch axis never mixes: no shaft crosses from cube 0 to
cube 1". LOD: `b ≤ 8` stacks; beyond, the stack becomes a selector.

### 3.5 Einsum rule: the axis that disappears is the dot-product axis (lesson 3.7 scene 3.7-1)

The reader types or picks an op from a list (`i,i->`, `ij,j->i`, `ij,jk->ik`, `ij,ij->`, `ij,kj->ik`, `bij,bjk->bik`),
the scene re-lays out live (spring to new positions, 250 ms) and the status bar names the **summed letters**: "j is
summed: it is the shaft". The index letters are drawn as axis labels on the block edges in `muted` mono; the summed
letter's label is `hot` while the pass runs. `ij,ij->` (Frobenius inner product) shows that two summed letters flatten
to one shaft of length 6.

### 3.6 Attention scores (`bhqd,bhkd->bhqk`, lesson 10.x scene ATT-1; preview in 1.7)

`b = 1`, `h = 2`, `q = 2`, `k = 3`, `d = 2`; values from seed 7. Two cubes side by side (heads = row of stacks), each
comparing every query row with every key row: the C face is the 2×3 score matrix. The 1.7 preview mounts the same
preset with `h = 1` and the caption "each score is the dot product of one query embedding with one key embedding";
softmax and `·V` are 10.x scenes that chain after this one (the C face becomes the row operand of the next DotUnit:
`bhqk,bhkd->bhqd`).

### 3.7 Layer `y = x Wᵀ + b` (`td,od->to`; the ladder's rung 4 and lesson 9.x)

Tokens as rows of x (t = 2, d = 3), W as (o = 2, d = 3): the cube of a linear layer, plus the bias as a last
translation of the C face (a `Dimension` arrow per output cell). Numbers: `x = [[1, 2, −1], [0, 1, 1]]`, `W = A`.

## 4. The zoom ladder (intro scene INTRO-2, `start/how-it-connects`)

One card, 3D, a **fixed timeline `ladder`** of five rungs over procedural content; every rung's content is a live preset
from section 3 placed in the world at increasing scale, and the camera dollies out from rung to rung
(`VIEW_MS` 250 smooth view each, `springProgress`):

| rung | content (preset) | camera | caption |
|---|---|---|---|
| 1 | one tile `p0 = 3·1` | ortho front, extent 2 | "One multiply: a 3 by 1 tile." |
| 2 | the DotUnit face 2 (row, column, stack) with the same tile in place | extent 6 | "Three multiplies, added: a dot product. Its stack is as tall as the shadow times \|a\|." |
| 3 | matmul cube 3.3 with the DotUnit's shaft highlighted as shaft (0, 0) | three-quarter, extent 5 | "A matrix times a matrix: every cell is one such stack." |
| 4 | layer cube 3.7 (`y = xWᵀ + b`) | extent 6 | "A layer of a network: the rows are tokens, the columns are the layer's neurons." |
| 5 | attention 3.6 (`h = 2`) drawn as two cubes beside the layer cube | extent 12 | "A transformer block: stacks of such cubes. Every number inside is a stack of tiles." |

**Dive-in**: at any rung the reader can click a number (a cell on a C face, a stack top, a tile) and the camera
smooth-views to that object while the ladder rewinds to the rung that shows it as the whole scene (a cell of rung 5
→ rung 3 cube of that head → rung 2 shaft → rung 1 tile), with the clicked path kept hot. Implementation: cells carry
`data-rung` and a world anchor; `dive(id)` sets `step.ladder` and the camera target; the Timeline range reflects it; Escape
or "Back up" (header button) climbs one rung. Keyboard: the DragKeys-style button list exposes "dive into cell (i, k)".
The ladder's spec values are the section 2/3 defaults so the number 3 is visible at rungs 1, 2, 3 (cell C[0,0]) and 4
(y[0,0] = 3 before the bias).

Reduced motion: camera cuts between rungs; the dive is a cut too. Mobile: 16:10 full width, captions in the status bar
and under the card (Timeline step text), 5 rungs on the range slider.

## 5. Mounting in the course

| page | scene id | what is mounted |
|---|---|---|
| `start/why-this-course` | INTRO-1 | the DotUnit (section 2) playing its timeline `unit` once on scroll into view, then free; caption "This is the whole course in 10 seconds" |
| `start/how-it-connects` | INTRO-2 | the zoom ladder (section 4), rungs 1–5, dive-in |
| `start/how-it-connects` | INTRO-3 | the einsum playground (3.5) with the five preset buttons: the reader sees "the same renderer" before learning any of it |
| home page (`index.mdx` hero) | – | a static cover render of rung 5 (cover art may be the space style); the "Start here" button links to `lesson:why-this-course` |
| 1.4 dot product | DP-teaser | the DotUnit with scrubbable `a`, `b` (the `\s{}` formula of 1.4), both faces; DP-1..4 keep their own specs |
| 2.2 matrix as transformation | 2.2-2, 2.2-3 | row view 3.1 and column view 3.2 |
| 2.4 matrix × matrix | 2.4-2 | the cube 3.3 (the "3D cube" of the four views) |
| 3.6 tensor products / 3.7 einsum | 3.6-1, 3.7-1 | stacks of cubes 3.4, the playground 3.5 |
| 1.7 cosine similarity, 10.x attention | ATT-1 | attention scores 3.6 |
| 9.x layer | – | layer cube 3.7 |

Until the scenes exist the pages carry `{/* scene: INTRO-n — … */}` slots (already in place) and the completion map
lists INTRO-1..3 in the named scene register.

## 6. Engine needs (using existing primitives)

1. `src/core/tensor.ts` (P5): `Tensor`, strides, `einsum` for two operands, `products`; NumPy fixtures; three-free.
2. `src/viz/dotunit/einsum.ts` (three-free, pure): `parseEinsum(op)`, `layoutEinsum(spec)` (section 1.2), LOD choice
   (section 1.4), unit-tested against the preset table.
3. `src/viz/dotunit/Tiles.tsx`: `InstancedMesh` tiles/legs/stack slabs with per-instance `progress/value/hot`
   attributes and the hatch fragment shader; hot outline as a second instanced draw (engine's `STYLE.hotPad`). Uses
   `usePxScale` for px-stable hatch and edges; labels through the existing DOM `Label` registry (LOD 1 only).
4. `src/viz/dotunit/DotUnit.tsx`: the composite (faces, legs, row/column, ruler, stack, projection bar), built from
   `Arrow` (with `legSegs`), `Segment`, `Point`, `RightAngle`, `AngleArc`, `Ruler`, `Dimension`, `Quad` and `Tiles`;
   timeline-driven through `step['unit']` and phases; drag through the existing registry (`move` mappings).
5. Presets `src/viz/dotunit/presets.ts` and the per-lesson entries (`src/viz/lessons/how-it-connects.tsx` +
   `.impl.tsx`, SSR-safe pattern), rows modules for the chip model.
6. Timeline: no new API; `dive(id)` is a lesson-level function writing `step.ladder` and the card's `nav.viewDir/target`.
7. Status-bar hints and tooltips in both locales through `UiStrings` namespaces (`dotunit`, `ladder`).
8. Dev page `dev/dotunit` for pixel/perf evidence (instance count, fps, LOD boundaries).

## 7. Acceptance checks

Numbers (bun tests, no DOM):
- `einsum` equals the NumPy fixtures to 1e-12 for the five presets and ten random specs; `products` sums to `einsum`.
- For every drawn tile `p{…}` the tile's area equals `|a_i·b_i|` and its hatch flag equals `a_i·b_i < 0`.
- **Stack height == projection value**: for 200 random `a`, `b` (components in [−4, 4], step 0.1, |a| > 0),
  `sum(products) == |a| · (a·b/|a|)` to 1e-9, and the drawn stack height (from the layout) equals `sum` to `fmt`
  precision; the "aha" tick is shown iff the two formatted numbers agree (always).
- Row view and column view produce the same `y` (same core call, compared cell by cell).
- `layoutEinsum`: axis letters per the table in section 1.2; conflict rule on the attention spec; two summed letters
  flatten to one shaft of the product length; rank beyond stack/row becomes selectors; cap at 64 throws a typed error.
- LOD boundaries at 8/32/64 per axis and 512/4096/20 000 cells.

Browser (headless Chrome, CDP, production build, both locales, 1280/360 px), recorded under
`.superpowers/sdd/<task>/dotunit-evidence/`:
- Timeline `unit` step 8: pixel columns of the stack top and the projection bar top agree within 1 px; the status text
  is the "Same height" line.
- Hovering tile `p2` lights the table cell `p2`, the formula term `p2` and the CodeView line 3; hovering the cell lights
  the tile (one id, three places).
- Dragging `b` at step 9 never changes `step.unit`; the numbers in the N-panel, formula and table are identical strings.
- Reduced motion: the same final frames with no intermediate frames differing from the cuts.
- Perf: 64×64 heatmap preset and the 8×8×8 batched preset (≤ 20 000 instances) render at ≥ 55 fps median over 5 s
  on the reference laptop iGPU while scrubbing; no allocations per frame in the instanced update (memtrack-style counter
  in the dev page).
- Mobile 360 px: chip rows equal the chip model; 0 layout-shift entries during the timeline and a drag.
- Zoom ladder: all five rungs show the number 3 at the documented cell; dive from a rung-5 cell lands on the rung-1 tile
  with the path hot; Escape climbs back.

## 8. Open questions (for the operator)

1. Should `n` (dimension) in the intro be editable, or fixed at 3 with the editable version reserved for 1.4?
   (Spec: editable 2..8; the projection face shows three components.)
2. Stack direction for matrix × vector outputs (section 3.1 overrides rule 5 with a horizontal bar). Accept, or keep the
   vertical stack and rotate the output vector?
3. Attention preset in 1.7 (preview) vs only in 10.x. (Spec: preview with `h = 1`.)
