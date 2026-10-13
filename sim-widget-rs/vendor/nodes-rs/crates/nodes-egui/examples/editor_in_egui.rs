//! F2b PoC: the REAL editor chrome (`nodes-chrome::HudStack`) composited over the graph in egui.
//!
//! Proves the shared-chrome path end-to-end in the egui embed — no toy widgets, no throwaway glue:
//! - the graph renders offscreen via `GraphViewportState` (unchanged);
//! - `nodes_chrome::HudStack` (the SAME panels the standalone editor uses — the top menu bar +
//!   subnet breadcrumb, the bottom status bar with live text, and the bottom-left minimap) is
//!   composited into the graph's OWN color texture — ONE texture, not a second `egui::Image` —
//!   through `GraphRenderer::overlay_targets` (the widget-agnostic compositing seam) plus a
//!   `wgpu-widgets` `WidgetRenderer` built with egui's `target_format`;
//! - input flows egui pointer -> `RawInput` -> `HudStack::handle_input` -> the SAME `InputRouter`
//!   the standalone editor uses (neutral door, F1). Drag a status-bar section splitter to resize it;
//!   click / drag the minimap to pan the camera; click the menu bar to open its drop-downs
//!   (actions are logged).
//!
//! The status bar and its input path are byte-identical to the standalone winit editor's — the
//! ONLY embed-specific glue is decoding egui pointer into `RawInput` and issuing the overlay pass.
//! See `docs/rfc-unified-egui-widget.md` phase F2b.
//!
//! Run: `cargo run -p nodes-egui --example editor_in_egui`

use eframe::egui;
use egui_wgpu::RenderState;
use glam::Vec2;
use winit::event::MouseButton;

use nodes_chrome::{HudStack, overlay_surface::screen_hud_view_proj, status_bar_wgpu::section};
use nodes_egui::{
    GraphViewportConfig, GraphViewportExtras, GraphViewportHost, GraphViewportState, SubnetHost,
};
use wgpu_widgets::{GlyphonBackend, RawInput, WidgetRenderer};

/// The editor chrome for the embed: the shared `HudStack` + a `WidgetRenderer` on egui's device.
struct Chrome {
    hud: HudStack,
    widgets: WidgetRenderer,
}

impl Chrome {
    fn new(rs: &RenderState) -> Self {
        // CRITICAL: the widget pipeline bakes the color-target format into itself, so it MUST match
        // the graph's offscreen color = egui's `target_format` (the same value nodes-egui passes to
        // `GraphRenderer::attach_wgpu`). A mismatched format = pipeline-incompatible-with-render-pass
        // validation error. `view_proj` here is a placeholder; `render` sets the real one per frame.
        let widgets =
            WidgetRenderer::new(&rs.device, rs.target_format, screen_hud_view_proj((1, 1)));
        Self {
            hud: HudStack::default(),
            widgets,
        }
    }
}

struct PocApp {
    viewport: GraphViewportState,
    host: SubnetHost,
    render_state: Option<RenderState>,
    chrome: Option<Chrome>,
}

impl PocApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut viewport = GraphViewportState::default();
        let render_state = cc.wgpu_render_state.clone();
        if let Some(rs) = render_state.clone() {
            viewport.configure_wgpu_render_state(rs);
        }
        Self {
            viewport,
            host: SubnetHost::with_builtins(),
            render_state,
            chrome: None,
        }
    }
}

