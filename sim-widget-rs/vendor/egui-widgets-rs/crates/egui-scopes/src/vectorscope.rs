//! GPU vectorscope — zero-copy scatter from the linear source texture.
//!
//! YUV vectorscope rendered via GPU compute. Depends on `wgpu` directly.
//! The naga fix for dynamic indexing of `let`-bound arrays in the draw
//! shader (using `var` instead of `let` for the marker table) is documented
//! in the WGSL below.
//!
//! # One chroma transform
//! The scatter pass, the dot readout in the draw pass and the R/Y/G/C/B/M graticule markers all use
//! the SAME BT.601-style transform (`u = (B-Y)*0.565` right, `v = (R-Y)*0.713` up), spliced into both
//! shaders from one WGSL prelude (`chroma_wgsl!`). The markers used to sit at HSV hue angles
//! (0°, 60°, …) that this plane never produces (pure red lands at ~109°, not 0°), and the draw pass
//! read the bins vertically flipped relative to the scatter.

use super::scope::{ScopePanel, ScopeSpec};
use super::{GpuPanel, PanelCtx};

/// `count[65536]`, `sum_r`, `sum_g`, `sum_b` — each 65536 u32.
const BINS_COUNT: u32 = 65_536 * 4;
const BINS_BYTES: u64 = BINS_COUNT as u64 * 4;

/// WGSL module-scope constants of the chroma transform: luma weights, chroma scales
/// (`u = (B-Y)*U_SCALE`, `v = (R-Y)*V_SCALE`), bins per unit chroma (256 bins span ±0.625 around 128)
/// and the draw span (chroma radius 1.0 maps to `DRAW_SPAN * R` px, so 100% bars land near 0.85 R).
/// A macro rather than a `const` so `concat!` can splice it into the `&'static str` shaders; the
/// test `wgsl_prelude_matches_rust_consts` pins it to the Rust twins used by the geometry tests.
macro_rules! chroma_wgsl {
    () => {
        "const KR: f32 = 0.299;\nconst KG: f32 = 0.587;\nconst KB: f32 = 0.114;\n\
         const U_SCALE: f32 = 0.565;\nconst V_SCALE: f32 = 0.713;\n\
         const BIN_SCALE: f32 = 204.8;\nconst DRAW_SPAN: f32 = 1.6;\n"
    };
}

const CLEAR_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> bins: array<atomic<u32>>;
@compute @workgroup_size(256, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= 262144u) { return; }
    atomicStore(&bins[gid.x], 0u);
}
"#;

const SCATTER_WGSL: &str = concat!(
    chroma_wgsl!(),
    r#"
struct Params { src_w: u32, src_h: u32, out_w: u32, out_h: u32 }
@group(0) @binding(0) var               src_tex: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> bins: array<atomic<u32>>;
@group(0) @binding(2) var<uniform>      params: Params;
@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.src_w || gid.y >= params.src_h) { return; }
    let px = textureLoad(src_tex, vec2i(i32(gid.x), i32(gid.y)), 0);
    let r  = clamp(px.r, 0.0, 1.0);
    let g  = clamp(px.g, 0.0, 1.0);
    let b  = clamp(px.b, 0.0, 1.0);
    let Y  = KR * r + KG * g + KB * b;
    let u  = (b - Y) * U_SCALE;
    let v  = (r - Y) * V_SCALE;
    let bx = i32(round(128.0 + u * BIN_SCALE));
    let by = i32(round(128.0 - v * BIN_SCALE));
    if (bx < 0 || bx >= 256 || by < 0 || by >= 256) { return; }
    let idx = u32(by) * 256u + u32(bx);
    atomicAdd(&bins[idx], 1u);
    atomicAdd(&bins[65536u + idx], u32(r * 255.0));
    atomicAdd(&bins[131072u + idx], u32(g * 255.0));
    atomicAdd(&bins[196608u + idx], u32(b * 255.0));
}
"#
);

