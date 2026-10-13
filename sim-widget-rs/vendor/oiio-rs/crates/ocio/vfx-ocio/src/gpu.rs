//! GPU processor support for real-time color transforms.
//!
//! Generates GLSL shader code from OCIO processors for use in OpenGL/Vulkan
//! viewers and compositing applications.
//!
//! # Example
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use vfx_ocio::{Config, GpuProcessor, GpuLanguage};
//!
//! let config = Config::from_file("config.ocio")?;
//! let processor = config.processor("ACEScg", "sRGB")?;
//!
//! let gpu = GpuProcessor::from_processor(&processor)?;
//! let shader = gpu.generate_shader(GpuLanguage::Glsl330)?;
//!
//! println!("{}", shader.fragment_code());
//! # Ok(())
//! # }
//! ```
//!
//! # Supported Operations
//!
//! - Matrix transforms (color matrices)
//! - CDL (ASC Color Decision List)
//! - 1D/3D LUT sampling (requires texture upload)
//! - Exponent/gamma correction
//! - Log transforms
//! - Range transforms (clamp/scale)
//!
//! # ACES2 on GPU — fully in-shader, no LUT bake
//!
//! ACES2-03 replaced the `[0,1]³` LUT bake for every ACES2 op with real in-shader emission over an
//! unbounded domain, staged:
//!
//! - **Stage 1** `Aces2RgbJmh` (`RGB <-> JMh`), **stage 2** `Aces2TonescaleCompress` (tonescale +
//!   chroma compress) and **stage 3** `Aces2GamutCompress` (gamut compress) emit their CAM16/JMh
//!   math directly, with the per-config matrices/scalars uploaded as uniforms and the reach-M / hue /
//!   cusp tables as `ArrayFloat` storage-buffer uniforms — the body lives in [`aces2`]'s emitters.
//!   `J∈[0,~170]`, `M∈[0,~200]`, `h∈[0,360)` are represented exactly, not clamped into a LUT corner.
//! - **Stage 4** `Aces2OutputTransform` — the full scene-referred `<->` display transform — is the
//!   in-shader COMPOSITION of the three stages above ([`aces2::emit_output_op`]): forward is
//!   `RGB->JMh(p_in)` → tonescale+chroma → gamut → `JMh->RGB(p_out)`, inverse the reverse with the
//!   two `RGB <-> JMh` ends swapping primaries. It re-emits no ACES2 maths and, crucially, no longer
//!   bakes: scene input brighter than `1.0` flows through the unbounded JMh domain instead of
//!   clamping at the old unit cube's far face, so HDR highlights match the verified CPU oracle.
//!
//! No ACES2 op remains on the (now-removed) LUT-bake path. All five op-directions plus the composed
//! output transform are guarded by `tests/aces2_gpu_emission.rs` (CPU==emit — including HDR
//! highlights well above 1.0 — naga parse, live wgpu).

use crate::OcioResult;
use crate::grading_bspline::{CurveUniformData, HUE_CURVE_COUNT, RGB_CURVE_COUNT};
use crate::processor::{Processor, ProcessorOp, TransferStyle};
use crate::transform::{
    AllocationType, CdlStyle, EcDynamic, ExposureContrastStyle, FixedFunctionStyle,
    GradingPrimaryStyle, NegativeStyle,
};
use vfx_math::layout::{Mat3, Mat4, RowMajor};

pub mod types;
pub use types::*;
// Bring private uniform helpers into scope for the GpuProcessor impl below.
use types::{GradingToneUniforms, OpUniforms, UniformRegistry};

// Per-method-group splits of the GpuProcessor impl. Each child module declares
// its own `impl super::GpuProcessor { ... }` block; they all compile together
// against the same struct definition below.
/// Shared ACES 2.0 `RGB <-> JMh` emission — the first ACES2 stage to leave the LUT-bake path.
mod aces2;
/// The published, data-form bind-group contract for the emitted WGSL (host API, PLAN9.md-style
/// SSOT with `runtime.rs`) — see its module docs. Always compiled (no `wgpu` dependency): a host
/// can inspect the contract without the `gpu` feature.
pub mod contract;
pub use contract::{
    GpuBindingContract, GpuBindingDesc, GpuBindingKind, OCIO_HOST_GROUP_START, OCIO_TEXTURE_GROUP,
    OCIO_UNIFORM_GROUP, WGSL_TRANSFORM_FUNCTION,
};
#[cfg(test)]
mod corpus;
mod dialect;
mod dispatch;
/// Shared PQ / gamma-log / double-log FixedFunction emission (all four languages).
mod ff_curves;
/// Shared ACES 1.3 gamut-compression emission (all four languages).
mod fixed_function;
/// Shared basic-gamma (`Exponent`) emission and the alpha terms of the ops that touch alpha.
mod gamma;
mod gamut_comp;
mod glsl;
mod grading;
mod hlsl;
mod hsy;
/// Shared camera-log (`LogCamera`) emission (all four languages).
mod log_camera;
mod lut1d_half;
mod lut3d;
mod metal;
/// Shared moncurve (`ExponentWithLinear`) emission: one body for all four languages.
mod moncurve;
mod ops;
/// Shared PQ (ST 2084) rationals: one encode and one decode body for every PQ op.
mod pq;
mod primary;
/// Shared, language-independent transfer-curve emission — see its module docs for why the four
/// backends no longer each carry their own copy of this math.
mod tone;
mod transfer;
mod wgsl;

/// GPU colour-processing RUNTIME (executes the emitted WGSL on the cluster's shared wgpu device).
/// Gated on the `gpu` feature so the default build pulls no wgpu; adopts `gpu_mem::shared_device()`
/// — see the module docs for the shared-device / single-wgpu rationale.
#[cfg(feature = "gpu")]
pub mod runtime;

/// Host-facing GPU resource builder (`@group(1)`/`@group(2)` bind groups) — the SSOT any host,
/// including this crate's own [`runtime::CompiledOcio`], uses to realize a [`GpuShaderCode`] on a
/// `wgpu::Device`. Gated on `gpu` like [`runtime`] (it needs `wgpu` types), but usable on a HOST'S
/// OWN device — it opens no [`gpu_mem`] shared device itself, unlike [`runtime`].
#[cfg(feature = "gpu")]
pub mod host;
#[cfg(feature = "gpu")]
pub use host::GpuOcioResources;

/// GPU processor for generating shader code.
#[derive(Debug, Clone)]
pub struct GpuProcessor {
    /// Operations to generate code for.
    ops: Vec<GpuOp>,
    /// LUT textures required by the shader.
    textures: Vec<GpuTexture>,
}

