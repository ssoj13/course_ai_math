//! Audit evidence: render a TILTED plane of text twice — once with
//! `TextFilterQuality::Linear`, once with `Anisotropic` — and write both PNGs
//! plus a pixel-diff so the two can be compared directly.
//!
//! Usage: `cargo run -p wgpu-text --example angled_quality -- <out_dir>`

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;

use glam::{Mat4, Vec3};
use pollster::FutureExt;
use wgpu::{
    CommandEncoderDescriptor, Device, Extent3d, Queue, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureViewDescriptor,
};

use wgpu_text::{PlaneLabel, TextEngine, TextFilterQuality};

const W: u32 = 900;
const H: u32 = 450;
const COLOR_FMT: TextureFormat = TextureFormat::Rgba8Unorm;

fn init() -> Result<(Device, Queue), String> {
    let instance = wgpu::Instance::default();
    let adapter = instance
        // `..Default::default()`, not a full literal: wgpu adds fields to this struct between
        // releases (`apply_limit_buckets` most recently), and an example that names every one of
        // them stops compiling on each bump while saying nothing about the text it exists to show.
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            ..Default::default()
        })
        .block_on()
        .map_err(|e| format!("no wgpu adapter: {e:?}"))?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("angled.device"),
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

/// A steeply tilted plane so glyphs are foreshortened/minified toward the top.
fn tilted_surface_xfm() -> Mat4 {
    let k = 0.011;
    let center = Mat4::from_translation(Vec3::new(-300.0, -130.0, 0.0));
    let scale = Mat4::from_scale(Vec3::new(k, -k, k)); // flip local +y-down to world +y-up
    let rot = Mat4::from_rotation_x(55.0_f32.to_radians());
    rot * scale * center
}

/// A flat, camera-facing plane (no tilt) — the common consumer path where LOD ≈ 0
/// and Linear must render exactly as it does today.
fn flat_surface_xfm() -> Mat4 {
    let k = 0.011;
    let center = Mat4::from_translation(Vec3::new(-300.0, -130.0, 0.0));
    let scale = Mat4::from_scale(Vec3::new(k, -k, k));
    scale * center
}

fn view_proj() -> Mat4 {
    let proj = glam::camera::rh::proj::directx::perspective(45.0_f32.to_radians(), W as f32 / H as f32, 0.1, 100.0);
    let view = glam::camera::rh::view::look_at_mat4(
        Vec3::new(0.0, 0.55, 3.0),
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::Y,
    );
    proj * view
}

fn label(xfm: Mat4, raster_scale: f32) -> PlaneLabel {
    let text = "\
Anisotropic Filtering Test\n\
The quick brown fox jumps\n\
over the lazy dog 0123456789\n\
mmmm nnnn iiii llll wwww ####\n\
AAA BBB CCC DDD EEE FFF GGG".to_string();
    let mut l = PlaneLabel::from_local_rect(xfm, 0.0, 0.0, 600.0, 260.0, text);
    l.font_size = 26.0;
    l.line_height = 34.0;
    l.color = [0.95, 0.96, 0.98, 1.0];
    l.raster_scale = raster_scale;
    l
}

fn render(device: &Device, queue: &Queue, quality: TextFilterQuality) -> Result<Vec<u8>, String> {
    // High bake resolution => atlas glyphs much larger than their on-screen
    // footprint on the receding plane => strong minification (the case mips /
    // anisotropy are supposed to help).
    render_xfm(device, queue, quality, tilted_surface_xfm(), 4.0)
}

