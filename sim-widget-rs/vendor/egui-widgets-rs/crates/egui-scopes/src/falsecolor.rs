//! GPU false-colour exposure map — zero-copy per-pixel LUT over the source.
//!
//! Unlike the histogram / waveform / vectorscope (which SCATTER the source into an
//! accumulation buffer), false colour is a straight per-pixel map: each source texel
//! is recoloured by its exposure and written to an output texture at SOURCE
//! resolution, then scaled into the panel by egui. It therefore does not use the
//! shared `ScopePanel` bins machinery — it is a single compute pass reading the
//! linear `wgpu::TextureView` directly (no CPU readback), rendered via wgpu 30.
//!
//! ## Convention — ARRI ALEXA false colour
//! The exposure bands follow the ARRI ALEXA false-colour chart, whose stops are
//! defined on the CAMERA VIDEO SIGNAL (IRE, 0..100), not on scene-linear light. The
//! source here is scene-linear, so the luma is first taken through the BT.709
//! (Rec.709) camera OETF to that signal before the stops are applied — a host feeding
//! already display-encoded pixels would double-encode, but the crate's contract is a
//! linear source (matching the other three scopes). The transfer MUST be BT.709, not
//! sRGB: ARRI's chart is defined on the camera signal, so 18% middle grey lands at
//! 40.9 IRE (inside the green band) and skin at ~+0.6..+0.8 stop; the sRGB OETF would
//! put mid-grey at 46.1 IRE — outside green — turning the tool's key reference wrong.
//! Stops:
//!
//! | signal (IRE) | colour  | meaning                              |
//! |--------------|---------|--------------------------------------|
//! | `< 2.5`      | purple  | black clipping (crushed shadows)     |
//! | `2.5 – 4`    | blue    | just above black                     |
//! | `38 – 42`    | green   | 18% middle grey                      |
//! | `52 – 56`    | pink    | skin-tone key (~+1 stop over grey)   |
//! | `97 – 99`    | yellow  | one stop below white clipping        |
//! | `> 99`       | red     | white clipping                       |
//! | otherwise    | grey    | the monochrome signal (image detail) |

use super::pipe::{PanelOutput, PanelParams, create_compute_pipeline, write_panel_params};
use super::{
    GpuPanel, PanelCtx, make_params_buffer, storage_tex_write_entry, sync_egui_texture,
    texture_2d_entry, uniform_entry,
};

const WGSL: &str = r#"
struct Params { src_w: u32, src_h: u32, out_w: u32, out_h: u32 }
@group(0) @binding(0) var               src_tex: texture_2d<f32>;
@group(0) @binding(1) var<uniform>      params: Params;
@group(0) @binding(2) var               out_tex: texture_storage_2d<rgba16float, write>;

// BT.709 (Rec.709) camera OETF: scene-linear → video signal (0..1). The ARRI
// false-colour stops are defined on THIS camera signal, NOT sRGB — with sRGB, 18%
// grey lands at 46.1 IRE (outside the 38–42 green band) while BT.709 puts it at 40.9.
// (Mirrored in the `bt709_oetf` test helper below; keep the two in sync.)
fn bt709_oetf(c: f32) -> f32 {
    let x = clamp(c, 0.0, 1.0);
    if (x < 0.018) { return 4.5 * x; }
    return 1.099 * pow(x, 0.45) - 0.099;
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.src_w || gid.y >= params.src_h) { return; }
    let px = textureLoad(src_tex, vec2i(i32(gid.x), i32(gid.y)), 0);
    // Rec.709 luma on the linear source, taken to the camera signal → IRE (0..100).
    let luma   = 0.2126 * px.r + 0.7152 * px.g + 0.0722 * px.b;
    let signal = bt709_oetf(luma);
    let ire    = signal * 100.0;
    // Neutral fallback = the monochrome signal, so image detail stays readable
    // between the marked exposure bands.
    var c = vec3f(signal);
    // ARRI ALEXA false-colour stops (order matters: clip checks last, high before low).
    if (ire < 2.5)                       { c = vec3f(0.40, 0.00, 0.60); } // purple: black clipping
    else if (ire < 4.0)                  { c = vec3f(0.00, 0.40, 1.00); } // blue:   just above black
    else if (ire >= 38.0 && ire <= 42.0) { c = vec3f(0.00, 1.00, 0.00); } // green:  18% middle grey
    else if (ire >= 52.0 && ire <= 56.0) { c = vec3f(1.00, 0.40, 0.80); } // pink:   skin-tone key
    else if (ire > 99.0)                 { c = vec3f(1.00, 0.00, 0.00); } // red:    white clipping
    else if (ire > 97.0)                 { c = vec3f(1.00, 1.00, 0.00); } // yellow: one stop below clip
    textureStore(out_tex, vec2i(i32(gid.x), i32(gid.y)), vec4f(c, 1.0));
}
"#;

/// Every WGSL pass of this scope, for the crate's headless naga-validation test.
#[cfg(test)]
pub(crate) const WGSL_SOURCES: &[(&str, &str)] = &[("falsecolor", WGSL)];

/// Lazily-built GPU objects for the false-colour pass.
struct FcPipes {
    pipeline: wgpu::ComputePipeline,
    bgl: wgpu::BindGroupLayout,
    params_buf: wgpu::Buffer,
}

