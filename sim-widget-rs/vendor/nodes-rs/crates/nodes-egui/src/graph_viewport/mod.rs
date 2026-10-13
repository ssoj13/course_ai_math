//! Shared graph viewport: input, camera, and GPU presentation for any [`GraphViewportHost`].
//!
//! ## Used by
//!
//! - `nodes-editor::panels::graph_viewport::ui_editor` — full editor (`GraphViewportConfig::EDITOR`)
//! - `mermaid-rs` diagram viewer — read-only or editable diagram hosts
//!
//! ## GPU path
//!
//! When `render_state` is set (via [`GraphViewportState::configure_wgpu_render_state`]):
//! resize → [`nodes_view::GraphRenderer::render`] → [`crate::GraphGpuBridge::present`] →
//! `egui::Image`. Otherwise the panel shows "Waiting for wgpu…".
//!
//! ## Host trait
//!
//! [`GraphViewportHost`] supplies subnet + registries; the editor impl is `EditorViewportHost` in `nodes-editor`.

mod config;
mod host;

pub use config::GraphViewportConfig;
pub use host::{
    GraphViewportExtras, GraphViewportHost, SubnetHost, ViewportEditKind, ViewportEditPhase,
    WireDropActions,
};

use std::collections::HashSet;

use egui::{self, Color32, Sense};
use glam::Vec2;
use nodes_core::{
    LayoutCache, MAX_NODE_H, MAX_NODE_W, MIN_NODE_H, MIN_NODE_W, NodeId, NodeTypeRegistry, PinSide,
    auto_layout_subnet, compute_static_layout, compute_static_layout_scoped,
    cycle_view_mode_and_resize, is_frame_node, is_note_node, layout_uv_bounds,
    layout_uv_bounds_selected, pin_visual_for_type, snap_to_grid,
};
use nodes_view::{
    CamControl, Camera, CameraKind, FlyAxes, GraphRenderer, GraphRendererDesc, NavInput,
    PickTarget, RenderParams, WirePreview, apply_nav,
};

use crate::GraphGpuBridge;

const ORBIT_SENSITIVITY: f32 = 0.004;
/// Mouse-look sensitivity while flying (radians per pixel of cursor motion).
/// Matches the winit editor's tuned `camera.look_sensitivity` default so RMB-fly
/// feels IDENTICAL across both frontends (was 10x too hot at 0.005).
const LOOK_SENSITIVITY: f32 = 0.0005;

// ── Drag/interaction state ────────────────────────────────────────────────────

enum DragState {
    None,
    Node {
        id: NodeId,
        node_start: [f32; 2],
        cursor_start: Vec2,
    },
    ResizeWidth {
        id: NodeId,
        left_x: f32,
        start_width: f32,
        cursor_start_x: f32,
    },
    ResizeBox {
        id: NodeId,
        top_left: [f32; 2],
        start_size: [f32; 2],
        cursor_start: Vec2,
    },
}

enum ResizeHit {
    None,
    Width,
    Box,
}

enum WireState {
    None,
    Dragging {
        from_node: NodeId,
        from_output: usize,
        start_world: [f32; 2],
    },
}

// ── Viewport state ────────────────────────────────────────────────────────────

/// Per-window graph viewport: renderer, camera, selection, and interaction state.
pub struct GraphViewportState {
    pub renderer: GraphRenderer,
    pub camera: Camera,
    pub gpu_bridge: GraphGpuBridge,
    render_state: Option<egui_wgpu::RenderState>,
    pub selected: HashSet<NodeId>,
    drag: DragState,
    wire: WireState,
    hover: Option<PickTarget>,
    marquee_start: Option<egui::Pos2>,
    marquee_end: Option<egui::Pos2>,
    marquee_nodes: HashSet<NodeId>,
    initial_fit_pending: bool,
    /// Last viewport size used for fit (refit when dock grows from zero).
    last_fit_viewport: Vec2,
    /// Graph panel rect from the last frame (for tab menu / paste).
    pub last_viewport_rect: egui::Rect,
    /// True while flying (RMB held): the Space fly backend is engaged with perspective open.
    /// Cleared on RMB release; the perspective FOV persists until an explicit flatten
    /// (H/Home/F1/Esc), matching the standalone nodes-rs editor.
    flying: bool,
    /// Shared perspective-open/flatten policy (animated FOV 2°<->60°) driving the fly camera.
    /// The shared host-agnostic camera gesture brain (lens Flat↔Perspective + FOV ease + orbit
    /// policy) — the SAME type the winit `nodes-editor` drives, so both frontends share one
    /// implementation instead of drifting copies. See [`nodes_view::CamControl`].
    cam_ctl: CamControl,
}

impl Default for GraphViewportState {
    fn default() -> Self {
        Self {
            renderer: GraphRenderer::new(GraphRendererDesc::default()),
            camera: Camera::default(),
            gpu_bridge: GraphGpuBridge::new(),
            render_state: None,
            selected: HashSet::new(),
            drag: DragState::None,
            wire: WireState::None,
            hover: None,
            marquee_start: None,
            marquee_end: None,
            marquee_nodes: HashSet::new(),
            initial_fit_pending: true,
            last_fit_viewport: Vec2::ZERO,
            last_viewport_rect: egui::Rect::NOTHING,
            flying: false,
            cam_ctl: CamControl::new(),
        }
    }
}

