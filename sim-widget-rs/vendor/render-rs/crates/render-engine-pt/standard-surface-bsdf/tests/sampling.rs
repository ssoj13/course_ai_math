//! CPU oracles of the sampling layer (plan §3, wave R2): counter RNG, cosine/cone samplers,
//! MIS heuristics, the anisotropic VNDF density (full-sphere normalisation, independent f64
//! density), chi-square goodness of fit of `sample` against the density, sample/pdf/eval
//! consistency, delta-lobe semantics and the white furnace. Deterministic: every uniform comes
//! from the counter RNG with fixed counters, no threads.

mod common;

use common::{FRAME, chi_square_p, dir, lum, mean_se, rich, u3, white};
use standard_surface_bsdf::fresnel::{
    mx_compute_fresnel, mx_init_fresnel_conductor, mx_init_fresnel_dielectric,
};
use standard_surface_bsdf::microfacet::{mx_average_alpha, mx_ggx_smith_g1};
use standard_surface_bsdf::sample::{Sample, ggx_lobe_pdf, pdf, pdf_with, sample, sample_with};
use standard_surface_bsdf::sampling::{
    balance_heuristic, cone_pdf, ggx_smith_g1_aniso, mx_cosine_hemisphere_pdf,
    mx_cosine_sample_hemisphere, pcg4d, power_heuristic, rng, sample_cone, sobol, sobol_point,
    u32_to_unit,
};
use standard_surface_bsdf::surface::{Layers, layers};
use standard_surface_bsdf::{
    Environment, Lobe, SurfaceInputs, eval_environment, eval_light, lobe_weights,
};

use std::f64::consts::PI;

// A cosine endpoint can become barely above the world-space horizon after f32
// basis conversion. Its almost opposite view then loses GGX support if a second,
// local-space half-vector test rounds V.H negative.
#[test]
fn grazing_cosine_endpoint_retains_reflection_density() {
    let i = SurfaceInputs {
        base_color: [0.18, 0.65, 0.9],
        metalness: 0.35,
        specular_roughness: 0.28,
        coat: 0.8,
        coat_roughness: 0.14,
        coat_affect_roughness: 0.0,
        thin_film_thickness: 420.0,
        thin_film_ior: 1.5,
        ..SurfaceInputs::MATERIALX_DEFAULT
    };
    let f = standard_surface_bsdf::ShadingFrame {
        n: [0.35713413, 0.81545717, -0.455505],
        tangent: [0.0, 1.0, 0.0],
        inside: false,
        curvature: 0.0,
    };
    let wo = [0.9123708, -0.20008695, 0.35713413];
    let weights = lobe_weights(&i, &f, wo);
    let s = sample_with(&i, &f, wo, [0.9596526, 0.5000334, 0.0], &weights);
    assert!(s.valid && !s.delta);
    assert_eq!(s.lobe, Lobe::Diffuse);
    let n_dot_l = standard_surface_bsdf::math::dot3(f.n, s.wi);
    assert!(n_dot_l > 0.0 && n_dot_l < 1.0e-6, "{n_dot_l}");
    // Before the repair every GGX term was zero: pdf=3.83e-10 and
    // weight~9.95e6. The retained coat alone must outweigh the cosine term.
    let coat = layers(&i, &f, wo).coat;
    let coat_pdf = ggx_lobe_pdf(wo, s.wi, f.n, f.tangent, coat.roughness);
    assert!(coat_pdf > 0.003 && coat_pdf < 0.0033, "{coat_pdf}");
    assert!(s.pdf > 0.005 && s.pdf < 0.006, "{}", s.pdf);
    for weight in s.weight {
        assert!(
            weight.is_finite() && (0.5..2.0).contains(&weight),
            "{weight}"
        );
    }
}

#[test]
fn grazing_opposite_upper_directions_have_positive_ggx_density() {
    // Different roughnesses include anisotropy; the support guarantee does not
    // depend on D's shape. Cosines span the f32 cancellation region.
    for nv in [1.0e-8_f32, 1.0e-6, 1.0e-4, 0.01] {
        for nl in [1.0e-8_f32, 1.0e-6, 1.0e-4, 0.01] {
            let wo = [(1.0 - nv * nv).sqrt(), 0.0, nv];
            let wi = [-(1.0 - nl * nl).sqrt(), 0.0, nl];
            for alpha in [[0.02, 0.02], [0.08, 0.08], [0.02, 0.3]] {
                let density = ggx_lobe_pdf(wo, wi, FRAME.n, FRAME.tangent, alpha);
                assert!(
                    density.is_finite() && density > 0.0,
                    "{nv} {nl} {alpha:?}: {density}"
                );
            }
        }
    }
}

#[test]
fn grazing_repair_keeps_view_and_opaque_hemisphere_support() {
    let i = SurfaceInputs::MATERIALX_DEFAULT;
    let wo = FRAME.n;
    let below = [0.8, 0.0, -0.6];
    // A GGX sample can fall below N, so the lobe helper retains its density.
    let density = ggx_lobe_pdf(wo, below, FRAME.n, FRAME.tangent, [0.2, 0.2]);
    let reference = ggx_density_f64(v3(wo), v3(below), 0.2, 0.2, G1Model::Anisotropic);
    assert!((f64::from(density) - reference).abs() < 1.0e-7);
    assert!(density > 0.0);
    // Opaque mixture support and back-facing views remain zero.
    assert_eq!(pdf(&i, &FRAME, wo, below), 0.0);
    assert_eq!(
        ggx_lobe_pdf(below, wo, FRAME.n, FRAME.tangent, [0.2, 0.2]),
        0.0
    );
    assert_eq!(
        ggx_lobe_pdf([1.0, 0.0, 0.0], wo, FRAME.n, FRAME.tangent, [0.2, 0.2]),
        0.0
    );
}

// ---------------------------------------------------------------------------------------------
// Independent f64 reference of the lobe densities.

