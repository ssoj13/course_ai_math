//! The 3D LUT body, written once for all four languages: port of OCIO `Lut3DOpGPU.cpp`
//! (`GetLut3DGPUShaderProgram`).
//!
//! - **Tetrahedral** (`Tetrahedral` / `Best`): the texture is sampled with NEAREST filtering at
//!   four lattice nodes (the cube's two diagonal corners and the two picked by the ordering of
//!   the fractional parts) and the four barycentric weights are applied in the shader, as OCIO
//!   does ("not subject to the 8-bit quantization of the fractional weights" of hardware
//!   filtering). OCIO's texture is Blue-fastest, hence its `.zyx` and "inverted" increments;
//!   this port's texture is Red-fastest, so the same six cases increment R, G, B directly.
//! - **Trilinear** (everything else; OCIO v2 evaluates Lut3D `Nearest` as trilinear on the CPU):
//!   the hardware's linear filter at the texel-centre coordinate `(t * (N - 1) + 0.5) / N`.
//!
//! The input is normalised by the LUT's domain and clamped to [0, 1] first, as the CPU
//! (`vfx_lut::interpolate_lattice`) clamps; OCIO's shader relies on the sampler's clamp-to-edge,
//! which gives the same values inside the domain.

use super::dialect::{Dialect, n};

fn v3(d: &Dialect, v: [f64; 3]) -> String {
    format!("{}({}, {}, {})", d.v3, n(v[0]), n(v[1]), n(v[2]))
}

/// Shader lines for one Lut3D op on an `size`^3 Red-fastest texture. `sample(coord)` is this
/// language's 3D texture read (a four-component expression) at a three-component coordinate.
pub fn emit(
    d: &Dialect,
    size: usize,
    domain_min: [f32; 3],
    domain_max: [f32; 3],
    tetrahedral: bool,
    sample: &dyn Fn(&str) -> String,
) -> Vec<String> {
    let dim = size as f64;
    let inv_range: [f64; 3] = std::array::from_fn(|c| {
        let r = (domain_max[c] - domain_min[c]) as f64;
        if r.abs() < 1e-10 { 0.0 } else { 1.0 / r }
    });
    let mut out = vec![
        format!(
            "{{ // Lut3D, {} (OCIO Lut3DOpGPU)",
            if tetrahedral {
                "tetrahedral"
            } else {
                "trilinear"
            }
        ),
        d.decl(
            "lut_t",
            &format!(
                "clamp((color.rgb - {}) * {}, {}, {})",
                v3(d, domain_min.map(|v| v as f64)),
                v3(d, inv_range),
                d.k(0.0),
                d.k(1.0)
            ),
        ),
    ];
    if !tetrahedral {
        out.push(d.decl(
            "lut_c",
            &format!("(lut_t * {} + {}) / {}", d.k(dim - 1.0), d.k(0.5), d.k(dim)),
        ));
        out.push(d.set(&format!("{}.rgb", sample("lut_c"))));
        out.push("}".into());
        return out;
    }
    let incr = 1.0 / dim;
    let at = |dv: [f64; 3]| sample(&format!("base_c + {}", v3(d, dv.map(|x| x * incr))));
    out.push(d.decl("coords", &format!("lut_t * {}", d.k(dim - 1.0))));
    out.push(d.decl("base_i", "floor(coords)"));
    out.push(d.decl("frac", "coords - base_i"));
    out.push(d.decl("base_c", &format!("(base_i + {}) / {}", d.k(0.5), d.k(dim))));
    out.push(d.decl("v1", &format!("{}.rgb", sample("base_c"))));
    out.push(d.decl("v4", &format!("{}.rgb", at([1.0, 1.0, 1.0]))));
    out.push(d.var3("v2", "v1"));
    out.push(d.var3("v3", "v1"));
    for f in ["f1", "f2", "f3", "f4"] {
        out.push(d.var_f(f, "0.0"));
    }
    // One of OCIO's six cases: the two middle corners and the weights.
    let case = |v2: [f64; 3], v3c: [f64; 3], f: [&str; 4]| -> Vec<String> {
        vec![
            format!("        v2 = {}.rgb;", at(v2)),
            format!("        v3 = {}.rgb;", at(v3c)),
            format!("        f1 = {};", f[0]),
            format!("        f2 = {};", f[1]),
            format!("        f3 = {};", f[2]),
            format!("        f4 = {};", f[3]),
        ]
    };
    let (r, g, b) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let rg = [1.0, 1.0, 0.0];
    let rb = [1.0, 0.0, 1.0];
    let gb = [0.0, 1.0, 1.0];
    out.push("    if (frac.r >= frac.g) {".into());
    out.push("      if (frac.g >= frac.b) { // R > G > B".into());
    out.extend(case(
        r,
        rg,
        [
            "1.0 - frac.r",
            "frac.r - frac.g",
            "frac.g - frac.b",
            "frac.b",
        ],
    ));
    out.push("      } else if (frac.r >= frac.b) { // R > B > G".into());
    out.extend(case(
        r,
        rb,
        [
            "1.0 - frac.r",
            "frac.r - frac.b",
            "frac.b - frac.g",
            "frac.g",
        ],
    ));
    out.push("      } else { // B > R > G".into());
    out.extend(case(
        b,
        rb,
        [
            "1.0 - frac.b",
            "frac.b - frac.r",
            "frac.r - frac.g",
            "frac.g",
        ],
    ));
    out.push("      }".into());
    out.push("    } else {".into());
    out.push("      if (frac.g <= frac.b) { // B > G > R".into());
    out.extend(case(
        b,
        gb,
        [
            "1.0 - frac.b",
            "frac.b - frac.g",
            "frac.g - frac.r",
            "frac.r",
        ],
    ));
    out.push("      } else if (frac.r >= frac.b) { // G > R > B".into());
    out.extend(case(
        g,
        rg,
        [
            "1.0 - frac.g",
            "frac.g - frac.r",
            "frac.r - frac.b",
            "frac.b",
        ],
    ));
    out.push("      } else { // G > B > R".into());
    out.extend(case(
        g,
        gb,
        [
            "1.0 - frac.g",
            "frac.g - frac.b",
            "frac.b - frac.r",
            "frac.r",
        ],
    ));
    out.push("      }".into());
    out.push("    }".into());
    out.push(d.set("f1 * v1 + f2 * v2 + f3 * v3 + f4 * v4"));
    out.push("}".into());
    out
}