/// GPU-compatible operation.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Lut1D/Lut3D reserved for future LUT texture support
enum GpuOp {
    /// Matrix multiply, plus a per-channel offset.
    ///
    /// The matrix is a [`Mat4<RowMajor>`], not a bare `[f32; 16]`, on purpose: OCIO's sources
    /// and the CPU path (`processor/apply.rs:395-401`, `out[i] = sum_j in[j]*matrix[i*4+j]`,
    /// i.e. `M*v`) are row-major, while every shading language's matrix constructor wants
    /// columns. Carrying the convention in the *type* is what makes an emitter that forgets to
    /// transpose fail to compile rather than silently render a transposed image -- which is
    /// exactly what GLSL and HLSL did (OCIO-M6), and what the same class of bug did in two
    /// other crates. See `vfx_math::layout`.
    Matrix {
        matrix: Mat4<RowMajor>,
        offset: [f32; 4],
    },
    /// CDL (slope, offset, power, saturation, style).
    Cdl {
        slope: [f32; 3],
        offset: [f32; 3],
        power: [f32; 3],
        saturation: f32,
        style: CdlStyle,
    },
    /// Exponent/gamma.
    Exponent {
        value: [f32; 4],
        negative_style: NegativeStyle,
    },
    /// Log transform.
    Log { base: f32, forward: bool },
    /// Range/clamp.
    Range {
        scale: f32,
        offset: f32,
        clamp_min: Option<f32>,
        clamp_max: Option<f32>,
    },
    /// 1D LUT (texture index + domain).
    Lut1D {
        texture_idx: usize,
        /// Per-channel input domain (R, G, B), as the CPU apply uses it.
        domain_min: [f32; 3],
        domain_max: [f32; 3],
        hue_adjust: crate::transform::Lut1DHueAdjust,
    },
    /// Half-domain 1D LUT: 65536 entries indexed by the input's half code, in a padded 2D
    /// texture (`gpu/lut1d_half.rs`, OCIO `Lut1DOpGPU.cpp`).
    Lut1DHalf {
        texture_idx: usize,
        width: usize,
        height: usize,
    },
    /// 3D LUT (texture index + domain). `tetrahedral`: OCIO's shader tetrahedral interpolation
    /// over NEAREST fetches (`Tetrahedral`/`Best`), else the hardware trilinear filter
    /// (`gpu/lut3d.rs`, OCIO `Lut3DOpGPU.cpp`).
    Lut3D {
        texture_idx: usize,
        domain_min: [f32; 3],
        domain_max: [f32; 3],
        tetrahedral: bool,
    },
    /// LogAffine transform (OCIO v2).
    LogAffine {
        base: f32,
        log_side_slope: [f32; 3],
        log_side_offset: [f32; 3],
        lin_side_slope: [f32; 3],
        lin_side_offset: [f32; 3],
        forward: bool,
    },
    /// LogCamera transform (ARRI LogC, Sony S-Log3).
    LogCamera {
        base: f32,
        log_side_slope: [f32; 3],
        log_side_offset: [f32; 3],
        lin_side_slope: [f32; 3],
        lin_side_offset: [f32; 3],
        lin_side_break: [f32; 3],
        linear_slope: [f32; 3],
        /// Encoded-domain break, derived once at compile time (`ProcessorOp::LogCamera`).
        log_side_break: [f32; 3],
        /// Offset of the linear toe, derived once at compile time (`ProcessorOp::LogCamera`).
        linear_offset: [f32; 3],
        forward: bool,
    },
    /// ExponentWithLinear (sRGB/Rec.709 style): moncurve, LINEAR or (`mirror`) MIRROR negatives.
    ExponentWithLinear {
        gamma: [f32; 4],
        offset: [f32; 4],
        mirror: bool,
        forward: bool,
    },
    /// Transfer function (sRGB, PQ, HLG, LogC, etc).
    Transfer { style: TransferStyle, forward: bool },
    /// Exposure/Contrast adjustment (OCIO parity).
    ExposureContrast {
        exposure: f32,
        contrast: f32,
        gamma: f32,
        pivot: f32,
        log_exposure_step: f32,
        log_mid_gray: f32,
        style: ExposureContrastStyle,
        /// Which of the three the host may move at run time (see
        /// [`GpuProcessor::dynamic_uniforms`]). Every one is a uniform whatever this says; the
        /// flag only tells a host which uniform stands for which property.
        dynamic: EcDynamic,
        /// `exposure`/`contrast`/`gamma` (and their uniforms) are FORWARD values in both
        /// directions; `false` emits OCIO's reverse shader (`AddECLinearRevShader` ...).
        forward: bool,
    },
    /// Grading Primary (OCIO parity: Log/Linear/Video styles).
    GradingPrimary {
        style: GradingPrimaryStyle,
        brightness: [f32; 3],
        contrast: [f32; 3],
        gamma: [f32; 3],
        exposure: [f32; 3],
        offset: [f32; 3],
        slope: [f32; 3],
        saturation: f32,
        pivot: f32,
        pivot_black: f32,
        pivot_white: f32,
        clamp_black: f32,
        clamp_white: f32,
        forward: bool,
        /// OCIO's local bypass (`processor::grading_primary_local_bypass`).
        local_bypass: bool,
    },
    /// Grading Tone (OCIO parity with 5 zones + S-contrast).
    GradingTone {
        style: crate::transform::GradingToneStyle,
        blacks: [f32; 6],
        shadows: [f32; 6],
        midtones: [f32; 6],
        highlights: [f32; 6],
        whites: [f32; 6],
        scontrast: f32,
        forward: bool,
    },
    /// Fixed function ops (color space conversions, ACES ops).
    FixedFunction {
        style: FixedFunctionStyle,
        params: Vec<f32>,
        forward: bool,
    },
    /// Grading RGB Curve (OCIO B-spline curves).
    GradingRgbCurve {
        /// Grading style (Log/Linear/Video).
        style: crate::transform::GradingToneStyle,
        /// Curve uniform data for RGBCurve.
        curve_data: CurveUniformData<{ RGB_CURVE_COUNT * 2 }>,
        /// Bypass lin-to-log conversion for Linear style.
        bypass_lin_to_log: bool,
        /// Forward direction.
        forward: bool,
    },
    /// Allocation transform (Uniform or Log2).
    Allocation {
        /// Allocation type (Uniform or Log2).
        allocation: AllocationType,
        /// Min and max values.
        min_val: f32,
        max_val: f32,
        /// Linear offset for Log2 allocation.
        lin_offset: f32,
        /// Forward direction.
        forward: bool,
    },
    /// Grading Hue Curve (8 curves with HSY conversion).
    GradingHueCurve {
        /// Grading style (Log/Linear/Video) for HSY conversion.
        style: crate::grading_hue_curve::GradingStyle,
        /// RGB->HSY conversion style.
        rgb_to_hsy: crate::transform::HsyTransformStyle,
        /// Curve uniform data for HueCurve.
        curve_data: CurveUniformData<{ HUE_CURVE_COUNT * 2 }>,
        /// Draw-curve-only mode.
        draw_curve_only: bool,
        /// Forward direction.
        forward: bool,
    },
    /// ACES 2.0 `RGB <-> JMh` (CAM16), emitted as in-shader math over an unbounded domain.
    ///
    /// Unlike the other four ACES2 ops it does **not** bake a `[0,1]³` LUT (ACES2-03): JMh values
    /// leave the unit cube in every component, so a cube LUT is meaningless. The per-config
    /// `JMhParams` (matrices + `cz`/`inv_cz`, plus `f_l_n`/`a_w_j` reserved for the tonescale
    /// stage) travel to the shader as uniforms built in `build_uniforms`; the body comes from
    /// [`aces2::emit`].
    Aces2RgbJmh {
        /// CAM16 JMh parameters for the display primaries (holds the matrices and scalars uploaded
        /// as uniforms). Boxed to keep `GpuOp` small.
        params: Box<crate::aces2::JMhParams>,
        /// `true` = `RGB -> JMh`, `false` = `JMh -> RGB`.
        forward: bool,
    },
    /// ACES 2.0 Tonescale + Chroma compress, emitted as in-shader math over the unbounded JMh
    /// domain rather than a `[0,1]³` LUT (ACES2-03, stage 2).
    ///
    /// Like [`GpuOp::Aces2RgbJmh`] it does not bake a cube LUT: `J`/`M`/`h` leave the unit cube. The
    /// tonescale `J`-curve is closed form; the chroma compress samples the per-hue reach limit from
    /// the 363-entry `reach_m_table`, uploaded as an `ArrayFloat` uniform. All params reach the
    /// shader as uniforms built in `build_uniforms`; the body comes from [`aces2::emit_tonescale_op`].
    Aces2TonescaleCompress {
        /// CAM16 JMh parameters (the `J <-> Y` conversion scalars are read as uniforms). Boxed to
        /// keep `GpuOp` small.
        jmh_params: Box<crate::aces2::JMhParams>,
        /// Tonescale curve parameters.
        ts_params: Box<crate::aces2::ToneScaleParams>,
        /// Shared compression parameters (holds `limit_j_max`, `model_gamma_inv`, `reach_m_table`).
        shared: Box<crate::aces2::SharedCompressionParams>,
        /// Chroma compression parameters.
        chroma: Box<crate::aces2::ChromaCompressParams>,
        /// `true` = forward (`tonescale_compress_fwd`), `false` = inverse.
        forward: bool,
    },
    /// ACES 2.0 Gamut compress, emitted as in-shader math over the unbounded JMh domain rather than
    /// a `[0,1]³` LUT (ACES2-03, stage 3).
    ///
    /// Like the other in-shader ACES2 stages it does not bake a cube LUT: `J`/`M`/`h` leave the unit
    /// cube. The op is closed-form except for the per-hue cusp point, which is looked up from a
    /// non-uniform hue axis by the same binary search + lerp the CPU `init_hue_gamut_params` runs; the
    /// per-hue reach limit is the same 363-entry `reach_m_table` the tonescale stage samples. Both the
    /// cusp table (packed `[J, M, gamma_top_inv]` per hue) and the hue axis ride as `ArrayFloat`
    /// uniforms. All params reach the shader as uniforms built in `build_uniforms`; the body comes
    /// from [`aces2::emit_gamut_op`]. The inverse is the CPU's two-pass fixed-point refinement.
    Aces2GamutCompress {
        /// Shared compression parameters (holds `limit_j_max`, `model_gamma_inv`, `reach_m_table`).
        shared: Box<crate::aces2::SharedCompressionParams>,
        /// Gamut compression parameters (holds `mid_j`, `focus_dist`, `lower_hull_gamma_inv`, the
        /// hue-linearity search range, the hue axis and the cusp table).
        gamut: Box<crate::aces2::GamutCompressParams>,
        /// `true` = forward (`gamut_compress_fwd`), `false` = inverse.
        forward: bool,
    },
    /// ACES 2.0 Output Transform — the full scene-referred <-> display transform, emitted as the
    /// in-shader composition of the three JMh-domain stages (ACES2-03, stage 4).
    ///
    /// This is the LAST ACES2 op to leave the `[0,1]^3` LUT bake. Its forward chain is
    /// `RGB->JMh(p_in)` -> tonescale + chroma -> gamut -> `JMh->RGB(p_out)`; the inverse reverses it
    /// with the two `RGB <-> JMh` ends swapping primaries. The bake clamped every scene-linear input
    /// `> 1.0` into the unit cube (highlights/HDR crushed vs the verified CPU); the composition
    /// carries the unbounded domain end to end. The whole pre-computed [`crate::aces2::Aces2State`]
    /// travels as uniforms (both `JMhParams`, the tonescale/chroma/shared/gamut scalars, and the
    /// reach/hue/cusp tables — see `build_uniforms`); the body comes from
    /// [`aces2::emit_output_op`], which composes the verified stage-1..3 emitters — no ACES2 maths
    /// is re-emitted here. Boxed to keep `GpuOp` small.
    Aces2OutputTransform {
        /// Full pre-computed ACES 2.0 Output Transform state (`p_in`/`p_out` JMh params, tonescale,
        /// shared/chroma/gamut params — the source of every uploaded uniform).
        state: Box<crate::aces2::Aces2State>,
        /// `true` = forward (`output_transform_fwd`), `false` = inverse.
        forward: bool,
    },
}

impl GpuProcessor {
    /// Creates a GPU processor from a CPU processor.
    pub fn from_processor(processor: &Processor) -> OcioResult<Self> {
        let mut ops = Vec::new();
        let mut textures = Vec::new();

        // Convert processor ops to GPU ops. A `None` here means a whole operation would silently
        // vanish from the GPU render, so the pixels differ from the CPU by an entire op with no
        // signal — the same silent-identity hazard the strict `from_processor_wgsl` sibling
        // already rejects. Error with the op named instead of dropping it.
        for op in processor.ops().iter().flat_map(Self::gpu_sequence) {
            let gpu_op = Self::convert_op(op, &mut textures).ok_or_else(|| {
                crate::error::OcioError::Transform(format!(
                    "GPU conversion does not support processor op `{}`",
                    op.variant_name()
                ))
            })?;
            ops.push(gpu_op);
        }

        Ok(Self { ops, textures })
    }