fn v3(a: [f32; 3]) -> [f64; 3] {
    a.map(f64::from)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = dot(a, a).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The lobe frame the library uses: tangent re-projected against `n`.
fn frame(n: [f64; 3], t: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let d = dot(t, n);
    let x = norm([t[0] - n[0] * d, t[1] - n[1] * d, t[2] - n[2] * d]);
    (x, cross(n, x))
}

fn g1_aniso_f64(v: [f64; 3], ax: f64, ay: f64) -> f64 {
    let a2 = (ax * ax * v[0] * v[0] + ay * ay * v[1] * v[1]) / (v[2] * v[2]);
    1.0 / (1.0 + 0.5 * (-1.0 + (1.0 + a2).sqrt()))
}

fn g1_iso_f64(vz: f64, a: f64) -> f64 {
    let tan2 = (1.0 - vz * vz) / (vz * vz);
    2.0 / (1.0 + (1.0 + a * a * tan2).sqrt())
}

/// Which G1 the reference density uses.
#[derive(Clone, Copy, PartialEq)]
enum G1Model {
    /// The VNDF sampler's true density term (plan C1).
    Anisotropic,
    /// MaterialX's `mx_environment_fis.glsl:14-16` choice: isotropic G1 of the average alpha.
    MaterialXIsotropic,
}

/// f64 VNDF reflection density `D G1 / (4 V.z)` at `wi` (local vectors).
fn ggx_density_f64(v: [f64; 3], l: [f64; 3], ax: f64, ay: f64, g1: G1Model) -> f64 {
    let h = norm([v[0] + l[0], v[1] + l[1], v[2] + l[2]]);
    if v[2] <= 0.0 || dot(v, h) <= 0.0 {
        return 0.0;
    }
    let (hx, hy, hz) = (h[0] / ax, h[1] / ay, h[2]);
    let denom = hx * hx + hy * hy + hz * hz;
    let d = 1.0 / (PI * ax * ay * denom * denom);
    let g = match g1 {
        G1Model::Anisotropic => g1_aniso_f64(v, ax, ay),
        G1Model::MaterialXIsotropic => g1_iso_f64(v[2], (ax * ay).sqrt()),
    };
    d * g / (4.0 * v[2])
}

const NT: usize = 32;
const NP: usize = 64;
const BELOW: usize = NT * NP;

/// (theta, phi) bin of a world direction about `+Z`, or the below-horizon slot.
fn bin_of(w: [f64; 3]) -> usize {
    if w[2] <= 0.0 {
        return BELOW;
    }
    let theta = w[2].min(1.0).acos();
    let phi = w[1].atan2(w[0]).rem_euclid(2.0 * PI);
    let it = ((theta / (0.5 * PI) * NT as f64) as usize).min(NT - 1);
    let ip = ((phi / (2.0 * PI) * NP as f64) as usize).min(NP - 1);
    it * NP + ip
}

/// Histogram (fractions per bin, plus the below-horizon slot) of one GGX lobe's reflection
/// density, by deterministic quadrature in the lobe's scaled slope space (`s = (ax a, ay b)`,
/// polar in `(a, b)`, `rho = u / (1 - u)`): `dw_h = cos^3 theta_h ax ay rho drho dphi` and
/// `P22 = 1 / (pi ax ay (1 + rho^2)^2)`, so every lobe, however narrow or anisotropic, is
/// resolved. Pure f64, independent of the library's f32 code.
fn ggx_histogram(wo: [f64; 3], t: [f64; 3], ax: f64, ay: f64, g1: G1Model) -> Vec<f64> {
    let n = [0.0, 0.0, 1.0];
    let (x, y) = frame(n, t);
    let v = [dot(wo, x), dot(wo, y), dot(wo, n)];
    let g = match g1 {
        G1Model::Anisotropic => g1_aniso_f64(v, ax, ay),
        G1Model::MaterialXIsotropic => g1_iso_f64(v[2], (ax * ay).sqrt()),
    };
    let mut hist = vec![0.0; NT * NP + 1];
    let (nu, nphi) = (2048usize, 512usize);
    let dphi = 2.0 * PI / nphi as f64;
    for iu in 0..nu {
        let u = (iu as f64 + 0.5) / nu as f64;
        let rho = u / (1.0 - u);
        let drho = 1.0 / ((1.0 - u) * (1.0 - u) * nu as f64);
        let radial = rho / (PI * (1.0 + rho * rho) * (1.0 + rho * rho)) * drho * dphi;
        for ip in 0..nphi {
            let phi = (ip as f64 + 0.5) * dphi;
            let (sx, sy) = (ax * rho * phi.cos(), ay * rho * phi.sin());
            let h = norm([-sx, -sy, 1.0]);
            let vh = dot(v, h);
            if vh <= 0.0 {
                continue;
            }
            // pdf(wi) dw_i = D G1 vh / v.z dw_h = P22 / cos_h G1 vh / v.z ax ay rho drho dphi
            let mass = radial * g * vh / (v[2] * h[2]);
            let l = [
                2.0 * vh * h[0] - v[0],
                2.0 * vh * h[1] - v[1],
                2.0 * vh * h[2] - v[2],
            ];
            let w = [
                x[0] * l[0] + y[0] * l[1] + n[0] * l[2],
                x[1] * l[0] + y[1] * l[1] + n[1] * l[2],
                x[2] * l[0] + y[2] * l[1] + n[2] * l[2],
            ];
            hist[bin_of(w)] += mass;
        }
    }
    hist
}

/// Histogram of the cosine lobe about `+Z` (`t = cos^2 theta` uniform, `dP = dt dphi / 2pi`).
fn cosine_histogram() -> Vec<f64> {
    let mut hist = vec![0.0; NT * NP + 1];
    let (nt, nphi) = (1024usize, 512usize);
    let cell = 1.0 / (nt * nphi) as f64;
    for it in 0..nt {
        let c = ((it as f64 + 0.5) / nt as f64).sqrt();
        let s = (1.0 - c * c).sqrt();
        for ip in 0..nphi {
            let phi = (ip as f64 + 0.5) / nphi as f64 * 2.0 * PI;
            hist[bin_of([s * phi.cos(), s * phi.sin(), c])] += cell;
        }
    }
    hist
}

/// GGX lobes of a material, with their alpha and tangent, from the library's layers.
fn ggx_lobes(s: &Layers) -> [(Lobe, [f32; 2], [f32; 3]); 3] {
    [
        (Lobe::Coat, s.coat.roughness, s.coat_tangent),
        (Lobe::Specular, s.specular.roughness, s.main_tangent),
        (Lobe::Metal, s.metal.roughness, s.main_tangent),
    ]
}

/// Expected fractions of the whole sampler: `sum_k p_k hist_k` (delta lobes excluded; the
/// materials of the chi-square grid have none).
fn mixture_histogram(i: &SurfaceInputs, wo: [f32; 3], g1: G1Model) -> Vec<f64> {
    let s = layers(i, &FRAME, wo);
    let w = lobe_weights(i, &FRAME, wo);
    let mut hist = vec![0.0; NT * NP + 1];
    for (lobe, alpha, t) in ggx_lobes(&s) {
        let p = f64::from(w[lobe.index()]);
        if p > 0.0 {
            let h = ggx_histogram(v3(wo), v3(t), f64::from(alpha[0]), f64::from(alpha[1]), g1);
            hist.iter_mut().zip(h).for_each(|(a, b)| *a += p * b);
        }
    }
    let p_cos = f64::from(w[Lobe::Sheen.index()] + w[Lobe::Diffuse.index()]);
    if p_cos > 0.0 {
        let h = cosine_histogram();
        hist.iter_mut().zip(h).for_each(|(a, b)| *a += p_cos * b);
    }
    hist
}

// ---------------------------------------------------------------------------------------------
// Materials.

fn only_specular(roughness: f32, anisotropy: f32) -> SurfaceInputs {
    SurfaceInputs {
        base: 0.0,
        specular_roughness: roughness,
        specular_anisotropy: anisotropy,
        specular_rotation: 0.0,
        ..SurfaceInputs::MATERIALX_DEFAULT
    }
}

fn only_metal(roughness: f32, anisotropy: f32) -> SurfaceInputs {
    SurfaceInputs {
        metalness: 1.0,
        base_color: [0.9, 0.7, 0.4],
        ..only_specular(roughness, anisotropy)
    }
}

fn only_coat(roughness: f32, anisotropy: f32) -> SurfaceInputs {
    SurfaceInputs {
        base: 0.0,
        specular: 0.0,
        coat: 1.0,
        coat_roughness: roughness,
        coat_anisotropy: anisotropy,
        ..SurfaceInputs::MATERIALX_DEFAULT
    }
}

fn mixed(roughness: f32, anisotropy: f32) -> SurfaceInputs {
    SurfaceInputs {
        specular_roughness: roughness,
        specular_anisotropy: anisotropy,
        coat_roughness: roughness.max(0.1),
        coat_anisotropy: anisotropy,
        thin_film_thickness: 0.0,
        ..rich()
    }
}

// ---------------------------------------------------------------------------------------------
// Tests.

#[test]
fn counter_rng_is_uniform_deterministic_and_matches_an_independent_pcg4d() {
    // Independent pcg4d transcription in u64 arithmetic mod 2^32.
    fn reference(v: [u32; 4]) -> [u32; 4] {
        let m = |a: u64| a & 0xffff_ffff;
        let mut v = v.map(|x| m(u64::from(x) * 1_664_525 + 1_013_904_223));
        let mix = |v: &mut [u64; 4]| {
            v[0] = m(v[0] + v[1] * v[3]);
            v[1] = m(v[1] + v[2] * v[0]);
            v[2] = m(v[2] + v[0] * v[1]);
            v[3] = m(v[3] + v[1] * v[2]);
        };
        mix(&mut v);
        for x in v.iter_mut() {
            *x ^= *x >> 16;
        }
        mix(&mut v);
        v.map(|x| x as u32)
    }
    for input in [[0, 0, 0, 0], [1, 2, 3, 4], [u32::MAX, 7, 1 << 31, 12345]] {
        assert_eq!(pcg4d(input), reference(input), "{input:?}");
    }
    assert_eq!(u32_to_unit(0), 0.0);
    assert!(u32_to_unit(u32::MAX) < 1.0);
    assert_eq!(u32_to_unit(u32::MAX), 1.0 - 1.0 / 16_777_216.0);
    // Deterministic and seed-sensitive.
    assert_eq!(rng(3, 4, 5, 6, 7).to_bits(), rng(3, 4, 5, 6, 7).to_bits());
    assert_ne!(rng(3, 4, 5, 6, 7), rng(3, 4, 5, 6, 8));
    // Uniformity (64 bins) and decorrelation of consecutive dimensions and pixels.
    let n = 1usize << 16;
    let mut bins = vec![0u64; 64];
    let (mut s0, mut s1, mut s01, mut s00, mut s11) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for k in 0..n as u32 {
        let a = f64::from(rng(k % 256, k / 256, 0, 0, 0));
        let b = f64::from(rng(k % 256, k / 256, 0, 1, 0));
        bins[(a * 64.0) as usize] += 1;
        s0 += a;
        s1 += b;
        s01 += a * b;
        s00 += a * a;
        s11 += b * b;
    }
    let (p, chi2, _) = chi_square_p(&bins, &vec![1.0 / 64.0; 64], n as f64);
    let nf = n as f64;
    let cov = s01 / nf - (s0 / nf) * (s1 / nf);
    let corr = cov / ((s00 / nf - (s0 / nf).powi(2)) * (s11 / nf - (s1 / nf).powi(2))).sqrt();
    println!("rng: chi2 {chi2:.1} (p {p:.3}), corr(dim0, dim1) {corr:.2e}");
    assert!(p > 1e-4, "rng uniformity p {p}");
    assert!(corr.abs() < 0.02, "rng correlation {corr}");
}

#[test]
fn sobol_points_match_the_joe_kuo_direction_tables() {
    // Independent reference: direction numbers built as a table from the Joe-Kuo parameters
    // (`s` degree, `a` coefficients, `m` initial numbers), then the textbook XOR over index bits.
    fn directions(s: usize, a: u32, m: [u32; 3]) -> [u32; 32] {
        let mut v = [0u32; 32];
        for i in 0..32 {
            v[i] = if s == 0 {
                1 << (31 - i)
            } else if i < s {
                m[i] << (31 - i)
            } else {
                let mut x = v[i - s] ^ (v[i - s] >> s);
                for k in 1..s {
                    if (a >> (s - 1 - k)) & 1 == 1 {
                        x ^= v[i - k];
                    }
                }
                x
            };
        }
        v
    }
    let tables = [
        directions(0, 0, [0; 3]),
        directions(1, 0, [1, 0, 0]),
        directions(2, 1, [1, 3, 0]),
        directions(3, 1, [1, 3, 1]),
    ];
    let reference = |index: u32, dim: usize| {
        (0..32)
            .filter(|bit| (index >> bit) & 1 == 1)
            .fold(0u32, |x, bit| x ^ tables[dim][bit])
    };
    for dim in 0..4u32 {
        for index in (0..4096).chain([u32::MAX, 1 << 31, 0xdead_beef, 0x7fff_ffff]) {
            assert_eq!(
                sobol_point(index, dim),
                reference(index, dim as usize),
                "dim {dim} index {index}"
            );
        }
    }
    // Published unscrambled points: the (0,2)-sequence of dimensions 0 and 1.
    let point = |i, d| f64::from(sobol_point(i, d)) / 4_294_967_296.0;
    assert_eq!([point(1, 0), point(2, 0), point(3, 0)], [0.5, 0.25, 0.75]);
    assert_eq!([point(1, 1), point(2, 1), point(3, 1)], [0.5, 0.75, 0.25]);
    assert_eq!(point(1, 2), 0.5);
}

#[test]
fn sobol_samples_are_stratified_per_pixel_and_decorrelated() {
    // Any 16 consecutive samples of one pixel hit every 1/4 x 1/4 cell of a dimension pair of
    // one group once (scrambled and shuffled (0,2)-net), unlike white noise.
    for (px, py) in [(0, 0), (17, 3), (1023, 511)] {
        for dims in [(0, 1), (4, 5), (2, 3)] {
            let mut cells = [0u32; 16];
            for i in 0..16 {
                let (u, v) = (sobol(px, py, i, dims.0), sobol(px, py, i, dims.1));
                assert!((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v));
                cells[(u * 4.0) as usize * 4 + (v * 4.0) as usize] += 1;
            }
            assert!(
                cells.iter().all(|&c| c == 1),
                "pixel {px},{py} dims {dims:?}: {cells:?}"
            );
        }
    }
    // Uniform over pixels, groups independent, neighbouring pixels do not share samples.
    let n = 1u32 << 14;
    let (mut ma, mut mb, mut mab, mut equal) = (0.0f64, 0.0f64, 0.0f64, 0);
    for i in 0..n {
        let a = f64::from(sobol(i, 7, 3, 0));
        let b = f64::from(sobol(i, 7, 3, 4));
        ma += a;
        mb += b;
        mab += a * b;
        equal += u32::from(sobol(i, 7, 3, 0) == sobol(i + 1, 7, 3, 0));
    }
    let [ma, mb, mab] = [ma, mb, mab].map(|m| m / f64::from(n));
    assert!(
        (ma - 0.5).abs() < 0.01 && (mb - 0.5).abs() < 0.01,
        "{ma} {mb}"
    );
    assert!(
        (mab - ma * mb).abs() < 0.01,
        "groups must be independent: {mab}"
    );
    assert!(equal < 4, "neighbouring pixels must not share samples");
}

#[test]
fn cosine_and_cone_samplers_match_their_densities() {
    let mut inv_pdf = Vec::new();
    for a in 0..64 {
        for b in 0..64 {
            let xi = [(a as f32 + 0.5) / 64.0, (b as f32 + 0.5) / 64.0];
            let w = mx_cosine_sample_hemisphere(xi);
            let len = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-6 && w[2] > 0.0);
            let c = f64::from(w[2]);
            inv_pdf.push(c * c / f64::from(mx_cosine_hemisphere_pdf(w[2])));
        }
    }
    // E[g / pdf] over the sampler = the integral of g = cos^2 over the hemisphere, 2 pi / 3.
    let (m, _) = mean_se(&inv_pdf);
    assert!(
        (m - 2.0 * PI / 3.0).abs() < 1e-3,
        "cosine E[cos^2 / pdf] {m}"
    );
    assert_eq!(mx_cosine_hemisphere_pdf(-0.3), 0.0);
    // Sun cone: 0.53 degree diameter.
    let half = (0.53_f64 / 2.0).to_radians();
    let omc = (2.0 * (half / 2.0).sin().powi(2)) as f32;
    let omega = 2.0 * PI * f64::from(omc);
    assert!((1.0 / f64::from(cone_pdf(omc)) - omega).abs() < 1e-6 * omega);
    let mut max_angle = 0.0_f64;
    let mut mean_z = 0.0;
    for a in 0..32 {
        for b in 0..32 {
            let w = sample_cone([(a as f32 + 0.5) / 32.0, (b as f32 + 0.5) / 32.0], omc);
            let len = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-6);
            let sin_t = f64::from(w[0]).hypot(f64::from(w[1]));
            max_angle = max_angle.max(sin_t.asin());
            mean_z += 1.0 - f64::from(w[2]);
        }
    }
    // Uniform in solid angle: E[1 - cos] = (1 - cos_max) / 2.
    mean_z /= 1024.0;
    assert!(
        max_angle <= half * (1.0 + 1e-4),
        "cone angle {max_angle} > {half}"
    );
    assert!(
        (mean_z / f64::from(omc) - 0.5).abs() < 0.01,
        "cone E[1-cos] {mean_z}"
    );
}

