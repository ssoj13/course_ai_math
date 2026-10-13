# vfx-ocio GPU shader emission (`gpu.rs` + `gpu/`)

`gpu.rs` turns a `Processor`'s `ProcessorOp`s into `GpuOp`s (`convert_op`) and builds their
uniforms (`build_uniforms`). Every op body is written once for every language, as OCIO writes
each `*OpGPU.cpp` once over `GpuShaderText`: `ops.rs` (`body` / `emit` / `helpers`) dispatches
to the op modules listed below, all over `Dialect`. `glsl.rs` (every GLSL version), `hlsl.rs`,
`metal.rs` (OCIO's `MetalShaderClassWrapper` struct) and `wgsl.rs` are only the shells around
`ops::body`: declarations, uniforms, textures, entry point. No colour maths lives in them.
`gpu/` also holds `dispatch.rs` (`GpuProcessor::generate_shader`), `corpus.rs` (every op x
style x direction, and the compiler gate),
`contract.rs` (the published bind-group contract, no `gpu` feature needed), `host.rs`
(`GpuOcioResources`, the `wgpu` resource builder, `gpu` feature only), `runtime.rs` (wgpu
execution on the shared device, `gpu` feature only) and `tests.rs`. Notes that apply to the whole
crate (reference location, the CPU side, the twin rule) are in `../processor/AGENTS.md`.
Open GPU items are in `docs/ROADMAP.md` (see the last section here).

## GPU host API (external hosts — vfx-view, usd-gl-engine, anything with its OWN `wgpu::Device`)

`GpuProcessor::wgsl_prelude()` (no `gpu` feature needed) returns the colour maths with NO entry
point; a host appends its own shell text directly — no `.find("@group(0)")` splice. `contract.rs`
publishes WHERE OCIO's own resources live (`OCIO_UNIFORM_GROUP` = 1, `OCIO_TEXTURE_GROUP` = 2,
`OCIO_HOST_GROUP_START` = 3, `GpuShaderCode::binding_contract()`) as DATA, derived from the same
`uniforms()`/`textures()` `wgsl.rs`'s emitters read — never hand-copy these numbers into a second
description. `host.rs`'s `GpuOcioResources` is the ONE place that packs scalar/array uniforms and
uploads LUT textures into real `wgpu` resources; `runtime.rs`'s `CompiledOcio::compile` calls
`GpuOcioResources::for_pipeline` — there is no second inline copy of that packing logic anymore.
A change to the packing layout, texture creation, or LUT sampler settings belongs in `host.rs`
ONLY; `runtime.rs` and any external host both go through it. See `README.md`'s "GPU host API"
section and `tests/gpu_host_api.rs` for the consumer-facing walkthrough and a worked example.

## Conventions

- **Write colour math once.** `Dialect` (`dialect.rs`) names the surface differences between the
  languages (vector type, splat, `let`/`var`, typed declarations, `mix`/`lerp`, swizzle
  assignment, ternary/`select`, uniform reads, texture declarations and samples per GLSL
  version). Each op module is a port of the OCIO `*OpGPU.cpp` of the same name, statement for
  statement:
  - `ops.rs`: Matrix, CDL, Exponent, Log, LogAffine, Range, Lut1D, ExposureContrast,
    Allocation, and the dispatch of everything else.
  - `primary.rs`: GradingPrimary (`GradingPrimaryOpGPU.cpp`).
  - `tone.rs`: GradingTone (`GradingToneOpGPU.cpp`).
  - `grading.rs`: GradingRgbCurve and GradingHueCurve with their B-spline evaluators, and OCIO's
    `AddLinToLogShader` / `AddLogToLinShader` (`lin_to_log` / `log_to_lin`).
  - `fixed_function.rs`: FixedFunction (`FixedFunctionOpGPU.cpp`); the styles below have their
    own modules.
  - `transfer.rs`: `TransferStyle` curves (`BuiltinTransfer`). `emit()` matches every style with
    no catch-all, so a new style is a compile error rather than a silent no-op.
  - `moncurve.rs`: `ExponentWithLinear` (OCIO moncurve), alpha included when its alpha
    parameters are not OCIO's identity 1 / 0.
  - `gamma.rs`: the one `Exponent` body (RGB + alpha with `value[3]`).
  - `ff_curves.rs`: the PQ, gamma-log and double-log FixedFunctions.
  - `pq.rs`: THE PQ encode and decode rationals (`vfx_transfer::pq`'s `oetf_ratio` /
    `eotf_ratio`). `transfer.rs` (ST 2084, absolute nits, clamped like the CPU) and
    `ff_curves.rs` (OCIO LIN_TO_PQ/PQ_TO_LIN, mirrored, unclamped) wrap them; do not write PQ
    again anywhere else.
  - `log_camera.rs`: `LogCamera`.
  - `gamut_comp.rs`: ACES 1.3 gamut compression (port of OCIO `Add_GamutComp_13_Shader`).
  - `lut3d.rs`: the Lut3D body (OCIO's tetrahedral cases over NEAREST fetches, or trilinear at
    the texel centre); `GpuOp::Lut3D.tetrahedral` picks it and the sampler filter.
  - `lut1d_half.rs`: the half-domain Lut1D (half code rebuilt in-shader, OCIO's padded 2D
    texture), used for half-domain tables and every baked 1D inverse.
  - `hsy.rs`, `aces2.rs` and `Dialect::lut1d_texel_center_remap`.

  A new op gets an arm in `ops::emit` and, if its body is long, a module of its own. Never write
  an op body in a shell file.
