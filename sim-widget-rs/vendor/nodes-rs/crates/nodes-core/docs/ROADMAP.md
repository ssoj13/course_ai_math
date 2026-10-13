# nodes-core — Roadmap (migrated from legacy nodes-rs)

**Status:** Phases 0–4 and 6 are implemented and shipping in `nodes-core`/`nodes-view`/
`nodes-interact`/`nodes-egui`/`nodes-editor` (corrected 2026-07-05 — this header previously
read "planning, no implementation lock yet", which was stale; see each crate's own
`CLAUDE.md` for current state). The phase breakdown below remains a useful historical
design record and backlog for what's NOT yet done (Phase 5 Data Bus expansion, Future
advanced view modes).  
**SSOT backlog:** `../../../md/TODO.md` (whole monorepo)  
**North star (design, archived):** `../../../md.old/task.md` (RustBox, Node, Subnet, Data Bus)  
**Editor UX checklist:** see Phase 4 in this file + items in `md/TODO.md` P3

---

## Strategic decisions (agreed)

| Topic | Decision |
|-------|----------|
| Snarl | **Removed entirely.** Former `src/snarl/` is not a dependency or compatibility layer — delete and replace. |
| Graph storage | **TARGET model** in headless `nodes-core` (RustBox, Node, Subnet, ConnectionIndex, Solver, Executor). |
| **Public API** | **`wgpu` first.** Render + input + camera are exposed without egui types. egui/Qt/eframe are optional hosts via thin adapters. |
| Viewport | **wgpu** for nodes, wires, grid, pins, 3D/spatial modes — never egui-shapes for graph canvas. |
| Chrome (v1) | Tab menu / attr panels may use **egui** (or anything else) **beside** the viewport; chrome does not define the API. |
| Current prototype | ADR-001 (`DynValue`, `Registry`, `eval`, old `graph.rs`) is **throwaway** toward core — salvage ideas, not structure. |
| mermaid-rs | Uses wgpu already; graph **interactive** mode pauses or stubs until `nodes-view` + adapter exist. |

---

## Crate layout (target)

```text
nodes-core/       headless: RustBox, Node, Subnet, connections, cook_*, serde
nodes-view/       PUBLIC API = wgpu (see below). No egui in public types.
nodes-egui/       optional: PaintCallback → calls nodes-view (not part of core contract)
nodes-editor/     reference app: winit+wgpu and/or eframe; egui only for chrome if needed
```

Optional later: `nodes-types/` if `Value` / plug-in types split from core.

**Full module tree + usd-view file copy matrix:** [`docs/STRUCTURE.md`](docs/STRUCTURE.md)

---

## Reference: `usd-view` layering (we copy this, not Snarl)

Template path: `vfx-rs/crates/geo/vfx-usd/crates/usd-view`

```text
usd-imaging::gl::Engine     →  nodes-view::GraphRenderer   (wgpu public API)
ViewportGpuState            →  nodes-view::present::GpuDisplay
ui_viewport + allocate_rect →  nodes-editor::panels::graph_viewport
ViewerApp + launcher        →  nodes-editor::GraphEditorApp
register_native_texture     →  nodes-egui::GraphGpuBridge
```

**Workflow:** copy `usd-view/src/**` into `nodes-editor/src/**`, rename domains (Stage→Subnet, prim→node), swap `Engine` calls for `GraphRenderer`. Implement `nodes-core` from TARGET.md (no USD). Move `gpu_display.rs` into `nodes-view/present/`. Delete legacy `snarl/` — not in the template.

`configure_wgpu_render_state` in the app (same as usd-view):

1. `renderer.attach_wgpu(adapter, device, queue, format)`
2. `gpu_bridge.set_render_state(egui_wgpu::RenderState)`
3. Each frame: `renderer.render()` → `present` → `egui::Image(texture_id)`

---

### `nodes-view` public API (wgpu — canonical)

Hosts (mermaid-rs, VFX tools, Blender add-on shell, raw winit) all integrate the same way:

```rust
// Construct once per GPU device / surface format
let mut viewport = GraphViewport::new(GraphViewportDesc {
    device, queue, format, msaa,
});

// Each frame (or offscreen pass)
viewport.resize(width, height);
viewport.update_layout(&subnet, &layout_state);
viewport.render(
    &RenderPassParams { color_view, depth_view, scissor_rect },
    &subnet,
    &camera,
);

// Input: pixel coords in viewport rect, no egui
let hits = viewport.hit_test(pointer_px, &subnet, &camera);
viewport.pointer_down(PointerButton::Middle, mods, pointer_px);
viewport.pointer_move(pointer_px, mods);
viewport.scroll(delta, pointer_px);
```

**Rules:**

- Dependencies of `nodes-view`: `wgpu`, `glam`/`nalgebra`, `nodes-core` — **not** `egui` / `eframe`.
- Textures for live 2D/3D node previews: `wgpu::TextureView` in API, not `egui::TextureId`.
- Transparency: host chooses `LoadOp` / composite; viewport can clear alpha 0 in its pass.
- **Embedding in egui** = host writes ~20 lines: `allocate_rect` → map rect to `scissor_rect` → call `viewport.render` inside `PaintCallback`. That lives in `nodes-egui`, not in the contract.

