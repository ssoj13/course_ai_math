# Scene design: 0.5 Law of cosines and 1.4 Dot product

Status: design for implementation (2026-10-10, rev 3: signature scenes S1/S2; visual language v2 — Blender-grade ViewportCard with header, gizmo, N-panel and status bar — applied to every scene). Builds on `docs/design/part0-1-storyboard.md` (lessons 0.5, 1.4; primitives section 4; risks section 5), `docs/design/signature-illustrations.md` (S1 "the missing rectangles", S2 "one number, three faces"), `docs/design/visual-language.md` v2 (ViewportCard anatomy: header, navigation gizmo, N-panel property rows, status bar; always-dark viewport; Blender axis colours; hot = selection outline; light-beam strokes; hatch for negatives; motion rules) and the engine contracts in `AGENTS.md` (one shared Canvas + drei Views, lazy-entry authoring, `lessonStore` vals/hot/step, Formula `\t{id}{}` / `\s{key}`, palette roles, Task 6b drag registry, Task 6c AngleArc/RightAngle/AreaTile/Parallelogram/Label, Task 7 NumTable/Timeline/CodeView and Formula `marks`).
Every decision below is final unless it contradicts a measured engine fact; an implementer should not need to invent anything. Section 5 lists the few capabilities the listed contracts do not cover.

## 0. Conventions shared by both lessons

### 0.1 Store, ids, values

- One `lessonStore` per lesson: id `law-of-cosines` (0.5) and `dot-product` (1.4). All scenes, formulas, tables, N-panel rows, status hints and challenges of a lesson read the same store; scene-private keys carry a prefix (`ik_`, `nl_`, `a3`/`b3`) so two scenes never fight over a key.
- Angles in the store are **degrees** (what the reader scrubs and reads); every derived function converts with `rad = deg * Math.PI / 180` once. Store vectors are 2D `[x, y]` or 3D `[x, y, z]` (y-up, as the engine draws them).
- Term ids are the same string in the scene object `id`, the formula `\t{id}{...}`, the NumTable cell id, the CodeView line id, the N-panel row id and the ledger bar id. One id = one thing; a term that appears on both sides of an equation and must be struck independently gets a side suffix (`na2_l` / `na2_r`).
- `hot` is a set but the engine lights exactly one id at a time (`hotOn`). Where a hover should light several things, the things share the id (e.g. the side `b` and the formula term `b` do; the side `b` and the tile `b2` do NOT).
- Derived values are pure functions `(vals) => number` passed to `Formula derived` and reused by the scene through one `derive(vals)` helper per lesson (`src/viz/lessons/<slug>.impl.tsx`), so the number in the formula and the geometry on the stage are computed by the same code path.
- Orientation convention for 0.5 (used by every rectangle below): the triangle is stored counter-clockwise `C -> B -> A`; the outward normal of an edge is the **clockwise** rotation of its direction (`cw([x, y]) = [y, -x]`). Squares and strips are built on the outward side, so they never cover the triangle.

### 0.2 Readout formatting (`fmt` functions)

| kind | format | examples |
|---|---|---|
| length, area, dot product | 2 decimals, `-0.00` normalized to `0.00` (`+0` trick in `fmt`) | `5.00`, `0.00` |
| angle | integer degrees when `Number.isInteger(v)`, else 1 decimal; suffix `^\circ` in TeX, `°` in DOM | `60^\circ`, `63.4°` |
| cosine, N·L, brightness | 3 decimals | `0.500`, `-0.707` |
| undefined (zero vector) | derived returns `NaN`; `fmt` maps non-finite to `\text{--}` in TeX and `–` in DOM; the readout's caption says why ("angle needs two non-zero arrows") | `--` |

`\s{key}` only accepts numbers; `NaN` is a number, so the sentinel passes `expand` and `fmt` decides what is shown. No readout may ever print `NaN`, `Infinity` or an empty cell. All live numbers (N-panel fields, table cells, viewport labels, status bar) are set in JetBrains Mono with tabular figures.

### 0.3 Dragging, snapping, keyboard

- Grid snap 0.1 on every vector handle (engine default `SNAP`). Scalar scrubs: `ScrubSpec` per key (table in each lesson).
- **Magnet** (section 5, item 2): while a handle moves, an angle value within `zone` of a target angle sticks to the target; targets are `[0, 30, 45, 60, 90, 120, 135, 150, 180]` degrees, `zone = 2.5°`. Magnets apply to pointer drags and to keyboard nudges of the same handle (both go through the handle's `move`), never to a scrubbed number (the number is already exact). A magnet catch is an *indirect* change in the sense of the motion rules: the handle eases onto the target with the 180 ms spring; the reader's pointer is not fought (the value sticks, the glyph eases).
- Keyboard: every handle has its DragKeys button (arrows = 0.1, Shift = 1.0, PageUp/Down = z); every scrub value (formula `\s{}` handle or N-panel field) is a `role=slider` with arrows and PageUp/Down (x10); N-panel fields additionally scrub by horizontal drag (Ctrl = snap: angles to the magnet targets, lengths to 0.5; Shift = fine, 1/10 step) and accept typed values on click (Enter commits, Esc reverts, out-of-range values clamp); every Timeline is a range input plus Prev/Next buttons (ArrowLeft/Right on the slider). Challenges are reachable with the keyboard alone (verified by the success rules below, which only read store values).
- Touch: as the engine already does (hit = 14 px, scroll lock only on a hit; 3D stages orbit on horizontal swipes).

### 0.4 Layout

- Every scene is one **ViewportCard** (visual-language v2): header (28 px: `2D`/`3D` indicator, `Persp`/`Ortho`, `Overlays ▾`, `Reset view`, scene name right-aligned in muted mono), the dark viewport with the navigation gizmo top-right (3D) or the static XY corner (2D), the collapsible N-panel on the right (200 px; `N` key / chevron), and the status bar (22 px) with context hints. Per scene this document lists: header controls, the Overlays popover entries (each with its default), the N-panel rows (E = editable drag-to-scrub field, R = read-only flat row, V = vector row with one field per component, C = checkbox row) and the status-bar hints per pointer target.
- Desktop (>= 900 px content width): the card spans the lesson column, viewport 16:10. Under the card, side by side: Formula (block, left) and Timeline bar (`◀ step 2/12 ▶ ───●───`, right) with the step text under it; then the multi-column NumTable or the Ledger where a scene has one (DP-1, DP-3, LC-1); then the Challenge card. Single-column readouts are N-panel rows, never a separate table.
- < 720 px: the N-panel becomes a horizontally scrollable row of chips directly under the viewport (same ids, fields still scrubbable). 360 px: viewport keeps 16:10 at full width; then the chip row, Timeline bar, step text, Formula, NumTable/Ledger, Challenge, stacked. NumTables wider than the viewport scroll horizontally inside their own box (`overflow-x: auto`), never the page. 3D viewports get the "tap to interact" overlay (storyboard R9); the status bar then reads "Tap to interact".
- Stage `extent` is chosen per scene so the largest configuration reachable by the scrub ranges fits with a 10% margin (values given per scene).

### 0.5 Motion (visual-language rules, applied)

- Direct manipulation (drag, scrub) is 1:1 and never animated: pieces, tiles and numbers follow the pointer in the same frame.
- Indirect changes (timeline steps, keyboard nudges, magnet catches, "reset") ease with the critically damped spring (~180 ms settle); `Reset view` and a gizmo click move the camera like Blender's smooth view (~250 ms). Polygon pieces (section 5, item 10) tween their vertices with that spring; formula terms fade/slide into place over 180 ms; a cancellation draws its strike line left to right in 300 ms; `dim` = opacity 0.35 over 180 ms; `hidden` = `display: none`, no animation.
- One orchestrated moment per lesson: the lesson's first scene draws itself once on first view (LC-0: the triangle's sides grow from C, then the three squares unfold outward, 600 ms total; DP-1: `a` and `b` grow from the origin, 600 ms). Nothing loops, nothing idles.
- `prefers-reduced-motion: reduce`: every item above is a hard cut (0 ms); a struck term shows its full line immediately; the "draws itself" moment is skipped.

### 0.6 Challenge boxes

A Challenge (section 5, item 6) is a DOM card with the task text, a live status (`not yet` / `done`), and a hint behind a disclosure. `check(vals)` is a pure predicate on the store; it is evaluated on every store change. Success is sticky for the page visit (a reader who wanders off keeps the tick) and is written to the progress store as `challenge:<lesson>:<n>` so LessonDone can list it.

### 0.7 Visual language v2, applied to every scene below

- **The viewport is always dark** (both site themes), so every colour below is calibrated once. Page-side things (Formula, NumTable, Ledger, Challenge, three-faces strip) follow the Starlight theme.
- **Vectors and sides are light beams**: core stroke 2.5 px + 9 px halo at 14% alpha of the same colour. Arrow heads sharp, concave base. Points: 6 px discs with a 1.5 px dark rim. Triangle sides in 0.5 are beams too (they are the vectors of 1.4 in disguise; the lesson text says so at the end).
- **Hot = Blender selection**: a 2 px `hot` (orange) outline behind the core stroke + halo 30%, on the hovered/linked object; an object's own colour never changes. Wherever this document says an object is "lit" or "hot", it means this outline. Formula terms highlight with `hot` at 22% alpha; N-panel rows and table cells get a `hot` outline.
- **Areas**: fill 18% + 1.5 px edge. **Negative signed areas are hatched** (45°, in the owning colour): sign is never colour-only. Every rectangle, strip and tile here is signed; "hatched" always means "negative".
- **Shadows (projections)**: a soft 6 px `muted` band (35% alpha) on the target line under a dashed `muted` drop line. Wherever the text says "shadow of b on a", the scene draws exactly that.
- **Angle arcs**: 1.5 px arc + 10% sector; value on the bisector in mono.
- **Grid and axes**: drei shader `Grid` fading with distance; axes in Blender colours (`axisX` red, `axisY` green, `axisZ` blue); component legs use the axis colour of their axis. Gizmo top-right in 3D (click = align, 250 ms); 2D shows the static XY corner.
- **N-panel**: the linked numbers view. Rows carry term ids (hover → outline in the scene + formula term); editable fields are Blender number fields (`field` grey, drag-to-scrub, click-to-type); read-only values are flat rows; vector values are one row with x/y/z sub-fields; booleans are checkbox rows. Separators with a muted caps label group rows.
- **Status bar**: one line, updated on pointer-over of any handle/object and during a timeline motion; idle text lists the navigation. Texts are given per scene.
- **Tooltips**: on N-panel rows and formula terms, 500 ms delay, title + one line + the TeX of the term.
- **Lit sphere**: unlit custom shader, `color = albedo * max(0, dot(N, L))`, raw-N·L toggle tints negatives cool (DP-4).
- Colours by role only: `vecA` sky blue, `vecB` orchid, `vecC` bluish green, `light` yellow, `hot` orange (outline only), `muted`/`ink` for shadows and labels, `axisX/Y/Z`; never hex in a lesson. (Rev-2 names `shadow`, `cobalt`, `amber`, `mint`, "HUD chip" are gone.)

---

## 1. Lesson 0.5 — Law of cosines = Pythagoras + a correction term

