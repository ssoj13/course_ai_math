//! CPU/WGSL parity of the BSDF (plan §3 "GPU harness", wave R3).
//!
//! `parity_entry.wgsl` is appended to `wgsl_source()` and evaluates, per case, every public
//! entry point (`ss_eval_light`, `ss_eval_light_disc`, `ss_eval_environment`,
//! `ss_eval_emission`, `ss_lobe_weights`, `ss_pdf`, `ss_sample`), the RNGs (`ss_pcg4d`,
//! `ss_rng`, `ss_sobol`) and four primitives (`mx_fresnel_airy`, `ss_ggx_lobe_pdf`,
//! `mx_ggx_importance_sample_vndf`, `ss_film_dir_albedo`). Every other twin function is reached
//! through these. The same values are computed by the Rust twin and compared field by field.
//!
//! Cases: the plan's deterministic grid (n.v x n.l x azimuth x roughness x IOR x anisotropy x
//! film = 134 400) plus 4096 random opaque cases from a fixed splitmix64 stream, plus 4096
//! random transmissive cases (Q6: `transmission > 0`, both sides of the surface, `wi` on the
//! whole sphere, delta and `specular_IOR = 1` lobes included). The transmission lobe's values
//! use the condition number of the generalized half vector (`conditioning_t`). Tolerances **[R]**
//! (plan §3), with `scale = max(|cpu|, |gpu|)`:
//! - eval/pdf/probes: `1e-6 + 2e-4 scale`; cases with a film (Airy): `1e-5 + 2e-3 scale`;
//! - sample: `wi` 1e-4 absolute; the GPU's `pdf` (2e-4 relative) and `weight` (1e-3, Airy
//!   2e-3) are checked against the Rust twin evaluated at the GPU's own `wi` (the two
//!   directions differ by up to ~5e-5, wider than a lobe of `alpha ~ 1e-4`);
//! - every GGX-dependent value adds its derived condition number (`conditioning`: the half
//!   vector's cancellation `eps / |wo + wi|`, divided by `alpha` in the slope, plus
//!   `eps / cos`), which generalises the R4 rule; values whose condition number exceeds 0.1
//!   (grazing near-mirror configurations of near-delta lobes, where f32 has no significant
//!   digits on either side) are counted and reported, not compared;
//! - `lobe`/`valid`/`delta` equal, except within 1e-5 of a lobe-interval boundary in `u.x`
//!   (counted and reported);
//! - RNG: bit-identical; anything non-finite fails.
//!
//! The GPU test is `#[ignore]`d (it needs a compute adapter); run it with
//! `cargo test -p standard-surface-bsdf --test gpu_parity -- --ignored --nocapture`. The naga
//! validation of the combined parity module is an ordinary test.

mod common;

use standard_surface_bsdf::fresnel::{
    film_dir_albedo, mx_fresnel_airy, mx_init_fresnel_dielectric,
};
use standard_surface_bsdf::math::{clampf, cross3, dot3, normalize3, scale3, sub3};
use standard_surface_bsdf::microfacet::{is_delta, mx_average_alpha};
use standard_surface_bsdf::sample::{ggx_lobe_pdf, pdf, sample};
use standard_surface_bsdf::sampling::{mx_ggx_importance_sample_vndf, pcg4d, rng, sobol, to_local};
use standard_surface_bsdf::surface::{main_alpha, shading_tangent};
use standard_surface_bsdf::{
    Environment, Lobe, ShadingFrame, SurfaceInputs, ThinFilmEnergy, eval_emission,
    eval_environment, eval_light, eval_light_disc, lobe_weights, wgsl_source,
};

const IN_STRIDE: usize = 83;
const OUT_STRIDE: usize = 67;
const PARITY_ENTRY: &str = include_str!("parity_entry.wgsl");

fn parity_source() -> String {
    format!("{}\n{}", wgsl_source(), PARITY_ENTRY)
}

/// One parity case.
#[derive(Clone, Copy, Debug)]
struct Case {
    i: SurfaceInputs,
    f: ShadingFrame,
    wo: [f32; 3],
    wi: [f32; 3],
    u: [f32; 3],
    half_angle: f32,
    env: Environment,
    seed: u32,
}

