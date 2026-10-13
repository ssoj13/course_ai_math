// Passthrough textured quad for egui present.
//
// Colour management (exposure, ACES 2.0 display transform) lives UPSTREAM of this
// presenter (playa PLAN9.md W3). Do not reintroduce is_hdr / Narkowicz / Reinhard /
// gamma here — that was the double-encode and "EV dead on 8-bit" path.

struct VsUniforms {
    model: mat4x4<f32>,
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> vs_uni: VsUniforms;

// Binding 1 kept for layout stability; fields are unused (OT is upstream).
struct FsUniforms {
    exposure: f32,
    gamma: f32,
    is_hdr: u32,
    tonemap_mode: u32,
}

@group(0) @binding(1)
var<uniform> fs_uni: FsUniforms;

@group(0) @binding(2)
var img: texture_2d<f32>;
@group(0) @binding(3)
var img_smp: sampler;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv_in: vec2<f32>,
) -> VsOut {
    var out: VsOut;
    let world = vs_uni.model * vec4<f32>(position, 0.0, 1.0);
    out.clip_position = vs_uni.proj * vs_uni.view * world;
    out.uv = uv_in;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Touch fs_uni so the binding is not stripped; values are ignored.
    let _keep = fs_uni.exposure;
    return textureSample(img, img_smp, in.uv);
}
