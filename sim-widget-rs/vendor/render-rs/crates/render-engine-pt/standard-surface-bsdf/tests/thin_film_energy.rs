//! Task R2b: the explicit thin-film energy option (plan TODO "found during R1"). `MaterialX`
//! (the default) keeps MaterialX 1.39.5 bit for bit, whose dielectric throughput ignores the
//! film (env white furnace 1.334 at 400 nm); `Conserving` takes the throughput from the
//! film-aware albedo `E(fd)`, so the layered furnace stays at 1 up to the albedo-fit error.

mod common;

use common::{FRAME, dir, lum, rich, white};
use standard_surface_bsdf::sample::{pdf, sample, sample_with};
use standard_surface_bsdf::sampling::rng;
use standard_surface_bsdf::{
    Environment, SurfaceInputs, ThinFilmEnergy, eval_emission, eval_environment, eval_light,
    eval_light_disc, lobe_weights,
};

const ENV: Environment = Environment {
    irradiance: [0.7, 0.8, 0.9],
    specular_radiance: [1.1, 0.9, 0.6],
    coat_radiance: [0.5, 0.6, 1.3],
    transmission_radiance: [0.4, 0.8, 1.2],
};

/// FNV-1a over the bits of every entry point on a grid of films, roughnesses and angles.
fn checksum(configure: impl Fn(SurfaceInputs) -> SurfaceInputs) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |x: f32| {
        h ^= u64::from(x.to_bits());
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    for thickness in [0.0_f32, 250.0, 400.0, 1200.0] {
        for roughness in [0.0_f32, 0.2, 0.6] {
            for metalness in [0.0_f32, 0.7] {
                let i = configure(SurfaceInputs {
                    thin_film_thickness: thickness,
                    thin_film_ior: 1.35,
                    specular_roughness: roughness,
                    coat_roughness: roughness,
                    metalness,
                    ..rich()
                });
                for c in [0.1_f64, 0.5, 0.95] {
                    let wo = dir(c, 0.3);
                    let wi = dir(0.7, 2.6);
                    let l = eval_light(&i, &FRAME, wo, wi);
                    let d = eval_light_disc(&i, &FRAME, wo, wi, 0.02);
                    let e = eval_environment(&i, &FRAME, wo, &ENV);
                    for v in [l.base, l.specular, d.base, d.specular, e.base, e.specular] {
                        v.into_iter().for_each(&mut feed);
                    }
                    eval_emission(&i, &FRAME, wo)
                        .into_iter()
                        .for_each(&mut feed);
                    // The R1/R2 weights; the transmission weight (Q6) is 0 for these opaque
                    // materials and is left out so the golden keeps its R1/R2 meaning.
                    let w = lobe_weights(&i, &FRAME, wo);
                    assert_eq!(w[5], 0.0);
                    w[..5].iter().copied().for_each(&mut feed);
                    feed(pdf(&i, &FRAME, wo, wi));
                    for u in [[0.1_f32, 0.2, 0.3], [0.5, 0.6, 0.7], [0.93, 0.4, 0.8]] {
                        let s = sample(&i, &FRAME, wo, u);
                        s.wi.into_iter().for_each(&mut feed);
                        s.weight.into_iter().for_each(&mut feed);
                        feed(s.pdf);
                    }
                }
            }
        }
    }
    h
}

#[test]
fn materialx_mode_is_bit_identical_to_r1_r2() {
    let got = checksum(|i| i);
    println!("MaterialX checksum {got:#018x}");
    #[cfg(not(feature = "cuda-math"))]
    assert_eq!(got, GOLDEN_MATERIALX, "MaterialX mode changed bits");
    // Native elementary functions have platform-specific final bits; keep the
    // default-mode semantics exact while the numerical oracles check accuracy.
    #[cfg(feature = "cuda-math")]
    assert_eq!(
        got,
        checksum(|i| SurfaceInputs {
            thin_film_energy: ThinFilmEnergy::MaterialX,
            ..i
        }),
        "Implicit and explicit MaterialX modes differ"
    );
}

/// Checksum of the R1/R2 code (render-rs 44fd77c + R2) before the option existed, updated once
/// for the smooth-interface energy fix (`exact_interface`): the roughness-0 rows of the grid
/// changed (delta mirrors reflect the exact uncompensated Fresnel, the throughput below is its
/// complement); the rough rows were verified bit-identical (rough-only checksum
/// 0x247e7c98e83df63d before and after).
#[cfg(not(feature = "cuda-math"))]
const GOLDEN_MATERIALX: u64 = 0x212b_c68f_ae47_b4fb;

fn conserving(i: SurfaceInputs) -> SurfaceInputs {
    SurfaceInputs {
        thin_film_energy: ThinFilmEnergy::Conserving,
        ..i
    }
}

const WHITE_ENV: Environment = Environment {
    irradiance: [1.0; 3],
    specular_radiance: [1.0; 3],
    coat_radiance: [1.0; 3],
    transmission_radiance: [1.0; 3],
};

