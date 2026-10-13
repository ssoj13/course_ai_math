use half::f16 as Half;
pub(crate) fn device() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("a GPU adapter (this test is #[ignore]d for machines without one)");
    let desc = wgpu::DeviceDescriptor {
        required_features: adapter.features() & wgpu::Features::FLOAT32_FILTERABLE,
        ..Default::default()
    };
    pollster::block_on(adapter.request_device(&desc)).expect("device")
}

pub(crate) fn read_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    w: u32,
    h: u32,
    texel: u32,
) -> Vec<u8> {
    let row = (w * texel).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu-test.readback"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    enc.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([enc.finish()]);
    buf.slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.expect("map"));
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("poll");
    let data = buf.slice(..).get_mapped_range().expect("mapped range");
    let mut out = Vec::with_capacity((w * h * texel) as usize);
    for y in 0..h {
        let r = (y * row) as usize;
        out.extend_from_slice(&data[r..r + (w * texel) as usize]);
    }
    out
}

/// A render target of `format` that can be read back.
pub(crate) fn target(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    w: u32,
    h: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gpu-test.target"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// `f16` RGBA texels read back as `f32`.
pub(crate) fn halves(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| Half::from_bits(u16::from_le_bytes(*b)).to_f32())
        .collect()
}

/// Every encoding of the present pass against its CPU twin (`present::encode`),
/// dither off, over canvas values from below black to far above white: 8-bit
/// unorm, an `*Srgb` format (decoded, re-encoded by the hardware), 10-bit, PQ, HLG
/// (10-bit and float, a 600-nit peak so the system gamma is not 1.2) and scRGB. A
/// uniform canvas per value (a clear), read back from a small target.
#[test]
#[ignore = "needs a GPU adapter"]
fn present_encodes_like_its_twin() {
    use egui_display::{self as present, Encoding, Output, PresentPass};
    use wgpu::TextureFormat as F;
    let (device, queue) = device();
    let (w, h) = (4u32, 4u32);
    // (output, format, its encoding, one code of the format for the tolerance).
    let cases: [(Output, F, Encoding, f32); 7] = [
        (Output::Sdr8, F::Rgba8Unorm, Encoding::Unorm, 1.0 / 255.0),
        (
            Output::Sdr8,
            F::Rgba8UnormSrgb,
            Encoding::SrgbFormat,
            1.0 / 255.0,
        ),
        (
            Output::Sdr10,
            F::Rgb10a2Unorm,
            Encoding::Unorm,
            1.0 / 1023.0,
        ),
        (Output::Hdr10, F::Rgb10a2Unorm, Encoding::Pq, 1.0 / 1023.0),
        (Output::Hlg, F::Rgb10a2Unorm, Encoding::Hlg, 1.0 / 1023.0),
        (Output::Hlg, F::Rgba16Float, Encoding::Hlg, 0.0),
        (Output::Scrgb, F::Rgba16Float, Encoding::Scrgb, 0.0),
    ];
    let values = [
        -0.3f32,
        0.0,
        0.02,
        0.18,
        0.4613,
        0.73,
        1.0,
        1.6,
        3.5,
        f32::NAN,
    ];
    let levels = present::Target {
        hdr: true,
        white: 240.0,
        peak: 600.0,
    };
    for (output, format, encoding, code) in cases {
        assert_eq!(output.encoding(format), encoding);
        let mut pass = PresentPass::new(&device, format, output);
        pass.dither = false;
        let frame = target(&device, format, w, h);
        let frame_view = frame.create_view(&wgpu::TextureViewDescriptor::default());
        for v in values {
            // The canvas is f16: the twin gets the value the canvas holds.
            let canvas = [v, v * 0.5, v * 0.25].map(|c| Half::from_f32(c).to_f32());
            let canvas_view = pass.canvas(&device, w, h).clone();
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present-test.canvas"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &canvas_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(canvas[0]),
                            g: f64::from(canvas[1]),
                            b: f64::from(canvas[2]),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.draw(&queue, &mut enc, &frame_view, levels);
            queue.submit([enc.finish()]);
            let texel = if format == F::Rgba16Float { 8 } else { 4 };
            let bytes = read_texture(&device, &queue, &frame, w, h, texel);
            let got: [f32; 3] = match format {
                F::Rgba16Float => {
                    let f = halves(&bytes[..8]);
                    [f[0], f[1], f[2]]
                }
                F::Rgb10a2Unorm => {
                    let p = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    [p & 1023, (p >> 10) & 1023, (p >> 20) & 1023].map(|c| c as f32 / 1023.0)
                }
                _ => [bytes[0], bytes[1], bytes[2]].map(|c| f32::from(c) / 255.0),
            };
            // What the surface holds: an `*Srgb` format stores the encode of the
            // pass's (decoded) output, so compare the sRGB codes.
            let want = present::encode(canvas, encoding, levels);
            let want = if encoding == Encoding::SrgbFormat {
                want.map(egui_display::transfer::oetf)
            } else {
                want
            };
            for c in 0..3 {
                let tol = code.max(1e-3 + want[c].abs() / 1024.0) + 1e-4;
                assert!(
                    (got[c] - want[c]).abs() <= tol,
                    "{output:?} {format:?} canvas {canvas:?} channel {c}: got {} want {}",
                    got[c],
                    want[c]
                );
            }
        }
    }
}

