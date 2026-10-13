//! Public API types and private uniform helpers for the GPU shader-generation backend.
//!
//! Extracted from `gpu.rs` during the STRUCT.1 split (TODO6.md). Public items
//! (`GpuLanguage`, `GpuTexture`, `GpuShaderCode`, …) are re-exported at the
//! parent `gpu` module level so external consumers (parity_tests, downstream
//! crates) keep using the original `crate::gpu::Gpu*` paths.
//!
//! Private helpers (`OpUniforms`, `UniformRegistry`) are marked `pub(super)` so the
//! parent module's `GpuProcessor` impl can construct, destructure, and call
//! them across the file boundary.

/// Target shader language (OCIO parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GpuLanguage {
    /// Nvidia Cg shader (deprecated in OCIO).
    Cg,
    /// GLSL 1.20 (OpenGL 2.1)
    Glsl120,
    /// GLSL 1.30 (OpenGL 3.0) - OCIO GPU_LANGUAGE_GLSL_1_3
    Glsl130,
    /// GLSL 3.30 (OpenGL 3.3)
    #[default]
    Glsl330,
    /// GLSL 4.00 (OpenGL 4.0)
    Glsl400,
    /// GLSL 4.60 for Vulkan - OCIO GPU_LANGUAGE_GLSL_VK_4_6
    GlslVulkan460,
    /// GLSL ES 1.00 (WebGL 1.0) - OCIO GPU_LANGUAGE_GLSL_ES_1_0
    GlslEs100,
    /// GLSL ES 3.00 (WebGL 2.0)
    GlslEs300,
    /// HLSL Shader Model 5.0
    Hlsl50,
    /// Open Shading Language (OSL)
    Osl,
    /// Metal Shading Language
    Metal,
    /// WebGPU Shading Language (WGSL)
    Wgsl,
}

impl GpuLanguage {
    /// Returns the version directive for this language.
    pub fn version_directive(&self) -> &'static str {
        match self {
            GpuLanguage::Cg => "",
            GpuLanguage::Glsl120 => "#version 120",
            GpuLanguage::Glsl130 => "#version 130",
            GpuLanguage::Glsl330 => "#version 330 core",
            GpuLanguage::Glsl400 => "#version 400 core",
            GpuLanguage::GlslVulkan460 => {
                "#version 460\n#extension GL_GOOGLE_include_directive : enable"
            }
            GpuLanguage::GlslEs100 => "#version 100",
            GpuLanguage::GlslEs300 => "#version 300 es",
            GpuLanguage::Hlsl50 => "",
            GpuLanguage::Osl => "",
            GpuLanguage::Metal => "",
            GpuLanguage::Wgsl => "", // WGSL doesn't use version directives
        }
    }

    /// Returns true if this is a GLSL variant.
    pub fn is_glsl(&self) -> bool {
        matches!(
            self,
            GpuLanguage::Glsl120
                | GpuLanguage::Glsl130
                | GpuLanguage::Glsl330
                | GpuLanguage::Glsl400
                | GpuLanguage::GlslVulkan460
                | GpuLanguage::GlslEs100
                | GpuLanguage::GlslEs300
        )
    }

    /// Returns true if this is WGSL.
    pub fn is_wgsl(&self) -> bool {
        matches!(self, GpuLanguage::Wgsl)
    }
}

/// GPU texture requirement for LUT sampling.
#[derive(Debug, Clone)]
pub struct GpuTexture {
    /// Texture name/identifier.
    pub name: String,
    /// Texture type.
    pub texture_type: GpuTextureType,
    /// Texture width.
    pub width: u32,
    /// Texture height (1 for 1D textures).
    pub height: u32,
    /// Texture depth (1 for 1D/2D textures).
    pub depth: u32,
    /// Pixel data (linear f32 RGBA).
    pub data: Vec<f32>,
    /// Interpolation mode.
    pub interpolation: GpuInterpolation,
}

/// Texture type for GPU LUTs.
/// OCIO ref: GpuShaderCreator::TextureType
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuTextureType {
    /// 1D texture (for 1D LUTs).
    Texture1D,
    /// 2D texture (for 1D LUT with channel separation).
    Texture2D,
    /// 3D texture (for 3D LUTs).
    Texture3D,
}

