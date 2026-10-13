# nodes-egui

egui bridge for `nodes-view`. `GraphGpuBridge` renders the graph to an offscreen wgpu
texture and registers it as a **native egui texture**, so the graph is just an
`egui::Image` in your panel.

Depends on `egui` + `egui-wgpu` only (not `eframe`) — an embedder doesn't drag
winit/glutin into its tree just to show a graph. (The example runs a standalone eframe
app via a dev-dependency.) This is the `vfx-rs` integration path.

**Camera:** the viewport drives the shared `nodes_view::CamControl` (ONE camera brain, same as the
winit editor) — orbit ensures the Houdini backend + FOV-scales the step, so an orbit right after an
RMB-fly tumbles around the graph instead of free-looking.

**Two embed levels:** `GraphViewportState` (this crate) is the **viewport-only** widget. For the
WHOLE editor (all HUD chrome + every control) as an egui widget, use `nodes_editor::EditorWidget`
(also egui + egui-wgpu only; RFC F4) — the heavier counterpart to this viewport.

**Custom node types:** `SubnetHost::with_libraries(&[&MyNodes])` is a ready `GraphViewportHost` with
the builtins **plus** your own `nodes_core::NodeLibrary`s (`SubnetHost::with_builtins()` is the
zero-library form). See [`../../docs/custom-nodes.md`](../../docs/custom-nodes.md).

Consumers of this viewport: `vfx-rs`, `mermaid-rs`, `review-rs`, `playa`, `egui-widgets-rs`.

Match the workspace egui/wgpu versions (egui 0.34, wgpu 29). See
[`../../docs/mdbook`](../../docs/mdbook) → Developer Guide › Using the crates.