impl GraphViewportState {
    /// Wire eframe/egui wgpu device into [`Self::renderer`] and enable the GPU viewport path.
    ///
    /// Called from `GraphEditorApp::new` (`nodes-editor`) when `CreationContext::wgpu_render_state`
    /// is available. Must use the same `device`/`queue` as egui.
    pub fn configure_wgpu_render_state(&mut self, render_state: egui_wgpu::RenderState) {
        let format = render_state.target_format;
        self.renderer
            .attach_wgpu(&render_state.device, &render_state.queue, format);
        self.render_state = Some(render_state);
    }

    pub fn reset_camera(&mut self) {
        self.camera.reset_top_down();
    }

    /// End any flight and return to the flat 2D lens: orbit backend + FOV animated back to
    /// ~2°. The egui counterpart of the winit editor's H/align "flatten" — perspective only
    /// closes here (or via view framing), never on RMB release, matching nodes-rs fidelity.
    fn return_to_flat(&mut self) {
        self.flying = false;
        // Ensure the orbit backend (reseating the pivot on the graph plane — a plain `set_kind`
        // would seed a floating 28-unit-ahead target) and request the flat lens; the shared
        // `CamControl::step` (driven each frame in `ui`) eases the FOV back to ~2° once top-down.
        self.cam_ctl.ensure_orbit(&mut self.camera);
        self.cam_ctl.request_flat();
    }

    pub fn request_initial_fit(&mut self) {
        self.initial_fit_pending = true;
    }

    pub fn reset_interaction(&mut self) {
        self.drag = DragState::None;
        self.wire = WireState::None;
        self.marquee_start = None;
        self.marquee_end = None;
        self.marquee_nodes.clear();
    }

    pub fn marquee_nodes(&self) -> &HashSet<NodeId> {
        &self.marquee_nodes
    }

    /// Same as **L** / [`GraphViewportHost::auto_layout`]: only `auto_layout_subnet`.
    pub fn layout_nodes(&mut self, subnet: &mut nodes_core::Subnet) {
        auto_layout_subnet(subnet);
    }

    pub fn view_all(&mut self, subnet: &nodes_core::Subnet, vp: Vec2, scope: Option<NodeId>) {
        let layout = compute_static_layout_scoped(subnet, scope);
        if let Some((min, max)) = layout_uv_bounds(&layout) {
            self.camera.fit_plane_bounds(
                Vec2::new(min[0], min[1]),
                Vec2::new(max[0], max[1]),
                vp,
                0.15,
                false,
            );
            self.last_fit_viewport = vp;
        }
    }

    pub fn view_selected(&mut self, subnet: &nodes_core::Subnet, vp: Vec2, scope: Option<NodeId>) {
        let layout = compute_static_layout_scoped(subnet, scope);
        let ids: Vec<NodeId> = self.selected.iter().copied().collect();
        if let Some((min, max)) = layout_uv_bounds_selected(&layout, &ids) {
            self.camera.fit_plane_bounds(
                Vec2::new(min[0], min[1]),
                Vec2::new(max[0], max[1]),
                vp,
                0.2,
                true,
            );
        }
    }

