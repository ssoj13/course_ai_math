# Backlog scene: "The tensor road" — rank 0 → N on one slider (3D)

User requirement (task.md): a slider over the number of dimensions, shown in 3D, convincingly. Lessons 3.1 (rank ladder),
3.2 (real data as tensors), 3.3 (memory, strides, reshape/view), 3.5 (reductions). Build after Part 1 engine.

## Core rule the scene must make obvious
**Each new axis = take the whole previous object and repeat it along a new direction.** Rank r+1 is r-rank copies laid
side by side; beyond 3 axes the copies are laid out as a row/grid/block *of blocks* (nesting), which is exactly how
`shape` reads left to right.

## Scene
- Slider `rank` 0…6 (snap to integers; keyboard), sizes per axis editable (default 2,3,4,2,2,3). Changing rank animates
  extrusion: the current object duplicates and slides out along the new direction (damped, ~400 ms; reduced motion = cut).
- Layout per rank: 0 one cell · 1 a row (x) · 2 a sheet (x,y) · 3 a block (x,y,z) · 4 a row of blocks (gap = level)
  · 5 a sheet of blocks · 6 a block of blocks. Gaps grow with nesting level so the hierarchy reads at a glance; each level
  has its own subtle frame and axis label `axis k (size n_k)`.
- HUD: `shape (2, 3, 4, 2)` with each number colored like its axis, `rank 4`, `elements 48 = 2·3·4·2`.
- **Real data at each rank** (toggle "what is this?"): 0 a loss value · 1 an embedding · 2 a grayscale image (cells take
  the pixel values) · 3 an RGB image H×W×C (the 3 channels peel apart as layers) · 4 a batch of images B×H×W×C ·
  5 a batch of video clips B×T×H×W×C. Uses a tiny real image (stored asset, provenance noted).
- **Index ↔ memory (3.3):** a flat memory strip under the scene. Hover/click a cell → its index tuple `(i, j, k, l)` and
  flat offset `Σ i_k · stride_k` light up, with the strides shown and the strip cell highlighted. `transpose` swaps two
  strides (cells don't move in memory, the walk order does — animate the walk); `reshape` re-reads the same strip with a
  new shape (works only when sizes multiply to the same count — show the error otherwise).
- **Reduction (3.5):** pick an axis and "sum over it": that axis collapses (copies slide together and merge, values add),
  rank drops by one, shape loses that number.

## Engine needs (later tasks)
Instanced cell grid (InstancedMesh, thousands of cells at 60 fps), nested layout function `place(index, shape, levels)`
(pure, tested), memory strip (DOM or 2D View), stride math (core Tensor from the plan's TS core), damped extrusion.
