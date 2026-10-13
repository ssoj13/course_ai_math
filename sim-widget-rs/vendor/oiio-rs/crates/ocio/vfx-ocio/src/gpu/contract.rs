//! The published, data-form description of the bind groups vfx-ocio's emitted WGSL uses.
//!
//! # Why this exists
//! The bind-group layout `wgsl.rs` emits (`@group(1)`/`@group(2)` bindings, PLAN9.md O0/O1) used to
//! live only as prose in doc comments and as matching-by-hand code in `gpu/runtime.rs`'s
//! `CompiledOcio::compile`. Any external host that runs the emitted WGSL on its OWN `wgpu::Device`
//! (vfx-view's `OcioGpuPass`, usd-gl-engine's `from_processor_wgsl` path) had no way to ask "what
//! did OCIO actually bind, and where can I put my own stuff" except by reading `runtime.rs` and
//! copying its numbers by hand — which is exactly how vfx-view's `wrap_ocio_wgsl` ended up
//! declaring its own uniform at `@group(1) @binding(0)`, colliding with `ocio_scalars`.
//!
//! This module makes the contract DATA, derived from the same [`super::GpuShaderCode`] that
//! `wgsl.rs`'s `emit_uniform_decls_wgsl`/`emit_texture_bindings_wgsl` used to decide what to emit —
//! not a second, hand-maintained description that can drift from the real shader text. See
//! `tests.rs`'s `host_contract_matches_wgsl_text` for the test that parses the actual `@group`/
//! `@binding` lines out of emitted WGSL and asserts they match [`GpuShaderCode::binding_contract`].
//!
//! # Mirrors OCIO's `GpuShaderDesc`
//! OCIO's C++ `GpuShaderCreator`/`GpuShaderDesc` exposes the same information to a host through
//! `getNumUniforms`/`getUniform`, `getNumTextures`/`getTexture`, `getNumTexture3D`/`getTexture3D`
//! (see `vfx.ref/OpenColorIO/include/OpenColorIO/OpenColorTransforms.h`,
//! `GpuShaderDesc::Impl` in `src/GpuShaderUtils.cpp`). vfx-ocio has no per-host shader-text
//! accumulation API (OCIO's `addToDeclareShaderCode`/`getShaderText` — every host gets the SAME
//! emitted prelude via [`super::GpuProcessor::wgsl_prelude`], mirroring `getShaderText()`'s
//! finished-text role rather than the incremental-builder one), so this module's job is narrower:
//! publish WHERE (group/binding) and WHAT KIND each OCIO-owned resource is, so a host can bind it.
//! [`super::WGSL_TRANSFORM_FUNCTION`] mirrors `getFunctionName()`.

use super::{GpuShaderCode, GpuTextureType, GpuUniformType};

/// Bind group holding OCIO's own scalar/array uniform storage buffers
/// (`ocio_scalars`/`ocio_arrays_i`/`ocio_arrays_f`, `wgsl.rs::emit_uniform_decls_wgsl`).
///
/// Fixed at 1: the shell (`@group(0)`, the host's own per-call input/output resources) always comes
/// first, and this crate has never emitted anything else at group 1 — kept as a named const rather
/// than a bare `1` so a host's own code documents itself.
pub const OCIO_UNIFORM_GROUP: u32 = 1;

/// Bind group holding OCIO's LUT textures + samplers (`wgsl.rs::emit_texture_bindings_wgsl`,
/// `(idx+1)*2` / `+1` per LUT). Fixed at 2, one past [`OCIO_UNIFORM_GROUP`].
pub const OCIO_TEXTURE_GROUP: u32 = 2;

/// First bind-group index a HOST is free to use for its own resources (its own uniforms, its own
/// input/output textures beyond the D1 shell, …) without colliding with anything vfx-ocio emits.
///
/// This is the number vfx-view's `OcioGpuPass` got wrong: it declared a host uniform at
/// `@group(1) @binding(0)`, which is exactly [`OCIO_UNIFORM_GROUP`]'s `ocio_scalars` slot. A host
/// shell should start its own group numbering here, e.g. `@group(3) @binding(0)`.
pub const OCIO_HOST_GROUP_START: u32 = 3;

/// The kind of WGSL resource a [`GpuBindingDesc`] describes — mirrors the handful of WGSL resource
/// binding types this crate's emitter ever declares (`wgsl.rs`'s `var<storage, read>` buffers and
/// `texture_{1d,2d,3d}<f32>` + `sampler` pairs). Not a general WGSL binding-kind enum: it only needs
/// to name what vfx-ocio itself emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuBindingKind {
    /// `var<storage, read> ocio_scalars: array<u32>` — every scalar uniform (Float/Bool/Int/Vec3/
    /// Vec4/Mat3/Mat4), bit-packed in registration order. Present only when at least one scalar
    /// uniform is registered (`wgsl.rs`'s `s_offset > 0`).
    ScalarStorageBuffer,
    /// `var<storage, read> ocio_arrays_i: array<i32>` — every `ArrayInt` uniform's payload,
    /// concatenated. Present only when at least one is registered.
    IntArrayStorageBuffer,
    /// `var<storage, read> ocio_arrays_f: array<f32>` — every `ArrayFloat` uniform's payload,
    /// concatenated. Present only when at least one is registered.
    FloatArrayStorageBuffer,
    /// A LUT `texture_1d<f32>` / `texture_2d<f32>` / `texture_3d<f32>`, one per [`super::GpuTexture`]
    /// in [`GpuShaderCode::textures`].
    LutTexture(GpuTextureType),
    /// The `sampler` paired with a LUT texture at `binding + 1`.
    LutSampler,
}

