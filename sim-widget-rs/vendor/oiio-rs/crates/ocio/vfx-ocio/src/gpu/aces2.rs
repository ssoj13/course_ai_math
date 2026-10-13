//! ACES 2.0 `RGB <-> JMh` conversion, emitted once for all four shading languages.
//!
//! # Why this exists (ACES2-03)
//!
//! Every ACES2 op used to be baked as a 33³ LUT over the hardcoded `[0,1]³` cube with no shaper
//! (`gpu.rs`, `bake_lut3d`). For the JMh-input directions that is catastrophic: JMh values reach
//! `J≈100`, `M≳60`, `h∈[0,360)`, none of which live inside `[0,1]³`, so the whole domain clamped
//! into a single LUT corner and five op-directions rendered as a constant. `RGB -> JMh` fared no
//! better on HDR input, where `>1.0` was crushed at the cube's far face.
//!
//! The reference emits the full CAM16 chain into the shader with an unbounded domain
//! (`FixedFunctionOpGPU.cpp`). This module now does the same for **every** ACES2 op — the LUT bake
//! is gone: the `RGB <-> JMh` pair (stage 1), the Tonescale+Chroma compress op (stage 2,
//! [`emit_tonescale_op`]), Gamut compress (stage 3, [`emit_gamut_op`]) and the composed
//! OutputTransform (stage 4, [`emit_output_op`], which chains the other three). All four share this
//! module's uniform plumbing — including the `ArrayFloat` reach-table upload the tonescale stage
//! introduced, which the gamut stage samples identically, plus the gamut cusp/hue tables. Every
//! emitted body is verified against the CPU oracle by `tests/aces2_gpu_emission.rs`.
//!
//! # Authority
//!
//! Transcribed term-for-term from OCIO
//! `src/OpenColorIO/ops/fixedfunction/FixedFunctionOpGPU.cpp`:
//! `_Add_RGB_to_JMh_Shader` (:431) = `_Add_RGB_to_Aab_Shader` (:377) + `_Add_Aab_to_JMh_Shader`
//! (:395); `_Add_JMh_to_RGB_Shader` (:490) = `_Add_JMh_to_Aab_Shader` (:454) +
//! `_Add_Aab_to_RGB_Shader` (:473). The per-config data (the four `MATRIX_*` matrices and `cz` /
//! `inv_cz`) arrives as uniforms; the universal CAM16 constants are sourced from the CPU
//! [`crate::aces2`] module so the shader and the verified CPU oracle
//! ([`crate::aces2::rgb_to_jmh_degrees`] / [`crate::aces2::jmh_degrees_to_rgb`]) can never drift.
//!
//! The CAM16 post-adaptation nonlinearity is identical on both sides: in `aces2.rs`, `F_L` is
//! folded into `mat_rgb_to_cam16_c` (see `init_jmh_params`), which is exactly why the reference's
//! `_Add_RGB_to_Aab_Shader` applies `pow(abs(lms), 0.42)` with no separate `F_L` factor — so a
//! straight transcription of the reference already equals `rgb_to_aab`/`aab_to_rgb`.
//!
//! # One deliberate reference/CPU quirk
//!
//! The reference guards `h = (Aab.g == 0) ? 0 : atan2(...)` and `M = (J == 0) ? 0 : sqrt(...)`.
//! The CPU takes `atan2` unconditionally and computes `M` unconditionally. `M`'s guard is dead
//! (this branch only runs for `aab.r > 0`, so `J = 100·aab.r^cz > 0`). `h`'s guard differs from
//! the CPU only on the measure-zero locus `aab.g == 0 ∧ aab.b ≠ 0`, unreachable for ordinary
//! inputs; the emitter follows the reference. Both facts are asserted in the crate's
//! `aces2_gpu_emission` integration test with inputs kept off that locus.

use super::dialect::{Dialect, n};
use crate::aces2::{
    CAM_NL_OFFSET, CHROMA_NORM_WEIGHTS, COMPRESSION_THRESHOLD, CUSP_MID_BLEND, FOCUS_GAIN_BLEND,
    J_SCALE, POST_ADAPT_POWER, POST_ADAPT_RA_LIMIT, SMOOTH_CUSPS, TABLE_FIRST_NOM,
    TABLE_LOWER_WRAP, TABLE_UPPER_WRAP,
};

/// `log10(e)`, for turning a natural log into a base-10 log without a `log10` builtin (GLSL/WGSL
/// lack one). The CPU oracle's `get_focus_gain` uses `f32::log10`; the shader computes
/// `log(x) * LOG10_E`, equal to within a transcendental ulp over the provably-positive argument.
const LOG10_E: f64 = std::f64::consts::LOG10_E;

/// Degrees per radian, for `atan2 -> hue`. Universal (`180/π`), not a per-config constant.
const DEG_PER_RAD: f64 = 180.0 / std::f64::consts::PI;
/// Radians per degree, for `hue -> h_rad`. Universal (`π/180`).
const RAD_PER_DEG: f64 = std::f64::consts::PI / 180.0;
/// One full turn in degrees; matches `aces2::HUE_LIMIT` and the reference's literal `360.0`.
const HUE_FULL: f64 = 360.0;

/// Dialect-appropriate access expressions for the `JMhParams` uniforms this conversion reads.
///
/// The caller (`ops::emit`) resolves the plain uniform names into the spelling its
/// language needs — a bare name for GLSL/HLSL/Metal, a `scalar_mat3(<NAME>_OFFSET)` /
/// `scalar_f32(<NAME>_OFFSET)` read for WGSL — so this module stays free of per-dialect
/// uniform-access knowledge (only the `M * v` spelling differs, and that lives on [`Dialect`]).
pub(super) struct JmhRefs<'a> {
    /// `MATRIX_RGB_to_CAM16_c` (column-major `mat3`).
    pub mat_rgb_to_cam16_c: &'a str,
    /// `MATRIX_CAM16_c_to_RGB` (column-major `mat3`).
    pub mat_cam16_c_to_rgb: &'a str,
    /// `MATRIX_cone_response_to_Aab` (column-major `mat3`).
    pub mat_cone_to_aab: &'a str,
    /// `MATRIX_Aab_to_cone_response` (column-major `mat3`).
    pub mat_aab_to_cone: &'a str,
    /// `cz` — the achromatic `J` exponent.
    pub cz: &'a str,
    /// `inv_cz` — its reciprocal, for the inverse.
    pub inv_cz: &'a str,
}

/// Emits the shader body for `RGB -> JMh` (`forward`) or `JMh -> RGB`.
///
/// Returned lines carry their own inner indentation and include the enclosing braces; callers add
/// their own outer indentation, exactly as [`super::hsy::emit`] does.
pub(super) fn emit(forward: bool, r: &JmhRefs, d: &Dialect) -> Vec<String> {
    if forward {
        rgb_to_jmh(r, d)
    } else {
        jmh_to_rgb(r, d)
    }
}

/// Resolves the [`super::types::OpUniforms::Aces2Jmh`] names into dialect access expressions and
/// emits the body — the single entry point `ops::emit` calls, so the
/// name-to-access rule (bare name vs WGSL `scalar_*(<NAME>_OFFSET)`) lives in exactly one place.
///
/// `storage_buffer` is set only for WGSL, whose uniforms are byte offsets into a storage buffer
/// read through `scalar_mat3`/`scalar_f32`; the C-family backends reference the uniform by name.
pub(super) fn emit_op(
    forward: bool,
    uniforms: Option<&super::types::OpUniforms>,
    d: &Dialect,
    storage_buffer: bool,
) -> Vec<String> {
    use super::types::OpUniforms;
    let Some(OpUniforms::Aces2Jmh {
        mat_rgb_to_cam16_c,
        mat_cam16_c_to_rgb,
        mat_cone_to_aab,
        mat_aab_to_cone,
        cz,
        inv_cz,
        ..
    }) = uniforms
    else {
        // build_uniforms always pairs this op with its uniforms; name the failure rather than
        // emitting a silently wrong (identity) body if that invariant is ever broken.
        return vec!["// ERROR: missing ACES2 JMh uniforms".to_string()];
    };

    let mat = |name: &str| {
        if storage_buffer {
            format!("scalar_mat3({}_OFFSET)", name.to_uppercase())
        } else {
            name.to_string()
        }
    };
    let flt = |name: &str| {
        if storage_buffer {
            format!("scalar_f32({}_OFFSET)", name.to_uppercase())
        } else {
            name.to_string()
        }
    };

    let (m_rc, m_cr, m_ca, m_ac) = (
        mat(mat_rgb_to_cam16_c),
        mat(mat_cam16_c_to_rgb),
        mat(mat_cone_to_aab),
        mat(mat_aab_to_cone),
    );
    let (cz_r, inv_cz_r) = (flt(cz), flt(inv_cz));

    let refs = JmhRefs {
        mat_rgb_to_cam16_c: &m_rc,
        mat_cam16_c_to_rgb: &m_cr,
        mat_cone_to_aab: &m_ca,
        mat_aab_to_cone: &m_ac,
        cz: &cz_r,
        inv_cz: &inv_cz_r,
    };
    emit(forward, &refs, d)
}

