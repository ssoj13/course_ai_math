//! The shading languages' surface syntax, factored out of the emitters that use it.
//!
//! # Why this is a type and not four copies
//!
//! GLSL, HLSL, Metal and WGSL differ from each other in a handful of spellings — the vector type
//! name, whether a splat repeats its scalar, the local-binding keyword, `mix` vs `lerp`, whether a
//! swizzle can be assigned through, whether a ternary exists. They do not differ in colour science.
//!
//! Every emitter that predated this module wrote its own copy of the maths *around* those
//! spellings, and the copies drifted: seven transfer curves and the RGB<->HSY conversion shipped
//! wrong in all four languages simultaneously, which is precisely the failure mode that
//! cross-language comparison cannot detect. Naming the differences explicitly here lets the maths
//! be written once — see [`super::transfer`] and [`super::hsy`].
//!
//! Anything the four languages agree on (`pow`, `log`, `log2`, `exp`, `exp2`, `sqrt`, `abs`,
//! `sign`, `min`, `max`, `clamp`, `step`, `dot`, `floor`, and vector/scalar arithmetic) is written
//! directly by the callers rather than wrapped here.

/// Floor applied to the logarithm argument of the transfer curves (`gpu/transfer.rs`). OCIO's
/// log ops floor at [`vfx_core::consts::LOG_MIN`] instead ([`log_min`], [`Dialect::ln_log_op`]).
///
/// Matches the guard the already-correct sibling curves (ACEScct, LogC3, V-Log) use, so the
/// convention is uniform across the file rather than per-curve.
pub const LOG_FLOOR: f64 = 1e-10;

/// Cap on a natural-exponential argument. `exp(88) ~ 1.6e38`, just under `f32::MAX`.
const EXP_CAP: f64 = 88.0;

/// Cap on a base-2 exponential argument. `exp2(127) ~ 1.7e38`, just under `f32::MAX`.
const EXP2_CAP: f64 = 127.0;

use super::{GpuLanguage, GpuTextureType};

/// `log10(e)`, for turning the natural log into a base-10 log.
///
/// GLSL and WGSL have no `log10` builtin, so base-10 logs are always expressed as
/// `log(x) * LOG10_E` rather than branching on dialect.
const LOG10_E: f64 = std::f64::consts::LOG10_E;

/// `log2(10)`, for expressing `10^x` as `exp2(x * LOG2_10)`.
const LOG2_10: f64 = std::f64::consts::LOG2_10;

/// The surface-syntax differences between the four shading languages.
///
/// Deliberately minimal: every field below is a difference that actually exists between the four
/// backends. Anything the languages agree on (`pow`, `log`, `log2`, `exp`, `exp2`, `sqrt`, `abs`,
/// `sign`, `min`, `max`, `step`, and vector/scalar arithmetic) is written directly.
pub struct Dialect {
    /// Three-component float type name: `vec3`, `float3`, or `vec3<f32>`.
    pub v3: &'static str,
    /// Two-component float type name: `vec2`, `float2`, or `vec2<f32>` (2D texture coordinates).
    pub v2: &'static str,
    /// Whether a splat must repeat the scalar three times.
    ///
    /// HLSL's house style in this crate is the explicit `float3(a, a, a)`; GLSL, Metal and WGSL
    /// take a one-argument splat.
    pub splat_triple: bool,
    /// Local-binding keyword: `vec3 `, `float3 `, or WGSL's type-inferred `let `.
    pub decl_kw: &'static str,
    /// Linear-interpolation builtin: `mix` everywhere except HLSL's `lerp`.
    pub mix_fn: &'static str,
    /// Whether writing the result must rebuild the whole `vec4`.
    ///
    /// WGSL forbids assigning through a swizzle (`color.rgb = ...`), so it needs
    /// `color = vec4<f32>(rgb, color.a)`.
    pub rebuild_vec4: bool,
    /// Immutable scalar binding keyword: `float ` or WGSL's type-inferred `let `.
    pub decl_f: &'static str,
    /// Mutable scalar binding keyword.
    ///
    /// WGSL distinguishes `let` (immutable) from `var` (mutable) and rejects assignment to a
    /// `let`; the C-family languages have no such split, so this equals `decl_f` for three of the
    /// four and is the reason the distinction has to be carried at all.
    pub var_f: &'static str,
    /// Mutable three-component binding keyword.
    pub var3_kw: &'static str,
    /// Whether the language has a `?:` ternary. WGSL does not, and needs `select(f, t, cond)`.
    pub has_ternary: bool,
    /// Whether full-circle arctangent is the two-argument overload of `atan`.
    ///
    /// GLSL spells `atan2(y, x)` as `atan(y, x)`; HLSL, Metal and WGSL have a dedicated `atan2`.
    pub two_arg_atan: bool,
    /// Whether a `mat3 * vec3` product must be written `mul(m, v)` rather than `m * v`.
    ///
    /// HLSL has no `*` operator overload for matrix/vector; every other backend applies its
    /// column-major `mat3` as `m * v`.
    pub matrix_mul_fn: bool,
    /// The language this dialect writes. The GLSL variants share every spelling above and differ
    /// in their texture declarations and samples ([`Dialect::sample`], [`Dialect::glsl_sampler_type`]),
    /// as OCIO's `getTexDecl` / `getTexSample` (`GpuShaderUtils.cpp`) do.
    pub lang: GpuLanguage,
}

