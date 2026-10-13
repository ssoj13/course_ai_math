// World-space line segments (gizmo axes). view_proj only.

struct Uniforms {
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(
    @location(0) in_pos: vec3<f32>,
    @location(1) in_color: vec4<f32>,
) -> VsOut {
    var o: VsOut;
    o.pos = u.view_proj * vec4<f32>(in_pos, 1.0);
    o.color = in_color;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