/// `RGB -> JMh`. Reference: `FixedFunctionOpGPU.cpp:377-451`.
fn rgb_to_jmh(r: &JmhRefs, d: &Dialect) -> Vec<String> {
    let off = d.splat(&n(CAM_NL_OFFSET as f64));
    let jscale = n(J_SCALE as f64);
    let power = d.k(POST_ADAPT_POWER as f64);
    let deg = n(DEG_PER_RAD);
    let full = n(HUE_FULL);

    let mut out = vec!["{ // ACES2 RGB -> JMh".to_string()];

    // RGB -> Aab: CAM16 cone response, post-adaptation nonlinearity, then the Aab matrix.
    // `F_L` is already inside `mat_rgb_to_cam16_c`, so `pow(abs(lms), 0.42)` alone is the
    // reference's `F_L_v` (`FixedFunctionOpGPU.cpp:386`).
    out.push(d.decl("lms", &d.mat_mul(r.mat_rgb_to_cam16_c, "color.rgb")));
    out.push(d.decl("fl", &format!("pow(abs(lms), {power})")));
    out.push(d.decl("rgb_a", &format!("(sign(lms) * fl) / ({off} + fl)")));
    out.push(d.decl("aab", &d.mat_mul(r.mat_cone_to_aab, "rgb_a")));

    // Aab -> JMh. `aab.r <= 0` yields black JMh (ref restates as an early `= 0`); expressed here as
    // a default of zero refined only when `aab.r > 0`, which keeps a single mutable `jmh`.
    out.push(d.var3("jmh", &d.k(0.0)));
    out.push("    if (aab.r > 0.0) {".to_string());
    out.push(format!(
        "    {}",
        d.decl_f("J", &format!("{jscale} * pow(aab.r, {})", r.cz))
    ));
    out.push(format!(
        "    {}",
        d.decl_f(
            "M",
            &d.sel("J == 0.0", "0.0", "sqrt(aab.g * aab.g + aab.b * aab.b)")
        )
    ));
    out.push(format!(
        "    {}",
        d.var_f(
            "h",
            &d.sel(
                "aab.g == 0.0",
                "0.0",
                &format!("{} * {deg}", d.atan2("aab.b", "aab.g"))
            ),
        )
    ));
    out.push(format!("        h = h - floor(h / {full}) * {full};"));
    out.push(format!(
        "        h = {};",
        d.sel("h < 0.0", &format!("h + {full}"), "h")
    ));
    out.push(format!("        jmh = {}(J, M, h);", d.v3));
    out.push("    }".to_string());

    out.push(d.set("jmh"));
    out.push("}".to_string());
    out
}

/// `JMh -> RGB`. Reference: `FixedFunctionOpGPU.cpp:454-503` (with `_Add_SinCos_Shader`, :363,
/// inlined from the hue channel — the standalone op has no preceding sin/cos stage).
fn jmh_to_rgb(r: &JmhRefs, d: &Dialect) -> Vec<String> {
    let inv_jscale = n(1.0 / J_SCALE as f64);
    let off = d.splat(&n(CAM_NL_OFFSET as f64));
    let ra_lim = d.k(POST_ADAPT_RA_LIMIT as f64);
    let inv_power = d.k(1.0 / POST_ADAPT_POWER as f64);
    let one = d.k(1.0);
    let rad = n(RAD_PER_DEG);

    let mut out = vec!["{ // ACES2 JMh -> RGB".to_string()];

    // `color` holds `(J, M, h)`. Read all three before writing: trig from the hue (`color.b`), the
    // colourfulness `M` (`color.g`), and the achromatic term from `J` (`color.r`).
    out.push(d.decl_f("h_rad", &format!("color.b * {rad}")));
    out.push(d.decl_f("cos_hr", "cos(h_rad)"));
    out.push(d.decl_f("sin_hr", "sin(h_rad)"));
    out.push(d.decl_f("m_val", "color.g"));

    // JMh -> Aab.
    out.push(d.decl(
        "aab",
        &format!(
            "{}(pow(color.r * {inv_jscale}, {}), m_val * cos_hr, m_val * sin_hr)",
            d.v3, r.inv_cz
        ),
    ));

    // Aab -> RGB: inverse Aab matrix, inverse post-adaptation nonlinearity, then back to RGB.
    out.push(d.decl("rgb_a", &d.mat_mul(r.mat_aab_to_cone, "aab")));
    out.push(d.decl("rgb_a_lim", &format!("min(abs(rgb_a), {ra_lim})")));
    out.push(d.decl(
        "lms",
        &format!("sign(rgb_a) * pow({off} * rgb_a_lim / ({one} - rgb_a_lim), {inv_power})"),
    ));
    out.push(d.set(&d.mat_mul(r.mat_cam16_c_to_rgb, "lms")));
    out.push("}".to_string());
    out
}

// ===========================================================================
// ACES2 Tonescale + Chroma compress (ACES2-03, stage 2)
// ===========================================================================
//
// Emits `aces2::tonescale_compress_{fwd,inv}` in-shader, replacing that op's `[0,1]^3` LUT bake.
// The op consumes JMh (`J` up to ~limit_J_max, `M` unbounded, `h` full-circle) — none of it inside
// the unit cube — so the bake collapsed it into a corner (ACES2-03). Two parts:
//
//   1. **Tonescale of `J`** (`aces2::tonescale_fwd_j` / `tonescale_inv_j`): a closed form, no
//      iteration. Forward and inverse both round-trip through Y (`J -> Y`, tonescale, `Y -> J`);
//      the inverse tonescale is the closed-form quadratic solve `ht = 0.5*(Z + sqrt(Z*(4t1+Z)))`.
//      Matches OCIO `_Add_Tonescale_func` (`FixedFunctionOpGPU.cpp:635`) term-for-term. The one
//      deliberate CPU-vs-ref divergence: the CPU oracle's `a_to_y` clamps the adapted response with
//      `min(ra, 0.99)` (`POST_ADAPT_RA_LIMIT`) where the ref shader omits it. The clamp is inactive
//      for any in-gamut `J` (`ra ~ 0.03` at `J ~ 100`), but it is kept so this emitter reproduces
//      the verified oracle exactly (`cpu_equals_emit_*` asserts zero delta).
//
//   2. **Chroma compress** (`aces2::chroma_compress_{fwd,inv}`): needs the per-hue reach limit
//      `reachMaxM = reach_m_from_table(h, reach_m_table)`, a 363-entry table not expressible in
//      closed form. OCIO uploads it as a 1D texture; here it rides as an `ArrayFloat` storage-buffer
//      uniform (the same channel `GradingRgbCurve`'s knots/coefs use), sampled by the same
//      `i_lo = floor(h) + TABLE_FIRST_NOM` / lerp the CPU `reach_m_from_table` does. `Mnorm`
//      (closed-form Fourier series) and the `toe` fwd/inv helper are inlined.

/// Dialect-resolved access expressions for the `Aces2Tonescale` uniforms.
///
/// Each field is already spelled the way its language needs (a bare uniform name for
/// GLSL/HLSL/Metal, a `scalar_f32(<NAME>_OFFSET)` read for WGSL) — resolved once in
/// [`emit_tonescale_op`] so the body stays free of per-dialect uniform-access knowledge. `reach` is
/// the raw array-uniform name; the reach sampler builds its own access from it because a WGSL array
/// read (`ocio_arrays_f[<NAME>_BASE + i]`) is shaped differently from a scalar read.
pub(super) struct TonescaleRefs<'a> {
    pub a_w_j: &'a str,
    pub inv_a_w_j: &'a str,
    pub f_l_n: &'a str,
    pub cz: &'a str,
    pub inv_cz: &'a str,
    pub n_r: &'a str,
    pub s_2: &'a str,
    pub m_2: &'a str,
    pub g: &'a str,
    pub t_1: &'a str,
    pub inverse_limit: &'a str,
    pub limit_j_max: &'a str,
    pub model_gamma_inv: &'a str,
    pub sat: &'a str,
    pub sat_thr: &'a str,
    pub compr: &'a str,
    pub ccs: &'a str,
    /// Raw name of the reach-`M` `ArrayFloat` uniform (363 entries).
    pub reach: &'a str,
}