#[test]
#[ignore = "GPU: needs an adapter"]
fn canvas_resize_and_output_switch_preserve_accounting() {
    let (device, queue) = device();
    let mut pass = egui_display::PresentPass::new(
        &device,
        wgpu::TextureFormat::Rgba8Unorm,
        egui_display::Output::Sdr8,
    );
    assert_eq!(pass.canvas_reserve(&device, 0, 0), 8);
    pass.prepare_canvas(&device, &queue, 0, 0);
    assert_eq!(pass.canvas_reserve(&device, 0, 0), 8);
    pass.prepare_canvas(&device, &queue, 17, 9);
    let view = pass.canvas(&device, 17, 9).clone();
    let same = pass.canvas(&device, 17, 9).clone();
    assert_eq!(view, same, "stable dimensions reuse the canvas");
    let reserve = pass.canvas_reserve(&device, 17, 9);
    assert!(reserve >= 17 * 9 * 8);
    let mut hdr = egui_display::PresentPass::new(
        &device,
        wgpu::TextureFormat::Rgba16Float,
        egui_display::Output::Scrgb,
    );
    hdr.inherit_canvas(&device, &mut pass);
    assert_eq!(
        hdr.canvas(&device, 17, 9),
        &view,
        "output switches reuse pixels and allocation"
    );
    assert_eq!(hdr.canvas_reserve(&device, 17, 9), reserve);
    hdr.prepare_canvas(&device, &queue, 23, 7);
    hdr.canvas(&device, 23, 7);
}