Same pattern as Maya VP2 / Blender viewport region: **GPU module owns the rect**; UI toolkit only schedules it.

---

## Phase 0 — Cleanup & docs

- Move full design from `md.old/task.md` → `docs/TARGET.md` (here or `nodes-core/docs/`) when Phase 1 starts.
- Track open work in `md/TODO.md` (not a second `task.md` in this crate).
- Plan deletion: `src/snarl/**`, `ui/viewer.rs`, `Snarl<GraphNode>` in `graph.rs`, mermaid `Snarl` integration.
- Workspace stubs: `nodes-core`, `nodes-view`; align demo on **wgpu** (match parent `mermaid-rs`).

**Acceptance:** written TARGET + ROADMAP; list of files to delete; `interactive` policy for mermaid-rs documented.

---

## Phase 1 — `nodes-core` (simple v1, correct foundation)

- `RustBox` + `Value` (or hybrid: RustBox meta + typed wire payload — decide before coding).
- `Node`, `Subnet`, `ConnectionIndex`.
- `GraphSolver` (topological order) + `Executor` (`cook`, `cook_dirty`, `cook_selected`).
- JSON save/load; paste subgraph API.

**Acceptance:** integration test — wire three nodes, `cook_dirty`, assert attr values; **no egui, no wgpu**.

---

## Phase 2 — `nodes-view` MVP (wgpu canvas)

- Shared `Device`/`Queue` with eframe **or** standalone winit example.
- Camera: pan (MMB), zoom (dolly) — **✅ done**: `Camera` (genuine 3D **perspective**, `eye/target/up/fov_y`, reversed-Z infinite-far) in `nodes-view::camera::flat`. `ortho_width` was removed 2026-07-06 (TODO3); zoom is a dolly and on-screen scale comes from `px_per_world_at`. The legacy 2D `FlatCamera`/`render_layout_flat` compat path was also deleted (everything is full 3D now).
- Draw: grid ✅, cubic-bezier wires ✅ (line pipeline), rounded filled node rects ⬜ (quad pipeline pending), glyphon text ⬜.
- Layout cache (CPU): node bounds, pin positions for hit-test — **✅ done** (`LayoutCache`, `PinPos`, `PinSide`).
- Positions from core / view sidecar (`view.pos` in RustBox or `SubnetMetadata`).

**Acceptance:** standalone **winit + wgpu** example (no egui) renders 10+ nodes and wires; public API documented without any egui types.

---

## Phase 3 — Input & wiring

- Pick: pin > wire > node; drag node; drag connection out→in; marquee selection.
- Events → `core.connect` / `disconnect` / selection set.
- Dirty propagation → `cook_dirty`.

**Acceptance:** build a small graph entirely with mouse; eval updates downstream attrs.

---

## Phase 4 — `nodes-editor` (Nuke-like v1)

- Tab menu + smart search over registered node types.
- Attr panel editing RustBox paths.
- One file per node type: register schema + optional custom `NodeUi` hook.
- Bypass, minimap (can be egui or wgpu later), shortcuts.
- Demo app: **wgpu** backend only.

**Acceptance:** matches Phase 4 / `md/TODO.md` P3 (Tab, custom nodes, wiring, eval, easy extension).

---

## Phase 5 — Data Bus & expressive nodes

- [x] Named slots / GDP-style bus per subnet scope. **DONE** — `Subnet::bus` (`DataBus`),
  `BusRead`/`BusWrite` builtins, `@slot` addressing, `SubnetFile.bus_slots` serde. See `DATA_BUS.md`.
- [x] Arbitrary attrs on node body (data-driven UI generation from schema). **DONE** — every
  computation input is a declared, connectable, inspector-editable port (2026-07-05 model split);
  serialization is fully generic. No bespoke "config field" concept remains.
- [x] **Epochs** — cook generation counter (`Graph::cook_gen`, bumped per `cook`/`cook_dirty`;
  each re-cooked node stamped via `Node::cooked_gen`). **DONE 2026-07-07** — freshness signal for
  live previews (M8/Future) without value-diffing.
- [ ] Parallel executor — **deferred by design.** Cook runs cheap per-node closures and the frame
  bottleneck is rendering, not cook; parallelizing the best-tested subsystem now is premature
  optimization (risk of concurrency bugs for no measured win). Revisit if profiling shows heavy
  per-node cook on large graphs. The epoch counter above is the prerequisite already in place.

---

## Phase 6 — mermaid-rs integration

- [x] Remove `Snarl<VisualNode>` / legacy `nodes-rs` from diagram graph path.
- [x] **A)** graph mode uses shared wgpu viewport; vector diagrams unchanged.
- [x] **B)** read-only: `NodeGraph` → `adapt_node_graph` → `DiagramGraphHost` → `nodes-egui::GraphViewportState` + mermaid overlays (clusters, styled edges, labels).
- [x] **C)** edit + cook + round-trip: `DiagramSession`, `sync_subnet_to_graph`, `export_flowchart`, `GraphViewportConfig::DIAGRAM_EDIT`, `DiagramViewer::to_mermaid()`.
- [x] **D)** dual undo/redo: independent network + UI stacks, `UNDO_REDO.md`, examples `roundtrip` / `dual_history`.