fn render_xfm(
    device: &Device,
    queue: &Queue,
    quality: TextFilterQuality,
    xfm: Mat4,
    raster_scale: f32,
) -> Result<Vec<u8>, String> {
    let color = device.create_texture(&TextureDescriptor {
        label: Some("angled.color"),
        size: Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: COLOR_FMT,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let color_view = color.create_view(&TextureViewDescriptor::default());
    let depth = device.create_texture(&TextureDescriptor {
        label: Some("angled.depth"),
        size: Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Depth32Float,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&TextureViewDescriptor::default());

    let mut engine = TextEngine::with_quality(device, queue, COLOR_FMT, quality);
    let labels = [label(xfm, raster_scale)];
    engine.prepare_plane(device, queue, &labels)?;
    engine.update_plane_view_proj(queue, view_proj());
    let (cw, mw) = engine.atlas_dims();
    eprintln!(
        "  quality={quality:?}: {} glyph instances, atlas dims color={cw} mask={mw}",
        engine.plane_instance_count()
    );

    let mut enc = device.create_command_encoder(&CommandEncoderDescriptor { label: Some("angled.enc") });
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("angled.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.08, g: 0.09, b: 0.11, a: 1.0 }),
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
        engine.render_plane(&mut pass);
    }
    queue.submit(Some(enc.finish()));

    read_back(device, queue, &color)
}

fn read_back(device: &Device, queue: &Queue, color: &wgpu::Texture) -> Result<Vec<u8>, String> {
    let unpadded = W * 4;
    let bytes_per_row = unpadded.div_ceil(256) * 256;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("angled.readback"),
        size: (bytes_per_row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&CommandEncoderDescriptor { label: Some("angled.copy") });
    enc.copy_texture_to_buffer(
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
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(H),
            },
        },
        Extent3d { width: W, height: H, depth_or_array_layers: 1 },
    );
    queue.submit(Some(enc.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).expect("recv dropped"));
    device
        .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
        .map_err(|e| format!("poll: {e:?}"))?;
    rx.recv().map_err(|e| format!("chan: {e}"))?.map_err(|e| format!("map: {e:?}"))?;

    let data = slice.get_mapped_range().expect("get_mapped_range"); // wgpu 30: fallible
    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for row in 0..H {
        let start = (row * bytes_per_row) as usize;
        rgba.extend_from_slice(&data[start..start + unpadded as usize]);
    }
    drop(data);
    readback.unmap();
    Ok(rgba)
}

fn write_png(path: &Path, rgba: &[u8]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("create png: {e}"))?;
    let mut enc = png::Encoder::new(BufWriter::new(file), W, H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| format!("png header: {e}"))?;
    w.write_image_data(rgba).map_err(|e| format!("png data: {e}"))?;
    Ok(())
}

fn non_clear(rgba: &[u8]) -> u32 {
    let mut n = 0;
    for px in rgba.as_chunks::<4>().0 {
        // clear colour is (20,23,28) approx
        if (px[0] as i32 - 20).abs() > 12 || (px[1] as i32 - 23).abs() > 12 || (px[2] as i32 - 28).abs() > 12 {
            n += 1;
        }
    }
    n
}

fn main() -> Result<(), String> {
    let out_dir: PathBuf = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let (device, queue) = init()?;

    eprintln!("rendering Linear...");
    let linear = render(&device, &queue, TextFilterQuality::Linear)?;
    eprintln!("rendering Anisotropic {{ level: 16 }}...");
    let aniso = render(&device, &queue, TextFilterQuality::Anisotropic { level: 16 })?;

    eprintln!("rendering flat top-down Linear (common consumer path, raster_scale=1)...");
    let flat = render_xfm(&device, &queue, TextFilterQuality::Linear, flat_surface_xfm(), 1.0)?;

    let p_lin = out_dir.join("angled_linear.png");
    let p_ani = out_dir.join("angled_aniso.png");
    let p_flat = out_dir.join("flat_linear.png");
    write_png(&p_lin, &linear)?;
    write_png(&p_ani, &aniso)?;
    write_png(&p_flat, &flat)?;

    // Pixel diff between the two renders.
    let mut diff_px = 0u32;
    let mut max_ch = 0i32;
    for (a, b) in linear.as_chunks::<4>().0.iter().zip(aniso.chunks_exact(4)) {
        let mut differs = false;
        for c in 0..4 {
            let d = (a[c] as i32 - b[c] as i32).abs();
            if d > max_ch { max_ch = d; }
            if d != 0 { differs = true; }
        }
        if differs { diff_px += 1; }
    }

    println!("linear:  {} ({} non-clear px)", p_lin.display(), non_clear(&linear));
    println!("aniso:   {} ({} non-clear px)", p_ani.display(), non_clear(&aniso));
    println!("flat:    {} ({} non-clear px, top-down Linear)", p_flat.display(), non_clear(&flat));
    println!("DIFF linear-vs-aniso: {diff_px} differing px of {}, max per-channel delta = {max_ch}", W * H);
    if non_clear(&linear) < 300 {
        return Err(format!("linear frame looks blank ({} non-clear px)", non_clear(&linear)));
    }
    Ok(())
}