/// The present pass with a backdrop against its CPU twin (`encode_over`), on every
/// output: a canvas of a half-transparent premultiplied colour (one value above SDR
/// white) over each backdrop kind, dither off, read back per pixel. The checker is
/// pinned off the target's corner with 2-pixel cells, so both cells and their parity
/// are in every 4x4 read.
#[test]
#[ignore = "needs a GPU adapter"]
fn present_composites_the_backdrop_like_its_twin() {
    use egui_display::{
        Backdrop, BackdropKind, BackdropView, Checker, Encoding, Output, PresentPass, encode_over,
    };
    use wgpu::TextureFormat as F;
    let (device, queue) = device();
    let (w, h) = (4u32, 4u32);
    let cases: [(Output, F, Encoding, f32); 6] = [
        (Output::Sdr8, F::Rgba8Unorm, Encoding::Unorm, 1.0 / 255.0),
        (
            Output::Sdr8,
            F::Rgba8UnormSrgb,
            Encoding::SrgbFormat,
            1.0 / 255.0,
        ),
        (
            Output::Sdr10,
            F::Rgb10a2Unorm,
            Encoding::Unorm,
            1.0 / 1023.0,
        ),
        (Output::Hdr10, F::Rgb10a2Unorm, Encoding::Pq, 1.0 / 1023.0),
        (Output::Hlg, F::Rgb10a2Unorm, Encoding::Hlg, 1.0 / 1023.0),
        (Output::Scrgb, F::Rgba16Float, Encoding::Scrgb, 0.0),
    ];
    let levels = egui_display::Target {
        hdr: true,
        white: 240.0,
        peak: 600.0,
    };
    let backdrops = [
        Backdrop {
            checker: Checker {
                size_px: 2.0,
                ..Checker::default()
            },
            ..Backdrop::default()
        },
        Backdrop {
            kind: BackdropKind::Black,
            ..Backdrop::default()
        },
        Backdrop {
            kind: BackdropKind::Gray50,
            ..Backdrop::default()
        },
        Backdrop {
            kind: BackdropKind::Color,
            color: [0.9, 0.2, 0.6, 1.0],
            ..Backdrop::default()
        },
    ];
    // Canvases in the contract's representation (encoded associated light): a
    // half-transparent pixel (blue above SDR white), an emissive one (alpha 0, light
    // 2 / 0.5 / 0.05) and a tiny alpha. The twin and the shader must agree on all three.
    let enc = egui_display::transfer::oetf;
    let canvases = [
        [enc(0.7 * 0.5), enc(0.2 * 0.5), enc(1.3 * 0.5), 0.5f32],
        [enc(2.0), enc(0.5), enc(0.05), 0.0],
        [enc(2.0), enc(0.5), enc(0.05), 1e-4],
    ]
    .map(|c| c.map(|v| Half::from_f32(v).to_f32()));
    // The grid pinned at (1, 1) px with 2-pixel cells (the slider's minimum).
    let view = BackdropView::new(egui::pos2(1.0, 1.0), 1.0);
    for canvas in canvases {
        for (output, format, encoding, code) in cases {
            let mut pass = PresentPass::new(&device, format, output);
            pass.dither = false;
            let frame = target(&device, format, w, h);
            let frame_view = frame.create_view(&wgpu::TextureViewDescriptor::default());
            for backdrop in backdrops {
                // Cleared rect: the first three columns; column 3 stays outside it.
                let hole = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(3.0, 4.0));
                let uniform = backdrop.uniform(&view, hole);
                pass.backdrop = Some(uniform);
                let canvas_view = pass.canvas(&device, w, h).clone();
                let mut enc =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("backdrop-test.canvas"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &canvas_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: f64::from(canvas[0]),
                                g: f64::from(canvas[1]),
                                b: f64::from(canvas[2]),
                                a: f64::from(canvas[3]),
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.draw(&queue, &mut enc, &frame_view, levels);
                queue.submit([enc.finish()]);
                let texel = if format == F::Rgba16Float { 8 } else { 4 };
                let bytes = read_texture(&device, &queue, &frame, w, h, texel);
                let mut seen = std::collections::BTreeSet::new();
                for y in 0..h {
                    for x in 0..w {
                        let i = ((y * w + x) * texel) as usize;
                        let got: [f32; 3] = match format {
                            F::Rgba16Float => {
                                let f = halves(&bytes[i..i + 8]);
                                [f[0], f[1], f[2]]
                            }
                            F::Rgb10a2Unorm => {
                                let p = u32::from_le_bytes([
                                    bytes[i],
                                    bytes[i + 1],
                                    bytes[i + 2],
                                    bytes[i + 3],
                                ]);
                                [p & 1023, (p >> 10) & 1023, (p >> 20) & 1023]
                                    .map(|c| c as f32 / 1023.0)
                            }
                            _ => {
                                [bytes[i], bytes[i + 1], bytes[i + 2]].map(|c| f32::from(c) / 255.0)
                            }
                        };
                        let px = [x as f32 + 0.5, y as f32 + 0.5];
                        let want = encode_over(canvas, Some(&uniform), px, encoding, levels);
                        if x == 3 {
                            // Outside the cleared rect: no backdrop, whatever the alpha.
                            assert_eq!(want, encode_over(canvas, None, px, encoding, levels));
                        }
                        let want = if encoding == Encoding::SrgbFormat {
                            want.map(egui_display::transfer::oetf)
                        } else {
                            want
                        };
                        seen.insert(uniform.color_at(px)[0].to_bits());
                        for c in 0..3 {
                            let tol = code.max(1e-3 + want[c].abs() / 1024.0) + 1e-4;
                            assert!(
                                (got[c] - want[c]).abs() <= tol,
                                "{output:?} {format:?} {:?} pixel ({x}, {y}) channel {c}: got {} want {}",
                                backdrop.kind,
                                got[c],
                                want[c]
                            );
                        }
                    }
                }
                let cells = if backdrop.kind == BackdropKind::Checker {
                    2
                } else {
                    1
                };
                assert_eq!(seen.len(), cells, "{:?} pattern seen", backdrop.kind);
            }
            // No backdrop: the canvas presents as it is (the pre-backdrop behaviour).
            pass.backdrop = None;
            let canvas_rgb = [canvas[0], canvas[1], canvas[2]];
            let want = egui_display::encode(canvas_rgb, encoding, levels);
            let want_none = encode_over(canvas, None, [0.5, 0.5], encoding, levels);
            assert_eq!(want, want_none, "encode_over(None) is encode");
        }
    }
}