const DRAW_WGSL: &str = concat!(
    chroma_wgsl!(),
    r#"
struct Params { src_w: u32, src_h: u32, out_w: u32, out_h: u32 }
@group(0) @binding(0) var<storage, read> bins: array<u32>;
@group(0) @binding(1) var<uniform>       params: Params;
@group(0) @binding(2) var                out_tex: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.out_w || gid.y >= params.out_h) { return; }
    let side = f32(min(params.out_w, params.out_h));
    let cx   = f32(params.out_w) * 0.5;
    let cy   = f32(params.out_h) * 0.5;
    let R    = side * 0.5 - 2.0;
    let px   = f32(gid.x) + 0.5;
    let py   = f32(gid.y) + 0.5;
    let dx   = px - cx;
    let dy   = py - cy;
    let dist = sqrt(dx * dx + dy * dy);

    var color = vec3f(20.0 / 255.0);
    if (abs(dist - R) < 1.5 || abs(dist - R * 0.5) < 1.0) {
        color = vec3f(60.0 / 255.0);
    }
    if (dist <= R * 0.5 && abs(dist - R * 0.5) >= 1.0) {
        color = vec3f(40.0 / 255.0);
    }

    if (dist <= R) {
        // Screen-up (dy < 0) is +v, matching the scatter's `by = 128 - v * BIN_SCALE`.
        let bx_f = 128.0 + (dx / (R * DRAW_SPAN)) * BIN_SCALE;
        let by_f = 128.0 + (dy / (R * DRAW_SPAN)) * BIN_SCALE;
        let bx   = i32(round(bx_f));
        let by   = i32(round(by_f));
        if (bx >= 0 && bx < 256 && by >= 0 && by < 256) {
            let idx = u32(by) * 256u + u32(bx);
            let count = bins[idx];
            if (count > 0u) {
                let inv = 1.0 / f32(count);
                // LIMIT: the sum_{r,g,b} bins accumulate value*255 per pixel as a u32. For a
                // near-uniform image whose pixels collapse into one chroma bin the sum is ~w*h*255,
                // which exceeds u32::MAX once w*h > ~16.8M px (beyond 4K); the atomic then wraps and
                // sum/count is garbage. The clamp bounds the dot colour to [0,1]; it is an exact no-op
                // without overflow (sum/count/255 == the per-channel mean, already in [0,1]).
                let dot_rgb = clamp(
                    vec3f(
                        f32(bins[65536u + idx]) * inv / 255.0,
                        f32(bins[131072u + idx]) * inv / 255.0,
                        f32(bins[196608u + idx]) * inv / 255.0,
                    ),
                    vec3f(0.0),
                    vec3f(1.0),
                );
                let pixel_count = params.src_w * params.src_h;
                let step = max(1u, pixel_count / 10000u);
                let log_max = log2(1.0 + f32(pixel_count / step) * 0.005);
                let intensity = clamp(log2(1.0 + f32(count)) / log_max, 0.0, 1.0);
                let a = 80.0 / 255.0 * intensity;
                color = mix(color, dot_rgb, a);
            }
        }
    }

    // Graticule markers at the chroma point of each 100% primary/secondary, from the SAME transform
    // as the scatter. naga forbids dynamic indexing of a `let`-bound (immutable, non-addressable)
    // array, and the loop below indexes by the runtime counter `i`, so the table must be an
    // addressable `var` or the module fails validation and the panel renders blank.
    var mcolors = array<vec3f, 6>(
        vec3f(1.0, 0.0, 0.0), vec3f(1.0, 1.0, 0.0), vec3f(0.0, 1.0, 0.0),
        vec3f(0.0, 1.0, 1.0), vec3f(0.0, 0.0, 1.0), vec3f(1.0, 0.0, 1.0),
    );
    for (var i = 0; i < 6; i = i + 1) {
        let c   = mcolors[i];
        let my_y = KR * c.r + KG * c.g + KB * c.b;
        let mx  = cx + (c.b - my_y) * U_SCALE * R * DRAW_SPAN;
        let my  = cy - (c.r - my_y) * V_SCALE * R * DRAW_SPAN;
        let ddx = px - mx;
        let ddy = py - my;
        if (ddx * ddx + ddy * ddy < 9.0) {
            color = c;
        }
    }

    textureStore(out_tex, vec2i(i32(gid.x), i32(gid.y)), vec4f(color, 1.0));
}
"#
);

/// Bins: `count` + `sum_r/g/b`, each 65536 u32 = 262144 slots; clear covers all
/// (`ceil(262144 / 256) = 1024`). No reduce pass — the draw normalises from the
/// raw counts directly.
const VECTORSCOPE_SPEC: ScopeSpec = ScopeSpec {
    label: "vs",
    bins_bytes: BINS_BYTES,
    clear_wgsl: CLEAR_WGSL,
    scatter_wgsl: SCATTER_WGSL,
    reduce_wgsl: None,
    draw_wgsl: DRAW_WGSL,
    clear_groups: 1024,
    reduce_groups: 0,
};