/// GLSL 3.30 dialect.
pub const GLSL: Dialect = Dialect {
    lang: GpuLanguage::Glsl330,
    decl_f: "float ",
    var_f: "float ",
    var3_kw: "vec3 ",
    has_ternary: true,
    two_arg_atan: true,
    matrix_mul_fn: false,
    v3: "vec3",
    v2: "vec2",
    splat_triple: false,
    decl_kw: "vec3 ",
    mix_fn: "mix",
    rebuild_vec4: false,
};

/// HLSL Shader Model 5 dialect.
pub const HLSL: Dialect = Dialect {
    lang: GpuLanguage::Hlsl50,
    decl_f: "float ",
    var_f: "float ",
    var3_kw: "float3 ",
    has_ternary: true,
    two_arg_atan: false,
    matrix_mul_fn: true,
    v3: "float3",
    v2: "float2",
    splat_triple: true,
    decl_kw: "float3 ",
    mix_fn: "lerp",
    rebuild_vec4: false,
};

/// Metal Shading Language dialect.
pub const METAL: Dialect = Dialect {
    lang: GpuLanguage::Metal,
    decl_f: "float ",
    var_f: "float ",
    var3_kw: "float3 ",
    has_ternary: true,
    two_arg_atan: false,
    matrix_mul_fn: false,
    v3: "float3",
    v2: "float2",
    splat_triple: false,
    decl_kw: "float3 ",
    mix_fn: "mix",
    rebuild_vec4: false,
};

/// WGSL dialect.
pub const WGSL: Dialect = Dialect {
    lang: GpuLanguage::Wgsl,
    decl_f: "let ",
    var_f: "var ",
    var3_kw: "var ",
    has_ternary: false,
    two_arg_atan: false,
    matrix_mul_fn: false,
    v3: "vec3<f32>",
    v2: "vec2<f32>",
    splat_triple: false,
    decl_kw: "let ",
    mix_fn: "mix",
    rebuild_vec4: true,
};

/// A value type in a declaration or a function signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    /// `float` / `f32`.
    F,
    /// `int` / `i32`.
    I,
    /// Three floats.
    V3,
    /// Four floats.
    V4,
}

/// Formats a constant for shader text.
///
/// Fixed-point with up to 12 fractional digits, trailing zeros trimmed.
///
/// Twelve is enough to saturate `f32` even for the smallest constants here (`2^-16` needs 11 to
/// stay inside an `f32` ulp). Trimming keeps round numbers readable as `0.0` / `12.92` rather than
/// `0.000000000000`, which matters because these strings are what a shader author actually reads
/// when debugging. Exponent-free in both cases, so the emitted text stays parseable by the simple
/// literal scanners the shader tests use.
/// [`vfx_core::consts::LOG_MIN`] as a shader literal (`1.1754944e-38`). [`n`]'s fixed-point
/// form would print it as `0.0`; every target language reads the exponent form.
pub fn log_min() -> String {
    format!("{:e}", vfx_core::consts::LOG_MIN)
}

