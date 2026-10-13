//! Shared deterministic helpers of the CPU oracle tests (no RNG, no threads).
#![allow(dead_code)]

use standard_surface_bsdf::sampling::rng;
use standard_surface_bsdf::{ShadingFrame, SurfaceInputs, ThinFilmEnergy};

/// The frame used by the oracles: `n = +Z`, tangent `+X`.
pub const FRAME: ShadingFrame = ShadingFrame {
    n: [0.0, 0.0, 1.0],
    tangent: [1.0, 0.0, 0.0],
    inside: false,
    curvature: 0.0,
};

/// Unit direction with `cos(theta) = cos_theta` about `+Z` at azimuth `phi` (radians).
pub fn dir(cos_theta: f64, phi: f64) -> [f32; 3] {
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    [
        (sin_theta * phi.cos()) as f32,
        (sin_theta * phi.sin()) as f32,
        cos_theta as f32,
    ]
}

pub fn lum(c: [f32; 3]) -> f64 {
    let [r, g, b] = c;
    (f64::from(r) + f64::from(g) + f64::from(b)) / 3.0
}

pub fn finite3(c: [f32; 3]) -> bool {
    c.iter().all(|v| v.is_finite())
}

pub fn non_negative3(c: [f32; 3]) -> bool {
    c.iter().all(|v| *v >= 0.0)
}

pub fn max_abs_diff3(a: [f32; 3], b: [f32; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (f64::from(*x) - f64::from(y)).abs())
        .fold(0.0, f64::max)
}

pub fn max_rel_diff3(a: [f32; 3], b: [f32; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| {
            let (x, y) = (f64::from(*x), f64::from(y));
            (x - y).abs() / x.abs().max(y.abs()).max(1e-30)
        })
        .fold(0.0, f64::max)
}

pub fn bits3(c: [f32; 3]) -> [u32; 3] {
    c.map(f32::to_bits)
}

/// A white, energy-conserving test material: every weight 1 unless changed by `f`.
pub fn white(f: impl FnOnce(&mut SurfaceInputs)) -> SurfaceInputs {
    let mut i = SurfaceInputs {
        base: 1.0,
        base_color: [1.0; 3],
        specular: 1.0,
        specular_color: [1.0; 3],
        sheen_color: [1.0; 3],
        coat_color: [1.0; 3],
        ..SurfaceInputs::MATERIALX_DEFAULT
    };
    f(&mut i);
    i
}

/// Every lobe switched on with distinct colours, the richest non-delta configuration.
pub fn rich() -> SurfaceInputs {
    SurfaceInputs {
        base: 0.9,
        base_color: [0.7, 0.4, 0.2],
        diffuse_roughness: 0.5,
        metalness: 0.3,
        specular: 0.8,
        specular_color: [0.9, 0.95, 1.0],
        specular_roughness: 0.35,
        specular_ior: 1.6,
        specular_anisotropy: 0.5,
        specular_rotation: 0.1,
        transmission: 0.0,
        transmission_color: [1.0, 1.0, 1.0],
        transmission_extra_roughness: 0.0,
        subsurface: 0.0,
        subsurface_color: [1.0, 1.0, 1.0],
        subsurface_radius: [1.0, 1.0, 1.0],
        subsurface_scale: 1.0,
        subsurface_anisotropy: 0.0,
        sheen: 0.6,
        sheen_color: [0.8, 0.9, 1.0],
        sheen_roughness: 0.4,
        coat: 0.7,
        coat_color: [0.9, 0.8, 0.7],
        coat_roughness: 0.25,
        coat_anisotropy: 0.3,
        coat_rotation: 0.2,
        coat_ior: 1.45,
        coat_affect_color: 0.5,
        coat_affect_roughness: 0.4,
        thin_film_thickness: 350.0,
        thin_film_ior: 1.4,
        emission: 0.5,
        emission_color: [1.0, 0.5, 0.25],
        thin_film_energy: ThinFilmEnergy::MaterialX,
    }
}

/// Midpoint quadrature of `f(wi) dw` over the upper hemisphere in `(cos theta, phi)`, which has
/// uniform solid-angle cells (`dw = d(cos theta) d(phi)`).
pub fn hemisphere_integral(n_cos: usize, n_phi: usize, mut f: impl FnMut([f32; 3]) -> f64) -> f64 {
    let d_cos = 1.0 / n_cos as f64;
    let d_phi = 2.0 * std::f64::consts::PI / n_phi as f64;
    let mut sum = 0.0;
    for a in 0..n_cos {
        let c = (a as f64 + 0.5) * d_cos;
        for b in 0..n_phi {
            let phi = (b as f64 + 0.5) * d_phi;
            sum += f(dir(c, phi));
        }
    }
    sum * d_cos * d_phi
}