/// Draw a gradient into `canvas`: red ramps 0 -> 2 across x (past SDR white), green
/// ramps down y, blue 0.3; alpha 1 on the left half, 0.5 (premultiplied) on the right.
fn gradient(device: &wgpu::Device, queue: &wgpu::Queue, canvas: &wgpu::TextureView, w: u32) {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("capture-test.gradient"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{
    let x = (p.x - 0.5) / {last}.0;
    let a = select(1.0, 0.5, x > 0.5);
    return vec4<f32>(2.0 * x, 1.0 - (p.y - 0.5) * 0.25, 0.3, 1.0) * a;
}}",
                last = w - 1
            )
            .into(),
        ),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("capture-test.gradient"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: egui_display::CANVAS_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("capture-test.gradient"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: canvas,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
    }
    queue.submit([enc.finish()]);
}

/// A capture for every output equals, pixel for pixel, what a present pass configured
/// for that output writes to its swapchain format: a gradient past SDR white with a
/// half-transparent half over a checker backdrop, dither on. The window's pass is an
/// SDR one, so a capture for another output must take that output's encoding, not the
/// window's.
#[test]
#[ignore = "needs a GPU adapter"]
fn capture_equals_the_swapchain_signal_on_every_output() {
    use egui_display::screenshot::Pixels;
    use egui_display::{Backdrop, BackdropView, Output, PresentPass, capture_format};
    let (device, queue) = device();
    let (w, h) = (16u32, 4u32);
    let levels = egui_display::Target {
        hdr: true,
        white: 203.0,
        peak: 1000.0,
    };
    let backdrop = Backdrop::default().uniform(
        &BackdropView::new(egui::pos2(0.0, 0.0), 1.0),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w as f32, h as f32)),
    );
    let mut window = PresentPass::new(&device, wgpu::TextureFormat::Rgba8Unorm, Output::Sdr8);
    window.backdrop = Some(backdrop);
    let canvas = window.canvas(&device, w, h).clone();
    gradient(&device, &queue, &canvas, w);
    for output in Output::ALL {
        let capture = window
            .capture(&device, &queue, output, levels)
            .expect("capture")
            .wait(&device)
            .expect("readback");
        assert_eq!(
            (capture.output, capture.width, capture.height),
            (output, w, h)
        );
        assert_eq!((capture.white_nits, capture.peak_nits), (203.0, 1000.0));
        // The reference: a pass for this output on its swapchain format, same canvas.
        let format = capture_format(output);
        let mut reference = PresentPass::new(&device, format, output);
        reference.backdrop = Some(backdrop);
        reference.inherit_canvas(&device, &mut window);
        let frame = target(&device, format, w, h);
        let mut enc = device.create_command_encoder(&Default::default());
        reference.draw(
            &queue,
            &mut enc,
            &frame.create_view(&Default::default()),
            levels,
        );
        queue.submit([enc.finish()]);
        let texel = if format == wgpu::TextureFormat::Rgba16Float {
            8
        } else {
            4
        };
        let bytes = read_texture(&device, &queue, &frame, w, h, texel);
        let mut fresh = PresentPass::new(&device, wgpu::TextureFormat::Rgba8Unorm, Output::Sdr8);
        fresh.backdrop = Some(backdrop);
        fresh.inherit_canvas(&device, &mut reference);
        window = fresh;
        let want = match format {
            wgpu::TextureFormat::Rgba16Float => Pixels::RgbaF16(
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| Half::from_le_bytes(*b))
                    .collect(),
            ),
            wgpu::TextureFormat::Rgb10a2Unorm => {
                // Compare 10-bit codes: the capture's 16-bit samples carry the code in
                // their top 10 bits (widening is unit-tested in capture.rs).
                let codes: Vec<u32> = bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|b| {
                        let p = u32::from_le_bytes(*b);
                        [p & 1023, (p >> 10) & 1023, (p >> 20) & 1023]
                    })
                    .collect();
                let Pixels::Rgba16(px) = &capture.pixels else {
                    panic!("{output:?}: 16-bit pixels");
                };
                let px4 = px.as_chunks::<4>().0;
                let got: Vec<u32> = px4
                    .iter()
                    .flat_map(|p| [p[0], p[1], p[2]].map(|v| u32::from(v) >> 6))
                    .collect();
                assert_eq!(got, codes, "{output:?}");
                assert!(px4.iter().all(|p| p[3] == 65535), "opaque");
                continue;
            }
            _ => Pixels::Rgba8(bytes),
        };
        assert_eq!(capture.pixels, want, "{output:?}");
        // The gradient really reaches past SDR white on the HDR outputs.
        if let Pixels::RgbaF16(px) = &capture.pixels {
            assert!(
                px.iter().step_by(4).any(|r| r.to_f32() > 1.5),
                "scRGB above white"
            );
        }
    }
}

