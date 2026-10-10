# Backlog scenes: how the error is computed and how backprop works — visual, step by step

User requirement (task.md). Lessons 5.6 (computation graphs, reverse mode), 8.2/8.5 (why squares, MSE/cross-entropy),
9.1–9.4 (regression → logistic → softmax → MLP with backprop by hand). All numbers are real: computed by the course's TS
autodiff core, and the drawn graph IS the autodiff tape (no fake data).

## B1 — The error, seen (8.2, 8.5, 9.1)
- 2D data points + a model line (draggable slope/intercept). Each residual is a vertical light-beam; its **square is drawn as
  an actual square tile** on the residual (area = error²) — "why squares" made literal. HUD: sum of tiles = MSE; dragging the
  line grows/shrinks tiles; the MSE-vs-slope parabola beside it shows where the minimum is (the derivative = 0 there, D1 link).
- Toggle |error| (L1): tiles become bars; the curve gets a kink (no derivative at the median) — contrast made visible.
- Classification view: predicted probability p for the true class as a bar; cross-entropy = −log p drawn as a curve with the
  probe on it — confident wrong answers shoot up.

## B2 — Computation graph, forward then backward (5.6)
- React Flow graph of a tiny expression first (L = (w·x + b − y)²), then a 2-2-1 MLP. Nodes show op + current value; edges
  carry glowing particles.
- **Forward (timeline steps):** values flow left→right, one node per step; each node lights with its formula term.
- **Loss node** shows the error (B1 tile miniature).
- **Backward:** a second color flows right→left. At each node the step shows the chain rule literally:
  `∂L/∂input = ∂L/∂output × local slope`, the local slope displayed as a tiny tangent glyph (the D1 slope) on the node, the
  incoming gradient, the product, and the result written onto the edge. Fan-in nodes show gradients **adding up** (sum rule)
  with stacked bars. Hover any edge → its gradient, where it came from, and the term in the chain-rule product.
- Machine layer: the same steps as the autodiff tape (list of recorded ops), forward pushes, backward pops.

## B3 — Learning step by step (5.5, 9.1–9.4)
- Buttons: Forward · Loss · Backward · Update · (Auto). Update shows `w ← w − η·∂L/∂w` per weight as arrows on a weight panel;
  learning-rate slider; too-big η makes the ball on the loss surface overshoot (D2 surface + descent path).
- Panels stay linked: data view (B1, boundary/line moves), graph (B2), loss curve over steps (sparkline), loss surface (two
  chosen weights, others frozen — the "freeze" idea from the derivative scenes).
- MLP view: weights as edge thickness/color, activations as node fill; per-step deltas animate (damped; reduced motion = cuts).

## Engine needs (later tasks)
TS core Tensor + reverse-mode autodiff recording a tape with node ids (planned core), React Flow graph view bound to the tape,
particle-along-edge animation (SVG/CSS, no three needed), loss surface from D2, sparkline, step controller reusing Timeline.
