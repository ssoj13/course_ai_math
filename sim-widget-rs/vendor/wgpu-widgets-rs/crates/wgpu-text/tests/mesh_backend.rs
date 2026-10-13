//! Headless smoke test for the 3D mesh text backend.
//!
//! Builds a [`MeshBackend`] from a system font, prepares a couple of [`TextRun`]s, and
//! asserts the renderer ends up with drawable meshes. Skips gracefully when no GPU adapter
//! is available (CI without a device) or the font is missing, rather than failing.
#![cfg(feature = "engine-mesh3d")]

use glam::Mat4;
use wgpu_text::{MeshBackend, TextBackend, TextRun};

/// Try to acquire a headless wgpu device; `None` if the environment has no usable adapter.
fn headless() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::FutureExt::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
        ..Default::default()
    }))
    .ok()?;
    let (device, queue) = pollster::FutureExt::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("mesh_backend.test"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
        experimental_features: Default::default(),
    }))
    .ok()?;
    Some((device, queue))
}

#[test]
fn mesh_backend_prepares_drawable_meshes() {
    let Some((device, queue)) = headless() else {
        eprintln!("mesh_backend test skipped: no GPU adapter");
        return;
    };
    let Ok(font) = std::fs::read("C:/Windows/Fonts/arial.ttf") else {
        eprintln!("mesh_backend test skipped: arial.ttf not found");
        return;
    };

    let mut backend = MeshBackend::new(
        &device,
        wgpu::TextureFormat::Rgba8Unorm,
        Some(wgpu::TextureFormat::Depth32Float),
        &font,
        0.1,
    )
    .expect("construct MeshBackend");

    let runs = [
        TextRun {
            text: "Hello".to_string(),
            local_rect: [10.0, 8.0, 80.0, 20.0],
            font_size: 16.0,
            ..Default::default()
        },
        TextRun {
            text: "World".to_string(),
            local_rect: [10.0, 32.0, 80.0, 20.0],
            font_size: 16.0,
            ..Default::default()
        },
    ];

    backend
        .prepare(&device, &queue, &runs)
        .expect("prepare runs");
    backend.update_view_proj(&queue, Mat4::IDENTITY);

    assert!(
        backend.instance_count() >= 2,
        "expected at least one mesh per non-empty run, got {}",
        backend.instance_count()
    );
}