#[test]
fn mis_heuristics() {
    assert_eq!(power_heuristic(2.0, 2.0), 0.5);
    assert_eq!(balance_heuristic(3.0, 3.0), 0.5);
    assert_eq!(power_heuristic(0.0, 0.0), 0.0);
    assert_eq!(balance_heuristic(0.0, 0.0), 0.0);
    for (a, b) in [(0.3_f32, 7.0_f32), (1e-3, 2.0), (5.0, 0.0)] {
        assert!((power_heuristic(a, b) + power_heuristic(b, a) - 1.0).abs() < 1e-6);
        assert!((balance_heuristic(a, b) + balance_heuristic(b, a) - 1.0).abs() < 1e-6);
    }
    assert!((power_heuristic(1.0, 3.0) - 0.1).abs() < 1e-7);
}

#[test]
fn anisotropic_g1_reduces_to_materialx_g1_and_differs_when_anisotropic() {
    for a in [1e-3_f32, 0.1, 0.5, 1.0] {
        for c in [0.05_f64, 0.3, 0.8, 1.0] {
            let v = dir(c, 0.7);
            let aniso = ggx_smith_g1_aniso(v, [a, a]);
            let iso = mx_ggx_smith_g1(v[2], a);
            assert!((aniso - iso).abs() < 2e-6, "a {a} c {c}: {aniso} vs {iso}");
        }
    }
    // Plan C1: along the long axis at grazing, the isotropic G1 of the average alpha is wrong.
    let v = dir(0.3, 0.0);
    let (ax, ay) = (1.0_f32, 0.035_f32);
    let aniso = ggx_smith_g1_aniso(v, [ax, ay]);
    let iso = mx_ggx_smith_g1(v[2], mx_average_alpha([ax, ay]));
    assert!(iso / aniso > 1.5, "iso {iso} vs aniso {aniso}");
}

