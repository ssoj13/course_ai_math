# Design: reference section (Part 0 + Part 1) — storyboard, term slice, primitives, risks

Sources read: `math_en/task.md`, `math_en/AGENTS.md`, `math_en/CURRICULUM.md`, `vllm_en/AGENTS.md`, `vllm_en/src/lib/course.ts`, `vllm_en/src/lib/element.ts`, `vllm_en/astro.config.mjs`, `vllm_en/src/data/course.json`. Library claims verified against drei docs (View), KaTeX docs (HTML extension), Chromium/WebKit sources (WebGL context cap).

## 1. Scope verdict: **Part 0 (compressed) + Part 1**

Build **0.G + 0.3 + 0.4 + 0.5 + 1.1–1.8** (12 lesson pages), where 0.G merges 0.1 and 0.2 into one short "Ground" lesson. Reasons:

1. **The section must stand alone.** 1.3 (length in n-D) is Pythagoras; 1.4 *is* the law of cosines rewritten. Shipping Part 1 alone means the centerpiece lesson starts with "recall the law of cosines", which this reader explicitly does not recall. The curriculum's own rule ("every lesson answers the question the previous one left open") breaks at the first page.
2. **Part 0 is the cheapest, best test-bed for the engine.** The unit circle (scrub θ → cos/sin light up in formula, numbers, scene) and the Pythagoras dissection (timeline) exercise every primitive (Stage, Arrow, Scrub, Formula term-linking, Timeline, Grid) in the simplest possible setting. Bugs in the linking model surface here, not inside the dot-product derivation.
3. **It is only +4 pages,** and 0.G is deliberately short: "square = area" (seed of "why squares") and "point = (x, y) on a plane". Slope/lines (0.2) is moved to 4.1 where it is actually needed; nothing in Part 1 uses slope.

Cost: the reader waits longer for the first "real" lesson. Mitigation: the roadmap lets a confident reader jump to 1.1; each Part 1 page links back to the exact Part 0 scene it relies on (`lesson:law-of-cosines#correction-term`), using the same `lesson:` link mechanism as vllm_en.

## 2. Lesson-by-lesson storyboard

Note (2026-10-10): the ids "0.1" and "0.2" in §1, §2 and §6 are the pre-2026-10-10 numbering (0.1 squares, 0.2 slope).
Since CURRICULUM.md 2661c02, **0.2 is the Powers lesson** (`powers.mdx`); its scenes are in §7.

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

Note: "0.2" in item 1 is the old slope lesson, not the Powers lesson 0.2 of §7.

1. **Merge 0.1 + 0.2 → 0.G** ("squares, roots, the plane"); move "line y = kx + b and slope" into 4.1 where it is first needed. Nothing in Parts 0–1 uses slope.
2. **1.6: orthonormal change of basis only.** The general change of basis needs the inverse (2.7). Say so explicitly and leave a forward link; otherwise 1.6 silently requires solving a linear system.
3. **1.4: add "a·a = |a|²" and the sign of the dot product** as named checkpoints (both are used by 1.5, 1.7, 8.3 and 10.2).
4. **1.7 uses `E @ E.T`** — keep it, but label it as a preview of Part 2 and also show the explicit loop of dot products so the reader isn't asked to trust an unexplained operator.
5. **1.8 stays, marked as a branch** in the roadmap (dashed edge to 2.6); its "next question" points to 2.6, not 2.1. Part 1's main open question ("move all arrows at once") must come from 1.7.
6. **0.5: make two-bone IK the CG anchor** — it is the one place a CG developer has already used the law of cosines without knowing its name; it converts the whole Part 0 from "school revision" to "that thing in my rig."
7. **Machine layer continuity:** 0.G (squared-distance comparisons) → 0.3 (`hypot`) → 1.3 (fp16 overflow, fp32 accumulation) → 1.4 (FMA, summation order) forms a single thread that lands in branch Q; name it as a thread in the roadmap so the "zoom down" promise is visible from lesson one.

