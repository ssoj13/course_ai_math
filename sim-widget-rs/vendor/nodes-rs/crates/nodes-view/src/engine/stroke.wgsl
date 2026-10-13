struct Uniforms {
    view_proj: mat4x4<f32>,
    viewport: vec2<f32>,
    _pad: vec2<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
}

// Segment-stroke vertex.
// We extrude a quad around the segment in *screen space* so thickness stays stable under zoom.
struct VsIn {
    @location(0) p: vec2<f32>,
    @location(1) a: vec2<f32>,
    @location(2) b: vec2<f32>,
    @location(3) side: f32,
    @location(4) width_px: f32,
    @location(5) color: vec4<f32>,
}

fn clip_xy(v: vec4<f32>) -> vec2<f32> {
    return v.xy / v.w;
}

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var o: VsOut;

    let ca = u.view_proj * vec4<f32>(input.a, 0.0, 1.0);
    let cb = u.view_proj * vec4<f32>(input.b, 0.0, 1.0);
    let cp = u.view_proj * vec4<f32>(input.p, 0.0, 1.0);

    // Direction in NDC (Y up). If degenerate, skip offset.
    let da = clip_xy(ca);
    let db = clip_xy(cb);
    let d = db - da;
    let len = length(d);

    var offset_ndc = vec2<f32>(0.0, 0.0);
    if (len > 1e-6) {
        let dir = d / len;
        let n = vec2<f32>(-dir.y, dir.x);
        // Convert pixel width to NDC: 2px / viewport.
        let px_to_ndc = vec2<f32>(2.0 / u.viewport.x, 2.0 / u.viewport.y);
        offset_ndc = n * (input.side * input.width_px) * px_to_ndc;
    }

    // Apply offset in clip space (scale by w).
    var pos = cp;
    pos.xy = pos.xy + offset_ndc * pos.w;

    o.pos = pos;
    o.color = input.color;
    return o;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    return input.color;
}
