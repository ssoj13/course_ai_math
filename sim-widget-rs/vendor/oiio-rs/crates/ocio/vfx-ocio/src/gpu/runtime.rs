//! GPU colour-processing RUNTIME: executes vfx-ocio's own emitted WGSL on the cluster's shared
//! wgpu device, applying an OCIO [`Processor`]'s colour transform to an RGBA-f32 image on-GPU.
//!
//! This promotes the compute harness that previously lived only in `tests/aces2_gpu_emission.rs`
//! into production: [`GpuProcessor::from_processor_wgsl`] EMITS the WGSL + describes the bindings;
//! this module is the piece that actually RUNS it.
//!
//! # Shared-device adoption (WHY)
//! This module opens NO [`wgpu::Instance`] and calls NO `request_adapter`/`request_device`. It
//! ADOPTS the one process-wide device negotiated exactly once by `gpu-info-rs`
//! ([`gpu_mem::shared_device`]) — the same context vfx-compute and jph-wgpu adopt — so the whole
//! cluster runs on ONE physical GPU (zero-copy interop, no split devices, and, crucially, no rogue
//! adapter negotiation). `wgpu` itself is reached through `gpu_mem::wgpu` (`pub use wgpu`), so the
//! graph carries exactly ONE wgpu — the cluster's.
//!
//! # Adoption is ONE-WAY — order matters (PLAN9.md O8)
//! [`gpu_mem::shared_device`] itself calls `Instance::new` → `request_adapter` → `request_device`
//! exactly once, cached in a process-wide `OnceLock`. **It has no setter and no injection point** —
//! it can only be the FIRST negotiation, never adopt one made elsewhere. `SharedGpu`'s four fields
//! (`instance`/`adapter`/`device`/`queue`) exist for the OTHER direction: a consumer clones them
//! into its OWN adoption entry point (`egui_wgpu::WgpuSetupExisting` for a GUI host, `Gpu::from_parts`
//! for a compute consumer such as jph-wgpu).
//!
//! **Consequence for any GUI host (playa's eframe, in particular): it MUST be constructed FROM
//! `shared_device()`'s handles, not the other way around.** If eframe negotiates its own device
//! first — which is what `eframe::run_native` does by default, with no prompting to do otherwise —
//! the UI and this module's GPU work end up on two independent physical devices, silently. There is
//! no error, no panic, and no later fix: the `GpuImage`/`wgpu::Texture` this runtime produces simply
//! cannot be handed to a renderer on the other device. The failure surfaces as a wgpu validation
//! error deep in a completely unrelated draw call, far from this ordering mistake.
//!
//! This is why Phase 0 of PLAN9.md exists as a standalone spike BEFORE any other GPU work: it proves
//! the ordering can be done (build eframe FROM `shared_device()`, run one real transform, draw one
//! real frame) before investing in everything that assumes it holds.
//!
//! # Precision (WHY F32)
//! The emitted shader works in f32 (`rgba32float` input/output textures) and the CPU oracle
//! [`Processor::apply_rgba`] is f32, so this runtime takes and returns RGBA `[f32; 4]` pixels with
//! no intermediate quantisation. Callers holding U8/U16 buffers convert to/from f32 outside this
//! function, keeping the GPU path lossless and its output directly comparable to the CPU path
//! (the parity test relies on exactly this equivalence).
//!
//! # Scope
//! Every transform runs fully here, including LUT-bearing ones (`Lut1D`/`Lut3D`, PLAN9.md O1):
//! `@group(2)` textures are uploaded once at compile time by [`CompiledOcio::compile`], same as
//! the `@group(1)` uniform buffers. Only ops with genuinely no WGSL emission fail, with a typed
//! [`OcioError::Transform`] — an honest failure beats a wrong image.

use gpu_mem::wgpu;