    /// Creates a WGSL-safe GPU processor from a CPU processor.
    ///
    /// Native WGSL ops are preserved as-is. The current conversion path is
    /// strict: any non-native op in this path is returned as an error.
    pub fn from_processor_wgsl(processor: &Processor) -> OcioResult<Self> {
        let mut ops = Vec::new();
        let mut textures = Vec::new();

        for op in processor.ops().iter().flat_map(Self::gpu_sequence) {
            let mut local_textures = Vec::new();
            let mut gpu_op = Self::convert_op(op, &mut local_textures).ok_or_else(|| {
                crate::error::OcioError::Transform(format!(
                    "GPU conversion does not support processor op `{}`",
                    op.variant_name()
                ))
            })?;
            if !Self::is_wgsl_native_op(&gpu_op) {
                return Err(crate::error::OcioError::Transform(
                    "encountered non-native GPU op in WGSL path".to_string(),
                ));
            }
            let texture_base = textures.len();
            Self::rebase_texture_indices(&mut gpu_op, texture_base);
            textures.extend(local_textures);
            ops.push(gpu_op);
        }

        Ok(Self { ops, textures })
    }

    /// The ops `op` runs as on the GPU: an inverse CDL is its [`crate::processor::op::cdl_inverse_ops`]
    /// sequence (ops every emitter already has); any other op is itself.
    fn gpu_sequence(op: &ProcessorOp) -> Vec<&ProcessorOp> {
        match op {
            ProcessorOp::Cdl {
                inverse: Some(seq), ..
            } => seq.iter().collect(),
            _ => vec![op],
        }
    }

    #[inline]
    fn rebase_texture_indices(op: &mut GpuOp, texture_offset: usize) {
        match op {
            GpuOp::Lut1D { texture_idx, .. }
            | GpuOp::Lut1DHalf { texture_idx, .. }
            | GpuOp::Lut3D { texture_idx, .. } => {
                *texture_idx += texture_offset;
            }
            _ => {}
        }
    }

    #[inline]
    fn is_wgsl_native_op(op: &GpuOp) -> bool {
        matches!(
            op,
            GpuOp::Matrix { .. }
                | GpuOp::Cdl { .. }
                | GpuOp::Exponent { .. }
                | GpuOp::Log { .. }
                | GpuOp::Range { .. }
                | GpuOp::Lut1D { .. }
                | GpuOp::Lut1DHalf { .. }
                | GpuOp::Lut3D { .. }
                | GpuOp::Transfer { .. }
                | GpuOp::ExposureContrast { .. }
                | GpuOp::Allocation { .. }
                | GpuOp::LogAffine { .. }
                | GpuOp::LogCamera { .. }
                | GpuOp::ExponentWithLinear { .. }
                | GpuOp::GradingPrimary { .. }
                | GpuOp::GradingTone { .. }
                | GpuOp::GradingRgbCurve { .. }
                | GpuOp::GradingHueCurve { .. }
                | GpuOp::FixedFunction { .. }
                // All four ACES2 ops emit native WGSL in-shader over the unbounded JMh domain — each
                // uploads only uniforms (scalars + reach/hue/cusp `ArrayFloat`s built self-contained in
                // `build_uniforms`), never a baked `[0,1]^3` LUT. Their bodies come from the verified
                // `aces2::emit{,_tonescale_op,_gamut_op,_output_op}` emitters (the Output Transform
                // composes the other three), so the strict WGSL path represents any of them directly
                // with no generic-fallback bake. Each is reachable standalone via `convert_op`.
                | GpuOp::Aces2RgbJmh { .. }
                | GpuOp::Aces2TonescaleCompress { .. }
                | GpuOp::Aces2GamutCompress { .. }
                | GpuOp::Aces2OutputTransform { .. }
        )
    }

    fn uniform_name(op_index: usize, field: &str) -> String {
        format!("ocio_u{}_{}", op_index, field)
    }

    /// The shader uniforms that carry a dynamic property: one per op that declared `prop`
    /// dynamic ([`crate::EcDynamic`]), in op order. Move them with
    /// `GpuShaderCode::set_uniform_value` / the runtime executors'
    /// `set_uniform_value` (and `GpuOcioResources::update_scalars` on a host's own device)
    /// instead of rebuilding the shader. The uniform holds the FORWARD value whatever the op's
    /// direction (an inverse op's shader inverts it), the same number
    /// `Processor::set_dynamic` takes. The names are `ocio_u<op>_exposure` and so on; an
    /// op that is not dynamic for `prop` has the same uniform but is not listed, so a
    /// pipeline's display-side gamma cannot be mistaken for its exposure dial.
    ///
    /// Only Exposure, Contrast and Gamma are scalar properties of this crate's ops; any other
    /// property gives an empty list.
    pub fn dynamic_uniforms(&self, prop: crate::DynamicPropertyType) -> Vec<String> {
        use crate::DynamicPropertyType as P;

        let field = match prop {
            P::Exposure => "exposure",
            P::Contrast => "contrast",
            P::Gamma => "gamma",
            _ => return Vec::new(),
        };
        self.ops
            .iter()
            .enumerate()
            .filter(|(_, op)| match (op, prop) {
                (GpuOp::ExposureContrast { dynamic, .. }, P::Exposure) => dynamic.exposure,
                (GpuOp::ExposureContrast { dynamic, .. }, P::Contrast) => dynamic.contrast,
                (GpuOp::ExposureContrast { dynamic, .. }, P::Gamma) => dynamic.gamma,
                _ => false,
            })
            .map(|(i, _)| Self::uniform_name(i, field))
            .collect()
    }

    /// Registers the six `RGB <-> JMh` (CAM16) uniforms for one `JMhParams` and returns the
    /// [`OpUniforms::Aces2Jmh`] naming them.
    ///
    /// The four CAM16 matrices upload column-major so `mat3(col0, col1, col2)` reconstructs `M` for
    /// the `M * v` / `mul(M, v)` the emitter writes. The transpose is spelled by the type, not by
    /// hand: `aces2::M33` is a row-major `[f32; 9]`, wrapped as `Mat3<RowMajor>` and converted via the
    /// named, compiler-checked `to_col_major()` (see `vfx_math::layout`, which exists because this
    /// exact transpose shipped wrong three times as a bare hand-rolled helper). Factored out so the
    /// standalone `Aces2RgbJmh` arm and BOTH ends of the composed `Aces2OutputTransform` (which needs
    /// the input primaries at the scene end and the output primaries at the display end) share one
    /// implementation of that conversion rather than three copies. `field_prefix` disambiguates the
    /// uniform names (`"jmh_"` for the standalone op, `"o_in_"` / `"o_out_"` for the two output ends).
    fn register_jmh_uniforms(
        registry: &mut UniformRegistry,
        idx: usize,
        field_prefix: &str,
        params: &crate::aces2::JMhParams,
    ) -> OpUniforms {
        let mut add_mat = |field: &str, m: &crate::aces2::M33| {
            let name = Self::uniform_name(idx, field);
            let col_major_upload = Mat3::<RowMajor>::from_array(*m).to_col_major().to_array();
            registry.add(
                name.clone(),
                GpuUniformType::Mat3,
                GpuUniformValue::Mat3(col_major_upload),
                None,
            );
            name
        };
        let mat_rgb_to_cam16_c = add_mat(
            &format!("{field_prefix}mat_rgb_cam16"),
            &params.mat_rgb_to_cam16_c,
        );
        let mat_cam16_c_to_rgb = add_mat(
            &format!("{field_prefix}mat_cam16_rgb"),
            &params.mat_cam16_c_to_rgb,
        );
        let mat_cone_to_aab = add_mat(
            &format!("{field_prefix}mat_cone_aab"),
            &params.mat_cone_to_aab,
        );
        let mat_aab_to_cone = add_mat(
            &format!("{field_prefix}mat_aab_cone"),
            &params.mat_aab_to_cone,
        );

        let mut add_f = |field: &str, v: f32| {
            let name = Self::uniform_name(idx, field);
            registry.add(
                name.clone(),
                GpuUniformType::Float,
                GpuUniformValue::Float(v),
                None,
            );
            name
        };
        let cz = add_f(&format!("{field_prefix}cz"), params.cz);
        let inv_cz = add_f(&format!("{field_prefix}inv_cz"), params.inv_cz);

        OpUniforms::Aces2Jmh {
            mat_rgb_to_cam16_c,
            mat_cam16_c_to_rgb,
            mat_cone_to_aab,
            mat_aab_to_cone,
            cz,
            inv_cz,
        }
    }