#[test]
fn conserving_film_keeps_the_white_furnace_at_one() {
    // Environment furnace of white dielectric stacks (specular, coat, sheen over diffuse) with a
    // film: MaterialX gains energy (1.334 at 400 nm in R1); Conserving stays at or below
    // 1 + the albedo-fit error 0.03 (R1's film-free furnace maximum is 1.00000).
    let (mut worst_conserving, mut worst_materialx) = (0.0_f64, 0.0_f64);
    let mut worst_metal = (0.0_f64, 0.0_f64);
    for thickness in [120.0_f32, 250.0, 400.0, 800.0, 1200.0, 2000.0] {
        for film_ior in [1.3_f32, 1.5, 2.0] {
            for (metalness, coat, sheen, roughness) in [
                (0.0, 0.0, 0.0, 0.0),
                (0.0, 0.0, 0.0, 0.4),
                (0.0, 1.0, 0.0, 0.2),
                (0.0, 0.5, 1.0, 0.6),
                (1.0, 0.0, 0.0, 0.6),
                (0.5, 0.5, 0.0, 0.6),
            ] {
                let i = white(|i| {
                    i.thin_film_thickness = thickness;
                    i.thin_film_ior = film_ior;
                    i.metalness = metalness;
                    i.coat = coat;
                    i.sheen = sheen;
                    i.specular_roughness = roughness;
                });
                for c in [0.05_f64, 0.3, 0.7, 1.0] {
                    let wo = dir(c, 0.0);
                    let max = |i: &SurfaceInputs| {
                        let e = eval_environment(i, &FRAME, wo, &WHITE_ENV).sum();
                        f64::from(e.into_iter().fold(0.0_f32, f32::max))
                    };
                    let (m, k) = (max(&i), max(&conserving(i)));
                    if metalness > 0.0 {
                        worst_metal = (worst_metal.0.max(m), worst_metal.1.max(k));
                    } else {
                        worst_materialx = worst_materialx.max(m);
                        worst_conserving = worst_conserving.max(k);
                    }
                }
            }
        }
    }
    println!(
        "white furnace with a film, dielectric stacks: MaterialX max {worst_materialx:.5}, Conserving max {worst_conserving:.5}; with metal: MaterialX {:.5}, Conserving {:.5}",
        worst_metal.0, worst_metal.1
    );
    assert!(worst_materialx > 1.1, "MaterialX control {worst_materialx}");
    assert!(
        worst_conserving <= 1.03,
        "Conserving furnace {worst_conserving}"
    );
    // A conductor with a film (task R2c): MaterialX's `FG(fd) comp` alone exceeds 1 (1.133);
    // Conserving bounds the film albedo by the GGX single-scatter energy and stays at 1.
    assert!(
        worst_metal.0 > 1.05,
        "MaterialX metal film control {:?}",
        worst_metal
    );
    assert!(
        worst_metal.1 <= 1.03,
        "Conserving metal film furnace {:?}",
        worst_metal
    );
}

#[test]
fn conserving_film_albedo_respects_the_single_scatter_bound() {
    // R2c: for every film and roughness, the Conserving film albedo times the energy
    // compensation stays <= Ess + Fss (1 - Ess) <= 1 + fit overshoot, for a conductor and a
    // dielectric, and the dielectric throughput never goes negative.
    use standard_surface_bsdf::fresnel::{
        film_dir_albedo, mx_ggx_energy_compensation, mx_init_fresnel_conductor,
        mx_init_fresnel_dielectric,
    };
    let mut worst = (0.0_f32, 0.0_f32);
    for thickness in [100.0_f32, 300.0, 700.0, 2000.0] {
        for film_ior in [1.2_f32, 1.6, 2.5] {
            for fd in [
                mx_init_fresnel_conductor([0.1, 0.1, 0.1], [9.0, 9.0, 9.0], thickness, film_ior),
                mx_init_fresnel_conductor([0.2, 0.9, 1.1], [3.9, 2.4, 2.2], thickness, film_ior),
                mx_init_fresnel_dielectric(1.5, thickness, film_ior),
            ] {
                for alpha in [1e-4_f32, 0.01, 0.1, 0.36, 1.0] {
                    for c in [0.02_f32, 0.2, 0.5, 0.9, 1.0] {
                        let comp = mx_ggx_energy_compensation(c, alpha, &fd);
                        for energy in [ThinFilmEnergy::MaterialX, ThinFilmEnergy::Conserving] {
                            let fg = film_dir_albedo(c, alpha, &fd, energy);
                            let e = fg
                                .iter()
                                .zip(comp)
                                .map(|(a, b)| a * b)
                                .fold(0.0_f32, f32::max);
                            if energy == ThinFilmEnergy::MaterialX {
                                worst.0 = worst.0.max(e);
                            } else {
                                worst.1 = worst.1.max(e);
                            }
                        }
                    }
                }
            }
        }
    }
    println!(
        "film albedo x comp: MaterialX max {:.5}, Conserving max {:.5}",
        worst.0, worst.1
    );
    assert!(worst.0 > 1.05, "MaterialX control {}", worst.0);
    assert!(worst.1 <= 1.03, "Conserving film albedo x comp {}", worst.1);
}