    /// One frame of graph panel UI: camera, input, GPU render, optional inline widgets.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        host: &mut impl GraphViewportHost,
        config: &GraphViewportConfig,
        extras: GraphViewportExtras<'_>,
    ) {
        // Capture the optional status-tint map up front (a `Copy` shared ref) so it survives the
        // partial move of `extras.wire_drop` below and can feed `RenderParams` at draw time.
        let node_tint = extras.node_tint;
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let w = rect.width().max(1.0) as u32;
        let h = rect.height().max(1.0) as u32;
        let vp = Vec2::new(rect.width(), rect.height());

        const MIN_FIT_W: f32 = 200.0;
        const MIN_FIT_H: f32 = 150.0;
        let viewport_ready = vp.x >= MIN_FIT_W && vp.y >= MIN_FIT_H;
        let dock_just_sized = self.last_fit_viewport.x < MIN_FIT_W && viewport_ready;
        if !host.subnet().nodes.is_empty()
            && viewport_ready
            && (self.initial_fit_pending || dock_just_sized)
        {
            self.view_all(host.subnet(), vp, host.scope());
            self.initial_fit_pending = false;
            self.last_fit_viewport = vp;
        }

        // The graph keyboard shortcuts are SUPPRESSED while flying: RMB-fly owns WASD/QE/Space/C
        // for thrust, so letting A=view-all / F=view-selected / H=home / L=layout fire at the same
        // time would yank the camera mid-flight (the "everything jumps" bug). This mirrors the winit
        // editor, where a grabbed-cursor flight has its own input path and the shortcuts are inert.
        if response.hovered() && !self.flying {
            let input = ui.input(|i| i.clone());
            if input.key_pressed(egui::Key::H) || input.key_pressed(egui::Key::Home) {
                // H/Home: recenter top-down AND flatten perspective back to the flat 2D lens
                // (the egui counterpart of the winit editor's H/align).
                self.return_to_flat();
                self.reset_camera();
            }
            if input.key_pressed(egui::Key::Escape) {
                // Esc: drop a persisted perspective back to the flat 2D view.
                self.return_to_flat();
            }
            if input.key_pressed(egui::Key::A) {
                self.view_all(host.subnet(), vp, host.scope());
            }
            if input.key_pressed(egui::Key::F) && !self.selected.is_empty() {
                self.view_selected(host.subnet(), vp, host.scope());
            }
            if config.allow_auto_layout && input.key_pressed(egui::Key::L) {
                host.on_viewport_edit(
                    ViewportEditKind::AutoLayout,
                    ViewportEditPhase::Begin,
                    &self.camera,
                    &self.selected,
                );
                host.auto_layout();
            }
            if config.diagram_editable {
                if input.key_pressed(egui::Key::Z)
                    && input.modifiers.command
                    && !input.modifiers.shift
                {
                    host.undo_network(self);
                }
                if input.key_pressed(egui::Key::Z)
                    && input.modifiers.shift
                    && !input.modifiers.command
                {
                    host.redo_network(self);
                }
            }
            if input.key_pressed(egui::Key::OpenBracket) {
                host.undo_ui(self);
            }
            if input.key_pressed(egui::Key::CloseBracket) {
                host.redo_ui(self);
            }
            if input.key_pressed(egui::Key::O) {
                self.camera.cycle_orbit_mode();
            }
            if config.allow_view_mode_toggle && input.key_pressed(egui::Key::M) {
                for id in self.selected.iter().copied().collect::<Vec<_>>() {
                    let Some(node) = host.subnet().nodes.get(&id) else {
                        continue;
                    };
                    if is_frame_node(node) {
                        continue;
                    }
                    let _ = cycle_view_mode_and_resize(host.subnet_mut(), id);
                }
            }
        }

        // Camera navigation: decode this frame's bindings into a frontend-neutral
        // `NavInput`, then apply it via the SHARED `nodes_view::apply_nav` (the same
        // single-source-of-truth driver the winit editor uses). Bindings + px->value
        // conversions stay here; how each nav maps to the camera lives in the driver.
        let mut nav = NavInput::default();

        // Fly (Unreal-style hold-to-fly, matching the winit editor): HOLD the RIGHT mouse
        // button to fly the Fps camera — cursor motion = mouse-look, WASD/QE = thrust,
        // Space/C = up/down, E/Q = roll, Shift = boost. RMB no longer zooms (zoom is the
        // scroll wheel); releasing RMB restores the orbit camera. This is the SAME gesture
        // the standalone winit editor uses, so the embedded viewport feels identical.
        let (fwd, rgt, upd, rol, boost) = ui.input(|i| {
            (
                (i.key_down(egui::Key::W) as i32 - i.key_down(egui::Key::S) as i32) as f32,
                (i.key_down(egui::Key::D) as i32 - i.key_down(egui::Key::A) as i32) as f32,
                (i.key_down(egui::Key::Space) as i32 - i.key_down(egui::Key::C) as i32) as f32,
                (i.key_down(egui::Key::E) as i32 - i.key_down(egui::Key::Q) as i32) as f32,
                i.modifiers.shift,
            )
        });
        // RMB is the single hold-to-fly gate: engage on press over the viewport, disengage
        // on release (or when the button is no longer down, e.g. the window lost focus).
        let (sec_pressed, sec_down, sec_released) = ui.input(|i| {
            (
                i.pointer.secondary_pressed(),
                i.pointer.secondary_down(),
                i.pointer.secondary_released(),
            )
        });
        if sec_pressed && response.contains_pointer() && !self.flying {
            // Enter flight exactly like the standalone nodes-rs editor: switch to the Space fly
            // backend (if currently orbiting) and OPEN PERSPECTIVE — the shared `CamControl` eases
            // the flat ~2° lens up to 60° with a compensating dolly (in `step`, each frame). This is
            // the signature "right-click opens perspective" mechanic, not a plain FPS toggle.
            self.flying = true;
            if !matches!(self.camera.kind(), CameraKind::Fps | CameraKind::Space) {
                self.camera.set_kind(CameraKind::Space);
            }
            // Apply the winit editor's calibrated Spaceflight tuning (~1 node/sec cruise) so WASD
            // thrust isn't twitchy — the untuned backend default would launch the camera on a keytap.
            self.camera
                .controller()
                .apply_inertia(nodes_view::InertiaSettings {
                    thrust_sensitivity: 384.0,
                    ..nodes_view::InertiaSettings::space()
                });
            self.cam_ctl.open_perspective();
        }
        if self.flying && (sec_released || !sec_down) {
            // Fly ended: KEEP the Space backend and the perspective FOV so the camera coasts to
            // a stop in 3D (matches winit). Return to the flat 2D view via H/Home/F1/Esc.
            self.flying = false;
        }
        if self.flying {
            // Raw per-frame cursor delta drives mouse-look. egui can't lock the cursor the
            // way the winit editor does, so look tracks on-screen motion rather than raw
            // device deltas; LOOK_SENSITIVITY keeps the turn rate matched to the winit feel.
            let look = ui.input(|i| i.pointer.delta());
            // Match the winit editor: Spaceflight look is dialed to 0.5x (its inertial angular
            // momentum makes the effective turn stronger).
            let look_sens = LOOK_SENSITIVITY
                * if matches!(self.camera.kind(), CameraKind::Space) {
                    0.5
                } else {
                    1.0
                };
            nav.fly = Some(FlyAxes {
                forward: fwd,
                right: rgt,
                up: upd,
                roll: rol,
                boost,
                look_dyaw: -look.x * look_sens,
                look_dpitch: -look.y * look_sens,
            });
        } else {
            // Scroll — zoom toward cursor.
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    let cursor = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
                    let local = cursor - rect.min;
                    nav.zoom = Some((Vec2::new(local.x, local.y), 1.0 + scroll * 0.001));
                }
            }
            // MMB — grab-style pan (content follows the cursor).
            if response.dragged_by(egui::PointerButton::Middle) {
                let delta = response.drag_delta();
                let ppw = self.camera.px_per_world_at_target(vp).max(1e-6);
                nav.pan_uv = Some(Vec2::new(delta.x / ppw, delta.y / ppw));
            }
            // Alt+LMB drag — orbit, via the shared brain: it ensures the Houdini orbit backend (so an
            // orbit right after a flight tumbles instead of free-looking — the Space backend maps
            // Orbit to mouse-LOOK), opens perspective, and FOV-scales the step (no near-ortho wild
            // spin). Applied directly here rather than through `NavInput::orbit`.
            if ui.input(|i| i.modifiers.alt) && response.dragged_by(egui::PointerButton::Primary) {
                let delta = response.drag_delta();
                self.cam_ctl.orbit(
                    &mut self.camera,
                    Vec2::new(delta.x, delta.y),
                    ORBIT_SENSITIVITY,
                );
            }
        }
        let dt = ui.input(|i| i.stable_dt).clamp(1.0e-4, 0.1);
        // Advance the shared perspective-open/flatten FOV tween before applying this frame's nav.
        self.cam_ctl.step(
            &mut self.camera,
            dt,
            true,
            nodes_view::camera_control::DEFAULT_FOV_ANIM_SECS,
        );
        apply_nav(&mut self.camera, &nav, dt, vp);

        let selected_vec: Vec<NodeId> = self.selected.iter().copied().collect();
        let mut layout_frame = compute_static_layout_scoped(host.subnet(), host.scope());
        if self.render_state.is_some() {
            self.renderer.sync_interaction_pick(
                &layout_frame,
                host.subnet(),
                host.registry(),
                host.value_types(),
                &selected_vec,
                &self.camera,
            );
        }

        if response.hovered() {
            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                let local = pos - rect.min;
                let world = screen_to_graph(&self.camera, local, rect.width(), rect.height());
                let pad = pick_padding_world(&self.camera, Vec2::new(rect.width(), rect.height()));
                self.hover = self.renderer.pick_target_at(world, pad);
            }
        } else {
            self.hover = None;
        }

        let alt_held = ui.input(|i| i.modifiers.alt);
        let shift_held = ui.input(|i| i.modifiers.shift);

        if response.drag_started_by(egui::PointerButton::Primary)
            && !alt_held
            && let Some(pos) = response.interact_pointer_pos()
        {
            let local = pos - rect.min;
            let world = screen_to_graph(&self.camera, local, rect.width(), rect.height());
            let pad = pick_padding_world(&self.camera, Vec2::new(rect.width(), rect.height()));
            let world_v = Vec2::new(world[0], world[1]);
            match self.renderer.pick_target_at(world, pad) {
                Some(PickTarget::Pin { node, side, index }) => {
                    if config.allow_wiring {
                        match side {
                            PinSide::Output => {
                                let pin_world = self
                                    .renderer
                                    .last_layout_pin(node, side, index)
                                    .unwrap_or(world);
                                self.wire = WireState::Dragging {
                                    from_node: node,
                                    from_output: index,
                                    start_world: pin_world,
                                };
                            }
                            PinSide::Input => {
                                if ui.input(|i| i.modifiers.ctrl) {
                                    host.on_viewport_edit(
                                        ViewportEditKind::WireDisconnect,
                                        ViewportEditPhase::Begin,
                                        &self.camera,
                                        &self.selected,
                                    );
                                    disconnect_input_idx(host.subnet_mut(), node, index);
                                }
                            }
                        }
                    }
                }
                Some(PickTarget::ViewModeToggle(hit_id)) => {
                    if config.allow_view_mode_toggle {
                        cycle_view_mode(host, hit_id);
                    }
                    if !shift_held {
                        self.selected.clear();
                    }
                    self.selected.insert(hit_id);
                }
                Some(PickTarget::BypassFlag(hit_id)) => {
                    host.subnet_mut().toggle_bypass(hit_id);
                    if !shift_held {
                        self.selected.clear();
                    }
                    self.selected.insert(hit_id);
                }
                Some(PickTarget::DisplayFlag(hit_id)) => {
                    host.subnet_mut().set_display_node(hit_id);
                    if !shift_held {
                        self.selected.clear();
                    }
                    self.selected.insert(hit_id);
                }
                Some(PickTarget::NodeField { node: hit_id, .. }) => {
                    // Inline field value box. The egui bridge has no in-viewport field editor
                    // (that lives in the wgpu editor's widget layer), so a field click just
                    // selects its owning node.
                    if !shift_held {
                        self.selected.clear();
                    }
                    self.selected.insert(hit_id);
                }
                Some(PickTarget::NodeTitle(hit_id)) => {
                    if config.allow_node_drag {
                        host.on_viewport_edit(
                            ViewportEditKind::NodeDrag,
                            ViewportEditPhase::Begin,
                            &self.camera,
                            &self.selected,
                        );
                        let node_start = host
                            .subnet()
                            .nodes
                            .get(&hit_id)
                            .map(|n| n.view_pos())
                            .unwrap_or([0.0; 2]);
                        self.drag = DragState::Node {
                            id: hit_id,
                            node_start,
                            cursor_start: world_v,
                        };
                    }
                    if !shift_held {
                        self.selected.clear();
                    }
                    self.selected.insert(hit_id);
                }
                Some(PickTarget::NodeBody(hit_id)) => {
                    if config.allow_resize || config.allow_node_drag {
                        match node_resize_hit(&layout_frame, host.subnet(), hit_id, world) {
                            ResizeHit::Width if config.allow_resize => {
                                host.on_viewport_edit(
                                    ViewportEditKind::ResizeWidth,
                                    ViewportEditPhase::Begin,
                                    &self.camera,
                                    &self.selected,
                                );
                                if let Some(node) = host.subnet_mut().nodes.get(&hit_id) {
                                    self.drag = DragState::ResizeWidth {
                                        id: hit_id,
                                        left_x: node.view_pos()[0],
                                        start_width: node.view_size()[0],
                                        cursor_start_x: world_v.x,
                                    };
                                    if !shift_held {
                                        self.selected.clear();
                                    }
                                    self.selected.insert(hit_id);
                                }
                            }
                            ResizeHit::Box if config.allow_resize => {
                                host.on_viewport_edit(
                                    ViewportEditKind::ResizeBox,
                                    ViewportEditPhase::Begin,
                                    &self.camera,
                                    &self.selected,
                                );
                                if let Some(node) = host.subnet_mut().nodes.get(&hit_id) {
                                    self.drag = DragState::ResizeBox {
                                        id: hit_id,
                                        top_left: node.view_pos(),
                                        start_size: [node.view_size()[0], node.view_size()[1]],
                                        cursor_start: world_v,
                                    };
                                    if !shift_held {
                                        self.selected.clear();
                                    }
                                    self.selected.insert(hit_id);
                                }
                            }
                            ResizeHit::None => {
                                if config.allow_node_drag {
                                    host.on_viewport_edit(
                                        ViewportEditKind::NodeDrag,
                                        ViewportEditPhase::Begin,
                                        &self.camera,
                                        &self.selected,
                                    );
                                    let node_start = host
                                        .subnet()
                                        .nodes
                                        .get(&hit_id)
                                        .map(|n| n.view_pos())
                                        .unwrap_or([0.0; 2]);
                                    self.drag = DragState::Node {
                                        id: hit_id,
                                        node_start,
                                        cursor_start: world_v,
                                    };
                                }
                                if !shift_held {
                                    self.selected.clear();
                                }
                                self.selected.insert(hit_id);
                            }
                            ResizeHit::Width | ResizeHit::Box => {}
                        }
                    } else {
                        if !shift_held {
                            self.selected.clear();
                        }
                        self.selected.insert(hit_id);
                    }
                }
                None => {
                    if !shift_held {
                        self.selected.clear();
                    }
                    self.marquee_start = Some(pos);
                    self.marquee_end = Some(pos);
                    self.marquee_nodes.clear();
                }
            }
        }

        if response.dragged_by(egui::PointerButton::Primary) && !alt_held {
            if let (Some(start), Some(pos)) = (self.marquee_start, response.interact_pointer_pos())
            {
                self.marquee_end = Some(pos);
                update_marquee_nodes(
                    &layout_frame,
                    &self.camera,
                    rect,
                    start,
                    pos,
                    &mut self.marquee_nodes,
                );
            }

            match &self.drag {
                DragState::Node {
                    id,
                    node_start,
                    cursor_start,
                } if config.allow_node_drag => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let local = pos - rect.min;
                        let world = self.camera.screen_to_graph(
                            Vec2::new(local.x, local.y),
                            rect.width(),
                            rect.height(),
                        );
                        let delta = Vec2::new(world.x - cursor_start.x, world.y - cursor_start.y);
                        let id = *id;
                        let ns = *node_start;
                        if let Some(node) = host.subnet_mut().nodes.get_mut(&id) {
                            node.set_view_pos([ns[0] + delta.x, ns[1] + delta.y]);
                            node.set_dirty(true);
                        }
                    }
                }
                DragState::ResizeWidth {
                    id,
                    left_x,
                    start_width,
                    cursor_start_x,
                } if config.allow_resize => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let local = pos - rect.min;
                        let world = self.camera.screen_to_graph(
                            Vec2::new(local.x, local.y),
                            rect.width(),
                            rect.height(),
                        );
                        let dx = world.x - cursor_start_x;
                        let new_w = (start_width + dx).clamp(MIN_NODE_W, MAX_NODE_W);
                        if let Some(node) = host.subnet_mut().nodes.get_mut(id) {
                            let h = node.view_size()[1];
                            node.set_view_size([new_w, h]);
                            let y = node.view_pos()[1];
                            node.set_view_pos([*left_x, y]);
                            node.set_dirty(true);
                        }
                    }
                }
                DragState::ResizeBox {
                    id,
                    top_left,
                    start_size,
                    cursor_start,
                } if config.allow_resize => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let local = pos - rect.min;
                        let world = self.camera.screen_to_graph(
                            Vec2::new(local.x, local.y),
                            rect.width(),
                            rect.height(),
                        );
                        let dx = world.x - cursor_start.x;
                        let dy = world.y - cursor_start.y;
                        let new_w = (start_size[0] + dx).clamp(MIN_NODE_W, MAX_NODE_W);
                        let new_h = (start_size[1] + dy).clamp(MIN_NODE_H, MAX_NODE_H);
                        if let Some(node) = host.subnet_mut().nodes.get_mut(id) {
                            node.set_view_size([new_w, new_h]);
                            node.set_view_pos(*top_left);
                            node.set_dirty(true);
                        }
                    }
                }
                DragState::None => {}
                _ => {}
            }
        }

        if response.drag_stopped_by(egui::PointerButton::Primary) && !alt_held {
            let ended_gesture = match &self.drag {
                DragState::Node { .. } => Some(ViewportEditKind::NodeDrag),
                DragState::ResizeWidth { .. } => Some(ViewportEditKind::ResizeWidth),
                DragState::ResizeBox { .. } => Some(ViewportEditKind::ResizeBox),
                DragState::None => None,
            };

            if self.marquee_start.is_some() {
                for id in self.marquee_nodes.drain() {
                    self.selected.insert(id);
                }
                self.marquee_start = None;
                self.marquee_end = None;
            }

            if let DragState::Node { id, .. } = self.drag
                && let Some(node) = host.subnet_mut().nodes.get_mut(&id)
            {
                node.set_view_pos(snap_to_grid(node.view_pos()));
                node.set_dirty(true);
            }

            if config.allow_wiring
                && let WireState::Dragging {
                    from_node,
                    from_output,
                    ..
                } = self.wire
            {
                let mut connected = false;
                if let Some(pos) = response.interact_pointer_pos() {
                    let local = pos - rect.min;
                    let world = screen_to_graph(&self.camera, local, rect.width(), rect.height());
                    let pad =
                        pick_padding_world(&self.camera, Vec2::new(rect.width(), rect.height()));
                    if let Some(PickTarget::Pin {
                        node: to_node,
                        side: PinSide::Input,
                        index: to_input,
                    }) = self.renderer.pick_target_at(world, pad)
                    {
                        host.on_viewport_edit(
                            ViewportEditKind::WireConnect,
                            ViewportEditPhase::Begin,
                            &self.camera,
                            &self.selected,
                        );
                        connect_ports(host, from_node, from_output, to_node, to_input);
                        connected = true;
                    }
                }
                if !connected && let Some(pos) = response.interact_pointer_pos() {
                    let from_type =
                        output_type_idx(host.subnet(), host.registry(), from_node, from_output)
                            .to_string();
                    if let Some(wire_drop) = extras.wire_drop {
                        wire_drop.open_link_search(pos, from_node, from_output, from_type);
                    }
                }
            }
            if let Some(kind) = ended_gesture {
                host.on_viewport_edit(kind, ViewportEditPhase::End, &self.camera, &self.selected);
            }
            self.wire = WireState::None;
            self.drag = DragState::None;
        }

        if config.allow_delete
            && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        {
            if !self.selected.is_empty() {
                host.on_viewport_edit(
                    ViewportEditKind::Delete,
                    ViewportEditPhase::Begin,
                    &self.camera,
                    &self.selected,
                );
            }
            // Batch delete: one O(n + edges) pass instead of O(n²) per-node removals.
            let ids: Vec<_> = self.selected.drain().collect();
            host.subnet_mut().remove_many(&ids);
        }

        let wire_preview = if config.allow_wiring
            && let WireState::Dragging {
                from_node,
                from_output,
                start_world,
            } = &self.wire
        {
            let cursor = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
            let local = cursor - rect.min;
            let end = self.camera.screen_to_graph(
                Vec2::new(local.x, local.y),
                rect.width(),
                rect.height(),
            );
            let from_type =
                output_type_idx(host.subnet(), host.registry(), *from_node, *from_output);
            let color = pin_visual_for_type(from_type, host.value_types()).color;
            Some(WirePreview {
                start_world: *start_world,
                end_world: [end.x, end.y],
                color,
            })
        } else {
            None
        };

        layout_frame = compute_static_layout(host.subnet());

        if let Some(rs) = self.render_state.as_ref() {
            self.renderer.resize(w, h);
            let marquee_uv =
                marquee_uv_from_screen(&self.camera, rect, self.marquee_start, self.marquee_end);
            let params = RenderParams {
                subnet: host.subnet(),
                camera: &self.camera,
                layout: Some(&layout_frame),
                selected: &selected_vec,
                wire_preview,
                marquee_uv,
                value_types: host.value_types(),
                registry: host.registry(),
                hover: self.hover.clone(),
                node_tint,
                // Titles are drawn BY THE RENDERER as graph-plane labels (they ride the node's
                // own surface transform, so they survive perspective/orbit/rotation). This host
                // has no `wgpu-widgets` title bar, so nothing else would draw them.
                draw_node_titles: config.draw_node_titles,
            };
            let _ = self.renderer.render(&params);
            self.gpu_bridge.present(rs, &self.renderer);
            if let Some(tex) = self.gpu_bridge.texture_id() {
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter().image(tex, rect, uv, Color32::WHITE);
            }

            // Sticky-note text overlay. nodes-view's GPU renderer draws the note CARD but not its
            // text -- in the full winit editor the wgpu widget layer draws AND edits it. This
            // lightweight egui viewport has no such widget layer, so paint the note's multi-line
            // text here with egui so it is at least VISIBLE. This overlay is deliberately
            // READ-ONLY: on-board text EDITING is a full-editor (`nodes-editor`) capability, not
            // part of the embedded viewport -- an architectural boundary, not a stopgap. Best-effort
            // placement (axis-aligned) suits the default near-top-down lens.
            let ppw = self
                .camera
                .px_per_world_at_target(Vec2::new(rect.width(), rect.height()))
                .max(1e-6);
            for (id, ln) in &layout_frame.nodes {
                let Some(node) = host.subnet().nodes.get(id) else {
                    continue;
                };
                if !is_note_node(node) {
                    continue;
                }
                let text = node.note_text();
                if text.is_empty() {
                    continue;
                }
                let (hw, hh) = (ln.size[0] * 0.5, ln.size[1] * 0.5);
                let tl = self.camera.graph_to_screen(
                    Vec2::new(ln.center[0] - hw, ln.center[1] - hh),
                    rect.width(),
                    rect.height(),
                );
                let br = self.camera.graph_to_screen(
                    Vec2::new(ln.center[0] + hw, ln.center[1] + hh),
                    rect.width(),
                    rect.height(),
                );
                let card = egui::Rect::from_two_pos(
                    rect.min + egui::vec2(tl.x, tl.y),
                    rect.min + egui::vec2(br.x, br.y),
                );
                // Clip the text to the card (intersected with the panel) so it never spills.
                let clip = card.intersect(rect);
                if clip.width() < 1.0 || clip.height() < 1.0 {
                    continue;
                }
                // Padding + font scale with zoom so the text reads as painted onto the card
                // (mirrors the winit note editor's PAD/DEFAULT_FONT in graph units).
                let anchor = card.min + egui::vec2(6.0 * ppw, 5.0 * ppw);
                ui.painter().with_clip_rect(clip).text(
                    anchor,
                    egui::Align2::LEFT_TOP,
                    text,
                    egui::FontId::proportional((14.0 * ppw).max(1.0)),
                    Color32::from_rgb(235, 240, 250),
                );
            }

            // NOTE: node titles are NOT painted here. They are graph-plane labels emitted by the
            // renderer (`RenderParams::draw_node_titles` -> `nodes_view::text_labels`), because a
            // screen-space egui overlay cannot follow a node once the camera leaves the flat
            // top-down lens: this viewport is a real 3D scene (perspective fly camera, orbit,
            // per-node rotation), where a node projects to an arbitrary quadrilateral. The overlay
            // that used to live here derived an axis-aligned rect from two projected corners, so
            // titles skewed, mis-scaled (one `px_per_world` for nodes at different depths), ignored
            // occlusion, and flew apart for nodes behind the eye. The plane label rides the node's
            // own surface transform and is correct in every camera pose.

            if config.show_orbit_hud && response.hovered() {
                let label = format!("Orbit: {} (O)", self.camera.orbit_mode().label());
                ui.painter().text(
                    rect.left_top() + egui::vec2(8.0, 8.0),
                    egui::Align2::LEFT_TOP,
                    label,
                    egui::FontId::proportional(12.0),
                    Color32::from_gray(180),
                );
            }
        } else {
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_rgb(24, 24, 28));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Waiting for wgpu…",
                egui::FontId::proportional(14.0),
                Color32::GRAY,
            );
        }

        self.last_viewport_rect = rect;

        if config.sync_after_frame {
            host.on_frame_end(self, config.diagram_editable);
        }
    }
}

