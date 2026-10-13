//! The subsurface closure (`subsurface_mix` of `standard_surface.mtlx:252-256` over
//! `mx_subsurface_bsdf`, `pbrlib/genglsl/mx_subsurface_bsdf.glsl`, and the Burley integration of
//! `lib/mx_microfacet_diffuse.glsl:158-199`) against an independent f64 transcription of
//! MaterialX's formula, its energy, and the identity at `subsurface = 0`.

mod common;

use common::{FRAME, dir, hemisphere_integral, white};
use standard_surface_bsdf::sample::{pdf, sample};
use standard_surface_bsdf::{
    Environment, Lobe, ShadingFrame, SurfaceInputs, eval_environment, eval_light, lobe_weights,
};

/// MaterialX's `mx_integrate_burley_diffusion` in f64: the Burley profile
/// `(exp(-s d) + exp(-s d / 3)) / max(d, 1e-8)` weighted mean of `max(cos(theta + x), 0)` over 32
/// arc samples of a circle of radius `radius`, shape `1 / max(mfp, 0.1)`.
fn burley_d_f64(theta: f64, radius: f64, mfp: f64) -> f64 {
    let shape = 1.0 / mfp.max(0.1);
    let width = 2.0 * std::f64::consts::PI / 32.0;
    let (mut sum_d, mut sum_r) = (0.0, 0.0);
    for i in 0..32 {
        let x = -std::f64::consts::PI + (f64::from(i) + 0.5) * width;
        let dist = radius * (2.0 * (x * 0.5).sin()).abs();
        let r = ((-shape * dist).exp() + (-shape * dist / 3.0).exp()) / dist.max(1e-8);
        sum_d += r * (theta + x).cos().max(0.0);
        sum_r += r;
    }
    sum_d / sum_r
}

/// Radius of MaterialX's approximation at a curvature: `1 / max(curvature, 0.01)`.
fn radius_of(curvature: f64) -> f64 {
    1.0 / curvature.max(0.01)
}

/// A pure subsurface material: `subsurface = 1` with colour `color`, no specular, sheen or coat,
/// mean free path `mfp` (radius `mfp`, scale 1).
fn subsurface_only(color: [f32; 3], mfp: f32) -> SurfaceInputs {
    white(|i| {
        i.specular = 0.0;
        i.subsurface = 1.0;
        i.subsurface_color = color;
        i.subsurface_radius = [mfp; 3];
        i.subsurface_scale = 1.0;
    })
}

fn frame_with_curvature(curvature: f32) -> ShadingFrame {
    ShadingFrame { curvature, ..FRAME }
}

/// `eval_light` of a pure subsurface material is `color * D(theta_l) / pi` with MaterialX's
/// integral `D`, independent of the view, for curvatures from flat to tight and several mean
/// free paths.
#[test]
fn subsurface_eval_matches_the_materialx_integral() {
    let color = [0.9_f32, 0.6, 0.3];
    let mut worst = 0.0_f64;
    for &curvature in &[0.0_f32, 0.3, 1.0, 4.0, 50.0] {
        for &mfp in &[0.05_f32, 0.5, 1.0, 3.0] {
            let i = subsurface_only(color, mfp);
            let f = frame_with_curvature(curvature);
            for &cos_l in &[0.02_f64, 0.2, 0.5, 0.8, 1.0] {
                for &cos_v in &[0.1_f64, 0.6, 1.0] {
                    let wo = dir(cos_v, 0.4);
                    let wi = dir(cos_l, 2.1);
                    let got = eval_light(&i, &f, wo, wi);
                    let d = burley_d_f64(
                        f64::from(wi[2]).acos(),
                        radius_of(f64::from(curvature)),
                        f64::from(mfp),
                    );
                    for (k, c) in color.iter().enumerate() {
                        let want = f64::from(*c) * d / std::f64::consts::PI;
                        let rel = (f64::from(got.base[k]) - want).abs() / want.max(1e-12);
                        worst = worst.max(rel);
                        assert!(
                            rel < 2e-4,
                            "curvature {curvature} mfp {mfp} cos_l {cos_l}: {} vs {want}",
                            got.base[k]
                        );
                    }
                    assert_eq!(got.specular, [0.0; 3]);
                    assert_eq!(got.transmission, [0.0; 3]);
                }
            }
        }
    }
    println!("worst relative error vs the f64 MaterialX integral: {worst:.2e}");
}

/// Energy: over the upper hemisphere the white subsurface closure reflects exactly 1 on a flat
/// surface (the Burley profile collapses onto the nearest arc samples, `D -> cos`) and at most 1
/// at any curvature and mean free path; it matches the f64 integral of MaterialX's `D`. What
/// MaterialX's approximation spreads below the horizon (e.g. 17% at curvature 1, mfp 1, where
/// its full-sphere total reaches 1.10) is dropped by the hemisphere contract.
#[test]
fn white_subsurface_albedo_is_bounded_and_one_when_flat() {
    for &curvature in &[0.0_f32, 0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 100.0] {
        for &mfp in &[0.1_f32, 0.5, 1.0, 2.0, 10.0] {
            let i = subsurface_only([1.0; 3], mfp);
            let f = frame_with_curvature(curvature);
            let wo = dir(0.7, 0.0);
            let albedo =
                hemisphere_integral(256, 32, |wi| f64::from(eval_light(&i, &f, wo, wi).base[1]));
            let want = hemisphere_integral(256, 32, |wi| {
                burley_d_f64(
                    f64::from(wi[2]).acos(),
                    radius_of(f64::from(curvature)),
                    f64::from(mfp),
                ) / std::f64::consts::PI
            });
            assert!(
                (albedo - want).abs() < 1e-4,
                "curvature {curvature} mfp {mfp}: albedo {albedo} vs f64 {want}"
            );
            assert!(
                albedo <= 1.0 + 1e-3,
                "curvature {curvature} mfp {mfp}: albedo {albedo} > 1"
            );
            if curvature == 0.0 && mfp <= 2.0 {
                assert!(
                    (albedo - 1.0).abs() < 1e-3,
                    "flat mfp {mfp}: albedo {albedo}, expected 1"
                );
            }
        }
    }
}

