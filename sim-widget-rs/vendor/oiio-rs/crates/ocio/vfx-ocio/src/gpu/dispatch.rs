//! Shader-language dispatch + uniform declaration emitters.
//!
//! Extracted from `gpu.rs` during the STRUCT.1 split (TODO6.md). Lives as a
//! child module of `gpu` so it has direct access to the parent's private
//! `GpuOp` enum and `build_uniforms` helper without explicit `pub(super)`
//! markers (Rust gives child modules visibility into their ancestors'
//! private items).
//!
//! This file owns the `generate_shader` entry point that branches on
//! `GpuLanguage`, plus the per-language uniform-declaration emitters
//! (`emit_uniform_decls_glsl/hlsl/metal`). The WGSL variant lives in
//! `gpu/wgsl.rs` once that split lands; for now it stays in `gpu.rs`.
//!
//! # Allocation handling vs OCIO C++ `LegacyShaderGen`
//!
//! OCIO C++ keeps two distinct mechanisms for colorspace
//! allocations:
//!
//! 1. The user-facing `AllocationTransform` that applies real
//!    linear/log2 fit-to-[0,1] math. vfx-ocio implements this in
//!    every backend — search for `GpuOp::Allocation` in
//!    `gpu/{wgsl,glsl,hlsl,metal}.rs`.
//! 2. `CreateGpuAllocationNoOp` markers that OCIO C++ slips into
//!    the op vector during `BuildColorSpaceOps` so its legacy
//!    OpenGL `LegacyShaderGen` path can wrap downstream ops with
//!    pack/unpack code for fixed-point texture samplers.
//!
//! vfx-ocio implements (1) and intentionally omits (2). The wgpu
//! WGSL target — and every other backend we ship — uses
//! float16/float32 textures and writes the colorspace math out
//! directly, so the wrap/unwrap frame the marker enables is dead
//! code in our pipeline. See the `AllocationTransform` rustdoc in
//! `transform.rs` and the `build_colorspace_chain` rustdoc in
//! `config/factory.rs` for the corresponding notes on the
//! ColorSpaceTransform-construction side.

use super::*;
use crate::gpu_shader_utils::ShaderText;
use std::fmt::Write;

impl GpuProcessor {
    /// Generates shader code for the given language.
    ///
    /// Cg and OSL are OCIO languages this port does not emit; asking for them is an
    /// `OcioError::Gpu` error (it used to return GLSL 3.30 with a comment, which a Cg or OSL
    /// compiler then rejects far from the cause; OCIO-10).
    pub fn generate_shader(&self, language: GpuLanguage) -> OcioResult<GpuShaderCode> {
        if matches!(language, GpuLanguage::Cg | GpuLanguage::Osl) {
            return Err(crate::OcioError::Gpu(format!(
                "{language:?} shader generation is not supported (GLSL, HLSL, Metal and WGSL are)"
            )));
        }
        let mut code = String::new();
        let (uniform_bindings, uniforms) = self.build_uniforms();
        writeln!(code, "{}", language.version_directive()).unwrap();
        writeln!(code).unwrap();

        match language {
            GpuLanguage::Cg | GpuLanguage::Osl => unreachable!("refused above"),
            GpuLanguage::Glsl120
            | GpuLanguage::Glsl130
            | GpuLanguage::Glsl330
            | GpuLanguage::Glsl400
            | GpuLanguage::GlslVulkan460
            | GpuLanguage::GlslEs100
            | GpuLanguage::GlslEs300 => {
                self.generate_glsl(language, &mut code, &uniform_bindings, &uniforms);
            }
            GpuLanguage::Hlsl50 => {
                self.generate_hlsl(&mut code, &uniform_bindings, &uniforms);
            }
            GpuLanguage::Metal => {
                self.generate_metal(&mut code, &uniform_bindings, &uniforms);
            }
            GpuLanguage::Wgsl => {
                self.generate_wgsl(&mut code, &uniform_bindings, &uniforms);
            }
        }

        Ok(GpuShaderCode {
            fragment: code,
            textures: self.textures.clone(),
            uniforms,
        })
    }