fn update_marquee_nodes(
    layout: &nodes_core::LayoutCache,
    camera: &Camera,
    viewport: egui::Rect,
    start_screen: egui::Pos2,
    end_screen: egui::Pos2,
    out: &mut HashSet<NodeId>,
) {
    let start = screen_to_graph_uv(camera, start_screen - viewport.min, viewport.size());
    let end = screen_to_graph_uv(camera, end_screen - viewport.min, viewport.size());
    let marquee = egui::Rect::from_two_pos(egui::pos2(start.x, start.y), egui::pos2(end.x, end.y));

    out.clear();
    for (id, node) in &layout.nodes {
        let hw = node.size[0] * 0.5;
        let hh = node.size[1] * 0.5;
        let cx = node.center[0];
        let cy = node.center[1];
        let node_rect =
            egui::Rect::from_min_max(egui::pos2(cx - hw, cy - hh), egui::pos2(cx + hw, cy + hh));
        if marquee.intersects(node_rect) {
            out.insert(*id);
        }
    }
}

/// Graph-UV marquee corners for [`RenderParams::marquee_uv`] (SDF overlay in nodes-view).
fn marquee_uv_from_screen(
    camera: &Camera,
    viewport: egui::Rect,
    start: Option<egui::Pos2>,
    end: Option<egui::Pos2>,
) -> Option<([f32; 2], [f32; 2])> {
    let (start, end) = start.zip(end)?;
    let start_uv = screen_to_graph_uv(camera, start - viewport.min, viewport.size());
    let end_uv = screen_to_graph_uv(camera, end - viewport.min, viewport.size());
    Some(([start_uv.x, start_uv.y], [end_uv.x, end_uv.y]))
}