use super::host::GpuOcioResources;
use super::{GpuLanguage, GpuProcessor, GpuUniformValue, OCIO_TEXTURE_GROUP, OCIO_UNIFORM_GROUP};
use crate::processor::Processor;
use crate::{OcioError, OcioResult};

/// Rounds `v` up to a multiple of `a` (used for the 256-byte texture->buffer readback row stride).
fn align_up(v: u32, a: u32) -> u32 {
    v.div_ceil(a) * a
}

/// Shared compiled-shader state, independent of any specific call's input/output (PLAN9.md O0):
/// the compute pipeline and the processor's own FIXED bind groups — `@group(1)` (scalars +
/// int/float arrays) and `@group(2)` (LUT textures + samplers, PLAN9.md O1). Built ONCE by
/// [`GpuBufferExecutor::compile`] / [`GpuTextureExecutor::compile`]; every `execute` call after
/// that only creates `@group(0)` (the per-call input/output textures, whose size can vary call to
/// call) and does `set_bind_group` + `dispatch_workgroups` — no `create_shader_module`, no
/// `create_compute_pipeline`, no re-serialising the uniform tables or re-uploading LUTs.
struct CompiledOcio {
    pipeline: wgpu::ComputePipeline,
    /// OCIO's own `@group(1)`/`@group(2)` resources — built by [`GpuOcioResources::for_pipeline`],
    /// the SAME builder an external host uses on its own device. There is no second copy of the
    /// packing/texture-creation logic here anymore (task: item 3).
    resources: GpuOcioResources,
    /// Kept so [`GpuBufferExecutor`]/[`GpuTextureExecutor`] callers can update dynamic uniforms
    /// (exposure/contrast/gamma, grading) via [`GpuOcioResources::update_scalars`] without
    /// re-deriving the shader code.
    code: super::GpuShaderCode,
}

impl CompiledOcio {
    /// Emits `processor`'s WGSL prelude via [`GpuProcessor::wgsl_prelude`] (the PUBLIC host API,
    /// task item 1) and appends `shell` (the caller's own compute-shell text) DIRECTLY — no marker
    /// string, no `.find()` splice (PLAN9.md O0/C6: `apply_gpu_texture` used to search the emitter's
    /// OWN D1-shelled output for `"@group(0) @binding(0) var input_texture:"` and cut there).
    /// Compiles the result once and builds OCIO's `@group(1)`/`@group(2)` resources via
    /// [`GpuOcioResources::for_pipeline`] — the same builder task item 3 publishes for external
    /// hosts, called here with `layout: None` (wgpu's automatic/reflection pipeline layout), which
    /// is what this runtime has always used.
    fn compile(processor: &Processor, shell: &str, label: &'static str) -> OcioResult<Self> {
        let gpu_proc = GpuProcessor::from_processor_wgsl(processor)?;

        // `code` (textures()/uniforms()) is also what `GpuOcioResources::for_pipeline` packs from
        // below — computed once, reused for both the prelude derivation and the resource build.
        let code = gpu_proc.generate_shader(GpuLanguage::Wgsl)?;
        let prelude = gpu_proc.wgsl_prelude();
        let shader_src = format!("{prelude}{shell}");

        // ADOPT the cluster's single shared device — NO Instance/request_adapter here.
        let gpu = gpu_mem::shared_device().ok_or_else(|| {
            OcioError::Gpu(
                "no shared GPU device available (gpu_mem::shared_device() returned None)".into(),
            )
        })?;
        let device = &gpu.device;
        let queue = &gpu.queue;

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let resources = GpuOcioResources::for_pipeline(device, queue, &code, &pipeline);

        Ok(Self {
            pipeline,
            resources,
            code,
        })
    }

    fn bg1(&self) -> Option<&wgpu::BindGroup> {
        self.resources.bind_group(OCIO_UNIFORM_GROUP)
    }

    fn bg2(&self) -> Option<&wgpu::BindGroup> {
        self.resources.bind_group(OCIO_TEXTURE_GROUP)
    }

