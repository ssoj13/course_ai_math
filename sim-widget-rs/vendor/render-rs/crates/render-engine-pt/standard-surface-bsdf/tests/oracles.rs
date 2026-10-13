//! CPU oracles of the MaterialX primitives: closed forms in f64, analytic limits, quadrature
//! of the fitted albedos, reciprocity with the energy compensation factored out.

mod common;

use common::{dir, fresnel_dielectric_f64, ggx_albedo_fit_f64, hemisphere_integral};
use standard_surface_bsdf::ThinFilmEnergy;
use standard_surface_bsdf::consts::{MX_FLOAT_EPS, MX_PI};
use standard_surface_bsdf::diffuse::{
    mx_oren_nayar_diffuse, mx_oren_nayar_diffuse_bsdf_reflection, mx_oren_nayar_diffuse_dir_albedo,
};
use standard_surface_bsdf::fresnel::{
    mx_artistic_ior, mx_compute_fresnel, mx_fresnel_airy, mx_fresnel_conductor,
    mx_fresnel_conductor_polarized, mx_fresnel_dielectric, mx_fresnel_dielectric_polarized,
    mx_ggx_energy_compensation, mx_init_fresnel_conductor, mx_init_fresnel_dielectric,
    mx_ior_to_f0, reduce_phase,
};
use standard_surface_bsdf::microfacet::{
    MxConductor, MxDielectric, mx_average_alpha, mx_conductor_bsdf_reflection,
    mx_dielectric_bsdf_reflection, mx_ggx_dir_albedo_analytic, mx_ggx_ndf, mx_ggx_smith_g1,
    mx_ggx_smith_g2, mx_roughness_anisotropy,
};
use standard_surface_bsdf::sheen::{
    mx_imageworks_sheen_brdf, mx_imageworks_sheen_dir_albedo, mx_sheen_bsdf_reflection,
};

const N: [f32; 3] = [0.0, 0.0, 1.0];
const X: [f32; 3] = [1.0, 0.0, 0.0];

#[test]
fn dielectric_fresnel_matches_the_closed_form() {
    assert!((f64::from(mx_ior_to_f0(1.5)) - 0.04).abs() < 1e-8);
    assert!((f64::from(mx_fresnel_dielectric(1.0, 1.5)) - 0.04).abs() < 1e-7);
    assert_eq!(mx_fresnel_dielectric(0.0, 1.5), 1.0);
    let mut max_err = 0.0_f64;
    for ior in [1.1_f32, 1.33, 1.5, 2.4, 5.0] {
        for k in 0..=100 {
            let c = k as f32 / 100.0;
            let exact = fresnel_dielectric_f64(f64::from(c), f64::from(ior));
            let got = f64::from(mx_fresnel_dielectric(c, ior));
            let [rp, rs] = mx_fresnel_dielectric_polarized(c, ior);
            let pol = 0.5 * (f64::from(rp) + f64::from(rs));
            max_err = max_err.max((got - exact).abs()).max((pol - exact).abs());
        }
    }
    assert!(max_err < 2e-6, "dielectric Fresnel max error {max_err:e}");
}

#[test]
fn conductor_fresnel_matches_normal_incidence_closed_form() {
    for (n, k) in [(0.18_f32, 3.42_f32), (1.1, 6.8), (2.9, 3.1), (1.5, 0.0)] {
        let got = mx_fresnel_conductor(1.0, [n; 3], [k; 3]);
        let (n, k) = (f64::from(n), f64::from(k));
        let exact = ((n - 1.0).powi(2) + k * k) / ((n + 1.0).powi(2) + k * k);
        for g in got {
            assert!((f64::from(g) - exact).abs() < 2e-6, "{g} vs {exact}");
        }
        // Grazing incidence reflects everything.
        let grazing = mx_fresnel_conductor(0.0, [n as f32; 3], [k as f32; 3]);
        assert!(grazing.iter().all(|g| (g - 1.0).abs() < 1e-6));
    }
}