/// An `f32` parameter as a shader literal: the shortest decimal that reads back as the same `f32`
/// (Rust's `Display`), with a fractional digit. A shader's `float` is an `f32`, so this is exact,
/// where widening to `f64` first ([`n`]) prints the widening's digits (`0.10000000149`).
pub fn nf(v: f32) -> String {
    let s = format!("{v}");
    if s.contains('.') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{s}.0")
    }
}

pub fn n(v: f64) -> String {
    let s = format!("{v:.12}");
    let trimmed = s.trim_end_matches('0');
    // Always leave at least one fractional digit: `12.` is not a valid literal in WGSL, and
    // `float3(12)` would be an integer literal in HLSL.
    if trimmed.ends_with('.') {
        format!("{trimmed}0")
    } else {
        trimmed.to_string()
    }
}

impl Dialect {
    /// The dialect of `lang`. Cg and OSL are not emitted (`generate_shader` refuses them first);
    /// they get the GLSL spellings.
    pub fn for_language(lang: GpuLanguage) -> Dialect {
        match lang {
            GpuLanguage::Hlsl50 => HLSL,
            GpuLanguage::Metal => METAL,
            GpuLanguage::Wgsl => WGSL,
            _ => Dialect { lang, ..GLSL },
        }
    }

    /// GLSL 1.20 and ES 1.00: `varying`, `gl_FragColor`, `textureND()`.
    pub(super) fn legacy_glsl(&self) -> bool {
        matches!(self.lang, GpuLanguage::Glsl120 | GpuLanguage::GlslEs100)
    }

    /// OpenGL ES: no 1D textures (OCIO: "1D textures are unsupported by OpenGL ES"), so a 1D LUT
    /// is declared and sampled as a one-row 2D texture, the texture data being the same.
    pub(super) fn is_es(&self) -> bool {
        matches!(self.lang, GpuLanguage::GlslEs100 | GpuLanguage::GlslEs300)
    }

    /// A scalar uniform's value: its name in GLSL and HLSL, a member of the `uniforms` buffer
    /// argument in Metal, a `scalar_f32(<NAME>_OFFSET)` read of the scalar storage buffer in WGSL.
    pub(super) fn uniform(&self, name: &str) -> String {
        match self.lang {
            GpuLanguage::Metal => format!("uniforms.{name}"),
            GpuLanguage::Wgsl => format!("scalar_f32({}_OFFSET)", name.to_uppercase()),
            _ => name.to_string(),
        }
    }

    /// A three-float uniform's value, as [`Dialect::uniform`] (`scalar_vec3` in WGSL).
    pub(super) fn uniform_v3(&self, name: &str) -> String {
        match self.lang {
            GpuLanguage::Wgsl => format!("scalar_vec3({}_OFFSET)", name.to_uppercase()),
            _ => self.uniform(name),
        }
    }

    /// OCIO `vectorCompareExpression(a, "!=", b)`: whether any component differs (`notEqual` in
    /// GLSL, which has no vector `!=`).
    pub(super) fn any_ne(&self, a: &str, b: &str) -> String {
        match self.lang {
            GpuLanguage::Hlsl50 | GpuLanguage::Metal | GpuLanguage::Wgsl => {
                format!("any({a} != {b})")
            }
            _ => format!("any(notEqual({a}, {b}))"),
        }
    }

    /// A scalar uniform named `name`, or the literal `lit` when there is no uniform (`name` empty).
    pub(super) fn uf(&self, name: &str, lit: f64) -> String {
        if name.is_empty() {
            n(lit)
        } else {
            self.uniform(name)
        }
    }

