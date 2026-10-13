// GPU parity entry for tests/gpu_parity.rs (plan §3 "GPU harness", wave R3). Appended to
// standard_surface_bsdf::wgsl_source(). One invocation per case: unpack IN_STRIDE floats,
// evaluate every public entry point plus the RNG and a few primitives, write OUT_STRIDE floats
// (u32 results bitcast). The layouts are mirrored by `pack_case` / `cpu_outputs` in the test.

const PARITY_IN_STRIDE: u32 = 83u;
const PARITY_OUT_STRIDE: u32 = 67u;

@group(0) @binding(0) var<storage, read> parity_in: array<f32>;
@group(0) @binding(1) var<storage, read_write> parity_out: array<f32>;

fn parity_v3(base: u32) -> vec3<f32> {
    return vec3<f32>(parity_in[base], parity_in[base + 1u], parity_in[base + 2u]);
}

fn parity_put3(base: u32, v: vec3<f32>) {
    parity_out[base] = v.x;
    parity_out[base + 1u] = v.y;
    parity_out[base + 2u] = v.z;
}

@compute @workgroup_size(64)
fn parity_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let case_index = gid.x;
    if (case_index >= arrayLength(&parity_in) / PARITY_IN_STRIDE) {
        return;
    }
    let b = case_index * PARITY_IN_STRIDE;
    let i = SsInputs(
        parity_in[b + 0u],
        parity_v3(b + 1u),
        parity_in[b + 4u],
        parity_in[b + 5u],
        parity_in[b + 6u],
        parity_v3(b + 7u),
        parity_in[b + 10u],
        parity_in[b + 11u],
        parity_in[b + 12u],
        parity_in[b + 13u],
        parity_in[b + 64u],
        parity_v3(b + 65u),
        parity_in[b + 68u],
        parity_in[b + 73u],
        parity_v3(b + 74u),
        parity_v3(b + 77u),
        parity_in[b + 80u],
        parity_in[b + 81u],
        parity_in[b + 14u],
        parity_v3(b + 15u),
        parity_in[b + 18u],
        parity_in[b + 19u],
        parity_v3(b + 20u),
        parity_in[b + 23u],
        parity_in[b + 24u],
        parity_in[b + 25u],
        parity_in[b + 26u],
        parity_in[b + 27u],
        parity_in[b + 28u],
        parity_in[b + 29u],
        parity_in[b + 30u],
        parity_in[b + 31u],
        parity_v3(b + 32u),
        u32(parity_in[b + 35u]),
    );
    let f = SsFrame(parity_v3(b + 36u), parity_v3(b + 39u), parity_in[b + 69u] > 0.5, parity_in[b + 82u]);
    let wo = parity_v3(b + 42u);
    let wi = parity_v3(b + 45u);
    let u = parity_v3(b + 48u);
    let half_angle = parity_in[b + 51u];
    let env = SsEnvironment(parity_v3(b + 52u), parity_v3(b + 55u), parity_v3(b + 58u), parity_v3(b + 70u));
    let seed = bitcast<u32>(parity_in[b + 61u]);

    let o = case_index * PARITY_OUT_STRIDE;
    let l = ss_eval_light(i, f, wo, wi);
    parity_put3(o + 0u, l.base);
    parity_put3(o + 3u, l.specular);
    parity_put3(o + 53u, l.transmission);
    let d = ss_eval_light_disc(i, f, wo, wi, half_angle);
    parity_put3(o + 6u, d.base);
    parity_put3(o + 9u, d.specular);
    parity_put3(o + 56u, d.transmission);
    let e = ss_eval_environment(i, f, wo, env);
    parity_put3(o + 12u, e.base);
    parity_put3(o + 15u, e.specular);
    parity_put3(o + 59u, e.transmission);
    parity_put3(o + 18u, ss_eval_emission(i, f, wo));
    parity_put3(o + 64u, ss_guide_albedo(i));
    var w = ss_lobe_weights(i, f, wo);
    for (var k: u32 = 0u; k < 5u; k = k + 1u) {
        parity_out[o + 21u + k] = w[k];
    }
    parity_out[o + 52u] = w[SS_LOBE_TRANSMISSION];
    parity_out[o + 26u] = ss_pdf(i, f, wo, wi);
    let s = ss_sample(i, f, wo, u);
    parity_put3(o + 27u, s.wi);
    parity_put3(o + 30u, s.weight);
    parity_out[o + 33u] = s.pdf;
    parity_out[o + 34u] = f32(s.lobe);
    parity_out[o + 35u] = select(0.0, 1.0, s.valid);
    parity_out[o + 36u] = select(0.0, 1.0, s.delta);
    let r = ss_pcg4d(vec4<u32>(case_index, seed, 3u, 5u));
    parity_out[o + 37u] = bitcast<f32>(r.x);
    parity_out[o + 38u] = bitcast<f32>(r.y);
    parity_out[o + 39u] = bitcast<f32>(r.z);
    parity_out[o + 40u] = bitcast<f32>(r.w);
    parity_out[o + 41u] = ss_rng(case_index, seed, 7u, 2u, 0x5eedu);
    parity_out[o + 62u] = ss_sobol(case_index, 7u, seed, case_index % 8u);
    parity_out[o + 63u] = ss_sobol(seed, case_index, case_index, seed % 8u);
    // Primitives: Airy reflectance at the view angle and the film albedo of the specular lobe.
    let fd = mx_init_fresnel_dielectric(i.specular_ior, i.thin_film_thickness, i.thin_film_ior);
    let ndotv = clamp(dot(f.n, wo), MX_FLOAT_EPS, 1.0);
    parity_put3(o + 42u, mx_fresnel_airy(ndotv, fd));
    let alpha = ss_main_alpha(i);
    let tangent = ss_shading_tangent(f.n, f.tangent, i.specular_rotation, i.specular_anisotropy);
    parity_out[o + 45u] = ss_ggx_lobe_pdf(wo, wi, f.n, tangent, alpha);
    let x = normalize(tangent - f.n * dot(tangent, f.n));
    let y = cross(f.n, x);
    parity_put3(o + 46u, mx_ggx_importance_sample_vndf(vec2<f32>(u.y, u.z), ss_to_local(wo, x, y, f.n), alpha));
    parity_put3(o + 49u, ss_film_dir_albedo(ndotv, mx_average_alpha(alpha), fd, i.thin_film_energy));
}
