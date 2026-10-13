//! Smoke test: attach wgpu and render one frame offscreen.

use nodes_core::Subnet;
use nodes_view::{Camera, GraphRenderer, GraphRendererDesc, RenderParams};

fn main() {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false, // wgpu 30
    }))
    .expect("adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("nodes_view_example"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: Default::default(),
        trace: Default::default(),
        experimental_features: Default::default(),
    }))
    .expect("device");

    let (registry, types) = nodes_core::default_registry();
    let mut subnet = Subnet::new();
    let _ = subnet.create_node("Float", [10.0, 10.0], &registry, &types);

    let mut renderer = GraphRenderer::new(GraphRendererDesc::default());
    renderer.attach_wgpu(&device, &queue, wgpu::TextureFormat::Rgba8Unorm);
    renderer.resize(640, 480);
    let camera = Camera::default();
    let params = RenderParams {
        subnet: &subnet,
        camera: &camera,
        layout: None,
        selected: &[],
        wire_preview: None,
        marquee_uv: None,
        value_types: &types,
        registry: &registry,
        hover: None,
        node_tint: None,
        // No widget layer in this bare example, so let the renderer label the cards.
        draw_node_titles: true,
    };
    renderer.render(&params).expect("render");
    assert!(renderer.color_texture_view().is_some());
    println!("nodes-view winit_minimal: render ok");
}
