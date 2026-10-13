// Per-node image preview: a texture-mapped quad drawn on a Live2D node's body content rect.
// Shares the camera/graph-plane uniform (group 0) with the SDF shape pass; group 1 carries the
// node's RGBA8 texture + sampler. Geometry mirrors `shape.wgsl` (unit quad -> body AABB -> rotate
// about the node pivot -> graph-plane -> reversed-Z clip with a per-instance depth bias).

struct Uniforms {
    view_proj: mat4x4<f32>,
    plane_origin: vec4<f32>,
    plane_axis_u: vec4<f32>,
    plane_axis_v: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var img_tex: texture_2d<f32>;
@group(1) @binding(1) var img_smp: sampler;

fn uv_to_world(uv: vec2<f32>) -> vec3<f32> {
    // Match GraphPlane::graph_to_world (+u right, +v down): both tangents negated.
    return u.plane_origin.xyz - u.plane_axis_u.xyz * uv.x - u.plane_axis_v.xyz * uv.y;
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) tex: vec2<f32>,
}

@vertex
fn vs_main(
    @location(0) corner: vec2<f32>,
    @location(1) aabb_min: vec2<f32>,
    @location(2) aabb_max: vec2<f32>,
    @location(3) pivot: vec2<f32>,
    @location(4) rotation: f32,
    @location(5) depth_bias: f32,
) -> VsOut {
    var o: VsOut;
    let uv = mix(aabb_min, aabb_max, corner);
    let cs = cos(rotation);
    let sn = sin(rotation);
    let rel = uv - pivot;
    let uv_draw = pivot + vec2<f32>(rel.x * cs - rel.y * sn, rel.x * sn + rel.y * cs);
    o.clip = u.view_proj * vec4<f32>(uv_to_world(uv_draw), 1.0);
    // Reversed-Z distance-independent depth bias (see shape.wgsl): ADD so a larger bias draws
    // nearer. The caller passes the node's Title-layer depth so the image sits above its body fill.
    o.clip.z = clamp(o.clip.z + depth_bias * o.clip.w, 0.0, o.clip.w);
    // `corner` is the unit [0,1] quad: corner.(0,0) = aabb_min = body top-left = texel row 0 (top),
    // which matches the RGBA8 buffer's [h, w, 4] top-first row order — no vertical flip needed.
    o.tex = corner;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(img_tex, img_smp, in.tex);
    if (c.a < 1.0 / 255.0) {
        discard;
    }
    return c;
}