/// Writes a reference scene's screenshot for every output into `$EGUI_DISPLAY_SHOTS` (skips when
/// unset), for looking at: the top half ramps 0 -> 2 x SDR white in red, green and blue bands; the
/// bottom half is an orange image whose alpha ramps 0 -> 1 left to right over the default
/// checker backdrop.
#[test]
#[ignore = "needs a GPU adapter; writes files for a visual check"]
fn write_reference_screenshots() {
    use egui_display::{Backdrop, BackdropView, Output, PresentPass};
    let Some(dir) = std::env::var_os("EGUI_DISPLAY_SHOTS").map(std::path::PathBuf::from) else {
        eprintln!("EGUI_DISPLAY_SHOTS unset: nothing written");
        return;
    };
    std::fs::create_dir_all(&dir).expect("output dir");
    let (device, queue) = device();
    let (w, h) = (512u32, 256u32);
    let mut pass = PresentPass::new(&device, wgpu::TextureFormat::Rgba8Unorm, Output::Sdr8);
    pass.backdrop = Some(Backdrop::default().uniform(
        &BackdropView::new(egui::pos2(0.0, 0.0), 1.0),
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w as f32, h as f32)),
    ));
    let canvas = pass.canvas(&device, w, h).clone();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("reference-scene"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}}
fn oetf(v: f32) -> f32 {{
    if v <= 0.0031308 {{ return 12.92 * v; }}
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{
    let x = (p.x - 0.5) / {wl}.0;
    let y = (p.y - 0.5) / {hl}.0;
    if y < 0.5 {{
        let band = u32(y * 6.0);
        let v = oetf(2.0 * x);
        var c = vec3<f32>(v, 0.0, 0.0);
        if band == 1u {{ c = vec3<f32>(0.0, v, 0.0); }}
        if band == 2u {{ c = vec3<f32>(0.0, 0.0, v); }}
        return vec4<f32>(c, 1.0);
    }}
    let a = x;
    return vec4<f32>(vec3<f32>(oetf(0.9), oetf(0.35), oetf(0.05)) * a, a);
}}",
                wl = w - 1,
                hl = h - 1
            )
            .into(),
        ),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("reference-scene"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: egui_display::CANVAS_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("reference-scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &canvas,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(&pipeline);
        rp.draw(0..3, 0..1);
    }
    queue.submit([enc.finish()]);
    let levels = egui_display::Target {
        hdr: true,
        white: 203.0,
        peak: 1000.0,
    };
    for output in Output::ALL {
        let target = if output.is_hdr() {
            levels
        } else {
            egui_display::Target::default()
        };
        let capture = pass
            .capture(&device, &queue, output, target)
            .expect("capture")
            .wait(&device)
            .expect("readback");
        let name = format!("reference-{:?}", output).to_lowercase();
        // scRGB is half-float EXR, which hosts write; this crate only encodes PNG.
        if output == Output::Scrgb {
            continue;
        }
        let path = dir.join(name).with_extension("png");
        let file = std::io::BufWriter::new(std::fs::File::create(&path).expect("create"));
        capture.write_png(file).expect("write_png");
        eprintln!("wrote {}", path.display());
    }
}