Sources consulted for library claims: [drei View docs](https://drei.docs.pmnd.rs/portals/view), [KaTeX supported functions (HTML extension)](https://katex.org/docs/supported.html), [Pyodide loading packages](https://pyodide.org/en/stable/usage/loading-packages.html), [Starlight CSS guide](https://starlight.astro.build/guides/css-and-tailwind/), [Chromium WebGL context limit review](https://codereview.chromium.org/14217005/), [Mozilla bug 1748592 (per-platform limits)](https://bugzilla.mozilla.org/show_bug.cgi?id=1748592), [WebKit max-active-contexts test](https://webkit.googlesource.com/WebKit/+/master/LayoutTests/webgl/max-active-contexts-oldest-context-lost-expected.txt).


## 7. Scenes for 0.G and 0.2 (squares are areas; powers)

Status: scene design for implementation (2026-10-11, rev 3 after recovery review: ladder geometry, TileGrid contract, clay look
per visual-language v3, complete store tables, real store API). Same precision as `lessons-law-of-cosines-dot-product.md`
§1; all conventions of its §0 apply (ids/derive, `fmt`, drag/snap/keyboard, ViewportCard layout, motion, challenge boxes).
Lesson 0.2 was added to the curriculum after §2 above was written (user request: "a square is an area — show it, maybe as a
3D widget; explain fractional and negative powers too"); its prose is `src/content/docs/ground/powers.mdx`, and the slot
ids below are the `{/* scene: ... */}` comments in `squares-and-plane.mdx` (0.G-1..3) and `powers.mdx` (0.2-1..5).

Existing engine API (`src/viz`, 6e): `lessonStore` with `vals`/`shown`, single-key `set(key, v, how)`,
`Challenge.check(vals, step)`, N-panel rows `num | vec | ro | bool`, and `Draggable.move(vals, raw, src)`.
Current `writeMove` writes multiple keys sequentially and is used by handles, not N-panel/Formula/Num.
This section requires the future linked atomic store contract of 7.13 for all those writers; scene delivery is
blocked until that contract is implemented and tested. Timeline/NumTable remain Task 7 contracts (planned).
The tables below specify the complete intended behavior, including required engine additions; this branch is content/spec only.

### 7.0 Look (visual-language v3)

Every object in 0.G-1, 0.G-3 and 0.2-1..0.2-4 is a countable solid, so those six cards are **clay cards**
(`look: 'clay'` on the ViewportCard: light studio background `#D6DAE1 → #C4C9D2`, no stars, grid `#B9BFC9`, same chrome).
Tiles and cubes are `ClayTile`/`ClayCube` bodies (matcap, `RoundedBoxGeometry` bevel 6 % of the smallest side, footprint
sprite under the bottom layer): the square of side x is `clayB` (sky), the root-driven square of 0.G-3 / 0.2-4 is `clayC`
(lilac), partial tiles are `clayAccent` (orange, the reference's "cut out" tiles), labels sit on `clayPlate` plates in
`clayInk`. Measured sides are **measurement arrows** (dimension lines in `clayInk`, 1 px, end ticks, value in mono) with the
drag handle at the far end — not glowing filaments, which would be lost on the light background. Hot = the orange outline
around the body silhouette (v3 clay rule). 0.G-2 (a point and two legs) and 0.2-5 (curves) draw lines, not solids: they are
**holo cards** (deep-space background, filaments, glass plates), roles `vecA`/`vecB`, axis colours for the legs.
A card never mixes both backgrounds.

### 7.1 Shared primitive: `TileGrid` (new, `scene/TileGrid.tsx`)

The one object this section draws: a box or a row of unit pieces, clipped exactly to a real-valued size, counted.

**Props.**

| prop | type | meaning |
|---|---|---|
| `id` | string | term id of the whole (hover outlines the whole shape, lights `area`/`xn`) |
| `shape` | `{ kind: 'box', size: [sx, sy, sz], thin: [boolean, boolean, boolean] }` \| `{ kind: 'row', len, wrap }` | **box**: unit pieces fill `[0, sx] × [0, sy] × [0, sz]`; only an explicit `thin: true` marks a display-only axis (one 0.12-thick layer, not counted). Active dimensions retain their exact size, even 0.1; no size threshold. **row**: `len` unit pieces laid along +x, wrapping to a new row (−y in 2D, +z in 3D) every `wrap` pieces; `len` may be fractional (last piece clipped). |
| `plane` | `'xy' \| 'xz'` (default `'xy'`) | 2D stages draw in the xy plane; a box lying on a 3D floor uses `'xz'` (its thin axis is y). Ignored for a full 3D box. |
| `fill` | number in [0, 1], optional | for a `row` with `len ≤ 1`: ONE unit outline (thin `clayInk` frame) with a body of width `fill` filled from the left — the "fraction of a tile" of the ladder's negative rungs. `len > 1` with `fill` throws. |
| `origin` | V (default 0) | low corner |
| `role` | clay role | body colour; partial pieces always `clayAccent` |
| `count` | boolean | publishes `whole`, `partial`, `pieces` (below) |
| `label` | boolean | centre plate with the total (`fmt`: integer when the size is integer, else 2 decimals) |
| `negative` | `'mirror'`, optional | a negative size draws the same shape mirrored through the origin on that axis (0.G-1 only); without it a negative size throws (a design bug, not a reader state) |

**Geometry.** One `InstancedMesh` of `RoundedBoxGeometry` unit bodies; instance `(i, j, k)` is scaled to
`min(1, sx − i) × min(1, sy − j) × min(1, sz − k)` (explicit thin axis: 0.12; active mirrored sizes use abs(size)) and anchored at its low corner, so the union of the
pieces IS the shape and partial pieces are clipped exactly (a 2.5 side gives 0.5-wide pieces, never a scaled 1-unit body).
Capacity: active box dimensions use `ceil(abs(size))`, each at most 4; explicit thin axes use 1.
Rows use `ceil(len)` instances with `wrap ≤ 16` controlling arrangement only. The largest
instance count is 64 for a cube and 27 for the largest ladder rung (base 3, exponent 3); one draw call per TileGrid. Row capacity is `ceil(len)`, not wrap times a box limit.
Matrices are rebuilt in `useFrame` only when the shown size changes (`moved` check; one module-level `Object3D`
temporary; no allocation per frame).

**Counting (`count`).** For boxes take the product over active axes only: `pieces = ∏ceil(abs(size))`,
`whole = ∏floor(abs(size))`, `total = ∏abs(size)`, `partial = total − whole`; explicit thin axes contribute 1.
For rows without fill: `pieces = ceil(len)`, `whole = floor(len)`, `total = len`; with fill: `pieces = fill > 0 ? 1 : 0`,
`whole = fill === 1 ? 1 : 0`, `total = fill`, and always `partial = total − whole`. The optional outline is not a counted body. These are
derived values (`derive(vals)`), not store keys; the scene's N-panel rows `whole`/`partial` and the formula terms read them.

**Material and transparency (decision).** Bodies are **opaque** clay (`MeshMatcapMaterial`, pastel `color`, no
`transparent`). Translucent instanced cubes are rejected: three.js does not sort instances inside an `InstancedMesh`, so
18 % faces with depth writes flicker by draw order and without depth writes 4×4×4 faces pile up into an opaque blob. With
opaque bodies the interior is hidden and the count is carried by the bevel seams between bodies (matcap shading makes
every seam a visible dark line, ≥ 1.5 px at the default camera) plus the per-piece edge frame in 2D. Acceptance check:
at `x = 3`, default 3D camera, the three layers of the cube are distinguishable by their seams; at `x = 2.5` the
orange partial slabs on three faces read as "cut".

**Instance-count transitions (0.2-2 morph, 0.2-3 splitting).** The instance count is the max over the two states; a
body that exists only in the target starts at `min(0.12, target clipped size)` on the growing axes and springs to its exact target size; a body that
exists only in the source springs to `min(0.12, source clipped size)` and is then hidden (render instance count set after the spring; numeric counts always describe the target truth). Nothing pops.

**Hover.** The whole shape gets the hot outline (never per piece). `whole`/`partial` rows: the matching pieces keep
their colour, the others use `mix(cardBackground, bodyColor, 0.35)` in their per-instance colour attribute. Alpha stays 1, depth writes stay on; this is a colour dim, never transparency.

**Degenerate.** A size of 0 draws nothing; labels read `0`; counts 0.

Other primitives used here, all existing unless marked: `Stage`, `ViewportCard` (Header, NPanel, StatusBar, Gizmo),
`Grid`, `Segment`, `Point`, `Arrow`, `AreaTile`, `RightAngle`, `Label`, `Formula` (`\t{}`, `\s{}`), `Num`, `Challenge`,
N-panel `bool` rows; `Timeline`, `NumTable` (Task 7, planned); **new**: `TileGrid` (above), `FnCurve` (0.2-5: a
pixel-stable polyline of `y = f(t)` over an interval on `Stroke`), the measurement arrow with a drag handle (v3 instrument
+ `Draggable`), clay card mode. All listed in 7.13.

### 7.2 Store `squares-and-plane` (0.G)

| key | type | initial | range / step | written by |
|---|---|---|---|---|
| `x` | number | 3 | [−4, 4] / 0.1 | `\s{x}` scrub (prose and formula), N-panel field, side handle drag (0.G-1); 0.G-3 writes it from `A` (always ≥ 0) |
| `A` | number | 9 | [0, 16] / 0.25 | N-panel field and `\s{A}` (0.G-3); 0.G-1 writes it from `x` |
| `P` | [x, y] | [−2, 3] | stage extent, snap 0.1 | point drag (0.G-2), two N-panel fields |

Both scenes write `x` and `A` together through the future store-level linked patch contract (7.13), including N-panel, Formula, Num and handles: 0.G-1's handle/field writes
`{ x: v, A: v·v }`; 0.G-3's field writes `{ A: v, x: sqrt(v) }`. So a negative `x` set in 0.G-1 becomes positive the
moment 0.G-3 writes `A` (the root is never negative); that is intended and the 0.G-3 status bar says so.

Derived (one `derive(vals)`; every id below appears in a formula, row or challenge):

| id | value | used in |
|---|---|---|
| `x2` | `x·x` | 0.G-1 formula, row |
| `whole` | `floor(|x|)²` | 0.G-1 row, formula line 2 |
| `partial` | `x2 − whole` | 0.G-1 row, formula line 2 |
| `sqrtA` | `sqrt(A)` | 0.G-3 formula, row, side label |
| `check` | `sqrtA²` | 0.G-3 formula, row |
| `px`, `py` | `P[0]`, `P[1]` | 0.G-2 fields, legs, formula |
| `px2`, `py2`, `sum` | `px²`, `py²`, `px2 + py2` | 0.G-2 rows, formula line 2, challenge 2 |

### 7.3 Scene 0.G-1 — "Why is x² called a square?" (lead scene, draws itself on first view, clay card)

**Purpose.** The number `x²` is the count of unit tiles in a square of side `x`. Aha: the tiles ARE the number; a
side of 2.5 has partial tiles and the formula still gives the exact area; a negative side draws the same square on the
other side of the origin and the area stays positive (squares forget the sign).

**Stage.** 2D, `extent = 4.5` (x = ±4 → the square reaches ±4, 10 % margin), centred `[0, 0]`. Grid step 1, axes on
(axisX/axisY, so "negative x" is visibly a direction).

**Objects.**

| id | primitive | geometry | role | label | drag |
|---|---|---|---|---|---|
| `sq` | TileGrid | `shape: box [x, x, 0], thin [false, false, true]`, `plane: 'xy'`, `negative: 'mirror'`, `count`, `label` | clayB (partials clayAccent) | `x² = 9` plate at the centre | — |
| `side` | measurement arrow + handle at `[x, 0]` | `[0, 0] → [x, 0]`, 14 px below the bottom edge | clayInk | `x = 3.0` in the middle (negative: the arrow points left, value `−3.0`) | handle: x only, `move` → `{ x: clamp(snap(px), −4, 4) }`; linked resolver derives A |
| `o` | Point | `[0, 0]` | clayInk | `0` | — |

**ViewportCard.** Header: `2D` · `Ortho` · `Overlays ▾` (grid on, tiles on, labels on, axes on) · `Reset view` ·
`0.G-1 · Square`. N-panel rows: `x` num [−4, 4; 0.1; Ctrl snaps 0.5] (writes `{ x, A }`) · separator AREA · `x2` ro (`x²`) ·
`whole` ro (`whole tiles`) · `partial` ro (`partial area`, `visible: partial > 0`). Status bar: over the handle — "LMB drag
the side · x can be negative"; over the square — "x² = 9 · 9 whole tiles"; idle — "Wheel zoom · MMB pan · N panel".

**Formula.** `\t{x2}{x^2} = \s{x} \cdot \s{x} = \s{x2}`; a second line while `partial > 0`:
`= \t{whole}{\s{whole}} \text{ whole tiles} + \t{partial}{\s{partial}}`.

**Hover map.** `x`: the measurement arrow + handle + `\s{x}` + field. `x2`: square outline + its plate + `x^2` + `\s{x2}` +
row. `whole`: whole tiles stay, partials dim with the 35 % body-colour mix of 7.1. `partial`: the reverse.

**Edge cases.** `x = 0`: no tiles, arrow of zero length (value `0.0`), plate `x² = 0`. `x = −3`: square on `[−3, 0]²`,
plate `x² = 9` (positive), arrow value `−3.0` to the left; row `x2` reads `9.00`. `x = 2.5`: 4 whole + 2.25 partial
(5 orange pieces), `x2 = 6.25`.

**Challenges** (`check(vals)`). 1. "Make the square hold exactly 16 tiles." `|x| === 4`. 2. "Make the area positive
with a negative side." `x < 0` (card: "the square is drawn to the left and down; its area is still x²").

**Motion.** First view: the arrow grows from the origin (300 ms), then the tiles rise into place row by row (300 ms;
`DrawIn`); reduced motion: the final state at once. Drags 1:1; field arrows/typed values ease (Ruling U).

### 7.4 Scene 0.G-2 — Two rulers, one point (holo card)

**Purpose.** A point is two readings; the two dashed legs are its coordinates, drawn. Aha: drag the dot and the numbers
change; type the numbers and the dot moves; there is nothing in between.

**Stage.** 2D, `extent = 5`, centred `[0, 0]`, grid step 1, axes on with tick labels every 1 (muted mono); rulers
overlay on (v3 instruments: the vertical ruler is the y scale).

**Objects.** `P` Point (ink, 6 px, label `(−2, 3)` live; drag free, snap 0.1); `legx` Segment dashed `P → [px, 0]`
(axisX colour, label `x = −2.0` at the foot); `legy` Segment dashed `P → [0, py]` (axisY colour, label `y = 3.0`);
`footx`, `footy` small Points (muted) at the feet; `rtx`, `rty` RightAngle at the feet (muted).

**ViewportCard.** Header `2D` · `Ortho` · `Overlays ▾` (grid, axes, legs, labels, rulers) · `Reset view` · `0.G-2 · Point`.
N-panel: `P` vec (two fields `x`, `y`, [−4.5, 4.5; 0.1]) · separator SQUARES · `px2` ro (`x²`) · `py2` ro (`y²`) · `sum` ro
(`x² + y²`). Status: over P — "LMB drag P · arrows nudge 0.1 · Shift 1.0"; over a leg — "x leg: the x coordinate, −2.0";
idle — "Drag P or type x, y in the N panel".

**Formula.** `P = (\s{px},\; \s{py})`; second line (the hand-off to 0.3): `\t{px2}{x^2} + \t{py2}{y^2} = \s{px2} + \s{py2}
= \s{sum}`, dim until the reader hovers a SQUARES row.

**Hover map.** `px`: leg x + foot + its label + `\s{px}` + the x field; `py` likewise; `px2`/`py2`: the leg brightens and
a transient glass `AreaTile` (axis colour) is drawn on the leg for the hover; `sum`: both tiles.

**NumTable** (Task 7; 2 rows `x`, `y`, 1 column) under the formula: the "two readings" view, cells `px`/`py`.

**Edge cases.** `P` on an axis: one leg has zero length, its label reads `0.0`, its foot coincides with P. `P` at the
origin: both.

**Challenges.** 1. "Put P at (−2, 3) by typing, not dragging." `px === −2 && py === 3` (any route; the hint names the
N panel). 2. "Make x² + y² equal 13 with a different point." `sum === 13 && !(px === −2 && py === 3)` (e.g. (3, 2)).

### 7.5 Scene 0.G-3 — The root undoes the square (clay card)

**Purpose.** Reverse the direction: the reader sets the AREA, and the side appears. Aha: most areas give an ugly side
(√2 = 1.4142…); the side is never negative.

**Stage.** As 0.G-1 (`extent = 4.5`), centred `[2, 2]`, axes off. Reuses `TileGrid` with size `sqrtA` — the same square
as 0.G-1, here driven by `A`.

**Objects.** `sq` TileGrid (`box [sqrtA, sqrtA, 0], thin [false, false, true]`, clayC, partials clayAccent, `count`, plate `A = 9`); `side`
measurement arrow `[0, 0] → [sqrtA, 0]`, value `√A = 3.0`, **no handle** (the side is the OUTPUT; the status bar says
so); `o` Point.

**ViewportCard.** Header `2D` · `Ortho` · `Overlays ▾` · `Reset view` · `0.G-3 · Root`. N-panel: `A` num [0, 16; 0.25;
Ctrl snaps to 0, 1, 4, 9, 16] (writes `{ A, x: sqrt(A) }`) · separator SIDE · `sqrtA` ro (`√A`, 4 decimals, so √2 shows
`1.4142`) · `check` ro (`(√A)²`, 2 decimals: the root squared returns the area). Status: over the square — "Scrub A in the
N panel · the side follows"; over the side — "√A = 1.4142 · output, not a handle · never negative"; idle — "Ctrl-drag A
snaps to perfect squares".

**Formula.** `\t{sqrtA}{\sqrt{\s{A}}} = \s{sqrtA}` and `\t{check}{(\sqrt{A})^2} = \s{check}`.

**Edge cases.** `A = 0`: nothing drawn, `sqrtA` 0. `A = 2`: side 1.4142, 1 whole tile + 3 partial tiles (partial area
1.00), `check` 2.00.

**Challenges.** 1. "Find an area whose side is a whole number, other than 9." `Number.isInteger(sqrtA) && A !== 9 && A > 0`.
2. "Make the side exactly 1.5." `A === 2.25`.

### 7.6 Store `powers` (0.2)

| key | type | initial | range / step | written by |
|---|---|---|---|---|
| `x` | number | 3 | [0, 4] / 0.1 | side handle, `\s{x}`, N-panel field (0.2-1, 0.2-2); the `A`/`V` fields (0.2-1, 0.2-4) through the root. Never negative (negative bases are prose-only) |
| `A` | number | 9 | [0, 16] / 0.25 | N-panel field + `\s{A}` (0.2-1, 0.2-4 2D); written from `x` otherwise |
| `V` | number | 27 | [0, 64] / 1 | N-panel field + `\s{V}` (0.2-4 3D); written from `x` otherwise |
| `from` | number 0/1/2 | 0 | — | which key the reader wrote last (0 = `x`, 1 = `A`, 2 = `V`); part of every patch below; read by 0.2-1 challenges |
| `n` | number (int), authoritative | 2 | 1..3 / 1 | Formula scrub, power Timeline adapter, power chips `x¹ x² x³` (0.2-2), keys 1/2/3, N-panel integer field |
| `base` | number | 2 | [1.5, 3] / 0.5 | N-panel field (0.2-3 only; its own key so the ladder base never fights the side `x`) |
| `d3` | number 0/1 | 0 | bool | header `2D / 3D` toggle and N-panel bool row (0.2-4) |
| `g` | number | 2.2 | [0.2, 32] / 0.1 | `\s{g}`, N-panel field (0.2-5); presets 2.2, 1/2.2, 8, 32 set it exactly (no rounding to step; all inside the same range) |
| `t` | number | 0.5 | [0, 1] / 0.01 | point drag along x, N-panel field (0.2-5) |
| `k` | number (int), authoritative | 3 | [−3, 3] / 1 | N-panel `kind: 'num', key: 'k', spec: { min: -3, max: 3, step: 1 }`, Formula scrub, ladder Timeline adapter |
| `step['ladder']` | mirrored 0..6 | 0 | — | read-only mirror `3 − k`; Timeline request i writes `k = 3 − i` via the linked adapter (7.13) |
| `step['power']` | 0..2 | 1 | — | read-only mirror `n − 1`; Timeline request i writes authoritative `n = i + 1` via the linked adapter (7.13), never a second independent source |

**One atomic linked write per interaction** (future store contract, 7.13): from `x`: `{ x, A: x², V: x³, from: 0 }`; from `A`: `{ A, x: sqrt(A),
V: sqrt(A)³, from: 1 }`; from `V`: `{ V, x: cbrt(V), A: cbrt(V)², from: 2 }` (clamped to the ranges first: `A = 20`
typed → `A = 16, x = 4, V = 64`). `x` is the canonical geometric size; `A` and `V` are synchronized writable projections and `from` records the input channel. `n` and `k` are authoritative keys; their timeline indices are read-only mirrors updated in the same transaction.

Derived (`derive(vals, step)`; every id appears in a formula, row, label or challenge):

| id | value | used in |
|---|---|---|
| `x2`, `x3` | `x²`, `x³` | 0.2-1 formula/row; 0.2-2 expansions |
| `xn` | `x^n` | 0.2-2 plate, formula, row |
| `whole`, `partial` | per TileGrid counting for the current shape | rows, formula line 2 (0.2-1), 0.2-2 rows |
| `layers` | `floor(x)` (n = 3 only) | 0.2-2 row, status |
| `sqrtA`, `check2` | `sqrt(A)`, `sqrtA²` | 0.2-1 line 2; 0.2-4 2D formula/rows |
| `cbrtV`, `check3` | `cbrt(V)`, `cbrtV³` | 0.2-4 3D formula/rows |
| ladder index | `3 − vals.k` (mirror in `step['ladder']`) | 0.2-3 Timeline; `k` itself is stored, not a derived duplicate |
| `p3 … pm3` | `base^j` for j = 3..−3 (seven ids: `p3, p2, p1, p0, pm1, pm2, pm3`) | 0.2-3 rung labels, rows |
| `cur` | `base^k` | 0.2-3 formula line 1 |
| `prevval` | `base^(k+1)` | 0.2-3 formula line 2 (hidden at k = 3) |
| `next` | `cur / base` (= `base^(k−1)`) | 0.2-3 "÷ base" annotation, step text |
| `y` | `t^g` | 0.2-5 point label, row, formula |
| `yinv` | `y^(1/g)` (reads back `t`) | 0.2-5 row, formula |

### 7.7 Scene 0.2-1 — "A square is an area" (lead scene of 0.2, clay card)

**Purpose.** Same square as 0.G-1 (the lesson says "the same tiles"), now with the root field next to it: the N-panel
has `x` AND `A`, and either one drives the other. Aha: the two fields are one fact read in two directions (x → x²
forward, √ backward); `x = 2.5` shows 4 whole tiles, two half pieces on the right, two on top and a quarter.

**Stage.** 2D, `extent = 4.5`, centred `[2, 2]` (positive quadrant: 0.2 has no negative sides), grid step 1, axes on.

**Objects.** `sq` TileGrid (`box [x, x, 0], thin [false, false, true]`, clayB, `count`, plate `x² = 9`); `side` measurement arrow + handle at
`[x, 0]` (value `x = 3.0`; drag: x only, `move` → `{ x: clamp(snap, 0, 4) }`; linked resolver derives A/V/from); `top` measurement arrow
(thin, no handle) along the top edge with value `x` (the second factor: the reader sees x·x, not x alone); `o` Point.

**ViewportCard.** Header `2D` · `Ortho` · `Overlays ▾` (grid, tiles, labels, axes) · `Reset view` · `0.2-1 · Square`.
N-panel: `x` num [0, 4; 0.1; Ctrl 0.5] · `A` num [0, 16; 0.25; Ctrl perfect squares] (label `x² = A`, a `√` glyph before
the field: typing here runs the root) · separator TILES · `whole` ro · `partial` ro (`visible: partial > 0`). Status: over
the handle — "LMB drag the side · or type A in the N panel"; over the square — "x² = 6.25 · 4 whole tiles + 2.25";
over the `A` field — "Type an area: the side becomes √A"; idle — "Wheel zoom · MMB pan · N panel".

**Formula.** `\t{x2}{x^2} = \s{x} \cdot \s{x} = \s{x2}` · second line `\s{x} = \t{sqrtA}{\sqrt{\s{A}}}` (always visible:
it is the lesson's point). Scrubbing `\s{A}` in the formula writes the `A` patch.

**Hover map.** As 0.G-1; `A`/`sqrtA`: the outline + the `A` field + the second formula line.

**Edge cases.** `x = 0`: nothing, `A = 0`. `A = 2` typed: `x = 1.4142`, 1 whole tile + 3 partial tiles (partial area
`1.00`). Typed values outside a range clamp (`A = 20` → `A = 16`, `x = 4`).

**Challenges** (`check(vals)`). 1. "Make the square hold exactly 4 whole tiles and nothing else." `x === 2`. 2. "Type an
area whose side is not a whole number, then read the side." `!Number.isInteger(x) && from === 1`. 3. "Get 6.25 without
typing it." `A === 6.25 && from === 0`.

**Motion.** First view: the arrow grows (300 ms) then the tiles rise (300 ms); reduced motion: cut. Drags and scrubs 1:1.

### 7.8 Scene 0.2-2 — The 3D widget: segment → square → cube (clay card)

**Purpose.** `x¹`, `x²`, `x³` are a length, an area, a volume. The reader morphs one object through the three with
the power chips (or the `power` timeline) and orbits the cube. Aha: the cube is layers of the square (27 = 3 layers of 9);
the count of unit pieces is `x^n` for every n; a side of 2.5 gives partial (orange) pieces.

**Stage.** 3D, `extent = 4.5`, camera at `[7, 5.5, 7]` looking at `[1.5, 1.5, 1.5]` (fits x = 4 with margin; set
explicitly — 7.13). Floor grid at y = 0, step 1; axes on (axisX/axisY/axisZ) with the gizmo top-right. Orbit on drag
(OrbitControls, damping off, `maxPolarAngle = 0.49π` so the floor never flips); touch rule as the engine does.

**Objects.** `obj` TileGrid, all three states are 3D boxes on the floor (y-up), low corner at the origin:

| n | `shape` | pieces | counted as |
|---|---|---|---|
| 1 | `box [x, 0.12, 0.12], thin [false, true, true]` | a row of unit-length sticks along x | `floor(x)` whole + partial length |
| 2 | `box [x, 0.12, x], thin [false, true, false]` | a slab of unit tiles on the floor (sweep along z) | `floor(x)²` whole tiles |
| 3 | `box [x, x, x], thin [false, false, false]` | unit cubes (lift along y) | `floor(x)³` whole cubes |

The morph between states is a pure scale of each body on its growing/shrinking axes from 0.12 to full (180 ms spring;
reduced motion: cut), with the instance-count rule of 7.1 (new bodies start at 0.12 thickness; departing ones shrink to
0.12, then hide). Role clayB, partials clayAccent. `side` measurement arrow along x on the floor with a handle at `[x, 0, 0]`
(drag: along world x via `rayPlane` on the floor; `move` → the `x` patch); edge values `x` on each visible edge of the
current shape (1, 2 or 3 arrows); plate on the top face: `x^n = \s{xn}` (text follows n: `x¹ = 3`, `x² = 9`, `x³ = 27`).

**ViewportCard.** Header `3D` (2D disabled) · `Persp` · `Overlays ▾` (grid, axes, edge arrows, floor) · `Reset view` ·
`0.2-2 · Cube`. N-panel: `x` num [0, 4; 0.1] · `n` integer chip field (`x¹ x² x³`, keys 1/2/3; 7.13) · separator COUNT ·
`xn` ro (label follows n) · `whole` ro (`whole pieces`) · `partial` ro (`visible: partial > 0`) · `layers` ro
(`visible: n === 3`, `floor(x)` layers of `floor(x)²`). Status: over the handle — "LMB drag along x"; over the shape —
"x³ = 27 · 3 layers of 9"; idle — "LMB orbit · Wheel zoom · 1/2/3 switch the power". 360 px: tap-to-interact overlay.

**Formula.** `\t{xn}{x^{\s{n}}} = \s{xn}` with `n` scrubbable (1..3); below it the expansion for the current n:
`= \s{x} \cdot \s{x} \cdot \s{x}` (n = 3), two factors (n = 2), `= \s{x}` (n = 1).

**Timeline `power`** (3 steps, mirrors `n`): 0 "A stick of length x: x¹." 1 "Sweep it sideways along z by x: a
square, x² tiles." 2 "Lift the square along y by x: a cube, x³ unit cubes, floor(x) layers of floor(x)² each."

**Hover map.** `x`: the side arrow + every edge arrow + field. `xn`: the whole shape outlined + plate + term.
`whole`/`partial` as in 7.1. `layers`: the cube's layers separate by 0.08 along y (exploded view) for the hover
(spring; cut under reduced motion).

**Edge cases.** `x = 0`: nothing drawn, counts 0. `x = 2.5`, `n = 3`: 8 whole cubes, partial 7.625, `xn = 15.625`.

**Challenges.** 1. "Make 27 unit cubes." `n === 3 && x === 3`. 2. "Find a side whose cube is smaller than its square."
`n === 3 && x > 0 && x < 1` (card: "between 0 and 1, more copies make it smaller"). 3. "Make a cube of 2 layers of 4."
`n === 3 && x === 2`.

### 7.9 Scene 0.2-3 — The ladder of powers (clay card)

**Purpose.** The exponent walks from 3 down to −3; each rung divides by the base, so x⁰ is one tile and x⁻¹ is a
fraction of a tile. Aha: nothing special happens at 0 or at the first negative rung; the division just continues; for
base 2 the tiles halve seven times, for base 3 they are cut in three.

**Layout (real ladder).** Seven rungs stacked top (k = 3) to bottom (k = −3), left-aligned at x = 0, each a TileGrid
`row` with `wrap = 8`; a rung's rows are 1 unit tall, rungs are separated by 0.5. Rung k ≥ 1: `row { len: base^k, wrap: 8 }`
(fractional `len` for non-integer bases: 2.5³ = 15.625 → 15 whole pieces + one 0.625 piece, orange). Rung k ≤ 0:
`row { len: 1, wrap: 8, fill: base^k }` — one unit frame with the left `base^k` of it filled (k = 0: full, `fill = 1`).
Labels in a column left of the tiles (x ∈ [−9, −0.5]): `\s{base}^{k} = \s{p_k}` on a clay plate per rung; between rungs,
at x = −0.5, a short down arrow. Its live `÷ \s{base}` label sits beside the arrow in the left column with at least 14 CSS px height, never inside the 0.5-unit gap. Labels reserve 90 CSS px at mono 11 px for stages ≥328 px. At a 320 px page (roughly 288 px stage), use a measured compact label budget ≤80 CSS px and keep arrow captions in a separate horizontal lane so they cannot overlap rung plates. Narrow cards omit repeated base from the plate (`k → value`), with full `base^k = value` in the row, formula and accessible text.

**Stage.** 2D, ortho; the tallest configuration is base 3: rung 3 = 27 pieces = 4 rows, rung 2 = 9 = 2 rows, rung 1 = 1
row, rungs 0..−3 = 1 row each → 11 rows + 6 gaps × 0.5 = 14 units; width 8 (tiles) + 9 (labels) = 17 units. Stage
`extent = 7.7` (half of 14 + 10 %; on the 16:10 viewport the half-width is then 12.3 ≥ 8.5, so everything fits without
panning at every base in [1.5, 3]), centred on `[−0.5, 0]` (tiles x ∈ [0, 8], labels x ∈ [−9, −0.5], rows from y = 7 down
to y = −7, the top-left tile of rung 3 at y = 6..7). Grid step 1, axes off. At 328–360 px stage width, the actual Stage margin gives roughly 11–12.5 px per unit, so the 8.5-unit label column provides 95–106 px. Use the compact labels and CSS-pixel arrow-label budget above; check text bounds at 320 and 360 px. Nothing scrolls.

**ViewportCard.** Header `2D` · `Ortho` · `Overlays ▾` (grid, labels, arrows) · `Reset view` · `0.2-3 · Ladder`.
N-panel: `base` num [1.5, 3; 0.5] · `k` num field (`key: 'k'`, integer spec −3..3; writes the authoritative key; Timeline mirrors `3 − k`)
· separator RUNGS · `p3 … pm3` ro (seven rows, labels `b³ … b⁻³` with the live base, 3 decimals for k < 0). Status: over a
rung — "`{base}^{k}` = `{p_k}` · `{pieces}` pieces" (k < 0: "`{fill}` of a tile"); over an arrow — "One step down divides
by `{base}`"; idle — "◀ ▶ step the ladder · base in the N panel".

**Formula.** `\t{cur}{\s{base}^{\s{k}}} = \s{cur}`; second line (hidden at k = 3):
`= \t{prevval}{\s{base}^{k+1}} / \s{base} = \s{prevval} / \s{base}`.

**Timeline `ladder`** (7 steps; index `3 − k`). Reached rungs `j > k` use the opaque 35 % colour mix of 7.1; current rung k has the hot outline. Not-yet-reached rungs `j < k` hide their bodies and values, preserving their layout slots and exponent labels. Their N-panel value rows use `visible: (_, step) => j >= 3 - step.ladder`; returning upward hides them again. Base edits recompute reached rungs without revealing hidden answers.
Step k → k−1 animation, generic in `base`: for k ≥ 1 "the rung's pieces fall into `base` equal groups; one group stays":
the pieces beyond index `base^(k−1)` shrink toward zero scale over 180 ms, with alpha fixed at 1 (a non-integer base leaves a clipped piece at the boundary),
then the kept group springs down into the next rung's row(s) as rung k−1 while rung k is redrawn in full above it (dimmed).
For k ≤ 0 "the single tile is cut into `base` equal parts; one part stays": the filled body shrinks from width `base^k`
to `base^(k−1)` (180 ms), the frame stays; non-integer bases cut into non-integer parts (1.5 parts of width 0.667).
Reduced motion: cuts. Step texts (`{base}` and the values are live):

| step | k | text |
|---|---|---|
| 0 | 3 | "`{base}`³ = `{p3}`: three copies of `{base}` multiplied. `{p3}` tiles." |
| 1 | 2 | "One rung down: divide by `{base}`. `{base}`² = `{p2}`." |
| 2 | 1 | "Again: `{base}`¹ = `{p1}`." |
| 3 | 0 | "Again: `{p1}` / `{base}` = 1. So `{base}`⁰ = 1 — one tile. Not zero: there is a tile here." |
| 4 | −1 | "Keep going: 1 / `{base}` = `{pm1}`. `{base}`⁻¹ is `{pm1}` of a tile. The minus sign says 'divide', not 'negative'." |
| 5 | −2 | "`{pm1}` / `{base}` = `{pm2}`: `{base}`⁻² = 1/`{base}`²." |
| 6 | −3 | "`{pm3}` of a tile. The ladder never stops; it just moves into fractions. Change the base in the N panel: 3⁰ is also one tile." |

**Hover map.** `pK`: rung + its plate + its row; `base`: every plate's base + field; `cur`: the current rung; `prevval`:
the rung above.

**Edge cases.** `base = 1.5`: rungs 3.375, 2.25, 1.5, 1, 0.667, 0.444, 0.296 — clipped pieces everywhere; 3 decimals.
`base` is never 0 or 1 (range starts at 1.5) so every rung is distinct. `base = 3`: rung 3 = 27 pieces = 3 rows of 8 + 3.
`base = 2.5`: rung 3 = 15.625 = 2 rows (8 + 7.625).

**Challenges.** 1. "Reach one tile." `k === 0`. 2. "Find the rung that is a quarter tile at base 2." `base === 2 && k === −2`.
3. "Make the b⁰ rung still read 1 with a different base." `k === 0 && base !== 2`.

### 7.10 Scene 0.2-4 — Fractional power: area → side, volume → edge (clay card, 2D/3D)

**Purpose.** `x^½` is the side of a square of area A; `x^⅓` is the edge of a cube of volume V. The reader scrubs the
AREA (or VOLUME) and watches the side follow. Aha: the forward direction (0.2-1/0.2-2) and the backward direction are the
same object; the root is the power whose "copies" must multiply back to the input.

**Stage.** `d3 = 0`: 2D (`extent = 4.5`, centred `[2, 2]`); `d3 = 1`: 3D (as 0.2-2). One ViewportCard whose header
`2D / 3D` toggle writes `d3`; the card mounts the 2D Stage or the 3D Stage accordingly (two Stages, one mounted at a
time; 7.13).

**Objects.** `sq` TileGrid (`box [sqrtA, sqrtA, 0], thin [false, false, true]`, clayC, in 2D; `box [cbrtV, cbrtV, cbrtV], thin [false, false, false]`, clayC, in 3D), plate
`A = 9` / `V = 27`; `side` measurement arrow along x, **no handle** (output), value `A^{1/2} = 3.0` / `V^{1/3} = 3.0`;
`o` Point.

**ViewportCard.** Header `2D ◧ 3D ◫` (live) · `Ortho`/`Persp` follows · `Overlays ▾` · `Reset view` · `0.2-4 · Root`.
N-panel: `d3` bool (`3D`) · 2D: `A` num [0, 16; 0.25; Ctrl perfect squares] · separator SIDE · `sqrtA` ro (`A^½`, 4
decimals) · `check2` ro (`(A^½)²`, 2 decimals). 3D: `V` num [0, 64; 1; Ctrl perfect cubes 0, 1, 8, 27, 64] · separator
EDGE · `cbrtV` ro (`V^⅓`, 4 decimals) · `check3` ro (`(V^⅓)³`). Rows of the other view are `visible: false`. Status:
over the shape — "Scrub A · the side is A^½"; over the side — "Output: the root"; idle — "2D / 3D in the header ·
Ctrl-drag snaps to perfect squares / cubes".

**Formula.** 2D: `\t{sqrtA}{\s{A}^{1/2}} = \sqrt{\s{A}} = \s{sqrtA}`, then `\t{check2}{(\s{sqrtA})^2} = \s{check2}`.
3D: `\t{cbrtV}{\s{V}^{1/3}} = \s{cbrtV}`, then `\t{check3}{(\s{cbrtV})^3} = \s{check3}`. The check line is the lesson's
"the copies multiply back" argument as numbers.

**Hover map.** `A`/`V`: shape outline + field + `\s{}`; `sqrtA`/`cbrtV`: the side arrow + its value + the SIDE/EDGE row;
`check2`/`check3`: the whole shape (it IS the check).

**Edge cases.** `A = 0`/`V = 0`: nothing drawn, roots 0. `V = 2`: edge 1.2599, 1 whole cube + 7 partial cubes (partial
volume 1.00). Switching `d3` keeps `x` (the shared key) so the 3D edge equals the 2D side the reader just set:
`A = 6.25` → `x = 2.5` → `V = 15.625` shown on switch.

**Challenges.** 1. "Find a volume whose edge is a whole number, other than 27." `d3 === 1 && Number.isInteger(cbrtV) &&
V !== 27 && V > 0`. 2. "Set the area so the edge in 3D is 2." `A === 4 && d3 === 1` (switch views and read `V = 8`).

### 7.11 Scene 0.2-5 — pow curves (gamma and specular; holo card)

**Purpose.** For a base between 0 and 1, a bigger exponent makes a SMALLER number. One curve `y = t^g` on [0, 1] with a
scrubbable `g`: at 2.2 the display → linear decode curve, at 1/2.2 the encode curve, at 32 the specular lobe that is
black except near 1. Aha: `g = 1` is the diagonal; `g` and `1/g` mirror across it (encode then decode returns the diagonal).

**Stage.** 2D, `extent = 0.6`, centred `[0.5, 0.5]` (the unit square fills the viewport with margin), grid step 0.1
(minor) / 0.5 (major), axes on with labels `0`, `0.5`, `1`; rulers overlay on.

**Objects.** `curve` FnCurve `y = t^g`, 65 samples, vecA filament; `diag` Segment `[0,0] → [1,1]` dashed muted (the `g = 1`
reference); `inv` FnCurve `y = t^{1/g}`, vecB, 1.5 px (Overlay "inverse", default on); `pt` Point on the curve at
`[t, y]` with dashed drops to both axes and label `t^g = \s{y}`; `pt` drag: along x only, writes `t` (snap 0.01).

**ViewportCard.** Header `2D` · `Ortho` · `Overlays ▾` (grid, diagonal, inverse curve, drops, rulers) · `Reset view` ·
`0.2-5 · pow`. N-panel: `g` num [0.2, 32; 0.1] with four preset chips under it: `2.2`, `1/2.2`, `8`, `32` (7.13) · `t` num
[0, 1; 0.01] · separator VALUES · `y` ro (`t^g`, 4 decimals) · `yinv` ro (`(t^g)^{1/g}`, 4 decimals: reads back `t`).
Status: over the curve — "y = t^2.2 · below the diagonal: darker"; over the point — "LMB drag along x"; idle —
"Presets: 2.2 decode · 1/2.2 encode · 8 / 32 specular".

**Formula.** `\t{y}{\s{t}^{\s{g}}} = \s{y}` and `\t{yinv}{(\s{t}^{\s{g}})^{1/\s{g}}} = \s{yinv}`.

**Hover map.** `g`: the curve + field + `\s{g}`; `y`: the point, its drops, the row; `yinv`: the inverse curve and the
mirrored point on it (drawn only during the hover).

**Edge cases.** `g = 32`, `t = 0.5`: `y` reads `0.0000` (true value 2.3e−10; the status bar shows the scientific form
`2.3e-10` so no reader thinks it is exactly zero; the 4-decimal readout first collapses to `0.0000` above g ≈ 14.3 at
t = 0.5). `t = 0`, `g = 0.2`: `0^0.2 = 0`; `t = 1`: every curve passes through 1. `g` never reaches 0 (range starts at
0.2) so `0^0` never appears here; the prose owns that warning.

**Challenges.** 1. "Make the two curves coincide." `g === 1`. 2. "Find g where 0.5 becomes 0.25." `g === 2 && t === 0.5`.
3. "Make the highlight: pick the preset where y(0.5) shows 0.0000." `g === 32 && t === 0.5` (the card says a highlight
needs a cosine very close to 1).

### 7.12 Reduced motion, 360 px, a11y (all scenes)

- `prefers-reduced-motion: reduce`: the tile rise-in, the layer explode, the stick → slab → cube morph, the ladder
  splitting and every spring are hard cuts; the "draws itself" moment of 0.G-1 and 0.2-1 is skipped; the hot pulse is static.
- 360 px: every card keeps 16:10; N-panel → chip row (ids unchanged; rows declared once per card, `chips.ts` model);
  0.2-3 fits at its fixed extent (no panning needed); 3D scenes get the tap-to-interact overlay; v3 instruments are off
  below 900 px.
- Keyboard: every handle has its DragKeys button; the power chips (0.2-2), the `2D/3D` toggle (0.2-4) and the presets
  (0.2-5) are buttons; Timelines are range inputs; every challenge is reachable by typing values in N-panel fields alone
  (checks read `vals`/`step` only).
- Every scene's accessible name is the server `<figcaption>` from `SceneIsland`; live values are in the N-panel rows
  (DOM), so a screen reader gets the numbers without the canvas.

### 7.13 What the engine must add for this section

| item | status | needed by |
|---|---|---|
| `TileGrid` (7.1: `shape` box/row with wrap, `plane`, `fill`, opaque clay instances, exact clipping, counts, mirror, instance-count transitions) | new, `scene/TileGrid.tsx` | all clay scenes |
| Clay card mode (`look: 'clay'`: light studio background, `ClayTile`/`ClayCube` matcap material, footprint sprites, clay roles in `palette.ts`) | new (visual-language v3 §Clay) | 0.G-1, 0.G-3, 0.2-1..4 |
| Measurement arrow with a drag handle (v3 dimension line + `Draggable`) | new, small | 0.G-1, 0.2-1, 0.2-2 |
| `FnCurve` (sampled polyline on `Stroke`) | new, small | 0.2-5 |
| Explicit 3D camera position/target per Stage (AGENTS.md open framing item) | engine fix | 0.2-2, 0.2-4 |
| Live `2D / 3D` on one ViewportCard: header toggle writes a store key; the card mounts the 2D or the 3D Stage (two Stages, one mounted) | new | 0.2-4 |
| N-panel integer chip field (`x¹ x² x³`) and preset chips under a num field | NPanel addition | 0.2-2, 0.2-5 |
| `Timeline`, `NumTable` | Task 7 contracts (planned) | 0.2-2, 0.2-3, 0.G-2 |
| Store-level linked atomic patch + timeline adapter (contract below) | **required new store API**, not supplied by current `writeMove`; no engine implementation in this content branch | 0.G coupling, 0.2 x/A/V/from, n/k fields and timelines |
| Clay axis contrast backing | new clay-card stroke/label backing (contract below) | 0.G-1, 0.2-1, 0.2-2 |


**Linked write contract (required before scene delivery).** Register pure lesson links once in the SSR-safe entry,
not during render. All existing `store.set(key, value, how)` callers (NPanel, Formula, Num and handles) must enter
one resolver: validate finite input, clamp the input key, then compute the complete patch from that input channel.
Publish the entire `vals` patch and the n/k timeline mirrors with one Zustand update; synchronous subscribers must
never observe new x with old A/V/from. Do not implement this as a subscription that writes back recursively.
Run the resolver before pruning no-op values: writing the same A/x must still update the input channel from. Current `writeMove` loops over `set` and cannot provide this guarantee: adapt it to one `setMany(patch, how)`
transaction. Handles submit only the authoritative input (`x`), letting the same resolver derive the patch; component
callbacks and duplicated coupling logic are unnecessary. Unlinked keys keep current behavior. A patch with conflicting
linked inputs is rejected; loading defaults supplies a coherent initial snapshot (x=3, A=9, V=27, from=0).

For `ease`, spring only canonical x in this linked group, deriving shown A=x² and V=x³ each frame; vals already
holds the exact target and from cuts immediately. For `cut`, publish both views together. Thus side, area and cube
remain geometrically consistent during easing. Reduced motion cuts. Scrub cancellation restores the complete
pre-press linked snapshot, including from, rather than calling a fresh A/x write that changes the input channel;
extend the shared scrub transaction hook to capture/restore that snapshot for NPanel, Formula and Num.

Timeline adapters map `setStep('power', i)` to `set('n', clamp(trunc(i)+1,1,3), 'ease')` and
`setStep('ladder', i)` to `set('k', 3-clamp(trunc(i),0,6), 'ease')`; key writes update their mirrors atomically.
Initialize mirrors to power=1 and ladder=0. Neither mirror writes back independently. Integer n/k writes are
normalized before publication, with no fractional shown exponents; their scene transition progress is separate
animation state. Formula scrubs keep their existing specs and store keys, including k. Chips/presets use that
same set path; g's row, Formula and presets share [0.2,32], preserving exact 1/2.2.

**Explicit thin-axis acceptance.** Every box must declare thin flags (2D xy: false/false/true; 3D stick:
false/true/true; xz slab: false/true/false; cube: all false). Zero on an active axis draws/counts nothing;
zero on a display-only axis is allowed. Transition thickness is capped by the exact clipped size, so a 0.1 body is never enlarged to 0.12. Active 0.1×0.1 square: one clipped piece, whole=0, partial=0.01;
0.1 cube: one clipped piece, whole=0, partial=0.001. Counts use absolute active lengths for mirrors and never
morph display thickness. Row fill counts measure fill, with whole=1 only at fill=1, otherwise whole=0.

**Clay-axis acceptance.** Keep Blender axis hues, but their bare contrast on the light studio is insufficient.
Clay cards add an opaque clayInk (#2A2E36) backing stroke (core width + 2 px each side) beneath every axis
filament, and cream clayPlate backing behind axis letters/ticks; text uses clayInk with the axis colour as a
swatch. Axis colours against the dark backing must each meet 3:1 and clayInk text against clayPlate 4.5:1;
verify both studio gradient ends and all axis states. Computed axis/backing ratios are X 3.79:1, Y 8.00:1, Z 4.23:1; clayInk/clayPlate text is 11.24:1. The gizmo keeps its existing dark chrome. This requirement
is local to the clay-card mode and does not recolour global axis tokens.