/// Exact unpolarised dielectric Fresnel in f64 (Born & Wolf): the independent oracle of
/// `mx_fresnel_dielectric` and of smooth interfaces.
pub fn fresnel_dielectric_f64(cos_i: f64, eta: f64) -> f64 {
    let sin_t2 = (1.0 - cos_i * cos_i) / (eta * eta);
    if sin_t2 >= 1.0 {
        return 1.0;
    }
    let cos_t = (1.0 - sin_t2).sqrt();
    let rs = (cos_i - eta * cos_t) / (cos_i + eta * cos_t);
    let rp = (eta * cos_i - cos_t) / (eta * cos_i + cos_t);
    0.5 * (rs * rs + rp * rp)
}

/// Independent f64 transcription of the GGX albedo fit (`mx_microfacet_specular.glsl:91-108`).
pub fn ggx_albedo_fit_f64(x: f64, y: f64) -> (f64, f64) {
    let c: [[f64; 4]; 9] = [
        [0.1003, 0.9345, 1.0, 1.0],
        [-0.6303, -2.323, -1.765, 0.2281],
        [9.748, 2.229, 8.263, 15.94],
        [-2.038, -3.748, 11.53, -55.83],
        [29.34, 1.424, 28.96, 13.08],
        [-8.245, -0.7684, -7.507, 41.26],
        [-26.44, 1.436, -36.11, 54.9],
        [19.99, 0.2913, 15.86, 300.2],
        [-5.448, 0.6286, 33.37, -285.1],
    ];
    let t = [
        1.0,
        x,
        y,
        x * y,
        x * x,
        y * y,
        x * x * y,
        x * y * y,
        x * x * y * y,
    ];
    let mut r = [0.0; 4];
    for (row, w) in c.iter().zip(t) {
        for (acc, v) in r.iter_mut().zip(row) {
            *acc += v * w;
        }
    }
    ((r[0] / r[2]).clamp(0.0, 1.0), (r[1] / r[3]).clamp(0.0, 1.0))
}

// Deterministic uniforms and small statistics (sampling and transmission oracles).

/// Three uniforms for sample `k` of a test stream `stream`.
pub fn u3(stream: u32, k: u32) -> [f32; 3] {
    [
        rng(k, stream, 0, 0, 0x5eed),
        rng(k, stream, 0, 1, 0x5eed),
        rng(k, stream, 0, 2, 0x5eed),
    ]
}

/// Mean and standard error of the mean.
pub fn mean_se(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let var = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n - 1.0);
    (mean, (var / n).sqrt())
}

/// ln Gamma (Lanczos, g = 7).
pub fn ln_gamma(x: f64) -> f64 {
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + 7.5;
    for (k, c) in C.iter().enumerate().skip(1) {
        a += c / (x + k as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// Regularised upper incomplete gamma Q(a, x) (Numerical Recipes `gammq`).
pub fn gamma_q(a: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    if x < a + 1.0 {
        let (mut sum, mut del, mut ap) = (1.0 / a, 1.0 / a, a);
        for _ in 0..10_000 {
            ap += 1.0;
            del *= x / ap;
            sum += del;
            if del.abs() < sum.abs() * 1e-15 {
                break;
            }
        }
        1.0 - sum * (-x + a * x.ln() - ln_gamma(a)).exp()
    } else {
        let tiny = 1e-300;
        let mut b = x + 1.0 - a;
        let mut c = 1.0 / tiny;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..10_000 {
            let an = -(i as f64) * (i as f64 - a);
            b += 2.0;
            d = an * d + b;
            if d.abs() < tiny {
                d = tiny;
            }
            c = b + an / c;
            if c.abs() < tiny {
                c = tiny;
            }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < 1e-15 {
                break;
            }
        }
        (-x + a * x.ln() - ln_gamma(a)).exp() * h
    }
}

/// Pearson chi-square p-value of `observed` counts against `expected` fractions (which sum
/// to 1); cells with an expected count below 5 are pooled (Mitsuba/PBRT practice).
pub fn chi_square_p(observed: &[u64], expected: &[f64], n: f64) -> (f64, f64, usize) {
    let mut chi2 = 0.0;
    let mut cells = 0usize;
    let (mut pooled_o, mut pooled_e) = (0.0, 0.0);
    for (o, e) in observed.iter().zip(expected) {
        let e = e * n;
        let o = *o as f64;
        if e < 5.0 {
            pooled_o += o;
            pooled_e += e;
        } else {
            chi2 += (o - e) * (o - e) / e;
            cells += 1;
        }
    }
    if pooled_e >= 5.0 {
        chi2 += (pooled_o - pooled_e) * (pooled_o - pooled_e) / pooled_e;
        cells += 1;
    } else if pooled_o > 0.0 && pooled_e < 1e-3 {
        // Samples where the density says (almost) nothing can land: a hard failure.
        chi2 += pooled_o * 1e6;
        cells += 1;
    }
    let df = cells.saturating_sub(1).max(1) as f64;
    (gamma_q(df / 2.0, chi2 / 2.0), chi2, cells)
}