/// Channel type for textures.
/// OCIO ref: GpuShaderCreator::TextureType (TEXTURE_RED_CHANNEL, TEXTURE_RGB_CHANNEL)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuChannelType {
    /// Single-channel (red) texture.
    Red,
    /// Three-channel RGB texture.
    #[default]
    Rgb,
}

/// Texture dimensions for GPU textures.
/// OCIO ref: GpuShaderCreator::TextureDimensions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureDimensions {
    /// One-dimensional texture.
    #[default]
    Texture1D,
    /// Two-dimensional texture.
    Texture2D,
}

/// Descriptor for batch texture upload.
/// Contains all metadata needed for efficient GPU texture creation.
/// OCIO ref: GpuShaderDesc texture API for batch operations
#[derive(Debug, Clone)]
pub struct BatchTextureDescriptor {
    /// Texture name/identifier.
    pub name: String,
    /// Shader binding index (for Vulkan/explicit binding).
    pub binding_index: usize,
    /// Texture type (1D, 2D, 3D).
    pub texture_type: GpuTextureType,
    /// Texture width.
    pub width: u32,
    /// Texture height.
    pub height: u32,
    /// Texture depth.
    pub depth: u32,
    /// Channel type (Red or RGB).
    pub channel_type: GpuChannelType,
    /// Interpolation mode.
    pub interpolation: GpuInterpolation,
    /// Dimensions (1D vs 2D for 1D LUT storage).
    pub dimensions: TextureDimensions,
    /// Offset into packed data buffer (set during batch_texture_data).
    pub data_offset: usize,
    /// Size of texture data in bytes.
    pub data_size: usize,
}

/// Texture interpolation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuInterpolation {
    /// Nearest neighbor.
    Nearest,
    /// Linear interpolation.
    #[default]
    Linear,
}

/// Generated shader code.
#[derive(Debug, Clone)]
pub struct GpuShaderCode {
    /// Fragment shader code.
    pub(super) fragment: String,
    /// Required textures.
    pub(super) textures: Vec<GpuTexture>,
    /// Uniform declarations.
    pub(super) uniforms: Vec<GpuUniform>,
}

/// Shader uniform variable.
#[derive(Debug, Clone)]
pub struct GpuUniform {
    /// Uniform name.
    pub name: String,
    /// Uniform type.
    pub uniform_type: GpuUniformType,
    /// Default value.
    pub default_value: GpuUniformValue,
    /// Max array length (for array uniforms).
    pub max_len: Option<usize>,
}

/// Uniform variable type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuUniformType {
    /// Single float.
    Float,
    /// Single bool.
    Bool,
    /// Single int.
    Int,
    /// 3-component vector.
    Vec3,
    /// 4-component vector.
    Vec4,
    /// 3x3 matrix.
    Mat3,
    /// 4x4 matrix.
    Mat4,
    /// Float array uniform.
    ArrayFloat,
    /// Int array uniform.
    ArrayInt,
}

/// Uniform value payload.
#[derive(Debug, Clone, PartialEq)]
pub enum GpuUniformValue {
    /// Single `float` scalar.
    Float(f32),
    /// Single `bool` scalar.
    Bool(bool),
    /// Single `int` scalar.
    Int(i32),
    /// `vec3` of floats.
    Vec3([f32; 3]),
    /// `vec4` of floats.
    Vec4([f32; 4]),
    /// Column-major `mat3` (9 floats).
    Mat3([f32; 9]),
    /// Column-major `mat4` (16 floats).
    Mat4([f32; 16]),
    /// Dynamic float array uniform.
    ArrayFloat(Vec<f32>),
    /// Dynamic int array uniform.
    ArrayInt(Vec<i32>),
}