fn cycle_view_mode(host: &mut impl GraphViewportHost, id: NodeId) {
    let _ = cycle_view_mode_and_resize(host.subnet_mut(), id);
}

fn node_resize_hit(
    layout: &LayoutCache,
    subnet: &nodes_core::Subnet,
    id: NodeId,
    world: [f32; 2],
) -> ResizeHit {
    let Some(n) = layout.nodes.get(&id) else {
        return ResizeHit::None;
    };
    let Some(node) = subnet.nodes.get(&id) else {
        return ResizeHit::None;
    };
    let right = n.center[0] + n.size[0] * 0.5;
    let bottom = n.center[1] + n.size[1] * 0.5;
    if is_frame_node(node) {
        const CORNER: f32 = 16.0;
        if world[0] >= right - CORNER
            && world[0] <= right + 4.0
            && world[1] >= bottom - CORNER
            && world[1] <= bottom + 4.0
        {
            return ResizeHit::Box;
        }
        return ResizeHit::None;
    }
    const STRIP: f32 = 12.0;
    if world[0] >= right - STRIP && world[0] <= right + 4.0 {
        ResizeHit::Width
    } else {
        ResizeHit::None
    }
}

fn pick_padding_world(camera: &Camera, viewport: Vec2) -> f32 {
    // 12 screen pixels in world units at the graph plane (perspective: 1 / pixels-per-world).
    12.0 / camera.px_per_world_at_target(viewport).max(1e-6)
}