#[test]
fn conserving_metal_film_direct_and_sampled_energy() {
    // The direct-light lobe of a white metal with a film integrates to at most 1 + fit error,
    // and the sampler (whose metal weight comes from the film albedo) matches the environment.
    let i = conserving(white(|i| {
        i.metalness = 1.0;
        i.thin_film_thickness = 250.0;
        i.thin_film_ior = 2.0;
        i.specular_roughness = 0.6;
    }));
    let wo = dir(0.7, 0.0);
    let direct =
        common::hemisphere_integral(256, 128, |wi| lum(eval_light(&i, &FRAME, wo, wi).sum()));
    let pre = lum(eval_environment(&i, &FRAME, wo, &WHITE_ENV).sum());
    println!("white metal 250 nm film: direct integral {direct:.5}, pre-integrated {pre:.5}");
    assert!(direct <= 1.03, "direct {direct}");
    assert!(pre <= 1.03, "pre-integrated {pre}");
    assert!(
        (direct - pre).abs() < 0.05,
        "direct {direct} vs environment {pre}"
    );
}

#[test]
fn conserving_equals_materialx_without_a_film() {
    let a = checksum(|i| SurfaceInputs {
        thin_film_thickness: 0.0,
        ..i
    });
    let b = checksum(|i| {
        conserving(SurfaceInputs {
            thin_film_thickness: 0.0,
            ..i
        })
    });
    assert_eq!(a, b);
    // Positive control: with a film the two modes differ.
    assert_ne!(checksum(|i| i), checksum(conserving));
}

#[test]
fn conserving_sampler_and_weights_follow_the_film_aware_throughput() {
    let i = conserving(white(|i| {
        i.thin_film_thickness = 400.0;
        i.thin_film_ior = 1.3;
        i.specular_roughness = 0.3;
    }));
    let materialx = SurfaceInputs {
        thin_film_energy: ThinFilmEnergy::MaterialX,
        ..i
    };
    let n = 1u32 << 16;
    for c in [0.3_f64, 0.8] {
        let wo = dir(c, 0.2);
        let w = lobe_weights(&i, &FRAME, wo);
        let wm = lobe_weights(&materialx, &FRAME, wo);
        assert_ne!(w, wm, "lobe weights must follow the film-aware throughput");
        let values: Vec<f64> = (0..n)
            .map(|k| {
                let u = [
                    rng(k, 7, 0, 0, 11),
                    rng(k, 7, 0, 1, 11),
                    rng(k, 7, 0, 2, 11),
                ];
                let s = sample_with(&i, &FRAME, wo, u, &w);
                if s.valid { lum(s.weight) } else { 0.0 }
            })
            .collect();
        let nf = f64::from(n);
        let mean = values.iter().sum::<f64>() / nf;
        let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (nf - 1.0);
        let se = (var / nf).sqrt();
        let pre = lum(eval_environment(&i, &FRAME, wo, &WHITE_ENV).sum());
        println!(
            "Conserving 400 nm NdotV {c}: E[weight] {mean:.5} +- {se:.5}, pre-integrated {pre:.5}"
        );
        assert!(mean <= 1.03 + 3.0 * se, "sampler furnace {mean}");
        assert!((mean - pre).abs() < 3.0 * se + 0.03, "{mean} vs {pre}");
    }
}

#[test]
fn conserving_is_finite_on_the_domain_corners() {
    let mut count = 0usize;
    for thickness in [0.0_f32, 50.0, 400.0, 2000.0] {
        for film_ior in [1.0_f32, 1.5, 5.0] {
            for roughness in [0.0_f32, 0.05, 1.0] {
                for ior in [1.0_f32, 1.5, 5.0] {
                    let i = conserving(SurfaceInputs {
                        thin_film_thickness: thickness,
                        thin_film_ior: film_ior,
                        specular_roughness: roughness,
                        specular_ior: ior,
                        coat: 1.0,
                        coat_ior: ior,
                        coat_roughness: roughness,
                        ..rich()
                    });
                    for c in [1e-3_f64, 0.4, 1.0] {
                        let wo = dir(c, 0.0);
                        let wi = dir(0.5, 2.0);
                        let l = eval_light(&i, &FRAME, wo, wi).sum();
                        let e = eval_environment(&i, &FRAME, wo, &WHITE_ENV).sum();
                        let w = lobe_weights(&i, &FRAME, wo);
                        let s = sample(&i, &FRAME, wo, [0.3, 0.6, 0.9]);
                        let all = l.into_iter().chain(e).chain(w).chain(s.weight);
                        for v in all {
                            assert!(v.is_finite() && v >= 0.0, "{i:?} c {c}: {v}");
                            count += usize::from(v > 0.0);
                        }
                    }
                }
            }
        }
    }
    assert!(count > 100);
}