---

## Future — Advanced view modes (post v1)

v1 is intentionally **flat 2D editor** (Nuke-style plane, Sugiyama-ish placement, manual drag).  
Architecture must **not** paint us into a corner: core stays view-agnostic; view modes are pluggable.

> **Increment shipped 2026-07-07 (M8):** `NodeViewKind::{Flat2D,Live2D,Live3D}` (per-node,
> `view.view_kind`) + a `Live2D` body-preview swatch in `nodes-view` driven by the node's cooked
> primary output (colour/scalar), toggled with **P**. `Live3D` (mesh/scope thumbnails) is the
> remaining unshipped part below.

### Live 2D / 3D node views

- Nodes are not only boxes on a plane — **live previews** embedded in the graph:
  - **2D:** textures, curves, scopes, image ops (GPU compositing preview).
  - **3D:** mesh/viewport thumbnails, material graphs, scene fragments.
- Previews are driven by **cooked data** from core (attrs / bus), not duplicate state in the view.
- View mode selects render path per node type (`NodeViewKind::Flat2D | Live2D | Live3D | …`).

> **Increment shipped 2026-07-07 (M9):** continuous force layout — `Settings -> Layout -> Live`
> runs a few `graph-layout-gpu` force iterations per frame seeded from current positions, so the
> graph settles organically while open; selected nodes are pinned (re-fixed each step) as
> constraints. Background-thread solving and settle/stop heuristics are the remaining polish.

### Energy layout & “tree” solving (continuous)

- Beyond one-shot Sugiyama: a **continuous layout solver** runs while the graph is open:
  - Nodes modeled as bodies with repulsion / spring edges / hierarchy forces.
  - **Minimize energy** so the graph settles into a readable **tree / DAG-like** structure without manual nudging.
  - Solver ticks on background thread or GPU compute; positions stream into `nodes-view` each frame.
- User can still **override** pin positions (manual fix); solver respects pinned nodes as constraints.
- Subnet boundaries: layout can be hierarchical (solve inside subnet, then place subnet node in parent).

### Space-sim navigation (“fly around the graph”)

- **Advanced navigation mode** (optional, like flying in a space sim):
  - Camera moves in **3D** around a graph laid out in 3D space (even if data is 2D, layout can extrude depth by rank/community).
  - MMB: pan/strafe; scroll: thrust forward/back; optional roll; inertia / damping for “cosmic” feel.
  - Nodes at depth appear smaller / fogged; wires as splines in 3D.
- Distinct from flat Nuke plane mode — user toggles **Editor mode**: `Flat` vs `Spatial3D`.
- Hit-test and wiring adapt to projected screen picks (ray → node shell in 3D).

### Implications for v1 design (do now, pay later)

| v1 choice | Why it matters for advanced modes |
|-----------|-----------------------------------|
| `nodes-core` separate from view | 3D/spatial view is another `ViewBackend` trait impl. |
| Camera as first-class struct | Extend to 6-DOF flight + inertia, not only 2D TSTransform. |
| Layout cache separate from core truth | Continuous solver writes layout channel without mutating cook graph. |
| Node bounds from schema | Live 2D/3D nodes declare intrinsic size / aspect for layout forces. |
| Wire geometry as paths, not egui shapes | Extrude to 3D curves later. |

---

## Open questions (before Phase 1 code)

1. **mermaid-rs:** disable graph interactive until B/C, or read-only `GraphViewport::render` first?  
2. **Connections v1:** UUID attrs in core from day one, or pin-index in view + mapping?  
3. **Monorepo split:** `nodes-view` separate crate immediately vs module until API stable?  
4. **Chrome:** reference `nodes-editor` uses egui for Tab/attrs only — confirm no egui in `nodes-view` dependency tree.

---

## Salvage vs delete (reference)

| Keep (concept / file ideas) | Delete |
|-----------------------------|--------|
| `dyn_value`, `registry`, `node!` → evolve into core | `src/snarl/**` |
| `ui/tab_menu`, shortcuts, colors | `ui/viewer.rs` (SnarlViewer) |
| `layout` → core layout + view cache | `graph.rs` as `Snarl<GraphNode>` |
| `eval` → `Executor` | `eframe` **glow** backend in nodes-rs |
| mermaid `viewer/controls.rs` → `nodes-view::Camera` | mermaid `GraphViewer` Snarl impl (until adapter) |

---

*Last updated: 2026-07-05 — header status corrected (was stale "planning"); Phase 6D done;
backlog SSOT = `md/TODO.md`. See also `NODES.md` §1 — the nested-subnet model described
there as `child_subnet: Option<Subnet>` was superseded by a flat `Subnet` with per-node
`parent`/`children`; `NODES.md` has been corrected to match.*
