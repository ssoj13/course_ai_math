//! Headless smoke test: render several procedural bezier tubes into a PNG.
//!
//! Usage:
//! `cargo run -p wgpu-tube --example tubes -- <out_dir>`

use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;

use glam::Vec3;
use pollster::FutureExt;
use wgpu::{
    CommandEncoderDescriptor, Device, Extent3d, Queue, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureViewDescriptor,
};
use wgpu_tube::{pack_caps, CapKind, TubeInstance, TubeRenderer, TubeRendererDesc};

const W: u32 = 1280;
const H: u32 = 720;
const COLOR_FMT: TextureFormat = TextureFormat::Rgba8Unorm;
const DEPTH_FMT: TextureFormat = TextureFormat::Depth32Float;

fn init() -> Result<(Device, Queue), String> {
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            // `..Default::default()`: wgpu adds fields here between releases, and an example that
            // names every one stops compiling on each bump. (A previous fix left a literal
            // backtick-n in this file, which is why it did not compile at all.)
            ..Default::default()
        })
        .block_on()
        .map_err(|e| format!("no wgpu adapter: {e:?}"))?;

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("wgpu_tube.example_device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
            experimental_features: Default::default(),
        })
        .block_on()
        .map_err(|e| format!("device request failed: {e:?}"))?;

    Ok((device, queue))
}

fn make_instance(
    p0: Vec3,
    p1: Vec3,
    p2: Vec3,
    p3: Vec3,
    r0: f32,
    r1: f32,
    color0: [f32; 4],
    color1: [f32; 4],
    start_cap: CapKind,
    end_cap: CapKind,
) -> TubeInstance {
    TubeInstance {
        p0: [p0.x, p0.y, p0.z, r0],
        p1: [p1.x, p1.y, p1.z, 0.0],
        p2: [p2.x, p2.y, p2.z, 0.0],
        p3: [p3.x, p3.y, p3.z, r1],
        color0,
        color1,
        caps: pack_caps(start_cap, end_cap),
        _pad: [0; 3],
    }
}

fn scene_instances() -> Vec<TubeInstance> {
    vec![
        make_instance(
            Vec3::new(-2.2, -0.6, 0.6),
            Vec3::new(-1.7, 0.9, 0.4),
            Vec3::new(-0.8, -0.8, -0.3),
            Vec3::new(0.0, 0.3, -0.7),
            0.12,
            0.05,
            [1.0, 0.45, 0.25, 0.95],
            [1.0, 0.92, 0.35, 0.7],
            CapKind::Flat,
            CapKind::Flat,
        ),
        make_instance(
            Vec3::new(-1.7, 1.1, 0.3),
            Vec3::new(-1.1, 1.4, -0.7),
            Vec3::new(-0.2, 0.8, -0.2),
            Vec3::new(0.9, 0.5, 0.2),
            0.08,
            0.16,
            [0.35, 0.9, 1.0, 0.85],
            [0.2, 0.45, 1.0, 0.65],
            CapKind::Round,
            CapKind::Round,
        ),
        make_instance(
            Vec3::new(-0.9, -1.2, 0.8),
            Vec3::new(-0.1, -0.9, 0.2),
            Vec3::new(0.8, -0.3, -0.4),
            Vec3::new(1.8, 0.0, -0.8),
            0.06,
            0.06,
            [0.52, 1.0, 0.58, 0.9],
            [0.1, 0.8, 0.25, 0.55],
            CapKind::Cube,
            CapKind::Cube,
        ),
        make_instance(
            Vec3::new(0.2, -0.4, 0.5),
            Vec3::new(0.9, 0.5, 0.9),
            Vec3::new(1.8, -0.3, 0.0),
            Vec3::new(2.6, 0.7, -0.3),
            0.05,
            0.09,
            [0.95, 0.55, 1.0, 0.85],
            [0.45, 0.1, 0.95, 0.75],
            CapKind::Flat,
            CapKind::Arrow,
        ),
        make_instance(
            Vec3::new(-2.5, 0.2, -0.8),
            Vec3::new(-1.8, -0.6, -1.4),
            Vec3::new(-0.7, 0.9, -1.2),
            Vec3::new(0.4, 0.4, -1.0),
            0.03,
            0.12,
            [0.95, 0.95, 0.95, 0.75],
            [0.3, 0.3, 0.3, 0.45],
            CapKind::Round,
            CapKind::Cube,
        ),
        // Strongly NON-PLANAR, high-curvature curve: the control points spiral out of any single
        // plane so the tangent sweeps through every world axis. The old axis-picking frame twisted
        // badly here; a correct RMF keeps the ring coherent along the whole loop.
        make_instance(
            Vec3::new(-1.4, -1.4, -1.6),
            Vec3::new(1.8, -1.2, 1.9),
            Vec3::new(-1.9, 1.3, 1.7),
            Vec3::new(1.6, 1.5, -1.6),
            0.10,
            0.10,
            [1.0, 0.30, 0.55, 0.95],
            [0.30, 1.0, 0.85, 0.9],
            CapKind::Round,
            CapKind::Arrow,
        ),
    ]
}