    /// Task item 4 (dynamic uniform updates): sets `name`'s value in `self.code` and re-uploads
    /// `ocio_scalars` via [`GpuOcioResources::update_scalars`] — the SAME packer
    /// [`GpuOcioResources::build`]/[`GpuOcioResources::for_pipeline`] used at compile time. Returns
    /// `false` (no error) if `name` is not a registered uniform of this processor, matching
    /// [`super::GpuShaderCode::set_uniform_value`]'s own "unknown name" contract.
    fn set_uniform_value(&mut self, name: &str, value: GpuUniformValue) -> OcioResult<bool> {
        if !self.code.set_uniform_value(name, value) {
            return Ok(false);
        }
        let gpu = gpu_mem::shared_device().ok_or_else(|| {
            OcioError::Gpu(
                "no shared GPU device available (gpu_mem::shared_device() returned None)".into(),
            )
        })?;
        Ok(self.resources.update_scalars(&gpu.queue, &self.code))
    }
}

/// A compiled, reusable D1 (host round-trip) executor (PLAN9.md O0). Build once via
/// [`Self::compile`], then call [`Self::execute`] as many times as needed — the shader/pipeline/
/// uniform-buffer cost is paid exactly once, not per call. See [`apply_gpu`] for the one-shot
/// convenience wrapper and the full precision/error contract (identical here).
pub struct GpuBufferExecutor {
    compiled: CompiledOcio,
}

impl GpuBufferExecutor {
    /// Compiles `processor` once.
    ///
    /// # Errors
    /// Same as [`apply_gpu`]'s compile-time errors: [`OcioError::Transform`] for a non-WGSL-native
    /// op, [`OcioError::Gpu`] for a missing shared device.
    pub fn compile(processor: &Processor) -> OcioResult<Self> {
        Ok(Self {
            compiled: CompiledOcio::compile(
                processor,
                &GpuProcessor::wgsl_d1_shell(),
                "vfx-ocio apply_gpu",
            )?,
        })
    }

    /// Updates a dynamic uniform (exposure/contrast/gamma, a GradingPrimary/RgbCurve/HueCurve
    /// field, …) by name and re-uploads `ocio_scalars` immediately — task item 4. `false` if `name`
    /// is not one of this processor's registered uniforms (see [`super::GpuShaderCode::uniforms`]
    /// for the live list); a compile-time [`OcioError::Gpu`] if the shared device has gone away.
    pub fn set_uniform_value(&mut self, name: &str, value: GpuUniformValue) -> OcioResult<bool> {
        self.compiled.set_uniform_value(name, value)
    }