fn pack_case(c: &Case) -> [f32; IN_STRIDE] {
    let i = &c.i;
    let mut v = [0.0_f32; IN_STRIDE];
    let scalars: [(usize, f32); 20] = [
        (0, i.base),
        (4, i.diffuse_roughness),
        (5, i.metalness),
        (6, i.specular),
        (10, i.specular_roughness),
        (11, i.specular_ior),
        (12, i.specular_anisotropy),
        (13, i.specular_rotation),
        (14, i.sheen),
        (18, i.sheen_roughness),
        (19, i.coat),
        (23, i.coat_roughness),
        (24, i.coat_anisotropy),
        (25, i.coat_rotation),
        (26, i.coat_ior),
        (27, i.coat_affect_color),
        (28, i.coat_affect_roughness),
        (29, i.thin_film_thickness),
        (30, i.thin_film_ior),
        (31, i.emission),
    ];
    for (k, x) in scalars {
        v[k] = x;
    }
    let vectors: [(usize, [f32; 3]); 14] = [
        (1, i.base_color),
        (7, i.specular_color),
        (15, i.sheen_color),
        (20, i.coat_color),
        (32, i.emission_color),
        (36, c.f.n),
        (39, c.f.tangent),
        (42, c.wo),
        (45, c.wi),
        (48, c.u),
        (52, c.env.irradiance),
        (55, c.env.specular_radiance),
        (58, c.env.coat_radiance),
        (61, [f32::from_bits(c.seed), 0.0, 0.0]),
    ];
    for (k, x) in vectors {
        v[k..k + 3].copy_from_slice(&x);
    }
    v[35] = i.thin_film_energy.code() as f32;
    v[51] = c.half_angle;
    v[64] = i.transmission;
    v[65..68].copy_from_slice(&i.transmission_color);
    v[68] = i.transmission_extra_roughness;
    v[69] = if c.f.inside { 1.0 } else { 0.0 };
    v[70..73].copy_from_slice(&c.env.transmission_radiance);
    v[73] = i.subsurface;
    v[74..77].copy_from_slice(&i.subsurface_color);
    v[77..80].copy_from_slice(&i.subsurface_radius);
    v[80] = i.subsurface_scale;
    v[81] = i.subsurface_anisotropy;
    v[82] = c.f.curvature;
    v
}

/// The Rust twin's outputs in the layout of `parity_entry.wgsl`.
fn cpu_outputs(c: &Case, case_index: u32) -> [f32; OUT_STRIDE] {
    let (i, f, wo, wi, u) = (&c.i, &c.f, c.wo, c.wi, c.u);
    let mut o = [0.0_f32; OUT_STRIDE];
    let mut put = |k: usize, x: [f32; 3]| o[k..k + 3].copy_from_slice(&x);
    let l = eval_light(i, f, wo, wi);
    put(0, l.base);
    put(3, l.specular);
    let d = eval_light_disc(i, f, wo, wi, c.half_angle);
    put(6, d.base);
    put(9, d.specular);
    let e = eval_environment(i, f, wo, &c.env);
    put(12, e.base);
    put(15, e.specular);
    put(18, eval_emission(i, f, wo));
    put(64, i.guide_albedo());
    let s = sample(i, f, wo, u);
    put(27, s.wi);
    put(30, s.weight);
    let fd = mx_init_fresnel_dielectric(i.specular_ior, i.thin_film_thickness, i.thin_film_ior);
    let ndotv = clampf(dot3(f.n, wo), 1e-8, 1.0);
    put(42, mx_fresnel_airy(ndotv, &fd));
    let alpha = main_alpha(i);
    let tangent = shading_tangent(f.n, f.tangent, i.specular_rotation, i.specular_anisotropy);
    let x = normalize3(sub3(tangent, scale3(f.n, dot3(tangent, f.n))));
    let y = cross3(f.n, x);
    put(
        46,
        mx_ggx_importance_sample_vndf([u[1], u[2]], to_local(wo, x, y, f.n), alpha),
    );
    put(
        49,
        film_dir_albedo(ndotv, mx_average_alpha(alpha), &fd, i.thin_film_energy),
    );
    put(53, l.transmission);
    put(56, d.transmission);
    put(59, e.transmission);
    let w = lobe_weights(i, f, wo);
    o[21..26].copy_from_slice(&w[..5]);
    o[52] = w[5];
    o[26] = pdf(i, f, wo, wi);
    o[33] = s.pdf;
    o[34] = s.lobe.index() as f32;
    o[35] = if s.valid { 1.0 } else { 0.0 };
    o[36] = if s.delta { 1.0 } else { 0.0 };
    let r = pcg4d([case_index, c.seed, 3, 5]);
    for (k, x) in r.iter().enumerate() {
        o[37 + k] = f32::from_bits(*x);
    }
    o[41] = rng(case_index, c.seed, 7, 2, 0x5eed);
    o[62] = sobol(case_index, 7, c.seed, case_index % 8);
    o[63] = sobol(c.seed, case_index, case_index, c.seed % 8);
    o[45] = ggx_lobe_pdf(wo, wi, f.n, tangent, alpha);
    o
}

