//! Shared multi-pass compute scope: `clear → scatter → [reduce] → draw`.
//!
//! The histogram, waveform and vectorscope are the SAME GPU shape — an atomic
//! `bins` storage buffer scattered from the linear source texture, optionally
//! reduced (to find a max for normalisation), then drawn into the panel's output
//! texture. They differ ONLY in their WGSL, the bins-buffer size, whether a reduce
//! pass exists, and the clear/reduce dispatch counts. This module captures the
//! common `Pipes` + init + `tick` once, so each scope crate-file is reduced to a
//! [`ScopeSpec`] const + a thin newtype (see `histogram.rs` / `waveform.rs` /
//! `vectorscope.rs`). Previously each of the three hand-rolled ~150 identical lines.

use super::pipe::{
    PanelOutput, PanelParams, bins_bg, bins_only_bgl, create_compute_pipeline, draw_bg, draw_bgl,
    scatter_bg, scatter_bgl, write_panel_params,
};
use super::{BucketSizer, PanelCtx, make_bins_buffer, make_params_buffer, sync_egui_texture};

/// Static description of one scope's compute passes. The WGSL, bins size and
/// dispatch counts are the only things that vary between scopes.
pub(crate) struct ScopeSpec {
    /// Short label prefix used for GPU object labels (e.g. `"hist"`).
    pub label: &'static str,
    /// Size of the atomic `bins` storage buffer, in bytes.
    pub bins_bytes: u64,
    /// Clear pass: zero the bins buffer.
    pub clear_wgsl: &'static str,
    /// Scatter pass: accumulate per-pixel contributions into the bins.
    pub scatter_wgsl: &'static str,
    /// Optional reduce pass (e.g. compute a column/global max for normalisation).
    /// `None` for scopes that draw straight from the raw bins (vectorscope).
    pub reduce_wgsl: Option<&'static str>,
    /// Draw pass: render the bins into the output texture.
    pub draw_wgsl: &'static str,
    /// Workgroup count for the 1-D clear pass (`ceil(bin_slots / 64)`-ish).
    pub clear_groups: u32,
    /// Workgroup count for the 1-D reduce pass (ignored when `reduce_wgsl` is `None`).
    pub reduce_groups: u32,
}

/// Live GPU objects for a scope. `reduce_pipeline` is `None` when the spec has no
/// reduce pass; everything else is common to all three scopes.
struct ScopePipes {
    clear_pipeline: wgpu::ComputePipeline,
    scatter_pipeline: wgpu::ComputePipeline,
    reduce_pipeline: Option<wgpu::ComputePipeline>,
    draw_pipeline: wgpu::ComputePipeline,
    scatter_bgl: wgpu::BindGroupLayout,
    draw_bgl: wgpu::BindGroupLayout,
    bins_buf: wgpu::Buffer,
    params_buf: wgpu::Buffer,
    bins_bg: wgpu::BindGroup,
    output: Option<PanelOutput>,
    draw_bg: Option<wgpu::BindGroup>,
    bucket: BucketSizer,
}

/// Build all GPU objects for `spec`. Lazily called on the first tick (and after a
/// [`ScopePanel::release`]), mirroring the pre-dedup per-scope `init_pipelines`.
fn init_pipes(spec: &ScopeSpec, device: &wgpu::Device) -> ScopePipes {
    let l = spec.label;
    let bins_bgl = bins_only_bgl(device, &format!("{l}_bins_bgl"));
    let scatter_bgl = scatter_bgl(device, &format!("{l}_scatter_bgl"));
    let draw_bgl = draw_bgl(device, &format!("{l}_draw_bgl"));

    let bins_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("{l}_bins_layout")),
        bind_group_layouts: &[Some(&bins_bgl)],
        immediate_size: 0,
    });
    let scatter_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("{l}_scatter_layout")),
        bind_group_layouts: &[Some(&scatter_bgl)],
        immediate_size: 0,
    });
    let draw_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("{l}_draw_layout")),
        bind_group_layouts: &[Some(&draw_bgl)],
        immediate_size: 0,
    });

    let bins_buf = make_bins_buffer(device, &format!("{l}_bins"), spec.bins_bytes);
    let params_buf = make_params_buffer(device);
    let bins_bg = bins_bg(device, &bins_bgl, &format!("{l}_bins_bg"), &bins_buf);

    ScopePipes {
        clear_pipeline: create_compute_pipeline(
            device,
            &format!("{l}_clear"),
            &bins_layout,
            spec.clear_wgsl,
        ),
        scatter_pipeline: create_compute_pipeline(
            device,
            &format!("{l}_scatter"),
            &scatter_layout,
            spec.scatter_wgsl,
        ),
        reduce_pipeline: spec
            .reduce_wgsl
            .map(|src| create_compute_pipeline(device, &format!("{l}_reduce"), &bins_layout, src)),
        draw_pipeline: create_compute_pipeline(
            device,
            &format!("{l}_draw"),
            &draw_layout,
            spec.draw_wgsl,
        ),
        scatter_bgl,
        draw_bgl,
        bins_buf,
        params_buf,
        bins_bg,
        output: None,
        draw_bg: None,
        bucket: BucketSizer::default(),
    }
}

