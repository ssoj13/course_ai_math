//! CPU oracles of the layered surface: hemisphere rule, MaterialX limits, the `Lobes`
//! partition against a literal layer/mix graph, delta-lobe semantics, disc widening, emission,
//! selection weights, validation, finiteness and energy bounds. Deterministic grids only.

mod common;

use common::{
    FRAME, bits3, dir, finite3, fresnel_dielectric_f64, hemisphere_integral, lum, max_rel_diff3,
    non_negative3, rich, white,
};
use standard_surface_bsdf::consts::{SS_ALPHA_MIN, SS_LOBE_COUNT};
use standard_surface_bsdf::diffuse::mx_oren_nayar_diffuse_bsdf_reflection;
use standard_surface_bsdf::fresnel::mx_fresnel_conductor;
use standard_surface_bsdf::math::MxBsdf;
use standard_surface_bsdf::microfacet::{
    mx_conductor_bsdf_reflection, mx_dielectric_bsdf_reflection, mx_orthonormal_basis,
};
use standard_surface_bsdf::sheen::mx_sheen_bsdf_reflection;
use standard_surface_bsdf::surface::{
    layers, mx_layer_bsdf, mx_mix_bsdf, mx_multiply_bsdf_color3, shading_tangent,
};
use standard_surface_bsdf::{
    Environment, InputsError, Lobe, Lobes, ShadingFrame, SurfaceInputs, dominant_dir,
    eval_emission, eval_environment, eval_light, eval_light_disc, lobe_weights,
};

const ENV_WHITE: Environment = Environment {
    irradiance: [1.0; 3],
    specular_radiance: [1.0; 3],
    coat_radiance: [1.0; 3],
    transmission_radiance: [1.0; 3],
};

#[test]
fn eval_light_is_exactly_zero_outside_the_hemisphere() {
    let i = rich();
    let cosines: [f64; 8] = [-1.0, -0.5, -1e-3, 0.0, 1e-3, 0.2, 0.6, 1.0];
    let mut inside = 0;
    for &cv in &cosines {
        for &cl in &cosines {
            for phi in [0.0, 1.3, 3.0] {
                let wo = dir(cv.abs(), 0.0);
                let wo = [wo[0], wo[1], if cv < 0.0 { -wo[2] } else { wo[2] }];
                let wi = dir(cl.abs(), phi);
                let wi = [wi[0], wi[1], if cl < 0.0 { -wi[2] } else { wi[2] }];
                let l = eval_light(&i, &FRAME, wo, wi);
                if cv > 0.0 && cl > 0.0 {
                    inside += 1;
                    assert!(lum(l.sum()) > 0.0, "cv {cv} cl {cl}: {l:?}");
                } else {
                    assert_eq!(l, Lobes::ZERO, "cv {cv} cl {cl}");
                }
            }
        }
    }
    assert!(inside > 0);
}

#[test]
fn coat_zero_is_the_identity() {
    let base = SurfaceInputs {
        coat: 0.0,
        ..rich()
    };
    let other = SurfaceInputs {
        coat_color: [0.1, 0.2, 0.3],
        coat_roughness: 0.9,
        coat_ior: 2.5,
        coat_anisotropy: 0.9,
        coat_rotation: 0.7,
        coat_affect_color: 1.0,
        coat_affect_roughness: 1.0,
        ..base
    };
    let wo = dir(0.7, 0.3);
    let wi = dir(0.5, 2.0);
    let a = eval_light(&base, &FRAME, wo, wi);
    let b = eval_light(&other, &FRAME, wo, wi);
    assert!(lum(a.sum()) > 0.0);
    assert_eq!(bits3(a.base), bits3(b.base));
    assert_eq!(bits3(a.specular), bits3(b.specular));
    let env = Environment {
        irradiance: [0.3, 0.5, 0.7],
        specular_radiance: [1.0, 0.8, 0.6],
        coat_radiance: [9.0, 9.0, 9.0],
        transmission_radiance: [9.0, 9.0, 9.0],
    };
    let ea = eval_environment(&base, &FRAME, wo, &env);
    let eb = eval_environment(&other, &FRAME, wo, &env);
    assert_eq!(bits3(ea.sum()), bits3(eb.sum()));
    assert_eq!(
        bits3(eval_emission(&base, &FRAME, wo)),
        bits3(eval_emission(&other, &FRAME, wo))
    );
}