    fn build_uniforms(&self) -> (Vec<Option<OpUniforms>>, Vec<GpuUniform>) {
        let mut registry = UniformRegistry::new();
        let mut bindings: Vec<Option<OpUniforms>> = Vec::with_capacity(self.ops.len());

        for (idx, op) in self.ops.iter().enumerate() {
            let binding = match op {
                GpuOp::ExposureContrast {
                    exposure,
                    contrast,
                    gamma,
                    ..
                } => {
                    let exposure_name = Self::uniform_name(idx, "exposure");
                    let contrast_name = Self::uniform_name(idx, "contrast");
                    let gamma_name = Self::uniform_name(idx, "gamma");
                    registry.add(
                        exposure_name.clone(),
                        GpuUniformType::Float,
                        GpuUniformValue::Float(*exposure),
                        None,
                    );
                    registry.add(
                        contrast_name.clone(),
                        GpuUniformType::Float,
                        GpuUniformValue::Float(*contrast),
                        None,
                    );
                    registry.add(
                        gamma_name.clone(),
                        GpuUniformType::Float,
                        GpuUniformValue::Float(*gamma),
                        None,
                    );
                    Some(OpUniforms::ExposureContrast {
                        exposure: exposure_name,
                        contrast: contrast_name,
                        gamma: gamma_name,
                    })
                }
                GpuOp::GradingPrimary {
                    brightness,
                    contrast,
                    gamma,
                    exposure,
                    offset,
                    slope,
                    saturation,
                    pivot,
                    pivot_black,
                    pivot_white,
                    clamp_black,
                    clamp_white,
                    local_bypass,
                    ..
                } => {
                    let brightness_name = Self::uniform_name(idx, "brightness");
                    let contrast_name = Self::uniform_name(idx, "contrast");
                    let gamma_name = Self::uniform_name(idx, "gamma");
                    let exposure_name = Self::uniform_name(idx, "exposure");
                    let offset_name = Self::uniform_name(idx, "offset");
                    let slope_name = Self::uniform_name(idx, "slope");
                    let saturation_name = Self::uniform_name(idx, "saturation");
                    let pivot_name = Self::uniform_name(idx, "pivot");
                    let pivot_black_name = Self::uniform_name(idx, "pivot_black");
                    let pivot_white_name = Self::uniform_name(idx, "pivot_white");
                    let clamp_black_name = Self::uniform_name(idx, "clamp_black");
                    let clamp_white_name = Self::uniform_name(idx, "clamp_white");
                    let local_bypass_name = Self::uniform_name(idx, "local_bypass");

                    registry.add(
                        brightness_name.clone(),
                        GpuUniformType::Vec3,
                        GpuUniformValue::Vec3(*brightness),
                        None,
                    );
                    registry.add(
                        contrast_name.clone(),
                        GpuUniformType::Vec3,
                        GpuUniformValue::Vec3(*contrast),
                        None,
                    );
                    registry.add(
                        gamma_name.clone(),
                        GpuUniformType::Vec3,
                        GpuUniformValue::Vec3(*gamma),
                        None,
                    );
                    registry.add(
                        exposure_name.clone(),
                        GpuUniformType::Vec3,
                        GpuUniformValue::Vec3(*exposure),
                        None,
                    );
                    registry.add(
                        offset_name.clone(),
                        GpuUniformType::Vec3,
                        GpuUniformValue::Vec3(*offset),
                        None,
                    );
                    registry.add(
                        slope_name.clone(),
                        GpuUniformType::Vec3,
                        GpuUniformValue::Vec3(*slope),
                        None,
                    );
                    registry.add(
                        saturation_name.clone(),
                        GpuUniformType::Float,
                        GpuUniformValue::Float(*saturation),
                        None,
                    );
                    registry.add(
                        pivot_name.clone(),
                        GpuUniformType::Float,
                        GpuUniformValue::Float(*pivot),
                        None,
                    );
                    registry.add(
                        pivot_black_name.clone(),
                        GpuUniformType::Float,
                        GpuUniformValue::Float(*pivot_black),
                        None,
                    );
                    registry.add(
                        pivot_white_name.clone(),
                        GpuUniformType::Float,
                        GpuUniformValue::Float(*pivot_white),
                        None,
                    );
                    registry.add(
                        clamp_black_name.clone(),
                        GpuUniformType::Float,
                        GpuUniformValue::Float(*clamp_black),
                        None,
                    );
                    registry.add(
                        clamp_white_name.clone(),
                        GpuUniformType::Float,
                        GpuUniformValue::Float(*clamp_white),
                        None,
                    );
                    registry.add(
                        local_bypass_name.clone(),
                        GpuUniformType::Bool,
                        GpuUniformValue::Bool(*local_bypass),
                        None,
                    );

                    Some(OpUniforms::GradingPrimary {
                        brightness: brightness_name,
                        contrast: contrast_name,
                        gamma: gamma_name,
                        exposure: exposure_name,
                        offset: offset_name,
                        slope: slope_name,
                        saturation: saturation_name,
                        pivot: pivot_name,
                        pivot_black: pivot_black_name,
                        pivot_white: pivot_white_name,
                        clamp_black: clamp_black_name,
                        clamp_white: clamp_white_name,
                        local_bypass: local_bypass_name,
                    })
                }
                GpuOp::GradingTone {
                    blacks,
                    shadows,
                    midtones,
                    highlights,
                    whites,
                    scontrast,
                    ..
                } => {
                    let blacks_r = Self::uniform_name(idx, "blacks_r");
                    let blacks_g = Self::uniform_name(idx, "blacks_g");
                    let blacks_b = Self::uniform_name(idx, "blacks_b");
                    let blacks_m = Self::uniform_name(idx, "blacks_m");
                    let blacks_start = Self::uniform_name(idx, "blacks_start");
                    let blacks_width = Self::uniform_name(idx, "blacks_width");

                    let shadows_r = Self::uniform_name(idx, "shadows_r");
                    let shadows_g = Self::uniform_name(idx, "shadows_g");
                    let shadows_b = Self::uniform_name(idx, "shadows_b");
                    let shadows_m = Self::uniform_name(idx, "shadows_m");
                    let shadows_start = Self::uniform_name(idx, "shadows_start");
                    let shadows_width = Self::uniform_name(idx, "shadows_width");

                    let midtones_r = Self::uniform_name(idx, "midtones_r");
                    let midtones_g = Self::uniform_name(idx, "midtones_g");
                    let midtones_b = Self::uniform_name(idx, "midtones_b");
                    let midtones_m = Self::uniform_name(idx, "midtones_m");
                    let midtones_start = Self::uniform_name(idx, "midtones_start");
                    let midtones_width = Self::uniform_name(idx, "midtones_width");

                    let highlights_r = Self::uniform_name(idx, "highlights_r");
                    let highlights_g = Self::uniform_name(idx, "highlights_g");
                    let highlights_b = Self::uniform_name(idx, "highlights_b");
                    let highlights_m = Self::uniform_name(idx, "highlights_m");
                    let highlights_start = Self::uniform_name(idx, "highlights_start");
                    let highlights_width = Self::uniform_name(idx, "highlights_width");

                    let whites_r = Self::uniform_name(idx, "whites_r");
                    let whites_g = Self::uniform_name(idx, "whites_g");
                    let whites_b = Self::uniform_name(idx, "whites_b");
                    let whites_m = Self::uniform_name(idx, "whites_m");
                    let whites_start = Self::uniform_name(idx, "whites_start");
                    let whites_width = Self::uniform_name(idx, "whites_width");

                    let scontrast_name = Self::uniform_name(idx, "scontrast");
                    let local_bypass_name = Self::uniform_name(idx, "local_bypass");

                    let add_float = |registry: &mut UniformRegistry, name: &String, value: f32| {
                        registry.add(
                            name.clone(),
                            GpuUniformType::Float,
                            GpuUniformValue::Float(value),
                            None,
                        );
                    };

                    add_float(&mut registry, &blacks_r, blacks[0]);
                    add_float(&mut registry, &blacks_g, blacks[1]);
                    add_float(&mut registry, &blacks_b, blacks[2]);
                    add_float(&mut registry, &blacks_m, blacks[3]);
                    add_float(&mut registry, &blacks_start, blacks[4]);
                    add_float(&mut registry, &blacks_width, blacks[5]);

                    add_float(&mut registry, &shadows_r, shadows[0]);
                    add_float(&mut registry, &shadows_g, shadows[1]);
                    add_float(&mut registry, &shadows_b, shadows[2]);
                    add_float(&mut registry, &shadows_m, shadows[3]);
                    add_float(&mut registry, &shadows_start, shadows[4]);
                    add_float(&mut registry, &shadows_width, shadows[5]);

                    add_float(&mut registry, &midtones_r, midtones[0]);
                    add_float(&mut registry, &midtones_g, midtones[1]);
                    add_float(&mut registry, &midtones_b, midtones[2]);
                    add_float(&mut registry, &midtones_m, midtones[3]);
                    add_float(&mut registry, &midtones_start, midtones[4]);
                    add_float(&mut registry, &midtones_width, midtones[5]);

                    add_float(&mut registry, &highlights_r, highlights[0]);
                    add_float(&mut registry, &highlights_g, highlights[1]);
                    add_float(&mut registry, &highlights_b, highlights[2]);
                    add_float(&mut registry, &highlights_m, highlights[3]);
                    add_float(&mut registry, &highlights_start, highlights[4]);
                    add_float(&mut registry, &highlights_width, highlights[5]);

                    add_float(&mut registry, &whites_r, whites[0]);
                    add_float(&mut registry, &whites_g, whites[1]);
                    add_float(&mut registry, &whites_b, whites[2]);
                    add_float(&mut registry, &whites_m, whites[3]);
                    add_float(&mut registry, &whites_start, whites[4]);
                    add_float(&mut registry, &whites_width, whites[5]);

                    add_float(&mut registry, &scontrast_name, *scontrast);

                    let local_bypass = blacks[0..4].iter().all(|v| (*v - 1.0).abs() < 1e-6)
                        && shadows[0..4].iter().all(|v| (*v - 1.0).abs() < 1e-6)
                        && midtones[0..4].iter().all(|v| (*v - 1.0).abs() < 1e-6)
                        && highlights[0..4].iter().all(|v| (*v - 1.0).abs() < 1e-6)
                        && whites[0..4].iter().all(|v| (*v - 1.0).abs() < 1e-6)
                        && (*scontrast - 1.0).abs() < 1e-6;
                    registry.add(
                        local_bypass_name.clone(),
                        GpuUniformType::Bool,
                        GpuUniformValue::Bool(local_bypass),
                        None,
                    );

                    Some(OpUniforms::GradingTone(Box::new(GradingToneUniforms {
                        blacks_r,
                        blacks_g,
                        blacks_b,
                        blacks_m,
                        blacks_start,
                        blacks_width,
                        shadows_r,
                        shadows_g,
                        shadows_b,
                        shadows_m,
                        shadows_start,
                        shadows_width,
                        midtones_r,
                        midtones_g,
                        midtones_b,
                        midtones_m,
                        midtones_start,
                        midtones_width,
                        highlights_r,
                        highlights_g,
                        highlights_b,
                        highlights_m,
                        highlights_start,
                        highlights_width,
                        whites_r,
                        whites_g,
                        whites_b,
                        whites_m,
                        whites_start,
                        whites_width,
                        scontrast: scontrast_name,
                        local_bypass: local_bypass_name,
                    })))
                }
                GpuOp::GradingRgbCurve { curve_data, .. } => {
                    let knots_offsets = Self::uniform_name(idx, "knots_offsets");
                    let knots = Self::uniform_name(idx, "knots");
                    let coefs_offsets = Self::uniform_name(idx, "coefs_offsets");
                    let coefs = Self::uniform_name(idx, "coefs");
                    let local_bypass = Self::uniform_name(idx, "local_bypass");

                    let target_offsets_len = HUE_CURVE_COUNT * 2;
                    let mut knots_offsets_vec = curve_data.knots_offsets.to_vec();
                    let mut coefs_offsets_vec = curve_data.coefs_offsets.to_vec();
                    while knots_offsets_vec.len() < target_offsets_len {
                        knots_offsets_vec.push(-1);
                        knots_offsets_vec.push(0);
                    }
                    while coefs_offsets_vec.len() < target_offsets_len {
                        coefs_offsets_vec.push(-1);
                        coefs_offsets_vec.push(0);
                    }

                    registry.add(
                        knots_offsets.clone(),
                        GpuUniformType::ArrayInt,
                        GpuUniformValue::ArrayInt(knots_offsets_vec),
                        Some(target_offsets_len),
                    );
                    registry.add(
                        knots.clone(),
                        GpuUniformType::ArrayFloat,
                        GpuUniformValue::ArrayFloat(curve_data.knots.clone()),
                        Some(crate::grading_bspline::MAX_NUM_KNOTS),
                    );
                    registry.add(
                        coefs_offsets.clone(),
                        GpuUniformType::ArrayInt,
                        GpuUniformValue::ArrayInt(coefs_offsets_vec),
                        Some(target_offsets_len),
                    );
                    registry.add(
                        coefs.clone(),
                        GpuUniformType::ArrayFloat,
                        GpuUniformValue::ArrayFloat(curve_data.coefs.clone()),
                        Some(crate::grading_bspline::MAX_NUM_COEFS),
                    );
                    registry.add(
                        local_bypass.clone(),
                        GpuUniformType::Bool,
                        GpuUniformValue::Bool(curve_data.local_bypass),
                        None,
                    );

                    Some(OpUniforms::GradingRgbCurve {
                        knots_offsets,
                        knots,
                        coefs_offsets,
                        coefs,
                        local_bypass,
                    })
                }
                GpuOp::GradingHueCurve { curve_data, .. } => {
                    let knots_offsets = Self::uniform_name(idx, "knots_offsets");
                    let knots = Self::uniform_name(idx, "knots");
                    let coefs_offsets = Self::uniform_name(idx, "coefs_offsets");
                    let coefs = Self::uniform_name(idx, "coefs");
                    let local_bypass = Self::uniform_name(idx, "local_bypass");

                    registry.add(
                        knots_offsets.clone(),
                        GpuUniformType::ArrayInt,
                        GpuUniformValue::ArrayInt(curve_data.knots_offsets.to_vec()),
                        Some(HUE_CURVE_COUNT * 2),
                    );
                    registry.add(
                        knots.clone(),
                        GpuUniformType::ArrayFloat,
                        GpuUniformValue::ArrayFloat(curve_data.knots.clone()),
                        Some(crate::grading_bspline::MAX_NUM_KNOTS),
                    );
                    registry.add(
                        coefs_offsets.clone(),
                        GpuUniformType::ArrayInt,
                        GpuUniformValue::ArrayInt(curve_data.coefs_offsets.to_vec()),
                        Some(HUE_CURVE_COUNT * 2),
                    );
                    registry.add(
                        coefs.clone(),
                        GpuUniformType::ArrayFloat,
                        GpuUniformValue::ArrayFloat(curve_data.coefs.clone()),
                        Some(crate::grading_bspline::MAX_NUM_COEFS),
                    );
                    registry.add(
                        local_bypass.clone(),
                        GpuUniformType::Bool,
                        GpuUniformValue::Bool(curve_data.local_bypass),
                        None,
                    );

                    Some(OpUniforms::GradingHueCurve {
                        knots_offsets,
                        knots,
                        coefs_offsets,
                        coefs,
                        local_bypass,
                    })
                }
                GpuOp::Aces2RgbJmh { params, .. } => {
                    // Only the fields `RGB <-> JMh` reads are registered (the four matrices +
                    // `cz`/`inv_cz`, uploaded via the shared `register_jmh_uniforms` — see there for
                    // the column-major transpose discipline). The tonescale/gamut stages add
                    // `f_l_n`/`a_w_j` when they consume them, rather than registering dead uniforms.
                    Some(Self::register_jmh_uniforms(
                        &mut registry,
                        idx,
                        "jmh_",
                        params,
                    ))
                }
                GpuOp::Aces2TonescaleCompress {
                    jmh_params,
                    ts_params,
                    shared,
                    chroma,
                    ..
                } => {
                    // Every scalar is copied straight from the precomputed CPU `aces2` params, never
                    // recomputed, so the emitted shader and the verified CPU oracle can never drift.
                    let mut add_f = |field: &str, v: f32| {
                        let name = Self::uniform_name(idx, field);
                        registry.add(
                            name.clone(),
                            GpuUniformType::Float,
                            GpuUniformValue::Float(v),
                            None,
                        );
                        name
                    };
                    let a_w_j = add_f("ts_a_w_j", jmh_params.a_w_j);
                    let inv_a_w_j = add_f("ts_inv_a_w_j", jmh_params.inv_a_w_j);
                    let f_l_n = add_f("ts_f_l_n", jmh_params.f_l_n);
                    let cz = add_f("ts_cz", jmh_params.cz);
                    let inv_cz = add_f("ts_inv_cz", jmh_params.inv_cz);
                    let n_r = add_f("ts_n_r", ts_params.n_r);
                    let s_2 = add_f("ts_s_2", ts_params.s_2);
                    let m_2 = add_f("ts_m_2", ts_params.m_2);
                    let g = add_f("ts_g", ts_params.g);
                    let t_1 = add_f("ts_t_1", ts_params.t_1);
                    let inverse_limit = add_f("ts_inverse_limit", ts_params.inverse_limit);
                    let limit_j_max = add_f("ts_limit_j_max", shared.limit_j_max);
                    let model_gamma_inv = add_f("ts_model_gamma_inv", shared.model_gamma_inv);
                    let sat = add_f("ts_sat", chroma.sat);
                    let sat_thr = add_f("ts_sat_thr", chroma.sat_thr);
                    let compr = add_f("ts_compr", chroma.compr);
                    let ccs = add_f("ts_ccs", chroma.chroma_compress_scale);

                    // Reach-M table: the 363-entry per-hue limit `chroma_compress` samples. Uploaded
                    // as an `ArrayFloat` uniform (the channel knots/coefs already use) rather than a
                    // texture, so no new GPU texture plumbing is needed and the reach lookup rides the
                    // same storage buffer for WGSL / named `float[]` for the C-family.
                    let reach_data = shared.reach_m_table.data.to_vec();
                    let reach_len = reach_data.len();
                    let reach = Self::uniform_name(idx, "reach_m_table");
                    registry.add(
                        reach.clone(),
                        GpuUniformType::ArrayFloat,
                        GpuUniformValue::ArrayFloat(reach_data),
                        Some(reach_len),
                    );

                    Some(OpUniforms::Aces2Tonescale {
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
                    })
                }
                GpuOp::Aces2GamutCompress { shared, gamut, .. } => {
                    // Every scalar is copied straight from the precomputed CPU `aces2` params, never
                    // recomputed, so the emitted shader and the verified CPU oracle can never drift.
                    let mut add_f = |field: &str, v: f32| {
                        let name = Self::uniform_name(idx, field);
                        registry.add(
                            name.clone(),
                            GpuUniformType::Float,
                            GpuUniformValue::Float(v),
                            None,
                        );
                        name
                    };
                    let limit_j_max = add_f("gc_limit_j_max", shared.limit_j_max);
                    let model_gamma_inv = add_f("gc_model_gamma_inv", shared.model_gamma_inv);
                    let mid_j = add_f("gc_mid_j", gamut.mid_j);
                    let focus_dist = add_f("gc_focus_dist", gamut.focus_dist);
                    let lower_hull_gamma_inv =
                        add_f("gc_lower_hull_gamma_inv", gamut.lower_hull_gamma_inv);
                    // The hue-linearity search bounds are `i32`, but small enough (a few slots) to be
                    // exactly representable as `f32`; uploaded as `Float` and read back with `int(..)`
                    // in the shader, avoiding a second `ArrayInt` binding for two scalars.
                    let search_lo =
                        add_f("gc_search_lo", gamut.hue_linearity_search_range[0] as f32);
                    let search_hi =
                        add_f("gc_search_hi", gamut.hue_linearity_search_range[1] as f32);

                    // Three `ArrayFloat` uniforms, all sampled per hue (see `aces2::emit_gamut_op`):
                    //   * reach-M — reused from the tonescale stage, sampled by `emit_reach_sample`,
                    //   * hue axis — the non-uniform hue table the cusp binary search walks,
                    //   * cusp table — `[J, M, gamma_top_inv]` per hue, flattened 3-per-entry so the
                    //     storage array carries the `Table3D` the reference transports as an RGB texture.
                    let mut add_arr = |field: &str, data: Vec<f32>| {
                        let len = data.len();
                        let name = Self::uniform_name(idx, field);
                        registry.add(
                            name.clone(),
                            GpuUniformType::ArrayFloat,
                            GpuUniformValue::ArrayFloat(data),
                            Some(len),
                        );
                        name
                    };
                    let reach = add_arr("gc_reach_m_table", shared.reach_m_table.data.to_vec());
                    let hue_table = add_arr("gc_hue_table", gamut.hue_table.data.to_vec());
                    let cusp_flat: Vec<f32> = gamut
                        .gamut_cusp_table
                        .data
                        .iter()
                        .flat_map(|e| e.iter().copied())
                        .collect();
                    let cusp_table = add_arr("gc_cusp_table", cusp_flat);

                    Some(OpUniforms::Aces2Gamut {
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
                    })
                }
                GpuOp::Aces2OutputTransform { state, .. } => {
                    // The union of the three JMh-domain stages' uniforms, sourced straight from the
                    // precomputed CPU `Aces2State` (never retyped) so the emitted shader and the
                    // verified CPU oracle cannot drift. Names are grouped: `o_in_`/`o_out_` for the
                    // two `RGB <-> JMh` primary sets, `o_ts_` for the tonescale/chroma scalars,
                    // `o_gc_` for the gamut scalars, and unprefixed `o_*` for the terms READ BY BOTH
                    // the tonescale and gamut stages (`limit_j_max`, `model_gamma_inv`, and the
                    // 363-entry reach-`M` table) — registered ONCE and shared by name so the reach
                    // table is uploaded a single time.
                    let st = state.as_ref();

                    // Both `RGB <-> JMh` primary sets via the shared helper (the column-major matrix
                    // upload lives in one place). `p_in` at the scene end, `p_out` at the display end.
                    let jmh_in = Self::register_jmh_uniforms(&mut registry, idx, "o_in_", &st.p_in);
                    let jmh_out =
                        Self::register_jmh_uniforms(&mut registry, idx, "o_out_", &st.p_out);

                    let mut add_f = |field: &str, v: f32| {
                        let name = Self::uniform_name(idx, field);
                        registry.add(
                            name.clone(),
                            GpuUniformType::Float,
                            GpuUniformValue::Float(v),
                            None,
                        );
                        name
                    };

                    // Shared compression scalars — read by BOTH tonescale and gamut, registered once.
                    let limit_j_max = add_f("o_limit_j_max", st.shared.limit_j_max);
                    let model_gamma_inv = add_f("o_model_gamma_inv", st.shared.model_gamma_inv);

                    // Tonescale stage: the `J <-> Y` conversion terms come from `p_in` (`cz`/`inv_cz`
                    // duplicate the `o_in_` scalars, but keeping the tonescale ref self-contained is
                    // clearer than threading a shared name for two floats), the `ToneScaleParams`
                    // curve, and the `ChromaCompressParams`.
                    let ts_a_w_j = add_f("o_ts_a_w_j", st.p_in.a_w_j);
                    let ts_inv_a_w_j = add_f("o_ts_inv_a_w_j", st.p_in.inv_a_w_j);
                    let ts_f_l_n = add_f("o_ts_f_l_n", st.p_in.f_l_n);
                    let ts_cz = add_f("o_ts_cz", st.p_in.cz);
                    let ts_inv_cz = add_f("o_ts_inv_cz", st.p_in.inv_cz);
                    let ts_n_r = add_f("o_ts_n_r", st.ts.n_r);
                    let ts_s_2 = add_f("o_ts_s_2", st.ts.s_2);
                    let ts_m_2 = add_f("o_ts_m_2", st.ts.m_2);
                    let ts_g = add_f("o_ts_g", st.ts.g);
                    let ts_t_1 = add_f("o_ts_t_1", st.ts.t_1);
                    let ts_inverse_limit = add_f("o_ts_inverse_limit", st.ts.inverse_limit);
                    let ts_sat = add_f("o_ts_sat", st.chroma.sat);
                    let ts_sat_thr = add_f("o_ts_sat_thr", st.chroma.sat_thr);
                    let ts_compr = add_f("o_ts_compr", st.chroma.compr);
                    let ts_ccs = add_f("o_ts_ccs", st.chroma.chroma_compress_scale);

                    // Gamut stage scalars.
                    let gc_mid_j = add_f("o_gc_mid_j", st.gamut.mid_j);
                    let gc_focus_dist = add_f("o_gc_focus_dist", st.gamut.focus_dist);
                    let gc_lower_hull_gamma_inv =
                        add_f("o_gc_lower_hull_gamma_inv", st.gamut.lower_hull_gamma_inv);
                    let gc_search_lo = add_f(
                        "o_gc_search_lo",
                        st.gamut.hue_linearity_search_range[0] as f32,
                    );
                    let gc_search_hi = add_f(
                        "o_gc_search_hi",
                        st.gamut.hue_linearity_search_range[1] as f32,
                    );

                    // Tables: reach shared by tonescale and gamut (uploaded ONCE), plus the gamut's
                    // non-uniform hue axis and packed `[J, M, gamma_top_inv]` cusp table.
                    let mut add_arr = |field: &str, data: Vec<f32>| {
                        let len = data.len();
                        let name = Self::uniform_name(idx, field);
                        registry.add(
                            name.clone(),
                            GpuUniformType::ArrayFloat,
                            GpuUniformValue::ArrayFloat(data),
                            Some(len),
                        );
                        name
                    };
                    let reach = add_arr("o_reach_m_table", st.shared.reach_m_table.data.to_vec());
                    let hue_table = add_arr("o_hue_table", st.gamut.hue_table.data.to_vec());
                    let cusp_flat: Vec<f32> = st
                        .gamut
                        .gamut_cusp_table
                        .data
                        .iter()
                        .flat_map(|e| e.iter().copied())
                        .collect();
                    let cusp_table = add_arr("o_cusp_table", cusp_flat);

                    let tonescale = OpUniforms::Aces2Tonescale {
                        a_w_j: ts_a_w_j,
                        inv_a_w_j: ts_inv_a_w_j,
                        f_l_n: ts_f_l_n,
                        cz: ts_cz,
                        inv_cz: ts_inv_cz,
                        n_r: ts_n_r,
                        s_2: ts_s_2,
                        m_2: ts_m_2,
                        g: ts_g,
                        t_1: ts_t_1,
                        inverse_limit: ts_inverse_limit,
                        limit_j_max: limit_j_max.clone(),
                        model_gamma_inv: model_gamma_inv.clone(),
                        sat: ts_sat,
                        sat_thr: ts_sat_thr,
                        compr: ts_compr,
                        ccs: ts_ccs,
                        reach: reach.clone(),
                    };
                    let gamut = OpUniforms::Aces2Gamut {
                        limit_j_max,
                        model_gamma_inv,
                        mid_j: gc_mid_j,
                        focus_dist: gc_focus_dist,
                        lower_hull_gamma_inv: gc_lower_hull_gamma_inv,
                        search_lo: gc_search_lo,
                        search_hi: gc_search_hi,
                        reach,
                        hue_table,
                        cusp_table,
                    };

                    Some(OpUniforms::Aces2Output {
                        jmh_in: Box::new(jmh_in),
                        jmh_out: Box::new(jmh_out),
                        tonescale: Box::new(tonescale),
                        gamut: Box::new(gamut),
                    })
                }
                _ => None,
            };

            bindings.push(binding);
        }

        (bindings, registry.uniforms)
    }