/// Resolves the [`super::types::OpUniforms::Aces2Tonescale`] names into dialect access expressions
/// and emits the body — the single entry point `ops::emit` calls.
///
/// `storage_buffer` is set only for WGSL, whose scalar uniforms are byte offsets read through
/// `scalar_f32` and whose arrays live in a shared `ocio_arrays_f` buffer indexed by `<NAME>_BASE`.
pub(super) fn emit_tonescale_op(
    forward: bool,
    uniforms: Option<&super::types::OpUniforms>,
    d: &Dialect,
    storage_buffer: bool,
) -> Vec<String> {
    use super::types::OpUniforms;
    let Some(OpUniforms::Aces2Tonescale {
        a_w_j,
        inv_a_w_j,
        f_l_n,
        cz,
        inv_cz,
        n_r,
        s_2,
        m_2,
        g,
        t_1,
        inverse_limit,
        limit_j_max,
        model_gamma_inv,
        sat,
        sat_thr,
        compr,
        ccs,
        reach,
    }) = uniforms
    else {
        // build_uniforms always pairs this op with its uniforms; name the failure rather than
        // emitting a silently wrong (identity) body if that invariant is ever broken.
        return vec!["// ERROR: missing ACES2 tonescale uniforms".to_string()];
    };

    let flt = |name: &str| {
        if storage_buffer {
            format!("scalar_f32({}_OFFSET)", name.to_uppercase())
        } else {
            name.to_string()
        }
    };

    let (
        a_w_j,
        inv_a_w_j,
        f_l_n,
        cz,
        inv_cz,
        n_r,
        s_2,
        m_2,
        g,
        t_1,
        inverse_limit,
        limit_j_max,
        model_gamma_inv,
        sat,
        sat_thr,
        compr,
        ccs,
    ) = (
        flt(a_w_j),
        flt(inv_a_w_j),
        flt(f_l_n),
        flt(cz),
        flt(inv_cz),
        flt(n_r),
        flt(s_2),
        flt(m_2),
        flt(g),
        flt(t_1),
        flt(inverse_limit),
        flt(limit_j_max),
        flt(model_gamma_inv),
        flt(sat),
        flt(sat_thr),
        flt(compr),
        flt(ccs),
    );

    let refs = TonescaleRefs {
        a_w_j: &a_w_j,
        inv_a_w_j: &inv_a_w_j,
        f_l_n: &f_l_n,
        cz: &cz,
        inv_cz: &inv_cz,
        n_r: &n_r,
        s_2: &s_2,
        m_2: &m_2,
        g: &g,
        t_1: &t_1,
        inverse_limit: &inverse_limit,
        limit_j_max: &limit_j_max,
        model_gamma_inv: &model_gamma_inv,
        sat: &sat,
        sat_thr: &sat_thr,
        compr: &compr,
        ccs: &ccs,
        reach,
    };
    tonescale_compress(forward, &refs, d, storage_buffer)
}

/// Inlines a `toe` evaluation (`aces2::toe_fwd` / `toe_inv`), declaring a fresh `t{tag}_out` in the
/// enclosing scope and assigning it inside a local block so the working names never collide.
///
/// The CPU `toe` early-returns `x` for `x > limit`; here that is a `select`/ternary, which evaluates
/// both arms. The unselected `toe` expression is finite wherever `x > limit` (the forward `sqrt`
/// argument is `mb^2 + 4·k2·k3·x` with `x > limit > 0`, so non-negative), so no NaN leaks — matching
/// the CPU, which computes the same expression in its own `else` branch for `x <= limit`.
/// The shader-expression operands of one `toe(x, limit, k1, k2)` evaluation (`aces2::toe_fwd` /
/// `toe_inv`, OCIO `toe`).
struct ToeOperands<'a> {
    x: &'a str,
    limit: &'a str,
    k1: &'a str,
    k2: &'a str,
}

fn push_toe(
    out: &mut Vec<String>,
    d: &Dialect,
    invert: bool,
    operands: ToeOperands<'_>,
    tag: usize,
) -> String {
    let ToeOperands { x, limit, k1, k2 } = operands;
    let p = format!("t{tag}_");
    let outv = format!("{p}out");
    out.push(d.var_f(&outv, "0.0"));
    out.push("    {".to_string());
    out.push(format!("    {}", d.decl_f(&format!("{p}x"), x)));
    out.push(format!("    {}", d.decl_f(&format!("{p}lim"), limit)));
    out.push(format!("    {}", d.decl_f(&format!("{p}k1i"), k1)));
    out.push(format!(
        "    {}",
        d.decl_f(&format!("{p}k2"), &format!("max({k2}, 0.001)"))
    ));
    out.push(format!(
        "    {}",
        d.decl_f(
            &format!("{p}k1"),
            &format!("sqrt({p}k1i * {p}k1i + {p}k2 * {p}k2)")
        )
    ));
    out.push(format!(
        "    {}",
        d.decl_f(
            &format!("{p}k3"),
            &format!("({p}lim + {p}k1) / ({p}lim + {p}k2)")
        )
    ));
    let toe_expr = if invert {
        format!("({p}x * {p}x + {p}k1 * {p}x) / ({p}k3 * ({p}x + {p}k2))")
    } else {
        // 0.5 * (mb + sqrt(mb^2 + 4*k2*k3*x)), mb = k3*x - k1.
        format!(
            "0.5 * (({p}k3 * {p}x - {p}k1) + sqrt(({p}k3 * {p}x - {p}k1) * ({p}k3 * {p}x - {p}k1) + 4.0 * {p}k2 * {p}k3 * {p}x))"
        )
    };
    out.push(format!(
        "        {outv} = {};",
        d.sel(&format!("{p}x > {p}lim"), &format!("{p}x"), &toe_expr)
    ));
    out.push("    }".to_string());
    outv
}

/// Emits the reach-`M` table lookup, shared by the tonescale (chroma compress) and, when it lands,
/// the gamut stage — both sample `reachMaxM` per hue, so the lookup lives here once rather than
/// being re-derived per stage.
///
/// Reproduces `aces2::reach_m_from_table` **exactly**: a LINEAR lerp between the two table entries
/// bracketing the hue — `i_lo = floor(h) + TABLE_FIRST_NOM`, `i_hi = i_lo + 1`, weight
/// `t = h - floor(h)` — **not** nearest-neighbour and **not** the hardware texture filtering OCIO's
/// 1D-texture transport does. That distinction is load-bearing: uploading the table as a storage
/// array (rather than a texture) makes the lookup *our* code's responsibility, and a storage array
/// only equals the reference texture where our lookup replicates the CPU's. The `cpu_equals_emit_*`
/// zero-delta at fractional hues (which fall *between* entries, exactly where nearest and lerp
/// diverge) is the proof it does.
///
/// Declares the immutable scalar `dst` from array `reach` sampled at `hue_var` (a hue in `[0,360)`).
/// `idx_tag` disambiguates the index local so a caller may emit more than one lookup in a scope.
/// WGSL reads the shared `ocio_arrays_f` buffer at `<NAME>_BASE`; the C-family index a named
/// `float reach[TABLE_TOTAL_SIZE]` uniform directly.
pub(super) fn emit_reach_sample(
    out: &mut Vec<String>,
    d: &Dialect,
    storage_buffer: bool,
    reach: &str,
    hue_var: &str,
    dst: &str,
    idx_tag: &str,
) {
    let ilo = format!("reach_ilo_{idx_tag}");
    let (lo, hi) = if storage_buffer {
        let base = format!("{}_BASE", reach.to_uppercase());
        out.push(format!(
            "    let {ilo} = u32(floor({hue_var})) + {TABLE_FIRST_NOM}u;"
        ));
        (
            format!("ocio_arrays_f[{base} + {ilo}]"),
            format!("ocio_arrays_f[{base} + {ilo} + 1u]"),
        )
    } else {
        out.push(format!(
            "    int {ilo} = int(floor({hue_var})) + {TABLE_FIRST_NOM};"
        ));
        (format!("{reach}[{ilo}]"), format!("{reach}[{ilo} + 1]"))
    };
    out.push(d.decl_f(
        dst,
        &d.mix(&lo, &hi, &format!("{hue_var} - floor({hue_var})")),
    ));
}