- **Parameters come from the CPU's own derivation, never retyped.** `moncurve.rs` uses
  `processor::op::moncurve_params` (port of `GammaOpUtils.cpp` `ComputeParamsFwd/Rev`);
  `ff_curves.rs` uses `GammaLogParams`/`DoubleLogParams` and the PQ constants from
  `vfx_transfer::pq`; `gamut_comp.rs` uses `GamutComp13Params`; `GpuOp::LogCamera` carries the
  toe offset and the encoded-domain break that `compile.rs` derives. The ACES 2.0 uniforms are
  copied from the precomputed CPU `aces2` state in `build_uniforms`. A literal typed into an
  emitter will drift from the CPU.
- **Matrices.** `GpuOp::Matrix` carries `vfx_math::layout::Mat4<RowMajor>`; emitters reach the
  upload order only through `to_col_major()`. Do not index `matrix[0], matrix[4], ...` by hand.
  All four backends want the same array for opposite reasons: GLSL/Metal/WGSL constructors are
  column-major and apply `m * v`; HLSL's `float3x3` reads rows but `mul(v, m)` is `m^T * v`, so
  it needs `M^T`, whose rows are M's columns.
- **An op the GPU cannot express is an error, not a dropped op.** `from_processor` and
  `from_processor_wgsl` both error with the op named. Example: a half-domain `Lut1d` with the
  DW3 hue adjust (`convert_op` returns `None`).
- **Inverse CDL.** `ProcessorOp::Cdl { inverse: Some(seq) }` is converted as its op sequence
  (`GpuProcessor::gpu_sequence`), ops every emitter has; there is no reverse-CDL shader body.
- **Lut3D order.** `ProcessorOp::Lut3d.lut` is Red-fastest (`vfx_lut::Lut3D` is Blue-fastest;
  `compile.rs` converts through `vfx_lut::OrderedLut3dData`). All four emitters sample the
  texture at `t` directly, with no swizzle swap.
- **ACES 2.0.** The four ACES 2.0 ops emit CAM16/JMh math in the shader over an unbounded
  domain, mirroring OCIO `FixedFunctionOpGPU.cpp`. There is no baked LUT, so HDR input above 1.0
  and JMh inputs are exact. `ProcessorOp::FixedFunction` with an ACES 2.0 style is routed
  through `processor::op::fixed_function_to_aces2_op` to the `GpuOp::Aces2*` ops.

## Verifying an emitter

- **Every language is compiled.** `corpus.rs` builds every op x style x direction;
  `emitted_shaders_compile` (ignored without the tools; set `OCIO_RS_GLSLANG` to
  `glslangValidator` and `OCIO_RS_FXC` to `fxc.exe`) compiles every entry in GLSL 1.20, 1.30,
  3.30, 4.00, ES 1.00, ES 3.00, Vulkan 4.60 (to SPIR-V, `-V`) and HLSL SM 5.0; `wgsl_validates`
  runs naga's validator on every entry's WGSL (always). Metal has no compiler on Windows.
  `write_snapshot` (`OCIO_RS_SHADER_SNAPSHOT`) writes every entry in every language for diffing
  a refactor.
- **WGSL runs.** The `--features gpu` tests execute WGSL on wgpu against the CPU processor:
  `gpu_grading_parity` (primary, tone, both curves, every FixedFunction style, every style and
  direction), `gpu_runtime_parity`, `gpu_builtin_parity`, `gpu_ff_curve_parity`,
  `gpu_moncurve_parity`, `aces2_gpu_emission`. Since the bodies are shared, a WGSL run checks the
  maths of all languages; the compile gate checks their spelling.