#[test]
fn metalness_one_has_no_base_lobe() {
    let i = SurfaceInputs {
        metalness: 1.0,
        ..rich()
    };
    for (cv, cl) in [(0.9, 0.9), (0.3, 0.8), (0.6, 0.1)] {
        let l = eval_light(&i, &FRAME, dir(cv, 0.0), dir(cl, 2.5));
        assert_eq!(l.base, [0.0; 3]);
        assert!(lum(l.specular) > 0.0);
        let e = eval_environment(&i, &FRAME, dir(cv, 0.0), &ENV_WHITE);
        assert_eq!(e.base, [0.0; 3]);
        assert!(lum(e.specular) > 0.0);
    }
}

#[test]
fn zero_film_thickness_ignores_the_film_ior() {
    let wo = dir(0.6, 0.0);
    let wi = dir(0.8, 2.8);
    let at = |thickness: f32, film_ior: f32| {
        let i = SurfaceInputs {
            thin_film_thickness: thickness,
            thin_film_ior: film_ior,
            ..rich()
        };
        (
            eval_light(&i, &FRAME, wo, wi).sum(),
            eval_environment(&i, &FRAME, wo, &ENV_WHITE).sum(),
        )
    };
    let (l0, e0) = at(0.0, 1.0);
    for film_ior in [1.3, 1.5, 3.0] {
        let (l, e) = at(0.0, film_ior);
        assert_eq!(bits3(l), bits3(l0));
        assert_eq!(bits3(e), bits3(e0));
    }
    // Positive control: a real film changes the result.
    let (l1, _) = at(400.0, 1.3);
    let (l2, _) = at(400.0, 2.0);
    assert_ne!(bits3(l1), bits3(l2));
}

/// The literal MaterialX node graph (`standard_surface.mtlx:215-362` with transmission and
/// subsurface off) composed from the closure functions.
fn nested_graph(i: &SurfaceInputs, wo: [f32; 3], wi: [f32; 3]) -> [f32; 3] {
    let s = layers(i, &FRAME, wo);
    let n = FRAME.n;
    let coat = mx_dielectric_bsdf_reflection(wo, wi, n, s.coat_tangent, &s.coat, 0.0);
    let spec = mx_dielectric_bsdf_reflection(wo, wi, n, s.main_tangent, &s.specular, 0.0);
    let metal = mx_conductor_bsdf_reflection(wo, wi, n, s.main_tangent, &s.metal, 0.0);
    let sheen = mx_sheen_bsdf_reflection(wo, wi, n, i.sheen, i.sheen_color, i.sheen_roughness);
    let diffuse = mx_oren_nayar_diffuse_bsdf_reflection(
        wo,
        wi,
        n,
        i.base,
        s.diffuse_color,
        i.diffuse_roughness,
        false,
    );
    let sheen_layer = mx_layer_bsdf(sheen, diffuse);
    let specular_layer = mx_layer_bsdf(spec, sheen_layer);
    let metalness_mix = mx_mix_bsdf(metal, specular_layer, i.metalness);
    let attenuation = i.coat_color.map(|c| 1.0 * (1.0 - i.coat) + c * i.coat);
    let attenuated = mx_multiply_bsdf_color3(metalness_mix, attenuation);
    let coat_layer: MxBsdf = mx_layer_bsdf(coat, attenuated);
    coat_layer.response
}