    /// The sampler variable's name for texture `name`: the texture itself in GLSL (a combined
    /// sampler), `<name>_sampler` elsewhere.
    pub(super) fn sampler_name(&self, name: &str) -> String {
        match self.lang {
            GpuLanguage::Hlsl50 | GpuLanguage::Metal | GpuLanguage::Wgsl => {
                format!("{name}_sampler")
            }
            _ => name.to_string(),
        }
    }

    /// A texture sample, `vec4`-valued, at `coord` (a scalar for 1D, `vec2`/`vec3` otherwise):
    /// OCIO's `getTexSample<N>`. GLSL 1.20 and ES 1.00 spell it `textureND`, later GLSL `texture`;
    /// ES samples a 1D LUT as a one-row 2D texture at `(coord, 0.5)`. HLSL `.Sample`, Metal
    /// `.sample`, WGSL `textureSampleLevel` at level 0 (a compute shader has no derivatives).
    pub(super) fn sample(&self, name: &str, kind: GpuTextureType, coord: &str) -> String {
        let s = self.sampler_name(name);
        let (dim, coord) = match kind {
            GpuTextureType::Texture1D if self.is_es() => (2, format!("{}({coord}, 0.5)", self.v2)),
            GpuTextureType::Texture1D => (1, coord.to_string()),
            GpuTextureType::Texture2D => (2, coord.to_string()),
            GpuTextureType::Texture3D => (3, coord.to_string()),
        };
        match self.lang {
            GpuLanguage::Hlsl50 => format!("{name}.Sample({s}, {coord})"),
            GpuLanguage::Metal => format!("{name}.sample({s}, {coord})"),
            GpuLanguage::Wgsl => format!("textureSampleLevel({name}, {s}, {coord}, 0.0)"),
            _ if self.legacy_glsl() => format!("texture{dim}D({s}, {coord})"),
            _ => format!("texture({s}, {coord})"),
        }
    }