    pub(super) fn emit_uniform_decls_glsl(&self, code: &mut String, uniforms: &[GpuUniform]) {
        if uniforms.is_empty() {
            return;
        }
        writeln!(code, "// Uniforms").unwrap();
        for u in uniforms {
            match u.uniform_type {
                GpuUniformType::Float => {
                    writeln!(code, "uniform float {};", u.name).unwrap();
                }
                GpuUniformType::Bool => {
                    writeln!(code, "uniform bool {};", u.name).unwrap();
                }
                GpuUniformType::Int => {
                    writeln!(code, "uniform int {};", u.name).unwrap();
                }
                GpuUniformType::Vec3 => {
                    writeln!(code, "uniform vec3 {};", u.name).unwrap();
                }
                GpuUniformType::Vec4 => {
                    writeln!(code, "uniform vec4 {};", u.name).unwrap();
                }
                GpuUniformType::Mat3 => {
                    writeln!(code, "uniform mat3 {};", u.name).unwrap();
                }
                GpuUniformType::Mat4 => {
                    writeln!(code, "uniform mat4 {};", u.name).unwrap();
                }
                GpuUniformType::ArrayFloat => {
                    let max_len = u.max_len.unwrap_or(0);
                    writeln!(code, "uniform float {}[{}];", u.name, max_len).unwrap();
                }
                GpuUniformType::ArrayInt => {
                    let max_len = u.max_len.unwrap_or(0);
                    writeln!(code, "uniform int {}[{}];", u.name, max_len).unwrap();
                }
            }
        }
        writeln!(code).unwrap();
    }

    /// Vulkan GLSL's uniforms: one `layout(set = 0, binding = 0) uniform` block of the same members
    /// ([`Self::emit_uniform_decls_glsl`] without the `uniform` keyword), as OCIO wraps its
    /// parameter declarations for `GPU_LANGUAGE_GLSL_VK_4_6`.
    pub(super) fn emit_uniform_block_glsl_vk(&self, code: &mut String, uniforms: &[GpuUniform]) {
        if uniforms.is_empty() {
            return;
        }
        let mut members = String::new();
        self.emit_uniform_decls_glsl(&mut members, uniforms);
        writeln!(
            code,
            "layout(set = 0, binding = 0) uniform ocio_transform_Parameters {{"
        )
        .unwrap();
        for line in members.lines().filter(|l| l.starts_with("uniform ")) {
            writeln!(code, "    {}", &line["uniform ".len()..]).unwrap();
        }
        writeln!(code, "}};").unwrap();
        writeln!(code).unwrap();
    }

    pub(super) fn emit_uniform_decls_hlsl(&self, st: &mut ShaderText, uniforms: &[GpuUniform]) {
        if uniforms.is_empty() {
            return;
        }
        st.line("cbuffer OcioUniforms : register(b0) {");
        for u in uniforms {
            match u.uniform_type {
                GpuUniformType::Float => {
                    st.linef(format!("float {};", u.name));
                }
                GpuUniformType::Bool => {
                    st.linef(format!("bool {};", u.name));
                }
                GpuUniformType::Int => {
                    st.linef(format!("int {};", u.name));
                }
                GpuUniformType::Vec3 => {
                    st.linef(format!("float3 {};", u.name));
                }
                GpuUniformType::Vec4 => {
                    st.linef(format!("float4 {};", u.name));
                }
                GpuUniformType::Mat3 => {
                    st.linef(format!("float3x3 {};", u.name));
                }
                GpuUniformType::Mat4 => {
                    st.linef(format!("float4x4 {};", u.name));
                }
                GpuUniformType::ArrayFloat => {
                    let max_len = u.max_len.unwrap_or(0);
                    st.linef(format!("float {}[{}];", u.name, max_len));
                }
                GpuUniformType::ArrayInt => {
                    let max_len = u.max_len.unwrap_or(0);
                    st.linef(format!("int {}[{}];", u.name, max_len));
                }
            }
        }
        st.line("};");
        st.new_line();
    }

    pub(super) fn emit_uniform_decls_metal(&self, st: &mut ShaderText, uniforms: &[GpuUniform]) {
        if uniforms.is_empty() {
            return;
        }
        st.line("struct OcioUniforms {");
        st.indent();
        for u in uniforms {
            match u.uniform_type {
                GpuUniformType::Float => st.linef(format!("float {};", u.name)),
                GpuUniformType::Bool => st.linef(format!("bool {};", u.name)),
                GpuUniformType::Int => st.linef(format!("int {};", u.name)),
                GpuUniformType::Vec3 => st.linef(format!("float3 {};", u.name)),
                GpuUniformType::Vec4 => st.linef(format!("float4 {};", u.name)),
                GpuUniformType::Mat3 => st.linef(format!("float3x3 {};", u.name)),
                GpuUniformType::Mat4 => st.linef(format!("float4x4 {};", u.name)),
                GpuUniformType::ArrayFloat => {
                    let max_len = u.max_len.unwrap_or(0);
                    st.linef(format!("float {}[{}];", u.name, max_len));
                }
                GpuUniformType::ArrayInt => {
                    let max_len = u.max_len.unwrap_or(0);
                    st.linef(format!("int {}[{}];", u.name, max_len));
                }
            }
        }
        st.dedent();
        st.line("};");
        st.new_line();
    }
}