#[test]
fn vndf_density_integrates_to_one_over_the_sphere() {
    // Integral of ggx_lobe_pdf (the library's f32 density) over the full sphere of wi, by the
    // slope-space quadrature of `ggx_histogram` with the library density as the integrand.
    let mut worst = 0.0_f64;
    let mut worst_iso = 0.0_f64;
    let mut max_below = 0.0_f64;
    let n = [0.0_f32, 0.0, 1.0];
    let t = [1.0_f32, 0.0, 0.0];
    for roughness in [0.05_f32, 0.2, 0.5, 1.0] {
        for anisotropy in [0.0_f32, 0.8, 0.98] {
            let alpha =
                standard_surface_bsdf::microfacet::mx_roughness_anisotropy(roughness, anisotropy);
            let (ax, ay) = (f64::from(alpha[0]), f64::from(alpha[1]));
            for (c, phi) in [(0.9, 0.0), (0.3, 0.0), (0.3, 1.1), (0.05, 0.5)] {
                let wo = dir(c, phi);
                let v = v3(wo);
                let g_ratio = g1_iso_f64(v[2], (ax * ay).sqrt()) / g1_aniso_f64(v, ax, ay);
                let (nu, nphi) = (1024usize, 256usize);
                let dphi = 2.0 * PI / nphi as f64;
                let (mut total, mut below) = (0.0, 0.0);
                for iu in 0..nu {
                    let u = (iu as f64 + 0.5) / nu as f64;
                    let rho = u / (1.0 - u);
                    let drho = 1.0 / ((1.0 - u) * (1.0 - u) * nu as f64);
                    for ip in 0..nphi {
                        let ph = (ip as f64 + 0.5) * dphi;
                        let h = norm([-ax * rho * ph.cos(), -ay * rho * ph.sin(), 1.0]);
                        let vh = dot(v, h);
                        if vh <= 0.0 {
                            continue;
                        }
                        let l = [
                            2.0 * vh * h[0] - v[0],
                            2.0 * vh * h[1] - v[1],
                            2.0 * vh * h[2] - v[2],
                        ];
                        let wi = norm(l).map(|x| x as f32);
                        let p = f64::from(ggx_lobe_pdf(wo, wi, n, t, alpha));
                        // dw_i = 4 vh dw_h, dw_h = cos_h^3 ax ay rho drho dphi.
                        let m = p * 4.0 * vh * h[2].powi(3) * ax * ay * rho * drho * dphi;
                        total += m;
                        if l[2] <= 0.0 {
                            below += m;
                        }
                    }
                }
                worst = worst.max((total - 1.0).abs());
                worst_iso = worst_iso.max((total * g_ratio - 1.0).abs());
                max_below = max_below.max(below);
                assert!(
                    (total - 1.0).abs() < 1e-3,
                    "r {roughness} aniso {anisotropy} wo {wo:?}: integral {total}"
                );
            }
        }
    }
    println!(
        "VNDF density: max |integral - 1| {worst:.2e}; max below-horizon share {max_below:.4}; \
         MaterialX isotropic-G1 density max |integral - 1| {worst_iso:.3}"
    );
    // The isotropic-G1 density (MaterialX's FIS caller) is not normalised when anisotropic.
    assert!(worst_iso > 0.2, "isotropic G1 control {worst_iso}");
}

