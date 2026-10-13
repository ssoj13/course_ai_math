//! GPU histogram — zero-copy scatter from the linear source texture.
//!
//! Four-channel RGBA+luma histogram rendered via GPU compute (wgpu 29). The
//! `clear → scatter → reduce → draw` plumbing lives in [`super::scope`]; this file
//! is just the WGSL + the [`ScopeSpec`] that describes it.

use super::scope::{ScopePanel, ScopeSpec};
use super::{GpuPanel, PanelCtx};

const BINS_COUNT: u32 = 1025;
const BINS_BYTES: u64 = BINS_COUNT as u64 * 4;

const CLEAR_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> bins: array<atomic<u32>>;
@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= 1025u) { return; }
    atomicStore(&bins[gid.x], 0u);
}
"#;

const SCATTER_WGSL: &str = r#"
struct Params { src_w: u32, src_h: u32, out_w: u32, out_h: u32 }
@group(0) @binding(0) var               src_tex: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> bins: array<atomic<u32>>;
@group(0) @binding(2) var<uniform>      params: Params;
@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.src_w || gid.y >= params.src_h) { return; }
    let px = textureLoad(src_tex, vec2i(i32(gid.x), i32(gid.y)), 0);
    let r = clamp(px.r, 0.0, 1.0);
    let g = clamp(px.g, 0.0, 1.0);
    let b = clamp(px.b, 0.0, 1.0);
    let l = clamp(0.2126 * r + 0.7152 * g + 0.0722 * b, 0.0, 1.0);
    atomicAdd(&bins[       u32(r * 255.0)], 1u);
    atomicAdd(&bins[256u + u32(g * 255.0)], 1u);
    atomicAdd(&bins[512u + u32(b * 255.0)], 1u);
    atomicAdd(&bins[768u + u32(l * 255.0)], 1u);
}
"#;

const REDUCE_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> bins: array<atomic<u32>>;
@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= 256u) { return; }
    let m = max(max(
        atomicLoad(&bins[gid.x]),
        atomicLoad(&bins[256u + gid.x])),
        atomicLoad(&bins[512u + gid.x]));
    atomicMax(&bins[1024u], m);
}
"#;

const DRAW_WGSL: &str = r#"
struct Params { src_w: u32, src_h: u32, out_w: u32, out_h: u32 }
@group(0) @binding(0) var<storage, read> bins: array<u32>;
@group(0) @binding(1) var<uniform>      params: Params;
@group(0) @binding(2) var               out_tex: texture_storage_2d<rgba16float, write>;
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.out_w || gid.y >= params.out_h) { return; }
    let bin  = min(u32((f32(gid.x) / f32(params.out_w)) * 256.0), 255u);
    let mx   = f32(max(bins[1024u], 1u));
    let frac = 1.0 - f32(gid.y) / f32(params.out_h);
    let rh   = f32(bins[       bin]) / mx;
    let gh   = f32(bins[256u + bin]) / mx;
    let bh   = f32(bins[512u + bin]) / mx;
    let bg   = vec3f(30.0 / 255.0);
    let a    = 150.0 / 255.0;
    var c    = bg;
    if (frac < rh) { c = mix(c, vec3f(1.0, 0.0, 0.0),                a); }
    if (frac < gh) { c = mix(c, vec3f(0.0, 1.0, 0.0),                a); }
    if (frac < bh) { c = mix(c, vec3f(80.0/255.0, 120.0/255.0, 1.0), a); }
    textureStore(out_tex, vec2i(i32(gid.x), i32(gid.y)), vec4f(c, 1.0));
}
"#;

/// Bins layout: 4 channels × 256 + 1 global-max slot; clear covers all 1025
/// (`ceil(1025 / 64) = 17`), reduce covers the 256 per-channel bins (`ceil(256 / 64) = 4`).
const HISTOGRAM_SPEC: ScopeSpec = ScopeSpec {
    label: "hist",
    bins_bytes: BINS_BYTES,
    clear_wgsl: CLEAR_WGSL,
    scatter_wgsl: SCATTER_WGSL,
    reduce_wgsl: Some(REDUCE_WGSL),
    draw_wgsl: DRAW_WGSL,
    clear_groups: 17,
    reduce_groups: 4,
};

/// Every WGSL pass of this scope, for the crate's headless naga-validation test.
#[cfg(test)]
pub(crate) const WGSL_SOURCES: &[(&str, &str)] = &[
    ("hist_clear", CLEAR_WGSL),
    ("hist_scatter", SCATTER_WGSL),
    ("hist_reduce", REDUCE_WGSL),
    ("hist_draw", DRAW_WGSL),
];

/// GPU RGBA + luma histogram panel. A thin newtype over the shared [`ScopePanel`].
pub struct GpuHistogramPanel(ScopePanel);

impl Default for GpuHistogramPanel {
    fn default() -> Self {
        Self(ScopePanel::new(HISTOGRAM_SPEC))
    }
}

impl GpuPanel for GpuHistogramPanel {
    fn tick(
        &mut self,
        ctx: &mut PanelCtx,
        src: &wgpu::TextureView,
        src_w: u32,
        src_h: u32,
        out_w: u32,
        out_h: u32,
    ) {
        self.0.tick(ctx, src, src_w, src_h, out_w, out_h);
    }

    fn release(&mut self, renderer: &mut egui_wgpu::Renderer) {
        self.0.release(renderer);
    }

    fn texture_id(&self) -> Option<egui::TextureId> {
        self.0.texture_id()
    }

    fn physical_bucket_size(&self) -> (u32, u32) {
        self.0.physical_bucket_size()
    }
}
