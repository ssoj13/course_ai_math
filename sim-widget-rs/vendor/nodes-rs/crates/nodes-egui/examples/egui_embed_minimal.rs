//! Minimal embed of the shared graph viewport in a bare `eframe` app.
//!
//! This is the ~50-line answer to "how do I drop a node graph panel into my own egui/wgpu
//! app?". It shows the whole contract:
//!
//! 1. Own a [`GraphViewportState`] and a [`GraphViewportHost`]. Here the host is the
//!    ready-made [`SubnetHost`] (empty subnet + all builtins registered) — no trait to
//!    hand-roll.
//! 2. Once, when the eframe wgpu backend is available, call
//!    [`GraphViewportState::configure_wgpu_render_state`] with the SAME device/queue egui uses.
//! 3. Each frame, call [`GraphViewportState::ui`] inside any panel with a config preset
//!    ([`GraphViewportConfig::EDITOR`]) and default [`GraphViewportExtras`].
//!
//! Run with: `cargo run -p nodes-egui --example egui_embed_minimal`.

use eframe::egui;
use nodes_egui::{GraphViewportConfig, GraphViewportExtras, GraphViewportState, SubnetHost};

struct EmbedApp {
    /// Per-window viewport: renderer + camera + interaction state.
    viewport: GraphViewportState,
    /// Graph data + registries. Any `GraphViewportHost` works; `SubnetHost` is the batteries-
    /// included one for "just a Subnet".
    host: SubnetHost,
    /// The wgpu path is only available when eframe was built/launched with the wgpu backend.
    configured: bool,
}

impl EmbedApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut viewport = GraphViewportState::default();
        // Reuse egui's own wgpu device/queue for the offscreen graph render target.
        let configured = if let Some(render_state) = cc.wgpu_render_state.clone() {
            viewport.configure_wgpu_render_state(render_state);
            true
        } else {
            false
        };
        Self {
            viewport,
            host: SubnetHost::with_builtins(),
            configured,
        }
    }
}

impl eframe::App for EmbedApp {
    // This eframe revision hands the app a ready `&mut Ui` (it owns the central panel).
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.configured {
            ui.label("This example needs the eframe wgpu backend (Renderer::Wgpu).");
            return;
        }
        // One call per frame: input, camera, GPU render, and (per config) inline widgets.
        self.viewport.ui(
            ui,
            &mut self.host,
            &GraphViewportConfig::EDITOR,
            GraphViewportExtras::default(),
        );
        // The viewport animates (camera inertia, fits), so keep the frame loop warm.
        ui.ctx().request_repaint();
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "nodes-egui — minimal embed",
        options,
        Box::new(|cc| Ok(Box::new(EmbedApp::new(cc)))),
    )
}