/// `subsurface_mix` is `mix(diffuse, subsurface, subsurface)` in direct and environment light:
/// the halfway blend is the mean of the two pure materials, with the diffuse lobe carrying both
/// (its selection weight grows with the subsurface colour).
#[test]
fn subsurface_mix_blends_diffuse_and_subsurface() {
    let f = frame_with_curvature(0.8);
    let diffuse = white(|i| {
        i.specular = 0.0;
        i.base_color = [0.2, 0.3, 0.4];
    });
    let pure = SurfaceInputs {
        subsurface: 1.0,
        subsurface_color: [0.9, 0.7, 0.5],
        subsurface_radius: [0.5, 0.25, 0.1],
        subsurface_scale: 2.0,
        ..diffuse
    };
    let half = SurfaceInputs {
        subsurface: 0.5,
        ..pure
    };
    let wo = dir(0.6, 0.3);
    let wi = dir(0.4, 2.5);
    let (a, b, m) = (
        eval_light(&diffuse, &f, wo, wi).base,
        eval_light(&pure, &f, wo, wi).base,
        eval_light(&half, &f, wo, wi).base,
    );
    let env = Environment {
        irradiance: [0.8, 0.9, 1.0],
        specular_radiance: [0.0; 3],
        coat_radiance: [0.0; 3],
        transmission_radiance: [0.0; 3],
    };
    let (ea, eb, em) = (
        eval_environment(&diffuse, &f, wo, &env).base,
        eval_environment(&pure, &f, wo, &env).base,
        eval_environment(&half, &f, wo, &env).base,
    );
    for k in 0..3 {
        assert!((m[k] - 0.5 * (a[k] + b[k])).abs() < 1e-6 * (a[k] + b[k]));
        assert!((em[k] - 0.5 * (ea[k] + eb[k])).abs() < 1e-6 * (ea[k] + eb[k]));
        // MaterialX indirect subsurface is `irradiance * color` (mx_subsurface_bsdf.glsl:27-31).
        assert!((eb[k] - env.irradiance[k] * pure.subsurface_color[k]).abs() < 1e-6);
    }
    // Only the diffuse lobe (index) is selected for a specular-free, sheen-free material.
    let w = lobe_weights(&half, &f, wo);
    assert_eq!(w[Lobe::Diffuse.index()], 1.0);
}

/// Sampling the subsurface-mixed diffuse lobe is unbiased: the Monte Carlo estimate of the
/// hemispherical albedo from `sample` weights equals the quadrature of `eval_light`, and `pdf`
/// equals the density `sample` reports.
#[test]
fn subsurface_sampling_is_consistent_with_eval() {
    let i = SurfaceInputs {
        base_color: [0.3, 0.5, 0.2],
        subsurface: 0.7,
        subsurface_color: [0.9, 0.8, 0.6],
        subsurface_radius: [1.0, 0.6, 0.3],
        ..subsurface_only([1.0; 3], 1.0)
    };
    let f = frame_with_curvature(1.5);
    let wo = dir(0.5, 1.0);
    let n = 200_000u32;
    let mut sum = 0.0_f64;
    for k in 0..n {
        let s = sample(&i, &f, wo, common::u3(91, k));
        if s.valid {
            assert!(!s.delta);
            assert_eq!(s.pdf.to_bits(), pdf(&i, &f, wo, s.wi).to_bits());
            sum += f64::from(s.weight[1]);
        }
    }
    let mc = sum / f64::from(n);
    let quad = hemisphere_integral(256, 64, |wi| f64::from(eval_light(&i, &f, wo, wi).base[1]));
    assert!(
        (mc - quad).abs() < 3e-3,
        "sampled albedo {mc} vs quadrature {quad}"
    );
}

/// `subsurface = 0` never evaluates the subsurface closure: its other inputs and the curvature
/// cannot change a single bit of any output.
#[test]
fn subsurface_zero_ignores_the_other_subsurface_inputs() {
    let plain = common::rich();
    let odd = SurfaceInputs {
        subsurface: 0.0,
        subsurface_color: [0.1, 5.0, 0.0],
        subsurface_radius: [0.0, 7.0, 0.3],
        subsurface_scale: 9.0,
        subsurface_anisotropy: -0.8,
        ..plain
    };
    let f = frame_with_curvature(3.0);
    for (cv, cl) in [(0.9, 0.3), (0.2, 0.95), (0.6, 0.6)] {
        let wo = dir(cv, 0.2);
        let wi = dir(cl, 2.9);
        assert_eq!(
            common::bits3(eval_light(&plain, &FRAME, wo, wi).base),
            common::bits3(eval_light(&odd, &f, wo, wi).base)
        );
        assert_eq!(
            lobe_weights(&plain, &FRAME, wo).map(f32::to_bits),
            lobe_weights(&odd, &f, wo).map(f32::to_bits)
        );
        assert_eq!(
            pdf(&plain, &FRAME, wo, wi).to_bits(),
            pdf(&odd, &f, wo, wi).to_bits()
        );
    }
}
