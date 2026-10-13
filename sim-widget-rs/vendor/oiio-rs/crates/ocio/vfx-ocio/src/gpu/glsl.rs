//! The GLSL shell for every GLSL version (1.20, 1.30, 3.30, 4.00, 4.60 Vulkan, ES 1.00, ES 3.00):
//! the version directive, precision, uniforms and textures in the version's spelling, then
//! `ocio_transform` around the op bodies of `gpu/ops.rs` (written once for every language,
//! ROADMAP G6). Nothing here is colour maths.
//!
//! The `writeln!` targets are `String`s, whose `fmt::Write` cannot fail; the `unwrap()`s are
//! that formality, not a guarded failure.

use super::*;
use std::fmt::Write;

impl GpuProcessor {
    /// Generates GLSL shader code for `language` (a GLSL variant).
    ///
    /// The shell follows the version: GLSL 1.20 and ES 1.00 read `varying` and write
    /// `gl_FragColor` with `texture2D`/`texture3D` (ES 1.00's 3D textures need
    /// `GL_OES_texture_3D`); later versions use `in`/`out` and `texture`. OpenGL ES has no default
    /// precision for `sampler3D` (nor, in ES 1.00, any float), so ES declares `highp`. ES has no 1D
    /// textures: a 1D LUT is a one-row `sampler2D` ([`dialect::Dialect::sample`]). Vulkan GLSL 4.60
    /// takes no loose uniforms: as OCIO (`GpuShaderCreator::createShaderText`) the parameters go in
    /// one `layout(set = 0, binding = 0) uniform ocio_transform_Parameters` block and every sampler
    /// has a binding (the input 1, the LUTs from 2), and the stage variables have locations.
    pub(super) fn generate_glsl(
        &self,
        language: GpuLanguage,
        code: &mut String,
        uniform_bindings: &[Option<OpUniforms>],
        uniforms: &[GpuUniform],
    ) {
        let d = dialect::Dialect::for_language(language);
        let vk = language == GpuLanguage::GlslVulkan460;
        let has_3d = self
            .textures
            .iter()
            .any(|t| t.texture_type == GpuTextureType::Texture3D);

        if language == GpuLanguage::GlslEs100 && has_3d {
            writeln!(code, "#extension GL_OES_texture_3D : enable").unwrap();
        }
        if d.is_es() {
            writeln!(code, "precision highp float;").unwrap();
            writeln!(code, "precision highp sampler2D;").unwrap();
            if has_3d {
                writeln!(code, "precision highp sampler3D;").unwrap();
            }
        }

        // Stage input/output
        if d.legacy_glsl() {
            writeln!(code, "varying vec2 v_texCoord;").unwrap();
        } else if vk {
            writeln!(code, "layout(location = 0) in vec2 v_texCoord;").unwrap();
            writeln!(code, "layout(location = 0) out vec4 fragColor;").unwrap();
        } else {
            writeln!(code, "in vec2 v_texCoord;").unwrap();
            writeln!(code, "out vec4 fragColor;").unwrap();
        }
        let binding = |i: usize| {
            if vk {
                format!("layout(set = 0, binding = {i}) ")
            } else {
                String::new()
            }
        };
        writeln!(code, "{}uniform sampler2D u_inputTexture;", binding(1)).unwrap();

        // LUT samplers
        for (i, tex) in self.textures.iter().enumerate() {
            writeln!(
                code,
                "{}uniform {} {};",
                binding(i + 2),
                d.glsl_sampler_type(tex.texture_type),
                tex.name
            )
            .unwrap();
        }
        writeln!(code).unwrap();

        if vk {
            self.emit_uniform_block_glsl_vk(code, uniforms);
        } else {
            self.emit_uniform_decls_glsl(code, uniforms);
        }

        // Helper functions (the curve ops' evaluators, gpu/ops.rs).
        for line in ops::helpers(self, &d, uniform_bindings) {
            writeln!(code, "{line}").unwrap();
        }

        // Color transform function
        writeln!(code, "vec4 ocio_transform(vec4 color) {{").unwrap();

        for line in ops::body(self, &d, uniform_bindings) {
            writeln!(code, "    {line}").unwrap();
        }

        writeln!(code, "    return color;").unwrap();
        writeln!(code, "}}").unwrap();
        writeln!(code).unwrap();

        // Main function
        let input = d.sample("u_inputTexture", GpuTextureType::Texture2D, "v_texCoord");
        writeln!(code, "void main() {{").unwrap();
        writeln!(code, "    vec4 color = {input};").unwrap();
        if d.legacy_glsl() {
            writeln!(code, "    gl_FragColor = ocio_transform(color);").unwrap();
        } else {
            writeln!(code, "    fragColor = ocio_transform(color);").unwrap();
        }
        writeln!(code, "}}").unwrap();
    }
}