fn init_pipes(device: &wgpu::Device) -> FcPipes {
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fc_bgl"),
        entries: &[
            texture_2d_entry(0),
            uniform_entry(1),
            storage_tex_write_entry(2),
        ],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fc_layout"),
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let pipeline = create_compute_pipeline(device, "fc", &layout, WGSL);
    let params_buf = make_params_buffer(device);
    FcPipes {
        pipeline,
        bgl,
        params_buf,
    }
}

/// GPU false-colour exposure-map panel. Renders at source resolution; egui scales
/// the result into the panel rect (full-texture UV, so no bucket crop is reported).
#[derive(Default)]
pub struct GpuFalseColorPanel {
    pipes: Option<FcPipes>,
    output: Option<PanelOutput>,
    /// Source size the output texture was last allocated for; a change forces realloc.
    dims: (u32, u32),
    texture_id: Option<egui::TextureId>,
}

impl GpuPanel for GpuFalseColorPanel {
    fn tick(
        &mut self,
        ctx: &mut PanelCtx,
        src: &wgpu::TextureView,
        src_w: u32,
        src_h: u32,
        _out_w: u32,
        _out_h: u32,
    ) {
        if src_w == 0 || src_h == 0 {
            return;
        }
        if self.pipes.is_none() {
            self.pipes = Some(init_pipes(ctx.device));
        }
        // (Re)allocate the output texture whenever the source resolution changes.
        if self.output.is_none() || self.dims != (src_w, src_h) {
            self.output = Some(PanelOutput::alloc(ctx.device, "fc_out", src_w, src_h));
            self.dims = (src_w, src_h);
        }
        let Some(p) = self.pipes.as_ref() else {
            return;
        };
        let Some(out) = self.output.as_ref() else {
            return;
        };

        write_panel_params(
            ctx.queue,
            &p.params_buf,
            PanelParams {
                src_w,
                src_h,
                out_w: src_w,
                out_h: src_h,
            },
        );
        let bg = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fc_bg"),
            layout: &p.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(src),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: p.params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&out.view),
                },
            ],
        });

        let mut enc = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fc_enc"),
            });
        {
            let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fc"),
                timestamp_writes: None,
            });
            cp.set_pipeline(&p.pipeline);
            cp.set_bind_group(0, &bg, &[]);
            cp.dispatch_workgroups(src_w.div_ceil(8), src_h.div_ceil(8), 1);
        }
        ctx.queue.submit(std::iter::once(enc.finish()));
        sync_egui_texture(ctx, &out.view, &mut self.texture_id);
    }

    fn release(&mut self, renderer: &mut egui_wgpu::Renderer) {
        if let Some(id) = self.texture_id.take() {
            renderer.free_texture(&id);
        }
        self.pipes = None;
        self.output = None;
        self.dims = (0, 0);
    }

    fn texture_id(&self) -> Option<egui::TextureId> {
        self.texture_id
    }
}

#[cfg(test)]
mod tests {
    //! Pins the ARRI false-colour convention numerically, without a GPU: the OETF and
    //! band chain here MIRROR the WGSL in `WGSL` above (the shader is the runtime
    //! source of truth; keep the two in sync). The point of the test is to catch the
    //! sRGB-vs-BT.709 class of error — mid-grey MUST land in green, which it does not
    //! under the sRGB OETF.

    /// BT.709 (Rec.709) camera OETF — scene-linear → video signal (0..1).
    fn bt709_oetf(x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        if x < 0.018 {
            4.5 * x
        } else {
            1.099 * x.powf(0.45) - 0.099
        }
    }

    /// Scene-linear luma → signal in IRE (0..100).
    fn ire(luma: f32) -> f32 {
        bt709_oetf(luma) * 100.0
    }

    #[derive(Debug, PartialEq)]
    enum Band {
        Purple,
        Blue,
        Green,
        Pink,
        Yellow,
        Red,
        Grey,
    }

    /// Mirrors the band chain in the WGSL `main` (same thresholds, same order).
    fn band(ire: f32) -> Band {
        if ire < 2.5 {
            Band::Purple
        } else if ire < 4.0 {
            Band::Blue
        } else if (38.0..=42.0).contains(&ire) {
            Band::Green
        } else if (52.0..=56.0).contains(&ire) {
            Band::Pink
        } else if ire > 99.0 {
            Band::Red
        } else if ire > 97.0 {
            Band::Yellow
        } else {
            Band::Grey
        }
    }

    #[test]
    fn mid_grey_is_green_and_clipped_white_is_red() {
        // 18% scene-linear middle grey must fall in the GREEN band. BT.709 → 40.9 IRE
        // (inside 38–42); the sRGB OETF would give 46.1 IRE → Grey, so reverting the
        // OETF fails this assertion (mutation check).
        let mid_grey = ire(0.18);
        assert!(
            (40.0..=41.5).contains(&mid_grey),
            "mid-grey IRE = {mid_grey}, expected ~40.9"
        );
        assert_eq!(band(mid_grey), Band::Green);
        // Clipped white → RED, crushed black → PURPLE.
        assert_eq!(band(ire(1.0)), Band::Red);
        assert_eq!(band(ire(0.0)), Band::Purple);
    }
}