#[test]
fn artistic_ior_reproduces_its_reflectivity() {
    // Gulbrandsen 2014: the normal-incidence reflectance of (n, k) is the input reflectivity.
    let mut max_err = 0.0_f64;
    for r in [0.02_f32, 0.1, 0.3, 0.5, 0.7, 0.9, 0.98] {
        for g in [0.0_f32, 0.25, 0.5, 1.0] {
            let a = mx_artistic_ior([r; 3], [g; 3]);
            let f = mx_fresnel_conductor(1.0, a.ior, a.extinction);
            for c in f {
                max_err = max_err.max((f64::from(c) - f64::from(r)).abs());
            }
        }
    }
    assert!(max_err < 1e-4, "artistic IOR round trip error {max_err:e}");
    // Reflectivity above 0.99 saturates (mx_artistic_ior.glsl:6).
    assert_eq!(
        mx_artistic_ior([1.5; 3], [0.5; 3]),
        mx_artistic_ior([0.99; 3], [0.5; 3])
    );
}

#[test]
fn ggx_albedo_fit_matches_transcription_and_limits() {
    let mut max_err = 0.0_f64;
    let mut max_white = 0.0_f32;
    for a in 0..=16 {
        for b in 0..=16 {
            let x = (a as f32 / 16.0).max(1e-3);
            let y = (b as f32 / 16.0).max(1e-4);
            let [e_a, _, _] = mx_ggx_dir_albedo_analytic(x, y, [1.0; 3], [0.0; 3]);
            let [e_b, _, _] = mx_ggx_dir_albedo_analytic(x, y, [0.0; 3], [1.0; 3]);
            let (ra, rb) = ggx_albedo_fit_f64(f64::from(x), f64::from(y));
            max_err = max_err
                .max((f64::from(e_a) - ra).abs())
                .max((f64::from(e_b) - rb).abs());
            // A and B are clamped to [0, 1] separately; their sum (the white albedo) may
            // overshoot 1 where the fit does, which the energy compensation then absorbs.
            assert!((0.0..=1.0).contains(&e_a) && (0.0..=1.0).contains(&e_b));
            let [e, _, _] = mx_ggx_dir_albedo_analytic(x, y, [1.0; 3], [1.0; 3]);
            max_white = max_white.max(e);
        }
    }
    println!("GGX albedo fit: max white albedo A + B = {max_white:.5}");
    // Measured 1.0315 at NdotV = 1e-3, alpha = 1e-4 (the fit's grazing smooth corner).
    assert!(max_white < 1.035, "white albedo overshoot {max_white}");
    assert!(max_err < 1e-5, "fit transcription error {max_err:e}");
    // A mirror at normal incidence reflects F0 (A -> 1, B -> 0).
    let [a, _, _] = mx_ggx_dir_albedo_analytic(1.0, 1e-4, [1.0; 3], [0.0; 3]);
    let [b, _, _] = mx_ggx_dir_albedo_analytic(1.0, 1e-4, [0.0; 3], [1.0; 3]);
    assert!(a > 0.99 && b < 0.01, "A = {a}, B = {b}");
}

#[test]
fn ggx_ndf_is_normalised() {
    // Integral of D(h) (n.h) over the hemisphere of microfacet normals is 1. Integrate in
    // t = tan(theta_h) mapped from [0, 1) so the peak of a narrow lobe is resolved.
    for alpha in [
        [0.2_f32, 0.2],
        [0.5, 0.5],
        [1.0, 1.0],
        [0.3, 0.1],
        [0.05, 0.4],
    ] {
        let scale = f64::from(alpha[0].max(alpha[1]));
        let (nu, nphi) = (4000, 64);
        let mut sum = 0.0;
        for i in 0..nu {
            let u = (i as f64 + 0.5) / nu as f64;
            let t = scale * u / (1.0 - u);
            let dt = scale / ((1.0 - u) * (1.0 - u));
            let theta = t.atan();
            let (sin_t, cos_t) = theta.sin_cos();
            let dtheta = dt / (1.0 + t * t);
            for j in 0..nphi {
                let phi = (j as f64 + 0.5) / nphi as f64 * 2.0 * std::f64::consts::PI;
                let h = [
                    (sin_t * phi.cos()) as f32,
                    (sin_t * phi.sin()) as f32,
                    cos_t as f32,
                ];
                let d = f64::from(mx_ggx_ndf(h, alpha));
                sum += d * cos_t * sin_t * dtheta;
            }
        }
        sum *= 2.0 * std::f64::consts::PI / nphi as f64 / nu as f64;
        assert!((sum - 1.0).abs() < 2e-3, "alpha {alpha:?}: integral {sum}");
    }
}

