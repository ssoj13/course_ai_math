//! GPU RGB parade — zero-copy scatter from the linear source texture.
//!
//! The broadcast RGB parade is three single-channel waveforms laid side by side:
//! the panel is split into equal left/middle/right thirds showing the R, G and B
//! column distributions respectively. It accumulates into the EXACT same bins as
//! [`super::waveform`] (3 channels × 256 columns × 256 value-rows + per-column
//! maxes), so it reuses that scope's `clear` / `scatter` / `reduce` passes verbatim
//! (see `waveform.rs`) and differs ONLY in this draw pass — the two scopes cannot
//! drift. Rendered via GPU compute (wgpu 30); no CPU readback.

use super::scope::{ScopePanel, ScopeSpec};
use super::waveform::{BINS_BYTES, CLEAR_WGSL, REDUCE_WGSL, SCATTER_WGSL};
use super::{GpuPanel, PanelCtx};

/// Draw pass: for each output pixel, pick which third (channel) it falls in, remap
/// the local x to a source column, and paint that channel's normalised trace tinted
/// by the channel colour. A faint separator marks each third boundary so the three
/// waveforms read as the standard parade layout.
const DRAW_WGSL: &str = r#"
struct Params { src_w: u32, src_h: u32, out_w: u32, out_h: u32 }
@group(0) @binding(0) var<storage, read> bins: array<u32>;
@group(0) @binding(1) var<uniform>      params: Params;
@group(0) @binding(2) var               out_tex: texture_storage_2d<rgba16float, write>;
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.out_w || gid.y >= params.out_h) { return; }
    let bg  = vec3f(25.0 / 255.0);
    // Split the panel into three equal horizontal thirds: R | G | B.
    let third_w = max(params.out_w / 3u, 1u);
    let third   = min(gid.x / third_w, 2u);
    let local_x = gid.x - third * third_w;
    // Faint separator line at each third boundary (skip the very first column).
    if (local_x == 0u && third > 0u) {
        textureStore(out_tex, vec2i(i32(gid.x), i32(gid.y)), vec4f(vec3f(60.0 / 255.0), 1.0));
        return;
    }
    // Local column within the third → source column 0..255.
    let col  = min(u32((f32(local_x) / f32(third_w)) * 256.0), 255u);
    let frac = 1.0 - f32(gid.y) / f32(params.out_h);
    let row  = min(u32(frac * 256.0), 255u);
    // Bins are laid out R | G | B, each 65536 slots (256 cols × 256 rows), followed
    // by per-column maxes at 196608 + channel*256. These offsets MUST match the ones
    // hardcoded in waveform.rs's DRAW_WGSL (same buffer) — change both in lockstep.
    let ch_base  = third * 65536u;
    let max_base = 196608u + third * 256u;
    let cnt = bins[ch_base + col * 256u + row];
    let mx  = max(bins[max_base + col], 1u);
    let v   = clamp(f32(cnt) / f32(mx), 0.0, 1.0);
    var tint = vec3f(1.0, 0.25, 0.25);
    if (third == 1u)      { tint = vec3f(0.25, 1.0, 0.25); }
    else if (third == 2u) { tint = vec3f(0.35, 0.55, 1.0); }
    let thr = 8.0 / 255.0;
    if (v > thr) {
        let a   = 130.0 / 255.0;
        let rgb = mix(bg, tint * v, a);
        textureStore(out_tex, vec2i(i32(gid.x), i32(gid.y)), vec4f(rgb, 1.0));
    } else {
        textureStore(out_tex, vec2i(i32(gid.x), i32(gid.y)), vec4f(bg, 1.0));
    }
}
"#;

/// Same bins + dispatch counts as the waveform (`ceil(197376 / 256) = 771` clear
/// workgroups, `ceil(256 / 64) = 4` reduce workgroups); only the draw pass differs.
const PARADE_SPEC: ScopeSpec = ScopeSpec {
    label: "parade",
    bins_bytes: BINS_BYTES,
    clear_wgsl: CLEAR_WGSL,
    scatter_wgsl: SCATTER_WGSL,
    reduce_wgsl: Some(REDUCE_WGSL),
    draw_wgsl: DRAW_WGSL,
    clear_groups: 771,
    reduce_groups: 4,
};

/// Every WGSL pass of this scope, for the crate's headless naga-validation test.
/// The clear/scatter/reduce passes are covered by the waveform's own list; only the
/// parade-specific draw is unique here.
#[cfg(test)]
pub(crate) const WGSL_SOURCES: &[(&str, &str)] = &[("parade_draw", DRAW_WGSL)];

/// GPU RGB parade panel. A thin newtype over the shared [`ScopePanel`].
pub struct GpuParadePanel(ScopePanel);

impl Default for GpuParadePanel {
    fn default() -> Self {
        Self(ScopePanel::new(PARADE_SPEC))
    }
}

impl GpuPanel for GpuParadePanel {
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