fn render(device: &Device, queue: &Queue) -> Result<Vec<u8>, String> {
    let color = device.create_texture(&TextureDescriptor {
        label: Some("wgpu_tube.example_color"),
        size: Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: COLOR_FMT,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color.create_view(&TextureViewDescriptor::default());

    let depth = device.create_texture(&TextureDescriptor {
        label: Some("wgpu_tube.example_depth"),
        size: Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: DEPTH_FMT,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&TextureViewDescriptor::default());

    let mut renderer = TubeRenderer::new(
        device,
        TubeRendererDesc {
            color_format: COLOR_FMT,
            depth_format: Some(DEPTH_FMT),
            sample_count: 1,
            ring_segments: 14,
            length_segments: 48,
            reversed_z: false,
        },
    );

    let view = glam::camera::rh::view::look_at_mat4(Vec3::new(0.5, 1.4, 5.4), Vec3::new(0.2, 0.2, -0.3), Vec3::Y);
    let proj = glam::camera::rh::proj::directx::perspective(47.0_f32.to_radians(), W as f32 / H as f32, 0.05, 100.0);
    renderer.set_camera_with_light(queue, proj * view, Vec3::new(0.45, 0.85, 0.35));

    let instances = scene_instances();
    renderer.update_instances(device, queue, &instances);

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("wgpu_tube.example_encoder"),
    });

    // Populate the RMF `frames` buffer before the render pass reads it in the vertex stage.
    renderer.encode_prepass(&mut encoder);

    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("wgpu_tube.example_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.05,
                        g: 0.06,
                        b: 0.08,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        renderer.draw(&mut pass);
    }

    queue.submit(Some(encoder.finish()));
    read_back(device, queue, &color)
}

fn read_back(device: &Device, queue: &Queue, color: &wgpu::Texture) -> Result<Vec<u8>, String> {
    let unpadded_bpr = W * 4;
    let padded_bpr = unpadded_bpr.div_ceil(256) * 256;

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("wgpu_tube.example_readback"),
        size: (padded_bpr * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("wgpu_tube.example_copy_encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: color,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bpr),
                rows_per_image: Some(H),
            },
        },
        Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));

    let (tx, rx) = channel();
    readback.slice(..).map_async(wgpu::MapMode::Read, move |res| {
        if tx.send(res).is_err() {
            log::warn!("wgpu-tube example: map callback receiver dropped");
        }
    });

    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .map_err(|e| format!("device poll failed: {e:?}"))?;

    let map_result = rx.recv().map_err(|e| format!("map channel receive failed: {e}"))?;
    map_result.map_err(|e| format!("buffer map failed: {e:?}"))?;

    let data = readback.slice(..).get_mapped_range().expect("get_mapped_range"); // wgpu 30: fallible
    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for row in 0..H {
        let offset = (row * padded_bpr) as usize;
        rgba.extend_from_slice(&data[offset..offset + unpadded_bpr as usize]);
    }
    drop(data);
    readback.unmap();

    Ok(rgba)
}

fn write_png(path: &Path, rgba: &[u8]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("create png failed: {e}"))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), W, H);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("png header failed: {e}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| format!("png write failed: {e}"))?;
    Ok(())
}

fn main() -> Result<(), String> {
    let out_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    fs::create_dir_all(&out_dir)
        .map_err(|e| format!("create output directory '{}' failed: {e}", out_dir.display()))?;

    let (device, queue) = init()?;
    let rgba = render(&device, &queue)?;

    let out_path = out_dir.join("tubes.png");
    write_png(&out_path, &rgba)?;

    println!("wrote {}", out_path.display());
    Ok(())
}