    /// Runs the compiled transform over a `width`×`height` RGBA-f32 image, returning the
    /// transformed pixels. Allocates only the per-call input/output textures and the readback
    /// buffer (their size may differ call to call) — no shader recompile, no pipeline rebuild.
    ///
    /// # Errors
    /// [`OcioError::Gpu`] on a pixel-count mismatch or any wgpu operation (poll, buffer map,
    /// readback) failure. Never `unwrap`s / panics.
    pub fn execute(
        &self,
        width: u32,
        height: u32,
        pixels: &[[f32; 4]],
    ) -> OcioResult<Vec<[f32; 4]>> {
        if width == 0 || height == 0 {
            return Ok(Vec::new());
        }
        let expected = (width as usize) * (height as usize);
        if pixels.len() != expected {
            return Err(OcioError::Gpu(format!(
                "GpuBufferExecutor::execute: pixel count {} != width*height {}",
                pixels.len(),
                expected
            )));
        }

        let gpu = gpu_mem::shared_device().ok_or_else(|| {
            OcioError::Gpu(
                "no shared GPU device available (gpu_mem::shared_device() returned None)".into(),
            )
        })?;
        let device = &gpu.device;
        let queue = &gpu.queue;

        let tex_size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let input_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("apply_gpu input"),
            size: tex_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let output_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("apply_gpu output"),
            size: tex_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        // `[f32; 4]` is `bytemuck::Pod` — reinterpret, don't reassemble byte-by-byte (PLAN9.md C6).
        let input_bytes: &[u8] = bytemuck::cast_slice(pixels);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &input_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            input_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 16),
                rows_per_image: Some(height),
            },
            tex_size,
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());

        let input_view = input_tex.create_view(&Default::default());
        let output_view = output_tex.create_view(&Default::default());
        let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("apply_gpu g0"),
            layout: &self.compiled.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&output_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        // Readback buffer: a texture->buffer copy requires `bytes_per_row` aligned to 256.
        let row_bytes = align_up(width * 16, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("apply_gpu readback"),
            size: (row_bytes as u64) * (height as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.compiled.pipeline);
            pass.set_bind_group(0, &bg0, &[]);
            if let Some(bg1) = self.compiled.bg1() {
                pass.set_bind_group(1, bg1, &[]);
            }
            if let Some(bg2) = self.compiled.bg2() {
                pass.set_bind_group(2, bg2, &[]);
            }
            // workgroup_size(8, 8): one invocation per pixel, tiled 8×8.
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &output_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: Some(height),
                },
            },
            tex_size,
        );
        queue.submit(std::iter::once(encoder.finish()));

        // Map + wait, then read back. Every fallible wgpu 30 call maps to a typed OcioError — no
        // `expect`/panic on the GPU path.
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            // Fire-and-forget: the receiver below always outlives this callback, so the only way
            // `send` can fail is a dropped `rx`, which cannot happen here.
            let _ = tx.send(r);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| OcioError::Gpu(format!("apply_gpu: device poll failed: {e}")))?;
        rx.recv()
            .map_err(|_| OcioError::Gpu("apply_gpu: map callback channel closed".into()))?
            .map_err(|e| OcioError::Gpu(format!("apply_gpu: buffer map failed: {e}")))?;

        let data = slice
            .get_mapped_range()
            .map_err(|e| OcioError::Gpu(format!("apply_gpu: get_mapped_range failed: {e}")))?;
        // Reinterpret each row's real (unpadded) bytes as `[f32; 4]` pixels directly - `row_bytes`
        // may carry trailing padding (256-byte copy alignment), so the cast is per-row (C6).
        let mut out = Vec::with_capacity(expected);
        for y in 0..height as usize {
            let row_start = y * row_bytes as usize;
            let row_end = row_start + (width as usize) * 16;
            let row_pixels: &[[f32; 4]] = bytemuck::cast_slice(&data[row_start..row_end]);
            out.extend_from_slice(row_pixels);
        }
        drop(data);
        readback.unmap();
        Ok(out)
    }
}

/// Applies `processor`'s colour transform to a `width`×`height` RGBA-f32 image on the GPU and
/// returns the transformed pixels (row-major, RGBA-f32, same dims).
///
/// One-shot convenience: compiles `processor` and runs it once. A caller applying the SAME
/// processor to many images (a compositor loop, a batch) should use [`GpuBufferExecutor`] directly
/// and reuse the compiled executor — PLAN9.md O0 exists precisely because this free function used
/// to pay the full shader-compile + pipeline-build cost on every call.
///
/// The transform is emitted as WGSL via the strict [`GpuProcessor::from_processor_wgsl`] path
/// (which errors on any non-WGSL-native op rather than dropping it), compiled as a compute shader,
/// and run on the [shared device](gpu_mem::shared_device). See the module docs for the F32
/// precision choice and the shared-device / single-wgpu rationale.
///
/// # Errors
/// - [`OcioError::Transform`] if the processor contains an op with no WGSL emission.
/// - [`OcioError::Gpu`] if no shared GPU device is available, or if any wgpu operation (poll,
///   buffer map, readback) fails. The GPU path never `unwrap`s / panics.
pub fn apply_gpu(
    processor: &Processor,
    width: u32,
    height: u32,
    pixels: &[[f32; 4]],
) -> OcioResult<Vec<[f32; 4]>> {
    GpuBufferExecutor::compile(processor)?.execute(width, height, pixels)
}