#[test]
fn lobes_partition_equals_the_nested_layer_graph() {
    let mut max_rel = 0.0_f64;
    let mut total = 0.0_f64;
    let configs = [
        rich(),
        SurfaceInputs::MATERIALX_DEFAULT,
        SurfaceInputs {
            metalness: 1.0,
            ..rich()
        },
        SurfaceInputs {
            coat: 1.0,
            coat_color: [0.4, 0.6, 0.9],
            ..rich()
        },
        white(|i| {
            i.sheen = 1.0;
            i.coat = 0.5;
        }),
    ];
    for i in &configs {
        for cv in [0.05, 0.3, 0.7, 1.0] {
            for cl in [0.05, 0.4, 0.9] {
                for phi in [0.0, 1.0, 2.5, 3.1] {
                    let wo = dir(cv, 0.2);
                    let wi = dir(cl, phi);
                    let got = eval_light(i, &FRAME, wo, wi).sum();
                    let want = nested_graph(i, wo, wi);
                    total += lum(got);
                    max_rel = max_rel.max(max_rel_diff3(got, want));
                }
            }
        }
    }
    assert!(max_rel < 1e-5, "partition vs nested graph: {max_rel:e}");
    assert!(total > 0.0, "the compared values must be non-zero");
}

/// f64 expected coat throughput `1 - E(NdotV, F0) comp` of a delta coat (alpha = 1e-8).
/// What a smooth coat passes below it: `1 - F(NdotV)` with the exact Fresnel, the complement of
/// what its delta mirror reflects (`exact_interface`).
fn delta_coat_throughput_f64(ndotv: f64, ior: f64) -> f64 {
    1.0 - fresnel_dielectric_f64(ndotv, ior)
}

#[test]
fn delta_coat_over_white_diffuse_keeps_its_throughput() {
    // Delta coat 1 (roughness 0) over a white Lambert base; no dielectric specular.
    let i = white(|i| {
        i.specular = 0.0;
        i.coat = 1.0;
        i.coat_roughness = 0.0;
    });
    for cv in [0.05, 0.3, 0.6, 1.0] {
        let wo = dir(cv, 0.0);
        let t = delta_coat_throughput_f64(cv, 1.5);
        // Environment: the base-lobe energy is exactly the coat throughput.
        let only_irradiance = Environment {
            irradiance: [1.0; 3],
            specular_radiance: [0.0; 3],
            coat_radiance: [0.0; 3],
            transmission_radiance: [0.0; 3],
        };
        let e = eval_environment(&i, &FRAME, wo, &only_irradiance);
        for c in e.base {
            assert!(
                (f64::from(c) - t).abs() < 1e-5,
                "NdotV {cv}: base {c} vs 1 - F {t}"
            );
        }
        assert_eq!(e.specular, [0.0; 3]);
        // Direct light: the delta coat has no response, the diffuse is attenuated by it.
        for (cl, phi) in [(0.9, 0.0), (0.5, 3.0), (cv, std::f64::consts::PI)] {
            let wi = dir(cl, phi);
            let l = eval_light(&i, &FRAME, wo, wi);
            assert_eq!(l.specular, [0.0; 3], "delta coat response must be zero");
            let lambert = f64::from(wi[2]) / std::f64::consts::PI;
            for c in l.base {
                assert!(
                    (f64::from(c) - t * lambert).abs() < 1e-6,
                    "NdotV {cv}: {c} vs {}",
                    t * lambert
                );
            }
        }
        // The delta coat keeps its selection weight.
        let w = lobe_weights(&i, &FRAME, wo);
        assert!(w[Lobe::Coat.index()] > 0.0 && w[Lobe::Diffuse.index()] > 0.0);
    }
}

