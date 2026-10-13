struct Uniforms {
    view_proj: mat4x4<f32>,
    plane_origin: vec4<f32>,
    plane_axis_u: vec4<f32>,
    plane_axis_v: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

fn uv_to_world(uv: vec2<f32>) -> vec3<f32> {
    // Match GraphPlane::graph_to_world: +u right, +v down (both tangents negated).
    return u.plane_origin.xyz - u.plane_axis_u.xyz * uv.x - u.plane_axis_v.xyz * uv.y;
}

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) @interpolate(flat) kind: u32,
    @location(2) @interpolate(flat) sides: u32,
    @location(3) @interpolate(flat) p0: vec4<f32>,
    @location(4) @interpolate(flat) p1: vec4<f32>,
    @location(5) @interpolate(flat) fill: vec4<f32>,
    @location(6) @interpolate(flat) stroke: vec4<f32>,
    @location(7) @interpolate(flat) pick_id: u32,
}

@vertex
fn vs_main(
    @location(0) corner: vec2<f32>,
    @location(1) aabb_min: vec2<f32>,
    @location(2) aabb_max: vec2<f32>,
    @location(3) kind: u32,
    @location(4) sides: u32,
    @location(5) p0: vec4<f32>,
    @location(6) p1: vec4<f32>,
    @location(7) fill: vec4<f32>,
    @location(8) stroke: vec4<f32>,
    @location(9) pick_id: u32,
    @location(10) pivot: vec2<f32>,
    @location(11) rotation: f32,
) -> VsOut {
    var o: VsOut;
    let uv = mix(aabb_min, aabb_max, corner);
    // Rotate the DRAWN position about the node pivot; the SDF stays evaluated in the shape's
    // LOCAL (unrotated) frame via `o.world = uv`, so a rounded-rect body becomes an oriented
    // rounded-rect without touching the SDF. `rotation == 0` leaves `uv` unchanged.
    let cs = cos(rotation);
    let sn = sin(rotation);
    let rel = uv - pivot;
    let uv_draw = pivot + vec2<f32>(rel.x * cs - rel.y * sn, rel.x * sn + rel.y * cs);
    let world3 = uv_to_world(uv_draw);
    o.clip = u.view_proj * vec4<f32>(world3, 1.0);
    // Depth-only draw-rank bias (no screen parallax). Under the reversed-Z infinite-far
    // projection `clip.z` is constant (= near) for every fragment, so a raw clip-Z offset
    // would be distance-dependent after the perspective divide. Scaling the bias by `clip.w`
    // makes it a distance-INDEPENDENT NDC offset: `ndc_z = (near + bias*w)/w = near/w + bias`.
    // ADD (not subtract) because reversed-Z puts nearer geometry at LARGER NDC z, and a
    // bigger `p1.z` (depth_bias) means "draw nearer the camera" (front). Clamp to [0, w] so a
    // large per-node bias can never push a node past a clip plane and vanish.
    o.clip.z = clamp(o.clip.z + p1.z * o.clip.w, 0.0, o.clip.w);
    o.world = uv;
    o.kind = kind;
    o.sides = sides;
    o.p0 = p0;
    o.p1 = p1;
    o.fill = fill;
    o.stroke = stroke;
    o.pick_id = pick_id;
    return o;
}

fn sd_circle(p: vec2<f32>, c: vec2<f32>, r: f32) -> f32 {
    return length(p - c) - r;
}

fn sd_rounded_box(p: vec2<f32>, bmin: vec2<f32>, bmax: vec2<f32>, r: f32) -> f32 {
    let c = 0.5 * (bmin + bmax);
    let h = 0.5 * (bmax - bmin);
    let q = abs(p - c) - h + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn sd_segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}

fn sd_regular_polygon(p: vec2<f32>, c: vec2<f32>, r: f32, sides: u32, rot: f32) -> f32 {
    let n = max(f32(sides), 3.0);
    let an = 6.2831853 / n;
    var d = sd_circle(p, c, r);
    let rel = p - c;
    let ang = atan2(rel.y, rel.x) - rot;
    let sector = floor(ang / an);
    let a0 = rot + sector * an;
    let a1 = a0 + an;
    let v0 = vec2<f32>(cos(a0), sin(a0)) * r;
    let v1 = vec2<f32>(cos(a1), sin(a1)) * r;
    d = min(d, sd_segment(rel, vec2<f32>(0.0), v0));
    d = min(d, sd_segment(rel, vec2<f32>(0.0), v1));
    return d;
}

