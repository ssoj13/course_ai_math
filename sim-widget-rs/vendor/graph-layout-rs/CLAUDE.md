# graph-layout — local notes

## What / why
Generic, **dependency-free** node-graph layout engine. Input: an abstract `LayoutGraph`
(nodes with `id: u64`, `size`, `pos` + directed `LayoutEdge`s). Output: `GraphLayout`
(`id → [x,y]`). Hosts adapt their own model to these plain types and write positions back.

Created (2026-06-04) to replace the removed `sugiyama-layout` crate (rust-sugiyama wrapper)
as the node-editor's auto-layout engine. We want ONE layout engine, written ourselves,
isolated and reusable in other apps — hence zero workspace deps (no nodes-core/glam/wgpu).

## API (src/graph_layout.rs)
- `LayoutNode { id, size, pos }`, `LayoutEdge { from, to }`, `LayoutGraph { nodes, edges }`
- `Direction { LeftRight, TopBottom }`, `LayoutParams { direction, node_gap, layer_gap }`
- `GraphLayout { positions: HashMap<u64,[f32;2]> }` + `.pos(id)`
- `trait LayoutEngine { fn layout(&self, &LayoutGraph, &LayoutParams) -> GraphLayout }`
- `IdentityLayout` — **stub**: returns each node's incoming `pos` unchanged.

## Status / TODO
- [x] Scaffold + `IdentityLayout` stub + tests.
- [x] **Real engine `SugiyamaLayout`** (src/sugiyama.rs): components → deterministic cycle
      break → longest-path ranks → barycenter crossing reduction → size/pin/stability-aware
      cross placement with overlap=0 → LR/TB transpose. Wired into `nodes_core::auto_layout_subnet`
      (editor **L**). Passes the original target-semantics tests (ranks advance, siblings
      spread, parallels share rank) + invariants (no overlap, bitwise determinism, cycles
      don't hang, pinned stays).
- [x] **Centered, drift-free placement.** Cross placement pulls each node toward the *mean*
      of ALL its neighbours' socket targets (predecessors AND successors per pass, not
      alternating) and packs layers with pool-adjacent-violators (`pack_layer_centered`)
      instead of a one-sided sweep. Fixes the screenshot bug where a sink with inputs from
      different ranks snapped flush to the near input and stranded the far one — it now sits
      *between* its inputs (balanced bends). Test `sink_centers_between_inputs`. Pinned layers
      still use the anchor-respecting `pack_layer`.
- [x] **Socket (pin) awareness.** Port-aware barycenter (bias by socket rank on the fixed
      neighbour; fraction in (0,1); reversal-safe via `rev`/`orig_from`/`orig_to`) so a parent
      feeding the upper socket sorts above one feeding the lower → no a/b crossings; placement
      aligns connected sockets → straight wires. Adapter feeds real socket y-offsets + exact
      (output,input) indices from SSOT `compute_static_layout` (matches how wires draw, incl.
      view modes); snaps only the column axis to grid so wires stay straight. Test
      `parents_ordered_by_target_socket` encodes the screenshot crossing bug.
- [ ] Local "tidy selection": `pinned` nodes are honored (kept at pos, movable placed around
      them) but ranking/order ignore pinned obstacle positions — full obstacle-aware local
      layout is a follow-up. Stability (`LayoutParams.stability`) is supported but unused by
      the L-key path (set to 0 for full reflow).
- [ ] v2 algorithm upgrades behind `LayoutEngine`: dummy nodes + long-edge waypoints,
      Brandes–Köpf Y-assignment, network-simplex ranking, component bin-packing.
- [ ] (separate decision) route mermaid diagram layout (selkie dagre in
      `src/layout/selkie_dagre.rs`, NOT removed) through this engine.

## Consumers
- `nodes-core::layout::auto_layout_subnet` builds a `LayoutGraph` from a `Graph` and applies
  the result. (Editor shortcut **L**.)

## Traps
- `id` is opaque `u64`; the host owns the NodeId↔u64 mapping.
- `pos` is top-left; engines may use it as a seed. The stub depends on it being the current
  position so "auto-layout" doesn't move anything yet.
- Keep this crate dependency-free — that's the whole point (reuse).