    /// Converts a processor op to a GPU op.
    fn convert_op(op: &ProcessorOp, textures: &mut Vec<GpuTexture>) -> Option<GpuOp> {
        // An exact inverse LUT cannot run per pixel on the GPU: bake it, as OCIO's GPU path does.
        if let Some(baked) = op.gpu_baked_inverse() {
            return Self::convert_op(&baked, textures);
        }
        match op {
            ProcessorOp::Matrix { matrix, offset } => Some(GpuOp::Matrix {
                // `ProcessorOp::Matrix` carries a bare row-major `[f32; 16]`; tag it here, at
                // the one place it enters the GPU path, so no emitter downstream sees an
                // untyped array.
                matrix: Mat4::<RowMajor>::from_array(*matrix),
                offset: *offset,
            }),
            ProcessorOp::Cdl {
                slope,
                offset,
                power,
                saturation,
                style,
                inverse: None,
            } => Some(GpuOp::Cdl {
                slope: *slope,
                offset: *offset,
                power: *power,
                saturation: *saturation,
                style: *style,
            }),
            // Expanded into its op sequence by `gpu_sequence` before conversion.
            ProcessorOp::Cdl { inverse: Some(_), .. } => None,
            ProcessorOp::Exponent {
                value,
                negative_style,
            } => Some(GpuOp::Exponent {
                value: *value,
                negative_style: *negative_style,
            }),
            ProcessorOp::Log { base, forward } => Some(GpuOp::Log {
                base: *base,
                forward: *forward,
            }),
            ProcessorOp::Range {
                scale,
                offset,
                clamp_min,
                clamp_max,
            } => Some(GpuOp::Range {
                scale: *scale,
                offset: *offset,
                clamp_min: *clamp_min,
                clamp_max: *clamp_max,
            }),
            // A half-domain LUT is indexed by the input's 16-bit half code, which the shader
            // rebuilds over a padded 2D texture (OCIO `Lut1DOpGPU.cpp:237-276`,
            // `gpu/lut1d_half.rs`). The DW3 hue adjust is not emitted on this path: refused (the
            // caller turns `None` into a named error).
            ProcessorOp::Lut1d { half_domain: true, lut, channels, hue_adjust, .. } => {
                if *hue_adjust != crate::transform::Lut1DHueAdjust::None {
                    return None;
                }
                let tex_idx = textures.len();
                let (data, width, height) = lut1d_half::padded_texture(lut, *channels);
                textures.push(GpuTexture {
                    name: format!("ocio_lut1d_{}", tex_idx),
                    texture_type: GpuTextureType::Texture2D,
                    width: width as u32,
                    height: height as u32,
                    depth: 1,
                    data,
                    interpolation: GpuInterpolation::Linear,
                });
                Some(GpuOp::Lut1DHalf { texture_idx: tex_idx, width, height })
            }
            // 1D LUT - create texture and return op with index
            ProcessorOp::Lut1d { lut, size, channels, domain_min, domain_max, hue_adjust, .. } => {
                let tex_idx = textures.len();
                let name = format!("ocio_lut1d_{}", tex_idx);

                // Convert to RGBA for GPU (expand if needed)
                let mut data = Vec::with_capacity(*size * 4);
                for i in 0..*size {
                    if *channels == 3 {
                        data.push(lut[i * 3]);
                        data.push(lut[i * 3 + 1]);
                        data.push(lut[i * 3 + 2]);
                        data.push(1.0);
                    } else {
                        // Single channel - replicate to RGB
                        let v = lut[i];
                        data.push(v);
                        data.push(v);
                        data.push(v);
                        data.push(1.0);
                    }
                }

                textures.push(GpuTexture {
                    name: name.clone(),
                    texture_type: GpuTextureType::Texture1D,
                    width: *size as u32,
                    height: 1,
                    depth: 1,
                    data,
                    interpolation: GpuInterpolation::Linear,
                });

                Some(GpuOp::Lut1D {
                    texture_idx: tex_idx,
                    domain_min: *domain_min,
                    domain_max: *domain_max,
                    hue_adjust: *hue_adjust,
                })
            }
            // 3D LUT - create texture and return op with index
            ProcessorOp::Lut3d { lut, size, interp, domain_min, domain_max, .. } => {
                // OCIO `Lut3DOpGPU.cpp`: the tetrahedral body fetches lattice nodes itself, so its
                // texture is sampled NEAREST; trilinear uses the hardware linear filter.
                let tetrahedral =
                    interp.lut3d_method() == vfx_lut::Interpolation::Tetrahedral;
                let tex_idx = textures.len();
                let name = format!("ocio_lut3d_{}", tex_idx);

                // Convert to RGBA for GPU
                let total = *size * *size * *size;
                let mut data = Vec::with_capacity(total * 4);
                for i in 0..total {
                    data.push(lut[i * 3]);
                    data.push(lut[i * 3 + 1]);
                    data.push(lut[i * 3 + 2]);
                    data.push(1.0);
                }

                textures.push(GpuTexture {
                    name: name.clone(),
                    texture_type: GpuTextureType::Texture3D,
                    width: *size as u32,
                    height: *size as u32,
                    depth: *size as u32,
                    data,
                    interpolation: if tetrahedral { GpuInterpolation::Nearest } else { GpuInterpolation::Linear },
                });

                Some(GpuOp::Lut3D {
                    texture_idx: tex_idx,
                    domain_min: *domain_min,
                    domain_max: *domain_max,
                    tetrahedral,
                })
            }
            // Transfer functions inlined as GLSL math
            ProcessorOp::Transfer { style, forward } => Some(GpuOp::Transfer {
                style: *style,
                forward: *forward,
            }),
            // ExposureContrast GPU support
            ProcessorOp::ExposureContrast {
                exposure,
                contrast,
                gamma,
                pivot,
                log_exposure_step,
                log_mid_gray,
                style,
                dynamic,
                forward,
            } => Some(GpuOp::ExposureContrast {
                dynamic: *dynamic,
                forward: *forward,
                exposure: *exposure,
                contrast: *contrast,
                gamma: *gamma,
                // Every style divides by the pivot; OCIO floors it on the GPU exactly as on the
                // CPU (`ExposureContrastOpGPU.cpp:100,134,169`), so the four emitters get the
                // floored value from here (OCIO-M4).
                pivot: pivot.max(vfx_core::consts::exposure_contrast::MIN_PIVOT),
                log_exposure_step: *log_exposure_step,
                log_mid_gray: *log_mid_gray,
                style: *style,
            }),
            ProcessorOp::FixedFunction { style, params, forward } => {
                match crate::processor::fixed_function_to_aces2_op(*style, params, *forward) {
                    Ok(Some(aces2_op)) => Self::convert_op(&aces2_op, textures),
                    Ok(None) => Some(GpuOp::FixedFunction {
                        style: *style,
                        params: params.clone(),
                        forward: *forward,
                    }),
                    // Degenerate ACES2 primaries: the CPU compile already refuses these, so a
                    // processor holding such an op cannot normally reach here; if one does, the
                    // op is unconvertible and the GPU build reports it.
                    Err(_) => None,
                }
            }
            ProcessorOp::Allocation { allocation, vars, forward } => {
                let (min_val, max_val) = match allocation {
                    AllocationType::Unknown => unreachable!("refused when the transform compiles"),
                    AllocationType::Uniform => (
                        vars.first().copied().unwrap_or(0.0),
                        vars.get(1).copied().unwrap_or(1.0),
                    ),
                    AllocationType::Log2 => (
                        vars.first().copied().unwrap_or(-10.0),
                        vars.get(1).copied().unwrap_or(6.0),
                    ),
                };
                let lin_offset = vars.get(2).copied().unwrap_or(0.0);
                Some(GpuOp::Allocation {
                    allocation: *allocation,
                    min_val,
                    max_val,
                    lin_offset,
                    forward: *forward,
                })
            }
            ProcessorOp::GradingPrimary {
                style, brightness, contrast, gamma, exposure, offset, slope,
                saturation, pivot, pivot_black, pivot_white, clamp_black, clamp_white, forward,
                raw,
            } => Some(GpuOp::GradingPrimary {
                local_bypass: crate::processor::grading_primary_local_bypass(
                    &crate::processor::grading_primary_params(*style, *forward, raw),
                    raw,
                ),
                style: *style,
                brightness: *brightness,
                contrast: *contrast,
                gamma: *gamma,
                exposure: *exposure,
                offset: *offset,
                slope: *slope,
                saturation: *saturation,
                pivot: *pivot,
                pivot_black: *pivot_black,
                pivot_white: *pivot_white,
                clamp_black: *clamp_black,
                clamp_white: *clamp_white,
                forward: *forward,
            }),
            ProcessorOp::GradingRgbCurve { style, curve_data, bypass_lin_to_log, forward, .. } => {
                Some(GpuOp::GradingRgbCurve {
                    style: *style,
                    curve_data: curve_data.clone(),
                    bypass_lin_to_log: *bypass_lin_to_log,
                    forward: *forward,
                })
            }
            ProcessorOp::GradingHueCurve { curves, rgb_to_hsy, curve_data, draw_curve_only, forward, .. } => {
                Some(GpuOp::GradingHueCurve {
                    style: curves.style,
                    rgb_to_hsy: *rgb_to_hsy,
                    curve_data: curve_data.clone(),
                    draw_curve_only: *draw_curve_only,
                    forward: *forward,
                })
            }
            // ACES2 Output Transform emits its full scene<->display chain in-shader over the
            // unbounded scene-linear domain rather than baking a [0,1]^3 LUT (ACES2-03, stage 4): the
            // bake clamped every input > 1.0 into the unit cube, crushing HDR highlights vs the
            // verified CPU. The whole `Aces2State` reaches the shader as uniforms (see
            // `build_uniforms`); the body comes from `aces2::emit_output_op`, which composes the
            // in-shader stage-1..3 emitters — no ACES2 maths is re-emitted.
            ProcessorOp::Aces2OutputTransform { state, forward } => {
                // `ProcessorOp`'s `state` is an `Arc` (PLAN9.md O10, shared across processors);
                // `GpuOp`'s is an owned `Box` (built once per shader compile, not a hot path) -
                // so this is a genuine one-time copy out of the Arc, not a redundant clone.
                Some(GpuOp::Aces2OutputTransform {
                    state: Box::new(state.as_ref().clone()),
                    forward: *forward,
                })
            }
            // ACES2 RGB<->JMh emits its CAM16 math in-shader over an unbounded domain rather than
            // baking a [0,1]^3 LUT (ACES2-03): JMh leaves the unit cube in every channel. The four
            // matrices and cz/inv_cz reach the shader as uniforms (see `build_uniforms`); the
            // remaining ACES2 ops (output/tonescale/gamut) stay on the bake path for now.
            ProcessorOp::Aces2RgbJmh { params, forward } => Some(GpuOp::Aces2RgbJmh {
                params: params.clone(),
                forward: *forward,
            }),
            // ACES2 Tonescale+Chroma compress emits its JMh-domain math in-shader over the unbounded
            // domain rather than baking a [0,1]^3 LUT (ACES2-03): J/M/h leave the unit cube, so the
            // bake clamped this op into a corner. Params (incl. the reach-M table) reach the shader
            // as uniforms (see `build_uniforms`); the body comes from `aces2::emit_tonescale_op`.
            ProcessorOp::Aces2TonescaleCompress { jmh_params, ts_params, shared, chroma, forward } => {
                Some(GpuOp::Aces2TonescaleCompress {
                    jmh_params: jmh_params.clone(),
                    ts_params: ts_params.clone(),
                    shared: shared.clone(),
                    chroma: chroma.clone(),
                    forward: *forward,
                })
            }
            // ACES2 Gamut compress emits its JMh-domain math in-shader over the unbounded domain
            // rather than baking a [0,1]^3 LUT (ACES2-03, stage 3): J/M/h leave the unit cube, so the
            // bake clamped this op into a corner. Params (the shared/gamut scalars, the reach-M table,
            // the hue axis and the cusp table) reach the shader as uniforms (see `build_uniforms`); the
            // body comes from `aces2::emit_gamut_op`, including the inverse's two-pass refinement.
            ProcessorOp::Aces2GamutCompress { shared, gamut, forward } => {
                Some(GpuOp::Aces2GamutCompress {
                    shared: shared.clone(),
                    gamut: gamut.clone(),
                    forward: *forward,
                })
            }
            ProcessorOp::GradingTone {
                style,
                value,
                pre_render,
                forward,
            } => {
                // Re-pack the GPU's [r,g,b,m,start,width] arrays from the resolved
                // value + precomputed ranges. Blacks/shadows/highlights/whites use
                // the resolved (vpr) start/width; midtones uses the authored
                // center/width. Mirrors the previous compile-time packing.
                let pack = |z: &crate::transform::GradingRgbmsw, start: f32, width: f32| {
                    [z.red as f32, z.green as f32, z.blue as f32, z.master as f32, start, width]
                };
                Some(GpuOp::GradingTone {
                    style: *style,
                    blacks: pack(
                        &value.blacks,
                        pre_render.blacks_start as f32,
                        pre_render.blacks_width as f32,
                    ),
                    shadows: pack(
                        &value.shadows,
                        pre_render.shadows_start as f32,
                        pre_render.shadows_width as f32,
                    ),
                    midtones: pack(
                        &value.midtones,
                        value.midtones.start as f32,
                        value.midtones.width as f32,
                    ),
                    highlights: pack(
                        &value.highlights,
                        pre_render.highlights_start as f32,
                        pre_render.highlights_width as f32,
                    ),
                    whites: pack(
                        &value.whites,
                        pre_render.whites_start as f32,
                        pre_render.whites_width as f32,
                    ),
                    scontrast: value.s_contrast as f32,
                    forward: *forward,
                })
            }
            // New transforms with full GPU support
            ProcessorOp::LogAffine {
                base,
                log_side_slope,
                log_side_offset,
                lin_side_slope,
                lin_side_offset,
                forward,
            } => Some(GpuOp::LogAffine {
                base: *base,
                log_side_slope: *log_side_slope,
                log_side_offset: *log_side_offset,
                lin_side_slope: *lin_side_slope,
                lin_side_offset: *lin_side_offset,
                forward: *forward,
            }),
            ProcessorOp::LogCamera {
                base,
                log_side_slope,
                log_side_offset,
                lin_side_slope,
                lin_side_offset,
                lin_side_break,
                linear_slope,
                log_side_break,
                linear_offset,
                forward,
                .. // log_base: the emitter takes ln(base) itself
            } => Some(GpuOp::LogCamera {
                base: *base,
                log_side_slope: *log_side_slope,
                log_side_offset: *log_side_offset,
                lin_side_slope: *lin_side_slope,
                lin_side_offset: *lin_side_offset,
                lin_side_break: *lin_side_break,
                linear_slope: *linear_slope,
                log_side_break: *log_side_break,
                linear_offset: *linear_offset,
                forward: *forward,
            }),
            ProcessorOp::ExponentWithLinear {
                gamma,
                offset,
                mirror,
                forward,
            } => Some(GpuOp::ExponentWithLinear {
                gamma: *gamma,
                offset: *offset,
                mirror: *mirror,
                forward: *forward,
            }),
        }
    }