Scene order on the page: **LC-0** (the missing rectangles, the lesson's signature and first scene), **LC-1** (the ledger explorer: the same equation as numbers, through 0°, 90°, 180°), **LC-2** (the algebraic proof timeline, for the reader who wants to see `−2ab cos θ` fall out of a square), **LC-3** (two-bone IK, the CG anchor). They share the triangle (`a`, `b`, `th`), so a triangle dragged in LC-0 is the one the ledger counts and the proof expands. Nothing is drawn twice: squares and strips live only in LC-0; the ledger bars only in LC-1; the right-triangle tiles only in LC-2.

### 1.1 Store (`law-of-cosines`)

| key | type | initial | range / step | written by |
|---|---|---|---|---|
| `a` | number | 3 | [0.5, 4] / 0.1 | `\s{a}` scrub, vertex B drag, Num in prose |
| `b` | number | 4 | [0.5, 4] / 0.1 | `\s{b}` scrub, vertex A drag (radial) |
| `th` | number (deg) | 60 | [0, 180] / 1 | `\s{th}` scrub, vertex A drag (angular, magnet) |
| `ik_hand` | [x, y] | [2.5, 1.5] | stage extent, snap 0.1 | hand target drag |
| `ik_l1` | number | 2 | [0.5, 3] / 0.1 | Num in prose |
| `ik_l2` | number | 1.5 | [0.5, 3] / 0.1 | Num in prose |
| `ik_flip` | number (0/1) | 0 | toggle | Toggle "elbow up / down" |
| `step` timelines | `cut` (LC-0) 0..11, `proof` (LC-2) 0..9 | 0 | | Timeline; the Timeline also writes `step['cut:max']` / `step['proof:max']` (highest step seen) |

Derived (one `derive(vals)`, all signed):
- triangle: `rad`, `cos`, `sin` (with `|cos| < 1e-12 -> 0`, so 90° is exact), `C = [0, 0]`, `B = [a, 0]`, `A = [b cos, b sin]`, `c = |A − B|`, `a2`, `b2`, `c2`, `corr = −2 a b cos`, `pyth = a2 + b2`;
- shadows: `p = b cos` (shadow of b on a, point `D = [p, 0]`), `q = a cos` (shadow of a on b, point `E = q·u`, `u = [cos, sin]`), `h = b sin` (height from A), `DB = a − p`, `EA = b − q`;
- the c side: `w = (A − B) / c` (B to A), `m = cw(w)` (outward), `FB = (C − B)·w` (= a cos B, signed), `AF = c − FB` (= b cos A), `F = B + FB·w`;
- rectangles (areas): `strip_a = a·p`, `strip_b = b·q` (both equal `a b cos`), `R1 = a·DB`, `R2 = b·EA`, `R1c = c·FB`, `R2c = c·AF` (`R1 === R1c`, `R2 === R2c` up to rounding; the scene shows both numbers);
- IK: `d = |ik_hand|`, `cosraw = (l1² + l2² − d²) / (2 l1 l2)`, `cosc = clamp(cosraw, −1, 1)`, `phi = acos(cosc)` (deg), `beta` (shoulder offset, 1.5), `reachMin = |l1 − l2|`, `reachMax = l1 + l2`.
- When `c < 1e-9` (A on B): `w`, `m`, `F` are undefined; derive returns `w = [0, 1]`, `m = [1, 0]`, `FB = AF = 0` and sets the flag `degenerate = true` that LC-0 uses to hide the c² square and its split (nothing divides by `c` elsewhere).

### 1.2 Scene LC-0 — "The missing rectangles" (S1, lead scene)

**Purpose.** Pythagoras fails for a corner that is not square by exactly two rectangles, and each rectangle is a side times the shadow of the other side — the same area twice. The reader *cuts* them out of the squares and watches the remainders move into the square on c (the Euclid I.47 motion, which here works for any triangle). Aha, three times: (1) the two strips have the same area although their shapes differ; (2) at 90° both strips are gone and the motion is Pythagoras' own proof; (3) the strip's area has a name — a·b — so the law of cosines is `|a − b|² = |a|² + |b|² − 2 a·b` seen as paper.

**Stage.** 2D, `extent = 8` (a = b = 4 at 60°: squares reach ±5.5; at 180° the strips reach x = −4 beyond C and the c² square (side 8) spans 8 outward), view centred on `[1.5, 0]`. Grid step 1 fading with distance (drei shader Grid), axes off. This scene draws itself on first view (0.5).

**ViewportCard**: see the block after the objects table.

**Objects.** Roles: everything "of a" is vecA (sky blue), "of b" vecB (orchid), "of c" vecC (bluish green); shadow bands, drops and feet `muted`; the step's focus object gets the hot outline, never a recolour.

| id | primitive | geometry (CCW triangle, outward = `cw(edge)`) | role / fill | label | drag |
|---|---|---|---|---|---|
| `C`, `B`, `A` | Point | `[0,0]`, `[a,0]`, `[b cos, b sin]` | ink | `C`, `B`, `A` | `B`: x only, writes `a = clamp(snap(x), 0.5, 4)`; `A`: polar, writes `b = clamp(hypot, 0.5, 4)`, `th = magnet(deg(atan2), targets)` clamped to [0, 180] (y < 0 folds to 0) |
| `a`, `b`, `c` | Segment (beam) | C→B, C→A, B→A | vecA, vecB, vecC | `a`, `b`, `c` on the outward side, offset 0.35 | — |
| `th` | AngleArc | at C, a→b, r = 0.6, sector 10%, ticks at magnet targets | ink | `θ = 60°` on the bisector | — |
| `rt` | RightAngle | at C when `th === 90` | ink | — | — |
| `sq_a` | Poly (section 5, item 10) | `[C, B, B + a·n_a, C + a·n_a]`, `n_a = [0, −1]` | vecA 16% | `a² = 9.00` (centre) | — |
| `sq_b` | Poly | `[A, C, C + b·n_b, A + b·n_b]`, `n_b = cw(−u) = [−sin, cos]` | vecB 16% | `b² = 16.00` | — |
| `sq_c` | Poly | `[B, A, A + c·m, B + c·m]`; hidden when `degenerate` | vecC 16% | `c² = 13.00` | — |
| `drop_a` | Segment dashed | A→D | muted | — | — |
| `p` | Segment band (6 px) | C→D on the x line; the band is drawn on the triangle side (y = 0) | muted; hot outline when lit | `b cos θ = 2.00` | — |
| `drop_b` | Segment dashed | B→E | muted | — | — |
| `q` | Segment band | C→E on the line of b | muted | `a cos θ = 1.50` | — |
| `strip_a` | Poly | `[C, D, D + a·n_a, C + a·n_a]` (x from 0 to `p`, height a); hatched when `p < 0`; overflows the square when `p > a` | vecA 28% (darker than the square so it reads as "cut"), hatched if negative | `a · b cos θ = 6.00` | — |
| `strip_b` | Poly | `[C, E, E + b·n_b, C + b·n_b]`; hatched when `q < 0` | vecB 28% | `b · a cos θ = 6.00` | — |
| `R1` | Poly, moving | at rest: `[D, B, B + a·n_a, D + a·n_a]` (x from `p` to `a`); hatched when `DB < 0` | vecA 16%, edge 1.5 px | `a · DB = 3.00` | — |
| `R2` | Poly, moving | at rest: `[E, A, A + b·n_b, E + b·n_b]` (along u from `q` to `b`); hatched when `EA < 0` | vecB 16% | `b · EA = 10.00` | — |
| `drop_c` | Segment dashed | C→F (the altitude), extended across `sq_c` as a thin dashed split line F→F + c·m | muted | — | — |
| `R1c` | Poly (outline only until step 7) | `[B, F, F + c·m, B + c·m]`; hatched when `FB < 0` | vecC 0% fill, dashed edge | `c · FB = 3.00` | — |
| `R2c` | Poly (outline) | `[F, A, A + c·m, F + c·m]`; hatched when `AF < 0` | vecC outline | `c · AF = 10.00` | — |
| `D`, `E`, `F` | Point small | feet of the shadows / altitude | muted | `D`, `E`, `F` | — |
| `rtD`, `rtE`, `rtF` | RightAngle | at the feet, shown with their drop | muted | — | — |

**ViewportCard.** Header: `2D` (3D disabled) · `Ortho` (fixed) · `Overlays ▾` · `Reset view` (recentres `[1.5, 0]`, extent 8) · name `LC-0 · Missing rectangles`. Wheel zooms, MMB / two-finger pans the 2D view.
Overlays (default): grid (on), labels (on), angle (on), shadows = drop lines + bands + feet + right-angle marks (on), squares `sq_*` (on), strips (on), pieces `R1`/`R2` and their targets (on), altitude `drop_c` (on). Turning an overlay off hides its objects but never changes the store or the step.
N-panel rows (ids; E editable field, R read-only): `a` E [0.5–4, 0.1] · `b` E · `th` E [0–180, 1; Ctrl snaps to the magnet targets] · `c` R · separator AREAS · `a2` R (`a²`) · `b2` R · `strip_a` R (`a·(b cos θ)`) · `strip_b` R (`b·(a cos θ)`) · `R1` R (`a·DB`) · `R2` R (`b·EA`) · `R1c` R (`c·FB`) · `R2c` R (`c·AF`) · `c2` R · `ab` R (`a·b`, from step 11). Rows appear with the step that introduces them (same reveal as formula lines). Hovering `strip_a` outlines the strip, its label, the band `p` and lights the formula term `strip_a`.
Status bar: over A — "LMB drag A · b and θ follow · Ctrl snap θ"; over B — "LMB drag B along a · sets a"; over a strip — "Strip a × (b cos θ) = 6.00 · hatched = negative"; over a piece — "Leftover a·DB = 3.00 · moves at steps 6–9"; over a square — "a² = 9.00"; during a piece motion — "Step 7/12 · ◀ ▶ step · drag A or B any time"; idle — "Wheel zoom · MMB pan · N panel · ◀ ▶ step".

**Formula (block, `aligned`, lines revealed by marks; `\t{}` always preceded by a space after `\\`).**

```
\t{c2}{c^2} &= \t{R1}{a\cdot DB} + \t{R2}{b\cdot EA} \\
 &= \bigl(\t{a2}{a^2} - \t{strip_a}{a\cdot b\cos\theta}\bigr) + \bigl(\t{b2}{b^2} - \t{strip_b}{b\cdot a\cos\theta}\bigr) \\
 &= \t{a2}{a^2} + \t{b2}{b^2} \t{corr}{- 2\,a\,b\cos\theta} \\
\s{c2} &= \s{a2} + \s{b2} - 2\cdot\s{strip_a}
```

Scrubs: `a`, `b` (`{min: 0.5, max: 4, step: 0.1}`), `th` (`{min: 0, max: 180, step: 1}`) in a one-line formula above the block: `a = \s{a},\; b = \s{b},\; \theta = \s{th}`.

No separate NumTable: the AREAS rows of the N-panel are the numbers view of this scene.

**Timeline `cut`** (12 steps). "Lit" = the Timeline sets `hot` to the step's focus id (a reader hover overrides; on pointer-out the step's light is restored, section 4). Piece motion uses the spring; reduced motion = hard cut to the step's final placement. At any step the reader may drag A or B; pieces are recomputed from the store in their current placement (the motion phases are functions of the store, so a drag during step 6 keeps the rotated piece consistent).