/// Emits the shader body for `tonescale_compress_fwd` (`forward`) or `_inv`.
///
/// Reference: OCIO `Add_Tonescale_Compress_{Fwd,Inv}_Shader` (`FixedFunctionOpGPU.cpp:1494/1522`)
/// composed of `_Add_Tonescale_func` (:635) + `_Add_ChromaCompressionNorm_Shader` (:696) +
/// `_Add_Tonescale_Compress_{Fwd,Inv}_Shader` (:724/:766) + `_Add_Reach_table` (:505). Transcribed
/// against the verified CPU oracle so `cpu_equals_emit_*` is exact.
fn tonescale_compress(
    forward: bool,
    r: &TonescaleRefs,
    d: &Dialect,
    storage_buffer: bool,
) -> Vec<String> {
    let off = n(CAM_NL_OFFSET as f64);
    let jscale = n(J_SCALE as f64);
    let inv_jscale = n(1.0 / J_SCALE as f64);
    let power = n(POST_ADAPT_POWER as f64);
    let inv_power = n(1.0 / POST_ADAPT_POWER as f64);
    let ra_lim = n(POST_ADAPT_RA_LIMIT as f64);
    let rad = n(RAD_PER_DEG);
    let full = n(HUE_FULL);
    let w: Vec<String> = CHROMA_NORM_WEIGHTS.iter().map(|x| n(*x as f64)).collect();

    let dir = if forward { "fwd" } else { "inv" };
    let mut out = vec![format!("{{ // ACES2 tonescale + chroma compress ({dir})")];

    // Bind the JMh channels; wrap the hue into [0, 360) exactly as `wrap_to_hue_limit`.
    out.push(d.decl_f("Jin", "color.r"));
    out.push(d.decl_f("Mch", "color.g"));
    out.push(d.var_f("h", "color.b"));
    out.push(format!("    h = h - floor(h / {full}) * {full};"));
    out.push(format!(
        "    h = {};",
        d.sel("h < 0.0", &format!("h + {full}"), "h")
    ));
    out.push(d.decl_f("h_rad", &format!("h * {rad}")));
    out.push(d.decl_f("cos_hr", "cos(h_rad)"));
    out.push(d.decl_f("sin_hr", "sin(h_rad)"));

    // Mnorm — the closed-form CAM16 chroma-normalisation Fourier series (`chroma_compress_norm`).
    out.push(d.decl_f("cc_cos2", "2.0 * cos_hr * cos_hr - 1.0"));
    out.push(d.decl_f("cc_sin2", "2.0 * cos_hr * sin_hr"));
    out.push(d.decl_f("cc_cos3", "4.0 * cos_hr * cos_hr * cos_hr - 3.0 * cos_hr"));
    out.push(d.decl_f("cc_sin3", "3.0 * sin_hr - 4.0 * sin_hr * sin_hr * sin_hr"));
    out.push(d.decl_f(
        "Mnorm",
        &format!(
            "({} * cos_hr + {} * cc_cos2 + {} * cc_cos3 + {} * sin_hr + {} * cc_sin2 + {} * cc_sin3 + {}) * {}",
            w[0], w[1], w[2], w[3], w[4], w[5], w[6], r.ccs
        ),
    ));

    // reachMaxM per hue — the `reach_m_from_table` linear lerp, via the shared emitter the gamut
    // stage will reuse (see `emit_reach_sample` for why the lerp must match the CPU, not a texture).
    emit_reach_sample(&mut out, d, storage_buffer, r.reach, "h", "reachMaxM", "ts");

    if forward {
        // Tonescale J -> J_ts (`tonescale_fwd_j`).
        out.push(d.decl_f(
            "ts_a",
            &format!("pow(abs(Jin) * {inv_jscale}, {})", r.inv_cz),
        ));
        out.push(d.decl_f("ts_ra", &format!("{} * ts_a", r.a_w_j)));
        out.push(d.decl_f("ts_ralim", &format!("min(ts_ra, {ra_lim})")));
        out.push(d.decl_f(
            "ts_yin",
            &format!(
                "pow({off} * ts_ralim / (1.0 - ts_ralim), {inv_power}) / {}",
                r.f_l_n
            ),
        ));
        out.push(d.decl_f(
            "ts_f",
            &format!("{} * pow(ts_yin / (ts_yin + {}), {})", r.m_2, r.s_2, r.g),
        ));
        out.push(d.decl_f(
            "ts_yout",
            &format!("max(0.0, ts_f * ts_f / (ts_f + {})) * {}", r.t_1, r.n_r),
        ));
        out.push(d.decl_f("ts_fly", &format!("pow(ts_yout * {}, {power})", r.f_l_n)));
        out.push(d.decl_f(
            "J_ts",
            &format!(
                "sign(Jin) * ({jscale} * pow((ts_fly / ({off} + ts_fly)) * {}, {}))",
                r.inv_a_w_j, r.cz
            ),
        ));

        // Chroma compress forward (`chroma_compress_fwd`).
        out.push(d.var_f("M_cp", "Mch"));
        out.push("    if (Mch != 0.0) {".to_string());
        out.push(format!(
            "    {}",
            d.decl_f("nJ", &format!("J_ts / {}", r.limit_j_max))
        ));
        out.push(format!("    {}", d.decl_f("snJ", "max(0.0, 1.0 - nJ)")));
        out.push(format!(
            "    {}",
            d.decl_f(
                "cc_limit",
                &format!("pow(nJ, {}) * reachMaxM / Mnorm", r.model_gamma_inv)
            )
        ));
        out.push(format!(
            "        M_cp = Mch * pow(J_ts / Jin, {});",
            r.model_gamma_inv
        ));
        out.push("        M_cp = M_cp / Mnorm;".to_string());
        let t1 = push_toe(
            &mut out,
            d,
            false,
            ToeOperands {
                x: "cc_limit - M_cp",
                limit: "cc_limit - 0.001",
                k1: &format!("snJ * {}", r.sat),
                k2: &format!("sqrt(nJ * nJ + {})", r.sat_thr),
            },
            1,
        );
        out.push(format!("        M_cp = cc_limit - {t1};"));
        let t2 = push_toe(
            &mut out,
            d,
            false,
            ToeOperands {
                x: "M_cp",
                limit: "cc_limit",
                k1: &format!("nJ * {}", r.compr),
                k2: "snJ",
            },
            2,
        );
        out.push(format!("        M_cp = {t2};"));
        out.push("        M_cp = M_cp * Mnorm;".to_string());
        out.push("    }".to_string());

        out.push(d.set(&format!("{}(J_ts, M_cp, h)", d.v3)));
    } else {
        // Tonescale J_ts -> J (`tonescale_inv_j`): `J -> Y`, closed-form inverse tonescale, `Y -> J`.
        out.push(d.decl_f(
            "ts_a",
            &format!("pow(abs(Jin) * {inv_jscale}, {})", r.inv_cz),
        ));
        out.push(d.decl_f("ts_ra", &format!("{} * ts_a", r.a_w_j)));
        out.push(d.decl_f("ts_ralim", &format!("min(ts_ra, {ra_lim})")));
        out.push(d.decl_f(
            "ts_yin",
            &format!(
                "pow({off} * ts_ralim / (1.0 - ts_ralim), {inv_power}) / {}",
                r.f_l_n
            ),
        ));
        out.push(d.decl_f("ts_ztn", &format!("ts_yin / {}", r.n_r)));
        out.push(d.decl_f(
            "ts_z",
            &format!("min(max(ts_ztn, 0.0), {})", r.inverse_limit),
        ));
        out.push(d.decl_f(
            "ts_ht",
            &format!("0.5 * (ts_z + sqrt(ts_z * (4.0 * {} + ts_z)))", r.t_1),
        ));
        out.push(d.decl_f(
            "ts_yout",
            &format!("{} / (pow({} / ts_ht, 1.0 / {}) - 1.0)", r.s_2, r.m_2, r.g),
        ));
        out.push(d.decl_f("ts_fly", &format!("pow(ts_yout * {}, {power})", r.f_l_n)));
        out.push(d.decl_f(
            "Jsolved",
            &format!(
                "sign(Jin) * ({jscale} * pow((ts_fly / ({off} + ts_fly)) * {}, {}))",
                r.inv_a_w_j, r.cz
            ),
        ));

        // Chroma compress inverse (`chroma_compress_inv`): `nJ` and the final ratio use the input
        // display `J_ts` (`Jin`); the ratio denominator and the output `J` are the solved `Jsolved`.
        out.push(d.var_f("Mo", "Mch"));
        out.push("    if (Mch != 0.0) {".to_string());
        out.push(format!(
            "    {}",
            d.decl_f("nJ", &format!("Jin / {}", r.limit_j_max))
        ));
        out.push(format!("    {}", d.decl_f("snJ", "max(0.0, 1.0 - nJ)")));
        out.push(format!(
            "    {}",
            d.decl_f(
                "cc_limit",
                &format!("pow(nJ, {}) * reachMaxM / Mnorm", r.model_gamma_inv)
            )
        ));
        out.push("        Mo = Mch / Mnorm;".to_string());
        let t1 = push_toe(
            &mut out,
            d,
            true,
            ToeOperands {
                x: "Mo",
                limit: "cc_limit",
                k1: &format!("nJ * {}", r.compr),
                k2: "snJ",
            },
            1,
        );
        out.push(format!("        Mo = {t1};"));
        let t2 = push_toe(
            &mut out,
            d,
            true,
            ToeOperands {
                x: "cc_limit - Mo",
                limit: "cc_limit - 0.001",
                k1: &format!("snJ * {}", r.sat),
                k2: &format!("sqrt(nJ * nJ + {})", r.sat_thr),
            },
            2,
        );
        out.push(format!("        Mo = cc_limit - {t2};"));
        out.push("        Mo = Mo * Mnorm;".to_string());
        out.push(format!(
            "        Mo = Mo * pow(Jin / Jsolved, -{});",
            r.model_gamma_inv
        ));
        out.push("    }".to_string());

        out.push(d.set(&format!("{}(Jsolved, Mo, h)", d.v3)));
    }

    out.push("}".to_string());
    out
}

// ===========================================================================
// ACES2 Gamut compress (ACES2-03, stage 3)
// ===========================================================================
//
// Emits `aces2::gamut_compress_{fwd,inv}` in-shader, replacing that op's `[0,1]^3` LUT bake. Gamut
// compress is closed-form except for one per-hue lookup: the display-gamut **cusp** `[J, M,
// gamma_top_inv]`, indexed off a NON-uniform hue axis (`hue_table`) by a binary search
// (`lookup_hue_interval`) then a linear lerp between the bracketing cusp entries. That search is
// transcribed here term-for-term from the CPU oracle — the same trap the reach lookup hit: uploading
// the cusp table as a storage array (not the reference's RGB texture) makes the lookup our code's
// responsibility, so it only equals the reference where it replicates the CPU indexing exactly.
//
// The per-hue reach limit `reachMaxM` is the same table the tonescale stage samples, via the shared
// `emit_reach_sample`. The inverse is the CPU's two-pass fixed-point refinement: below the analytical
// threshold a single `compress_gamut_core::<true>`; above it, one pass to refine `Jx` (taking only its
// J) then a second pass with that `Jx` — matching OCIO `_Add_Gamut_Compress_Inv_Shader`
// (`FixedFunctionOpGPU.cpp:1247`) and, bit-for-bit, our verified CPU `gamut_compress_inv`.