// Signed distance to the instance's shape at the fragment's graph-UV position. Shared by the
// color, id, and pick-only paths so all three test identical geometry.
fn shape_sdf(in: VsOut) -> f32 {
    if (in.kind == 0u) {
        return sd_rounded_box(in.world, in.p0.xy, in.p0.zw, in.p1.x);
    } else if (in.kind == 1u) {
        return sd_circle(in.world, in.p0.xy, in.p0.z);
    } else if (in.kind == 2u) {
        return sd_regular_polygon(in.world, in.p0.xy, in.p0.z, in.sides, in.p0.w);
    } else {
        // Wire capsule: p0.xy = a, p0.zw = b, p1.x = half_width (graph UV).
        return sd_segment(in.world, in.p0.xy, in.p0.zw) - in.p1.x;
    }
}

// Shape "inradius" — the largest antialiasing half-width that still leaves a solid interior. The
// SDF AA band (`aa`) is `fwidth(d)` = how fast the signed distance changes per screen pixel; at
// grazing camera angles a foreshortened shape changes `d` by MANY graph-UV units per pixel, so
// `fwidth(d)` explodes and `smoothstep(-aa, aa, d)` washes the whole fill out toward 0.5 — the
// node's far half "fades away" (looked like clipping). Capping `aa` at the inradius keeps the AA
// band from exceeding the shape, so deep-interior fragments stay fully filled at any angle while
// the edge still antialiases at normal (non-grazing) views (where `fwidth(d)` << inradius, so the
// cap never bites).
fn shape_inradius(in: VsOut) -> f32 {
    if (in.kind == 0u) {
        let half = 0.5 * (in.p0.zw - in.p0.xy);
        return max(min(half.x, half.y), 0.0001);
    } else if (in.kind == 1u || in.kind == 2u) {
        return max(in.p0.z, 0.0001);
    } else {
        return max(in.p1.x, 0.0001);
    }
}

// Geometric fill coverage (0..1) independent of any color alpha. Used by the id pass for
// pick-only shapes so an invisible (alpha-0) rect still stamps its id inside its bounds.
fn shape_fill_coverage(in: VsOut) -> f32 {
    let d = shape_sdf(in);
    let aa = clamp(fwidth(d), 0.0001, shape_inradius(in));
    return 1.0 - smoothstep(-aa, aa, d);
}

fn shape_coverage(in: VsOut) -> vec2<f32> {
    let d = shape_sdf(in);
    let sw = in.p1.y;
    let aa = clamp(fwidth(d), 0.0001, shape_inradius(in));
    let fill_a = 1.0 - smoothstep(-aa, aa, d);
    if (in.kind == 3u) {
        return vec2<f32>(fill_a * in.fill.a, fill_a * in.fill.a);
    }
    let stroke_a = 1.0 - smoothstep(-aa, aa, abs(d) - sw * 0.5);
    let alpha = max(fill_a * in.fill.a, stroke_a * in.stroke.a);
    return vec2<f32>(stroke_a, alpha);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Pick-only shapes (p1.w flag, e.g. inline-field hit rects) exist SOLELY to stamp object ids;
    // they never draw color. Discarding here also skips the depth write, so an invisible pick rect
    // can't occlude the widget/text drawn later in the color target.
    if (in.p1.w >= 0.5) {
        discard;
    }
    let coverage = shape_coverage(in);
    let stroke_a = coverage.x;
    let alpha = coverage.y;
    if (alpha < 1.0 / 255.0) {
        discard;
    }
    if (in.kind == 3u) {
        return vec4<f32>(in.fill.rgb, alpha);
    }
    let col = mix(in.fill, in.stroke, stroke_a);
    return vec4<f32>(col.rgb, alpha);
}

@fragment
fn fs_id(in: VsOut) -> @location(0) u32 {
    // Pick-only shapes gate on GEOMETRIC coverage (ignoring color alpha), so an alpha-0 field rect
    // still writes its id everywhere inside its bounds.
    if (in.p1.w >= 0.5) {
        if (shape_fill_coverage(in) < 1.0 / 255.0) {
            discard;
        }
        return in.pick_id;
    }
    let coverage = shape_coverage(in);
    let alpha = coverage.y;
    if (alpha < 1.0 / 255.0) {
        discard;
    }
    return in.pick_id;
}