/// A generic GPU scope panel driven by a [`ScopeSpec`]. The public scope types
/// (`GpuHistogramPanel`, …) are thin newtypes that delegate their [`super::GpuPanel`]
/// impl to one of these.
pub(crate) struct ScopePanel {
    spec: ScopeSpec,
    pipes: Option<ScopePipes>,
    texture_id: Option<egui::TextureId>,
}

impl ScopePanel {
    /// A panel for `spec` with no GPU objects yet (built lazily on the first tick).
    pub fn new(spec: ScopeSpec) -> Self {
        Self {
            spec,
            pipes: None,
            texture_id: None,
        }
    }

    /// (Re)build the pipelines lazily and (re)allocate the output texture + its
    /// draw bind group whenever the panel's bucket size changes.
    fn ensure_output(&mut self, device: &wgpu::Device, logical_w: u32, logical_h: u32) {
        if self.pipes.is_none() {
            self.pipes = Some(init_pipes(&self.spec, device));
        }
        let l = self.spec.label;
        let p = self.pipes.as_mut().expect("pipes just initialised");
        let ((bucket_w, bucket_h), reallocated) = p.bucket.update(logical_w, logical_h);
        if reallocated || p.output.is_none() {
            let output = PanelOutput::alloc(device, &format!("{l}_out"), bucket_w, bucket_h);
            p.draw_bg = Some(draw_bg(
                device,
                &p.draw_bgl,
                &format!("{l}_draw_bg"),
                &p.bins_buf,
                &p.params_buf,
                &output.view,
            ));
            p.output = Some(output);
        }
    }

    /// One frame: `clear → scatter → [reduce] → draw`, then register/update the
    /// egui texture. Degenerate sizes early-out (never panics).
    pub fn tick(
        &mut self,
        ctx: &mut PanelCtx,
        src: &wgpu::TextureView,
        src_w: u32,
        src_h: u32,
        out_w: u32,
        out_h: u32,
    ) {
        if src_w == 0 || src_h == 0 || out_w == 0 || out_h == 0 {
            return;
        }
        self.ensure_output(ctx.device, out_w, out_h);
        let l = self.spec.label;
        let clear_groups = self.spec.clear_groups;
        let reduce_groups = self.spec.reduce_groups;
        let Some(p) = self.pipes.as_mut() else {
            return;
        };
        let Some(out) = p.output.as_ref() else {
            return;
        };
        let Some(draw_bg) = p.draw_bg.as_ref() else {
            return;
        };

        write_panel_params(
            ctx.queue,
            &p.params_buf,
            PanelParams {
                src_w,
                src_h,
                out_w,
                out_h,
            },
        );
        let scatter_bg = scatter_bg(
            ctx.device,
            &p.scatter_bgl,
            &format!("{l}_scatter_bg"),
            src,
            &p.bins_buf,
            &p.params_buf,
        );

        let mut enc = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some(&format!("{l}_enc")),
            });
        {
            let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(l),
                timestamp_writes: None,
            });
            cp.set_pipeline(&p.clear_pipeline);
            cp.set_bind_group(0, &p.bins_bg, &[]);
            cp.dispatch_workgroups(clear_groups, 1, 1);

            cp.set_pipeline(&p.scatter_pipeline);
            cp.set_bind_group(0, &scatter_bg, &[]);
            cp.dispatch_workgroups(src_w.div_ceil(16), src_h.div_ceil(16), 1);

            if let Some(reduce) = p.reduce_pipeline.as_ref() {
                cp.set_pipeline(reduce);
                cp.set_bind_group(0, &p.bins_bg, &[]);
                cp.dispatch_workgroups(reduce_groups, 1, 1);
            }

            cp.set_pipeline(&p.draw_pipeline);
            cp.set_bind_group(0, draw_bg, &[]);
            cp.dispatch_workgroups(out_w.div_ceil(8), out_h.div_ceil(8), 1);
        }
        ctx.queue.submit(std::iter::once(enc.finish()));
        sync_egui_texture(ctx, &out.view, &mut self.texture_id);
    }

    /// Free the egui texture + drop the pipelines/output (rebuilt lazily next tick).
    pub fn release(&mut self, renderer: &mut egui_wgpu::Renderer) {
        if let Some(id) = self.texture_id.take() {
            renderer.free_texture(&id);
        }
        self.pipes = None;
    }

    /// egui handle to the output texture (None until the first successful tick).
    pub fn texture_id(&self) -> Option<egui::TextureId> {
        self.texture_id
    }

    /// Physical bucket size of the output texture (for the UV crop in `show_panel`).
    pub fn physical_bucket_size(&self) -> (u32, u32) {
        self.pipes
            .as_ref()
            .map(|p| p.bucket.physical())
            .unwrap_or((0, 0))
    }
}
