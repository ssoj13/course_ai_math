//! HdrView through a real render pass: an 8-bit image's codes must land in the target as
//! the same display values, whether the target stores encoded values (egui-display's
//! float canvas, a `*Unorm` surface) or encodes on write (an `*Srgb` surface).

use std::sync::{Arc, Mutex};

use egui_hdr_view::{HdrFormat, HdrPaintCallback, HdrView, Mvp};
use egui_wgpu::CallbackTrait;

fn device() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("a GPU adapter (this test is #[ignore]d for machines without one)");
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device")
}

/// Draw a 2x2 RGBA8 image of `code` grey (opaque) over a 4x4 `format` target through
/// `HdrView` and read back the centre pixel's red channel, as stored.
fn centre(format: wgpu::TextureFormat, code: u8) -> Vec<u8> {
    let (device, queue) = device();
    let (w, h) = (4u32, 4u32);
    let mut view = HdrView::new();
    view.set_output_format(format);
    view.stage_frame(
        HdrFormat::Rgba8,
        Arc::new([code, code, code, 255].repeat(4)),
        2,
        2,
        Mvp::default(),
    );
    let callback = HdrPaintCallback {
        inner: Arc::new(Mutex::new(view)),
    };
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hdr-view-test"),
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
    });
    let mut resources = egui_wgpu::CallbackResources::new();
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [w, h],
        pixels_per_point: 1.0,
    };
    let mut encoder = device.create_command_encoder(&Default::default());
    callback.prepare(&device, &queue, &screen, &mut encoder, &mut resources);
    {
        let target_view = target.create_view(&Default::default());
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hdr-view-test"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
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
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(w as f32, h as f32));
        let info = egui::PaintCallbackInfo {
            viewport: rect,
            clip_rect: rect,
            pixels_per_point: 1.0,
            screen_size_px: [w, h],
        };
        callback.paint(info, &mut pass, &resources);
    }
    let texel = if format == wgpu::TextureFormat::Rgba16Float {
        8
    } else {
        4
    };
    let row = 256u32;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("hdr-view-test.readback"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
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
    queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.expect("map"));
    #[allow(
        clippy::disallowed_methods,
        reason = "test readback on a private device"
    )]
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("poll");
    let data = buffer.slice(..).get_mapped_range().expect("mapped");
    let at = (2 * row + 2 * texel) as usize;
    data[at..at + texel as usize].to_vec()
}

/// Mid grey (code 128) stays code 128: in the encoded float canvas it is 128/255, not
/// the sRGB decode 0.216 an `Rgba8UnormSrgb` texture sampled into it gave (two
/// stops too dark); on an `*Srgb` target the hardware re-encodes the decoded sample to
/// the same code.
#[test]
#[ignore = "needs a GPU adapter"]
fn eight_bit_mid_grey_keeps_its_code() {
    let canvas = centre(wgpu::TextureFormat::Rgba16Float, 128);
    let red = half::f16::from_le_bytes([canvas[0], canvas[1]]).to_f32();
    assert!(
        (red - 128.0 / 255.0).abs() < 2e-3,
        "float canvas got {red}, want {}",
        128.0 / 255.0
    );
    let unorm = centre(wgpu::TextureFormat::Rgba8Unorm, 128);
    assert_eq!(unorm[0], 128, "Unorm target");
    let srgb = centre(wgpu::TextureFormat::Rgba8UnormSrgb, 128);
    assert!(srgb[0].abs_diff(128) <= 1, "sRGB target got {}", srgb[0]);
}