/// Texture-IO compute shell appended to the emitted WGSL prelude by [`apply_gpu_texture`].
///
/// The emitter's own `generate_wgsl` factors the colour maths into `fn ocio_transform(vec4) -> vec4`
/// and then appends a compute shell that samples an `rgba32float` texture (the D1 buffer/readback
/// path). This shell REPLACES that trailing shell only: it reuses the very same `ocio_transform`
/// (no colour maths re-emitted) but binds the cluster's canonical `Rgba16Float` [`gpu_mem::GpuImage`]
/// textures directly — `textureLoad` the input, run `ocio_transform`, `textureStore` the output —
/// with NO host round-trip. The output format is `rgba16float` to match `GpuImage`'s canonical
/// format (a storage texture's WGSL format must equal the bound texture's). `textureLoad` needs no
/// sampler and does no filtering, so it reads the exact resident texel.
const TEXTURE_SHELL_WGSL: &str = "\
@group(0) @binding(0) var in_tex: texture_2d<f32>;
@group(0) @binding(1) var out_tex: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(in_tex);
    if (gid.x >= dims.x || gid.y >= dims.y) { return; }
    let src = textureLoad(in_tex, vec2<i32>(gid.xy), 0);
    let result = ocio_transform(src);
    textureStore(out_tex, vec2<i32>(gid.xy), result);
}
";

/// A compiled, reusable zero-copy texture-IO executor (PLAN9.md O0). Build once via
/// [`Self::compile`], then call [`Self::execute`] (caller-owned output, no allocation) or
/// [`Self::execute_new`] (allocates a fresh output each call, matching [`apply_gpu_texture`]'s
/// shape) as many times as needed — the shader/pipeline/uniform-buffer cost is paid exactly once.
pub struct GpuTextureExecutor {
    compiled: CompiledOcio,
}

impl GpuTextureExecutor {
    /// Compiles `processor` once.
    ///
    /// # Errors
    /// Same as [`apply_gpu_texture`]'s compile-time errors: [`OcioError::Transform`] for a
    /// non-WGSL-native op, [`OcioError::Gpu`] for a missing shared device.
    pub fn compile(processor: &Processor) -> OcioResult<Self> {
        Ok(Self {
            compiled: CompiledOcio::compile(
                processor,
                TEXTURE_SHELL_WGSL,
                "vfx-ocio apply_gpu_texture",
            )?,
        })
    }

    /// [`GpuBufferExecutor::set_uniform_value`]'s sibling for the texture-IO executor — same
    /// contract (task item 4).
    pub fn set_uniform_value(&mut self, name: &str, value: GpuUniformValue) -> OcioResult<bool> {
        self.compiled.set_uniform_value(name, value)
    }

