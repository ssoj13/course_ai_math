# Backlog scenes: derivatives and partial derivatives from 1D to 6D (3D viewport)

User requirement (task.md): the derivative of a graph in 3D — orbit, zoom, 3D guides that explain everything, hover
effects, beautiful; plus partial derivatives for 1D, 2D, 3D inputs and 4D/5D/6D where the extra axes are time-like
(1, 2, 3 time parameters). Lessons 4.1–4.7, 5.1–5.5; reused by 9.x (gradients of a loss).

## The one idea that makes N-D visible
You cannot see 6 axes at once. A partial derivative is exactly the trick: **freeze every input except one, and the
function becomes an ordinary 1D curve whose slope you can see.** The gradient is just the list of those slopes.
Every scene below offers the same "freeze" control (lock icons on sliders) so this idea is felt, not stated.

## Shared look & interaction (visual-language.md)
Viewport frame + gnomon + HUD chips; the function is a glowing tube/surface; guides are thin light-beam strokes:
rise/run triangle, tangent, normal, slice planes, contour lines on the floor. Every guide has a term id: hover the guide →
it brightens, its formula term lights, and a short caption chip explains it ("Δx: how far we stepped"). Orbit + zoom with a
"microscope" lens that magnifies around the probe point.

## D1 — 1D: the derivative as the slope you see when you zoom in (4.1–4.3)
- Curve y = f(x) (presets: x², sin x, eˣ, |x|, a Maya-like animation curve) as a tube in a 3D viewport (z for depth only).
- Probe P draggable along the curve; second point Q = P + h; secant line through P,Q; scrub `h` → secant rotates into the
  tangent (the limit, felt). Rise/run triangle Δx, Δy with live values; HUD `Δy/Δx → f'(x)`.
- **Zoom to straight:** microscope lens / camera dolly onto P — the curve straightens; at |x| the kink never straightens
  (no derivative) — the scene says so.
- Linked derivative plot f'(x) on a back plane: the probe's slope is the height there (hover links both).
- CG anchor: Graph Editor tangent handles = the derivative; toggle shows Maya-style tangent handles on the curve.

## D2 — 2D input: surface z = f(x, y) (5.1–5.4)
- Surface with contour lines projected on the floor; probe P draggable on the floor (x, y) — the surface point follows.
- Slice planes x = const (axisX tint) and y = const (axisY tint) cut the surface into two curves; their tangents at P are
  ∂f/∂x and ∂f/∂y (arrows in the planes, values in HUD). Lock icons: freezing y shows the x-slice curve alone, etc.
- The two tangents span the **tangent plane** (translucent) — "zoomed in, a surface is a plane" (2D version of D1).
- **Gradient** arrow on the floor, ⟂ to the contour through P, length = steepness.
- **Directional derivative:** rotate a direction u on the floor; the slice plane along u gives slope ∇f·u — the dot product
  from lesson 1.4 lights up (cross-lesson link). Best direction = along ∇f.
- Gradient-descent ball as a follow-up toggle (5.5).

## D3 — 3D input: field f(x, y, z) (5.2–5.4)
- Scalar field in a volume: isosurfaces via three/addons `MarchingCubes` (level slider), three orthogonal slice planes with
  heat-map textures, instanced gradient arrows on a sparse grid (⟂ to isosurfaces).
- Probe point draggable in 3D (plane-constrained drag per 6b machinery); HUD shows ∂f/∂x, ∂f/∂y, ∂f/∂z and the gradient
  arrow; locking two coordinates shows the 1D line through the volume and its slope.

## D4–D6 — extra "time" inputs (x, y, z, t₁[, t₂[, t₃]])
- Same volume; t₁ is a scrubbable/animated time slider; ∂f/∂t₁ = how fast the value at the probe changes — shown as a
  sparkline of f(probe, t) with its own tangent (D1 again).
- 5D/6D: t₂, t₃ as extra sliders (e.g. a moving source, a pulsing frequency). The probe's **gradient becomes a bar chart of
  6 partials** (∂x ∂y ∂z ∂t₁ ∂t₂ ∂t₃), each bar linked to its slider and to its 1D slice sparkline.
- **Slice explorer:** pick any two of the six inputs → a 2D surface (D2 view) with the other four frozen at their slider values;
  small multiples show several frozen values side by side.
- AI bridge: a network's loss has millions of inputs; the gradient is the same bar chart, just longer (9.x).

## Engine needs (later tasks)
Parametric tube/surface meshes with per-vertex update, contour extraction on a grid (marching squares, pure + tested),
MarchingCubes from three/addons, slice-plane heat textures (DataTexture), instanced arrow field, sparkline (DOM SVG),
lock-able slider group, microscope lens (second View with tighter camera on the probe — drei View makes this cheap).
