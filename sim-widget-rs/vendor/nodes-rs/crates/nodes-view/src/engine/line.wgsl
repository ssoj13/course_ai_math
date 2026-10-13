struct Uniforms {
    view_proj: mat4x4<f32>,
    plane_origin: vec4<f32>,
    plane_axis_u: vec4<f32>,
    plane_axis_v: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

fn uv_to_world(uv: vec2<f32>) -> vec3<f32> {
    // Match GraphPlane::graph_to_world: +u right, +v screen-down (both tangents negated).
    return u.plane_origin.xyz - u.plane_axis_u.xyz * uv.x - u.plane_axis_v.xyz * uv.y;
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(
    @location(0) in_pos: vec2<f32>,
    @location(1) in_color: vec4<f32>,
) -> VsOut {
    var o: VsOut;
    let world3 = uv_to_world(in_pos);
    o.pos = u.view_proj * vec4<f32>(world3, 1.0);
    o.color = in_color;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