/// Uniform names of one GradingTone op: the five RGBM + start + width controls, s-contrast
/// and the local-bypass flag.
#[derive(Debug, Clone)]
pub(super) struct GradingToneUniforms {
    pub(super) blacks_r: String,
    pub(super) blacks_g: String,
    pub(super) blacks_b: String,
    pub(super) blacks_m: String,
    pub(super) blacks_start: String,
    pub(super) blacks_width: String,
    pub(super) shadows_r: String,
    pub(super) shadows_g: String,
    pub(super) shadows_b: String,
    pub(super) shadows_m: String,
    pub(super) shadows_start: String,
    pub(super) shadows_width: String,
    pub(super) midtones_r: String,
    pub(super) midtones_g: String,
    pub(super) midtones_b: String,
    pub(super) midtones_m: String,
    pub(super) midtones_start: String,
    pub(super) midtones_width: String,
    pub(super) highlights_r: String,
    pub(super) highlights_g: String,
    pub(super) highlights_b: String,
    pub(super) highlights_m: String,
    pub(super) highlights_start: String,
    pub(super) highlights_width: String,
    pub(super) whites_r: String,
    pub(super) whites_g: String,
    pub(super) whites_b: String,
    pub(super) whites_m: String,
    pub(super) whites_start: String,
    pub(super) whites_width: String,
    pub(super) scontrast: String,
    pub(super) local_bypass: String,
}

#[derive(Debug, Clone)]
pub(super) enum OpUniforms {
    ExposureContrast {
        exposure: String,
        contrast: String,
        gamma: String,
    },
    GradingPrimary {
        brightness: String,
        contrast: String,
        gamma: String,
        exposure: String,
        offset: String,
        slope: String,
        saturation: String,
        pivot: String,
        pivot_black: String,
        pivot_white: String,
        clamp_black: String,
        clamp_white: String,
        local_bypass: String,
    },
    /// Boxed: its 32 uniform names would otherwise make this the largest variant by far.
    GradingTone(Box<GradingToneUniforms>),
    GradingRgbCurve {
        knots_offsets: String,
        knots: String,
        coefs_offsets: String,
        coefs: String,
        local_bypass: String,
    },
    GradingHueCurve {
        knots_offsets: String,
        knots: String,
        coefs_offsets: String,
        coefs: String,
        local_bypass: String,
    },
    /// ACES 2.0 `RGB <-> JMh` (CAM16) uniform names.
    ///
    /// Holds exactly the `JMhParams` fields this conversion reads: the four CAM16 matrices (`Mat3`
    /// uniforms uploaded column-major) and the `cz`/`inv_cz` `J` exponents (`Float`). The
    /// tonescale/gamut stages (2-5) that share this op's `JMhParams` will extend this variant and
    /// its `build_uniforms` arm with `f_l_n`/`a_w_j` when they add the `J <-> Y` conversions that
    /// consume them — registering them before anything reads them would only be dead uniforms.
    Aces2Jmh {
        mat_rgb_to_cam16_c: String,
        mat_cam16_c_to_rgb: String,
        mat_cone_to_aab: String,
        mat_aab_to_cone: String,
        cz: String,
        inv_cz: String,
    },
    /// ACES 2.0 Tonescale + Chroma compress uniform names (ACES2-03, stage 2).
    ///
    /// Holds the scalars `tonescale_compress_{fwd,inv}` reads — the `J <-> Y` conversion terms
    /// (`a_w_j`/`inv_a_w_j`/`f_l_n`/`cz`/`inv_cz`), the `ToneScaleParams` curve (`n_r`/`s_2`/`m_2`/
    /// `g`/`t_1`/`inverse_limit`), the shared compression scalars (`limit_j_max`/`model_gamma_inv`),
    /// and the `ChromaCompressParams` (`sat`/`sat_thr`/`compr`/`ccs`) — plus `reach`, the name of the
    /// 363-entry reach-`M` `ArrayFloat` uniform the chroma compress samples per hue. All are sourced
    /// from the CPU `aces2` params in `build_uniforms`, never retyped.
    Aces2Tonescale {
        a_w_j: String,
        inv_a_w_j: String,
        f_l_n: String,
        cz: String,
        inv_cz: String,
        n_r: String,
        s_2: String,
        m_2: String,
        g: String,
        t_1: String,
        inverse_limit: String,
        limit_j_max: String,
        model_gamma_inv: String,
        sat: String,
        sat_thr: String,
        compr: String,
        ccs: String,
        reach: String,
    },
    /// ACES 2.0 Gamut compress uniform names (ACES2-03, stage 3).
    ///
    /// Holds the scalars `gamut_compress_{fwd,inv}` reads — the shared compression terms
    /// (`limit_j_max`/`model_gamma_inv`), the `GamutCompressParams` scalars (`mid_j`/`focus_dist`/
    /// `lower_hull_gamma_inv`) and the two `hue_linearity_search_range` bounds (`search_lo`/
    /// `search_hi`, uploaded as `Float` because they are small exactly-representable integers) — plus
    /// three `ArrayFloat` uniforms: `reach` (363-entry reach-`M`, reused from the tonescale stage),
    /// `hue_table` (363-entry non-uniform hue axis the cusp binary-search walks) and `cusp_table`
    /// (363×3 packed `[J, M, gamma_top_inv]` per hue). All are sourced from the CPU `aces2` params in
    /// `build_uniforms`, never retyped.
    Aces2Gamut {
        limit_j_max: String,
        model_gamma_inv: String,
        mid_j: String,
        focus_dist: String,
        lower_hull_gamma_inv: String,
        search_lo: String,
        search_hi: String,
        reach: String,
        hue_table: String,
        cusp_table: String,
    },
    /// ACES 2.0 Output Transform uniform names (ACES2-03, stage 4) — the full scene-referred <->
    /// display transform, emitted as the in-shader composition of the three JMh-domain stages.
    ///
    /// Rather than re-listing the ~40 scalar names, this carries the four sub-op uniform sets the
    /// composed body dispatches to, each an [`OpUniforms`] of the matching stage variant:
    /// `jmh_in`/`jmh_out` (two [`OpUniforms::Aces2Jmh`] param sets — `RGB <-> JMh` runs with the
    /// input primaries at one end and the output primaries at the other), `tonescale`
    /// ([`OpUniforms::Aces2Tonescale`]) and `gamut` ([`OpUniforms::Aces2Gamut`]).
    /// [`super::aces2::emit_output_op`] hands each straight to the verified stage-1..3 wrapper, so no
    /// name-resolution and no ACES2 maths is duplicated. Boxed to keep the enum small.
    Aces2Output {
        jmh_in: Box<OpUniforms>,
        jmh_out: Box<OpUniforms>,
        tonescale: Box<OpUniforms>,
        gamut: Box<OpUniforms>,
    },
}