| step | text | scene | formula marks | N-panel rows revealed / lit |
|---|---|---|---|---|
| 0 | "Three squares, one on each side. Pythagoras promises a² + b² = c² — but only for a square corner. Here 9 + 16 is not 13. Twelve units of area are missing. Where did they go?" | `sq_a`, `sq_b`, `sq_c` with area labels; lit: `sq_c` | all hidden | `a2`, `b2`, `c2` |
| 1 | "Shine a light straight down onto side a. Side b casts a shadow of length b cos θ (lesson 0.4: cos is the shadow of a length-1 stick)." | `drop_a`, `p` band, `D`, `rtD` appear; lit: `p` | hidden | — |
| 2 | "Cut a strip of that width off the square on a. Its area is a × (b cos θ)." | `strip_a` appears inside `sq_a` with a 180 ms fill-in from C toward D (vertex tween from zero width); `sq_a`'s label moves to the remainder; lit: `strip_a` | hidden | `strip_a` |
| 3 | "Now light side b instead: a casts a shadow a cos θ on it. Cut that strip off the square on b. Its area is b × (a cos θ) — the same number. Two different rectangles, one area: a·b·cos θ." | `drop_b`, `q`, `E`, `rtE`, then `strip_b` fills in; lit: `strip_b`; the N-panel rows `strip_a` and `strip_b` pulse once together (hot outline 300 ms; cut under reduced motion) | hidden | `strip_b` |
| 4 | "What is left of the two squares: a rectangle a × DB and a rectangle b × EA." | `R1`, `R2` get their labels; `sq_a`, `sq_b` outlines dim to 8%; lit: `R1` then `R2` on hover; default `R1` | hidden (line 1 claims c² = R1 + R2, which step 9 shows) | `R1`, `R2` |
| 5 | "The square on c can be split too: drop the height from C onto c and extend it. Two rectangles, c × FB and c × AF." | `drop_c`, `F`, `rtF`, the split line across `sq_c`; `R1c`, `R2c` outlines with labels; lit: `drop_c` | hidden | `R1c`, `R2c` |
| 6 | "Watch the leftover rectangle on a move. First it leans: sliding the top edge sideways keeps the base and the height, so the area does not change." | motion phase 1: `R1` vertices tween to the parallelogram `[B, B + a·n_a, B + a·n_a + (A − B), A]` (base B→B+a·n_a kept, far side slid so the slanted side is B→A) | hidden | — |
| 7 | "Then it turns a quarter turn around B. Turning changes nothing about area." | phase 2: the parallelogram rotates **clockwise 90° about B** (vertex tween along the arc, 180 ms spring): `B + a·n_a → C`, `A → B + c·m`; it now lies on side c with slanted side B→C | hidden | — |
| 8 | "Lean it back upright: it is exactly the rectangle c × FB. The leftover of a has become the first part of c²." | phase 3: shear to `[B, F, F + c·m, B + c·m]`; `R1c` outline fills (vecA 18% keeps the piece's colour inside the green square); lit: `R1c` | hidden (line 1 appears at step 9) | row `R1c` lights and shows `= R1` |
| 9 | "The leftover of b does the same dance around A (lean, quarter turn counter-clockwise, lean) and lands as c × AF. Together they fill the square on c: c² = a·DB + b·EA." | `R2` runs its three phases as one chained spring sequence (3 × 180 ms; reduced motion: one cut): shear to `[A, A + b·n_b, A + b·n_b + (B − A), B]`, rotate **counter-clockwise 90° about A** (`A + b·n_b → C`, `B → A + c·m`), shear to `[F, A, A + c·m, F + c·m]`; lit: `R2c` | line 1 visible, `R1`, `R2` lit | `R2c` |
| 10 | "Count the paper. The leftovers were the squares minus the strips. So c² = a² + b² − (strip) − (strip) = a² + b² − 2·a·b cos θ. Drag θ to 90°: the shadows shrink to a point, the strips vanish, and this is Pythagoras — in fact, the dance you just watched is Euclid's own proof of it. Drag past 90°: the shadows point backwards, the strips flip to hatched (negative area) and get *added*." | strips, `p`, `q` lit in turn; at `th === 90` `rt` appears and both strips are gone; for `th > 90` strips are hatched outside the squares beyond C and `R1`, `R2` overflow their squares by the strip; lit: `corr` | lines 2–3 visible; `corr` lit; at `th === 90` `strip_a`, `strip_b`, `corr` `dim` and a caption "= Pythagoras" | all rows |
| 11 | "One more look at the strip: its area is the length of side a times the shadow of side b on it. In the next lessons the sides become arrows, written in bold **a**, **b**, with lengths |**a**|, |**b**|; the strip is |**a**| (|**b**| cos θ). Lesson 1.4 gives that product a name — the dot product **a**·**b**, a new meaning of the dot (not plain multiplication) — and shows it needs no angle at all. So the law of cosines reads |**a** − **b**|² = |**a**|² + |**b**|² − 2 **a**·**b**: a Pythagoras with two rectangles cut out." | `strip_a` lit; N-panel row `ab` appears; both strips' labels switch to `a·b = 6.00` | line 4 visible with live numbers; a fifth line appears: `\t{ab}{a\cdot b} = \t{strip_a}{\lvert a\rvert\,(b\cos\theta)} = \s{strip_a}` | `ab` row added |

**Hover map.**

| hot id | scene | formula | N-panel |
|---|---|---|---|
| `a`, `b`, `c` | the beam side | `\s{a}` / `\s{b}` handles | field |
| `th` | arc + sector | `\s{th}` | field |
| `p` / `q` | band + drop + foot | the `b\cos\theta` / `a\cos\theta` factor inside `strip_a` / `strip_b` (nested `\t{p}{b\cos\theta}` inside `\t{strip_a}{a\cdot \t{p}{b\cos\theta}}`; inner span wins per `closest`) | — |
| `sq_a` / `a2` | square outline + label | `a^2`; `\s{a2}` | row |
| `strip_a` / `strip_b` | the strip (outline + fill 28% → 40%), its label, its band | the term | row |
| `R1` / `R2` | the moving piece wherever it currently is | the term (line 1, 2) | row |
| `R1c` / `R2c` | the outline / filled target in `sq_c` | — | row |
| `corr` | both strips + both bands | the correction term | rows `strip_a`, `strip_b` |
| `ab` | `strip_a` | line 5 | row `ab` |

**Edge cases (all signed, never blank).**
- `th = 90`: `p = q = 0` exactly (derive rounds); `strip_a`, `strip_b` are zero-width polygons (Poly hides a polygon whose area is < 1e-9 and shows the label `0.00` at the foot); `D = E = C`; `R1 = sq_a`, `R2 = sq_b`; steps 6–9 run the classical I.47 motion; `rt` at C.
- `th > 90`: `p, q < 0`: strips hatched, placed beyond C (x ∈ [p, 0] for `strip_a`), `R1` spans x ∈ [p, a] (square + hatched strip, i.e. `R1 = a² + |strip|`); `F` may fall beyond B or A only if an angle at B or A is obtuse (cannot both happen; derive handles signs).
- `p > a` (angle at B obtuse; e.g. a = 1, b = 4, θ = 30°): `strip_a` overflows the square (drawn at true size, overflow part dashed-edged), `DB < 0` → `R1` hatched beyond B, `FB < 0` → `R1c` hatched beyond B on side c; the identities hold with signs and the step-4 text gets an appended sentence: "Here the strip is wider than the square: the leftover is negative (hatched) and counts as a debt."
- `th = 0`: A on the x line; if `a === b` the triangle is a doubled segment, `c = 0`, `degenerate`: `sq_c`, `R1c`, `R2c`, `drop_c` hidden; step texts 5–9 get the appended sentence "c = 0: there is no square on c; move A to continue." Nothing throws (`w` has a fallback and no division by `c` occurs in placements because `F = B + FB·w` with `FB = 0`).
- `th = 180`: `p = −b`, `q = −a`, `c = a + b`; strips hatched, `sq_c` has side `a + b` and `F` lies between (FB = a).
- Dragging during a motion phase: pieces are pure functions of (store, step), so the drag reshapes the piece in place; no animation is triggered by a drag.

**360 px.** The scene keeps 16:10; the N-panel becomes the chip row under it (fields still scrubbable); the formula's four lines wrap by KaTeX — line 2 is the widest; at 360 px it is rendered with `\\` after the first bracket (an alternate TeX string selected by a `narrow` prop on Formula based on the container width, section 5 item 12). Chip labels are the short forms (`a·(b cos θ)` → `a·p`).

**Challenges.**
1. "Make both strips disappear." Success: `th === 90`.
2. "Make the strips negative (hatched) and watch c grow past √(a² + b²)." Success: `th > 90`.
3. "Find a triangle where the strip is wider than its square." Success: `p > a + 1e-9 || q > b + 1e-9` (hint: a short a, long b, small θ).
4. "Make the two strips *look* the same, not just measure the same." Success: `|a − b| < 1e-9` (a = b: the strips are congruent; the card explains that for a ≠ b they only share the area).

### 1.3 Scene LC-1 — The ledger (the equation as numbers)

**Purpose.** The same equation as an area balance: `a²` and `b²` stack up, the correction is a signed band, `c²` is the result. Where LC-0 showed *shapes*, LC-1 shows the *bookkeeping* and lets the reader sweep θ from 0° to 180° and read c continuously. Aha: the band shrinks to a line at 90° and the formula visibly *becomes* Pythagoras; past 90° it flips and `c` overtakes √(a² + b²).

**Stage.** 2D, `extent = 5.5`, centred on `[1.5, 1]`. Grid step 1, axes off. Only the triangle: no squares, no strips (those are LC-0's).

**ViewportCard.** Header: `2D` · `Ortho` · `Overlays ▾` · `Reset view` · `LC-1 · Ledger`. Overlays: grid (on), labels (on), angle (on), shadow `p` + `drop_a` (off — shown on hover of `corr` regardless), right angle (on).
N-panel: `a` E · `b` E · `th` E (Ctrl snaps to magnet targets) · `cos` R (`cos θ`, 3 decimals) · `c` R · separator AREAS · `a2` R · `b2` R · `corr` R (`−2ab cos θ`, hot outline while `th === 90` reads `0.00` and the caption shows) · `c2` R.
Status bar: A/B as in LC-0; over the ledger band — "Correction −2ab cos θ · zero at 90° · adds past 90°"; idle — "Wheel zoom · MMB pan · N panel".

**Objects.** `C`, `B`, `A` (same ids, same drags as LC-0), `a`, `b`, `c` beams, `th` arc with magnet ticks, `rt` at 90°; plus `p` band + `drop_a` shown only while `hot` has `p` or `corr` (the shadow is the reason the band exists).

**Ledger (DOM SVG, section 5 item 5)**, right of the formula. A stacked column, width 56 px, fixed scale 3.4 px per unit (max reachable `16 + 16 + 32 = 64` units = 218 px; fixed, not auto-fit, so bars visibly grow when the reader drags). From the bottom: `a2` (vecA), `b2` (vecB), then `corr`: when `corr < 0` a hatched band of height `|corr|` sits between the `c2` rule and the top of `b2` with a down arrow glyph; when `corr > 0` a solid vecC-tinted band of height `corr` is added on top with an up arrow; when `corr === 0` the band disappears and a thin `light`-coloured line labelled `= Pythagoras` is drawn at the top. A horizontal rule at height `c2` labelled `c²` is the result.

**Formula (block).**

```
\t{c2}{c^2} = \t{a2}{a^2} + \t{b2}{b^2} \t{corr}{- 2\,a\,b\cos\theta} \\[2pt]
\s{c2} = \s{a2} + \s{b2} \t{corr}{\;-\; 2 \cdot \s{a} \cdot \s{b} \cdot \s{cos}} \\[2pt]
c = \s{c}
```

Scrubs `a`, `b`, `th` as in LC-0 (`th` in the line `\theta = \s{th},\; \cos\theta = \s{cos}`). When `th === 90`: `corr` gets `dim` and the caption "= Pythagoras" appears (in `light`).

No separate NumTable (the N-panel AREAS rows are the numbers view); the Ledger sits right of the formula.

**Hover map.** `a`/`b`/`c`: beam + scrub handle + N-panel field. `th`: arc + `\s{th}`, `\s{cos}` + field. `a2`/`b2`: formula term + row + ledger bar. `c2`: `c^2`, `\s{c2}`, row, ledger rule. `corr`: band `p` + drop appear, the correction term, row, ledger band.

**Edge cases.** `th = 0`: `c = |a − b|`; if `a === b` then `c = 0`: side `c` has zero length (nothing drawn), N-panel rows read `0.00`, ledger rule at the bottom; arc has zero sweep. `th = 180`: `corr = +2ab`, solid band on top, rule at the very top. `th = 90`: exact zero (derive). Drag limits as LC-0.

**Challenges.**
1. "Make the correction disappear without touching a or b." Success: `th === 90`.
2. "Make c longer than a + b." Impossible by design; success fires on `th === 180` with the text "c ≤ a + b always: at 180° you reach equality (the triangle inequality)".
3. "Make c = 5 with a = 3, b = 4." Success: `a === 3 && b === 4 && th === 90`.

### 1.4 Scene LC-2 — The algebraic proof timeline

**Purpose.** For the reader who wants the formula to come out of symbols, not scissors: the same hidden right triangle, Pythagoras once, and `−2ab cos θ` falls out of expanding `(a − b cos θ)²`. Aha: step 8, `b² cos²θ + b² sin²θ` collapses to `b²`, leaving one term — the strip of LC-0, twice.

**Stage.** 2D, same triangle objects as LC-1 (same ids, same store), `extent = 6`, centred on `[1.5, 1]`. The reader can drag A/B at any step; dragging does not change the step.

**ViewportCard.** Header: `2D` · `Ortho` · `Overlays ▾` · `Reset view` · `LC-2 · Proof`. Overlays: grid (on), labels (on), angle (on), shadow + height (`p`, `drop_a`, `D`, `rtD`; on), tiles (on), `rest` segment (on).
N-panel: `a` E · `b` E · `th` E · separator PIECES · `p` R (`b cos θ`) · `rise` R (`b sin θ`) · `rest` R (`a − b cos θ`) · `rest2` R (`(a − b cos θ)²`) · `rise2` R (`(b sin θ)²`) · `c2` R — PIECES rows hidden until their step.
Status bar: A/B as in LC-0; over a tile — its label and value ("(b sin θ)² = 12.00"); over `rest` — "DB = a − b cos θ · negative when θ > 90°"; idle — "◀ ▶ step · drag A or B any time".

**Timeline `proof`**, 10 steps. Formula (block) is ONE `aligned` block whose lines are revealed with `marks`:

```
\t{c2}{c^2} &= \t{rest2}{(a - b\cos\theta)^2} + \t{rise2}{(b\sin\theta)^2} \\
 &= \t{a2}{a^2} \t{corr}{- 2ab\cos\theta} + \t{bcos2}{b^2\cos^2\theta} + \t{bsin2}{b^2\sin^2\theta} \\
 &= \t{a2}{a^2} + \t{b2}{b^2\,\t{ident}{(\cos^2\theta + \sin^2\theta)}} \t{corr}{- 2ab\cos\theta} \\
 &= \t{a2}{a^2} + \t{b2}{b^2} \t{corr}{- 2ab\cos\theta}
```

| step | text | scene appears / lit | formula marks |
|---|---|---|---|
| 0 | "Your triangle again: sides a and b meet at C with angle θ; c is the gap. No square corner, so Pythagoras does not apply — yet." | `a`, `b`, `c`, `th`; lit: `c` | all hidden |
| 1 | "Drop a perpendicular from A onto the line of a. Call the foot D. Now there ARE right angles." | `drop_a` (solid beam now), `D`, `rtD`; lit: `drop_a` | hidden |
| 2 | "CD is the shadow of b on a: CD = b cos θ. The height AD = b sin θ." | `p` band with label `b cos θ`, label on `drop_a` = `b sin θ`; lit: `p` | hidden |
| 3 | "The rest of side a is DB = a − b cos θ. (If θ > 90°, b cos θ is negative and D is behind C: DB is longer than a. The formula does not care.)" | `rest` Segment D→B (vecA, offset 0.25 below a, label `a − b cos θ`); lit: `rest` | hidden |
| 4 | "Triangle ADB is right-angled at D. Pythagoras: c² = DB² + AD²." | tiles `rest2` (square on DB), `rise2` (square on AD), `c2` (square on c), fill 16%, roles vecA / vecB / vecC, labels `DB²`, `AD²`, `c²`; lit: `c2` | line 1 visible with `rest2`, `rise2` `dim` |
| 5 | "Substitute the two legs we named." | tile labels switch to `(a − b cos θ)²`, `(b sin θ)²`; lit: `rest2`, `rise2` | line 1 full |
| 6 | "Expand the square: (a − b cos θ)² = a² − 2ab cos θ + b² cos²θ. The middle term is the correction — it is the two strips of the first scene, written as algebra." | `rest2` becomes outline; AreaTile `a2` on CB appears (vecA); lit: `corr` | line 2 visible; `corr` lit |
| 7 | "b² cos²θ + b² sin²θ: two pieces of the same square." | tiles `bcos2` (square on CD) and `bsin2` (= `rise2`, relabelled) lit together | line 3 visible, `ident` lit |
| 8 | "cos²θ + sin²θ = 1 (the unit-circle point is at distance 1). The two pieces merge into b²." | `bcos2`, `rise2` fade out; AreaTile `b2` on CA appears; lit: `b2` | line 3: `ident` `struck`; line 4 visible |
| 9 | "Done: c² = a² + b² − 2ab cos θ. Drag θ to 90°: cos θ = 0 and this is Pythagoras." | `a2`, `b2`, `c2` tiles + arc; `rt` at C if θ = 90; lit: `corr` | lines 1–3 `dim`, line 4 full; `corr` `dim` when `th === 90` |

No separate NumTable: the PIECES rows of the N-panel are the numbers view.

**Edge cases.** `th = 90`: D = C, `p` has zero length (hidden), step 2 appends "Right now θ = 90°, so the shadow is zero and D sits on C." `th = 0` or `180`: `rise = 0`, `rise2` tile vanishes, step 4 appends "θ = 0° or 180° is the flattened case: AD = 0 and the proof still holds." `A ≈ B` (c < 1e-9): `c2` tile hidden; no division anywhere.

**Challenge.** "Find the step where the correction term is born." Success: `step['proof:max'] >= 6` (so a reader who scrubs straight to 9 is sent back to look).

### 1.5 Scene LC-3 — Two-bone IK (the CG anchor)

**Purpose.** The law of cosines is the thing inside every elbow/knee solver: three lengths in, the elbow angle out, and the `acos` argument must be clamped. Aha: dragging the hand out of reach pins the arm straight while the readout shows the raw cosine below −1 and the clamped one at −1.

**Stage.** 2D, `extent = 6.5` (max reach 3 + 3 = 6), centred at the shoulder `[0, 0]`, grid step 1, axes off.

**ViewportCard.** Header: `2D` · `Ortho` · `Overlays ▾` · `Reset view` · `LC-3 · Two-bone IK`. Overlays: grid (on), labels (on), reach rings (on), distance `d` (off — shown on hover of `d`/`cosphi` regardless), angle φ (on), right angle (on).
N-panel: `ik_hand` V (x, y fields; E) · `ik_l1` E (`L₁`, [0.5–3, 0.1]) · `ik_l2` E (`L₂`) · `ik_flip` C ("Elbow down" checkbox) · separator SOLVE · `d` R · `cosraw` R (`cos φ raw`, hot outline while `|cosraw| > 1`) · `cosc` R (`clamped`) · `phi` R (`φ`) · `reach` R (`0.5 … 3.5`). The bone lengths and the flip live only here (no prose controls).
Status bar: over the hand — "LMB drag target · arrows 0.1 · Shift 1.0"; over the elbow — "Elbow · solved by the law of cosines · not draggable"; over a ring — "Reach limit |L₁ − L₂| … L₁ + L₂"; while `cosraw < −1` — "Target out of reach · cos φ clamped to −1 · arm straight"; while `cosraw > 1` — "Target too close · cos φ clamped to 1 · arm folded"; idle — "Wheel zoom · MMB pan · N panel".

Geometry (derive): shoulder `S = [0, 0]`; `d = |ik_hand|`; direction `u = ik_hand / d`, or `[1, 0]` when `d < 1e-9` (fixed convention, documented in the caption); `cosraw = (l1² + l2² − d²) / (2 l1 l2)`; `cosc = clamp(cosraw)`; `phi = acos(cosc)` deg (interior elbow angle, [0, 180]); shoulder offset `cosb = clamp((l1² + d² − l2²) / (2 l1 d))`, `cosb = 1` when `d < 1e-9`; `beta = acos(cosb)`; sign `s = ik_flip ? −1 : +1`; elbow `E = l1 · rot(u, s·beta)`; wrist `W = E + l2 · normalize(T − E)`, `T = ik_hand` (`T − E` is never zero because `l2 ≥ 0.5`... it is zero only if `d = l1` and `l2`... `T = E` requires `|T| = l1` and `beta = 0`, i.e. `d = l1` and `cosb = 1` → `l2 = 0`: impossible in range). Unreachable (`cosraw < −1`): `beta = 0`, `phi = 180`, `E = l1 u`, `W = (l1 + l2) u`, dashed gap W→T. Too close (`cosraw > 1`): `phi = 0`, forearm folds back along the upper arm.

**Objects.**

| id | primitive | geometry | role | label | drag |
|---|---|---|---|---|---|
| `S` | Point | `[0,0]` | ink | `shoulder` | none |
| `reach_max` | Circle (section 5 item 7) | centre S, r = `l1 + l2` | muted dashed | `max reach` | — |
| `reach_min` | Circle | r = `|l1 − l2|`, hidden when < 1e-9 | muted dashed | `min reach` | — |
| `l1` | Segment beam, width 6 | S→E | vecA | `L₁ = 2.0` | — |
| `l2` | Segment beam, width 6 | E→W | vecB | `L₂ = 1.5` | — |
| `E` | Point (joint, larger) | elbow | ink | `elbow` | none |
| `W` | Point | wrist | vecB | — | — |
| `hand` | Point | `ik_hand` | vecC | `target` | yes (vector key, grid snap) |
| `gap` | Segment dashed | W→T, visible when `d > reachMax + 1e-9` | axisX (warning red) | `out of reach` | — |
| `d` | Segment dashed | S→T, visible while `hot` has `d` or `cosphi` | vecC | `d = 2.92` | — |
| `phi` | AngleArc | at E, E→S to E→W, r = 0.5, sector 10% | ink | `φ = 112°` | — |
| `rt` | RightAngle | at E when `|cosc| < 1e-12` | ink | — | — |

The checkbox row `ik_flip` is a rig's pole vector in one bit (flips which side of S→T the elbow bends to).

**Formula (block).**

```
\t{d}{d} = \sqrt{x^2 + y^2} = \s{d} \\[2pt]
\cos\t{phi}{\varphi} = \t{cosphi}{\frac{L_1^2 + L_2^2 - d^2}{2 L_1 L_2}} = \s{cosraw} \;\to\; \t{clamp}{\operatorname{clamp}} \;\to\; \s{cosc} \\[2pt]
\t{phi}{\varphi} = \arccos(\s{cosc}) = \s{phi}
```

**CodeView** (ids per line):

```
d  = length(hand - shoulder)                     # d
c  = (L1*L1 + L2*L2 - d*d) / (2*L1*L2)           # cosphi
c  = clamp(c, -1.0, 1.0)                         # clamp
phi = acos(c)                                    # phi
```

No separate NumTable: the SOLVE rows of the N-panel are the numbers view.

**Hover map.** `d`: segment `d` + formula line 1 + code line 1 + N-panel row. `cosphi`: formula fraction + code line 2 + `cosraw` row; the scene lights `l1`, `l2`, `d` together (three lengths in, one angle out — implemented by the scene checking `hot.has('cosphi')` for those three). `clamp`: code line 3 + `cosc` row + `gap` (if visible) + `reach_max`. `phi`: arc + formula + code line 4 + row.

**Edge cases.** As in the geometry paragraph; `d = 0`: caption "target on the shoulder: direction is a convention here"; `l1 === l2`: `reach_min` hidden; `phi === 180` draws the arc as a semicircle on the elbow's outer side.

**Challenges.**
1. "Put the hand exactly where the elbow is straight (φ = 180°) and still reachable." Success: `|d − reachMax| < 0.05 && d ≤ reachMax + 1e-9`.
2. "Make the solver clamp." Success: `cosraw < −1 || cosraw > 1`; the card: "Your rig does this silently; without the clamp `acos` returns NaN and the bone disappears."
3. "Find a target with φ = 90°." Success: `|cosc| < 1e-9` (with L₁ = 2, L₂ = 1.5: d = 2.5 → `[2.5, 0]`, `[1.5, 2]`, `[2, 1.5]`, `[0.7, 2.4]`). Hint: "Pythagoras: which d makes the strips of the first scene vanish?"

### 1.6 Quiz (misconception each targets)

1. θ = 120°, a = b = 1. Is c bigger or smaller than at 90°? — *bigger* (cos 120° < 0: the strips are negative and get added). Targets: "the correction always subtracts".
2. `np.cos(60)` gives cos 60°. — *false* (radians). Targets: degree/radian confusion.
3. "The law of cosines only works if one angle is 90°." — *false*; at 90° it *reduces* to Pythagoras. Targets: special case vs general.
4. In `c² = a² + b² − 2ab cos θ`, θ is the angle… — *opposite side c* (between a and b). Targets: which angle.
5. Two bones 2 and 1.5, target at distance 4. The elbow angle is… — *undefined; a solver clamps to 180°*. Targets: unreachable targets and the acos domain.
6. The two strips a × (b cos θ) and b × (a cos θ) have… — *the same area, different shapes (unless a = b)*. Targets: "same area means same rectangle".
7. If `cos θ` is positive, c is… — *shorter than* √(a² + b²). Targets: sign reasoning.

### 1.7 Hand exercises (answers behind reveal)

1. a = 3, b = 4, θ = 90° → c = 5.
2. a = 3, b = 4, θ = 60° → c² = 9 + 16 − 24·0.5 = 13, c ≈ 3.61. Strip area: 3·(4·0.5) = 6; two strips = 12 = 25 − 13.
3. a = 3, b = 4, θ = 120° → c² = 25 + 12 = 37, c ≈ 6.08 (strips negative: −6 each, added).
4. a = b = 1, c = 1 → cos θ = (1 + 1 − 1)/2 = 0.5 → θ = 60°.
5. Bones 2 and 1.5, target at d = 2.5 → cos φ = (4 + 2.25 − 6.25)/6 = 0 → φ = 90°.
6. Bones 2 and 1.5, target at d = 0.3 → cos φ = (6.25 − 0.09)/6 = 1.027 → clamp → φ = 0° (folded; target closer than |L₁ − L₂| = 0.5).

### 1.8 Python cell (Pyodide / Colab)

```python
import numpy as np

def law_of_cosines(a, b, theta_deg):
    th = np.radians(theta_deg)
    return np.sqrt(a*a + b*b - 2*a*b*np.cos(th))

print(law_of_cosines(3, 4, 90))   # 5.0  (Pythagoras)
print(law_of_cosines(3, 4, 60))   # 3.605...
print(law_of_cosines(3, 4, 120))  # 6.082...

# the two strips: a * (b cos th) and b * (a cos th) are the same number
a, b, th = 3.0, 4.0, np.radians(60)
print(a * (b*np.cos(th)), b * (a*np.cos(th)))   # 6.000000000000002 6.000000000000002 (float rounding; round() in the lesson)
print(a*a + b*b - 2*6.0)                        # 13.0 = c^2

# sweep the corner: c grows monotonically from |a-b| to a+b
for deg in (0, 30, 60, 90, 120, 150, 180):
    print(deg, round(law_of_cosines(3, 4, deg), 3))

# two-bone IK elbow angle, with the clamp every solver needs
def elbow_angle(L1, L2, d):
    c = (L1*L1 + L2*L2 - d*d) / (2*L1*L2)
    c_raw = c
    c = np.clip(c, -1.0, 1.0)
    return np.degrees(np.arccos(c)), c_raw

print(elbow_angle(2.0, 1.5, 2.5))  # (90.0, 0.0)
print(elbow_angle(2.0, 1.5, 4.0))  # (180.0, -1.625)  raw < -1: unreachable
print(np.arccos(-1.625))           # nan  <- what happens without the clamp
```

---

## 2. Lesson 1.4 — The dot product, derived from the law of cosines

Scene order: **DP-1** derivation timeline (first scene; draws itself), **DP-2** sign explorer with the area face (S2), **DP-3** 3D explorer, **DP-4** the N·L lamp. S2's thread "one number, three faces" runs through DP-1 (step 8 introduces the rectangle), DP-2 (the three-faces strip is permanent) and DP-4 (the brightness face is the whole scene).

### 2.1 Store (`dot-product`)

| key | type | initial | range / step | written by |
|---|---|---|---|---|
| `a` | [x, y] | [3, 1] | stage extent, snap 0.1 | tip drag (DP-1, DP-2) |
| `b` | [x, y] | [1, 2] | same | tip drag (magnet to ⊥ in DP-2) |
| `a3` | [x, y, z] | [2, 1, 1] | snap 0.1 | tip drag (DP-3) |
| `b3` | [x, y, z] | [1, 2, −1] | snap 0.1 | tip drag |
| `nl_light` | [x, y, z] | [2, 2.5, 1.5] | snap 0.1, kept outside the sphere (`|v| ≥ r + 0.3`, enforced by the handle's `move`) | light drag (DP-4) |
| `nl_pt` | [x, y, z] | normalize([1, 0.6, 0.8]) · r | constrained to the sphere surface by `move` | surface point drag |
| `nl_raw` | number 0/1 | 0 | toggle | "show raw N·L" toggle |
| `step` timelines | `derive` 0..8 | 0 | | Timeline (+ `step['derive:max']`) |

Initial 2D values give whole-number arithmetic: a·b = 3·1 + 1·2 = 5, |a|² = 10, |b|² = 5, |a − b|² = 2² + (−1)² = 5, and θ = 45° exactly (√50 · 0.7071 = 5). Initial 3D: a3·b3 = 2 + 2 − 1 = 3.

Derived (one `derive(vals)`): `na2 = a·a`, `nb2`, `na`, `nb`, `amb = a − b`, `namb2`, `dot = a·b`, `cosT = dot / (na nb)` (NaN when `na nb === 0`), `th = acos(clamp(cosT))` deg (NaN when undefined), `rhs = na · nb · cos(rad(th))` (same number by another route; rounding may differ in the 3rd decimal, which step 7 mentions as a feature), components `a1, a2, b1, b2, d1 = a1 − b1, d2, d1sq, d2sq, a1sq, a2sq, b1sq, b2sq, p1 = a1 b1, p2 = a2 b2`; **area face**: `ahat = a / na`, `shadow = dot / na` (signed length of b's shadow on a), `rect = na · shadow` (= `dot`), `side = −sign(cross(a, b))` (the rectangle is built on the side of a's line away from b; `+1` when `cross === 0`); **brightness face**: `bright = max(0, cosT)` (what Lambert would give if a and b were N and L). Same for 3D with suffix `3`. N·L: `N = normalize(nl_pt)`, `L = normalize(nl_light − nl_pt)`, `ndotl`, `brightNL = max(0, ndotl)`, `thNL`.

### 2.2 Scene DP-1 — Derivation timeline (centrepiece)

**Purpose.** The component formula and the geometric formula are forced equal by the law of cosines, with `|a|²` and `|b|²` struck off both sides. Aha: step 6, the strike-through, then step 7, where two different computations show the same live number while the reader drags; step 8 shows the number as the strip of 0.5.

**Stage.** 2D, `extent = 5`, origin offset `[−0.5, −0.5]`. Grid step 1, axes on in Blender colours (components matter here). Draws itself on first view.

**ViewportCard.** Header: `2D` · `Ortho` · `Overlays ▾` · `Reset view` · `DP-1 · Derivation`. Overlays: grid (on), axes (on), legs (on), labels (on), angle (on), squares (on from step 1), shadow + rectangle (on from step 8).
N-panel: `a` V (x, y; E) · `b` V (E) · `na` R (`|a|`) · `nb` R · `th` R (`θ`) · separator ONE NUMBER · `dot` R (`Σaᵢbᵢ`, from step 4) · `rhs` R (`|a||b| cos θ`, from step 7) · `rect` R (`area |a| × shadow`, from step 8). Dragging a tip updates the V fields live; typing into a field moves the tip.
Status bar: over a tip — "LMB drag tip of a · arrows 0.1 · Shift 1.0"; over `amb` — "a − b · tip of b to tip of a · not draggable"; over a square — "|a|² = 10.00"; over the rectangle — "|a| × shadow = a·b = 5.00"; idle — "◀ ▶ step · drag tips any time".

**Objects.**

| id | primitive | geometry | role | label | drag |
|---|---|---|---|---|---|
| `a` | Arrow (beam), `legs` | O→`a` | vecA | `a` | tip |
| `b` | Arrow, `legs` | O→`b` | vecB | `b` | tip |
| `amb` | Arrow | `from: 'b', to: 'a'`, `drag: 'none'` | vecC | `a − b` | — |
| `th` | AngleArc | at O, a→b, r = 0.7, sector 10% | ink | `θ = 45°` | — |
| `na2` | AreaTile | square on `a`, outward (side away from b) | vecA 16% | `|a|² = 10` | from step 1 |
| `nb2` | AreaTile | square on `b` | vecB 16% | `|b|² = 5` | — |
| `namb2` | AreaTile | square on `amb` | vecC 16% | `|a−b|² = 5` | — |
| `rect` | Poly | `[O, shadow·ahat, shadow·ahat + side·na·cw(ahat)… ]` precisely: base from O along `ahat` of signed length `shadow`, height `na` on the side `side`; hatched when `shadow < 0` | vecA 28% | `a·b = 5.00` | from step 8 |
| `shadow` | Segment band + `drop` dashed from tip of b to `shadow·ahat` | on the line of a | muted | `shadow of b on a` | from step 8 |
| `rt` | RightAngle | at O when `|dot| < 1e-9` | ink | — | — |

**NumTable `comp`** (page-side, under the card; the multi-column view the N-panel cannot hold) (columns revealed per step; ids in cells):

| row | `i` | `aᵢ` | `bᵢ` | `aᵢ − bᵢ` | `(aᵢ − bᵢ)²` | `aᵢ²` | `bᵢ²` | `aᵢ bᵢ` |
|---|---|---|---|---|---|---|---|---|
| 1 | 1 | `a1` 3 | `b1` 1 | `d1` 2 | `d1sq` 4 | `a1sq` 9 | `b1sq` 1 | `p1` 3 |
| 2 | 2 | `a2` 1 | `b2` 2 | `d2` −1 | `d2sq` 1 | `a2sq` 1 | `b2sq` 4 | `p2` 2 |
| Σ | | | | | `namb2` 5 | `na2` 10 | `nb2` 5 | `dot` 5 |

At 360 px the table scrolls inside its box and the newly revealed column is scrolled into view.

**Formula (block, `aligned`, lines revealed by marks).**

```
\t{namb2}{|a-b|^2} &= \t{na2_r}{|a|^2} + \t{nb2_r}{|b|^2} - \t{corr}{2\,|a|\,|b|\cos\theta} \\
\t{namb2}{|a-b|^2} &= \t{expand}{\sum_i (a_i-b_i)^2} = \t{na2_l}{\sum_i a_i^2} - \t{cross}{2\sum_i a_i b_i} + \t{nb2_l}{\sum_i b_i^2} \\
\t{na2_l}{|a|^2} + \t{nb2_l}{|b|^2} - \t{cross}{2\sum_i a_i b_i} &= \t{na2_r}{|a|^2} + \t{nb2_r}{|b|^2} - \t{corr}{2\,|a|\,|b|\cos\theta} \\
\t{sum}{\sum_i a_i b_i} &= \t{rhs}{|a|\,|b|\cos\theta} \\
\t{dot}{a\cdot b} &:= \t{sum}{\sum_i a_i b_i} = \s{dot} \qquad \t{rhs}{|a|\,|b|\cos\theta} = \s{rhs} \qquad \t{rect}{|a|\cdot(\text{shadow})} = \s{rect}
```

**Timeline `derive`**, 9 steps.

| step | text | scene | formula marks | table |
|---|---|---|---|---|
| 0 | "Two arrows a and b from the origin, angle θ between them. The third side of the triangle is the arrow a − b (tip of b to tip of a; lesson 1.1). Drag the tips — the triangle follows." | `a`, `b`, `amb`, `th`; lit: `amb` | all hidden | `aᵢ`, `bᵢ` |
| 1 | "We can already measure every side: |a|² = Σaᵢ², |b|² = Σbᵢ² (1.3), |a − b|² = Σ(aᵢ − bᵢ)² (0.3). Three squares, three sums." | tiles `na2`, `nb2`, `namb2` appear; lit: `namb2` | hidden | + `aᵢ − bᵢ`, `(aᵢ − bᵢ)²`, `aᵢ²`, `bᵢ²` with sums |
| 2 | "The law of cosines (0.5) ties the three sides to the corner: |a − b|² = |a|² + |b|² − 2|a||b| cos θ. (In 0.5 that last term was two strips of area |a|·|b| cos θ.)" | lit: `th` + arc | line 1 visible; `corr` lit | — |
| 3 | "The same left side, written in components and expanded: each (aᵢ − bᵢ)² = aᵢ² − 2aᵢbᵢ + bᵢ²." | lit: `amb` | line 2 visible, `expand` lit | + `aᵢ bᵢ` (no sum yet); `d1sq`, `d2sq` lit |
| 4 | "Regroup: the aᵢ² add up to |a|², the bᵢ² to |b|² — the tiles you already have. What is new is the cross term −2Σaᵢbᵢ." | lit: `cross` (tiles `na2`, `nb2` on hover) | line 2: `cross` lit | `p1`, `p2`, sum `dot` lit; N-panel row `dot` appears |
| 5 | "Put the two descriptions of |a − b|² face to face." | lit: `namb2` | line 3 visible; lines 1–2 `dim` | — |
| 6 | "|a|² appears on both sides — cancel it. |b|² too. Only the two cross terms survive." | tiles `na2`, `nb2` fade to outline (opacity 0.15) | line 3: `na2_l` + `na2_r` `struck` (together, 300 ms), then after 350 ms `nb2_l` + `nb2_r` `struck` (reduced motion: both instantly) | `na2`, `nb2` sum cells struck |
| 7 | "Divide by −2. Left: the sum of products of matching components. Right: lengths times the cosine of the angle. Two computations, one number — drag a or b and watch both stay equal." | lit: `a`, `b` on hover; arc | line 3 `dim` (struck terms `hidden`), line 4 visible; line 5 visible with live `\s{dot}`, `\s{rhs}` (the `rect` term still `hidden`) | `dot` cell lit; N-panel row `rhs` appears |
| 8 | "Name it: a·b, the dot product. The components give it without measuring an angle; the geometry says what it means: |a||b| cos θ. And 0.5 already drew it: |a| times the shadow of b on a is a rectangle whose area is a·b. One number, three faces — the third face lights a surface in the last scene." | `shadow` band + drop appear, then `rect` fills in from O along a (180 ms); `rt` when ⊥; lit: `rect` | lines 1–4 `dim`, line 5 full including the `rect` term | `dot`; N-panel row `rect` appears |

**Live readouts** at steps 7–8: `\s{dot}`, `\s{rhs}`, `\s{rect}` (2 decimals). When `na === 0 || nb === 0`: `rhs`, `rect` are `NaN` → `--`, caption: "|a||b| cos θ and the shadow need two non-zero arrows; Σaᵢbᵢ is still fine: `\s{dot}`" (the component route never fails — a quiet machine-layer point).

**Hover map.** `a1`/`a2` cells: arrow `a` (whole arrow until leg ids exist, section 5 item 8). `namb2`: tile + arrow `amb` (the step's `lights` list carries both; hover lights the tile only). `na2_l`/`na2_r`: tile `na2` (subscribes to both ids) + table `na2` + the terms. `corr`/`cross`: arc + formula; `cross` also lights `p1`, `p2`, `dot`. `sum`/`dot`: `dot` cell + both arrows + N-panel row. `rhs`: arc + both arrows + row. `rect`: rectangle + shadow band + row + `dot` cell (the three faces light together from any one of `dot`, `rhs`, `rect`: implement as the scene treating these three ids as one group `faces` — a lesson-level `alias` map `{rect: 'faces', dot: 'faces', rhs: 'faces'}` resolved by the lesson before `hotOn`, so the engine still lights one id).

**Edge cases.** `a === b`: `amb` zero-length, `namb2` hidden, θ = 0; `rect` is the full square `na2` (shadow = |a|). `a = 0` or `b = 0`: arrows hidden, arc hidden, `th`, `rhs`, `rect` `--`, `dot = 0`. Antiparallel: `rhs = −|a||b|`, `rect` hatched with width |b|, on the far side of O. All reachable by keyboard.

**Challenges.**
1. "Find a, b where Σaᵢbᵢ is 0 but neither arrow is 0." Success: `|dot| < 1e-9 && na > 0 && nb > 0`. Hint: swap the components of a and flip one sign: `b = [−a₂, a₁]`.
2. "Make the two routes give a negative number." Success: `dot < −1e-9` (the rectangle turns hatched).
3. "Make a·b = |a|²." Success: `|dot − na2| < 1e-9 && na > 0` (b = a, or any b whose shadow equals a — preview of 1.5; the card says both).

### 2.3 Scene DP-2 — Sign explorer with the area face (S2)

**Purpose.** The sign of a·b is the side of the line perpendicular to a on which b's tip lies; zero is exactly that line; and the same number is the area of the rectangle |a| × shadow(b), which flips to hatched when b crosses the line and collapses on it. Aha: the plane is pre-coloured, so the reader predicts the sign before reading numbers; and the "three faces" strip shows the sum of products, the rectangle's area and the Lambert brightness changing as one.

**Stage.** 2D, `extent = 4.5`, origin at centre, grid 1, axes on.

**ViewportCard.** Header: `2D` · `Ortho` · `Overlays ▾` · `Reset view` · `DP-2 · Sign`. Overlays: grid (on), axes (on), labels (on), half-planes (on), boundary `perp` (on), shadow (on), rectangle (on), angle (on), legs (off).
N-panel: `a` V (E) · `b` V (E) · `na` R · `nb` R · `shadow` R (`shadow of b on a`) · `cosT` R (`cos θ`) · `th` R · separator THREE FACES · `dot` R (`Σaᵢbᵢ`) · `rect` R (`|a| × shadow`) · `bright` R (`max(0, cos θ)`) · separator CHECK · `aa` R (`a·a = |a|²`; hover outlines `a` and the `na` row; tooltip: "with b = a the rectangle is the full square").
Status bar: over tip of b — "LMB drag tip of b · snaps to ⊥ near 90°"; over tip of a — "LMB drag tip of a · the half-planes turn with it"; over a half-plane — "a·v > 0 here" / "a·v < 0 here"; over the boundary — "a·v = 0 · every ⊥ direction"; over the rectangle — "area = a·b = 5.00 · hatched = negative"; idle — "Wheel zoom · MMB pan · N panel".

**Objects.**

| id | primitive | geometry | role | label | drag |
|---|---|---|---|---|---|
| `a` | Arrow (beam) | O→`a` | vecA | `a` | tip |
| `b` | Arrow | O→`b` | vecB | `b` | tip; **magnet to ⊥**: when `|cosT| < sin 3°` (≈ 0.052) during a drag or nudge, b's tip is replaced by `nb · cw(ahat) · sign(cross(a, b))` (off-grid, exactly perpendicular); the grid snap is skipped for that write |
| `hp_pos` | HalfPlane (section 5 item 3) | `{v : a·v > 0}` | vecA fill 10% | `a·v > 0` in the far corner | — |
| `hp_neg` | HalfPlane | `{v : a·v < 0}` | `muted` fill 10% | `a·v < 0` | — |
| `perp` | Segment dashed | the boundary through O, `±extent · cw(ahat)` | muted; hot outline when `|dot| < 1e-9` | `a·v = 0` | — |
| `shadow` | Segment band + dashed drop from tip of b | on the line of a, O → `shadow·ahat` | muted | `|b| cos θ = 1.58` | — |
| `rect` | Poly | base O→`shadow·ahat`, height `na` on side `side`; hatched when `shadow < 0` | vecA 28% | `a·b = 5.00` | — |
| `th` | AngleArc | at O, a→b, r = 0.6 | ink | `θ = 45°` | — |
| `rt` | RightAngle | at O when `|dot| < 1e-9` | ink | — | — |
| `badge` | Label at the tip of b, when `|dot| < 1e-9 && na > 0 && nb > 0` | `chrome` chip with hot outline | `⊥  a·b = 0` | — |

Half-planes, `perp`, `shadow` and `rect` are drawn only while `na > 0`; when a is zero the caption says "a = 0 has no perpendicular line and no shadow".

**Three faces strip (DOM, page-side, under the card, full width; built from the N-panel field component of section 5 item 11 at `size: 'lg'`).** Three cells, each a field-styled cell with a glyph, a label and the live number:
- `sum` — glyph: the two component columns with products (tiny `3·1 + 1·2`); label "sum of products"; value `\s{dot}`.
- `rect` — glyph: a small rectangle whose width/height follow `shadow`/`na` (an inline SVG scaled to 24 px, hatched when negative); label "area |a| × shadow"; value `\s{rect}`.
- `bright` — glyph: a 24 px disc whose luminance is `max(0, cosT)` (a DOM div with `opacity = bright` over the base colour vecC on a `field`-grey background), with a tiny "N·L" caption; label "brightness if a = N, b = L"; value `\s{bright}` (3 decimals). Hovering it outlines both arrows and the arc; the card says "the sphere in the last scene is this cell, wrapped around a ball".
All three cells carry the alias group `faces` (hover any → all three cells, the rectangle, the `dot` row and both arrows light). The sign of the number is also shown as a word under the strip: `agree (> 0)` / `⊥ (= 0)` / `disagree (< 0)`, ids `sgn_pos`, `sgn_zero`, `sgn_neg`, the active one filled with the matching tint; hovering a sign cell lights the corresponding half-plane / boundary.

**Formula (block).**

```
\t{dot}{a\cdot b} = \t{sum}{\s{a1}\cdot\s{b1} + \s{a2}\cdot\s{b2}} = \s{dot} \\[2pt]
\t{rhs}{|a|\,|b|\cos\theta} = \s{na}\cdot\s{nb}\cdot\s{cosT} = \s{rhs} \\[2pt]
\t{rect}{|a|\cdot(|b|\cos\theta)} = \s{na}\cdot\s{shadow} = \s{rect} \\[2pt]
\theta = \s{th}
```

Components are read-only `\s{}` (vector components are not scrubbable in the Formula contract; the tips are the input — section 5 item 9 notes the wanted extension).

No separate NumTable: the N-panel rows above are the numbers view; the `aa` row is the "a·a = |a|²" checkpoint named in the storyboard.

**Hover map.** `dot`/`sum`/`rect`/`bright` (group `faces`): both arrows, rectangle + shadow band, the three cells, the THREE FACES rows. `rhs`: arc + arrows. `hp_pos`/`hp_neg`: half-plane fill 10% → 20% + sign cell. `perp`: boundary + `sgn_zero` + badge if present. `th`: arc + row. `shadow`: band + drop + row + the `|b|\cos\theta` factor.

**Edge cases.** `|dot| < 1e-9` after the magnet: badge + `rt` + `⊥` cell + the rectangle collapses to a zero-width polygon (hidden) with its label `0.00` at O + `bright = 0.000`. `b` on a's line: θ = 0 or 180, rectangle = full square (`shadow = ±nb`), hatched at 180. Zero vectors: as DP-1 (`bright` is `NaN` → `--` and the disc is drawn at 0 opacity with a dashed outline).

**Challenges.**
1. "Make a·b = 0 without making either vector zero." Success: `|dot| < 1e-9 && na > 0 && nb > 0`. Hint 1: "drag b toward the dashed line". Hint 2: "integer solution: a = [2, 1], b = [−1, 2]".
2. "Keep a fixed and make a·b as large as you can with |b| = 2." Success: `|nb − 2| < 0.05 && cosT > 0.995` (the rectangle is tallest… widest: `shadow = |b|`).
3. "Find b with |b| = 2 and a·b = −2|a| (the most negative possible)." Success: `|nb − 2| < 0.05 && cosT < −0.995`.
4. "Make the brightness face read 0.500 (a 60° lamp)." Success: `|cosT − 0.5| < 0.01`.

### 2.4 Scene DP-3 — 3D explorer

**Purpose.** Nothing changes with a third component: the sum gets one more term, the zero-dot set is now a plane (a disc in the viewport). Aha: orbiting shows the ⊥ set is a disc, and `a3·b3` in the table is just a third row.

**Stage.** 3D, `extent = 3.5`, orbit (engine default), floor grid y = 0 fading with distance, axes in Blender colours with letter glyphs, navigation gizmo top-right (click = align, 250 ms).

**ViewportCard.** Header: `3D` · `Persp`/`Ortho` toggle (enabled; Ortho keeps the orbit) · `Overlays ▾` · `Reset view` · `DP-3 · 3D`. Overlays: floor grid (on), axes (on), legs (on), labels (on), disc `perp3` (on), shadow + rectangle (on), angle (on).
N-panel: `a3` V (x, y, z; E) · `b3` V (E) · `na3` R · `nb3` R · `th3` R · separator ONE NUMBER · `dot3` R · `rhs3` R · `rect3` R.
Status bar: over a tip — "LMB drag tip · PageUp/PageDown z · Shift 1.0"; over the disc — "a·v = 0 plane · b on the far side → negative"; idle — "MMB orbit · Wheel zoom · Shift+MMB pan · click gizmo to align · N panel"; before first tap on touch — "Tap to interact".

**Objects.** `a3` Arrow (vecA, legs, tip drag), `b3` Arrow (vecB, legs, tip drag), `th3` AngleArc at O (for antiparallel vectors pass `normal = [0, 1, 0]`, falling back to `[1, 0, 0]` when a3 is parallel to y), `perp3` HalfPlane in 3D (section 5 item 3: a disc of radius `extent` through O with normal `a3`; pos side vecA 12%, neg side `muted` 12%, as two single-sided discs), `shadow3` band on the line of a3 + dashed drop from the tip of b3, `rect3` Poly in the plane spanned by a3 and b3 (base along `â3` of signed length `shadow3`, height `|a3|` on the side away from b3; hatched when negative; label `a·b = 3.00`), `rt3` RightAngle when `|dot3| < 1e-9`, `badge3` as in DP-2.

**Formula**: `\t{dot3}{a\cdot b} = \s{a31}\cdot\s{b31} + \s{a32}\cdot\s{b32} + \s{a33}\cdot\s{b33} = \s{dot3}`; `\t{rhs3}{|a|\,|b|\cos\theta} = \s{rhs3}`; `\t{rect3}{|a|\cdot(\text{shadow})} = \s{rect3}`; `\theta = \s{th3}`.

**NumTable `comp3`** (page-side, under the card): rows i = 1..3 with `aᵢ`, `bᵢ`, `aᵢbᵢ` and the sum `dot3`; ids `a31`…`p33`, `dot3`.

**CodeView** (two tabs, NumPy and GLSL, ids per line):

```
dot = a[0]*b[0] + a[1]*b[1] + a[2]*b[2]   # sum
dot = (a*b).sum()                          # sum
dot = a @ b                                # dot3
```
```
float d = dot(a, b);                      // dot3
float c = d / (length(a)*length(b));      // cosT3
float th = acos(clamp(c, -1.0, 1.0));     // th3
```

**Challenges.** 1. "Make a·b = 0 in 3D." Success: `|dot3| < 1e-9 && na3 > 0 && nb3 > 0`. 2. "Put b on the far side of the disc (a·b < 0) while keeping b₃ > 0." Success: `dot3 < −1e-9 && b3[2] > 0`.

**Edge cases.** As 2D; antiparallel arc needs `normal` (derive picks it); the rectangle's plane is undefined when a3 ∥ b3 — `rect3` is then drawn in the plane spanned by a3 and the axis least parallel to it (so the full/hatched square is still visible). Tip drags use the engine's camera-facing drag plane; PageUp/Down for z.

### 2.5 Scene DP-4 — The N·L lamp (the brightness face)

**Purpose.** Lambert shading *is* a dot product: brightness = max(0, N·L), the terminator is DP-2's ⊥ line wrapped around a sphere, and the probe point's `bright` row is the same brightness face the reader met in DP-2. Aha: the raw-N·L toggle paints the far hemisphere "negative light", which `max()` throws away.

**Stage.** 3D, `extent = 4`, orbit; no grid (a lamp scene), axes off, navigation gizmo on.

**ViewportCard.** Header: `3D` · `Persp`/`Ortho` · `Overlays ▾` · `Reset view` · `DP-4 · N·L lamp`. Overlays: terminator (on), `N` arrow (on), `L` arrow (on), angle (on), rectangle `rectNL` (on), labels (on), light rays (off: optional thin `light`-coloured lines from the lamp to P and to the terminator, decorative only).
N-panel: `nl_light` V (x, y, z; E — typing a value inside the sphere clamps to `r + 0.3` along the same direction) · `nl_pt` V (R: "drag P in the viewport") · `nl_raw` C ("Show raw N·L (no max)") · separator PROBE · `N` R (vector) · `L` R (vector) · `NdotL` R · `thNL` R (`θ`) · `bright` R (`brightness`; label becomes `raw N·L` and the value may be negative, hatched glyph, while `nl_raw`).
Status bar: over the lamp — "LMB drag light · stays outside the sphere"; over P — "LMB drag P on the surface"; over the sphere — "Surface brightness = max(0, N·L)"; over the terminator — "N·L = 0 here · the ⊥ line of DP-2, wrapped"; idle — "MMB orbit · Wheel zoom · click gizmo to align · N panel".

**Objects.**

| id | primitive | geometry | role | label | drag |
|---|---|---|---|---|---|
| `sphere` | LitSphere (section 5 item 4) | centre O, r = 1.5; `light: 'nl_light'`, `mode: nl_raw ? 'raw' : 'lambert'` | base vecC, dark `vp-bg-bottom`, negative tint `axisZ` (a cool blue: neither the green base nor the yellow light) | — | — |
| `term` | Circle | the horizon of the point light: centre `O + (r²/|light|)·normalize(light)`, radius `r·sqrt(1 − (r/|light|)²)`, normal `normalize(light)`, offset outward by 0.01 | muted dashed; hot outline when `|ndotl| < 0.02` | `N·L = 0` | — |
| `light` | Point (sun glyph, larger) | `nl_light` | role `light` (yellow) | `light` | yes; `move` keeps `|v| ≥ r + 0.3` |
| `L_dir` | Arrow (beam) | P → P + 1.2·L | vecA | `L` | — |
| `P` | Point | `nl_pt` | ink | `P` | yes; `move` projects `P + delta` back onto the sphere |
| `N` | Arrow (beam) | P → P + 1.2·N | vecB | `N` | — |
| `thNL` | AngleArc | at P from N to L, r = 0.5 | ink | `θ = 38°` | — |
| `rt` | RightAngle | at P when `|ndotl| < 1e-9` | ink | — | — |
| `rectNL` | Poly, in the plane of N and L at P | base along N of signed length `ndotl` (= the shadow of the unit L on the unit N), height 1 (|N|), scaled by 0.6 for display; hatched when negative | vecB 28% | `N·L = 0.800` | — |

Point light at `nl_light` (not directional): `L = normalize(nl_light − P)` per surface point, matching the GLSL the reader will write; the terminator is then the horizon circle above, which is why `move` keeps the light outside the sphere.

**Checkbox row** `nl_raw` ("Show raw N·L (no max)"): in raw mode the LitSphere colours `N·L < 0` with the cool `axisZ` tint at intensity `|N·L|`, and the `bright` row reads `raw N·L`.

**Formula (block).**

```
\t{bright}{\text{brightness}} = \max\bigl(0,\; \t{NdotL}{N\cdot L}\bigr) = \max(0,\; \s{ndotl}) = \s{brightNL} \\[2pt]
\t{NdotL}{N\cdot L} = |N|\,|L|\cos\theta = 1\cdot 1\cdot\cos\s{thNL} = \s{ndotl} \\[2pt]
\t{rectNL}{|N|\cdot(\text{shadow of } L)} = 1\cdot\s{ndotl}
```

No separate NumTable: the PROBE rows are the numbers view; the `bright` row carries the same luminance-disc glyph as DP-2's third face, so the reader recognises it.

**CodeView** (GLSL, ids per line):

```
vec3 N = normalize(P - center);        // N
vec3 L = normalize(lightPos - P);      // L
float ndotl = dot(N, L);               // NdotL
float k = max(0.0, ndotl);             // bright
color = albedo * k;                    // sphere
```

**Hover map.** `N`: arrow + row + line 1. `L`: arrow + light glyph + row + line 2. `NdotL` (group `faces` with `rectNL` and `bright`): both arrows + arc + the small rectangle + the PROBE rows + lines 3–4. `term`: the circle + `rt` if present. `sphere`: not hoverable.

**Edge cases.** `nl_pt` is never zero (projected to the sphere; a zero delta keeps the old point; init validation throws on a zero vector). Light directly above P: θ = 0, brightness 1.000, `rectNL` is the full unit square. P on the far side: `ndotl < 0`, brightness `0.000`, raw row e.g. `−0.634`, `rectNL` hatched. `|ndotl| < 1e-9`: `rt` + terminator lit + `rectNL` hidden with label `0.000`.

**Challenges.**
1. "Put P on the terminator." Success: `|ndotl| < 0.02`. Hint: "the dashed circle is every point with N·L = 0 — DP-2's line, bent around the sphere."
2. "Make P as bright as possible without moving the light." Success: `ndotl > 0.995`.
3. "Switch to raw N·L and find the darkest point." Success: `nl_raw === 1 && ndotl < −0.995`.

### 2.6 Quiz (misconception each targets)

1. The dot product of two 3D vectors is… — *a number*. Targets: "dot product is a vector".
2. a·b = 0. Therefore… — *they are perpendicular, or at least one is zero*. Targets: "one of them must be zero".
3. a·b versus b·a — *equal* (the two strips of 0.5). Targets: non-commutativity by analogy with matrices/cross product.
4. For unit vectors, a·b can be 1.5. — *false; |a·b| ≤ |a||b| = 1*. Targets: range of cos.
5. "To compute a·b you must know the angle." — *false; Σaᵢbᵢ needs no angle; the angle is what you get out*. Targets: the whole point of the lesson.
6. The rectangle |a| × shadow(b on a) has area… — *a·b, negative (hatched) when θ > 90°*. Targets: "area cannot be negative" vs signed area.
7. Lambert brightness for N·L = −0.3 is… — *0 (max clamps)*. Targets: negative light.
8. `np.dot(a, b)` and `(a*b).sum()` on the same fp32 arrays of length 10⁶ can differ. — *true, in the last digits; summation order*. Targets: "floating point is exact".

### 2.7 Hand exercises (answers)

1. (1, 2)·(3, 4) = 3 + 8 = 11.
2. (1, 0)·(0, 1) = 0.
3. (2, 2)·(1, −1) = 2 − 2 = 0 (perpendicular).
4. Angle between (1, 0) and (1, 1): cos θ = 1/√2 → 45°.
5. (3, 4)·(3, 4) = 25 = |(3, 4)|².
6. a = (2, 1, 1), b = (1, 2, −1): a·b = 3; |a| = |b| = √6; cos θ = 0.5 → 60°.
7. a = (3, 1), b = (1, 2): shadow of b on a = 5/√10 ≈ 1.58; rectangle |a| × 1.58 = √10 · 1.58 = 5 = a·b.
8. N = (0, 1, 0), L = (0.6, 0.8, 0): N·L = 0.8 → brightness 0.8; with L = (0.6, −0.8, 0): N·L = −0.8 → brightness 0.

### 2.8 Python cell

```python
import numpy as np

a = np.array([3., 1.]); b = np.array([1., 2.])

# face 1: components (no angle needed)
s = (a * b).sum()
print(s, np.dot(a, b), a @ b)          # 5.0 5.0 5.0

# face 2: lengths and the angle (law of cosines rearranged)
na, nb = np.linalg.norm(a), np.linalg.norm(b)
cos_t = (a @ b) / (na * nb)
theta = np.degrees(np.arccos(np.clip(cos_t, -1, 1)))   # clip: rounding can give 1.0000000002
print(theta)                              # 45.0
print(na * nb * np.cos(np.radians(theta)))  # 5.000000000000001 — same number, rounded differently

# face 2b: the rectangle |a| x shadow(b on a)
shadow = (a @ b) / na
print(na * shadow)                        # 5.0

# the derivation, numerically: |a-b|^2 = |a|^2 + |b|^2 - 2 a.b
print(((a - b)**2).sum(), (a**2).sum() + (b**2).sum() - 2 * (a @ b))   # 5.0 5.0

# face 3: Lambert brightness is a clamped dot product of unit vectors
N = np.array([0., 1., 0.]); L = np.array([0.6, 0.8, 0.])
print(max(0.0, N @ L), max(0.0, N @ np.array([0.6, -0.8, 0.])))   # 0.8 0.0

# machine layer: summation order changes fp32 results in the last digits
rng = np.random.default_rng(0)
x = rng.standard_normal(1_000_000).astype(np.float32)
y = rng.standard_normal(1_000_000).astype(np.float32)
print(np.dot(x, y), np.dot(x[::-1], y[::-1]), float(np.sum((x * y).astype(np.float64))))
```

---

## 3. Term ids used by both lessons (for the implementer's `ids.ts`)

0.5: `C B A D E F a b c th rt rtD rtE rtF sq_a sq_b sq_c drop_a drop_b drop_c p q strip_a strip_b R1 R2 R1c R2c ab a2 b2 c2 corr rest rest2 rise rise2 bcos2 bsin2 ident pyth` and IK `S E W hand gap d phi cosphi cosraw cosc clamp reach reach_max reach_min l1 l2`.
1.4: `a b amb th rt na2 nb2 namb2 na2_l na2_r nb2_l nb2_r expand cross corr sum rhs dot rect shadow bright a1 a2 b1 b2 d1 d2 d1sq d2sq a1sq a2sq b1sq b2sq p1 p2 na nb cosT aa hp_pos hp_neg perp badge sgn_pos sgn_zero sgn_neg faces` and 3D `a3 b3 th3 perp3 shadow3 rect3 rt3 badge3 dot3 rhs3 rect3 cosT3 na3 nb3 a31… p33` and N·L `sphere term light L_dir P N thNL NdotL bright brightNL ndotl rectNL`.

All of these live in the lesson's `impl.tsx` as `const ID = {...} as const` and are referenced from scene, formula, table, N-panel row, status-hint and code props — a typo then fails `astro check`.

## 4. What lights when: cross-scene rules

- Within a lesson, scenes share the store, so hovering `a` in LC-1 lights `a` in LC-0 too if both are on screen. That is intended (one id, one thing).
- The Timeline sets `hot` to the current step's focus id on step change; a pointer hover overrides; on pointer-out the Timeline's light is restored (the Timeline subscribes to `hot` becoming empty and re-applies — implemented in the Timeline, not in lessons).
- Alias groups (`faces` in 1.4): the lesson maps member ids to the group id before calling `hotOn`, and every member view checks `useHot(group)`. The engine keeps lighting one id.
- N-panel rows, formula terms, table cells and status-bar hints are four views of the same ids; the status bar additionally shows the hint of the *hovered* object (not of the linked ones).
- Marks are computed by one `marksFor(step, vals)` per timeline in the lesson impl and passed to Formula/NumTable; no component decides visibility by itself. Piece placements for LC-0 are computed by one `placements(step, vals)` in the same file: a pure function returning the vertex list of every Poly for the given step (phases 6/7/8 return the sheared / rotated / final quads), so there is exactly one source for what the pieces look like.

## 5. Engine capabilities these scenes need beyond the listed contracts

Short and honest; everything else above composes Grid/Arrow/Point/Segment/AngleArc/RightAngle/AreaTile/Label/Formula(marks)/NumTable/Timeline/CodeView/Num. Items 10–12 are new in rev 2 (S1/S2 and the visual language).

1. **Custom handle mapping** — `Draggable.move?(vals, raw: readonly number[]) => Record<string, Val>`; when present, `DragLayer` and `nudge` call it instead of `moved()`, and `writeMove` accepts numbers as well as vectors. Needed by: LC-0/LC-1/LC-2 vertex A (polar → `b`, `th`) and vertex B (x → `a`), DP-2 `b` magnet-to-perpendicular, DP-4 `P` (sphere-constrained) and light (keep-outside). One optional field, no new primitive.
2. **Magnet helper** — `magnet(v: number, targets: readonly number[], zone: number): number` in `scene/drag.ts` (pure, tested): nearest target when `|v − t| ≤ zone`, else `v`. Used inside `move` functions; AngleArc gets `ticks?: readonly number[]` (0.08-radius radial dashes at the targets).
3. **HalfPlane** — `scene/HalfPlane.tsx` `{ lesson, id, normal: V | key, side: 'pos' | 'neg', role, opacity }`: 2D = a quad covering the stage extent clipped by the line through O; 3D = a single-sided disc of radius `extent` through O (render order below lines, `depthWrite: false`). Hover doubles the opacity. Needed by DP-2, DP-3.
4. **LitSphere** — `scene/LitSphere.tsx` `{ lesson, id, center: V, r, light: key, mode: 'lambert' | 'raw', roles: { base, dark, neg } }`: a `ShaderMaterial` computing per-fragment `ndotl` for a point light at `light` (uniform), `lambert` = `mix(dark, base, max(0, ndotl))`, `raw` adds `mix(dark, neg, -ndotl)` for negatives. Flat canvas, no scene lights. The lamp glyph uses the v2 palette role `light`; `dark` is `vp-bg-bottom`, `neg` is `axisZ`.
5. **Ledger** — `viz/Ledger.tsx` (DOM SVG) `{ lesson, items: { id, value: (vals) => number, role, label }[], total: { id, label, value }, scale }`: stacked bars with a signed last item (hatched when negative) and a result rule. LC-1 only here; 1.3/1.5 reuse it for Pythagoras tiles.
6. **Challenge** — `viz/Challenge.tsx` (DOM) `{ lesson, n, text, hint?: string[], check: (vals, step) => boolean }`: sticky success, progress key `challenge:<lesson>:<n>`; the Timeline writes `step['<name>:max']` so step-based checks are plain store reads.
7. **Circle** — `scene/Circle.tsx` `{ lesson, id, center: V | (vals) => V, r: number | (vals) => number, normal?: (vals) => V, role, dashPx? }`: 64-segment `SLine` loop (2D in the plane; 3D with a normal; `arcPoints` sweeps ≤ 180°, so a full loop needs its own parametric list). LC-3 reach rings, DP-4 terminator.
8. **Arrow leg ids** (nice-to-have) — `legs` exposing `${id}:x`, `${id}:y`, `${id}:z` so a component cell lights one leg. Fallback specified above: the whole arrow.
9. **Checkbox row** — a `kind: 'bool'` N-panel row (item 11) writing 0/1 to a numeric key (`ik_flip`, `nl_raw`); no separate Toggle component.
10. **Poly** (S1/S2 core) — `scene/Poly.tsx` `{ lesson, id, pts: (vals, step) => V[], role, fill?: number, hatched?: (vals) => boolean, edge?: 'solid' | 'dashed' | 'none', label?: (vals) => string }`: a filled polygon (2D, or 3D coplanar) whose vertices are a pure function of the store and the timeline step; vertex count must be constant for one id (quads here). On a *step* change the vertices tween with the spring (180 ms; a chained sequence of placements is expressed by the lesson as sub-placements with a `phases` count on the step, each phase 180 ms); on a *store* change they jump. Polygons with `|area| < 1e-9` are hidden and keep their label at `pts[0]`. Negative = hatched: a shared `hatchMaterial(role)` (screen-space 45° stripes, 6 px period, via a small shader; also used by AreaTile for its signed variant so the two never drift). Rotation about a point is just a vertex list, so no transform API is needed.
11. **ViewportCard + NPanel** (DOM, visual-language v2) — `viz/ViewportCard.tsx` wraps a Stage with the header (`mode`, `projection: 'persp' | 'ortho' | 'fixed'`, `overlays: { id, label, default }[]`, `name`), the N-panel and the status bar; `NPanel` rows: `{ id, label, kind: 'num' | 'vec' | 'bool' | 'ro', key?, spec?: ScrubSpec, snap?: number[] /* Ctrl targets */, value?: (vals) => number | V, fmt?, glyph?: 'rect' | 'disc' | 'sum', visible?: (vals, step) => boolean, group?: string }[]`. Rows carry term ids (hover → `hotOn`), JetBrains Mono tabular figures, hot outline when lit; `vec` rows render one field per component and write the whole vector through `store.set`. Overlay toggles are plain booleans the scene reads to show/hide object groups (never touching the store). Status hints: `hints: Record<handleId | objectId, string>` plus `idle` and optional state-dependent entries `(vals) => string | null` evaluated first. `glyph` draws the three-faces icons (a rectangle following `shadow`/`na`, a luminance disc, the product columns); DP-2's page-side "three faces" strip reuses the field cell at `size: 'lg'`. At < 720 px NPanel renders as the chip row. Tooltips (500 ms) on rows take `{ title, text, tex }`.
12. **Formula `narrow`** — `Formula { tex, texNarrow?: string }`: the component picks `texNarrow` when its container is < 420 px (ResizeObserver), for the long `aligned` lines of LC-0 and DP-1. Alternative considered and rejected: KaTeX has no automatic line breaking in `aligned`; a second TeX string is the honest fix.
13. **Segment `band`** — `Segment { band?: boolean }`: the 6 px soft shadow band (`muted` role, 35% alpha, round caps) used for every projection. Could be composed from a wide `SLine` with low alpha; one prop keeps the look identical across lessons.

Not needed: any animation library (vertex spring + CSS strike), a 3D text font, a physics step, a polygon boolean/clipping library (every cut here is a quad with known corners), per-stage Canvas, or new store fields (`step['cut:max']` is a plain step key). The navigation gizmo, the dark viewport gradient, the fading shader grid, the beam strokes and the selection outline are Stage/SLine-level visual-language work that every lesson inherits, not lesson needs.
