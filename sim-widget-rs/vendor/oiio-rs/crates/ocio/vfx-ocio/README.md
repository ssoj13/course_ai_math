# vfx-ocio

A Rust implementation of OpenColorIO: config loading, a CPU processor, and GPU shader generation.
The reference is OpenColorIO 2.5 (`C:\projects\projects.rust.cg\vfx.ref\OpenColorIO`), and the
numbers are checked against PyOpenColorIO 2.5.1.

## What it does

- Loads `.ocio` configs (YAML) and `.ocioz` archives, and writes configs back out
  (`Config::serialize`, `config/saving.rs`: OCIO's section order, tags and keys, every transform;
  the reader and writer use exactly OCIO's keys, value shapes and defaults; every built-in OCIO
  config round-trips to text PyOpenColorIO re-serializes identically. Inline LUTs are refused, as
  OCIO's YAML has none). Configs cover colour spaces, roles, displays/views, looks, view
  transforms, named transforms, file rules, viewing rules and context variables.
  `validate_config` reports problems as a list of issues instead of stopping at the first error.
- Builds a `Processor` from a config (`config.processor(src, dst)`, `display_processor`) or from
  a single `Transform` (`Processor::from_transform`). It applies to RGB/RGBA `f32` pixels and to
  packed or planar buffers of other bit depths.
- Supports every OCIO transform type: Matrix, CDL, Log/LogAffine/LogCamera, Exponent,
  ExponentWithLinear, Range, Allocation, Lut1D/Lut3D, FileTransform, ExposureContrast,
  FixedFunction (including ACES 2.0), Grading Primary/Tone/RGBCurve/HueCurve, Builtin,
  BuiltinTransfer, ColorSpace/Look/DisplayView and Group.
- Supports all 98 OCIO builtin transform styles (`BuiltinTransform`). Style names are matched
  case-insensitively and otherwise exactly; unknown names are an error, as in OCIO.
  `builtin::transforms::styles()` lists them.
- FileTransform formats: `.cube`, `.3dl`, `.csp`, `.spi1d`, `.spi3d`, `.clf`, `.ctf`, `.cc`,
  `.ccc`, `.cdl` (parsed by `vfx-lut`).
- File rules follow OCIO's `FileRules.cpp`. A Basic rule (pattern + extension) compiles to one
  anchored regex `^(pattern)(\.extension)$`. A Regex rule must match the whole path. The
  `ColorSpaceNamePathSearch` rule (`FileRuleKind::PathSearch`) takes the colour space from the
  path itself.
- Generates shaders (`GpuProcessor::from_processor(..)?.generate_shader(language)`) in GLSL,
  HLSL, Metal and WGSL. Requesting Cg or OSL produces GLSL 3.30 output instead. The ACES 2.0 ops
  are emitted as in-shader CAM16/JMh math over an unbounded domain, with no baked LUT.
- Bakes LUTs (`Baker`).
- Embeds every built-in config OCIO 2.5 ships (`builtin::embedded`): the ACES CG and the ACES
  Studio config of each `OpenColorIO-Config-ACES` release (ACES 1.3 v1.0.0 / v2.1.0 / v2.2.0,
  ACES 2.0 v4.0.0), OCIO's own text as PyOpenColorIO 2.5.1 hands it out (8 files, 377 KB), plus
  the Studio all-views v4.0.0 config (93 KB, not an OCIO built-in). `Config::from_file` takes
  `ocio://` URIs like OCIO (`ocio://default` and `ocio://cg-config-latest` = the ACES 2.0 CG
  config, `ocio://studio-config-latest`, `ocio://<name>`; `Config::resolve_config_path` is
  OCIO's `ResolveConfigPath`); `Config::create_from_builtin_config(name)` too.
  `builtin::default_config()` is `ocio://default`. Each CG config is a subset of the Studio
  config of the same release (same colour spaces, roles, views, named transforms; tested).
  `tests/data/gen_ocio_configs.py` rewrites `src/builtin/data/` from PyOpenColorIO.
- Discovers monitor ICC profiles (`SystemMonitors`) on Windows (GDI `GetICMProfileW`) and macOS
  (ColorSync). Linux has no standard API; pass the profile path to
  `Config::instantiate_display_from_icc_profile(path)`, which returns the new display's index.

## Cargo features

| Feature | Default | What it adds |
|---|---|---|
| `gpu` | no | Runs the emitted WGSL on the shared wgpu device from `gpu-info-rs`: `apply_gpu`, `apply_gpu_texture`, `GpuBufferExecutor`, `GpuTextureExecutor` |
| `parallel` | no | `Processor::apply_rgb`/`apply_rgba` split their pixel loop across threads with rayon |

Shader text generation does not need the `gpu` feature.

### GPU host API — running the emitted WGSL on YOUR OWN `wgpu::Device`

`apply_gpu`/`GpuBufferExecutor` etc. run on `gpu-info-rs`'s cluster-wide shared device. A host with
its own device (a viewer's `egui_wgpu`/`wgpu` surface, a DCC's render graph) does not use those —
it needs the WGSL text and OCIO's bind groups, built on ITS device, with ITS own entry point and
its own `@group(0)`/`@group(3)`+ resources alongside OCIO's. Three pieces, all `gpu`-feature-gated
except the first:

- `GpuProcessor::wgsl_prelude()` — the colour maths only (uniform decls, LUT texture bindings,
  helper fns, `fn ocio_transform(vec4<f32>) -> vec4<f32>`), no entry point. Append your own compute
  shell directly (no string search/splice). No `gpu` feature needed — it returns a `String`.
- `GpuShaderCode::binding_contract()` / `OCIO_UNIFORM_GROUP` (1) / `OCIO_TEXTURE_GROUP` (2) /
  `OCIO_HOST_GROUP_START` (3) — the published, DATA form of what `@group`/`@binding` the prelude
  declares (only what's actually non-empty: no dynamic uniforms → no `@group(1)` at all). Start
  your own bindings at group 3 or later.
- `GpuOcioResources` (`gpu` feature) — builds the real `wgpu::Buffer`/`wgpu::Texture`/
  `wgpu::BindGroup` resources for `@group(1)`/`@group(2)`: `for_pipeline(device, queue, &code,
  &pipeline)` if you build your pipeline with `layout: None` (auto/reflection layout), or
  `build(device, queue, &code)` if you want the owned `wgpu::BindGroupLayout`s to assemble an
  explicit `wgpu::PipelineLayout` yourself (for a compute shader); `build_for(device, queue, &code,
  wgpu::ShaderStages::FRAGMENT)` gives the same layouts for a render pipeline that calls
  `ocio_transform` from its fragment shader (a LUT-bearing transform then needs
  `wgpu::Features::FLOAT32_FILTERABLE`). `update_scalars(queue, &code)` re-uploads
  `ocio_scalars` after you change a dynamic property via `GpuShaderCode::set_uniform_value` (or use
  `GpuBufferExecutor`/`GpuTextureExecutor::set_uniform_value`, which do this on the shared device).

This is the SAME builder `gpu::runtime::CompiledOcio` uses internally — one implementation, not a
second copy per host. See `tests/gpu_host_api.rs` for a complete worked example (own entry point,
own `@group(3)` uniform, both the auto-layout and explicit-layout pipeline-building styles) and
`src/gpu/contract.rs`'s module docs for the OCIO `GpuShaderDesc` mapping.

## Example

```rust
use vfx_ocio::Config;

let config = Config::from_file("config.ocio")?;
let processor = config.processor("ACEScg", "sRGB - Display")?;
let mut pixels = [[0.18_f32, 0.18, 0.18]];
processor.apply_rgb(&mut pixels);
```

`Config` and `Processor` fields are private. Use the accessors (`colorspaces()`, `displays()`,
`looks()`, `description()`, ...).

## Tests

```
cargo test -p vfx-ocio                   # unit tests + integration tests
cargo test -p vfx-ocio --features gpu    # adds the wgpu runtime tests
```

- `tests/builtin_parity_table.rs` checks every builtin style against a PyOpenColorIO table
  (`tests/data/ocio_builtins_pyocio.json`, generated by `tests/data/gen_ocio_builtins.py`).
- `tests/ocio_config_yaml_parity.rs` checks the embedded configs against PyOpenColorIO
  (`tests/data/ocio_configs_pyocio.json`, `gen_ocio_configs.py`): the registry and each text,
  `ocio://` URIs, CG-subset-of-Studio, aliases and interop IDs, every conversion, `serialize`.
- Unit tests use the all-views config (`builtin::aces_2_0_studio_all_views()`) explicitly.
- `tests/red_gate_*.rs` hold values ported from OCIO's own C++ tests (ACES 2.0, grading, matrix
  convention).
- `tests/aces2_gpu_emission.rs` runs in the default test run. It compiles the emitted WGSL with
  naga and runs it on the GPU. Without a GPU adapter the live cases report SKIPPED through
  `vfx_core::test_support`; with `VFX_REQUIRE_GPU=1` a missing adapter fails them instead.
- The `--features gpu` tests (`gpu_runtime_parity.rs`, `gpu_builtin_parity.rs`,
  `gpu_ff_curve_parity.rs`, `gpu_moncurve_parity.rs`) compare GPU output with the CPU. They
  expect a GPU and fail without one.
- `tests/gpu_host_api.rs` (`--features gpu`) exercises the GPU HOST API above from a from-scratch
  host pipeline (own entry point, own `@group(3)` uniform, both the auto-layout and
  explicit-`PipelineLayout` styles) against the CPU oracle.

Test tooling (oracle, feature matrix, env vars) is described in
[`docs/testing.md`](../../../docs/testing.md). Open work and known gaps are in
[`docs/ROADMAP.md`](../../../docs/ROADMAP.md).

## Dependencies

Uses `vfx-lut` (LUT files), `vfx-transfer` (transfer curves), `vfx-icc` (monitor ICC profiles),
`vfx-math` and `vfx-core`. Used by `vfx-io` (optional `ocio` feature) and `vfx-ops`.