/// Dialect-resolved access expressions for the `Aces2Gamut` uniforms.
///
/// Scalars are already spelled the way each language needs (bare name for GLSL/HLSL/Metal,
/// `scalar_f32(<NAME>_OFFSET)` for WGSL); the three array names stay raw because the reach / hue /
/// cusp samplers build their own access (`ocio_arrays_f[<NAME>_BASE + i]` for WGSL, a named
/// `float[]` for the C-family).
pub(super) struct GamutRefs<'a> {
    pub limit_j_max: &'a str,
    pub model_gamma_inv: &'a str,
    pub mid_j: &'a str,
    pub focus_dist: &'a str,
    pub lower_hull_gamma_inv: &'a str,
    pub search_lo: &'a str,
    pub search_hi: &'a str,
    /// Raw name of the reach-`M` `ArrayFloat` uniform (363 entries), reused from the tonescale stage.
    pub reach: &'a str,
    /// Raw name of the non-uniform hue-axis `ArrayFloat` uniform (363 entries).
    pub hue_table: &'a str,
    /// Raw name of the cusp `ArrayFloat` uniform (363×3, `[J, M, gamma_top_inv]` per hue).
    pub cusp_table: &'a str,
}

/// Resolves the [`super::types::OpUniforms::Aces2Gamut`] names into dialect access expressions and
/// emits the body — the single entry point `ops::emit` calls.
pub(super) fn emit_gamut_op(
    forward: bool,
    uniforms: Option<&super::types::OpUniforms>,
    d: &Dialect,
    storage_buffer: bool,
) -> Vec<String> {
    use super::types::OpUniforms;
    let Some(OpUniforms::Aces2Gamut {
        limit_j_max,
        model_gamma_inv,
        mid_j,
        focus_dist,
        lower_hull_gamma_inv,
        search_lo,
        search_hi,
        reach,
        hue_table,
        cusp_table,
    }) = uniforms
    else {
        // build_uniforms always pairs this op with its uniforms; name the failure rather than
        // emitting a silently wrong (identity) body if that invariant is ever broken.
        return vec!["// ERROR: missing ACES2 gamut uniforms".to_string()];
    };

    let flt = |name: &str| {
        if storage_buffer {
            format!("scalar_f32({}_OFFSET)", name.to_uppercase())
        } else {
            name.to_string()
        }
    };
    let (
        limit_j_max,
        model_gamma_inv,
        mid_j,
        focus_dist,
        lower_hull_gamma_inv,
        search_lo,
        search_hi,
    ) = (
        flt(limit_j_max),
        flt(model_gamma_inv),
        flt(mid_j),
        flt(focus_dist),
        flt(lower_hull_gamma_inv),
        flt(search_lo),
        flt(search_hi),
    );

    let refs = GamutRefs {
        limit_j_max: &limit_j_max,
        model_gamma_inv: &model_gamma_inv,
        mid_j: &mid_j,
        focus_dist: &focus_dist,
        lower_hull_gamma_inv: &lower_hull_gamma_inv,
        search_lo: &search_lo,
        search_hi: &search_hi,
        reach,
        hue_table,
        cusp_table,
    };
    gamut_compress(forward, &refs, d, storage_buffer)
}

/// `estimate_line_boundary_m(j_int, slope, inv_gamma, j_max, m_max, j_ref)` as one expression.
///
/// CPU: `j_ref * pow(j_int/j_ref, inv_gamma) * m_max / (j_max - slope*m_max)`. Pure and side-effect
/// free, so it stays an inline expression rather than a pushed block.
fn elbm(
    j_int: &str,
    slope: &str,
    inv_gamma: &str,
    j_max: &str,
    m_max: &str,
    j_ref: &str,
) -> String {
    format!(
        "(({j_ref}) * pow(({j_int}) / ({j_ref}), {inv_gamma}) * ({m_max}) / (({j_max}) - ({slope}) * ({m_max})))"
    )
}

/// Inlines `get_focus_gain(jx, thr, limit, focus_dist)` into a mutable `{dst}` in the enclosing scope.
///
/// CPU: `gain = limit*focus_dist; if jx > thr { adj = log10((limit-thr)/max(limit-jx, 0.0001)); gain
/// *= adj*adj + 1 }`. The `if` is real control flow (the reference's too), so no NaN leaks from an
/// untaken arm. `log10` is `log(x) * LOG10_E` — GLSL/WGSL have no `log10`; the argument is provably
/// positive here so the flooring `Dialect::log10` would add is unnecessary and would only widen the
/// gap to the CPU's `f32::log10`.
/// The shader-expression operands of one `get_focus_gain(jx, thr, limit, focus_dist)` (OCIO
/// `getFocusGain`).
struct FocusGainOperands<'a> {
    jx: &'a str,
    thr: &'a str,
    limit: &'a str,
    focus_dist: &'a str,
}

fn push_focus_gain(
    out: &mut Vec<String>,
    d: &Dialect,
    dst: &str,
    operands: FocusGainOperands<'_>,
    tag: &str,
) {
    let FocusGainOperands {
        jx,
        thr,
        limit,
        focus_dist,
    } = operands;
    let p = format!("fg{tag}_");
    out.push(d.var_f(dst, &format!("({limit}) * ({focus_dist})")));
    out.push("    {".to_string());
    out.push(format!("    {}", d.decl_f(&format!("{p}j"), jx)));
    out.push(format!("        if (({p}j) > ({thr})) {{"));
    out.push(format!(
        "        {}",
        d.decl_f(
            &format!("{p}g"),
            &format!("(({limit}) - ({thr})) / max(0.0001, ({limit}) - {p}j)")
        )
    ));
    out.push(format!(
        "        {}",
        d.decl_f(&format!("{p}l"), &format!("log({p}g) * {}", n(LOG10_E)))
    ));
    out.push(format!("            {dst} = {dst} * ({p}l * {p}l + 1.0);"));
    out.push("        }".to_string());
    out.push("    }".to_string());
}

/// Inlines `solve_j_intersect(j, m, focusj, max_j, slope_gain)` into a mutable `{dst}`.
///
/// Two closed-form quadratic-root branches on `j < focusj`; emitted as a real `if/else` so the
/// untaken branch's `sqrt(det)` (which can be of a negative `det`) is never evaluated — matching the
/// CPU, which branches too.
#[allow(clippy::too_many_arguments)]
fn push_solve_j(
    out: &mut Vec<String>,
    d: &Dialect,
    dst: &str,
    j: &str,
    m: &str,
    focusj: &str,
    limit: &str,
    slope_gain: &str,
    tag: &str,
) {
    let p = format!("sj{tag}_");
    out.push(d.var_f(dst, "0.0"));
    out.push("    {".to_string());
    out.push(format!(
        "    {}",
        d.decl_f(&format!("{p}ms"), &format!("({m}) / ({slope_gain})"))
    ));
    out.push(format!(
        "    {}",
        d.decl_f(&format!("{p}a"), &format!("{p}ms / ({focusj})"))
    ));
    out.push(format!("    {}", d.decl_f(&format!("{p}j"), j)));
    out.push(format!("        if (({p}j) < ({focusj})) {{"));
    out.push(format!(
        "        {}",
        d.decl_f(&format!("{p}b"), &format!("1.0 - {p}ms"))
    ));
    out.push(format!(
        "        {}",
        d.decl_f(&format!("{p}c"), &format!("-{p}j"))
    ));
    out.push(format!(
        "        {}",
        d.decl_f(
            &format!("{p}det"),
            &format!("{p}b * {p}b - 4.0 * {p}a * {p}c")
        )
    ));
    out.push(format!(
        "            {dst} = -2.0 * {p}c / ({p}b + sqrt({p}det));"
    ));
    out.push("        } else {".to_string());
    out.push(format!(
        "        {}",
        d.decl_f(
            &format!("{p}b"),
            &format!("-(1.0 + {p}ms + ({limit}) * {p}a)")
        )
    ));
    out.push(format!(
        "        {}",
        d.decl_f(&format!("{p}c"), &format!("({limit}) * {p}ms + {p}j"))
    ));
    out.push(format!(
        "        {}",
        d.decl_f(
            &format!("{p}det"),
            &format!("{p}b * {p}b - 4.0 * {p}a * {p}c")
        )
    ));
    out.push(format!(
        "            {dst} = -2.0 * {p}c / ({p}b - sqrt({p}det));"
    ));
    out.push("        }".to_string());
    out.push("    }".to_string());
}