    /// Returns true if this processor can be fully represented on GPU.
    pub fn is_complete(&self) -> bool {
        !self.ops.is_empty()
    }

    /// Returns the number of GPU operations.
    pub fn num_ops(&self) -> usize {
        self.ops.len()
    }

    // =========================================================================
    // Batch Texture API (OCIO parity: GpuShaderDesc texture methods)
    // =========================================================================

    /// Returns the number of 1D/2D textures.
    /// OCIO ref: GpuShaderDesc::getNumTextures()
    #[inline]
    pub fn num_textures(&self) -> usize {
        self.textures
            .iter()
            .filter(|t| t.texture_type != GpuTextureType::Texture3D)
            .count()
    }

    /// Returns the number of 3D textures.
    /// OCIO ref: GpuShaderDesc::getNum3DTextures()
    #[inline]
    pub fn num_3d_textures(&self) -> usize {
        self.textures
            .iter()
            .filter(|t| t.texture_type == GpuTextureType::Texture3D)
            .count()
    }

    /// Returns all textures.
    /// OCIO ref: GpuShaderDesc texture iteration
    #[inline]
    pub fn textures(&self) -> &[GpuTexture] {
        &self.textures
    }

    /// Returns a 1D/2D texture by index.
    /// OCIO ref: GpuShaderDesc::getTexture(idx)
    pub fn texture(&self, idx: usize) -> Option<&GpuTexture> {
        self.textures
            .iter()
            .filter(|t| t.texture_type != GpuTextureType::Texture3D)
            .nth(idx)
    }

