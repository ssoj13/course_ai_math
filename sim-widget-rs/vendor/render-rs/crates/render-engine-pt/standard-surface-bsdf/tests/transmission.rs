//! CPU oracles of the transmission lobe (render-rs Q6, `src/transmission.rs`): Snell refraction,
//! the density's normalisation, chi-square goodness of fit of the sampler against the density,
//! the BTDF's generalized reciprocity, sample/pdf/eval consistency, delta (smooth glass)
//! semantics, the white furnace outside and inside a glass object, and the opaque default.
//! Deterministic: every uniform comes from the counter RNG with fixed counters, no threads.

mod common;

use common::{FRAME, chi_square_p, dir, lum, mean_se, rich, u3};
use standard_surface_bsdf::consts::SS_LOBE_COUNT;
use standard_surface_bsdf::math::{cross3, dot3, scale3};
use standard_surface_bsdf::microfacet::MxDielectric;
use standard_surface_bsdf::sample::{pdf, sample, sample_with};
use standard_surface_bsdf::surface::{layers, transmission_tint};
use standard_surface_bsdf::transmission::{
    ggx_transmission, ggx_transmission_pdf, refract, transmission_is_delta,
};
use standard_surface_bsdf::{
    Lobe, Lobes, ShadingFrame, SurfaceInputs, ThinFilmEnergy, eval_light, lobe_weights,
};

use std::f64::consts::PI;

/// A white glass: transmission 1 under a white dielectric specular, no coat, no metal.
fn glass(roughness: f32, anisotropy: f32) -> SurfaceInputs {
    SurfaceInputs {
        base_color: [1.0; 3],
        specular: 1.0,
        specular_color: [1.0; 3],
        specular_roughness: roughness,
        specular_anisotropy: anisotropy,
        specular_ior: 1.5,
        transmission: 1.0,
        transmission_color: [1.0; 3],
        ..SurfaceInputs::MATERIALX_DEFAULT
    }
}

/// Transmission only (no specular layer above it): every sample comes from the BTDF.
fn bare_btdf(roughness: f32, anisotropy: f32) -> SurfaceInputs {
    SurfaceInputs {
        specular: 0.0,
        ..glass(roughness, anisotropy)
    }
}

/// A transmission closure of alpha pair `alpha` and relative IOR `eta` (weight and tint unused).
fn closure(alpha: [f32; 2], eta: f32) -> MxDielectric {
    MxDielectric {
        weight: 1.0,
        tint: [1.0; 3],
        ior: eta,
        roughness: alpha,
        thinfilm_thickness: 0.0,
        thinfilm_ior: 1.5,
        thin_film_energy: ThinFilmEnergy::MaterialX,
    }
}

const INSIDE: ShadingFrame = ShadingFrame {
    inside: true,
    ..FRAME
};

/// Midpoint quadrature of `f(wi) dw` over the lower hemisphere in `(cos theta, phi)`.
fn lower_hemisphere_integral(
    n_cos: usize,
    n_phi: usize,
    mut f: impl FnMut([f32; 3]) -> f64,
) -> f64 {
    let d_cos = 1.0 / n_cos as f64;
    let d_phi = 2.0 * PI / n_phi as f64;
    let mut sum = 0.0;
    for a in 0..n_cos {
        let c = (a as f64 + 0.5) * d_cos;
        for b in 0..n_phi {
            let phi = (b as f64 + 0.5) * d_phi;
            let w = dir(c, phi);
            sum += f([w[0], w[1], -w[2]]);
        }
    }
    sum * d_cos * d_phi
}