#[test]
fn delta_threshold_is_alpha_min() {
    // alpha = roughness^2: just below and just above SS_ALPHA_MIN.
    let below = (SS_ALPHA_MIN * 0.96).sqrt();
    let above = (SS_ALPHA_MIN * 1.04).sqrt();
    let wo = dir(1.0, 0.0);
    let wi = dir(1.0, 0.0);
    let metal = |r: f32| {
        white(|i| {
            i.metalness = 1.0;
            i.specular_roughness = r;
            i.base_color = [0.9; 3];
        })
    };
    assert_eq!(eval_light(&metal(below), &FRAME, wo, wi), Lobes::ZERO);
    assert_eq!(eval_light(&metal(0.0), &FRAME, wo, wi), Lobes::ZERO);
    let rough = eval_light(&metal(above), &FRAME, wo, wi);
    assert!(lum(rough.specular) > 0.0 && finite3(rough.specular));
    // The environment path stays finite and non-zero for a delta metal.
    let e = eval_environment(&metal(0.0), &FRAME, wo, &ENV_WHITE);
    assert!(lum(e.specular) > 0.5 && finite3(e.specular));
}

#[test]
fn disc_widening_lets_a_mirror_see_the_sun() {
    // Plan §4 C2b/R1: roughness 0 metal, sun of 0.53 deg diameter, normal incidence:
    // radiance = response * L * Omega must be F(1) * L within 10%.
    let half_angle = (0.53_f64 / 2.0).to_radians();
    let omega = 2.0 * std::f64::consts::PI * (1.0 - half_angle.cos());
    let i = white(|i| {
        i.metalness = 1.0;
        i.specular_roughness = 0.0;
        i.base_color = [0.9, 0.6, 0.3];
    });
    let n = FRAME.n;
    let l = eval_light_disc(&i, &FRAME, n, n, half_angle as f32);
    let s = layers(&i, &FRAME, n);
    let f = mx_fresnel_conductor(1.0, s.metal.ior_n, s.metal.ior_k);
    for (c, f) in l.specular.iter().zip(f) {
        let radiance = f64::from(*c) * omega;
        let ratio = radiance / f64::from(f);
        assert!((ratio - 1.0).abs() < 0.1, "mirror sun ratio {ratio}");
    }
    // A point light is never seen in a mirror.
    assert_eq!(eval_light(&i, &FRAME, n, n), Lobes::ZERO);
    // half_angle = 0 is bit-identical to eval_light (rough material).
    let r = rich();
    let (wo, wi) = (dir(0.7, 0.0), dir(0.6, 2.9));
    let a = eval_light(&r, &FRAME, wo, wi);
    let b = eval_light_disc(&r, &FRAME, wo, wi, 0.0);
    assert_eq!(
        (bits3(a.base), bits3(a.specular)),
        (bits3(b.base), bits3(b.specular))
    );
    // Widening a rough lobe by a small disc barely changes it.
    let c = eval_light_disc(&r, &FRAME, wo, wi, half_angle as f32);
    assert!(max_rel_diff3(a.sum(), c.sum()) < 1e-3);
}

#[test]
fn emission_follows_the_materialx_edf() {
    let i = SurfaceInputs {
        emission: 2.0,
        emission_color: [1.0, 0.5, 0.25],
        coat: 0.0,
        ..rich()
    };
    let e = [2.0, 1.0, 0.5];
    assert_eq!(eval_emission(&i, &FRAME, dir(0.4, 0.0)), e);
    let coated = SurfaceInputs {
        coat: 1.0,
        coat_color: [0.5, 1.0, 1.0],
        coat_ior: 1.5,
        ..i
    };
    // Normal incidence: E coat_color (1 - F0) with F0 = 0.04.
    let normal = eval_emission(&coated, &FRAME, [0.0, 0.0, 1.0]);
    let want = [2.0 * 0.5 * 0.96, 0.96, 0.5 * 0.96];
    for (g, w) in normal.iter().zip(want) {
        assert!((f64::from(*g) - w).abs() < 1e-6, "{normal:?}");
    }
    // Grazing: (1 - (1 - NdotV)^5) -> small.
    let grazing = eval_emission(&coated, &FRAME, dir(1e-3, 0.0));
    assert!(lum(grazing) < 0.01 && lum(grazing) >= 0.0);
}

