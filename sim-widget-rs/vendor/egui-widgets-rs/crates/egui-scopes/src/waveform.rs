//! GPU waveform — zero-copy scatter from the linear source texture.
//!
//! Per-column RGB waveform rendered via GPU compute (wgpu 29).
//! Depends on `wgpu` directly (same version as egui_wgpu).

use super::scope::{ScopePanel, ScopeSpec};
use super::{GpuPanel, PanelCtx};

const BINS_COUNT: u32 = 197_376;
pub(crate) const BINS_BYTES: u64 = BINS_COUNT as u64 * 4;

// The clear / scatter / reduce passes are `pub(crate)` because the RGB parade
// (`parade.rs`) accumulates into the SAME bins layout (3 channels × 256 cols × 256
// rows + per-column maxes) and differs from the waveform ONLY in its draw pass. It
// reuses these verbatim so the two scopes cannot drift.
pub(crate) const CLEAR_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> bins: array<atomic<u32>>;
@compute @workgroup_size(256, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= 197376u) { return; }
    atomicStore(&bins[gid.x], 0u);
}
"#;

pub(crate) const SCATTER_WGSL: &str = r#"
struct Params { src_w: u32, src_h: u32, out_w: u32, out_h: u32 }
@group(0) @binding(0) var               src_tex: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> bins: array<atomic<u32>>;
@group(0) @binding(2) var<uniform>      params: Params;
@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.src_w || gid.y >= params.src_h) { return; }
    let px = textureLoad(src_tex, vec2i(i32(gid.x), i32(gid.y)), 0);
    let col = min(u32((f32(gid.x) / f32(params.src_w)) * 256.0), 255u);
    let r = clamp(px.r, 0.0, 1.0);
    let g = clamp(px.g, 0.0, 1.0);
    let b = clamp(px.b, 0.0, 1.0);
    let rr = u32(r * 255.0);
    let gr = u32(g * 255.0);
    let br = u32(b * 255.0);
    atomicAdd(&bins[          col * 256u + rr], 1u);
    atomicAdd(&bins[65536u  + col * 256u + gr], 1u);
    atomicAdd(&bins[131072u + col * 256u + br], 1u);
}
"#;

pub(crate) const REDUCE_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> bins: array<atomic<u32>>;
@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= 256u) { return; }
    let col = gid.x;
    var mr: u32 = 1u;
    var mg: u32 = 1u;
    var mb: u32 = 1u;
    for (var row: u32 = 0u; row < 256u; row = row + 1u) {
        mr = max(mr, atomicLoad(&bins[          col * 256u + row]));
        mg = max(mg, atomicLoad(&bins[65536u  + col * 256u + row]));
        mb = max(mb, atomicLoad(&bins[131072u + col * 256u + row]));
    }
    atomicStore(&bins[196608u + col], mr);
    atomicStore(&bins[196864u + col], mg);
    atomicStore(&bins[197120u + col], mb);
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
    let col  = min(u32((f32(gid.x) / f32(params.out_w)) * 256.0), 255u);
    let frac = 1.0 - f32(gid.y) / f32(params.out_h);
    let row  = min(u32(frac * 256.0), 255u);
    let rr   = bins[          col * 256u + row];
    let gr   = bins[65536u  + col * 256u + row];
    let br   = bins[131072u + col * 256u + row];
    let mr   = max(bins[196608u + col], 1u);
    let mg   = max(bins[196864u + col], 1u);
    let mb   = max(bins[197120u + col], 1u);
    let r    = f32(rr) / f32(mr);
    let g    = f32(gr) / f32(mg);
    let b    = f32(br) / f32(mb);
    let bg   = vec3f(25.0 / 255.0);
    let thr  = 8.0 / 255.0;
    if (r > thr || g > thr || b > thr) {
        let a   = 120.0 / 255.0;
        let rgb = mix(bg, vec3f(r, g, b), a);
        textureStore(out_tex, vec2i(i32(gid.x), i32(gid.y)), vec4f(rgb, 1.0));
    } else {
        textureStore(out_tex, vec2i(i32(gid.x), i32(gid.y)), vec4f(bg, 1.0));
    }
}
"#;

/// Bins: 3 channels × 256 cols × 256 rows + 3×256 per-column maxes; clear covers
/// all 197376 (`ceil(197376 / 256) = 771`), reduce runs one workgroup per 64 cols
/// (`ceil(256 / 64) = 4`).
const WAVEFORM_SPEC: ScopeSpec = ScopeSpec {
    label: "wf",
    bins_bytes: BINS_BYTES,
    clear_wgsl: CLEAR_WGSL,
    scatter_wgsl: SCATTER_WGSL,
    reduce_wgsl: Some(REDUCE_WGSL),
    draw_wgsl: DRAW_WGSL,
    clear_groups: 771,
    reduce_groups: 4,
};

/// Every WGSL pass of this scope, for the crate's headless naga-validation test
/// (wgpu only validates at pipeline creation, so a shader typo would otherwise pass
/// `cargo build` and only fail at runtime on a real device).
#[cfg(test)]
pub(crate) const WGSL_SOURCES: &[(&str, &str)] = &[
    ("wf_clear", CLEAR_WGSL),
    ("wf_scatter", SCATTER_WGSL),
    ("wf_reduce", REDUCE_WGSL),
    ("wf_draw", DRAW_WGSL),
];

/// GPU per-column RGB waveform panel. A thin newtype over the shared [`ScopePanel`].
pub struct GpuWaveformPanel(ScopePanel);

impl Default for GpuWaveformPanel {
    fn default() -> Self {
        Self(ScopePanel::new(WAVEFORM_SPEC))
    }
}

impl GpuPanel for GpuWaveformPanel {
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
