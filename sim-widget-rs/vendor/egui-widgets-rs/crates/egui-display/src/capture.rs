//! Capturing the presented frame: the canvas encoded by the present shader for any
//! [`Output`], into an offscreen texture of that output's format, read back. The single
//! screenshot path; [`crate::screenshot`] writes what it returns.
//!
//! Same shader, same parameters ([`PresentPass::params_for`]) as the swapchain draw, so
//! a capture holds exactly the signal the window would show on that output, backdrop
//! and dither included. The GPU work is submitted and waited for through `gpu_mem`
//! ([`gpu_mem::wait::submit`], [`gpu_mem::wait::map_read`]), never a blocking
//! `Device::poll` (see that module for why).

use crate::screenshot::{Capture, Pixels};
use crate::{Output, PresentPass, Target};

/// Why a capture failed.
#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    /// The pass has no canvas yet (nothing was drawn).
    #[error("no canvas to capture")]
    NoCanvas,
    #[error("readback: {0}")]
    Wait(#[from] gpu_mem::wait::WaitError),
    #[error("mapped range: {0}")]
    Range(String),
}

/// The surface format a capture for `output` is encoded into: the output's own
/// integer or float format (8-bit SDR in `Rgba8Unorm`, 10-bit SDR / PQ / HLG in
/// `Rgb10a2Unorm`, scRGB in `Rgba16Float`).
pub fn capture_format(output: Output) -> wgpu::TextureFormat {
    use wgpu::TextureFormat as F;
    match output {
        Output::Sdr8 => F::Rgba8Unorm,
        Output::Sdr10 | Output::Hdr10 | Output::Hlg => F::Rgb10a2Unorm,
        Output::Scrgb => F::Rgba16Float,
    }
}

/// A capture submitted to the GPU. [`Self::wait`] may run on another thread, so the
/// window's thread never waits for the readback.
#[must_use = "wait for it to get the pixels"]
pub struct PendingCapture {
    buffer: wgpu::Buffer,
    stride: u32,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    output: Output,
    target: Target,
    _submission: gpu_mem::wait::Submission,
}

impl PresentPass {
    /// Encode the current canvas for `output` at the levels of `target` into an
    /// offscreen texture of [`capture_format`] and submit its readback. Call it after
    /// the frame's canvas was drawn (and before the next frame draws into it).
    pub fn capture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        output: Output,
        target: Target,
    ) -> Result<PendingCapture, CaptureError> {
        let canvas = self.canvas.as_ref().ok_or(CaptureError::NoCanvas)?;
        let (width, height) = canvas.size;
        let format = capture_format(output);
        let pipeline = crate::present::make_pipeline(device, &self.module, &self.layout, format);
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("egui_display.capture.params"),
            size: std::mem::size_of::<crate::present::Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &params,
            0,
            bytemuck::bytes_of(&self.params_for(output, format, target)),
        );
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("egui_display.capture"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&canvas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("egui_display.capture"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let texel = texel_bytes(format);
        let stride = (width * texel).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("egui_display.capture.readback"),
            size: u64::from(stride) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("egui_display.capture"),
        });
        {
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_display.capture"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            extent,
        );
        let submission = gpu_mem::wait::submit(queue, "egui-display capture", [encoder.finish()]);
        Ok(PendingCapture {
            buffer,
            stride,
            width,
            height,
            format,
            output,
            target,
            _submission: submission,
        })
    }
}

impl PendingCapture {
    /// Block until the readback finished and return the pixels.
    pub fn wait(self, device: &wgpu::Device) -> Result<Capture, CaptureError> {
        let slice = self.buffer.slice(..);
        gpu_mem::wait::map_read(device, &slice)?;
        let pixels = {
            let bytes = slice
                .get_mapped_range()
                .map_err(|e| CaptureError::Range(e.to_string()))?;
            let row = (self.width * texel_bytes(self.format)) as usize;
            let rows = bytes
                .chunks(self.stride as usize)
                .take(self.height as usize)
                .flat_map(|r| r.get(..row).unwrap_or_default());
            decode(self.format, rows.copied())
        };
        self.buffer.unmap();
        Ok(Capture {
            output: self.output,
            width: self.width,
            height: self.height,
            white_nits: self.target.white,
            peak_nits: self.target.peak,
            pixels,
        })
    }
}

/// Bytes per texel of a capture format.
fn texel_bytes(format: wgpu::TextureFormat) -> u32 {
    match format {
        wgpu::TextureFormat::Rgba16Float => 8,
        _ => 4,
    }
}

/// The tightly packed texel bytes of a capture format as [`Pixels`].
fn decode(format: wgpu::TextureFormat, bytes: impl Iterator<Item = u8>) -> Pixels {
    let bytes: Vec<u8> = bytes.collect();
    match format {
        wgpu::TextureFormat::Rgba16Float => Pixels::RgbaF16(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| half::f16::from_le_bytes(*b))
                .collect(),
        ),
        wgpu::TextureFormat::Rgb10a2Unorm => Pixels::Rgba16(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|b| {
                    let p = u32::from_le_bytes(*b);
                    let widen = |v: u32| ((v << 6) | (v >> 4)) as u16;
                    [
                        widen(p & 1023),
                        widen((p >> 10) & 1023),
                        widen((p >> 20) & 1023),
                        ((p >> 30) * 0x5555) as u16,
                    ]
                })
                .collect(),
        ),
        _ => Pixels::Rgba8(bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 10-bit codes widen by bit replication: the ends land on the 16-bit ends, every
    /// code stays distinct and in order; 2-bit alpha 3 is opaque.
    #[test]
    fn ten_bit_codes_widen_exactly() {
        let pack =
            |r: u32, g: u32, b: u32, a: u32| (r | (g << 10) | (b << 20) | (a << 30)).to_le_bytes();
        let bytes: Vec<u8> = [pack(0, 1023, 512, 3), pack(1, 2, 1022, 0)].concat();
        let Pixels::Rgba16(px) = decode(wgpu::TextureFormat::Rgb10a2Unorm, bytes.into_iter())
        else {
            panic!("16-bit pixels");
        };
        assert_eq!(&px[..4], &[0, 65535, (512 << 6) | (512 >> 4), 65535]);
        assert_eq!(&px[4..], &[64, 128, (1022 << 6) | (1022 >> 4), 0]);
    }
}