impl eframe::App for PocApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let Some(rs) = self.render_state.clone() else {
            ui.label("This PoC needs the eframe wgpu backend (Renderer::Wgpu).");
            return;
        };

        // 1) Graph renders offscreen into GraphRenderer's own color texture, shown as an egui image.
        self.viewport.ui(
            ui,
            &mut self.host,
            &GraphViewportConfig::EDITOR,
            GraphViewportExtras::default(),
        );

        let (w, h) = self.viewport.renderer.viewport_size();
        if w == 0 || h == 0 {
            return;
        }
        let vp_px = (w, h);
        let vp = Vec2::new(w as f32, h as f32);
        let hud_vp = screen_hud_view_proj(vp_px);

        // Values that need `self.host` / `self.viewport.camera` are read BEFORE the `&mut chrome`
        // borrow, so the later `overlay_targets` (&mut renderer) has no field-borrow conflict.
        let nodes = self.host.subnet().nodes.len();
        let ppw = self.viewport.camera.px_per_world_at_target(vp);
        let full = self.viewport.last_viewport_rect;

        let chrome = self.chrome.get_or_insert_with(|| Chrome::new(&rs));

        // 2) Neutral input: egui pointer -> RawInput -> the SAME InputRouter the editor uses.
        //    The status-bar surface is screen-pinned, so its cursor space is full-viewport pixels.
        if let Some(p) = ui.input(|i| i.pointer.hover_pos())
            && full.contains(p)
        {
            let (lx, ly) = (p.x - full.min.x, p.y - full.min.y);
            let (pressed, released) =
                ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released()));
            let mut events = vec![RawInput::CursorMoved { x: lx, y: ly }];
            if pressed {
                events.push(RawInput::MouseButton {
                    button: MouseButton::Left,
                    pressed: true,
                });
            }
            if released {
                events.push(RawInput::MouseButton {
                    button: MouseButton::Left,
                    pressed: false,
                });
            }
            for ev in events {
                // Bool panels (status-bar splitter, minimap pan) go through HudStack::handle_input;
                // the menu bar is an action-returning panel, driven directly and logged.
                chrome
                    .hud
                    .handle_input(ev.clone(), hud_vp, vp, &mut self.viewport.camera);
                if let Some(action) = chrome.hud.menu_bar.handle_input(ev, hud_vp, vp).action {
                    log::info!("embed menu action: {action}");
                }
            }
        }

        // 3) Per-panel sync (host supplies each panel's own data) + live status text.
        chrome
            .hud
            .status_bar
            .set_text(section::MODE, "Mode: Embed (egui)");
        chrome
            .hud
            .status_bar
            .set_text(section::SPEED, "Fly: RMB (winit only)");
        chrome
            .hud
            .status_bar
            .set_text(section::ZOOM, format!("Zoom: {ppw:.2}"));
        chrome
            .hud
            .status_bar
            .set_text(section::NODES, format!("Nodes: {nodes}"));
        chrome.hud.status_bar.sync(vp_px);
        chrome.hud.minimap.sync(
            self.host.subnet(),
            &self.viewport.selected,
            &self.viewport.camera,
            vp_px,
        );
        // Full editor chrome: the top menu bar + subnet breadcrumb render too (the embed sits at
        // the root scope, so the breadcrumb path is empty / depth 0).
        chrome.hud.breadcrumb.sync("", 0, vp_px);
        chrome.hud.menu_bar.sync(vp_px);

        // 4) Composite the HUD overlay into the graph's OWN color texture (single target), via the
        //    widget-agnostic `overlay_targets` seam — the embed equivalent of the editor drawing
        //    HUD into its winit surface. Runs after the graph render, so it's a LoadOp::Load pass.
        let draw = chrome.hud.overlay(vp_px);
        let widgets = &mut chrome.widgets;
        let res = self.viewport.renderer.overlay_targets().map(|t| {
            widgets.render(
                &mut GlyphonBackend::new(t.text),
                t.device,
                t.queue,
                &draw,
                t.color,
                t.depth,
                hud_vp,
            )
        });
        if let Some(Err(e)) = res {
            log::warn!("HUD overlay render failed: {e}");
        }

        ui.ctx().request_repaint();
    }
}

fn main() -> eframe::Result<()> {
    env_logger::init();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "nodes-egui — full editor chrome (HudStack) in egui",
        options,
        Box::new(|cc| Ok(Box::new(PocApp::new(cc)))),
    )
}