#[test]
fn pdf_matches_an_independent_f64_density() {
    let mut worst = 0.0_f64;
    for i in [
        mixed(0.3, 0.8),
        mixed(0.6, 0.0),
        only_coat(0.1, 0.98),
        only_metal(0.05, 0.5),
    ] {
        for (c, phi) in [(0.6_f64, 0.4_f64), (0.2, 2.0)] {
            let wo = dir(c, phi);
            let s_wo = layers(&i, &FRAME, wo);
            let w = lobe_weights(&i, &FRAME, wo);
            for (cl, pl) in [(0.61, 3.5), (0.9, 1.0), (0.3, 3.3), (0.05, 4.0), (1.0, 0.0)] {
                let wi = dir(cl, pl);
                let got = f64::from(pdf(&i, &FRAME, wo, wi));
                let mut want = 0.0;
                let mut min_alpha = f64::MAX;
                for (lobe, alpha, t) in ggx_lobes(&s_wo) {
                    let p = f64::from(w[lobe.index()]);
                    if p > 0.0 {
                        let (x, y) = frame([0.0, 0.0, 1.0], v3(t));
                        let loc = |a: [f32; 3]| {
                            let a = v3(a);
                            [dot(a, x), dot(a, y), a[2]]
                        };
                        let (ax, ay) = (f64::from(alpha[0]), f64::from(alpha[1]));
                        min_alpha = min_alpha.min(ax.min(ay));
                        want += p * ggx_density_f64(loc(wo), loc(wi), ax, ay, G1Model::Anisotropic);
                    }
                }
                want += f64::from(w[Lobe::Sheen.index()] + w[Lobe::Diffuse.index()])
                    * f64::from(wi[2])
                    / PI;
                // Plan R4 tolerance: 1e-5 relative, or 1e-3 relative for alpha < 1e-2 (f32 H.xy/alpha).
                let tol = if min_alpha < 1e-2 { 1e-3 } else { 1e-5 };
                let rel = (got - want).abs() / want.max(1e-30);
                worst = worst.max(rel);
                assert!(
                    rel < tol,
                    "{i:?}\nwo {wo:?} wi {wi:?}: {got} vs {want} (rel {rel:e})"
                );
            }
        }
    }
    println!("pdf vs independent f64 density: max relative error {worst:.2e}");
}