/// Inlines `find_gamut_boundary_m(...)` into an immutable `{dst}`: `smin_scaled` of the lower- and
/// upper-hull `estimate_line_boundary_m`s, exactly as the CPU (`aces2::find_gamut_boundary_m` +
/// `smin_scaled`). The upper hull feeds `-slope`, so its `elbm` denominator becomes `f_jm + slope*m`.
#[allow(clippy::too_many_arguments)]
fn push_find_gamut_boundary(
    out: &mut Vec<String>,
    d: &Dialect,
    dst: &str,
    j_int_src: &str,
    j_int_cusp: &str,
    slope: &str,
    cusp_j: &str,
    cusp_m: &str,
    gamma_top_inv: &str,
    gamma_bottom_inv: &str,
    limit: &str,
    tag: &str,
) {
    let p = format!("gb{tag}_");
    out.push(d.decl_f(
        &format!("{p}ml"),
        &elbm(
            j_int_src,
            slope,
            gamma_bottom_inv,
            cusp_j,
            cusp_m,
            j_int_cusp,
        ),
    ));
    out.push(d.decl_f(&format!("{p}fjc"), &format!("({limit}) - ({j_int_cusp})")));
    out.push(d.decl_f(&format!("{p}fjs"), &format!("({limit}) - ({j_int_src})")));
    out.push(d.decl_f(&format!("{p}fjm"), &format!("({limit}) - ({cusp_j})")));
    out.push(d.decl_f(
        &format!("{p}mu"),
        &elbm(
            &format!("{p}fjs"),
            &format!("-({slope})"),
            gamma_top_inv,
            &format!("{p}fjm"),
            cusp_m,
            &format!("{p}fjc"),
        ),
    ));
    // smin_scaled(ml, mu, cusp_m): s = SMOOTH_CUSPS*cusp_m; h = max(s - |ml-mu|, 0)/s;
    //                              min(ml, mu) - h^3 * s / 6.
    out.push(d.decl_f(
        &format!("{p}s"),
        &format!("{} * ({cusp_m})", n(SMOOTH_CUSPS as f64)),
    ));
    out.push(d.decl_f(
        &format!("{p}h"),
        &format!("max({p}s - abs({p}ml - {p}mu), 0.0) / {p}s"),
    ));
    out.push(d.decl_f(
        dst,
        &format!(
            "min({p}ml, {p}mu) - {p}h * {p}h * {p}h * {p}s * {}",
            n(1.0 / 6.0)
        ),
    ));
}

/// Inlines `remap_M_{fwd,inv}(m, gamut_m, reach_m)` into a mutable `{dst}` (default `m`, the CPU's
/// pass-through when the guard trips). The guard `m <= threshold || proportion >= 1` is a real `if`,
/// and the inverse's `nd >= 1` split is one too, so the singular `nd/(nd-1)` at `nd == 1` is never
/// reached — matching `aces2::reinhard_inv`.
#[allow(clippy::too_many_arguments)]
fn push_remap(
    out: &mut Vec<String>,
    d: &Dialect,
    invert: bool,
    dst: &str,
    m: &str,
    gamut_m: &str,
    reach_m: &str,
    tag: &str,
) {
    let p = format!("rm{tag}_");
    out.push(d.var_f(dst, m));
    out.push("    {".to_string());
    out.push(format!("    {}", d.decl_f(&format!("{p}m"), m)));
    out.push(format!("    {}", d.decl_f(&format!("{p}gm"), gamut_m)));
    out.push(format!("    {}", d.decl_f(&format!("{p}rm"), reach_m)));
    out.push(format!(
        "    {}",
        d.decl_f(&format!("{p}ratio"), &format!("{p}gm / {p}rm"))
    ));
    out.push(format!(
        "    {}",
        d.decl_f(
            &format!("{p}prop"),
            &format!("max({p}ratio, {})", n(COMPRESSION_THRESHOLD as f64))
        )
    ));
    out.push(format!(
        "    {}",
        d.decl_f(&format!("{p}thr"), &format!("{p}prop * {p}gm"))
    ));
    out.push(format!(
        "        if (!({p}prop >= 1.0 || {p}m <= {p}thr)) {{"
    ));
    out.push(format!(
        "        {}",
        d.decl_f(&format!("{p}moff"), &format!("{p}m - {p}thr"))
    ));
    out.push(format!(
        "        {}",
        d.decl_f(&format!("{p}goff"), &format!("{p}gm - {p}thr"))
    ));
    out.push(format!(
        "        {}",
        d.decl_f(&format!("{p}roff"), &format!("{p}rm - {p}thr"))
    ));
    out.push(format!(
        "        {}",
        d.decl_f(
            &format!("{p}scale"),
            &format!("{p}roff / ({p}roff / {p}goff - 1.0)")
        )
    ));
    out.push(format!(
        "        {}",
        d.decl_f(&format!("{p}nd"), &format!("{p}moff / {p}scale"))
    ));
    if invert {
        out.push(format!("            if ({p}nd >= 1.0) {{"));
        out.push(format!("                {dst} = {p}thr + {p}scale;"));
        out.push("            } else {".to_string());
        out.push(format!(
            "                {dst} = {p}thr + {p}scale * (-({p}nd / ({p}nd - 1.0)));"
        ));
        out.push("            }".to_string());
    } else {
        out.push(format!(
            "            {dst} = {p}thr + {p}scale * {p}nd / (1.0 + {p}nd);"
        ));
    }
    out.push("        }".to_string());
    out.push("    }".to_string());
}

/// Inlines one `compress_gamut_core::<INV>` pass, writing its J into `{dst_j}` and M into `{dst_m}`
/// (both `var`s the caller declares). `jx` is the focus-gain argument (the input J on the first
/// pass, the refined J on the inverse's second). The original `gJ`/`gM` (bound once at the op top)
/// feed `solve_j_intersect`; only `jx` differs between the inverse's two passes.
#[allow(clippy::too_many_arguments)]
fn push_compress_core(
    out: &mut Vec<String>,
    d: &Dialect,
    invert: bool,
    tag: &str,
    jx: &str,
    r: &GamutRefs,
    dst_j: &str,
    dst_m: &str,
    cusp_j: &str,
    cusp_m: &str,
    gamma_top_inv: &str,
    focusj: &str,
) {
    let p = format!("cg{tag}_");
    push_focus_gain(
        out,
        d,
        &format!("{p}sg"),
        FocusGainOperands {
            jx,
            thr: "gc_thr",
            limit: r.limit_j_max,
            focus_dist: r.focus_dist,
        },
        tag,
    );
    push_solve_j(
        out,
        d,
        &format!("{p}jis"),
        "gJ",
        "gM",
        focusj,
        r.limit_j_max,
        &format!("{p}sg"),
        &format!("{tag}s"),
    );
    // compute_compression_slope: dir = (jis < focusj) ? jis : (limit - jis); dir*(jis-focusj)/(focusj*sg).
    out.push(d.var_f(
        &format!("{p}gs"),
        &d.sel(
            &format!("{p}jis < ({focusj})"),
            &format!("{p}jis"),
            &format!("({}) - {p}jis", r.limit_j_max),
        ),
    ));
    out.push(format!(
        "    {p}gs = {p}gs * ({p}jis - ({focusj})) / (({focusj}) * {p}sg);"
    ));
    push_solve_j(
        out,
        d,
        &format!("{p}jic"),
        cusp_j,
        cusp_m,
        focusj,
        r.limit_j_max,
        &format!("{p}sg"),
        &format!("{tag}c"),
    );
    push_find_gamut_boundary(
        out,
        d,
        &format!("{p}gm"),
        &format!("{p}jis"),
        &format!("{p}jic"),
        &format!("{p}gs"),
        cusp_j,
        cusp_m,
        gamma_top_inv,
        r.lower_hull_gamma_inv,
        r.limit_j_max,
        tag,
    );
    // gamut_m <= 0 short-circuits to [J, 0, h] (the default here); otherwise remap M and shift J.
    out.push(d.var_f(dst_j, "gJ"));
    out.push(d.var_f(dst_m, "0.0"));
    out.push(format!("    if ({p}gm > 0.0) {{"));
    out.push(format!(
        "    {}",
        d.decl_f(
            &format!("{p}rm"),
            &elbm(
                &format!("{p}jis"),
                &format!("{p}gs"),
                r.model_gamma_inv,
                r.limit_j_max,
                "reachMaxM",
                r.limit_j_max
            ),
        )
    ));
    push_remap(
        out,
        d,
        invert,
        &format!("{p}rem"),
        "gM",
        &format!("{p}gm"),
        &format!("{p}rm"),
        tag,
    );
    out.push(format!("        {dst_j} = {p}jis + {p}rem * {p}gs;"));
    out.push(format!("        {dst_m} = {p}rem;"));
    out.push("    }".to_string());
}