fn screen_to_graph(camera: &Camera, local: egui::Vec2, w: f32, h: f32) -> [f32; 2] {
    let v = camera.screen_to_graph(Vec2::new(local.x, local.y), w, h);
    [v.x, v.y]
}

fn screen_to_graph_uv(camera: &Camera, local: egui::Vec2, size: egui::Vec2) -> egui::Vec2 {
    let v = camera.screen_to_graph(Vec2::new(local.x, local.y), size.x, size.y);
    egui::vec2(v.x, v.y)
}

/// Declared output port type for `node`'s output at `index`, from the registered
/// [`nodes_core::NodeDef`] (mirrors `nodes-view`'s resolver).
fn output_type_idx<'a>(
    subnet: &nodes_core::Subnet,
    registry: &'a NodeTypeRegistry,
    node: NodeId,
    index: usize,
) -> &'a str {
    subnet
        .nodes
        .get(&node)
        .and_then(|n| registry.get(n.type_name()))
        .and_then(|d| d.outputs.get(index))
        .map(|p| p.port_type.as_str())
        .unwrap_or("float")
}

/// Disconnect the input at port `index` on `node` (resolves the index to an attr name).
fn disconnect_input_idx(subnet: &mut nodes_core::Subnet, node: NodeId, index: usize) {
    if let Some(name) = subnet
        .nodes
        .get(&node)
        .and_then(|n| n.input_names().get(index).cloned())
    {
        subnet.disconnect_input(node, &name);
    }
}