#[test]
fn sample_matches_pdf_chi_square() {
    // Plan §3: 32 x 64 (theta, phi) grid plus a below-horizon slot, p > 1e-4, expected counts
    // from the independent f64 density; the MaterialX isotropic-G1 density must be rejected
    // wherever it differs from the sampler (anisotropic lobes).
    let n = 1u32 << 16;
    let mut configs: Vec<(String, SurfaceInputs, [f32; 3])> = Vec::new();
    for r in [0.05_f32, 0.2, 0.5, 1.0] {
        for a in [0.0_f32, 0.8, 0.98] {
            let wo = if a > 0.0 {
                dir(0.3, 0.0)
            } else {
                dir(0.5, 0.3)
            };
            configs.push((format!("specular r {r} aniso {a}"), only_specular(r, a), wo));
        }
    }
    for (r, a) in [(0.2_f32, 0.0_f32), (0.5, 0.8), (1.0, 0.98)] {
        configs.push((
            format!("metal r {r} aniso {a}"),
            only_metal(r, a),
            dir(0.3, 0.0),
        ));
        configs.push((
            format!("coat r {r} aniso {a}"),
            only_coat(r, a),
            dir(0.3, 0.0),
        ));
        configs.push((format!("mix r {r} aniso {a}"), mixed(r, a), dir(0.4, 0.2)));
    }
    configs.push((
        "diffuse".into(),
        SurfaceInputs {
            specular: 0.0,
            ..SurfaceInputs::MATERIALX_DEFAULT
        },
        dir(0.5, 0.0),
    ));
    configs.push((
        "sheen".into(),
        SurfaceInputs {
            base: 0.0,
            specular: 0.0,
            sheen: 1.0,
            ..SurfaceInputs::MATERIALX_DEFAULT
        },
        dir(0.5, 0.0),
    ));
    let mut rejected_iso = 0;
    let mut min_p = 1.0_f64;
    for (stream, (name, i, wo)) in configs.iter().enumerate() {
        let weights = lobe_weights(i, &FRAME, *wo);
        let mut observed = vec![0u64; NT * NP + 1];
        for k in 0..n {
            let s = sample_with(i, &FRAME, *wo, u3(stream as u32, k), &weights);
            assert!(!s.delta, "{name}: unexpected delta sample");
            let slot = if s.valid { bin_of(v3(s.wi)) } else { BELOW };
            assert!(
                s.valid || s.wi[2] <= 0.0,
                "{name}: invalid sample above the horizon"
            );
            observed[slot] += 1;
        }
        let expected = mixture_histogram(i, *wo, G1Model::Anisotropic);
        let total: f64 = expected.iter().sum();
        assert!((total - 1.0).abs() < 2e-3, "{name}: expected mass {total}");
        let (p, chi2, cells) = chi_square_p(&observed, &expected, f64::from(n));
        min_p = min_p.min(p);
        let below = observed[BELOW] as f64 / f64::from(n);
        println!(
            "chi2 {name}: {chi2:.1} over {cells} cells, p {p:.4}, below-horizon share {below:.4}"
        );
        assert!(
            p > 1e-4,
            "{name}: chi-square p {p} (chi2 {chi2}, {cells} cells)"
        );

        // Negative control: MaterialX's isotropic-G1 density.
        let iso = mixture_histogram(i, *wo, G1Model::MaterialXIsotropic);
        let differs = iso
            .iter()
            .zip(&expected)
            .map(|(a, b)| (a - b).abs())
            .sum::<f64>()
            > 0.05;
        if differs {
            let (p_iso, _, _) = chi_square_p(&observed, &iso, f64::from(n));
            assert!(
                p_iso < 1e-4,
                "{name}: isotropic-G1 density not rejected (p {p_iso})"
            );
            rejected_iso += 1;
        }
    }
    println!(
        "chi-square: min p {min_p:.4}; isotropic-G1 density rejected in {rejected_iso} configs"
    );
    assert!(
        rejected_iso >= 6,
        "only {rejected_iso} anisotropic configs discriminate"
    );
}