#[test]
fn smith_and_roughness_mapping_follow_materialx() {
    for a in [1e-4_f32, 0.1, 0.5, 1.0] {
        assert!((mx_ggx_smith_g1(1.0, a) - 1.0).abs() < 1e-7);
        assert!((mx_ggx_smith_g2(1.0, 1.0, a) - 1.0).abs() < 1e-7);
        let g = mx_ggx_smith_g2(0.3, 0.7, a);
        assert!((g - mx_ggx_smith_g2(0.7, 0.3, a)).abs() < 1e-7 && g > 0.0 && g <= 1.0);
        assert!(mx_ggx_smith_g1(0.05, a) < mx_ggx_smith_g1(0.5, a) || a < 1e-3);
    }
    assert_eq!(mx_roughness_anisotropy(0.0, 0.0), [MX_FLOAT_EPS; 2]);
    assert_eq!(mx_roughness_anisotropy(0.5, 0.0), [0.25, 0.25]);
    let [ax, ay] = mx_roughness_anisotropy(0.5, 0.75);
    assert!(
        (ax - 0.5).abs() < 1e-7 && (ay - 0.125).abs() < 1e-7,
        "{ax} {ay}"
    );
    // Anisotropy clamps at 0.98 (mx_roughness_anisotropy.glsl:6).
    assert_eq!(
        mx_roughness_anisotropy(0.2, 1.0),
        mx_roughness_anisotropy(0.2, 0.98)
    );
    assert!((mx_average_alpha([0.5, 0.125]) - 0.25).abs() < 1e-7);
}

#[test]
fn oren_nayar_branches_and_albedo() {
    // Roughness 0 is Lambert: A = 1, B = 0.
    assert_eq!(mx_oren_nayar_diffuse(0.4, 0.7, 0.9, 0.0), 1.0);
    assert_eq!(mx_oren_nayar_diffuse_dir_albedo(0.3, 0.0), 1.0);
    // s <= 0 returns A exactly (mx_microfacet_diffuse.glsl:11).
    let sigma2 = 0.25_f32;
    let a = 1.0 - 0.5 * (sigma2 / (sigma2 + 0.33));
    assert_eq!(mx_oren_nayar_diffuse(0.5, 0.5, 0.1, 0.5), a);
    assert!(mx_oren_nayar_diffuse(0.5, 0.5, 0.9, 0.5) > a);
    // The analytic albedo fit against quadrature of the qualitative model.
    let mut max_err = 0.0_f64;
    for r in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
        for c in [0.1_f64, 0.3, 0.5, 0.7, 0.9, 1.0] {
            let v = dir(c, 0.0);
            let e = hemisphere_integral(256, 128, |l| {
                let ndotl = l[2];
                let ldotv = (l[0] * v[0] + l[1] * v[1] + l[2] * v[2]).max(MX_FLOAT_EPS);
                f64::from(mx_oren_nayar_diffuse(c as f32, ndotl, ldotv, r) * ndotl)
                    / std::f64::consts::PI
            });
            let fit = f64::from(mx_oren_nayar_diffuse_dir_albedo(c as f32, r));
            max_err = max_err.max((fit - e).abs());
        }
    }
    // The quadrature mirrors MaterialX's own Monte Carlo reference
    // (`mx_microfacet_diffuse.glsl:41-73`, LdotV clamped to M_FLOAT_EPS); the difference is the
    // accuracy of MaterialX's rational fit, measured 0.0231.
    println!("Oren-Nayar albedo fit vs quadrature: max |err| = {max_err:.5}");
    assert!(max_err < 0.025, "Oren-Nayar albedo fit error {max_err}");
}