#[test]
fn lobe_weights_are_normalised_and_single_lobe_materials_select_their_lobe() {
    for i in [
        rich(),
        SurfaceInputs::MATERIALX_DEFAULT,
        white(|i| i.sheen = 1.0),
    ] {
        for cv in [0.02, 0.4, 1.0] {
            let w = lobe_weights(&i, &FRAME, dir(cv, 0.0));
            let sum: f32 = w.iter().sum();
            assert!(
                (sum - 1.0).abs() < 1e-6 && w.iter().all(|p| *p >= 0.0),
                "{w:?}"
            );
        }
    }
    let wo = dir(0.6, 0.0);
    let only = |f: fn(&mut SurfaceInputs)| {
        let mut i = SurfaceInputs {
            base: 0.0,
            specular: 0.0,
            ..SurfaceInputs::MATERIALX_DEFAULT
        };
        f(&mut i);
        lobe_weights(&i, &FRAME, wo)
    };
    let diffuse = only(|i| i.base = 1.0);
    assert_eq!(diffuse[Lobe::Diffuse.index()], 1.0);
    let metal = only(|i| i.metalness = 1.0);
    assert_eq!(metal[Lobe::Metal.index()], 1.0);
    let specular = only(|i| i.specular = 1.0);
    assert_eq!(specular[Lobe::Specular.index()], 1.0);
    let sheen = only(|i| i.sheen = 1.0);
    assert_eq!(sheen[Lobe::Sheen.index()], 1.0);
    let coat = only(|i| i.coat = 1.0);
    assert_eq!(coat[Lobe::Coat.index()], 1.0);
    let transmission = only(|i| {
        i.specular = 0.0;
        i.transmission = 1.0;
    });
    assert_eq!(transmission[Lobe::Transmission.index()], 1.0);
    assert_eq!(only(|_| {}), [0.0; SS_LOBE_COUNT as usize]);
    let order: Vec<usize> = Lobe::ALL.iter().map(|l| l.index()).collect();
    assert_eq!(order, [0, 1, 2, 3, 4, 5]);
}

#[test]
fn validate_rejects_out_of_domain_inputs() {
    assert_eq!(SurfaceInputs::MATERIALX_DEFAULT.validate(), Ok(()));
    assert_eq!(rich().validate(), Ok(()));
    let d = SurfaceInputs::MATERIALX_DEFAULT;
    let cases: [(SurfaceInputs, &str); 8] = [
        (
            SurfaceInputs {
                base: f32::NAN,
                ..d
            },
            "base",
        ),
        (
            SurfaceInputs {
                base_color: [0.5, -0.1, 0.5],
                ..d
            },
            "base_color.g",
        ),
        (
            SurfaceInputs {
                specular_ior: 0.9,
                ..d
            },
            "specular_IOR",
        ),
        (SurfaceInputs { coat: 1.5, ..d }, "coat"),
        (
            SurfaceInputs {
                thin_film_thickness: -1.0,
                ..d
            },
            "thin_film_thickness",
        ),
        (
            SurfaceInputs {
                emission: f32::INFINITY,
                ..d
            },
            "emission",
        ),
        (
            SurfaceInputs {
                specular_roughness: 1.01,
                ..d
            },
            "specular_roughness",
        ),
        (
            SurfaceInputs {
                coat_color: [0.0, 0.0, f32::NAN],
                ..d
            },
            "coat_color.b",
        ),
    ];
    for (i, name) in cases {
        match i.validate() {
            Err(InputsError::NonFinite { input, .. } | InputsError::OutOfRange { input, .. }) => {
                assert_eq!(input, name)
            }
            Ok(()) => panic!("{name} accepted"),
        }
    }
    let names: Vec<&str> = d.domain().iter().map(|(n, _, _)| *n).collect();
    assert_eq!(names.len(), 49);
    assert!(names.contains(&"specular_IOR") && names.contains(&"emission_color.b"));
    assert!(names.contains(&"transmission_extra_roughness"));
    assert!(names.contains(&"subsurface_radius.b") && names.contains(&"subsurface_anisotropy"));
    let backward = SurfaceInputs {
        subsurface_anisotropy: -1.0,
        ..d
    };
    assert_eq!(backward.validate(), Ok(()));
    let negative_radius = SurfaceInputs {
        subsurface_radius: [1.0, -0.1, 1.0],
        ..d
    };
    assert!(negative_radius.validate().is_err());
    let signed = SurfaceInputs {
        transmission_extra_roughness: -1.0,
        ..d
    };
    assert_eq!(signed.validate(), Ok(()));
    let below = SurfaceInputs {
        transmission_extra_roughness: -1.01,
        ..d
    };
    assert!(below.validate().is_err());
    let message = SurfaceInputs { coat: 1.5, ..d }
        .validate()
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(
        message.contains("coat") && message.contains("1.5"),
        "{message}"
    );
}