- Use asymmetric data. An identity or symmetric 3D LUT cannot tell a swapped axis order from a
  correct one.
- A test that re-implements the emitted expression in Rust shows self-consistency, not parity
  with OCIO. Say so in the test.

## Traps

- **The reference is OCIO, not our CPU.** For most past findings the CPU was right and the GPU
  wrong, but several were the other way round (GradingRGBCurve, the hue-curve inverse, HSY).
  Compare both sides with `vfx.ref/OpenColorIO` before deciding which one to change.
- **`-0.0` differs.** The CPU honours the sign bit (`copysign`); a shader cannot without bit
  casts. `ff_curves.rs` uses `copysign(1, x)` semantics (`copysign_one`), not `sign()`, which is 0
  at the mirror point.
- **HueCurve Video luma gain (EMIT-14) diverges from OCIO on purpose.** Upstream's CPU keys the
  additive-vs-multiplicative gain on `m_isLinear`, its GPU on `style == GRADING_LOG`, so the two
  disagree for Video. We key on `!Linear` on both sides (CPU `grading_hue_curve.rs`, every
  emitter); gate `tests/red_gate_hue_curve_video.rs`. Read the comment on `grading::hue_curve`
  before "fixing" it to match either upstream backend.
- **Cg and OSL** requests are `OcioError::Gpu` (`dispatch.rs`, OCIO-10 fixed 2026-09-23).
- `fixed_function::emit` panics (`unreachable!`) on an ACES 2.0 style: `convert_op` routes them
  through `fixed_function_to_aces2_op` to the `GpuOp::Aces2*` ops, never to
  `GpuOp::FixedFunction`.
- **Legacy GLSL (1.20, ES 1.00) has no integer `min`/`max` and no implicit int conversion.**
  Write `int(max(float(a), float(b)))` as OCIO does, or keep the index a float
  (`fixed_function::hue_weight`).

**OCIO-M9 remainder (2026-09-23): the Lut1D input domain is per channel on the GPU**
(`GpuOp::Lut1D` carries `[f32; 3]`; `Dialect::lut1d_domain_t` is the one sample-coordinate
expression for all four emitters, a zero-width channel samples entry 0 like the CPU).

**OCIO-M4 GPU half (2026-09-23): ExposureContrast.** The pivot is floored at `MIN_PIVOT` in
`convert_op` (the one entry to the GPU path). WGSL now reads exposure/contrast/gamma from the
uniforms `build_uniforms` registers: a registered uniform the WGSL body never reads makes naga's
auto layout drop the buffer, and the runtime's `create_bind_group` then fails. Any new op that
registers uniforms must read them in its WGSL body. HLSL and Metal read them too (all four
emitters agree). Metal uniforms are members of the `constant OcioUniforms& uniforms` argument:
write `uniforms.<name>`, never the bare name (`hlsl_and_metal_read_every_uniform_they_declare`).

**ExposureContrast, contrast*gamma == 1 (2026-10-08).** OCIO scales (`rgb * exposure`) and raises
about the pivot only `if (contrast != 1.0)` (`ExposureContrastOpGPU.cpp:105-124`), as its CPU renderers do;
an unconditional `pow(max(0, ..))` clamped negatives of an exposure-only grade. `ops::exposure_contrast`
now emits OCIO's `if` in all four languages; gate: `tests/legacy_viewing_pipeline.rs` (GPU test with
sub-toe ACEScct pixels, `gpu` feature) and the PyOCIO ACEScct cases. Uniform names are
`ocio_u<op>_exposure|contrast|gamma`; every EC op has them, so use `GpuProcessor::dynamic_uniforms(prop)`
to find the ones an `EcDynamic` flag made the host's dial (the uniforms are FORWARD values in both directions: an inverse op gets OCIO's reverse shader, `AddECLinearRevShader` and friends, fed the same numbers; gates: the inverse and host-path tests in `tests/legacy_viewing_pipeline.rs`) (a pipeline's display-side gamma op has an
`_exposure` uniform too).

**EMIT-12 (2026-09-23): GradingPrimary's local bypass is OCIO's**, computed once
(`processor::grading_primary_local_bypass`, from the direction-resolved params and the raw
saturation/clamps) and carried on `GpuOp::GradingPrimary`; the emitters read it through the
`local_bypass` uniform. Do not recompute it in `gpu.rs`: the old copy used tolerances, compared
the `2^stops` exposure multiplier to 0 (never bypassed) and ignored the clamps.

## Open items

Tracked in `docs/ROADMAP.md`: the P3 items EMIT-18, ACES2-05, PLAN9 C3 and PLAN10 §6, and an
OSL dialect (to be validated with osl-rs).