// ---------------------------------------------------------------------------------------------
// Cases.

fn unit_dir(cos_theta: f32, phi: f32) -> [f32; 3] {
    let s = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    [s * phi.cos(), s * phi.sin(), cos_theta]
}

fn grid_cases() -> Vec<Case> {
    let cosines = [1e-3_f32, 0.01, 0.1, 0.3, 0.5, 0.7, 0.9, 1.0];
    let azimuths = [0.0_f32, 45.0, 90.0, 180.0];
    let roughnesses = [0.0_f32, 0.01, 0.05, 0.1, 0.2, 0.5, 1.0];
    let iors = [1.0_f32, 1.33, 1.5, 2.4, 5.0];
    let anisotropies = [0.0_f32, 0.5, 0.98];
    let films = [0.0_f32, 100.0, 400.0, 1000.0, 2000.0];
    let frame = common::FRAME;
    let mut cases = Vec::with_capacity(134_400);
    let mut idx = 0u32;
    for &cv in &cosines {
        for &cl in &cosines {
            for &az in &azimuths {
                for &r in &roughnesses {
                    for &ior in &iors {
                        for &a in &anisotropies {
                            for &film in &films {
                                let k = idx as usize;
                                let i = SurfaceInputs {
                                    specular_roughness: r,
                                    coat_roughness: r,
                                    specular_ior: ior,
                                    coat_ior: ior,
                                    specular_anisotropy: a,
                                    coat_anisotropy: a,
                                    thin_film_thickness: film,
                                    metalness: [0.0, 0.3, 1.0][k % 3],
                                    thin_film_energy: if k.is_multiple_of(2) {
                                        ThinFilmEnergy::MaterialX
                                    } else {
                                        ThinFilmEnergy::Conserving
                                    },
                                    ..common::rich()
                                };
                                let u = [
                                    ((k % 8) as f32 + 0.5) / 8.0,
                                    (((k / 8) % 8) as f32 + 0.5) / 8.0,
                                    (((k / 64) % 8) as f32 + 0.5) / 8.0,
                                ];
                                cases.push(Case {
                                    i,
                                    f: frame,
                                    wo: unit_dir(cv, 0.3),
                                    wi: unit_dir(cl, 0.3 + az.to_radians() + std::f32::consts::PI),
                                    u,
                                    half_angle: 0.0046,
                                    env: Environment {
                                        irradiance: [0.8, 0.9, 1.0],
                                        specular_radiance: [1.2, 1.0, 0.7],
                                        coat_radiance: [0.6, 0.8, 1.4],
                                        transmission_radiance: [0.9, 0.7, 0.5],
                                    },
                                    seed: idx.wrapping_mul(2_654_435_761),
                                });
                                idx += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    cases
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn unit(state: &mut u64) -> f32 {
    (splitmix(state) >> 40) as f32 / 16_777_216.0
}

/// `n` random cases from a fixed stream. Opaque cases (`transmissive = false`, the original
/// 4096) draw exactly the stream they drew before the transmission lobe existed; transmissive
/// cases use their own seed and draw the transmission inputs, the side and a `wi` anywhere on
/// the sphere.
fn random_cases(n: usize, transmissive: bool) -> Vec<Case> {
    let mut st = if transmissive {
        0x7a45_2026_0924_u64
    } else {
        0x5eed_2026_0923_u64
    };
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let c3 = |s: &mut u64| [unit(s), unit(s), unit(s)];
        let mut i = SurfaceInputs {
            base: unit(&mut st),
            base_color: c3(&mut st),
            diffuse_roughness: unit(&mut st),
            metalness: unit(&mut st),
            specular: unit(&mut st),
            specular_color: c3(&mut st),
            specular_roughness: unit(&mut st),
            specular_ior: 1.0 + 4.0 * unit(&mut st),
            specular_anisotropy: unit(&mut st),
            specular_rotation: unit(&mut st),
            transmission: 0.0,
            transmission_color: [1.0, 1.0, 1.0],
            transmission_extra_roughness: 0.0,
            subsurface: 0.0,
            subsurface_color: [1.0, 1.0, 1.0],
            subsurface_radius: [1.0, 1.0, 1.0],
            subsurface_scale: 1.0,
            subsurface_anisotropy: 0.0,
            sheen: unit(&mut st),
            sheen_color: c3(&mut st),
            sheen_roughness: unit(&mut st),
            coat: unit(&mut st),
            coat_color: c3(&mut st),
            coat_roughness: unit(&mut st),
            coat_anisotropy: unit(&mut st),
            coat_rotation: unit(&mut st),
            coat_ior: 1.0 + 4.0 * unit(&mut st),
            coat_affect_color: unit(&mut st),
            coat_affect_roughness: unit(&mut st),
            thin_film_thickness: if k % 3 == 0 {
                0.0
            } else {
                2000.0 * unit(&mut st)
            },
            thin_film_ior: 1.0 + 2.0 * unit(&mut st),
            emission: 2.0 * unit(&mut st),
            emission_color: c3(&mut st),
            thin_film_energy: if k % 2 == 0 {
                ThinFilmEnergy::MaterialX
            } else {
                ThinFilmEnergy::Conserving
            },
        };
        let n = normalize3([
            unit(&mut st) - 0.5,
            unit(&mut st) - 0.5,
            unit(&mut st) + 0.05,
        ]);
        let t = [
            unit(&mut st) - 0.5,
            unit(&mut st) - 0.5,
            unit(&mut st) - 0.5,
        ];
        // wo, wi on the hemisphere of n.
        let hemi = |s: &mut u64| {
            let d = normalize3([unit(s) - 0.5, unit(s) - 0.5, unit(s) - 0.5]);
            if dot3(d, n) < 0.0 { scale3(d, -1.0) } else { d }
        };
        let wo = hemi(&mut st);
        let mut wi = hemi(&mut st);
        let mut inside = false;
        if transmissive {
            i.transmission = 0.05 + 0.95 * unit(&mut st);
            i.transmission_color = c3(&mut st);
            i.transmission_extra_roughness = 2.0 * unit(&mut st) - 1.0;
            inside = k % 2 == 1;
            // Smooth glass (delta transmission) and index-matched (eta = 1) lobes.
            if k % 7 == 0 {
                i.specular_roughness = 0.0;
                i.transmission_extra_roughness = 0.0;
                i.coat_affect_roughness = 0.0;
            }
            if k % 13 == 0 {
                i.specular_ior = 1.0;
            }
            // Three quarters of the directions below the surface.
            if unit(&mut st) < 0.75 {
                wi = scale3(wi, -1.0);
            }
        }
        out.push(Case {
            i,
            f: ShadingFrame {
                n,
                tangent: t,
                inside,
                curvature: 0.0,
            },
            wo,
            wi,
            u: [unit(&mut st), unit(&mut st), unit(&mut st)],
            half_angle: 0.05 * unit(&mut st),
            env: Environment {
                irradiance: c3(&mut st),
                specular_radiance: c3(&mut st),
                coat_radiance: c3(&mut st),
                transmission_radiance: [0.3, 0.6, 0.9],
            },
            seed: splitmix(&mut st) as u32,
        });
    }
    out
}

/// `n` random subsurface cases: the opaque random cases with the subsurface inputs and the
/// curvature drawn from their own stream (`subsurface` in (0, 1], colours, a mean free path of
/// 0..2 per channel times a scale of 0..2, curvature 0..4 per unit length, including 0).
fn subsurface_cases(n: usize) -> Vec<Case> {
    let mut st = 0x55b5_2026_0925_u64;
    random_cases(n, false)
        .into_iter()
        .enumerate()
        .map(|(k, mut c)| {
            let c3 = |s: &mut u64| [unit(s), unit(s), unit(s)];
            c.i.subsurface = 0.05 + 0.95 * unit(&mut st);
            c.i.subsurface_color = c3(&mut st);
            c.i.subsurface_radius = [
                2.0 * unit(&mut st),
                2.0 * unit(&mut st),
                2.0 * unit(&mut st),
            ];
            c.i.subsurface_scale = 2.0 * unit(&mut st);
            c.i.subsurface_anisotropy = 2.0 * unit(&mut st) - 1.0;
            c.f.curvature = if k % 5 == 0 { 0.0 } else { 4.0 * unit(&mut st) };
            c
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// GPU.

fn run_gpu(cases: &[Case]) -> Vec<f32> {
    use wgpu::util::DeviceExt;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .expect("a wgpu compute adapter (this test is --ignored for machines without one)");
    let info = adapter.get_info();
    println!(
        "adapter: {} ({:?}, driver {} {})",
        info.name, info.backend, info.driver, info.driver_info
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("bsdf_parity_device"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("device");

    let t0 = std::time::Instant::now();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("bsdf_parity_shader"),
        source: wgpu::ShaderSource::Wgsl(parity_source().into()),
    });
    let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("bsdf_parity_bgl"),
        entries: &[storage(0, true), storage(1, false)],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("bsdf_parity_pl"),
        bind_group_layouts: &[Some(&bgl)],
        ..Default::default()
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("bsdf_parity_pipeline"),
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("parity_main"),
        compilation_options: Default::default(),
        cache: None,
    });
    println!(
        "parity pipeline created in {:.2} s",
        t0.elapsed().as_secs_f64()
    );

    let mut out = Vec::with_capacity(cases.len() * OUT_STRIDE);
    let chunk_cases = 8192usize;
    let t1 = std::time::Instant::now();
    for chunk in cases.chunks(chunk_cases) {
        // The GPU's case index (RNG counter) is chunk-local; `check_all` mirrors that.
        let input: Vec<f32> = chunk.iter().flat_map(pack_case).collect();
        let in_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bsdf_parity_in"),
            contents: bytemuck::cast_slice(&input),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let size = (chunk.len() * OUT_STRIDE * 4) as u64;
        let out_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bsdf_parity_out"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bsdf_parity_readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bsdf_parity_bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: in_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: out_buf.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("bsdf_parity_enc"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("bsdf_parity_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bg, &[]);
            pass.dispatch_workgroups((chunk.len() as u32).div_ceil(64), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&out_buf, 0, &readback, 0, size);
        queue.submit(std::iter::once(encoder.finish()));
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            tx.send(r).expect("map result channel");
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device poll");
        rx.recv().expect("map callback").expect("map");
        {
            let data = slice.get_mapped_range().expect("mapped range");
            out.extend_from_slice(bytemuck::cast_slice::<u8, f32>(&data));
        }
        readback.unmap();
    }
    println!(
        "{} cases evaluated on the GPU in {:.2} s ({} per submit)",
        cases.len(),
        t1.elapsed().as_secs_f64(),
        chunk_cases
    );
    out
}

// ---------------------------------------------------------------------------------------------
// Comparison.

#[derive(Default)]
struct Stat {
    max_abs: f64,
    max_rel: f64,
    violations: usize,
    compared: usize,
    worst: Option<(usize, f32, f32)>,
}

impl Stat {
    fn check(&mut self, case: usize, cpu: f32, gpu: f32, abs_tol: f64, rel_tol: f64) {
        let (c, g) = (f64::from(cpu), f64::from(gpu));
        let err = (c - g).abs();
        let scale = c.abs().max(g.abs());
        self.compared += 1;
        let rel = if scale > 0.0 { err / scale } else { 0.0 };
        if err > self.max_abs {
            self.max_abs = err;
        }
        if rel > self.max_rel && err > abs_tol {
            self.max_rel = rel;
        }
        if err > abs_tol + rel_tol * scale {
            self.violations += 1;
            if self
                .worst
                .is_none_or(|(_, wc, wg)| (f64::from(wc) - f64::from(wg)).abs() < err)
            {
                self.worst = Some((case, cpu, gpu));
            }
        }
    }
}

fn check_all(cases: &[Case], gpu: &[f32], chunk_cases: usize) -> bool {
    use std::collections::BTreeMap;
    let mut stats: BTreeMap<String, Stat> = BTreeMap::new();
    let (mut boundary, mut nonfinite, mut skipped_delta_probe) = (0usize, 0usize, 0usize);
    let mut lobe_mismatch = 0usize;
    let mut rng_mismatch = 0usize;
    let mut ill = 0usize;
    for (k, case) in cases.iter().enumerate() {
        let local = (k % chunk_cases) as u32;
        let cpu = cpu_outputs(case, local);
        let g = &gpu[k * OUT_STRIDE..(k + 1) * OUT_STRIDE];
        let airy = case.i.thin_film_thickness > 0.0;
        // Transmissive cases (Q6) are reported in their own rows.
        let tag = if case.i.transmission > 0.0 {
            " [T]"
        } else if case.i.subsurface > 0.0 {
            " [S]"
        } else {
            ""
        };
        let (a_eval, r_eval): (f64, f64) = if airy { (1e-5, 2e-3) } else { (1e-6, 2e-4) };
        let cond = case_conditioning(case, case.wi);
        for (name, range) in [
            ("eval_light", 0..6),
            ("eval_light_disc", 6..12),
            ("eval_environment", 12..18),
            ("eval_emission", 18..21),
            ("lobe_weights", 21..26),
            ("lobe_weights", 52..53),
            ("eval_light", 53..56),
            ("eval_light_disc", 56..59),
            ("eval_environment", 59..62),
            ("guide_albedo", 64..67),
            ("pdf", 26..27),
            ("mx_fresnel_airy", 42..45),
            ("ss_film_dir_albedo", 49..52),
        ] {
            for j in range {
                if !g[j].is_finite() || !cpu[j].is_finite() {
                    nonfinite += 1;
                    continue;
                }
                let lobe_dependent = !matches!(
                    name,
                    "eval_emission"
                        | "guide_albedo"
                        | "lobe_weights"
                        | "mx_fresnel_airy"
                        | "ss_film_dir_albedo"
                        | "eval_environment"
                );
                if lobe_dependent && cond > ILL_CONDITIONED {
                    ill += 1;
                    continue;
                }
                let st = stats.entry(format!("{name}{tag}")).or_default();
                // The Airy probe always takes the Airy path, whatever the case's thickness.
                let (a, r) = if name == "mx_fresnel_airy" {
                    (1e-5, 2e-3)
                } else {
                    (a_eval, r_eval)
                };
                st.check(k, cpu[j], g[j], a, r + cond);
            }
        }
        // Raw primitives of the specular lobe: meaningless for a delta alpha (D ~ 1e15).
        let alpha = main_alpha(&case.i);
        if is_delta(alpha) {
            skipped_delta_probe += 1;
        } else {
            let c = conditioning(alpha[0].min(alpha[1]), case.wo, case.wi, case.f.n);
            if c > ILL_CONDITIONED {
                ill += 1;
            } else {
                stats
                    .entry(format!("ss_ggx_lobe_pdf{tag}"))
                    .or_default()
                    .check(k, cpu[45], g[45], 1e-6, 2e-4 + c);
            }
            for j in 46..49 {
                stats
                    .entry(format!("mx_ggx_importance_sample_vndf{tag}"))
                    .or_default()
                    .check(k, cpu[j], g[j], 1e-4, 0.0);
            }
        }
        // RNG: bit-identical.
        for j in (37..42).chain(62..64) {
            if cpu[j].to_bits() != g[j].to_bits() {
                rng_mismatch += 1;
            }
        }
        // Sample.
        if !g[27..37].iter().all(|x| x.is_finite()) {
            nonfinite += 1;
            continue;
        }
        let same_branch = cpu[34] == g[34] && cpu[35] == g[35] && cpu[36] == g[36];
        if !same_branch {
            let w = lobe_weights(&case.i, &case.f, case.wo);
            let mut acc = 0.0_f32;
            let near = w.iter().any(|p| {
                acc += p;
                (case.u[0] - acc).abs() < 1e-5
            });
            if near {
                boundary += 1;
            } else {
                lobe_mismatch += 1;
                if lobe_mismatch <= 5 {
                    println!(
                        "sample branch mismatch case {k}: cpu lobe/valid/delta {:?} gpu {:?} \
                         cpu wi {:?} gpu wi {:?}",
                        &cpu[34..37],
                        &g[34..37],
                        &cpu[27..30],
                        &g[27..30]
                    );
                }
            }
            continue;
        }
        if cpu[35] == 0.0 {
            continue;
        }
        for j in 27..30 {
            stats
                .entry(format!("sample.wi{tag}"))
                .or_default()
                .check(k, cpu[j], g[j], 1e-4, 0.0);
        }
        // The sampled direction itself: 1e-4 absolute (above). Its density and weight are a
        // different question: CPU and GPU directions differ by up to ~5e-5, more than the width
        // of a lobe with alpha ~ 1e-4, so pdf and weight are checked at the GPU's own `wi`
        // against the Rust twin evaluated there.
        let delta = cpu[36] == 1.0;
        let g_wi = [g[27], g[28], g[29]];
        if delta {
            // Mirror direction and discrete probability: plain value comparisons.
            stats
                .entry(format!("sample.pdf (delta){tag}"))
                .or_default()
                .check(k, cpu[33], g[33], 0.0, r_eval);
            for j in 30..33 {
                stats
                    .entry(format!("sample.weight (delta){tag}"))
                    .or_default()
                    .check(k, cpu[j], g[j], a_eval, r_eval);
            }
        } else {
            let lobe_alpha = sampled_lobe_alpha(case, cpu[34] as usize);
            let cond_s = if dot3(case.f.n, g_wi) < 0.0 {
                case_conditioning(case, g_wi)
            } else {
                conditioning(
                    lobe_alpha.min(min_nondelta_alpha(case)),
                    case.wo,
                    g_wi,
                    case.f.n,
                )
            };
            if cond_s > ILL_CONDITIONED {
                ill += 1;
                continue;
            }
            let p_at = pdf(&case.i, &case.f, case.wo, g_wi);
            stats
                .entry(format!("sample.pdf at gpu wi{tag}"))
                .or_default()
                .check(k, p_at, g[33], 0.0, 2e-4 + cond_s);
            let e_at = eval_light(&case.i, &case.f, case.wo, g_wi).sum();
            let r_w = if airy { 2e-3 } else { 1e-3 };
            for (j, ek) in (30..33).zip(e_at) {
                stats
                    .entry(format!("sample.weight at gpu wi{tag}"))
                    .or_default()
                    .check(k, ek / p_at, g[j], 1e-6, r_w + cond_s);
            }
        }
    }
    println!(
        "{:<30} {:>9} {:>12} {:>12} {:>10}",
        "field", "compared", "max |err|", "max rel", "violations"
    );
    let mut ok = true;
    for (name, st) in &stats {
        println!(
            "{name:<30} {:>9} {:>12.3e} {:>12.3e} {:>10}",
            st.compared, st.max_abs, st.max_rel, st.violations
        );
        if st.violations > 0 {
            ok = false;
            if let Some((case, c, g)) = st.worst {
                println!(
                    "  worst: case {case} cpu {c:e} gpu {g:e}\n  {:?}",
                    cases[case]
                );
            }
        }
    }
    println!(
        "sample branches: {boundary} within 1e-5 of a lobe boundary (allowed), {lobe_mismatch} \
         other mismatches; RNG bit mismatches {rng_mismatch}; non-finite {nonfinite}; \
         delta-alpha probe cases skipped {skipped_delta_probe}; ill-conditioned values (condition number > {ILL_CONDITIONED}) counted, not compared: {ill}"
    );
    ok && lobe_mismatch == 0 && rng_mismatch == 0 && nonfinite == 0
}

/// Unit-roundoff scale of the two twins' direction arithmetic (normalize/dot/cross in f32 on
/// either side differ by a few ulp).
const DIR_EPS: f64 = 3e-7;

/// Relative condition number of a GGX evaluation at `(wo, wi)`, derived rather than tuned. The
/// half vector `H = (wo + wi) / |wo + wi|` inherits a component error `DIR_EPS / h` with
/// `h = |wo + wi| / 2` (catastrophic cancellation at grazing mirror configurations), which the
/// slope `H.xy / alpha` divides by `alpha` again; the NDF's relative change is at most ~2x the
/// slope error. Clamped cosines near the horizon add `DIR_EPS / cos`.
fn conditioning(alpha_min: f32, wo: [f32; 3], wi: [f32; 3], n: [f32; 3]) -> f64 {
    let s = [wo[0] + wi[0], wo[1] + wi[1], wo[2] + wi[2]];
    let h = 0.5 * f64::from(dot3(s, s)).sqrt();
    let cos_min = f64::from(dot3(n, wo).min(dot3(n, wi))).max(1e-8);
    2.0 * DIR_EPS / (f64::from(alpha_min) * h.max(1e-8)) + 2.0 * DIR_EPS / cos_min
}

/// The transmission lobe's counterpart of [`conditioning`]: the generalized half vector
/// `wo + eta wi` loses `DIR_EPS (1 + eta) / |wo + eta wi|` to cancellation (grazing
/// refractions), which the slope divides by `alpha` and the Jacobian `(eta wi.m + wo.m)^2 =
/// |wo + eta wi|^2` doubles; clamped cosines add `DIR_EPS / cos` on either side.
fn conditioning_t(alpha_min: f32, wo: [f32; 3], wi: [f32; 3], n: [f32; 3], eta: f32) -> f64 {
    let s = [
        wo[0] + eta * wi[0],
        wo[1] + eta * wi[1],
        wo[2] + eta * wi[2],
    ];
    let len = f64::from(dot3(s, s)).sqrt().max(1e-8);
    let e = DIR_EPS * (1.0 + f64::from(eta)) / len;
    let cos_min = f64::from(dot3(n, wo).min(-dot3(n, wi))).max(1e-8);
    2.0 * e / f64::from(alpha_min) + 2.0 * e + 2.0 * DIR_EPS / cos_min
}

/// The condition number of every lobe-dependent value of `case` at `wi`: the transmission
/// lobe's below the surface, the reflection lobes' above it.
fn case_conditioning(case: &Case, wi: [f32; 3]) -> f64 {
    if dot3(case.f.n, wi) < 0.0 {
        let s = standard_surface_bsdf::surface::layers(&case.i, &case.f, case.wo);
        let a = s.transmission.roughness;
        let alpha = if is_delta(a) { 1.0 } else { a[0].min(a[1]) };
        conditioning_t(alpha, case.wo, wi, case.f.n, s.transmission.ior)
    } else {
        conditioning(min_nondelta_alpha(case), case.wo, wi, case.f.n)
    }
}

/// Above this condition number an f32 evaluation carries no significant digits on either
/// side (plan §3 asks for the maximum errors; these cases are counted, not compared).
const ILL_CONDITIONED: f64 = 0.1;

fn lobe_alphas(case: &Case) -> [([f32; 2], Lobe); 4] {
    let s = standard_surface_bsdf::surface::layers(&case.i, &case.f, case.wo);
    [
        (s.coat.roughness, Lobe::Coat),
        (s.specular.roughness, Lobe::Specular),
        (s.metal.roughness, Lobe::Metal),
        (s.transmission.roughness, Lobe::Transmission),
    ]
}

/// The smallest non-delta alpha of the reflection lobes (the ones above the surface).
fn min_nondelta_alpha(case: &Case) -> f32 {
    lobe_alphas(case)
        .iter()
        .filter(|(a, l)| !is_delta(*a) && *l != Lobe::Transmission)
        .map(|(a, _)| a[0].min(a[1]))
        .fold(1.0, f32::min)
}

fn sampled_lobe_alpha(case: &Case, lobe: usize) -> f32 {
    lobe_alphas(case)
        .iter()
        .find(|(_, l)| l.index() == lobe)
        .map(|(a, _)| a[0].min(a[1]))
        .unwrap_or(1.0)
}

/// FNV-1a over the bit patterns of `values`.
fn fnv1a_bits(values: &[f32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for v in values {
        for b in v.to_bits().to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

#[test]
fn parity_module_validates_with_naga() {
    let source = parity_source();
    let module = match naga::front::wgsl::parse_str(&source) {
        Ok(m) => m,
        Err(e) => panic!("{}", e.emit_to_string(&source)),
    };
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    if let Err(e) = validator.validate(&module) {
        panic!("{}", e.emit_to_string(&source));
    }
    assert_eq!(module.entry_points.len(), 1);
    // The packed layouts agree with the WGSL entry.
    assert!(PARITY_ENTRY.contains("const PARITY_IN_STRIDE: u32 = 83u;"));
    assert!(PARITY_ENTRY.contains("const PARITY_OUT_STRIDE: u32 = 67u;"));
}

#[test]
#[ignore = "requires a wgpu compute adapter"]
fn cpu_and_wgsl_twins_agree_on_the_grid_and_random_cases() {
    let mut cases = grid_cases();
    let grid = cases.len();
    cases.extend(random_cases(4096, false));
    cases.extend(random_cases(4096, true));
    let without_subsurface = cases.len();
    cases.extend(subsurface_cases(4096));
    println!(
        "{grid} grid cases + 4096 random opaque + 4096 random transmissive + 4096 random \
         subsurface cases"
    );
    let gpu = run_gpu(&cases);
    assert_eq!(gpu.len(), cases.len() * OUT_STRIDE);
    // Bit hashes of the first `HASHED` outputs of the cases without subsurface. They were the
    // same before the subsurface closure existed (Q6 follow-up A, 2026-09-25: cpu
    // 25d90b90737050a1, gpu 896a71eb21ec1c43 on RTX 3080 Ti / NVIDIA 616.64 Vulkan, Windows),
    // which proves that `subsurface = 0` leaves every output bit-identical; the CPU hash is
    // same-OS (libm). 207473e (smooth interfaces conserve energy, E + T = 1) changed the
    // outputs on purpose: cpu 73a23ed129d725a1, gpu 8389f7a75b57aa24 (same machine, measured
    // 2026-10-11 at 207473e and at each later commit). Outputs appended later (the Sobol probes
    // at 62..64, `guide_albedo` at 64..67) are outside the hash; `check_all` compares them.
    const HASHED: usize = 62;
    let cpu: Vec<f32> = cases[..without_subsurface]
        .iter()
        .enumerate()
        .flat_map(|(k, c)| cpu_outputs(c, (k % 8192) as u32)[..HASHED].to_vec())
        .collect();
    let gpu_hashed: Vec<f32> = gpu[..without_subsurface * OUT_STRIDE]
        .as_chunks::<OUT_STRIDE>()
        .0
        .iter()
        .flat_map(|o| o[..HASHED].iter().copied())
        .collect();
    let cpu_hash = fnv1a_bits(&cpu);
    let gpu_hash = fnv1a_bits(&gpu_hashed);
    println!("output bit hashes without subsurface: cpu {cpu_hash:016x} gpu {gpu_hash:016x}");
    if cfg!(windows) {
        assert_eq!(
            cpu_hash, 0x73a2_3ed1_29d7_25a1,
            "CPU outputs of the subsurface-free cases changed"
        );
    }
    assert!(
        check_all(&cases, &gpu, 8192),
        "CPU/WGSL parity violations (see table)"
    );
}