#[cfg(test)]
mod tests {
    use super::super::dialect::{GLSL, HLSL, METAL, WGSL};
    use super::*;

    fn wgsl_module(tetrahedral: bool) -> String {
        let body = emit(&WGSL, 17, [0.0; 3], [1.0; 3], tetrahedral, &|c| {
            format!("textureSampleLevel(t, t_sampler, {c}, 0.0)")
        });
        format!(
            "@group(0) @binding(0) var t: texture_3d<f32>;\n@group(0) @binding(1) var t_sampler: sampler;\n\
             fn f(c: vec4<f32>) -> vec4<f32> {{\n var color = c;\n{}\n return color;\n}}\n",
            body.join("\n")
        )
    }

    /// Both bodies are valid WGSL (full naga validation).
    #[test]
    fn wgsl_bodies_validate() {
        for tetrahedral in [true, false] {
            let src = wgsl_module(tetrahedral);
            let module =
                naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{e}\n{src}"));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}\n{src}"));
        }
    }

    /// The four languages get the same statements (one body, four spellings).
    #[test]
    fn all_dialects_emit_the_same_statement_count() {
        for tetrahedral in [true, false] {
            let counts: Vec<usize> = [&GLSL, &HLSL, &METAL, &WGSL]
                .iter()
                .map(|d| {
                    emit(d, 9, [0.0; 3], [1.0; 3], tetrahedral, &|c| {
                        format!("s({c})")
                    })
                    .len()
                })
                .collect();
            assert!(counts.windows(2).all(|w| w[0] == w[1]), "{counts:?}");
        }
    }
}