#[test]
fn refract_follows_snell_and_reports_total_internal_reflection() {
    let m = [0.0, 0.0, 1.0];
    for eta in [1.5_f32, 1.0 / 1.5, 1.33, 2.4] {
        for c in [1.0_f64, 0.9, 0.6, 0.3, 0.05] {
            let wo = dir(c, 0.7);
            let wi = refract(wo, m, eta);
            let sin_o = (1.0 - c * c).sqrt();
            let sin_i = sin_o / f64::from(eta);
            if sin_i >= 1.0 {
                assert_eq!(wi, [0.0; 3], "eta {eta} cos {c}: expected TIR");
                continue;
            }
            let len = f64::from(dot3(wi, wi)).sqrt();
            assert!((len - 1.0).abs() < 1e-6, "unit length {len}");
            assert!(wi[2] < 0.0, "below the interface");
            let got_sin_i = (1.0 - f64::from(wi[2]).powi(2)).sqrt();
            assert!(
                (got_sin_i - sin_i).abs() < 2e-6,
                "Snell: {got_sin_i} vs {sin_i}"
            );
            // Coplanar with wo and m, on the opposite side of m.
            let plane = cross3(wo, m);
            assert!(f64::from(dot3(plane, wi)).abs() < 1e-6);
            if sin_o > 1e-3 {
                assert!(wi[0] * wo[0] <= 0.0 && wi[1] * wo[1] <= 0.0);
            }
        }
    }
}

#[test]
fn transmission_density_integrates_to_the_refracted_share() {
    // Over the lower hemisphere the density integrates to the share of VNDF half vectors whose
    // refraction ends below the horizon (1 minus total internal reflection and the few grazing
    // refractions that come back up), measured independently by sampling.
    let t = [1.0_f32, 0.0, 0.0];
    let n = [0.0_f32, 0.0, 1.0];
    let count = 1u32 << 16;
    for (stream, (roughness, anisotropy, eta, c)) in [
        (0.4_f32, 0.0_f32, 1.5_f32, 0.8_f64),
        (0.7, 0.0, 1.5, 0.3),
        (0.5, 0.8, 1.33, 0.6),
        (0.6, 0.0, 1.0 / 1.5, 0.9),
        (0.8, 0.5, 1.0 / 1.5, 0.5),
    ]
    .into_iter()
    .enumerate()
    {
        let i = bare_btdf(roughness, anisotropy);
        let alpha = layers(&i, &FRAME, dir(c, 0.2)).transmission.roughness;
        let wo = dir(c, 0.2);
        let mass = lower_hemisphere_integral(1024, 1024, |wi| {
            f64::from(ggx_transmission_pdf(wo, wi, n, t, &closure(alpha, eta)))
        });
        let valid = (0..count)
            .filter(|k| {
                let u = u3(300 + stream as u32, *k);
                let h = standard_surface_bsdf::sampling::mx_ggx_importance_sample_vndf(
                    [u[1], u[2]],
                    wo,
                    alpha,
                );
                refract(wo, h, eta)[2] < 0.0
            })
            .count() as f64
            / f64::from(count);
        println!(
            "roughness {roughness} aniso {anisotropy} eta {eta:.3} cos {c}: mass {mass:.4}, \
             refracted share {valid:.4}"
        );
        assert!((mass - valid).abs() < 0.01, "{mass} vs {valid}");
        if eta > 1.0 {
            assert!(
                mass > 0.97,
                "entering a denser medium almost never fails: {mass}"
            );
        }
    }
}

const NT: usize = 32;
const NP: usize = 64;
const INVALID: usize = NT * NP;

/// (theta, phi) bin of a direction mirrored into the upper hemisphere.
fn lower_bin(w: [f32; 3]) -> usize {
    let (x, y, z) = (f64::from(w[0]), f64::from(w[1]), -f64::from(w[2]));
    let theta = z.clamp(-1.0, 1.0).acos();
    let phi = y.atan2(x).rem_euclid(2.0 * PI);
    let it = ((theta / (0.5 * PI) * NT as f64) as usize).min(NT - 1);
    let ip = ((phi / (2.0 * PI) * NP as f64) as usize).min(NP - 1);
    it * NP + ip
}