/// Connect `from_node`'s output `from_idx` to `to_node`'s input `to_idx` (by port
/// index), honoring the same type-compatibility gate as [`nodes_core::Graph::connect`].
///
/// The host trait exposes `subnet`/`registry` separately, so the type check runs
/// under an immutable borrow first; the source is then written under a mutable borrow.
fn connect_ports(
    host: &mut impl GraphViewportHost,
    from_node: NodeId,
    from_idx: usize,
    to_node: NodeId,
    to_idx: usize,
) {
    // Phase 1 (immutable): resolve port names, source attr uuid, and type compat.
    let resolved = {
        let subnet = host.subnet();
        let registry = host.registry();
        let value_types = host.value_types();
        let Some(src) = subnet.nodes.get(&from_node) else {
            return;
        };
        let Some(dst) = subnet.nodes.get(&to_node) else {
            return;
        };
        let Some(out_name) = src.output_names().get(from_idx).cloned() else {
            return;
        };
        let Some(in_name) = dst.input_names().get(to_idx).cloned() else {
            return;
        };
        let Some(from_attr) = src.output_attr_id(&out_name) else {
            return;
        };
        if dst.input_attr_id(&in_name).is_none() {
            return;
        }
        let from_ty = registry
            .get(src.type_name())
            .and_then(|d| d.output_type(&out_name))
            .unwrap_or("Any");
        let to_ty = registry
            .get(dst.type_name())
            .and_then(|d| d.input_type(&in_name))
            .unwrap_or("Any");
        // Same gate `Graph::connect` runs — the value-type registry owns compatibility now.
        if !value_types.compatible(from_ty, to_ty) {
            return;
        }
        (in_name, from_attr)
    };
    let (in_name, from_attr) = resolved;

    // Phase 2 (mutable): write the source and refresh the index.
    let subnet = host.subnet_mut();
    if let Some(dst) = subnet.nodes.get_mut(&to_node) {
        dst.set_source(&in_name, from_node, &from_attr);
        dst.set_dirty(true);
    }
    subnet.rebuild_index();
}
