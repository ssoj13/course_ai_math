# Design: reference section (Part 0 + Part 1) — storyboard, term slice, primitives, risks

Sources read: `math_en/task.md`, `math_en/AGENTS.md`, `math_en/CURRICULUM.md`, `vllm_en/AGENTS.md`, `vllm_en/src/lib/course.ts`, `vllm_en/src/lib/element.ts`, `vllm_en/astro.config.mjs`, `vllm_en/src/data/course.json`. Library claims verified against drei docs (View), KaTeX docs (HTML extension), Chromium/WebKit sources (WebGL context cap).

## 1. Scope verdict: **Part 0 (compressed) + Part 1**

Build **0.G + 0.3 + 0.4 + 0.5 + 1.1–1.8** (12 lesson pages), where 0.G merges 0.1 and 0.2 into one short "Ground" lesson. Reasons:

1. **The section must stand alone.** 1.3 (length in n-D) is Pythagoras; 1.4 *is* the law of cosines rewritten. Shipping Part 1 alone means the centerpiece lesson starts with "recall the law of cosines", which this reader explicitly does not recall. The curriculum's own rule ("every lesson answers the question the previous one left open") breaks at the first page.
2. **Part 0 is the cheapest, best test-bed for the engine.** The unit circle (scrub θ → cos/sin light up in formula, numbers, scene) and the Pythagoras dissection (timeline) exercise every primitive (Stage, Arrow, Scrub, Formula term-linking, Timeline, Grid) in the simplest possible setting. Bugs in the linking model surface here, not inside the dot-product derivation.
3. **It is only +4 pages,** and 0.G is deliberately short: "square = area" (seed of "why squares") and "point = (x, y) on a plane". Slope/lines (0.2) is moved to 4.1 where it is actually needed; nothing in Part 1 uses slope.

Cost: the reader waits longer for the first "real" lesson. Mitigation: the roadmap lets a confident reader jump to 1.1; each Part 1 page links back to the exact Part 0 scene it relies on (`lesson:law-of-cosines#correction-term`), using the same `lesson:` link mechanism as vllm_en.

## 2. Lesson-by-lesson storyboard

Conventions used below. **Page template** (fixed order, every lesson): Hook → Scenes (each with a one-sentence "what to drag") → "Say it simpler" box → Hand exercises (answers behind a reveal) → Quiz → Python cell (Pyodide, lazy; "Open in Colab" badge) → Machine layer (collapsible `<details>`) → "Next question" → Local term map (1–2 hops). **Linking**: every scene publishes values to a per-lesson store; formula terms, table cells, code lines and scene objects share **term ids**; hovering/selecting any of them sets `hot` and all four views highlight. **Timeline** = stepped, scrubbable derivation (never autoplay).

### 0.G — Ground: squares, roots, the plane (merged 0.1 + 0.2)
- **Hook:** "Why do we call x² a *square*?"
- **Scenes:** (1) Scrub `x` in the formula `x² = ?`; a literal square of side x is drawn on the grid; area readout; drag x negative — the square is the same, the area is still positive (plant the seed: *squares forget the sign*). (2) Number line → plane: drag a point, `(x, y)` updates in the formula and in a 2-row numbers table; axes and the two dashed "legs" to the axes light up with the matching coordinate. (3) `√` as the inverse: scrub area, side appears.
- **CG analogy:** Maya viewport grid, world origin; Houdini `@P.x`, `@P.y`.
- **Simpler:** "x² is the area of a square of side x. A point is two readings on two rulers."
- **Exercises:** 7², (−3)², √49, √(2²+2²)? (leave as √8 ≈ 2.83), plot (−2, 3).
- **Quiz (misconception):** (−3)² = −9 (sign); √(a²) = a always (should be |a|); "a point and a pair of numbers are different things" (they are one thing seen two ways).
- **Python:** `x = np.arange(-3, 4); x**2` and plot points with matplotlib (or a plain table if matplotlib is too heavy for the first cell — decide in the Pyodide spike).
- **Machine:** `x*x` is one multiply; `sqrt` is a hardware instruction but several times slower than a multiply (roughly 3–10× on current CPUs) and *not* needed to compare sizes — why CG code compares squared distances (`dist2 < r*r`).
- **Next question:** How far apart are two points?

