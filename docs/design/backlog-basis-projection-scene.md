# Backlog scene: "Matrix × vector = shadows on new rulers" (3D)

User requirement (task.md): show in 3D how a vector is projected onto another orthonormal basis, how the
matrix "multiplication" happens visually, and what goes wrong when the basis is not orthonormal (a checkbox/flag).
Lessons: 1.6 (basis, orthonormal change of basis), 2.2 (matrix = rows/columns), 2.5 (transpose = inverse for
orthonormal), 2.7 (inverse), A1 (Gram–Schmidt). Build after Part 1 engine; reuse Arrow/drag/HUD/Formula/NumTable.

## Scene
- World axes (Maya colors) + vector `v` (draggable). Three new basis vectors u1, u2, u3 ("rulers", vecA/vecB/vecC tints),
  drawn as long thin rulers with tick marks; orbit camera; gnomon.
- **Row view (the multiplication itself):** step through rows of R = [u1; u2; u3]. For row i: drop a perpendicular from
  the tip of v onto ruler u_i, light the shadow segment on u_i, show `u_i · v` = Σ_k u_ik v_k with the three products as
  NumTable cells and a 3-segment bar (sign by hatch). The i-th result component lights in the output vector `R v`.
- **Transfer animation:** the three rulers rotate back onto the world axes carrying their shadows; the shadows become
  the coordinates of `R v` on x, y, z — "multiplying by R = reading v with the new rulers".
- **Column view:** `R v` rebuilt head-to-tail as v1·col1 + v2·col2 + v3·col3 (same numbers, other reading).
- **Orthonormal flag:** checkbox "Keep orthonormal" (on = drag of a ruler re-orthonormalizes the set with Gram–Schmidt,
  order u1 → u2 → u3; off = rulers drag freely). Live status: per-ruler |u_i| = 1 ✓/✗, per-pair u_i·u_j = 0 ✓/✗, and the
  Gram matrix RᵀR as a 3×3 heatmap that is the identity exactly when the flag is green; det sign badge (right/left-handed).
- **What breaks when not orthonormal:** reconstruction Σ (u_i·v) u_i drawn as a ghost vector next to v with a dashed
  error gap (it no longer equals v); the true coordinates need oblique drop lines parallel to the other rulers (skewed
  lattice / parallelepiped cell) — shown side by side with the perpendicular shadows; optional "dual rulers" (rows of
  R⁻¹ᵀ) explain why oblique coordinates are dot products with a different set. Lands the message: for orthonormal
  bases the inverse is the transpose — shadows are coordinates; otherwise you must solve (2.7).

## Engine needs (later tasks)
Oblique projection segments (parallel to a given direction), ruler primitive with ticks, Gram–Schmidt helper in geom.ts
(tested), 3×3 heatmap (DOM, linked term ids), "keep orthonormal" drag constraint via Draggable.move (6e mapping).