/// One `@group(N) @binding(M)` resource OCIO's emitted WGSL declares, and what it is.
#[derive(Debug, Clone)]
pub struct GpuBindingDesc {
    /// The WGSL name the resource is declared under (`ocio_scalars`, or the LUT's own
    /// [`super::GpuTexture::name`] / `"{name}_sampler"`).
    pub name: String,
    /// `@group(N)` — always [`OCIO_UNIFORM_GROUP`] or [`OCIO_TEXTURE_GROUP`] today.
    pub group: u32,
    /// `@binding(M)`.
    pub binding: u32,
    /// What kind of resource this is.
    pub kind: GpuBindingKind,
}

/// The full, ordered list of `@group`/`@binding` resources a [`GpuShaderCode`] declares — the
/// published form of the layout `runtime.rs`'s `CompiledOcio::compile` used to build purely from
/// prose + hand-matched code. Empty groups genuinely emit no bindings at all (`wgsl.rs` omits an
/// empty `array<u32>` decl, since WGSL forbids it and an unused binding would force a host to bind a
/// dummy buffer) — [`Self::bindings`] reflects that: a processor with no dynamic uniforms and no
/// LUTs returns an empty list, and a host must not try to bind group 1/2 at all in that case.
#[derive(Debug, Clone, Default)]
pub struct GpuBindingContract {
    /// Every OCIO-declared binding, in ascending `(group, binding)` order.
    pub bindings: Vec<GpuBindingDesc>,
}

impl GpuBindingContract {
    /// True if the shader declares `@group(1)` at all (any scalar or array uniform storage buffer).
    pub fn has_uniform_group(&self) -> bool {
        self.bindings.iter().any(|b| b.group == OCIO_UNIFORM_GROUP)
    }

    /// True if the shader declares `@group(2)` at all (at least one LUT texture).
    pub fn has_texture_group(&self) -> bool {
        self.bindings.iter().any(|b| b.group == OCIO_TEXTURE_GROUP)
    }

    /// Every binding declared within `group`, in ascending binding order.
    pub fn group(&self, group: u32) -> impl Iterator<Item = &GpuBindingDesc> {
        self.bindings.iter().filter(move |b| b.group == group)
    }
}

impl GpuShaderCode {
    /// The published, data-form binding contract for this shader (see the module docs). Derived
    /// straight from [`Self::uniforms`]/[`Self::textures`] — the SAME data `wgsl.rs`'s
    /// `emit_uniform_decls_wgsl`/`emit_texture_bindings_wgsl` read to decide what to emit — so this
    /// can never describe a binding the WGSL text does not actually declare, or vice versa.
    pub fn binding_contract(&self) -> GpuBindingContract {
        let mut bindings = Vec::new();

        // @group(1): scalars (binding 0), arrays_i (binding 1), arrays_f (binding 2) — present iff
        // at least one uniform of the matching category is registered. Mirrors `wgsl.rs`'s
        // `s_offset`/`i_base`/`f_base` > 0 checks exactly (those are u32-slot counters; presence,
        // not the count, is all this contract needs).
        let has_scalar = self.uniforms.iter().any(|u| {
            !matches!(
                u.uniform_type,
                GpuUniformType::ArrayFloat | GpuUniformType::ArrayInt
            )
        });
        let has_arrays_i = self
            .uniforms
            .iter()
            .any(|u| u.uniform_type == GpuUniformType::ArrayInt);
        let has_arrays_f = self
            .uniforms
            .iter()
            .any(|u| u.uniform_type == GpuUniformType::ArrayFloat);
        if has_scalar {
            bindings.push(GpuBindingDesc {
                name: "ocio_scalars".to_string(),
                group: OCIO_UNIFORM_GROUP,
                binding: 0,
                kind: GpuBindingKind::ScalarStorageBuffer,
            });
        }
        if has_arrays_i {
            bindings.push(GpuBindingDesc {
                name: "ocio_arrays_i".to_string(),
                group: OCIO_UNIFORM_GROUP,
                binding: 1,
                kind: GpuBindingKind::IntArrayStorageBuffer,
            });
        }
        if has_arrays_f {
            bindings.push(GpuBindingDesc {
                name: "ocio_arrays_f".to_string(),
                group: OCIO_UNIFORM_GROUP,
                binding: 2,
                kind: GpuBindingKind::FloatArrayStorageBuffer,
            });
        }

        // @group(2): one texture + sampler pair per LUT, at `(idx+1)*2` / `+1` — MUST match
        // `wgsl.rs::emit_texture_bindings_wgsl`'s `binding_base` exactly.
        for (idx, tex) in self.textures.iter().enumerate() {
            let binding_base = ((idx + 1) * 2) as u32;
            bindings.push(GpuBindingDesc {
                name: tex.name.clone(),
                group: OCIO_TEXTURE_GROUP,
                binding: binding_base,
                kind: GpuBindingKind::LutTexture(tex.texture_type),
            });
            bindings.push(GpuBindingDesc {
                name: format!("{}_sampler", tex.name),
                group: OCIO_TEXTURE_GROUP,
                binding: binding_base + 1,
                kind: GpuBindingKind::LutSampler,
            });
        }

        GpuBindingContract { bindings }
    }
}

/// The WGSL function name a host's own compute-shell entry point calls after `textureLoad`/
/// `textureSampleLevel`-ing its input — the one name every prelude [`super::GpuProcessor::
/// wgsl_prelude`] emits regardless of processor content. Mirrors OCIO's
/// `GpuShaderDesc::getFunctionName()`.
pub const WGSL_TRANSFORM_FUNCTION: &str = "ocio_transform";