#[test]
fn sample_is_consistent_with_pdf_and_eval() {
    let mut count = 0;
    for i in [
        mixed(0.05, 0.0),
        mixed(0.3, 0.8),
        mixed(0.7, 0.98),
        rich(),
        only_metal(0.08, 0.5),
    ] {
        for (c, phi) in [(0.9_f64, 0.0_f64), (0.4, 1.0), (0.08, 2.5)] {
            let wo = dir(c, phi);
            let w = lobe_weights(&i, &FRAME, wo);
            let s_layers = layers(&i, &FRAME, wo);
            let min_alpha = ggx_lobes(&s_layers)
                .iter()
                .filter(|(l, _, _)| w[l.index()] > 0.0)
                .map(|(_, a, _)| a[0].min(a[1]))
                .fold(1.0_f32, f32::min);
            let tol = if min_alpha < 1e-2 { 1e-3 } else { 1e-5 };
            for a in 0..8 {
                for b in 0..8 {
                    for d in 0..8 {
                        let u = [
                            (a as f32 + 0.5) / 8.0,
                            (b as f32 + 0.5) / 8.0,
                            (d as f32 + 0.5) / 8.0,
                        ];
                        let s = sample(&i, &FRAME, wo, u);
                        // Single source of the lobe weights (I12): bitwise identical.
                        assert_eq!(s, sample_with(&i, &FRAME, wo, u, &w));
                        if !s.valid {
                            assert!(s.wi[2] <= 0.0, "invalid sample above the horizon: {s:?}");
                            continue;
                        }
                        count += 1;
                        assert!(s.wi[2] > 0.0 && s.pdf > 0.0);
                        let p = pdf(&i, &FRAME, wo, s.wi);
                        assert_eq!(p.to_bits(), pdf_with(&i, &FRAME, wo, s.wi, &w).to_bits());
                        let rel = (f64::from(s.pdf) - f64::from(p)).abs() / f64::from(p);
                        assert!(rel <= tol, "pdf {} vs {p}", s.pdf);
                        let e = eval_light(&i, &FRAME, wo, s.wi).sum();
                        for (wk, ek) in s.weight.iter().zip(e) {
                            let want = f64::from(ek) / f64::from(p);
                            let err = (f64::from(*wk) - want).abs() / want.abs().max(1e-12);
                            assert!(err <= tol.max(1e-6), "weight {wk} vs {want}");
                        }
                    }
                }
            }
        }
    }
    assert!(count > 1000, "only {count} valid samples");
}

/// Expected mirror energy `F(NdotV) comp w` of a delta dielectric (independent assembly).
/// The delta (smooth) dielectric mirror's reflectance: the exact Fresnel, uncompensated (a
/// smooth interface has no microfacet multiple scattering).
fn delta_dielectric_expected(ndotv: f32, ior: f32) -> [f32; 3] {
    mx_compute_fresnel(ndotv, &mx_init_fresnel_dielectric(ior, 0.0, 1.5))
}

#[test]
fn delta_lobes_sample_the_mirror_with_fresnel_weight() {
    let wo = dir(0.6, 0.4);
    let mirror = [-wo[0], -wo[1], wo[2]];
    // Metal 1, roughness 0: every sample is the delta metal lobe with p = 1.
    let metal = white(|i| {
        i.metalness = 1.0;
        i.specular_roughness = 0.0;
        i.base_color = [0.9, 0.6, 0.3];
    });
    let s_layers = layers(&metal, &FRAME, wo);
    let fd = mx_init_fresnel_conductor(s_layers.metal.ior_n, s_layers.metal.ior_k, 0.0, 1.5);
    // The smooth mirror reflects the exact Fresnel, without microfacet compensation.
    let f = mx_compute_fresnel(wo[2], &fd);
    for k in 0..64 {
        let s = sample(&metal, &FRAME, wo, u3(900, k));
        assert!(s.valid && s.delta && s.lobe == Lobe::Metal, "{s:?}");
        assert!((s.pdf - 1.0).abs() < 1e-6);
        for (a, b) in s.wi.iter().zip(mirror) {
            assert!((a - b).abs() < 1e-6);
        }
        for (wk, fk) in s.weight.iter().zip(f) {
            assert!(
                (f64::from(*wk) - f64::from(fk)).abs() < 1e-5,
                "{wk} vs {fk}"
            );
        }
    }
    // A delta-only material has no density and no direct response anywhere.
    for (c, phi) in [(0.6, 3.54), (0.9, 1.0), (0.2, 0.0)] {
        assert_eq!(pdf(&metal, &FRAME, wo, dir(c, phi)), 0.0);
        assert_eq!(lum(eval_light(&metal, &FRAME, wo, dir(c, phi)).sum()), 0.0);
    }
}

#[test]
fn delta_coat_over_white_diffuse_oracle() {
    // Plan §3 (R3): delta coat 1 over a white Lambert base. The coat branch returns the exact F;
    // the base branch's expected energy is the coat throughput 1 - F (a smooth interface:
    // `exact_interface`), within 3 sigma; branch frequencies follow p_k.
    let i = white(|i| {
        i.specular = 0.0;
        i.coat = 1.0;
        i.coat_roughness = 0.0;
    });
    for c in [0.2_f64, 0.6, 1.0] {
        let wo = dir(c, 0.0);
        let w = lobe_weights(&i, &FRAME, wo);
        let (p_coat, p_diffuse) = (w[Lobe::Coat.index()], w[Lobe::Diffuse.index()]);
        let n = 1u32 << 16;
        let mut coat_hits = 0u32;
        let mut base = Vec::with_capacity(n as usize);
        let coat_expected = delta_dielectric_expected(wo[2], 1.5);
        for k in 0..n {
            let s = sample_with(&i, &FRAME, wo, u3(100 + (c * 10.0) as u32, k), &w);
            assert!(s.valid);
            match s.lobe {
                Lobe::Coat => {
                    assert!(s.delta && (s.pdf - p_coat).abs() < 1e-7);
                    coat_hits += 1;
                    for (wk, ek) in s.weight.iter().zip(coat_expected) {
                        assert!((wk * p_coat - ek).abs() < 1e-5, "{wk} * {p_coat} vs {ek}");
                    }
                    base.push(0.0);
                }
                Lobe::Diffuse => {
                    assert!(!s.delta);
                    base.push(lum(s.weight));
                }
                other => panic!("unexpected lobe {other:?}"),
            }
        }
        let frac = f64::from(coat_hits) / f64::from(n);
        let sigma = (f64::from(p_coat) * f64::from(p_diffuse) / f64::from(n)).sqrt();
        assert!(
            (frac - f64::from(p_coat)).abs() < 4.0 * sigma,
            "coat fraction {frac} vs {p_coat}"
        );
        let only_irradiance = Environment {
            irradiance: [1.0; 3],
            specular_radiance: [0.0; 3],
            coat_radiance: [0.0; 3],
            transmission_radiance: [0.0; 3],
        };
        let want = lum(eval_environment(&i, &FRAME, wo, &only_irradiance).base);
        let (m, se) = mean_se(&base);
        println!("delta coat NdotV {c}: base energy {m:.5} +- {se:.5} vs 1 - E comp {want:.5}");
        assert!(
            (m - want).abs() < 3.0 * se + 1e-4,
            "{m} vs {want} (se {se})"
        );
    }
}