    /// Returns a 3D texture by index.
    /// OCIO ref: GpuShaderDesc::get3DTexture(idx)
    pub fn texture_3d(&self, idx: usize) -> Option<&GpuTexture> {
        self.textures
            .iter()
            .filter(|t| t.texture_type == GpuTextureType::Texture3D)
            .nth(idx)
    }

    /// Returns texture values (LUT data) for a 1D/2D texture.
    /// OCIO ref: GpuShaderDesc::getTextureValues(idx)
    pub fn texture_values(&self, idx: usize) -> Option<&[f32]> {
        self.texture(idx).map(|t| t.data.as_slice())
    }

    /// Returns texture values (LUT data) for a 3D texture.
    /// OCIO ref: GpuShaderDesc::get3DTextureValues(idx)
    pub fn texture_3d_values(&self, idx: usize) -> Option<&[f32]> {
        self.texture_3d(idx).map(|t| t.data.as_slice())
    }

    /// Returns the shader binding index for a 1D/2D texture.
    /// For Vulkan/explicit binding languages.
    /// OCIO ref: GpuShaderDesc::getTextureShaderBindingIndex(idx)
    pub fn texture_binding_index(&self, idx: usize) -> Option<usize> {
        // In our implementation, textures are bound in order starting from 1
        // (index 0 is reserved for the input texture)
        if idx < self.num_textures() {
            let pos = self
                .textures
                .iter()
                .enumerate()
                .filter(|(_, t)| t.texture_type != GpuTextureType::Texture3D)
                .nth(idx)
                .map(|(i, _)| i)?;
            Some(pos + 1) // +1 for input texture
        } else {
            None
        }
    }