#[test]
fn transmission_sampler_matches_its_density_chi_square() {
    // Expected fractions: the density integrated over each (theta, phi) bin of the lower
    // hemisphere by 12 x 12 midpoints per bin; the rest is the invalid slot.
    let n = 1u32 << 17;
    let mut min_p = 1.0_f64;
    for (stream, (name, i, frame, wo)) in [
        (
            "rough 0.4 entering",
            bare_btdf(0.4, 0.0),
            FRAME,
            dir(0.7, 0.3),
        ),
        (
            "rough 0.7 entering grazing",
            bare_btdf(0.7, 0.0),
            FRAME,
            dir(0.25, 0.0),
        ),
        (
            "aniso 0.8 entering",
            bare_btdf(0.5, 0.8),
            FRAME,
            dir(0.5, 0.0),
        ),
        (
            "rough 0.5 exiting",
            bare_btdf(0.5, 0.0),
            INSIDE,
            dir(0.9, 0.3),
        ),
        (
            "aniso 0.98 exiting",
            bare_btdf(0.8, 0.98),
            INSIDE,
            dir(0.8, 0.0),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        // The transmission lobe alone (outside it already is; inside the interface would also
        // select its reflection).
        let mut weights = [0.0; SS_LOBE_COUNT as usize];
        weights[Lobe::Transmission.index()] = 1.0;
        let mut observed = vec![0u64; INVALID + 1];
        for k in 0..n {
            let s = sample_with(&i, &frame, wo, u3(400 + stream as u32, k), &weights);
            assert_eq!(s.lobe, Lobe::Transmission);
            assert!(!s.delta);
            let slot = if s.valid { lower_bin(s.wi) } else { INVALID };
            observed[slot] += 1;
        }
        let s = layers(&i, &frame, wo);
        let mut expected = vec![0.0_f64; INVALID + 1];
        let sub = 12usize;
        let d_theta = 0.5 * PI / NT as f64 / sub as f64;
        let d_phi = 2.0 * PI / NP as f64 / sub as f64;
        for it in 0..NT * sub {
            let theta = (it as f64 + 0.5) * d_theta;
            let solid = theta.sin() * d_theta * d_phi;
            for ip in 0..NP * sub {
                let phi = (ip as f64 + 0.5) * d_phi;
                let w = [
                    (theta.sin() * phi.cos()) as f32,
                    (theta.sin() * phi.sin()) as f32,
                    -(theta.cos()) as f32,
                ];
                let p = ggx_transmission_pdf(wo, w, s.n, s.main_tangent, &s.transmission);
                expected[(it / sub) * NP + ip / sub] += f64::from(p) * solid;
            }
        }
        let mass: f64 = expected[..INVALID].iter().sum();
        expected[INVALID] = (1.0 - mass).max(0.0);
        let (p, chi2, cells) = chi_square_p(&observed, &expected, f64::from(n));
        min_p = min_p.min(p);
        println!("chi2 {name}: {chi2:.1} over {cells} cells, p {p:.4}, mass {mass:.4}");
        assert!(
            p > 1e-4,
            "{name}: chi-square p {p} (chi2 {chi2}, {cells} cells)"
        );
    }
    println!("transmission chi-square: min p {min_p:.4}");
}

#[test]
fn btdf_obeys_generalized_reciprocity() {
    // Adjoint form: f(wo, wi) / eta_wi^2 = f(wi, wo) / eta_wo^2, with f = value / |n.wi|.
    let n = [0.0_f32, 0.0, 1.0];
    let t = [1.0_f32, 0.0, 0.0];
    let mut checked = 0;
    let mut worst = 0.0_f64;
    for (alpha, fresnel) in [
        ([0.3_f32, 0.3], false),
        ([0.1, 0.4], false),
        ([0.8, 0.5], false),
        ([0.3, 0.3], true),
        ([0.8, 0.5], true),
    ] {
        for eta in [1.5_f32, 1.33, 1.0 / 1.5] {
            for co in [0.2_f64, 0.5, 0.9] {
                for ci in [0.15_f64, 0.5, 0.95] {
                    for dphi in [0.0_f64, 0.4, 2.0, 3.1] {
                        let wo = dir(co, 0.3);
                        let below = dir(ci, 0.3 + PI + dphi);
                        let wi = [below[0], below[1], -below[2]];
                        let into = closure(alpha, eta);
                        let forward =
                            f64::from(ggx_transmission(wo, wi, n, t, &into, 0.0, fresnel))
                                / (-f64::from(wi[2]));
                        let n_flip = scale3(n, -1.0);
                        let back = closure(alpha, 1.0 / eta);
                        let backward =
                            f64::from(ggx_transmission(wi, wo, n_flip, t, &back, 0.0, fresnel))
                                / f64::from(wo[2]);
                        let lhs = forward / f64::from(eta * eta);
                        let rhs = backward;
                        if lhs == 0.0 && rhs == 0.0 {
                            continue;
                        }
                        let rel = (lhs - rhs).abs() / lhs.abs().max(rhs.abs());
                        worst = worst.max(rel);
                        assert!(rel < 1e-4, "alpha {alpha:?} eta {eta}: {lhs} vs {rhs}");
                        checked += 1;
                    }
                }
            }
        }
    }
    println!("reciprocity: {checked} pairs, max relative error {worst:.2e}");
    assert!(checked > 100, "{checked} pairs");
}

#[test]
fn sample_is_consistent_with_pdf_and_eval_for_transmissive_materials() {
    let mixed = SurfaceInputs {
        transmission: 0.6,
        transmission_color: [0.9, 0.7, 0.5],
        transmission_extra_roughness: 0.2,
        ..rich()
    };
    let mut transmitted = 0;
    let mut checked = 0;
    for (stream, (i, frame)) in [
        (glass(0.3, 0.0), FRAME),
        (glass(0.6, 0.8), FRAME),
        (mixed, FRAME),
        (glass(0.4, 0.0), INSIDE),
        (mixed, INSIDE),
    ]
    .into_iter()
    .enumerate()
    {
        for c in [0.2_f64, 0.6, 0.95] {
            let wo = dir(c, 0.4);
            for k in 0..2048 {
                let s = sample(&i, &frame, wo, u3(500 + stream as u32, k));
                if !s.valid || s.delta {
                    continue;
                }
                let p = pdf(&i, &frame, wo, s.wi);
                assert!(
                    (s.pdf - p).abs() <= 1e-5 * p.max(1.0),
                    "pdf {} vs {p}",
                    s.pdf
                );
                let e = eval_light(&i, &frame, wo, s.wi).sum();
                for (w, ek) in s.weight.iter().zip(e) {
                    let want = ek / p;
                    assert!(
                        (w - want).abs() <= 1e-5 * want.abs().max(1.0),
                        "{w} vs {want}"
                    );
                }
                if s.lobe == Lobe::Transmission {
                    assert!(s.wi[2] < 0.0);
                    transmitted += 1;
                }
                checked += 1;
            }
        }
    }
    println!("{checked} continuous samples checked, {transmitted} transmitted");
    assert!(transmitted > 1000);
}

#[test]
fn smooth_glass_refracts_as_a_delta_lobe() {
    let i = glass(0.0, 0.0);
    let s0 = layers(&i, &FRAME, dir(0.7, 0.0));
    assert!(transmission_is_delta(
        s0.transmission.roughness,
        s0.transmission.ior
    ));
    for c in [0.95_f64, 0.7, 0.3] {
        let wo = dir(c, 0.5);
        let w = lobe_weights(&i, &FRAME, wo);
        let p_t = w[Lobe::Transmission.index()];
        // u.x just below 1 selects the last positive lobe, the transmission.
        let s = sample_with(&i, &FRAME, wo, [0.999_999, 0.3, 0.6], &w);
        assert_eq!(s.lobe, Lobe::Transmission);
        assert!(s.delta && s.valid);
        assert_eq!(s.wi, refract(wo, [0.0, 0.0, 1.0], 1.5));
        assert_eq!(s.pdf, p_t);
        let tint = transmission_tint(&layers(&i, &FRAME, wo), wo);
        for (got, want) in s.weight.iter().zip(tint) {
            assert!((got - want / p_t).abs() < 1e-6);
        }
        // A delta lobe has no density and no direct-light response.
        assert_eq!(pdf(&i, &FRAME, wo, s.wi), 0.0);
        assert_eq!(eval_light(&i, &FRAME, wo, s.wi), Lobes::ZERO);
    }
    // Exiting smooth glass beyond the critical angle (41.8 degrees): the refraction is invalid.
    let wo = dir(0.3, 0.0);
    let w = lobe_weights(&i, &INSIDE, wo);
    let s = sample_with(&i, &INSIDE, wo, [0.999_999, 0.3, 0.6], &w);
    assert_eq!(s.lobe, Lobe::Transmission);
    assert!(s.delta && !s.valid);
    // Index-matched (eta = 1) glass is a straight-through delta whatever its roughness.
    let matched = SurfaceInputs {
        specular_ior: 1.0,
        ..glass(0.5, 0.0)
    };
    let wo = dir(0.6, 0.9);
    let w = lobe_weights(&matched, &FRAME, wo);
    let s = sample_with(&matched, &FRAME, wo, [0.999_999, 0.3, 0.6], &w);
    assert!(s.delta && s.valid);
    let straight = scale3(wo, -1.0);
    assert!(s.wi.iter().zip(straight).all(|(a, b)| (a - b).abs() < 1e-6));
}

#[test]
fn white_furnace_of_glass() {
    // Outside: the energy-compensated dielectric specular plus the Fresnel-free BTDF under its
    // throughput. Inside: the bare interface, total internal reflection included. Neither BTDF
    // has a multiple-scattering compensation (MaterialX has none for transmission to port), so
    // rough glass loses the single-scatter masking share: measured 0.75 (outside, grazing) and
    // 0.49 (inside) at roughness 1. The upper bound allows MaterialX's GGX albedo-fit overshoot
    // (up to 3% at grazing smooth angles, README).
    let n = 1u32 << 16;
    for (stream, (name, frame)) in [("outside", FRAME), ("inside", INSIDE)]
        .into_iter()
        .enumerate()
    {
        for (roughness, lower) in [(0.0_f32, 0.98), (0.2, 0.98), (0.5, 0.74), (1.0, 0.45)] {
            for c in [0.3_f64, 0.8] {
                let i = glass(roughness, 0.0);
                let wo = dir(c, 0.1);
                let w = lobe_weights(&i, &frame, wo);
                let values: Vec<f64> = (0..n)
                    .map(|k| {
                        let u = u3(600 + stream as u32 * 16 + (roughness * 10.0) as u32, k);
                        let s = sample_with(&i, &frame, wo, u, &w);
                        if s.valid { lum(s.weight) } else { 0.0 }
                    })
                    .collect();
                let (m, se) = mean_se(&values);
                println!("furnace glass {name} roughness {roughness} NdotV {c}: {m:.4} +- {se:.4}");
                assert!(m <= 1.03 + 3.0 * se, "{name} r {roughness} c {c}: {m}");
                assert!(m >= lower, "{name} r {roughness} c {c}: {m} below {lower}");
            }
        }
    }
}

#[test]
fn smooth_glass_reflection_and_transmission_sum_to_one() {
    // A smooth interface is energy-preserving (OpenPBR, E + T = 1): the delta reflection and
    // the delta refraction must split exactly what arrives, at every IOR and angle, outside and
    // inside (total internal reflection included). The expected sample weight of delta lobes is
    // the sum of their responses, so this is exact, not sampled. Regression: the throughput under
    // the specular took MaterialX's albedo fit with a grazing reflectance of 1 while the mirror
    // returned the exact Fresnel; IOR 1 glass lost 79% at NdotV 0.05.
    use standard_surface_bsdf::sample::delta_response;
    for ior in [1.0_f32, 1.05, 1.33, 1.5, 2.4] {
        for (name, frame) in [("outside", FRAME), ("inside", INSIDE)] {
            for c in [1.0_f64, 0.7, 0.3, 0.05, 0.01] {
                let i = SurfaceInputs {
                    specular_ior: ior,
                    ..glass(0.0, 0.0)
                };
                let wo = dir(c, 0.1);
                let s = layers(&i, &frame, wo);
                let total = lum(delta_response(&s, wo, Lobe::Specular))
                    + lum(delta_response(&s, wo, Lobe::Transmission));
                assert!(
                    (total - 1.0).abs() < 2e-4,
                    "{name} ior {ior} NdotV {c}: reflection + transmission = {total}"
                );
            }
        }
    }
}

#[test]
fn index_matched_rough_glass_passes_everything() {
    // IOR 1 has no interface: no reflection at any roughness, the refraction is the identity.
    let n = 1u32 << 14;
    for c in [0.8_f64, 0.3, 0.05] {
        let i = SurfaceInputs {
            specular_ior: 1.0,
            ..glass(0.4, 0.0)
        };
        let wo = dir(c, 0.1);
        let w = lobe_weights(&i, &FRAME, wo);
        let values: Vec<f64> = (0..n)
            .map(|k| {
                let s = sample_with(&i, &FRAME, wo, u3(900, k), &w);
                if s.valid { lum(s.weight) } else { 0.0 }
            })
            .collect();
        let (m, se) = mean_se(&values);
        assert!((m - 1.0).abs() < 1e-3 + 3.0 * se, "NdotV {c}: {m} +- {se}");
    }
}

#[test]
fn transmission_zero_keeps_the_opaque_model() {
    // Below the horizon an opaque material still has no response and no density, and the
    // `inside` flag is ignored without transmission (an opaque surface has no interior).
    let i = rich();
    for c in [0.2_f64, 0.7] {
        let wo = dir(c, 0.3);
        let below = {
            let d = dir(0.5, 2.0);
            [d[0], d[1], -d[2]]
        };
        assert_eq!(eval_light(&i, &FRAME, wo, below), Lobes::ZERO);
        assert_eq!(pdf(&i, &FRAME, wo, below), 0.0);
        let wi = dir(0.6, 2.8);
        assert_eq!(
            eval_light(&i, &INSIDE, wo, wi),
            eval_light(&i, &FRAME, wo, wi)
        );
        assert_eq!(lobe_weights(&i, &INSIDE, wo), lobe_weights(&i, &FRAME, wo));
        assert_eq!(
            lobe_weights(&i, &FRAME, wo)[Lobe::Transmission.index()],
            0.0
        );
    }
    // transmission_mix scales sheen and diffuse (MaterialX `transmission_mix`).
    let half = SurfaceInputs {
        transmission: 0.5,
        ..i
    };
    let wo = dir(0.6, 0.0);
    let wi = dir(0.5, 2.5);
    let full = eval_light(&i, &FRAME, wo, wi);
    let mixed = eval_light(&half, &FRAME, wo, wi);
    for (a, b) in mixed.base.iter().zip(full.base) {
        assert!((a - 0.5 * b).abs() <= 1e-6 * b);
    }
    assert_eq!(mixed.specular, full.specular);
    // Normalised weights with a sixth lobe.
    let w = lobe_weights(&half, &FRAME, wo);
    assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    assert!(w[Lobe::Transmission.index()] > 0.0);
}