#[test]
fn white_furnace_of_the_sampler() {
    let n = 1u32 << 16;
    let mut worst = 0.0_f64;
    let env = Environment {
        irradiance: [1.0; 3],
        specular_radiance: [1.0; 3],
        coat_radiance: [1.0; 3],
        transmission_radiance: [1.0; 3],
    };
    let configs: [(&str, SurfaceInputs); 5] = [
        ("dielectric", white(|i| i.specular_roughness = 0.4)),
        (
            "metal",
            white(|i| {
                i.metalness = 1.0;
                i.specular_roughness = 0.5;
            }),
        ),
        (
            "coat",
            white(|i| {
                i.coat = 1.0;
                i.coat_roughness = 0.3;
            }),
        ),
        (
            "delta coat over rough base",
            white(|i| {
                i.coat = 1.0;
                i.coat_roughness = 0.0;
                i.specular_roughness = 0.6;
            }),
        ),
        (
            "coat over black",
            SurfaceInputs {
                base_color: [0.0; 3],
                specular: 0.0,
                coat: 1.0,
                coat_roughness: 0.3,
                ..white(|_| {})
            },
        ),
    ];
    for (stream, (name, i)) in configs.iter().enumerate() {
        for c in [0.3_f64, 0.8] {
            let wo = dir(c, 0.1);
            let w = lobe_weights(i, &FRAME, wo);
            let values: Vec<f64> = (0..n)
                .map(|k| {
                    let s = sample_with(
                        i,
                        &FRAME,
                        wo,
                        u3(200 + stream as u32 * 7 + (c * 10.0) as u32, k),
                        &w,
                    );
                    if s.valid { lum(s.weight) } else { 0.0 }
                })
                .collect();
            let (m, se) = mean_se(&values);
            let pre = lum(eval_environment(i, &FRAME, wo, &env).sum());
            println!(
                "furnace {name} NdotV {c}: E[weight] {m:.5} +- {se:.5}, pre-integrated {pre:.5}"
            );
            worst = worst.max(m);
            assert!(m <= 1.03 + 3.0 * se, "{name}: furnace {m}");
            // Against the pre-integrated environment path: MC error plus the measured
            // MaterialX albedo-fit error (0.0235, surface.rs integrated_direct_light test).
            assert!((m - pre).abs() < 3.0 * se + 0.03, "{name}: {m} vs {pre}");
        }
    }
    // Metal roughness 0: every sample carries exactly F comp (p = 1).
    let delta_metal = white(|i| {
        i.metalness = 1.0;
        i.specular_roughness = 0.0;
    });
    let wo = dir(0.7, 0.0);
    let s = sample(&delta_metal, &FRAME, wo, [0.3, 0.6, 0.9]);
    let e = lum(eval_environment(&delta_metal, &FRAME, wo, &env).specular);
    println!(
        "delta metal: weight {:.5}, pre-integrated FG comp {e:.5}; sampler max furnace {worst:.5}",
        lum(s.weight)
    );
    assert!(s.delta && lum(s.weight) <= 1.0 + 1e-5);
}

#[test]
fn pdf_upper_hemisphere_mass_is_one_minus_the_below_horizon_share() {
    // Monte Carlo over the uniform sphere: E[4 pi pdf] = sum_k p_k (1 - below_k), where the
    // below-horizon share of each GGX lobe comes from the independent f64 histogram.
    for (idx, i) in [mixed(0.5, 0.0), mixed(0.8, 0.8), rich()]
        .iter()
        .enumerate()
    {
        let wo = dir(0.3, 0.0);
        let w = lobe_weights(i, &FRAME, wo);
        let s_layers = layers(i, &FRAME, wo);
        let mut want = f64::from(w[Lobe::Sheen.index()] + w[Lobe::Diffuse.index()]);
        for (lobe, alpha, t) in ggx_lobes(&s_layers) {
            let p = f64::from(w[lobe.index()]);
            if p > 0.0 {
                let h = ggx_histogram(
                    v3(wo),
                    v3(t),
                    f64::from(alpha[0]),
                    f64::from(alpha[1]),
                    G1Model::Anisotropic,
                );
                want += p * (1.0 - h[BELOW]);
            }
        }
        let n = 1u32 << 16;
        let values: Vec<f64> = (0..n)
            .map(|k| {
                let [a, b, _] = u3(300 + idx as u32, k);
                let z = 1.0 - 2.0 * f64::from(a);
                let r = (1.0 - z * z).max(0.0).sqrt();
                let phi = 2.0 * PI * f64::from(b);
                let wi = [(r * phi.cos()) as f32, (r * phi.sin()) as f32, z as f32];
                4.0 * PI * f64::from(pdf_with(i, &FRAME, wo, wi, &w))
            })
            .collect();
        let (m, se) = mean_se(&values);
        println!("uniform-sphere E[4 pi pdf] {m:.4} +- {se:.4} vs sum p_k (1 - below_k) {want:.4}");
        assert!((m - want).abs() < 3.0 * se, "{m} vs {want} (se {se})");
    }
}

#[test]
fn invalid_views_and_empty_materials_give_invalid_samples() {
    let i = rich();
    let below = [0.3_f32, 0.0, -0.95];
    let s = sample(&i, &FRAME, below, [0.5, 0.5, 0.5]);
    assert!(!s.valid && s.weight == [0.0; 3]);
    assert_eq!(pdf(&i, &FRAME, below, [0.0, 0.0, 1.0]), 0.0);
    let black = SurfaceInputs {
        base: 0.0,
        specular: 0.0,
        ..SurfaceInputs::MATERIALX_DEFAULT
    };
    assert_eq!(
        sample(&black, &FRAME, dir(0.5, 0.0), [0.1, 0.2, 0.3]),
        Sample::INVALID
    );
    // Positive control: a real material gives valid samples from the same uniforms.
    assert!(sample(&i, &FRAME, dir(0.5, 0.0), [0.1, 0.2, 0.3]).valid);
}