    /// Returns the shader binding index for a 3D texture.
    /// For Vulkan/explicit binding languages.
    /// OCIO ref: GpuShaderDesc::get3DTextureShaderBindingIndex(idx)
    pub fn texture_3d_binding_index(&self, idx: usize) -> Option<usize> {
        if idx < self.num_3d_textures() {
            let pos = self
                .textures
                .iter()
                .enumerate()
                .filter(|(_, t)| t.texture_type == GpuTextureType::Texture3D)
                .nth(idx)
                .map(|(i, _)| i)?;
            Some(pos + 1) // +1 for input texture
        } else {
            None
        }
    }

    /// Creates a batch texture descriptor for efficient GPU upload.
    /// Returns all texture metadata needed for batch upload.
    /// OCIO ref: Batch texture operations
    pub fn batch_texture_descriptors(&self) -> Vec<BatchTextureDescriptor> {
        self.textures
            .iter()
            .enumerate()
            .map(|(idx, tex)| {
                BatchTextureDescriptor {
                    name: tex.name.clone(),
                    binding_index: idx + 1, // +1 for input texture
                    texture_type: tex.texture_type,
                    width: tex.width,
                    height: tex.height,
                    depth: tex.depth,
                    channel_type: if tex.data.len() / (tex.width * tex.height * tex.depth) as usize
                        == 4
                    {
                        GpuChannelType::Rgb
                    } else {
                        GpuChannelType::Red
                    },
                    interpolation: tex.interpolation,
                    dimensions: match tex.texture_type {
                        GpuTextureType::Texture1D => TextureDimensions::Texture1D,
                        GpuTextureType::Texture2D | GpuTextureType::Texture3D => {
                            TextureDimensions::Texture2D
                        }
                    },
                    data_offset: 0, // Calculated during batch upload
                    data_size: tex.data.len() * std::mem::size_of::<f32>(),
                }
            })
            .collect()
    }

    /// Returns all texture data packed into a single buffer.
    /// Useful for batch GPU upload.
    pub fn batch_texture_data(&self) -> (Vec<f32>, Vec<usize>) {
        let mut data = Vec::new();
        let mut offsets = Vec::with_capacity(self.textures.len());

        for tex in &self.textures {
            offsets.push(data.len());
            data.extend_from_slice(&tex.data);
        }

        (data, offsets)
    }

    /// Validates all textures have compatible formats for batch upload.
    pub fn validate_textures(&self) -> Result<(), String> {
        for (idx, tex) in self.textures.iter().enumerate() {
            // Validate dimensions
            if tex.width == 0 || tex.height == 0 || tex.depth == 0 {
                return Err(format!("Texture {} '{}' has zero dimension", idx, tex.name));
            }

            // Validate data size
            let expected_size = (tex.width * tex.height * tex.depth * 4) as usize; // RGBA
            if tex.data.len() != expected_size {
                return Err(format!(
                    "Texture {} '{}' data size mismatch: expected {}, got {}",
                    idx,
                    tex.name,
                    expected_size,
                    tex.data.len()
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