    /// Runs the compiled transform from `input` into the CALLER-OWNED `output` (same dims as
    /// `input`) — no allocation here (PLAN9.md O0: "take the output `GpuImage` as a parameter
    /// instead of allocating it"). Submit-only, no readback — see [`apply_gpu_texture`]'s docs for
    /// the zero-copy / queue-ordering guarantee.
    ///
    /// # Errors
    /// [`OcioError::Gpu`] if no shared GPU device is available.
    pub fn execute(&self, input: &gpu_mem::GpuImage, output: &gpu_mem::GpuImage) -> OcioResult<()> {
        let gpu = gpu_mem::shared_device().ok_or_else(|| {
            OcioError::Gpu(
                "no shared GPU device available (gpu_mem::shared_device() returned None)".into(),
            )
        })?;
        let device = &gpu.device;
        let queue = &gpu.queue;

        // @group(0): input (textureLoad) + output (storage write). No sampler binding.
        let input_view = input.view();
        let output_view = output.view();
        let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("apply_gpu_texture g0"),
            layout: &self.compiled.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&input_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&output_view),
                },
            ],
        });

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.compiled.pipeline);
            pass.set_bind_group(0, &bg0, &[]);
            if let Some(bg1) = self.compiled.bg1() {
                pass.set_bind_group(1, bg1, &[]);
            }
            if let Some(bg2) = self.compiled.bg2() {
                pass.set_bind_group(2, bg2, &[]);
            }
            // workgroup_size(8, 8): one invocation per texel, tiled 8×8.
            pass.dispatch_workgroups(input.width().div_ceil(8), input.height().div_ceil(8), 1);
        }
        // Submit only — NO readback. The transformed pixels stay resident in `output`'s texture;
        // queue ordering guarantees this compute completes before any later same-queue consumer (a
        // display / encode pass, or a test's own read_rgba_f32) touches it — the zero-copy boundary.
        queue.submit(std::iter::once(encoder.finish()));
        Ok(())
    }

    /// [`Self::execute`], allocating a fresh output [`gpu_mem::GpuImage`] (canonical `Rgba16Float`,
    /// same dims as `input`) each call. Convenience for callers that don't want to manage the
    /// output's lifetime themselves — matches [`apply_gpu_texture`]'s exact shape.
    ///
    /// # Errors
    /// [`OcioError::Gpu`] if no shared GPU device is available or the output image cannot be
    /// allocated.
    pub fn execute_new(&self, input: &gpu_mem::GpuImage) -> OcioResult<gpu_mem::GpuImage> {
        let output = gpu_mem::GpuImage::new(input.width(), input.height()).map_err(|e| {
            OcioError::Gpu(format!(
                "GpuTextureExecutor::execute_new: GpuImage::new failed: {e}"
            ))
        })?;
        self.execute(input, &output)?;
        Ok(output)
    }
}

/// Apply `processor`'s colour transform from `input` to a NEW output [`gpu_mem::GpuImage`], entirely
/// on the GPU (`textureLoad` → emitted OCIO colour → `textureStore`) — no host round-trip. Both
/// textures are the canonical `Rgba16Float` on the shared device. This is the zero-copy path for the
/// media hub: a decoder's resident texture (jph-wgpu / exv) → colour-managed texture → display /
/// encode, with no CPU copy.
///
/// One-shot convenience: compiles `processor` and runs it once. A caller applying the SAME
/// processor across frames (the actual compositor loop this exists for) should use
/// [`GpuTextureExecutor`] directly and reuse the compiled executor — PLAN9.md O0 exists precisely
/// because this free function used to rebuild the shader, pipeline AND uniform buffers on every
/// single call, which is what made it unusable at frame rate.
///
/// The colour maths is NOT re-emitted: the WGSL is produced by the strict
/// [`GpuProcessor::from_processor_wgsl`] path (which errors on any non-WGSL-native op rather than
/// dropping it), and its `ocio_transform` prelude ([`GpuProcessor::generate_wgsl_prelude`]) is
/// reused under a texture-IO shell ([`TEXTURE_SHELL_WGSL`]) whose only substantive difference from
/// the D1 [`apply_gpu`] shell is texture-in/texture-out over the canonical `Rgba16Float` format
/// instead of a host buffer round-trip. The [shared device](gpu_mem::shared_device) is adopted,
/// never re-negotiated.
///
/// # Errors
/// - [`OcioError::Transform`] if the processor contains an op with no WGSL emission.
/// - [`OcioError::Gpu`] if no shared GPU device is available, if the output image cannot be
///   allocated, or if any wgpu operation fails. The GPU path never `unwrap`s / panics.
pub fn apply_gpu_texture(
    processor: &Processor,
    input: &gpu_mem::GpuImage,
) -> OcioResult<gpu_mem::GpuImage> {
    GpuTextureExecutor::compile(processor)?.execute_new(input)
}