#[test]
fn sheen_albedo_fit_matches_quadrature() {
    let mut max_err = 0.0_f64;
    for r in [0.1_f32, 0.3, 0.5, 0.8, 1.0] {
        for c in [0.1_f64, 0.3, 0.5, 0.7, 1.0] {
            let v = dir(c, 0.0);
            let e = hemisphere_integral(256, 128, |l| {
                let h = [l[0] + v[0], l[1] + v[1], l[2] + v[2]];
                let len = (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt();
                let ndoth = (h[2] / len).clamp(MX_FLOAT_EPS, 1.0);
                f64::from(mx_imageworks_sheen_brdf(l[2], c as f32, ndoth, r) * l[2])
            });
            let fit = f64::from(mx_imageworks_sheen_dir_albedo(c as f32, r));
            max_err = max_err.max((fit - e).abs());
        }
    }
    // Mirrors MaterialX's Monte Carlo reference (`mx_microfacet_sheen.glsl:50-79`); the
    // difference is the accuracy of the rational fit, measured 0.0357.
    println!("Imageworks sheen albedo fit vs quadrature: max |err| = {max_err:.5}");
    assert!(
        mx_imageworks_sheen_dir_albedo(0.5, 0.3) > 0.1,
        "sheen albedo must be non-zero"
    );
    assert!(max_err < 0.04, "sheen albedo fit error {max_err}");
}

#[test]
fn airy_film_is_invisible_when_it_matches_the_substrate() {
    // A film whose IOR equals the dielectric substrate's has no second interface (R23 = 0), so
    // Airy reduces to the plain dielectric Fresnel of the film IOR. In f32, R23 is a rounding
    // residue of ~1e-7, and MaterialX's `r123 = sqrt(R12 R23)` amplifies it to ~1e-4
    // (`mx_microfacet_specular.glsl:351-352`); the bound below is that residue, measured.
    let mut max_err = 0.0_f64;
    let mut max_r23 = 0.0_f32;
    for ior in [1.33_f32, 1.5, 2.4] {
        let fd = mx_init_fresnel_dielectric(ior, 500.0, ior);
        for k in 0..=50 {
            let c = (k as f32 / 50.0).max(1e-3);
            let airy = mx_fresnel_airy(c, &fd);
            let plain = f64::from(mx_fresnel_dielectric(c, ior));
            let cos_t = (1.0 - (1.0 - c * c) / (ior * ior)).sqrt();
            let r23 = mx_fresnel_conductor_polarized(cos_t, [1.0; 3], [0.0; 3]);
            for v in r23.p.iter().chain(&r23.s) {
                max_r23 = max_r23.max(v.abs());
            }
            for a in airy {
                max_err = max_err.max((f64::from(a) - plain).abs());
            }
        }
    }
    println!("matched-film Airy: max |R23| {max_r23:e}, max |Airy - Fresnel| {max_err:e}");
    assert!(max_r23 < 1e-6, "second-interface residue {max_r23:e}");
    assert!(max_err < 2e-4, "matched-film Airy error {max_err:e}");
    // Positive control: the compared reflectances are the real, non-zero Fresnel values.
    let fd = mx_init_fresnel_dielectric(1.5, 500.0, 1.5);
    let [a, _, _] = mx_fresnel_airy(0.5, &fd);
    assert!(a > 0.05 && mx_fresnel_dielectric(0.5, 1.5) > 0.05);
}

#[test]
fn airy_is_bounded_and_zero_thickness_is_plain_fresnel() {
    for tf_ior in [0.5_f32, 1.0, 1.5, 3.0] {
        let fd = mx_init_fresnel_dielectric(1.5, 0.0, tf_ior);
        assert!(!fd.airy);
        assert_eq!(
            mx_compute_fresnel(0.7, &fd),
            [mx_fresnel_dielectric(0.7, 1.5); 3]
        );
    }
    let mut varies = false;
    for thickness in [50.0_f32, 250.0, 400.0, 1000.0, 2000.0] {
        for tf_ior in [1.0_f32, 1.33, 2.0, 3.0] {
            for fd in [
                mx_init_fresnel_dielectric(1.5, thickness, tf_ior),
                mx_init_fresnel_conductor([0.2, 0.9, 1.1], [3.9, 2.4, 2.2], thickness, tf_ior),
            ] {
                let mut last = None;
                for k in 0..=20 {
                    let c = (k as f32 / 20.0).max(1e-3);
                    let f = mx_fresnel_airy(c, &fd);
                    assert!(
                        f.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                        "Airy {f:?} at t {thickness} ior {tf_ior} cos {c}"
                    );
                    varies |= last.is_some_and(|l| l != f);
                    last = Some(f);
                }
            }
        }
    }
    assert!(varies, "Airy reflectance must vary with angle");
}

#[test]
fn phase_reduction_preserves_the_cosine() {
    let mut max_err = 0.0_f64;
    for k in 0..20_000 {
        let x = (k as f32 - 10_000.0) * 0.0537;
        let r = reduce_phase(x);
        assert!(r.abs() <= MX_PI * (1.0 + 1e-6), "reduce({x}) = {r}");
        let err = (f64::from(libm::cosf(r)) - f64::from(x).cos()).abs();
        max_err = max_err.max(err);
    }
    // f32 x up to ~537 rad carries 3e-5 absolute ulp; the reduction adds at most a few ulp.
    assert!(max_err < 2e-4, "cos(reduce(x)) error {max_err:e}");
}

#[test]
fn energy_compensation_is_one_for_smooth_lobes_and_grows_with_roughness() {
    let fd = mx_init_fresnel_conductor([0.2; 3], [3.9; 3], 0.0, 1.5);
    let [smooth, _, _] = mx_ggx_energy_compensation(0.8, 1e-4, &fd);
    let [rough, _, _] = mx_ggx_energy_compensation(0.8, 1.0, &fd);
    assert!((smooth - 1.0).abs() < 0.01, "smooth comp {smooth}");
    assert!(rough > 1.1, "rough comp {rough}");
}

#[test]
fn ggx_and_diffuse_closures_are_reciprocal_without_compensation() {
    // f(v, l) = response / NdotL; for GGX lobes divide the NdotV-only compensation out.
    let dielectric = MxDielectric {
        weight: 1.0,
        tint: [1.0; 3],
        ior: 1.5,
        roughness: [0.3, 0.12],
        thinfilm_thickness: 0.0,
        thinfilm_ior: 1.5,
        thin_film_energy: ThinFilmEnergy::MaterialX,
    };
    let conductor = MxConductor {
        weight: 1.0,
        ior_n: [0.2, 0.9, 1.1],
        ior_k: [3.9, 2.4, 2.2],
        roughness: [0.2, 0.2],
        thinfilm_thickness: 0.0,
        thinfilm_ior: 1.5,
        thin_film_energy: ThinFilmEnergy::MaterialX,
    };
    let mut max_rel = 0.0_f64;
    let pairs = [
        (0.9, 0.0, 0.4, 2.0),
        (0.3, 1.0, 0.8, 4.0),
        (0.6, 0.5, 0.2, 3.5),
    ];
    for (cv, pv, cl, pl) in pairs {
        let v = dir(cv, pv);
        let l = dir(cl, pl);
        let fd_d = mx_init_fresnel_dielectric(1.5, 0.0, 1.5);
        let alpha_d = mx_average_alpha(dielectric.roughness);
        let comp_d = |c: f32| mx_ggx_energy_compensation(c, alpha_d, &fd_d);
        let f_vl = mx_dielectric_bsdf_reflection(v, l, N, X, &dielectric, 0.0).response[0]
            / l[2]
            / comp_d(v[2])[0];
        let f_lv = mx_dielectric_bsdf_reflection(l, v, N, X, &dielectric, 0.0).response[0]
            / v[2]
            / comp_d(l[2])[0];
        max_rel = max_rel.max(rel(f_vl, f_lv));

        let fd_c = mx_init_fresnel_conductor(conductor.ior_n, conductor.ior_k, 0.0, 1.5);
        let comp_c = |c: f32| mx_ggx_energy_compensation(c, 0.2, &fd_c);
        let c_vl = mx_conductor_bsdf_reflection(v, l, N, X, &conductor, 0.0).response[1]
            / l[2]
            / comp_c(v[2])[1];
        let c_lv = mx_conductor_bsdf_reflection(l, v, N, X, &conductor, 0.0).response[1]
            / v[2]
            / comp_c(l[2])[1];
        max_rel = max_rel.max(rel(c_vl, c_lv));

        let on_vl = mx_oren_nayar_diffuse_bsdf_reflection(v, l, N, 1.0, [0.5; 3], 0.7, false)
            .response[0]
            / l[2];
        let on_lv = mx_oren_nayar_diffuse_bsdf_reflection(l, v, N, 1.0, [0.5; 3], 0.7, false)
            .response[0]
            / v[2];
        max_rel = max_rel.max(rel(on_vl, on_lv));

        let sh_vl = mx_sheen_bsdf_reflection(v, l, N, 1.0, [1.0; 3], 0.4).response[0] / l[2];
        let sh_lv = mx_sheen_bsdf_reflection(l, v, N, 1.0, [1.0; 3], 0.4).response[0] / v[2];
        max_rel = max_rel.max(rel(sh_vl, sh_lv));
        assert!(f_vl > 0.0 && c_vl > 0.0 && on_vl > 0.0 && sh_vl > 0.0);
    }
    assert!(max_rel < 1e-5, "reciprocity error {max_rel:e}");
}

fn rel(a: f32, b: f32) -> f64 {
    let (a, b) = (f64::from(a), f64::from(b));
    (a - b).abs() / a.abs().max(b.abs()).max(1e-30)
}
