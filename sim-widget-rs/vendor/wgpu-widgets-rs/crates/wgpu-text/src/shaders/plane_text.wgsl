// Plane-locked text: local glyph quads → surface_xfm → view_proj, sample mask/color atlas.

struct Uniforms {
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

@group(1) @binding(0) var color_atlas_texture: texture_2d<f32>;
@group(1) @binding(1) var mask_atlas_texture: texture_2d<f32>;
@group(1) @binding(2) var atlas_sampler: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) content_type: u32,
    @location(3) local_pos: vec2<f32>,
    @location(4) @interpolate(flat) clip_rect: vec4<f32>,
};

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

@vertex
fn vs_main(
    @builtin(vertex_index) corner: u32,
    @location(0) sc0: vec4<f32>,
    @location(1) sc1: vec4<f32>,
    @location(2) sc2: vec4<f32>,
    @location(3) sc3: vec4<f32>,
    @location(4) local_rect: vec4<f32>,
    @location(5) uv_rect: vec4<f32>,
    @location(6) color: vec4<f32>,
    @location(7) content_type: u32,
    @location(8) clip_rect: vec4<f32>,
) -> VsOut {
    let surface_xfm = mat4x4<f32>(sc0, sc1, sc2, sc3);
    let c = vec2<f32>(f32(corner & 1u), f32((corner >> 1u) & 1u));
    let local_pos = local_rect.xy + c * local_rect.zw;
    let world = surface_xfm * vec4<f32>(local_pos, 0.0, 1.0);

    var o: VsOut;
    o.clip = u.view_proj * world;
    o.uv = mix(uv_rect.xy, uv_rect.zw, c);
    o.color = color;
    o.content_type = content_type;
    o.local_pos = local_pos;
    o.clip_rect = clip_rect;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Sample BOTH atlases with automatic LOD *before* any discard/branch so the
    // fetch happens in uniform control flow. `textureSample` needs screen-space
    // derivatives of `uv`, which are only well-defined when every fragment of the
    // 2x2 quad reaches the call — a `discard` or a branch on the flat-interpolated
    // `content_type` first would make the flow non-uniform and the derivatives
    // (hence the mip/anisotropy LOD selection) undefined. Sampling both is cheap
    // and lets mips + anisotropy engage on minified/oblique text. On the Linear/
    // Nearest paths the atlas has a single mip level, so auto-LOD collapses to
    // level 0 and the result is bit-identical to the old `textureSampleLevel(..,0)`.
    let color_texel = textureSample(color_atlas_texture, atlas_sampler, in.uv);
    let mask_a = textureSample(mask_atlas_texture, atlas_sampler, in.uv).x;

    // Rectangular clip in label-local space, applied after the (uniform) fetch.
    if in.local_pos.x < in.clip_rect.x || in.local_pos.x > in.clip_rect.z
        || in.local_pos.y < in.clip_rect.y || in.local_pos.y > in.clip_rect.w {
        discard;
    }

    switch in.content_type {
        case 0u: {
            return color_texel;
        }
        case 1u: {
            return vec4<f32>(in.color.rgb, in.color.a * mask_a);
        }
        default: {
            return vec4<f32>(0.0);
        }
    }
}
