//! Interactive winit + wgpu graph viewport (pan, zoom, drag nodes, pick).
//!
//! Run: `cargo run -p nodes-view --example winit_interactive`

use std::sync::Arc;

use glam::Vec2;
use nodes_core::{NodeId, NodeTypeRegistry, Subnet, ValueTypes, cook, snap_to_grid};
use nodes_view::{Camera, GraphRenderer, GraphRendererDesc, PickTarget, RenderParams};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

struct App {
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    config: Option<wgpu::SurfaceConfiguration>,
    renderer: GraphRenderer,
    camera: Camera,
    subnet: Subnet,
    registry: NodeTypeRegistry,
    value_types: ValueTypes,
    selected: Option<NodeId>,
    drag: Option<DragNode>,
    pan: Option<Vec2>,
    cursor: Vec2,
    size: (u32, u32),
}

#[derive(Clone)]
struct DragNode {
    id: NodeId,
    node_start: [f32; 2],
    cursor_start: Vec2,
}

impl Default for App {
    fn default() -> Self {
        let (registry, value_types) = nodes_core::default_registry();
        let mut subnet = Subnet::new();
        for row in 0..4 {
            for col in 0..5 {
                let _ = subnet
                    .create_node(
                        "Float",
                        [col as f32 * 220.0, row as f32 * 140.0],
                        &registry,
                        &value_types,
                    )
                    .expect("create node");
            }
        }
        let _ = subnet.create_node("Add", [500.0, 200.0], &registry, &value_types);
        cook(&mut subnet, &registry, &value_types);

        Self {
            window: None,
            surface: None,
            device: None,
            queue: None,
            config: None,
            renderer: GraphRenderer::new(GraphRendererDesc::default()),
            camera: Camera::default(),
            subnet,
            registry,
            value_types,
            selected: None,
            drag: None,
            pan: None,
            cursor: Vec2::ZERO,
            size: (1280, 720),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("nodes-view interactive")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
                )
                .expect("window"),
        );
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false, // wgpu 30
        }))
        .expect("adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("winit_interactive"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: Default::default(),
            trace: Default::default(),
            experimental_features: Default::default(),
        }))
        .expect("device");
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            color_space: wgpu::SurfaceColorSpace::Auto, // wgpu 30
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        self.renderer.attach_wgpu(&device, &queue, format);
        self.renderer.resize(config.width, config.height);
        // Default zoom: 1 world unit ≈ 1 screen pixel at the plane (perspective dolly).
        self.camera
            .reset_default_zoom(Vec2::new(config.width as f32, config.height as f32));
        self.size = (config.width, config.height);

        self.window = Some(window);
        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.config = Some(config);
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && matches!(event.logical_key, Key::Named(NamedKey::Escape))
                {
                    event_loop.exit();
                }
                if event.state == ElementState::Pressed
                    && matches!(event.logical_key, Key::Named(NamedKey::Home))
                {
                    self.camera.reset_top_down();
                }
            }
            WindowEvent::Resized(size) => {
                if let (Some(surface), Some(device), Some(config)) = (
                    self.surface.as_ref(),
                    self.device.as_ref(),
                    self.config.as_mut(),
                ) {
                    config.width = size.width.max(1);
                    config.height = size.height.max(1);
                    surface.configure(device, config);
                    self.renderer.resize(config.width, config.height);
                    self.camera
                        .reset_default_zoom(Vec2::new(config.width as f32, config.height as f32));
                    self.size = (config.width, config.height);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let uv = self.cursor;
                match (state, button) {
                    (ElementState::Pressed, MouseButton::Middle) => {
                        self.pan = Some(uv);
                    }
                    (ElementState::Released, MouseButton::Middle) => {
                        self.pan = None;
                    }
                    (ElementState::Pressed, MouseButton::Left) => {
                        if let Some(PickTarget::NodeBody(id)) =
                            self.renderer.pick_target_at([uv.x, uv.y], 2.0)
                        {
                            self.selected = Some(id);
                            if let Some(n) = self.subnet.nodes.get(&id) {
                                self.drag = Some(DragNode {
                                    id,
                                    node_start: n.view_pos(),
                                    cursor_start: uv,
                                });
                            }
                        } else {
                            self.selected = None;
                            self.drag = None;
                        }
                    }
                    (ElementState::Released, MouseButton::Left) => {
                        self.drag = None;
                    }
                    _ => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = self.screen_to_graph(Vec2::new(position.x as f32, position.y as f32));
                let uv = self.cursor;
                if let Some(start) = self.pan {
                    self.camera.pan_on_plane(uv - start);
                    self.pan = Some(uv);
                }
                if let Some(drag) = self.drag.clone() {
                    let delta = uv - drag.cursor_start;
                    let new_pos =
                        snap_to_grid([drag.node_start[0] + delta.x, drag.node_start[1] + delta.y]);
                    if let Some(n) = self.subnet.nodes.get_mut(&drag.id) {
                        n.set_view_pos(new_pos);
                        n.set_dirty(true);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let factor = match delta {
                    MouseScrollDelta::LineDelta(_, y) => 1.0 + y * 0.08,
                    MouseScrollDelta::PixelDelta(p) => 1.0 + (p.y as f32) * 0.002,
                };
                let screen = Vec2::new(self.cursor.x, self.cursor.y);
                self.camera
                    .zoom_toward(screen, factor.clamp(0.5, 2.0), self.viewport_size());
            }
            WindowEvent::RedrawRequested => {
                self.draw_frame();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}

impl App {
    fn viewport_size(&self) -> Vec2 {
        Vec2::new(self.size.0 as f32, self.size.1 as f32)
    }

    fn screen_to_graph(&self, screen: Vec2) -> Vec2 {
        self.camera
            .screen_to_graph(screen, self.size.0 as f32, self.size.1 as f32)
    }

    fn draw_frame(&mut self) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let selected: Vec<NodeId> = self.selected.into_iter().collect();
        let params = RenderParams {
            subnet: &self.subnet,
            camera: &self.camera,
            layout: None,
            selected: &selected,
            wire_preview: None,
            marquee_uv: None,
            value_types: &self.value_types,
            registry: &self.registry,
            hover: None,
            node_tint: None,
            // No widget layer in this bare example, so let the renderer label the cards.
            draw_node_titles: true,
        };
        let _ = self.renderer.render_to(&params, &view);
        if let Some(q) = self.queue.as_ref() {
            q.present(frame);
        } // wgpu 30: Queue::present
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("run");
}
