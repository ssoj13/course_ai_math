# Signature illustrations — one unifying idea: everything is light, shadows and areas

User requirement (task.md): illustrations nobody has done before, beautiful and so clear that anyone looks and says
"I get it". The course's visual language (visual-language.md) is "the viewport and the light"; these illustrations make
that a single story: **a dot product is a shadow times a length — an area; a sum along an axis is light accumulating;
a derivative is what's left when you zoom until the curve is straight.**

## S1 — Law of cosines: "the missing rectangles" (lesson 0.5)
- Triangle with draggable corner angle θ; squares built outward on all three sides (a², b², c²), real areas, light fill.
- Drop the shadow of b onto a (length b·cosθ). Two **rectangles a × (b·cosθ)** appear — and, animated, they are cut out
  of the a² and b² squares (Euclid, Elements II.12–13, made physical): what remains rearranges exactly into c².
  So c² = a² + b² − 2·(rectangle). At 90° the shadow is 0 → rectangles vanish → Pythagoras. Past 90° the shadow points
  backwards → the rectangles flip to hatched (negative) and get *added*.
- **The reveal (bridge to 1.4):** that rectangle's area is |a|·(shadow of b on a) = **a·b**. The dot product is literally the
  area of the missing rectangle. The law of cosines is |a−b|² = |a|² + |b|² − 2(a·b), seen as paper cut-outs.
- Keeps the IK arm (CG anchor) and the correction-bar ledger as follow-up scenes.

## S2 — The dot product as lighting and as an area (lesson 1.4)
- Same rectangle reused: drag a and b, the rectangle |a| × shadow(b) is the dot product; it flips to hatched when θ > 90°,
  collapses at 90°. The N·L sphere shows the same number as brightness. One number, three faces: sum of products, area,
  brightness — all linked by hover.

## S3 — Matrix/tensor multiplication: "the light box" (lessons 2.4, 3.6)
- A (m×k) is a wall on the left, B (k×n) lies on the floor. Each cell of the m×k×n box between them lights up with
  brightness a_ik·b_kj (sign by hue + hatch). **C = A·B is the shadow of that box on the back wall**: light accumulates
  along k (sum = integral of light), so each C cell's brightness is a dot product of a row of A and a column of B.
  Scrub k: slice by slice, watch the wall fill up. Hover a C cell → its whole beam of k products and the row/column light.
- Batched matmul = a stack of light boxes; transposes = rotating the wall/floor before lighting.

## S4 — Einsum as plugs and cables (lesson 3.6)
- Each tensor is a block with labeled sockets (one per axis, size printed). Contracting = plugging two sockets together with a
  cable (that axis disappears, its values are multiplied and summed); free sockets become the result's shape. Drag blocks,
  connect cables, the output shape and the einsum string (`bij,bjk->bik`) update live; the light box (S3) shows the numbers.
  This is Penrose/tensor-network notation made tactile.

## S5 — Attention is lighting (lesson 10.1)
- Tokens as small surfaces (keys) in a space; a query is a light. Brightness of each key = q·k (S2 lighting);
  √d dims the exposure (10.2); softmax normalizes total light to 1 (an exposure meter); the output is the light-weighted
  average of values (7.4). A CG person's own mental model, made exact.

## S6 — The derivative for everyone (lesson 4.2): "zoom until it's straight"
- One gesture teaches it: scroll-zoom into any point of a curve in the 3D viewport; a microscope ring shows the curve
  straightening; its slope is printed as "speed" on a little car riding the curve (rise/run triangle as the car's
  speedometer). Kinks never straighten (|x|) — no derivative. Graph Editor tangent handles shown as the same line.