/// Emits the per-hue cusp lookup, declaring immutable `cuspJ`, `cuspM`, `gammaTopInv` from `hvar`.
///
/// Reproduces `aces2::init_hue_gamut_params`'s `lookup_hue_interval` (binary search over the
/// non-uniform `hue_table`, seeded/clamped from the config's `hue_linearity_search_range`) + the
/// `lerp(cusp[i_hi-1], cusp[i_hi], (h - hue[i_hi-1])/(hue[i_hi] - hue[i_hi-1]))` — NOT a nearest
/// sample and NOT hardware texture filtering. The search loop is capped at 16 iterations (≥
/// `ceil(log2(361))`), the largest possible window; the natural `i_lo + 1 >= i_hi` break makes the
/// cap moot for termination and only guards WGSL's structured-loop requirement.
fn emit_cusp_sample(out: &mut Vec<String>, d: &Dialect, wgsl: bool, r: &GamutRefs, hvar: &str) {
    let first = TABLE_FIRST_NOM;
    let loww = TABLE_LOWER_WRAP;
    let uppw = TABLE_UPPER_WRAP;
    if wgsl {
        let hue_base = format!("{}_BASE", r.hue_table.to_uppercase());
        let cusp_base = format!("{}_BASE", r.cusp_table.to_uppercase());
        out.push(format!("    let gc_i_start: i32 = i32({hvar}) + {first};"));
        out.push(format!(
            "    var gc_i_lo: i32 = max({loww}, gc_i_start + i32({}));",
            r.search_lo
        ));
        out.push(format!(
            "    var gc_i_hi: i32 = min({uppw}, gc_i_start + i32({}));",
            r.search_hi
        ));
        out.push("    var gc_i: i32 = gc_i_start;".to_string());
        out.push("    for (var gc_it: i32 = 0; gc_it < 16; gc_it = gc_it + 1) {".to_string());
        out.push("        if (gc_i_lo + 1 >= gc_i_hi) { break; }".to_string());
        out.push(format!(
            "        let gc_hcur = ocio_arrays_f[{hue_base} + u32(gc_i)];"
        ));
        out.push(format!(
            "        if ({hvar} > gc_hcur) {{ gc_i_lo = gc_i; }} else {{ gc_i_hi = gc_i; }}"
        ));
        out.push("        gc_i = (gc_i_lo + gc_i_hi) / 2;".to_string());
        out.push("    }".to_string());
        out.push("    gc_i_hi = max(gc_i_hi, 1);".to_string());
        out.push(format!(
            "    let gc_lo_j = ocio_arrays_f[{cusp_base} + u32(gc_i_hi - 1) * 3u];"
        ));
        out.push(format!(
            "    let gc_lo_m = ocio_arrays_f[{cusp_base} + u32(gc_i_hi - 1) * 3u + 1u];"
        ));
        out.push(format!(
            "    let gc_lo_g = ocio_arrays_f[{cusp_base} + u32(gc_i_hi - 1) * 3u + 2u];"
        ));
        out.push(format!(
            "    let gc_hi_j = ocio_arrays_f[{cusp_base} + u32(gc_i_hi) * 3u];"
        ));
        out.push(format!(
            "    let gc_hi_m = ocio_arrays_f[{cusp_base} + u32(gc_i_hi) * 3u + 1u];"
        ));
        out.push(format!(
            "    let gc_hi_g = ocio_arrays_f[{cusp_base} + u32(gc_i_hi) * 3u + 2u];"
        ));
        out.push(format!(
            "    let gc_hlo = ocio_arrays_f[{hue_base} + u32(gc_i_hi - 1)];"
        ));
        out.push(format!(
            "    let gc_hhi = ocio_arrays_f[{hue_base} + u32(gc_i_hi)];"
        ));
    } else {
        out.push(format!("    int gc_i_start = int({hvar}) + {first};"));
        out.push(format!(
            "    int gc_i_lo = int(max(float({loww}), float(gc_i_start) + {}));",
            r.search_lo
        ));
        out.push(format!(
            "    int gc_i_hi = int(min(float({uppw}), float(gc_i_start) + {}));",
            r.search_hi
        ));
        out.push("    int gc_i = gc_i_start;".to_string());
        out.push("    for (int gc_it = 0; gc_it < 16; gc_it++) {".to_string());
        out.push("        if (gc_i_lo + 1 >= gc_i_hi) break;".to_string());
        out.push(format!("        float gc_hcur = {}[gc_i];", r.hue_table));
        out.push(format!(
            "        if ({hvar} > gc_hcur) gc_i_lo = gc_i; else gc_i_hi = gc_i;"
        ));
        out.push("        gc_i = (gc_i_lo + gc_i_hi) / 2;".to_string());
        out.push("    }".to_string());
        out.push("    gc_i_hi = int(max(float(gc_i_hi), 1.0));".to_string());
        out.push(format!(
            "    float gc_lo_j = {}[(gc_i_hi - 1) * 3];",
            r.cusp_table
        ));
        out.push(format!(
            "    float gc_lo_m = {}[(gc_i_hi - 1) * 3 + 1];",
            r.cusp_table
        ));
        out.push(format!(
            "    float gc_lo_g = {}[(gc_i_hi - 1) * 3 + 2];",
            r.cusp_table
        ));
        out.push(format!(
            "    float gc_hi_j = {}[gc_i_hi * 3];",
            r.cusp_table
        ));
        out.push(format!(
            "    float gc_hi_m = {}[gc_i_hi * 3 + 1];",
            r.cusp_table
        ));
        out.push(format!(
            "    float gc_hi_g = {}[gc_i_hi * 3 + 2];",
            r.cusp_table
        ));
        out.push(format!("    float gc_hlo = {}[gc_i_hi - 1];", r.hue_table));
        out.push(format!("    float gc_hhi = {}[gc_i_hi];", r.hue_table));
    }
    out.push(d.decl_f("gc_ct", &format!("({hvar} - gc_hlo) / (gc_hhi - gc_hlo)")));
    out.push(d.decl_f("cuspJ", &d.mix("gc_lo_j", "gc_hi_j", "gc_ct")));
    out.push(d.decl_f("cuspM", &d.mix("gc_lo_m", "gc_hi_m", "gc_ct")));
    out.push(d.decl_f("gammaTopInv", &d.mix("gc_lo_g", "gc_hi_g", "gc_ct")));
}

/// Emits the shader body for `gamut_compress_fwd` (`forward`) or `_inv`.
///
/// Reference: OCIO `Add_Gamut_Compress_{Fwd,Inv}_Shader` (`FixedFunctionOpGPU.cpp:1550/1593`).
/// Transcribed against the verified CPU oracle `aces2::gamut_compress_{fwd,inv}` so `cpu_equals_emit_*`
/// is exact (f32-vs-transcendental tol only).
fn gamut_compress(forward: bool, r: &GamutRefs, d: &Dialect, storage_buffer: bool) -> Vec<String> {
    let full = n(HUE_FULL);
    let dir = if forward { "fwd" } else { "inv" };
    let mut out = vec![format!("{{ // ACES2 gamut compress ({dir})")];

    // Bind the JMh channels; wrap the hue into [0, 360) exactly as `wrap_to_hue_limit`.
    out.push(d.decl_f("gJ", "color.r"));
    out.push(d.decl_f("gM", "color.g"));
    out.push(d.var_f("gh", "color.b"));
    out.push(format!("    gh = gh - floor(gh / {full}) * {full};"));
    out.push(format!(
        "    gh = {};",
        d.sel("gh < 0.0", &format!("gh + {full}"), "gh")
    ));

    // sr.reach_max_m (shared reach sampler) and the per-hue display cusp.
    emit_reach_sample(
        &mut out,
        d,
        storage_buffer,
        r.reach,
        "gh",
        "reachMaxM",
        "gc",
    );
    emit_cusp_sample(&mut out, d, storage_buffer, r, "gh");

    // init_hue_gamut_params: focus_j (compute_focus_j) and analytical_threshold (lerp @ FOCUS_GAIN_BLEND).
    out.push(d.decl_f(
        "focusJ",
        &d.mix(
            "cuspJ",
            r.mid_j,
            &format!(
                "min({} - cuspJ / ({}), 1.0)",
                n(CUSP_MID_BLEND as f64),
                r.limit_j_max
            ),
        ),
    ));
    out.push(d.decl_f(
        "gc_thr",
        &d.mix("cuspJ", r.limit_j_max, &n(FOCUS_GAIN_BLEND as f64)),
    ));

    out.push(d.var3("gc_out", &format!("{}(gJ, gM, gh)", d.v3)));
    out.push("    if (gJ <= 0.0) {".to_string());
    out.push(format!("        gc_out = {}(0.0, 0.0, gh);", d.v3));
    out.push(format!(
        "    }} else if (gM <= 0.0 || gJ > ({})) {{",
        r.limit_j_max
    ));
    out.push(format!("        gc_out = {}(gJ, 0.0, gh);", d.v3));
    out.push("    } else {".to_string());
    if forward {
        push_compress_core(
            &mut out,
            d,
            false,
            "f",
            "gJ",
            r,
            "cgf_J",
            "cgf_M",
            "cuspJ",
            "cuspM",
            "gammaTopInv",
            "focusJ",
        );
        out.push(format!("        gc_out = {}(cgf_J, cgf_M, gh);", d.v3));
    } else {
        // Two-pass fixed point: refine Jx above the analytical threshold, then a final pass.
        out.push(d.var_f("gc_jx", "gJ"));
        out.push("        if (gJ > gc_thr) {".to_string());
        push_compress_core(
            &mut out,
            d,
            true,
            "i1",
            "gc_jx",
            r,
            "ci1_J",
            "ci1_M",
            "cuspJ",
            "cuspM",
            "gammaTopInv",
            "focusJ",
        );
        out.push("            gc_jx = ci1_J;".to_string());
        out.push("        }".to_string());
        push_compress_core(
            &mut out,
            d,
            true,
            "i2",
            "gc_jx",
            r,
            "ci2_J",
            "ci2_M",
            "cuspJ",
            "cuspM",
            "gammaTopInv",
            "focusJ",
        );
        out.push(format!("        gc_out = {}(ci2_J, ci2_M, gh);", d.v3));
    }
    out.push("    }".to_string());

    out.push(d.set("gc_out"));
    out.push("}".to_string());
    out
}