pub(super) struct UniformRegistry {
    pub(super) uniforms: Vec<GpuUniform>,
}

impl UniformRegistry {
    pub(super) fn new() -> Self {
        Self {
            uniforms: Vec::new(),
        }
    }

    pub(super) fn add(
        &mut self,
        name: String,
        uniform_type: GpuUniformType,
        default_value: GpuUniformValue,
        max_len: Option<usize>,
    ) -> String {
        self.uniforms.push(GpuUniform {
            name: name.clone(),
            uniform_type,
            default_value,
            max_len,
        });
        name
    }
}

impl GpuShaderCode {
    /// Returns the fragment shader code.
    pub fn fragment_code(&self) -> &str {
        &self.fragment
    }

    /// Returns required textures.
    pub fn textures(&self) -> &[GpuTexture] {
        &self.textures
    }

    /// Returns required uniforms.
    pub fn uniforms(&self) -> &[GpuUniform] {
        &self.uniforms
    }

    /// Returns mutable uniforms.
    pub fn uniforms_mut(&mut self) -> &mut [GpuUniform] {
        &mut self.uniforms
    }

    /// Finds a uniform by name.
    pub fn uniform(&self, name: &str) -> Option<&GpuUniform> {
        self.uniforms.iter().find(|u| u.name == name)
    }

    /// Finds a uniform by name (mutable).
    pub fn uniform_mut(&mut self, name: &str) -> Option<&mut GpuUniform> {
        self.uniforms.iter_mut().find(|u| u.name == name)
    }

    /// Sets a uniform value by name (if present).
    pub fn set_uniform_value(&mut self, name: &str, value: GpuUniformValue) -> bool {
        if let Some(u) = self.uniform_mut(name) {
            u.default_value = value;
            true
        } else {
            false
        }
    }

    /// Returns true if any textures are required.
    pub fn has_textures(&self) -> bool {
        !self.textures.is_empty()
    }
}