#[test]
fn every_entry_point_is_finite_on_the_domain_corners() {
    let mut count = 0usize;
    for roughness in [0.0_f32, 0.01, 0.05, 0.2, 1.0] {
        for anisotropy in [0.0_f32, 0.98] {
            for ior in [1.0_f32, 1.5, 5.0] {
                for thickness in [0.0_f32, 400.0, 2000.0] {
                    for (coat, metal, weight) in [(0.0, 0.0, 1.0), (1.0, 1.0, 0.0), (1.0, 0.0, 1.0)]
                    {
                        let i = SurfaceInputs {
                            base: weight,
                            base_color: [1.0, 0.0, 0.5],
                            diffuse_roughness: roughness,
                            metalness: metal,
                            specular: weight,
                            specular_roughness: roughness,
                            specular_ior: ior,
                            specular_anisotropy: anisotropy,
                            sheen: weight,
                            sheen_roughness: roughness,
                            coat,
                            coat_roughness: roughness,
                            coat_anisotropy: anisotropy,
                            coat_ior: ior,
                            coat_affect_color: 1.0,
                            coat_affect_roughness: 1.0,
                            thin_film_thickness: thickness,
                            thin_film_ior: ior,
                            emission: 1.0,
                            ..SurfaceInputs::MATERIALX_DEFAULT
                        };
                        assert_eq!(i.validate(), Ok(()));
                        for cv in [1e-3, 0.1, 0.5, 1.0] {
                            let wo = dir(cv, 0.3);
                            for (cl, phi) in [(1e-3, 0.0), (0.3, 1.5), (0.9, 3.1), (1.0, 0.0)] {
                                let wi = dir(cl, phi);
                                for l in [
                                    eval_light(&i, &FRAME, wo, wi),
                                    eval_light_disc(&i, &FRAME, wo, wi, 0.1),
                                    eval_environment(&i, &FRAME, wo, &ENV_WHITE),
                                ] {
                                    assert!(
                                        finite3(l.base)
                                            && finite3(l.specular)
                                            && non_negative3(l.base)
                                            && non_negative3(l.specular),
                                        "{i:?} cv {cv} cl {cl}: {l:?}"
                                    );
                                    count += usize::from(lum(l.sum()) > 0.0);
                                }
                            }
                            let e = eval_emission(&i, &FRAME, wo);
                            assert!(finite3(e) && non_negative3(e));
                            let w = lobe_weights(&i, &FRAME, wo);
                            assert!(w.iter().all(|p| p.is_finite() && *p >= 0.0), "{w:?}");
                        }
                    }
                }
            }
        }
    }
    assert!(count > 1000, "only {count} non-zero evaluations");
}

