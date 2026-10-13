# egui-nodes-view

Working notebook for this crate. Keep current.

## What it is
A **thin egui-widgets-rs surface over nodes-rs's GPU node graph**. `nodes-egui`
(from the sibling `nodes-rs` repo) ALREADY ships the complete egui widget —
`GraphViewportState::ui(ui, host, config, extras)` does camera (MMB pan / RMB zoom
/ Alt orbit), marquee + click select, node drag/resize, wiring, delete, and the
wgpu render → `egui::Image` present. This crate:
- re-exports that widget + its config/host-trait/edit enums under the `egui-*`
  naming (`pub use nodes_egui::{GraphViewportState, GraphViewportConfig,
  GraphViewportExtras, GraphViewportHost, ViewportEditKind, …}`), and
- adds a ready-made **`SubnetHost`** that implements the 4 required
  `GraphViewportHost` methods (`subnet`/`subnet_mut`/`registry`/`value_types`) over
  a plain `nodes_core::Subnet` + registries, so a host gets a GPU node panel
  without hand-writing the trait (undo/redo/scope use the trait defaults).
- re-exports `nodes_core` so a host builds a `Subnet` without a second dep.

## Why thin (design decision)
`nodes-egui` is ALREADY the egui bridge, so wrapping it further would be a
prokladka-over-prokladka. The value here is only: (a) one crate on the `egui-*`
surface, (b) the convenience `SubnetHost`. Do NOT grow this into a re-implementation.

## The only node graph
`egui-node-graph` (egui-snarl, CPU immediate-mode) is DELETED. There is one
node-graph engine in cglibs and it is nodes-rs; this crate is its egui surface.
Consequence worth knowing: a wgpu device is now required for any node graph, because
the hermetic CPU alternative no longer exists.

## Gotchas
- **Heavy git-ref tree**: pulls `nodes-egui` + `nodes-core` from
  `ssh://git@github.com/ssoj13/nodes-rs.git` (branch=main), which transitively pulls
  nodes-view + wgpu-text + box_rs + graph-layout + eframe. First build clones several
  repos. This is the one **non-wasm, non-hermetic-by-design** widget crate besides
  egui-puffin (see root CLAUDE.md wasm note).
- Versions must match the workspace: nodes-rs tracks egui 0.34 / wgpu 29 / eframe
  0.34 / glam 0.33 — same pins as here (verified).
- Needs a wgpu device: call `state.configure_wgpu_render_state(rs.clone())` once when
  the eframe `RenderState` is available, then `state.ui(...)` per frame. The panel
  shows "Waiting for wgpu…" until configured. See the demo's `page_nodes_view` +
  `make_nodes_host` for a worked minimal embed.
- nodes-rs gaps observed while building this live in `nodes-rs/TODO2.md` (a ready
  SubnetHost upstream, a `default_registry()` helper, a lighter bridge without the
  `eframe` dep, an egui-embed example).