    /// The GLSL sampler type of a texture of `kind` (ES: 1D is 2D).
    pub(super) fn glsl_sampler_type(&self, kind: GpuTextureType) -> &'static str {
        match kind {
            GpuTextureType::Texture1D if self.is_es() => "sampler2D",
            GpuTextureType::Texture1D => "sampler1D",
            GpuTextureType::Texture2D => "sampler2D",
            GpuTextureType::Texture3D => "sampler3D",
        }
    }

    /// The spelling of `ty`.
    pub(super) fn ty(&self, ty: Ty) -> &'static str {
        match (ty, self.lang) {
            (Ty::F, GpuLanguage::Wgsl) => "f32",
            (Ty::I, GpuLanguage::Wgsl) => "i32",
            (Ty::F, _) => "float",
            (Ty::I, _) => "int",
            (Ty::V3, _) => self.v3,
            (Ty::V4, GpuLanguage::Wgsl) => "vec4<f32>",
            (Ty::V4, GpuLanguage::Hlsl50 | GpuLanguage::Metal) => "float4",
            (Ty::V4, _) => "vec4",
        }
    }

    /// A function header, `ret name(params) {`: `fn name(p: T, ...) -> R {` in WGSL.
    pub(super) fn fn_begin(&self, ret: Ty, name: &str, params: &[(Ty, &str)]) -> String {
        if self.lang == GpuLanguage::Wgsl {
            let ps: Vec<String> = params
                .iter()
                .map(|(t, n)| format!("{n}: {}", self.ty(*t)))
                .collect();
            format!("fn {name}({}) -> {} {{", ps.join(", "), self.ty(ret))
        } else {
            let ps: Vec<String> = params
                .iter()
                .map(|(t, n)| format!("{} {n}", self.ty(*t)))
                .collect();
            format!("{} {name}({}) {{", self.ty(ret), ps.join(", "))
        }
    }

    /// A typed immutable binding, `T name = expr;` (`let name: T = expr;` in WGSL).
    pub(super) fn let_(&self, ty: Ty, name: &str, expr: &str) -> String {
        if self.lang == GpuLanguage::Wgsl {
            format!("let {name}: {} = {expr};", self.ty(ty))
        } else {
            format!("{} {name} = {expr};", self.ty(ty))
        }
    }

    /// A typed mutable binding, `T name = expr;` (`var name: T = expr;` in WGSL).
    pub(super) fn var_(&self, ty: Ty, name: &str, expr: &str) -> String {
        if self.lang == GpuLanguage::Wgsl {
            format!("var {name}: {} = {expr};", self.ty(ty))
        } else {
            format!("{} {name} = {expr};", self.ty(ty))
        }
    }

    /// A typed mutable binding without a value (`var name: T;` in WGSL).
    pub(super) fn var_uninit(&self, ty: Ty, name: &str) -> String {
        if self.lang == GpuLanguage::Wgsl {
            format!("var {name}: {};", self.ty(ty))
        } else {
            format!("{} {name};", self.ty(ty))
        }
    }

    /// Element `idx` (an `int` expression) of the uniform array `name`, an int array when `int`:
    /// `name[idx]` (Metal: a member of the uniform buffer), in WGSL a read of the concatenated
    /// array storage buffer at the array's base.
    pub(super) fn arr_at(&self, name: &str, int: bool, idx: &str) -> String {
        match self.lang {
            GpuLanguage::Wgsl => format!(
                "ocio_arrays_{}[{}_BASE + u32({idx})]",
                if int { "i" } else { "f" },
                name.to_uppercase()
            ),
            GpuLanguage::Metal => format!("uniforms.{name}[{idx}]"),
            _ => format!("{name}[{idx}]"),
        }
    }

    /// A bool uniform as a condition: in WGSL the scalar buffer holds it as a `u32`.
    pub(super) fn uniform_bool(&self, name: &str) -> String {
        match self.lang {
            GpuLanguage::Wgsl => format!("(scalar_u32({}_OFFSET) != 0u)", name.to_uppercase()),
            _ => self.uniform(name),
        }
    }

    /// The index `i` (an `int` declared by the caller, starting at 0) of the first step in
    /// `0..limit` whose `stop` condition holds, or `limit`: OCIO's
    /// `for (i = 0; i < limit; ++i) { if (stop) break; }` (`GradingBSplineCurve.cpp`). `pre`
    /// statements run before the test in each step. WGSL spells it as a `loop`.
    pub(super) fn find_index(
        &self,
        i: &str,
        limit: &str,
        pre: &[String],
        stop: &str,
    ) -> Vec<String> {
        let mut out = Vec::new();
        if self.lang == GpuLanguage::Wgsl {
            out.push("loop {".to_string());
            out.push(format!("    if ({i} >= {limit}) {{ break; }}"));
            out.extend(pre.iter().map(|l| format!("    {l}")));
            out.push(format!("    if ({stop}) {{ break; }}"));
            out.push(format!("    {i} = {i} + 1;"));
        } else {
            out.push(format!("for ({i} = 0; {i} < {limit}; ++{i}) {{"));
            out.extend(pre.iter().map(|l| format!("    {l}")));
            out.push(format!("    if ({stop}) {{ break; }}"));
        }
        out.push("}".to_string());
        out
    }

    /// Splats a scalar expression to three components.
    ///
    /// Always used, even where a language would accept an implicit scalar: WGSL requires matching
    /// operand types for `min`, `max`, `step` and `pow`, so an unsplatted scalar is a WGSL compile
    /// error rather than a portability nicety.
    pub(super) fn splat(&self, s: &str) -> String {
        if self.splat_triple {
            format!("{}({s}, {s}, {s})", self.v3)
        } else {
            format!("{}({s})", self.v3)
        }
    }

    /// Splats a numeric constant.
    pub(super) fn k(&self, v: f64) -> String {
        self.splat(&n(v))
    }

    /// Splats an `f32` parameter ([`nf`]).
    pub(super) fn kf(&self, v: f32) -> String {
        self.splat(&nf(v))
    }

    /// A local binding: `vec3 name = expr;` / `let name = expr;`.
    pub(super) fn decl(&self, name: &str, expr: &str) -> String {
        format!("    {}{name} = {expr};", self.decl_kw)
    }

    /// Writes the RGB result back into `color`.
    pub(super) fn set(&self, expr: &str) -> String {
        if self.rebuild_vec4 {
            format!("    color = vec4<f32>({expr}, color.a);")
        } else {
            format!("    color.rgb = {expr};")
        }
    }

    /// `mix(a, b, t)`, spelled `lerp` on HLSL.
    pub(super) fn mix(&self, a: &str, b: &str, t: &str) -> String {
        format!("{}({a}, {b}, {t})", self.mix_fn)
    }

    /// `step(edge, x)` with the edge splatted.
    pub(super) fn step(&self, edge: f64, x: &str) -> String {
        format!("step({}, {x})", self.k(edge))
    }

    /// LUT1D texel-center remap: `t = (t * (size-1) + 0.5) / size;`, in
    /// whichever splat form this dialect needs for the `0.5` addend.
    ///
    /// A GPU texture's normalized `[0,1]` coordinate space is a different
    /// parameterization than the CPU reference's (`processor/apply.rs`
    /// `ProcessorOp::Lut1d`) domain-normalized fractional index into
    /// `[0, size-1]` texel CENTERS — texel `i` sits at `(i+0.5)/size` in
    /// texture-coordinate space, so sampling at the raw domain-normalized
    /// value samples a DIFFERENT input than the CPU path (correct only at
    /// the 0.0/1.0 endpoints, where `ClampToEdge` happens to paper over
    /// it — the WGSL emitter's own PLAN9.md O1 comment has the full
    /// derivation). WGSL got this fix; GLSL/HLSL/Metal did not, until this
    /// shared function closed that gap — one derivation, not four
    /// independently-maintained copies of the same off-by-half-texel
    /// correction, which is exactly the shape that let three of the four
    /// silently miss it in the first place.
    ///
    /// Caller is responsible for the enclosing statement's indentation and
    /// for having already declared a mutable `t` of this dialect's `v3`
    /// type — this returns only the bare assignment.
    pub(super) fn lut1d_texel_center_remap(&self, tex_size: f32) -> String {
        format!(
            "t = (t * {} + {}) / {};",
            n((tex_size - 1.0) as f64),
            self.k(0.5),
            n(tex_size as f64)
        )
    }

    /// The 1D-LUT sample coordinate before the [0,1] clamp: `(color.rgb - min) * inv_range`, per
    /// channel, as `processor/apply.rs` `ProcessorOp::Lut1d` indexes (a zero-width channel scales
    /// by 0, so it samples the first entry like the CPU).
    pub(super) fn lut1d_domain_t(&self, domain_min: [f32; 3], domain_max: [f32; 3]) -> String {
        let inv: [f64; 3] = std::array::from_fn(|c| {
            let range = (domain_max[c] - domain_min[c]) as f64;
            if range.abs() < 1e-10 {
                0.0
            } else {
                1.0 / range
            }
        });
        let v3 = |v: [f64; 3]| format!("{}({}, {}, {})", self.v3, n(v[0]), n(v[1]), n(v[2]));
        format!(
            "(color.rgb - {}) * {}",
            v3(domain_min.map(|v| v as f64)),
            v3(inv)
        )
    }

    /// A logarithm argument floored at [`LOG_FLOOR`]. See the module's NaN discipline note.
    pub(super) fn safe(&self, expr: &str) -> String {
        format!("max({expr}, {})", self.k(LOG_FLOOR))
    }

    /// Natural log for OCIO's log ops (Log, LogAffine, LogCamera, Allocation LG2): the argument is
    /// floored at [`vfx_core::consts::LOG_MIN`] as `LogOpGPU.cpp` does, not at [`LOG_FLOOR`].
    pub(super) fn ln_log_op(&self, expr: &str) -> String {
        format!("log(max({expr}, {}))", self.splat(&log_min()))
    }

    /// Natural log of a floored argument.
    pub(super) fn ln(&self, expr: &str) -> String {
        format!("log({})", self.safe(expr))
    }

    /// Base-2 log of a floored argument.
    pub(super) fn log2(&self, expr: &str) -> String {
        format!("log2({})", self.safe(expr))
    }

    /// Base-10 log of a floored argument, via `log(x) * log10(e)`.
    pub(super) fn log10(&self, expr: &str) -> String {
        format!("(log({}) * {})", self.safe(expr), n(LOG10_E))
    }

    /// `exp(x)` with the argument capped at [`EXP_CAP`].
    pub(super) fn exp(&self, expr: &str) -> String {
        format!("exp(min({expr}, {}))", self.k(EXP_CAP))
    }

    /// `exp2(x)` with the argument capped at [`EXP2_CAP`].
    pub(super) fn exp2(&self, expr: &str) -> String {
        format!("exp2(min({expr}, {}))", self.k(EXP2_CAP))
    }

    /// `10^x`, expressed as `exp2(x * log2(10))` and capped.
    ///
    /// GLSL and WGSL have no `pow10`/`log10` pair, and `pow(vec3(10.0), x)` costs an extra splat
    /// for no benefit.
    pub(super) fn pow10(&self, expr: &str) -> String {
        self.exp2(&format!("({expr}) * {}", n(LOG2_10)))
    }

    /// An immutable scalar binding: `float name = expr;` / `let name = expr;`.
    pub(super) fn decl_f(&self, name: &str, expr: &str) -> String {
        format!("    {}{name} = {expr};", self.decl_f)
    }

    /// A mutable scalar binding. Use only where the value is genuinely reassigned later — WGSL
    /// warns on a `var` that is never written.
    pub(super) fn var_f(&self, name: &str, expr: &str) -> String {
        format!("    {}{name} = {expr};", self.var_f)
    }

    /// A mutable three-component binding.
    pub(super) fn var3(&self, name: &str, expr: &str) -> String {
        format!("    {}{name} = {expr};", self.var3_kw)
    }

    /// `cond ? t : f`, spelled `select(f, t, cond)` on WGSL.
    ///
    /// Note the argument order flips: WGSL's `select` takes the FALSE value first. Both arms are
    /// evaluated either way, so neither form may contain an expression that can trap.
    pub(super) fn sel(&self, cond: &str, t: &str, f: &str) -> String {
        if self.has_ternary {
            format!("(({cond}) ? ({t}) : ({f}))")
        } else {
            format!("select({f}, {t}, {cond})")
        }
    }

    /// `atan2(y, x)`, the full-circle arctangent.
    ///
    /// GLSL spells it as the two-argument overload of `atan(y, x)`; HLSL, Metal and WGSL all have a
    /// dedicated `atan2(y, x)`. The argument order (`y` first) is identical in both spellings.
    pub(super) fn atan2(&self, y: &str, x: &str) -> String {
        if self.two_arg_atan {
            format!("atan({y}, {x})")
        } else {
            format!("atan2({y}, {x})")
        }
    }

    /// A 3x3-matrix times a 3-vector, `M * v`.
    ///
    /// `m` is a dialect-appropriate access expression for a column-major `mat3` (a bare uniform name
    /// for GLSL/Metal/HLSL, a `scalar_mat3(<NAME>_OFFSET)` read for WGSL). GLSL, Metal and WGSL have
    /// column-major `mat3` types applied as `m * v`. HLSL's `float3x3` uniform is packed
    /// column-major too, and `mul(M, v)` is the column-vector product `M * v` — the same reasoning
    /// that made the inline `Matrix` op transpose, but a uniform reconstructs `M` itself rather than
    /// `M^T`, so no transpose is applied here. Feeding a column-major matrix to every backend keeps
    /// one uploaded array (`GpuUniformValue::Mat3`, column-major) correct for all four.
    pub(super) fn mat_mul(&self, m: &str, v: &str) -> String {
        if self.matrix_mul_fn {
            format!("mul({m}, {v})")
        } else {
            format!("({m}) * ({v})")
        }
    }
}