#[test]
fn white_furnace_of_the_environment_path_is_bounded() {
    // irradiance = radiance = 1: the layered albedo stays at or below 1 + 0.03 without a film.
    let mut worst = (0.0_f64, String::new());
    let mut worst_film = 0.0_f64;
    let mut darkest = f64::MAX;
    let configs: [(&str, SurfaceInputs); 5] = [
        ("dielectric", white(|_| {})),
        ("metal", white(|i| i.metalness = 1.0)),
        ("coat", white(|i| i.coat = 1.0)),
        ("sheen", white(|i| i.sheen = 1.0)),
        (
            "all",
            white(|i| {
                i.coat = 1.0;
                i.sheen = 1.0;
                i.metalness = 0.5;
            }),
        ),
    ];
    for (name, base) in &configs {
        for r in [0.0_f32, 0.1, 0.3, 0.6, 1.0] {
            for cv in [0.02, 0.2, 0.5, 0.8, 1.0] {
                let wo = dir(cv, 0.0);
                for film in [0.0_f32, 400.0] {
                    let i = SurfaceInputs {
                        specular_roughness: r,
                        coat_roughness: r,
                        diffuse_roughness: r,
                        sheen_roughness: r.max(0.05),
                        thin_film_thickness: film,
                        ..*base
                    };
                    let e = eval_environment(&i, &FRAME, wo, &ENV_WHITE).sum();
                    let max = e.iter().copied().fold(0.0_f32, f32::max) as f64;
                    darkest = darkest.min(lum(e));
                    if film == 0.0 {
                        if max > worst.0 {
                            worst = (max, format!("{name} r {r} NdotV {cv}"));
                        }
                    } else {
                        worst_film = worst_film.max(max);
                    }
                }
            }
        }
    }
    println!(
        "environment white furnace: max {:.5} ({}); with a 400 nm film max {:.5}",
        worst.0, worst.1, worst_film
    );
    assert!(worst.0 <= 1.03, "furnace {worst:?}");
    // Lower bound: only Oren-Nayar at roughness 1 (albedo ~0.65) loses energy by design.
    println!("environment white furnace: min {darkest:.5}");
    assert!(darkest > 0.6, "furnace minimum {darkest}");
}

#[test]
fn integrated_direct_light_matches_the_environment_path() {
    // Quadrature of eval_light over the hemisphere (uniform radiance 1) vs eval_environment with
    // irradiance = radiance = 1: the difference is the MaterialX albedo-fit error (analytic GGX
    // albedo vs the exact-Fresnel lobe integral). Rough lobes only (the grid resolves them).
    let mut worst = 0.0_f64;
    let mut worst_total = 0.0_f64;
    let mut least_total = f64::MAX;
    let configs: [SurfaceInputs; 4] = [
        white(|_| {}),
        white(|i| i.metalness = 1.0),
        white(|i| {
            i.coat = 1.0;
            i.coat_roughness = 0.5;
        }),
        white(|i| i.sheen = 1.0),
    ];
    for base in &configs {
        for r in [0.4_f32, 0.7, 1.0] {
            for cv in [0.4, 1.0] {
                let i = SurfaceInputs {
                    specular_roughness: r,
                    ..*base
                };
                let wo = dir(cv, 0.0);
                let direct =
                    hemisphere_integral(256, 128, |wi| lum(eval_light(&i, &FRAME, wo, wi).sum()));
                let env = lum(eval_environment(&i, &FRAME, wo, &ENV_WHITE).sum());
                worst = worst.max((direct - env).abs());
                worst_total = worst_total.max(direct);
                least_total = least_total.min(direct);
            }
        }
    }
    println!(
        "direct-light quadrature vs environment path: max |diff| {worst:.5}, max integral {worst_total:.5}"
    );
    assert!(worst_total <= 1.03, "direct white furnace {worst_total}");
    assert!(
        least_total > 0.6,
        "direct white furnace minimum {least_total}"
    );
    assert!(worst < 0.05, "direct vs environment {worst}");
}