/// Overlays drawn by egui over the backdrop's hole (viewport chrome, OFX interacts, HUD text)
/// go through egui-wgpu's real renderer: an opaque panel, `show_backdrop` clearing a rect, nothing
/// for the image there (fully transparent), and a 50%-alpha white overlay across half of the
/// hole and half of the panel. egui's blend leaves canvas alpha as "over" coverage, so the present
/// pass puts the backdrop under the overlay exactly once: overlay over 50% Gray in linear light.
/// The overlay outside the hole sits on the panel, alpha 1, untouched by the backdrop.
#[test]
#[ignore = "needs a GPU adapter"]
fn overlay_over_the_hole_shows_the_backdrop_once() {
    use egui_display::{Backdrop, BackdropKind, BackdropView, Output, PresentPass, transfer};
    let (device, queue) = device();
    let (w, h) = (16u32, 8u32);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w as f32, h as f32));
    let hole = egui::Rect::from_min_max(egui::pos2(4.0, 0.0), egui::pos2(12.0, 8.0));
    let overlay = egui::Rect::from_min_max(egui::pos2(8.0, 0.0), egui::pos2(16.0, 4.0));
    let panel_grey = egui::Color32::from_gray(64);
    let backdrop = Backdrop {
        kind: BackdropKind::Gray50,
        ..Backdrop::default()
    };
    let mut renderer = egui_wgpu::Renderer::new(
        &device,
        egui_display::CANVAS_FORMAT,
        egui_wgpu::RendererOptions {
            dithering: false,
            ..Default::default()
        },
    );
    // Two passes: the first pass's layout settles; the second is drawn. Every pass's texture
    // uploads (the font atlas) go to the renderer.
    let mut shapes = Vec::new();
    for _ in 0..2 {
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        });
        let painter = ctx.layer_painter(egui::LayerId::background());
        painter.rect_filled(screen, 0.0, panel_grey);
        egui_display::show_backdrop(
            &painter,
            hole,
            &backdrop,
            &BackdropView::new(egui::Pos2::ZERO, 1.0),
        );
        painter.rect_filled(overlay, 0.0, egui::Color32::from_white_alpha(128));
        let mut out = ctx.end_pass();
        for (id, deltas) in &out.textures_delta.set {
            for delta in deltas {
                renderer.update_texture(&device, &queue, *id, delta);
            }
        }
        out.textures_delta.clear();
        shapes = out.shapes;
    }
    let uniform = egui_display::take_backdrop(&ctx).expect("published");
    let jobs = ctx.tessellate(shapes, 1.0);
    let mut pass = PresentPass::new(&device, wgpu::TextureFormat::Rgba16Float, Output::Scrgb);
    pass.dither = false;
    pass.backdrop = Some(uniform);
    let canvas = pass.canvas(&device, w, h).clone();
    let screen_desc = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [w, h],
        pixels_per_point: 1.0,
    };
    let mut enc = device.create_command_encoder(&Default::default());
    let commands = renderer.update_buffers(&device, &queue, &mut enc, &jobs, &screen_desc);
    {
        let mut rp = enc
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("overlay-test.egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &canvas,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            })
            .forget_lifetime();
        renderer.render(&mut rp, &jobs, &screen_desc);
    }
    let frame = target(&device, wgpu::TextureFormat::Rgba16Float, w, h);
    // scRGB at white 80 nits: the output is linear light relative to SDR white.
    let levels = egui_display::Target {
        hdr: true,
        white: 80.0,
        peak: 1000.0,
    };
    pass.draw(
        &queue,
        &mut enc,
        &frame.create_view(&Default::default()),
        levels,
    );
    queue.submit(commands.into_iter().chain([enc.finish()]));
    let bytes = read_texture(&device, &queue, &frame, w, h, 8);
    let px = |x: u32, y: u32| {
        let i = ((y * w + x) * 8) as usize;
        halves(&bytes[i..i + 8])[0]
    };
    let grey = transfer::eotf(0.5);
    let panel = transfer::eotf(64.0 / 255.0);
    let half = 128.0 / 255.0;
    let close = |got: f32, want: f32, what: &str| {
        assert!((got - want).abs() < 4e-3, "{what}: got {got}, want {want}");
    };
    close(px(1, 6), panel, "panel, no overlay");
    close(px(6, 6), grey, "hole, nothing over it: the backdrop");
    // White at 50% coverage over the backdrop, the backdrop counted once. egui stored
    // `enc(S) * u`; the canvas contract reads it as encoded associated light, so the
    // overlay's light is eotf(0.5), not 0.5 (the documented residual of partial-alpha
    // light UI over a transparent canvas, see `backdrop.rs`).
    close(
        px(10, 2),
        transfer::eotf(half) + grey * (1.0 - half),
        "overlay over the hole",
    );
    close(px(10, 6), grey, "hole beside the overlay");
    // Over the opaque panel the overlay blends in egui's (encoded) space; no backdrop there.
    let on_panel = transfer::eotf(half + (64.0 / 255.0) * (1.0 - half));
    close(px(14, 2), on_panel, "overlay over the panel");
}