// ===========================================================================
// ACES2 Output Transform (ACES2-03, stage 4) — the full composition
// ===========================================================================
//
// Emits `aces2::output_transform_{fwd,inv}` in-shader, replacing the LAST ACES2 op still on the
// `[0,1]^3` LUT bake. The bake sampled the CPU output transform over the unit cube with no shaper, so
// every scene-linear value > 1.0 clamped at the cube's far face and HDR highlights flattened; the
// composition below carries the unbounded scene-linear domain end to end.
//
// It is PURE composition: the CPU `output_transform_fwd`/`_inv` chain the exact same sub-ops this
// module already emits verified (stages 1-3), so `emit_output_op` reuses their wrapper entry points
// (`emit_op` / `emit_tonescale_op` / `emit_gamut_op`) wholesale and writes NO new ACES2 maths. Each
// sub-block reads and writes the running `color` and preserves hue through tonescale/gamut, which is
// exactly why chaining the standalone bodies equals the CPU (whose `jmh_to_aab_trig` reuses the input
// hue's sin/cos — identical because hue is invariant). Reference:
// `FixedFunctionOpGPU.cpp`'s `Add_ACES_OutputTransform_{Fwd,Inv}_Shader`, which composes the same
// sub-shaders in the same order.

/// Emits the shader body for `output_transform_fwd` (`forward`) or `_inv`, as the composition of the
/// verified stage-1..3 emitters.
///
/// Forward (scene AP0 -> display): `RGB->JMh(p_in)` -> tonescale + chroma -> gamut -> `JMh->RGB(p_out)`.
/// Inverse (display -> scene AP0): `RGB->JMh(p_out)` -> gamut_inv -> tonescale + chroma_inv ->
/// `JMh->RGB(p_in)`. The two `RGB <-> JMh` stages use DIFFERENT primaries (`p_in` at the scene end,
/// `p_out` at the display end), which is the whole reason this cannot be a naive replay of one JMh
/// op — the [`super::types::OpUniforms::Aces2Output`] carries both param sets so the right matrices
/// reach each end.
pub(super) fn emit_output_op(
    forward: bool,
    uniforms: Option<&super::types::OpUniforms>,
    d: &Dialect,
    storage_buffer: bool,
) -> Vec<String> {
    use super::types::OpUniforms;
    let Some(OpUniforms::Aces2Output {
        jmh_in,
        jmh_out,
        tonescale,
        gamut,
    }) = uniforms
    else {
        // build_uniforms always pairs this op with its uniforms; name the failure rather than
        // emitting a silently wrong (identity) body if that invariant is ever broken.
        return vec!["// ERROR: missing ACES2 output-transform uniforms".to_string()];
    };

    let (jmh_in, jmh_out, tonescale, gamut) = (
        jmh_in.as_ref(),
        jmh_out.as_ref(),
        tonescale.as_ref(),
        gamut.as_ref(),
    );

    let mut out = Vec::new();
    if forward {
        out.extend(emit_op(true, Some(jmh_in), d, storage_buffer));
        out.extend(emit_tonescale_op(true, Some(tonescale), d, storage_buffer));
        out.extend(emit_gamut_op(true, Some(gamut), d, storage_buffer));
        out.extend(emit_op(false, Some(jmh_out), d, storage_buffer));
    } else {
        out.extend(emit_op(true, Some(jmh_out), d, storage_buffer));
        out.extend(emit_gamut_op(false, Some(gamut), d, storage_buffer));
        out.extend(emit_tonescale_op(false, Some(tonescale), d, storage_buffer));
        out.extend(emit_op(false, Some(jmh_in), d, storage_buffer));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::dialect::{GLSL, HLSL, METAL, WGSL};

    /// Bare-name refs; the statement count does not depend on the access spelling.
    fn refs() -> JmhRefs<'static> {
        JmhRefs {
            mat_rgb_to_cam16_c: "m_rc",
            mat_cam16_c_to_rgb: "m_cr",
            mat_cone_to_aab: "m_ca",
            mat_aab_to_cone: "m_ac",
            cz: "cz",
            inv_cz: "inv_cz",
        }
    }

    /// All four dialects must emit the same number of statements for each direction — the invariant
    /// the crate relies on to keep the languages from silently diverging.
    #[test]
    fn dialects_agree_structurally() {
        for forward in [true, false] {
            let expect = emit(forward, &refs(), &GLSL).len();
            for (name, d) in [("HLSL", &HLSL), ("Metal", &METAL), ("WGSL", &WGSL)] {
                assert_eq!(
                    emit(forward, &refs(), d).len(),
                    expect,
                    "{name} emits a different statement count (forward={forward})"
                );
            }
        }
    }

    /// The matrix multiply must be `M * v` on the column-major backends and `mul(M, v)` on HLSL —
    /// the OCIO-M6 hazard, pinned here so a regression to a bare `*` on HLSL is caught.
    #[test]
    fn hlsl_uses_mul_others_use_star() {
        let hlsl = emit(true, &refs(), &HLSL).join("\n");
        assert!(
            hlsl.contains("mul(m_rc, color.rgb)"),
            "HLSL must use mul():\n{hlsl}"
        );
        for d in [&GLSL, &METAL, &WGSL] {
            let body = emit(true, &refs(), d).join("\n");
            assert!(
                body.contains("(m_rc) * (color.rgb)"),
                "non-HLSL must use `*`:\n{body}"
            );
        }
    }

    /// GLSL spells full-circle arctangent `atan(y, x)`; the others `atan2(y, x)`.
    #[test]
    fn atan2_spelling_per_dialect() {
        assert!(
            emit(true, &refs(), &GLSL)
                .join("\n")
                .contains("atan(aab.b, aab.g)")
        );
        for d in [&HLSL, &METAL, &WGSL] {
            assert!(
                emit(true, &refs(), d)
                    .join("\n")
                    .contains("atan2(aab.b, aab.g)")
            );
        }
    }

    /// Bare-name gamut refs; the statement count does not depend on the access spelling.
    fn gamut_refs() -> GamutRefs<'static> {
        GamutRefs {
            limit_j_max: "ljm",
            model_gamma_inv: "mgi",
            mid_j: "mj",
            focus_dist: "fd",
            lower_hull_gamma_inv: "lhg",
            search_lo: "slo",
            search_hi: "shi",
            reach: "reach",
            hue_table: "hue",
            cusp_table: "cusp",
        }
    }

    /// All four dialects must emit the same gamut-compress statement count for each direction — the
    /// WGSL storage-array cusp/reach samplers and the C-family named-array ones are shaped to match
    /// line-for-line, so a dialect silently diverging (e.g. a dropped loop line) fails here. WGSL is
    /// the only backend fully checked (naga + live wgpu) in `tests/aces2_gpu_emission.rs`; this pins
    /// the other three structurally, the same bar `dialects_agree_structurally` sets for RGB<->JMh.
    #[test]
    fn gamut_dialects_agree_structurally() {
        for forward in [true, false] {
            // storage_buffer is set only for WGSL; it selects the array-access shape, not the count.
            let expect = gamut_compress(forward, &gamut_refs(), &GLSL, false).len();
            for (name, d, sb) in [
                ("HLSL", &HLSL, false),
                ("Metal", &METAL, false),
                ("WGSL", &WGSL, true),
            ] {
                assert_eq!(
                    gamut_compress(forward, &gamut_refs(), d, sb).len(),
                    expect,
                    "{name} emits a different gamut statement count (forward={forward})"
                );
            }
        }
    }

    /// The inverse must emit the two-pass fixed point (two `compress_gamut_core` passes, tags i1/i2),
    /// where the forward emits one (tag f) — the load-bearing structural difference. Pinned so an
    /// inverse regressing to a single pass, or the forward growing a spurious second, is caught.
    #[test]
    fn gamut_inverse_is_two_pass() {
        let fwd = gamut_compress(true, &gamut_refs(), &WGSL, true).join("\n");
        let inv = gamut_compress(false, &gamut_refs(), &WGSL, true).join("\n");
        // Each core pass declares its own reach-boundary local `cg<tag>_rm`.
        assert!(
            fwd.contains("cgf_rm") && !fwd.contains("cgi2_rm"),
            "forward must be a single core pass"
        );
        assert!(
            inv.contains("cgi1_rm") && inv.contains("cgi2_rm"),
            "inverse must emit two core passes"
        );
    }
}