#[test]
fn anisotropy_axis_follows_the_rotation() {
    let n = [0.0_f32, 0.0, 1.0];
    // Degenerate tangent (parallel to n) falls back to the Duff basis.
    let [bx, _, _] = mx_orthonormal_basis(n);
    assert_eq!(shading_tangent(n, n, 0.0, 0.0), bx);
    // Isotropic: no rotation, just the projection.
    let t = shading_tangent(n, [2.0, 0.0, 5.0], 0.25, 0.0);
    assert!((t[0] - 1.0).abs() < 1e-7 && t[1].abs() < 1e-7 && t[2].abs() < 1e-7);
    // Anisotropic, a quarter turn (MaterialX's v * M convention: +X goes to -Y).
    let q = shading_tangent(n, [1.0, 0.0, 0.0], 0.25, 0.5);
    assert!(
        q[0].abs() < 1e-6 && (q[1] + 1.0).abs() < 1e-6 && q[2].abs() < 1e-6,
        "{q:?}"
    );
    // The highlight depends on the rotation only when the lobe is anisotropic.
    let wo = dir(0.7, 0.0);
    let wi = dir(0.7, std::f64::consts::PI + 0.4);
    let eval = |aniso: f32, rotation: f32| {
        let i = white(|i| {
            i.base = 0.0;
            i.specular_roughness = 0.3;
            i.specular_anisotropy = aniso;
            i.specular_rotation = rotation;
        });
        eval_light(&i, &FRAME, wo, wi).specular
    };
    assert_eq!(bits3(eval(0.0, 0.0)), bits3(eval(0.0, 0.3)));
    assert_ne!(bits3(eval(0.8, 0.0)), bits3(eval(0.8, 0.3)));
}

#[test]
fn dominant_direction_limits() {
    let f = ShadingFrame {
        n: [0.0, 0.0, 1.0],
        tangent: [1.0, 0.0, 0.0],
        inside: false,
        curvature: 0.0,
    };
    let wo = dir(0.6, 0.5);
    let r = [-wo[0], -wo[1], wo[2]];
    let d0 = dominant_dir(&f, wo, 0.0);
    assert!(max_rel_diff3(d0, r) < 1e-6 || d0.iter().zip(r).all(|(a, b)| (a - b).abs() < 1e-6));
    let d1 = dominant_dir(&f, wo, 1.0);
    assert!(
        d1.iter().zip(f.n).all(|(a, b)| (a - b).abs() < 1e-6),
        "{d1:?}"
    );
    let half = dominant_dir(&f, wo, 0.5);
    assert!(half[2] > r[2] && half[2] < 1.0);
}

#[test]
fn materialx_raster_variant_keeps_the_clamped_delta_response() {
    use standard_surface_bsdf::eval_light_materialx;
    // Non-delta materials: identical to eval_light.
    let r = rich();
    for (cv, cl, phi) in [(0.7, 0.6, 2.9), (0.2, 0.9, 0.4)] {
        let (wo, wi) = (dir(cv, 0.0), dir(cl, phi));
        let a = eval_light(&r, &FRAME, wo, wi);
        let b = eval_light_materialx(&r, &FRAME, wo, wi);
        assert_eq!(
            (bits3(a.base), bits3(a.specular)),
            (bits3(b.base), bits3(b.specular))
        );
    }
    // A roughness-0 metal: eval_light is zero, the MaterialX variant keeps its finite spike at
    // the mirror direction and is (numerically) zero away from it.
    let metal = white(|i| {
        i.metalness = 1.0;
        i.specular_roughness = 0.0;
    });
    let n = FRAME.n;
    assert_eq!(eval_light(&metal, &FRAME, n, n), Lobes::ZERO);
    let spike = eval_light_materialx(&metal, &FRAME, n, n);
    assert!(
        finite3(spike.specular) && lum(spike.specular) > 1e10,
        "{spike:?}"
    );
    let off = eval_light_materialx(&metal, &FRAME, dir(0.7, 0.0), dir(0.6, 2.0));
    assert!(lum(off.specular) < 1e-6);
    // The hemisphere rule still holds.
    let below = [0.0_f32, 0.6, -0.8];
    assert_eq!(eval_light_materialx(&metal, &FRAME, n, below), Lobes::ZERO);
}