/// Every WGSL pass of this scope, for the crate's headless naga-validation test.
#[cfg(test)]
pub(crate) const WGSL_SOURCES: &[(&str, &str)] = &[
    ("vs_clear", CLEAR_WGSL),
    ("vs_scatter", SCATTER_WGSL),
    ("vs_draw", DRAW_WGSL),
];

/// GPU YUV vectorscope panel. A thin newtype over the shared [`ScopePanel`].
pub struct GpuVectorscopePanel(ScopePanel);

impl Default for GpuVectorscopePanel {
    fn default() -> Self {
        Self(ScopePanel::new(VECTORSCOPE_SPEC))
    }
}

impl GpuPanel for GpuVectorscopePanel {
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

#[cfg(test)]
mod tests {
    /// Rust twins of the `chroma_wgsl!` prelude (the shaders use the WGSL copy).
    const KR: f32 = 0.299;
    const KG: f32 = 0.587;
    const KB: f32 = 0.114;
    const U_SCALE: f32 = 0.565;
    const V_SCALE: f32 = 0.713;
    const BIN_SCALE: f32 = 204.8;
    const DRAW_SPAN: f32 = 1.6;

    /// Chroma `(u, v)` of an RGB triple — the scatter pass's transform (u right, v up).
    fn chroma_uv(rgb: [f32; 3]) -> (f32, f32) {
        let y = KR * rgb[0] + KG * rgb[1] + KB * rgb[2];
        ((rgb[2] - y) * U_SCALE, (rgb[0] - y) * V_SCALE)
    }

    /// Angle of a chroma point in degrees, counter-clockwise from +u (right), `v` up.
    fn angle(rgb: [f32; 3]) -> f32 {
        let (u, v) = chroma_uv(rgb);
        v.atan2(u).to_degrees().rem_euclid(360.0)
    }

    /// The WGSL prelude and the Rust twins cannot drift.
    #[test]
    fn wgsl_prelude_matches_rust_consts() {
        let want = format!(
            "const KR: f32 = {KR:?};\nconst KG: f32 = {KG:?};\nconst KB: f32 = {KB:?};\n\
             const U_SCALE: f32 = {U_SCALE:?};\nconst V_SCALE: f32 = {V_SCALE:?};\n\
             const BIN_SCALE: f32 = {BIN_SCALE:?};\nconst DRAW_SPAN: f32 = {DRAW_SPAN:?};\n"
        );
        assert_eq!(chroma_wgsl!(), want);
    }

    /// The graticule markers must sit where the scatter puts pure colours — NOT at HSV hue angles.
    #[test]
    fn marker_angles_follow_the_chroma_transform() {
        let cases = [
            ([1.0, 0.0, 0.0], 108.675), // R: up-left
            ([1.0, 1.0, 0.0], 170.777), // Y: left
            ([0.0, 1.0, 0.0], 231.606), // G: down-left
            ([0.0, 1.0, 1.0], 288.675), // C: down-right
            ([0.0, 0.0, 1.0], 350.777), // B: right
            ([1.0, 0.0, 1.0], 51.606),  // M: up-right
        ];
        for (rgb, want) in cases {
            let got = angle(rgb);
            assert!((got - want).abs() < 0.01, "{rgb:?}: {got} vs {want}");
        }
        // Neutral greys have no chroma: the centre of the scope.
        let (u, v) = chroma_uv([0.5, 0.5, 0.5]);
        assert!(u.abs() < 1e-6 && v.abs() < 1e-6);
    }

    /// 100% primaries/secondaries land at 0.81..0.86 of the scope radius (the old marker radius 0.85),
    /// and inside the 256-bin plane the scatter can address.
    #[test]
    fn marker_radius_is_near_the_graticule_ring() {
        for rgb in [
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
        ] {
            let (u, v) = chroma_uv(rgb);
            let r = u.hypot(v) * DRAW_SPAN;
            assert!((0.80..0.86).contains(&r), "{rgb:?}: {r}");
            assert!(
                u.abs().max(v.abs()) * BIN_SCALE < 128.0,
                "{rgb:?}: outside the bin plane"
            );
        }
    }
}