### 0.3 — Distance and Pythagoras (2D → 3D → n-D)
- **Hook:** Two draggable points; "how far?"
- **Scenes:** (1) Drag P, Q: the right triangle with legs `dx`, `dy` appears; **area tiles**: a square drawn on each leg and on the hypotenuse; the three areas are shown as numbers (`dx²`, `dy²`, `c²`) that light up with the corresponding formula terms. (2) **Dissection proof** as a Timeline: four copies of the triangle rearrange inside a (a+b)² square, step/scrub, showing a² + b² = c² without algebra. (3) 3D: the same twice — floor diagonal, then up; orbit camera; formula grows to three terms. (4) n-D: a vector of differences as a row of numbers, each squared, summed, rooted; "nothing changes except the count."
- **CG analogy:** `length(p − q)`, `distance()`, Houdini `@dist`; "Manhattan vs Euclidean" = city grid vs flying.
- **Simpler:** "Walk east, then north; the straight path is shorter; Pythagoras says exactly how much."
- **Exercises:** (0,0)–(3,4); (1,2)–(4,6); 3D (1,1,1)–(2,3,3)=3; which is farther from origin: (5,0) or (3,4)? (tie).
- **Quiz:** distance = dx + dy (Manhattan trap); √(a²+b²) = a + b; sign of dx matters (it doesn't once squared); n-D distance "needs geometry we can't picture" (it doesn't — same formula).
- **Python:** `np.linalg.norm(p - q)`, manual `np.sqrt(((p-q)**2).sum())`, show both equal.
- **Machine:** `np.hypot` / `std::hypot` avoid overflow/underflow of the squares (e.g. legs ~1e200 or ~1e−200 in fp64; ~3e19 in fp32); squared-distance comparisons; fp16 note deferred to 1.3.
- **Next question:** Distance says *how far*, not *which way*.

### 0.4 — Angles, radians, the unit circle
- **Hook:** "How do we describe a direction with numbers?"
- **Scenes:** (1) Point on the unit circle, scrub θ (drag the point or the number in `θ = 60°`): `cos θ` is the x-shadow (dashed drop to the x-axis), `sin θ` the y-shadow; both terms, both table cells, both dashed segments share colors. Degrees ↔ radians toggle: the arc length literally unrolls onto a ruler (radian = arc length on a radius-1 circle). (2) Unroll the graphs: the circle on the left, the cos/sin curves on the right, drag on either — the other follows. Negative cos past 90° is visible as the shadow flipping to the left. (3) Special angles table (0, 30, 45, 60, 90, 120, 180) with snap.
- **CG analogy:** Maya rotates in degrees, shaders/Houdini VEX in radians; `sin(time)` oscillation; `atan2` recovers the angle from (x, y).
- **Simpler:** "cos θ is how much of a length-1 arrow points along x; sin θ how much along y."
- **Exercises:** cos 60°, sin 90°, 180° in radians, cos 120°, which θ gives (cos, sin) = (0, −1)?
- **Quiz:** cos θ can exceed 1; cos of an obtuse angle is positive; `np.cos(60)` returns cos of 60° (it's radians); "radians are harder" (radian = arc length, the natural unit).
- **Python:** `np.cos(np.radians(60))`, `np.degrees(np.arctan2(y, x))`.
- **Machine:** sin/cos are range-reduction + polynomial approximations (`__sinf` fast intrinsic in CUDA/GLSL, LUTs in old games); precision collapses for huge angles — wrap first.
- **Next question:** What if the triangle has no right angle?

### 0.5 — Law of cosines = Pythagoras + a correction term
- **Hook:** Two sticks of length a, b joined at angle θ; how long is the gap c?
- **Scenes:** (1) Drag θ (and a, b): `c² = a² + b² − 2ab cos θ`; the term `−2ab cos θ` is highlighted in its own color and shown as a signed bar next to the a², b² area tiles from 0.3; at 90° the bar is zero and the formula visibly *becomes* Pythagoras (the term fades); at 0° c = |a − b|; at 180° c = a + b. (2) **Proof Timeline**: drop the perpendicular, split into two right triangles, Pythagoras twice, substitute `b cos θ`, collect — each step highlights the segment it talks about. (3) CG scene: **two-bone IK** — drag the hand target, the elbow angle is solved with exactly this formula; `acos` argument shown live with the clamp.
- **CG analogy:** 2-bone IK (knee/elbow), bone-chain reach.
- **Simpler:** "Pythagoras, plus a fix-up for a corner that isn't square. The fix-up is bigger when the corner is sharper."
- **Exercises:** a=3,b=4,θ=90° (5); θ=60° (√13); θ=120° (√37); given a=b=1, c=1 find θ (60°).
- **Quiz:** sign of the correction when θ > 90° (c grows); plugging degrees into cos; "the law of cosines needs a right angle"; which side is opposite θ.
- **Python:** `np.sqrt(a*a + b*b - 2*a*b*np.cos(np.radians(th)))`; sweep θ and plot c.
- **Machine:** `acos` of a value slightly outside [−1, 1] after rounding → NaN; every IK solver clamps. Range/precision of `acos` near ±1.
- **Next question:** The two sticks are arrows. What if we treat them as arrows everywhere?

### 1.1 — A vector: an arrow and a list of numbers
- **Hook:** "Same arrow, two descriptions."
- **Scenes:** (1) Drag the arrow tip: `[x, y]` in the formula and table update; the colored dashed legs to the axes are the components. (2) Drag the *whole* arrow by its shaft: numbers don't change — a vector has no address. Toggle "position" (anchored at origin) vs "direction" (free). (3) 3D arrow with orbit camera, three colored legs, `[x, y, z]`. (4) Vector between two points: drag A and B, `v = B − A` with each component's subtraction highlighted.
- **CG analogy:** `MPoint` vs `MVector` (Maya), `@P` vs `@N`/`@v` (Houdini); normals ignore translation; "the w = 1 / w = 0 trick" (named only, explained in 2.3).
- **Simpler:** "An arrow is 'go this much right and this much up'. Where you start doesn't matter."
- **Exercises:** vector from (1,2) to (4,6); components of a 3D arrow; is (2,3) and "(2,3) starting at (5,5)" the same vector? (yes); dimension of [1,0,0,0].
- **Quiz:** a vector has a location; B − A equals A − B; "dimension" means length; a point and a vector are interchangeable always (position = vector *from the origin*).
- **Python:** `np.array([3., 4.])`, `.shape`, `B - A`.
- **Machine:** three contiguous float32 = 12 bytes; `float3` in std140/HLSL cbuffers is padded to 16 — the first appearance of "layout matters" (prepares 3.3).
- **Next question:** What can we *do* with arrows?

### 1.2 — Adding, scaling, linear combinations
- **Scenes:** (1) Head-to-tail: drag a, b; the sum drawn both as the parallelogram diagonal and as "walk a then b"; table shows `aₓ + bₓ` etc. with pairwise highlight. (2) Scale: scrub `s` in `s·a`; negative flips; the components scale in the table. (3) `s·a + t·b` with two scrubs; a "paint" toggle that stamps every reachable point as the reader scrubs → the plane fills (span preview, name not yet given). (4) 3D version with three vectors.
- **CG analogy:** blend shapes = base + Σ wᵢ·deltaᵢ (literally a linear combination — show a 2D "face" of 6 points morphing); velocity += gravity·dt.
- **Simpler:** "Adding = chaining the walks. Scaling = same direction, different length (negative = turn around)."
- **Exercises:** (1,2)+(3,−1); 3·(2,−1); 2a − b for given a, b; find s, t with s(1,0)+t(0,1) = (3,4).
- **Quiz:** |a + b| = |a| + |b| (only when aligned); scaling changes direction (only sign); a − b is "the arrow from a to b" (it's from b to a); linear combination coefficients must be positive.
- **Python:** `a + b`, `2*a`, `s*a + t*b`; note NumPy does it componentwise for any length.
- **Machine:** SIMD: a 4-wide add is one instruction (`_mm_add_ps`, GPU `vec4`); AVX = 8 fp32 lanes; this is why "add two vectors" is cheap and "add a million" is a loop of 125 k instructions, not a million.
- **Next question:** How long is an arrow in n dimensions?

### 1.3 — Length, Σ notation, normalization
- **Scenes:** (1) Pythagoras tiles reused: a square on each component; `|v|² = Σ vᵢ²` **unfolded by a Timeline**: step i highlights component i, its tile, and the term `vᵢ²` as the running sum grows. (2) Normalize: scrub a "blend" from v to v/|v|, the tip lands on the unit circle (2D) / unit sphere (3D); the formula `v̂ = v / |v|` with the divisor highlighted. (3) n-D: 8-component bar chart; length readout; dragging bars updates it — "no picture needed, the same formula."
- **CG analogy:** `normalize()`; the zero-length normal NaN; `length` vs `length2`.
- **Simpler:** "Length = Pythagoras with as many legs as there are components. Normalizing = keep the direction, make the length 1."
- **Exercises:** |(3,4)|; |(1,1,1)| = √3; normalize (0,5); normalize (1,1); |−v| vs |v|.
- **Quiz:** normalizing changes direction; |(1,1)| = 2; |−v| = −|v|; a vector of length 0 can be normalized.
- **Python:** `np.linalg.norm(v)`, `v / np.linalg.norm(v)`, check `norm(v_hat) == 1.0` (show 0.9999999 and discuss).
- **Machine:** `rsqrt` (fast inverse square root; `rsqrtss`, GPU `inversesqrt`), epsilon guard; **fp16 overflows at 65504: the sum of squares of a 100-element fp16 vector with entries ~30 overflows** → norms/LayerNorm are accumulated in fp32 even when weights are fp16 (first hook to the vllm course).
- **Next question:** How do we measure the *angle* between two arrows?

### 1.4 — The dot product, derived from the law of cosines (centerpiece)
- **Hook:** Two arrows a, b from the origin, angle θ between them. We can already measure |a|, |b| (1.3) and |a − b| (0.3). The law of cosines (0.5) connects them — let's see what falls out.
- **Scenes:** (1) **Derivation Timeline** (the key scene of the section). Triangle with sides a, b, a − b drawn. Steps: ① `|a−b|² = |a|² + |b|² − 2|a||b|cosθ` (0.5, each term lights the matching side/tile); ② expand the left side in components: `Σ(aᵢ−bᵢ)² = Σaᵢ² − 2Σaᵢbᵢ + Σbᵢ²` (the numbers table shows the expansion for the current a, b, every product `aᵢbᵢ` as a cell); ③ `Σaᵢ²` and `|a|²` are the same thing (1.3) — they cancel with a strike-through animation; same for b; ④ what's left: `Σaᵢbᵢ = |a||b|cosθ`. Name it: **a·b**. Both sides are shown as live numbers that stay equal while the reader drags a and b. (2) **Dot product explorer**: drag a, b; readouts: the component sum (table of products), `|a||b|cosθ`, and θ; sign flips past 90° (background tint), zero at 90° with a snap and a "⊥" badge; a·a = |a|² (link to 1.3). (3) 3D with orbit. (4) **N·L lamp**: a sphere shaded with `max(0, N·L)`; drag the light; one highlighted point on the sphere shows its N, the L arrow, θ and the number driving its brightness.
- **CG analogy:** Lambert `dot(N, L)`; back-face test `dot(N, V) < 0`; "how much do two arrows agree."
- **Simpler:** "Multiply matching components, add them up. The result is big when the arrows agree, zero when they're perpendicular, negative when they disagree."
- **Exercises:** (1,2)·(3,4); (1,0)·(0,1); (2,2)·(1,−1); angle between (1,0) and (1,1); check |a|² = a·a for (3,4).
- **Quiz:** the dot product is a vector; a·b = 0 means one of them is zero; a·b ≠ b·a; for unit vectors a·b can exceed 1; "we need θ to compute a·b" (no — components suffice; that's the point).
- **Python:** `np.dot(a, b)`, `a @ b`, `(a*b).sum()`; `np.degrees(np.arccos(np.clip(a@b/(norm(a)*norm(b)), -1, 1)))`.
- **Machine:** `acc = fma(aᵢ, bᵢ, acc)` chain; 8 lanes + horizontal reduction; **summation order changes the rounding** — cell demonstrates `np.dot` vs a Python loop vs reversed order on 1e6 fp32 values differing in the last digits; BLAS `sdot`; tensor cores accumulate fp16 products in fp32.
- **Next question:** The formula tells us the number is |a||b|cosθ. What does that number *look like*?

### 1.5 — Projection: shadows on an axis
- **Scenes:** (1) Light from directly "above" b: the shadow of a on the line of b has length `|a| cos θ = a·b̂`; drag a — the shadow slides, goes negative past 90°; formula terms `a·b̂` and the shadow segment share a color. (2) The vector projection `(a·b̂) b̂` and the leftover `a − proj`, with the right angle marked; Pythagoras again: `|a|² = |proj|² + |perp|²` (tiles). (3) **Coordinates are shadows**: project a onto e₁ = (1,0) and e₂ = (0,1): the shadows are exactly `aₓ`, `a_y`; the table reads `a·e₁ = aₓ`. This is the "aha" the curriculum names. (4) 3D: shadow onto a plane = two projections.
- **CG analogy:** slide along a surface (remove the normal component), reflection `r = d − 2(d·n)n`, "project the velocity onto the ground."
- **Simpler:** "The dot product with a unit arrow is the length of the shadow. Coordinates are shadows on the axes."
- **Exercises:** project (3,4) onto (1,0); onto (1,1)/√2; perpendicular remainder of (3,4) w.r.t. (1,0); scalar vs vector projection of (2,1) onto (0,1).
- **Quiz:** the projection onto b depends on |b| (not after normalizing — show the two formulas); the scalar projection is a vector; projection is always shorter than a (yes, |cos| ≤ 1 — a true statement used as a reasoning check); the remainder is parallel to b.
- **Python:** `proj = (a @ bh) * bh`, `perp = a - proj`, `proj @ perp` ≈ 0.
- **Machine:** projecting onto k unit vectors = k dot products = one small matrix–vector product (GEMV) — first glimpse of a matrix as "many dot products at once."
- **Next question:** We used the x and y axes. Which axes are allowed?

### 1.6 — Basis, span, independence, dimension (orthonormal change of basis only)
- **Scenes:** (1) Two draggable basis arrows u₁, u₂; the grid of all `s·u₁ + t·u₂` (integer s, t) is drawn — drag and the grid shears (the direct preview of 2.1). (2) Collapse: drag u₂ onto the line of u₁ → the grid flattens to a line; "span = a line, the vectors are dependent, dimension 1"; a dependence meter (|cross| / area, name not given yet). (3) 3D: three arrows; when coplanar, the span is a plane (drawn as a translucent quad). (4) **Change of basis with an orthonormal basis**: rotate a pair of perpendicular unit axes; a fixed point's new coordinates are its *shadows* on the new axes (1.5) — the two tables (standard vs new coordinates) side by side; a note that non-perpendicular bases need "undoing a matrix" → 2.7.
- **CG analogy:** object space vs world space; tangent space (T, B, N) for normal maps is a basis; UV space.
- **Simpler:** "A basis is a set of rulers. Coordinates are the readings. Rulers that point the same way are useless (dependent)."
- **Exercises:** express (3,1) in the basis (1,1)/√2, (1,−1)/√2 (shadows); are (1,2), (2,4) independent? dimension of span{(1,0,0), (0,1,0), (1,1,0)}; coordinates of (2,3) in basis (0,1), (1,0).
- **Quiz:** a basis must be perpendicular; three vectors in 2D can be independent; dimension = how many vectors you wrote down; changing basis changes the arrow (it changes only its *description*).
- **Python:** `B = np.stack([u1, u2])`, `B @ v` for orthonormal B (shadows); `np.linalg.matrix_rank` (named, not explained).
- **Machine:** for an orthonormal basis the inverse is the transpose (free); repeated rotations drift in fp32 → re-orthonormalize (Gram–Schmidt named, A1).
- **Next question:** Can *meaning* be an arrow?

### 1.7 — Cosine similarity and embeddings
- **Scenes:** (1) Toy semantic space: 2D/3D arrows labeled "cat", "dog", "car"; drag, cosine readout, the angle arc. (2) **Real small table**: ~60 words × 16 dims (precomputed, stored as JSON in `src/data/`; source and generation script recorded — no fake data), shown as a heatmap of cosine similarities; hover a cell → the two word vectors appear as component bar rows with the products `aᵢbᵢ` (1.4 table) and the two arrows in a 3-PC projection (label it as a projection for display, not the real geometry). (3) Scale invariance: scrub a scale on one vector; cosine unchanged, Euclidean distance changes. (4) Nearest neighbours list recomputed live when the reader edits one coordinate.
- **CG analogy:** smooth-shading angle threshold `N₁·N₂ < cos(30°)` = hard edge; image/asset search by feature vectors.
- **Simpler:** "Cosine similarity ignores length and keeps only direction: 1 = same direction, 0 = unrelated, −1 = opposite."
- **Exercises:** cosine of (1,1) and (2,2) (1); of (1,0) and (0,1) (0); of (1,2) and (2,1); which pair is more similar: (3,4),(4,3) or (3,4),(6,8)?
- **Quiz:** cosine similarity is a distance; scaling an embedding changes its similarities; negative cosine always means "opposite meaning"; in high dimensions random vectors are typically nearly perpendicular (true — seed for 10.2).
- **Python:** normalize rows, `S = E @ E.T` — explicitly flagged "this `@` between matrices is Part 2; for now read it as all pairwise dot products at once."
- **Machine:** embedding table lookup = picking a row; pre-normalized vectors make cosine one dot; vector DBs quantize embeddings to int8/fp16 (link to branch Q).
- **Next question:** How do we move *all* arrows at once? (→ 2.1)

### 1.8 — Cross product (3D side branch)
- **Scenes:** (1) a × b in 3D: perpendicular result, length = parallelogram area (the parallelogram is filled and its area number is linked to |a × b|); swap order → flips; drag toward parallel → shrinks to 0. (2) 2D signed area `aₓb_y − a_yb_x`: positive when counter-clockwise, the sign visualized as the parallelogram tint (orientation). (3) Triangle normal from two edges, flip the winding order, the normal flips; "this is why back-faces go dark."
- **CG analogy:** face normals, `cross(T, B)`, winding order, Houdini `@N = normalize(cross(e1, e2))`.
- **Simpler:** "The cross product makes the arrow that sticks out of the plane of two arrows; its length is the area they span."
- **Exercises:** (1,0,0)×(0,1,0); (1,2,0)×(2,4,0); area of the parallelogram of (2,0),(1,3); normal of triangle (0,0,0),(1,0,0),(0,1,0).
- **Quiz:** a×b = b×a; cross product exists in 2D as a vector; cross of parallel vectors is undefined (it is zero); |a×b| = |a||b| (only when perpendicular).
- **Python:** `np.cross(a, b)`, `np.linalg.norm(np.cross(a,b))` as area.
- **Machine:** 6 mul + 3 sub, no sqrt; `dot(cross(a,b), c)` = volume — the determinant before its name (→ 2.6).
- **Next question:** Volume, orientation, "squashing" — all return in 2.6.

## 3. Term-graph slice (`terms.json` entries for this section)

Format: `id — "X is Y built on Z"` then edges. Depth: `g` ground, `v` vectors, `m` machine.

| id | definition | edges |
|---|---|---|
| `square` (g) | The square of x is the area of a square with side x. | built-on → `number` |
| `sqrt` (g) | The square root undoes squaring: the side of a square with a given area. | built-on → `square` |
| `coordinate` (g) | A coordinate is a reading of a point on one ruler (axis). | built-on → `number`; used-in → `point` |
| `point` (g) | A point is a place described by its coordinates. | built-on → `coordinate` |
| `distance` (g) | Distance is the straight-line length between two points, computed by Pythagoras. | built-on → `pythagoras` |
| `pythagoras` (g) | Pythagoras: the squared hypotenuse is the sum of the squared legs. | built-on → `square`; used-in → `distance`, `norm`, `law-of-cosines` |
| `angle` (g) | An angle is the amount of turn between two directions. | built-on → `point` |
| `radian` (g) | A radian is an angle measured as arc length on a circle of radius 1. | built-on → `angle`, `unit-circle` |
| `unit-circle` (g) | The unit circle is the set of points at distance 1 from the origin. | built-on → `distance` |
| `cosine` (g) | cos θ is the x-coordinate of the unit-circle point at angle θ (its shadow on x). | built-on → `unit-circle`, `coordinate`; used-in → `law-of-cosines`, `dot-product`, `cosine-similarity` |
| `sine` (g) | sin θ is the y-coordinate of the unit-circle point at angle θ. | built-on → `unit-circle`, `coordinate` |
| `law-of-cosines` (g) | The law of cosines is Pythagoras plus a correction −2ab cos θ for a non-right corner. | built-on → `pythagoras`, `cosine`; special-case-of ← `pythagoras` (θ = 90°); used-in → `dot-product` |
| `vector` (v) | A vector is an arrow with no fixed location, written as a list of components. | built-on → `coordinate` |
| `component` (v) | A component is a vector's shadow on one axis. | built-on → `vector`; equivalent-to → `coordinate`; equivalent-to → `projection` (onto a standard basis vector) |
| `scalar` (v) | A scalar is a plain number (it scales vectors). | is-a → `number` |
| `vector-add` (v) | Vector addition chains arrows head to tail, adding components. | built-on → `vector` |
| `scalar-mul` (v) | Scalar multiplication stretches an arrow, multiplying every component. | built-on → `vector`, `scalar` |
| `linear-combination` (v) | A linear combination is a sum of scaled vectors Σ sᵢ vᵢ. | built-on → `vector-add`, `scalar-mul`; used-in → `span`, `matrix-vector` (Part 2) |
| `sigma-notation` (v) | Σ notation is a compact way to write "add these up for i = 1..n". | built-on → `vector-add` |
| `norm` (v) | The norm (length) of a vector is Pythagoras over all its components. | built-on → `pythagoras`, `sigma-notation`; equivalent-to → `distance` (from the origin) |
| `unit-vector` (v) | A unit vector has length 1. | built-on → `norm` |
| `normalize` (v) | Normalizing divides a vector by its length to get a unit vector. | built-on → `norm`, `scalar-mul`; used-in → `cosine-similarity`, `projection` |
| `dot-product` (v) | The dot product Σ aᵢbᵢ equals \|a\|\|b\|cos θ; it measures agreement of two arrows. | built-on → `law-of-cosines`, `norm`, `sigma-notation`; used-in → `projection`, `cosine-similarity`, `orthogonal`, `angle-between` |
| `angle-between` (v) | The angle between vectors is acos of the dot product of their unit vectors. | built-on → `dot-product`, `normalize` |
| `orthogonal` (v) | Two vectors are orthogonal when their dot product is 0. | built-on → `dot-product`; special-case-of → `angle-between` (θ = 90°) |
| `projection` (v) | The projection of a onto b is the shadow of a on b's line: (a·b̂) b̂. | built-on → `dot-product`, `unit-vector`; used-in → `component`, `change-of-basis` |
| `standard-basis` (v) | The standard basis is the unit arrows along the axes. | is-a → `basis`; built-on → `unit-vector` |
| `span` (v) | The span of vectors is everything their linear combinations reach. | built-on → `linear-combination` |
| `linear-independence` (v) | Vectors are independent when none is a combination of the others. | built-on → `linear-combination`, `span` |
| `basis` (v) | A basis is an independent set of vectors that spans the space. | built-on → `span`, `linear-independence` |
| `dimension` (v) | Dimension is the number of vectors in any basis. | built-on → `basis` |
| `coordinates-in-basis` (v) | Coordinates in a basis are the coefficients that rebuild a vector from that basis. | built-on → `basis`, `linear-combination` |
| `orthonormal-basis` (v) | An orthonormal basis is a basis of mutually orthogonal unit vectors. | is-a → `basis`; built-on → `orthogonal`, `unit-vector` |
| `change-of-basis` (v) | Changing basis re-describes the same arrow with different rulers; for an orthonormal basis the new coordinates are projections. | built-on → `coordinates-in-basis`, `projection`; used-in → `matrix` (Part 2) |
| `cosine-similarity` (v) | Cosine similarity is the dot product of two normalized vectors: direction agreement, length ignored. | built-on → `dot-product`, `normalize`; equivalent-to → `cosine` of `angle-between` |
| `embedding` (v) | An embedding is a vector that stands for a word, image or token so that similar things point the same way. | built-on → `vector`; used-in → `cosine-similarity`, `attention` (Part 10) |
| `cross-product` (v) | The cross product of two 3D vectors is the perpendicular arrow whose length is the area they span. | built-on → `vector`, `orthogonal`; used-in → `normal`, `determinant` (Part 2) |
| `normal` (v) | A normal is a unit vector perpendicular to a surface. | built-on → `cross-product`, `normalize` |
| `signed-area` (v) | Signed area is the 2D cross product; its sign is the orientation (winding). | built-on → `cross-product`; used-in → `determinant` |
| `fma` (m) | FMA computes a·b + c with one rounding. | used-in → `dot-product` |
| `simd-lane` (m) | A SIMD lane is one of several numbers processed by a single instruction. | used-in → `vector-add`, `dot-product` |
| `rounding` (m) | Rounding is the error from storing a real number in finite bits. | used-in → `summation-order`, `normalize`; built-on → `float` (branch Q) |
| `summation-order` (m) | Summation order changes a floating-point sum because rounding is not associative. | built-on → `rounding`; used-in → `dot-product` |
| `fp16-overflow` (m) | fp16 overflows above 65504, so sums of squares must be accumulated in fp32. | built-on → `rounding`; used-in → `norm` |

Build-time checks (from AGENTS.md): all referenced ids exist, `built-on` is acyclic; the one deliberate cycle-looking pair is `component` ≡ `coordinate` ≡ projection — those are `equivalent-to`, not `built-on`, so the DAG holds.

## 4. Minimal reusable primitives (and what NOT to build yet)

All React (`src/viz/`), TypeScript strict, props are plain data; every id string is a **term id** shared across views.

**State**
- `createLessonStore(id, initial)` → zustand vanilla store `{ vals: Record<string, number|number[]>, set(k, v), hot: Set<string>, setHot(ids) }`. One store per lesson page, module-level singleton keyed by lesson id so separate islands share it (see risk R2).
- `useVal(k)`, `useHot(id)` hooks.

**Scene (r3f)**
- `Stage` — `{ mode: '2d'|'3d', bounds: [w,h] | fitRadius, height?: number (CSS), interactive?: boolean, caption: string (a11y + fallback text) }`. In `2d`: OrthographicCamera, no orbit, pixel-stable line widths. In `3d`: PerspectiveCamera + OrbitControls (drei) with damping off, zoom clamped. Renders inside a drei `<View>` (R1). Pulls palette from CSS vars (R4).
- `Grid` — `{ step, extent, minor?, axes?: boolean }`.
- `Arrow` — `{ id, from?: Vec, to: Vec | valKey, color: role, label?: string, drag?: 'tip'|'whole'|'none', legs?: boolean (dashed component legs) }`. Drag writes to the store; `hot` highlights.
- `Point` — `{ id, at, drag?, label? }`.
- `Segment` — `{ id, a, b, dashed?, color }` (shadows, drop lines, triangle sides).
- `AngleArc` — `{ id, at, from, to, label? }` and `RightAngle` marker.
- `AreaTile` — `{ id, side: number|valKey, along: Vec, color }` (squares on legs; signed bar variant for the correction term).
- `Parallelogram` — `{ id, a, b, fill }` (1.2, 1.8, 1.6 span).
- `Label` — drei `Html` for short text/number next to an object (KaTeX allowed for one symbol; no heavy use).

**Prose/formula**
- `Formula` — `{ tex, terms: Record<termId, {color?: role}>, scrub?: Record<valKey, {min,max,step}> }`. KaTeX `renderToString` with `trust: true`, `strict: 'ignore'` for `htmlExtension`; `\htmlData{t=dot}{a\cdot b}` → `[data-t]`; hover/click toggles `hot`; `\htmlData{scrub=theta}{60^\circ}` renders a drag-to-scrub number (pointer capture + ArrowUp/Down keys).
- `Num` — scrubbable number in prose (same engine as Formula's scrub).
- `NumTable` — `{ rows: [{id, label, valKey|values}], ops?: 'product'|'square'|'diff' }` — the "numbers" view; cells carry term ids.
- `Timeline` — `{ steps: {id, text, tex?}[] }` — step/back/scrub slider; publishes `step` to the store; scenes and formulas read it to reveal/strike terms. Respects `prefers-reduced-motion` (no tweens, hard cuts).
- `CodeView` — static highlighted snippet with `data-t` on lines (links to hot). No execution.
- `PyCell` — Pyodide in a Web Worker, lazy on click, `numpy` only, "Open in Colab" fallback.
- `TermMap` — `@xyflow/react` + elkjs (layered), input `{ focus: termId, hops: 1|2 }`, reads `terms.json`; lazy (`client:visible`), elk in a worker.

**Reused as-is from vllm_en** (custom elements, no React): `Quiz`, `LessonDone`, `ProgressExport`, `ColabBadge`, Sidebar/PageTitle/Footer overrides, `course.json` + `course.ts`, `remark-lesson-links`, `lab.css` tokens, `store.ts` (progress), `element.ts`. Two component models on one page is fine; the only rule is `not-content` on card roots (their trap list).

**NOT needed yet (do not build):** TS `core/` Tensor + autodiff (first use is 3.3/5.6); MatmulCube, MemoryStrip, Jacobian zoom, surface/contour plots; computation-graph view of xyflow (only the term map); genealogy/depth-stack/flashcards views of the term graph (local map + build validation only); a generic "linked views" framework beyond the store (`hot` + `vals` is the framework); Rust/wasm; heatmap GPU widget (1.7's 60×60 cosine table is an HTML table with background colors); drei `Text` (troika) — use `Html` labels; animation library (Timeline is stepped; transitions are CSS/`useFrame` lerps of ≤ 200 ms).

## 5. Architecture risks and mitigations

**R1 — Many r3f scenes per page vs WebGL context limit.** Verified: Chromium caps active contexts (16 per domain on desktop, 8 on Android, oldest context lost; console "There are too many active WebGL contexts on this page"). A lesson has 3–5 scenes, the home/roadmap could have more, and Starlight keeps pages in one document. Mitigation (recommended): **one `<Canvas>` per page + drei `<View>` per scene** (verified in drei docs: views share a single canvas via `gl.scissor`, track their DOM element on scroll/resize, `View.Port` inside the Canvas, `eventSource` set to the common container; r3f ≥ 8.1 required when the canvas isn't fullscreen). Place the Canvas host in a Starlight **`PageFrame` override** (fixed, `inset:0`, `pointer-events:none`, z-index below content; views re-enable pointer events). **Spike A (must run first):** confirm that `<View>` instances living in *separate Astro islands* (different React roots) are picked up by the `View.Port` in the host island — drei uses a module-level tunnel store, so it should work when Vite bundles drei into one shared chunk, but this is **[unverified]**. Fallback if it fails: one `<Canvas frameloop="demand">` per scene island, `client:visible`, unmount on leaving the viewport (IntersectionObserver), hard cap 6 scenes/page. Either way use `frameloop="demand"` and `invalidate()` on store changes — no continuous rendering on a textbook page.

**R2 — Cross-island state.** Islands are separate React roots; the zustand store must be a module singleton. Works when all islands import the same module from one chunk (Vite dedupes) — verify in Spike A as well. Astro's documented route for island-to-island state is nanostores; zustand vanilla is the same mechanism (module-level store); if Spike A shows duplication, switch to nanostores without touching components (the hooks are the only API).

**R3 — Astro SSR + three.js.** Anything importing three/r3f/drei is `client:only="react"` (r3f `Canvas` cannot SSR). Reserve layout with CSS `aspect-ratio` on the island wrapper to avoid CLS, and render the `caption` text server-side as the placeholder/a11y fallback (the island wrapper is an `.astro` component that prints the caption and mounts the React scene). KaTeX `Formula` *can* SSR via `renderToString` in the island (`client:load` with hydration) — keep static formulas in MDX via `rehype-katex` and use the island only for linked/scrubbable ones.

**R4 — Theme (Starlight light/dark) inside WebGL.** Starlight toggles `data-theme` on `<html>` and exposes `--sl-color-*` custom properties (Starlight docs point to `props.css` for the list; the exact `data-theme` attribute name is from Starlight's ThemeSelect source — **[verify in `@astrojs/starlight` source once installed]**). Mitigation: a single `palette.ts` with semantic roles (`vecA`, `vecB`, `axis`, `grid`, `hot`, `shadow`) defined as CSS vars in `lab.css` for both themes; scenes read them via `getComputedStyle` on mount and on a `MutationObserver` of `data-theme`, and call `invalidate()`. No hard-coded hex in components. Color-blind-safe pair for a/b (blue/orange), not red/green.

**R5 — KaTeX term highlighting.** Verified: `\htmlId`, `\htmlClass`, `\htmlStyle`, `\htmlData{k=v}{…}` exist, are off by default, need `trust: true` (or a `trust` callback allowing only `\htmlData`/`\htmlClass`) and loosening `strict` for `htmlExtension`. Use `\htmlData{t=…}` (not `\htmlId`) because the same term appears in several formulas on a page — ids must stay unique, data attributes need not. Author formulas as TeX strings in a per-lesson `formulas.ts`, never inline in MDX (brace-escaping traps in `.astro`/MDX are already in vllm's notes). Scrubbable numbers: `\htmlData{scrub=theta}{60^\circ}` → the span gets pointer handlers; the TeX is re-rendered on value change (KaTeX render of one formula is ~1 ms; fine at pointer-move rate, throttle with rAF).

**R6 — Pyodide.** Load only on click from the jsDelivr CDN (never bundle), in a Web Worker, with a visible progress line (core + numpy is tens of MB on first load; browser-cached afterwards). GitHub Pages cannot set COOP/COEP headers, so no SharedArrayBuffer — irrelevant for numpy (single-threaded) but rules out any threaded wasm later. Pin a version at implementation time; the version string returned by the docs fetch today ("314.0.7") looks implausible — **[unverified; check pyodide.org release list]**. Each cell has the Colab badge as the escape hatch.

**R7 — Bundle size on GitHub Pages.** Budgets: shared viz chunk (three + r3f + drei subset) ≤ 250 kB gz; per-lesson code ≤ 100 kB gz; KaTeX JS ~100 kB + only the fonts actually used; xyflow + elkjs loaded lazily (elk.bundled is ~1.4 MB raw — worker + lazy, `client:visible` on the map). Import drei pieces individually (`@react-three/drei/core/...` is **[unverified]** as a public subpath — check; otherwise rely on tree-shaking and measure). Add a `bun run build` size report in CI and fail on budget regressions, like vllm's `chunkSizeWarningLimit`.

**R8 — Accessibility.** Every scene has three non-visual twins by construction: the Formula, the NumTable and the caption; that is the accessible representation, not an afterthought. Draggables are focusable (`tabIndex`, roving focus inside a scene) and respond to arrow keys; scrub numbers are `role="slider"` with `aria-valuenow`; Timeline is a `<input type="range">` + buttons; `prefers-reduced-motion` disables lerps.

**R9 — Mobile (360 px).** 2D stages fit width with a fixed aspect; 3D stages get a "tap to interact" overlay so page scroll isn't hijacked (`touch-action` is only disabled while a drag is active); hit targets for arrow tips ≥ 24 px via invisible hit spheres; formula + scene stack vertically; the term map opens full-screen. Follow vllm's rule: no horizontal page scroll at 360 px.

**R10 — Starlight + React islands styling.** Starlight's `.sl-markdown-content` leaks margins into siblings; keep vllm's `not-content` rule on every island root and the `lab.css` primitives. Add `@astrojs/react` to the integrations; islands and custom elements coexist.

**R11 — "No fake data" for 1.7.** The embedding table must be real and reproducible: a small build script (Python, in `notebooks/build/`, mirroring vllm's generated-notebook discipline) that extracts ~60 word vectors from a stated public model/file, reduces to 16 dims, and writes `src/data/embeddings-small.json` with provenance in the file. Document the source; don't hand-type numbers.

**Spike order before any lesson content:** A (View across islands + shared store) → B (KaTeX trust/scrub) → C (theme vars → three) → D (Pyodide worker on Pages) → E (bundle budget). Each spike is a throwaway page in the repo, one day each at most.

## 6. Curriculum ordering changes (brief)

1. **Merge 0.1 + 0.2 → 0.G** ("squares, roots, the plane"); move "line y = kx + b and slope" into 4.1 where it is first needed. Nothing in Parts 0–1 uses slope.
2. **1.6: orthonormal change of basis only.** The general change of basis needs the inverse (2.7). Say so explicitly and leave a forward link; otherwise 1.6 silently requires solving a linear system.
3. **1.4: add "a·a = |a|²" and the sign of the dot product** as named checkpoints (both are used by 1.5, 1.7, 8.3 and 10.2).
4. **1.7 uses `E @ E.T`** — keep it, but label it as a preview of Part 2 and also show the explicit loop of dot products so the reader isn't asked to trust an unexplained operator.
5. **1.8 stays, marked as a branch** in the roadmap (dashed edge to 2.6); its "next question" points to 2.6, not 2.1. Part 1's main open question ("move all arrows at once") must come from 1.7.
6. **0.5: make two-bone IK the CG anchor** — it is the one place a CG developer has already used the law of cosines without knowing its name; it converts the whole Part 0 from "school revision" to "that thing in my rig."
7. **Machine layer continuity:** 0.G (squared-distance comparisons) → 0.3 (`hypot`) → 1.3 (fp16 overflow, fp32 accumulation) → 1.4 (FMA, summation order) forms a single thread that lands in branch Q; name it as a thread in the roadmap so the "zoom down" promise is visible from lesson one.

Sources consulted for library claims: [drei View docs](https://drei.docs.pmnd.rs/portals/view), [KaTeX supported functions (HTML extension)](https://katex.org/docs/supported.html), [Pyodide loading packages](https://pyodide.org/en/stable/usage/loading-packages.html), [Starlight CSS guide](https://starlight.astro.build/guides/css-and-tailwind/), [Chromium WebGL context limit review](https://codereview.chromium.org/14217005/), [Mozilla bug 1748592 (per-platform limits)](https://bugzilla.mozilla.org/show_bug.cgi?id=1748592), [WebKit max-active-contexts test](https://webkit.googlesource.com/WebKit/+/master/LayoutTests/webgl/max-active-contexts-oldest-context-lost-expected.txt).
