# vfx-ocio: config, compile and CPU apply

Agent notes for the CPU side of vfx-ocio and for the crate as a whole. GPU shader emission
(`gpu.rs`, `gpu/`) has its own notes in `../gpu/AGENTS.md`. Open work is in `docs/ROADMAP.md`
(see the last section here); finding IDs such as `OCIO-11` resolve in `.bughunt/INDEX.md`.

## The reference tree is outside this repo

`C:/projects/projects.rust.cg/vfx.ref/OpenColorIO/` is a sibling of `cglibs/`, not inside the
repo. Searching near the worktree finds nothing, which makes it look unavailable. It is there.
PyOpenColorIO 2.5.1 is the numeric reference for tables (`tests/data/gen_ocio_builtins.py`).

## Where things live

- `config.rs`, `config/`: `loading.rs` (YAML to `Config`), `parsing.rs`, `types.rs`,
  `factory.rs` (building processors from colour spaces, displays and looks), `tests.rs`;
  `config_builder.rs`, `validate.rs` (issue-collecting port of `Config::validate`),
  `context.rs` (context variables, OCIO's substitution rules).
- `processor.rs`: `Processor`, `optimize`. `processor/compile.rs`: `Transform` to `ProcessorOp`
  (`compile_transform`, `compile_lut3d`, `compile_clf`, FileTransform by extension).
  `processor/op.rs`: the `ProcessorOp` enum and every shared parameter derivation.
  `processor/apply.rs`: CPU evaluation; `apply_packed`/`apply_planar` take byte slices (OCIO's
  packed/planar descriptors as `origin` + byte strides), validated once by `Layout`, then go
  through `load_sample`/`store_sample`. No `unsafe` in this crate. `processor/math.rs`: LUT inversion, half-domain Lut1D indexing,
  transfer-style dispatch. `processor/accessors.rs`: rebuilds `Transform`s from ops for
  introspection.
- `builtin_transforms/`: port of OCIO's `BuiltinTransformRegistry` in registration order. Each
  style expands into ordinary `Transform`s compiled by `compile.rs`, so no op is implemented
  twice. `color_matrix.rs` is an f64 port of `ColorMatrixHelpers.cpp` and the source of every
  builtin matrix; `cameras.rs` carries OCIO's vendor constants.
- `builtin/`: the embedded configs (`embedded.rs`: OCIO's `BuiltinConfigRegistry`, every ACES CG
  and Studio release, plus the all-views config; `ocio://` resolution) and the public
  `builtin::transforms` style list. The `.ocio` files are generated: change them only by
  rerunning `tests/data/gen_ocio_configs.py` (PyOpenColorIO 2.5.1). `default_config()` is
  `ocio://default` (CG); unit tests take the all-views config by name.
- `viewing.rs`: `LegacyViewingPipeline` (OCIO's app helper: `linear_cc` in `scene_linear`, `color_timing_cc`,
  looks, channel view, display/view, `display_cc` -> ONE `Processor`). It only composes existing
  transforms (`ColorSpaceTransform`, `LookTransform`, `DisplayViewTransform`, `Group`) and goes through
  `Config::processor_from_transform_with_context`; `Config::looks_result_space` is
  `LookTransform::GetLooksResultColorSpace` (with `|` fall-back options; it checks looks, process
  spaces and look FILES, it does not compile the look transforms). `Config::find_view` is OCIO's
  `getView` (case-insensitive, not narrowed by `active_views`). Its errors are structured variants
  carrying OCIO's text (`RoleRequired`, `InputColorSpaceNotFound`, `Pipeline`, `LookNotFound`,
  `MissingFile`, `DynamicPropertyNotFound`). Parity: `tests/legacy_viewing_pipeline.rs` against
  `tests/data/ocio_viewing_pipeline_pyocio.json` (regenerate with `gen_ocio_viewing_pipeline.py`).
- **Dynamic properties** (`Processor::set_dynamic` / `dynamic`, in `accessors.rs`): only
  ExposureContrast, via `ExposureContrastTransform::dynamic: EcDynamic` ->
  `ProcessorOp::ExposureContrast {dynamic, forward}`. The op keeps the FORWARD parameters in both
  directions and `forward` picks OCIO's reverse renderer (`apply.rs`) / reverse shader
  (`gpu/ops.rs`), so a dynamic value and a shader uniform mean the same in either direction and
  `ProcessorOp::inverse()` only flips `forward`. A dynamic op is never `is_identity`, never pairs in
  the optimizer, and its value is not in `cache_id`. `has_dynamic_property` / `is_dynamic` follow
  the flags (grading ops carry no flag in this crate, so they are never dynamic). A config
  `ExposureContrastTransform` with a missing exposure/contrast/gamma key loads that parameter
  dynamic and the writer omits dynamic ones (`OCIOYaml.cpp:1273`, `:1298`). `Processor` is `Clone`
  (a host moves a CPU twin without locks). The old `DynamicProcessor` is deprecated, see
  `dynamic.rs`.
- **Known, intended difference from OCIO ("C++ or better").** The Logarithmic REVERSE ExposureContrast
  uses the op's `log_exposure_step`, as OCIO's GPU shader does. OCIO's CPU renderer never sets it
  (`ECLogarithmicRevRenderer::updateData`, `ExposureContrastOpCPU.cpp:651-657`) and keeps 0.088, so
  its CPU is not the inverse of its own forward for another step. Pinned both ways by
  `log_reverse_follows_the_ocio_gpu_not_its_cpu_step_quirk` (pyocio 2.5.1 numbers).- `grading_bspline.rs` (OCIO's `EstimateRGBSlopes`/`FitRGBSpline` and the `(A*t+B)*t+C`
  evaluation), `hsy.rs` (the only CPU RGB<->HSY; `vfx-ops` re-exports it), `aces2.rs`.

## Conventions

- **One derivation per concept.** Compile-time inverses, `ProcessorOp::inverse()` and the GPU
  all call the same function in `op.rs`: `invert_range_params` (the inverse's clamp bounds are the forward's input bounds),
  `grading_primary_params` (port of `GradingPrimaryPreRender::update`), `invert_matrix_offset`
  (f64, refuses only an exactly singular matrix, as OCIO does), `moncurve_params`,
  `GammaLogParams`/`DoubleLogParams`, `GamutComp13Params`, `cdl_inverse_ops`. Before writing an
  inverse or a parameter formula, grep `op.rs`. A second, independent derivation passes every
  round-trip test and still drifts.
- **GradingPrimary fields are direction-resolved.** `ProcessorOp::GradingPrimary` holds the
  output of `grading_primary_params`; CPU apply and the emitters use them as-is. The op also
  keeps `raw` (the unresolved `dynamic::GradingPrimary`); `inverse()` re-runs
  `grading_primary_params(style, !forward, raw)`. `GradingRgbCurve` likewise keeps `raw`.
- **Op-level `inverse()`** refuses where only the compile path has the information: Lut1D/Lut3D
  (need the domain and a monotonicity check), GradingHueCurve. The other ops flip their
  direction; a CDL keeps its FORWARD parameters and `inverse: Some(cdl_inverse_ops(..))`
  (`ProcessorOp::cdl`), so the optimizer can pair it (OCIO `CDLOpData` has a direction).
- **No silent fallbacks.** An unknown `BuiltinTransfer` or `BuiltinTransform` style is a typed
  error. Style names use OCIO's spelling (`APPLE_LOG`, `CANON_CLOG2`, ...).
- **Identity detection is conservative.** An ASC CDL with unit parameters still clamps, and an
  Exponent with unit value still clamps under `Clamp`; `is_identity` returns false for both so
  the optimizer keeps them, and `identity_replacement` turns them into the clamp OCIO keeps.
- **Optimizer (OCIO-11, 2026-09-23).** `Processor::optimize` is `OpOptimizers.cpp`'s pass
  order: RemoveNoOps, ReplaceOps (`simpler_replacement`), ReplaceIdentityOps, RemoveInverseOps
  (`pair_identity_with`), one CombineOps (`combine_with`; LUT composition by level). A new
  op type that OCIO pairs or composes gets its rule in those `op.rs` functions; gate it in
  `processor/tests_optimizer.rs` against PyOCIO op lists, not only pixels.
- **3D LUT memory order.** `vfx_lut::Lut3D` is Blue-fastest; `ProcessorOp::Lut3d.lut` and the
  GPU texture are Red-fastest. Convert only through `vfx_lut::OrderedLut3dData` (the two sites
  in `compile.rs` and `baker.rs::write_cube_3d`). On disk, `.cube` is Red-fastest;
  `.spi3d`/`.3dl`/`.clf` are Blue-fastest.
- **3D LUT interpolation** is `vfx_lut::interpolate_lattice`, with OCIO names mapped by
  `transform::Interpolation::lut3d_method` (OCIO v2 evaluates Lut3D `Nearest` as trilinear).
- **GradingRGBCurve is evaluated, not baked**: `apply.rs` calls `eval_curve_fwd`/`eval_curve_rev`
  unbounded in both directions (`GradingRGBCurveOpCPU.cpp`). A curve with fewer than 2 points is
  a compile error.
- **`Processor::metadata()` files** are recorded in the FileTransform arm of
  `compile_transform`, the only place the path still exists; `Processor::combine` carries them.
- `Config::description()` is its own field; `name()` is separate.

## Traps

- **`is_ok()` does not test a transform.** A silent identity is a valid processor. Assert that
  the output values move, against reference numbers.
- **Curve kernels agree at the knots.** Pin values between knots; interpolating the control
  points is not discriminating.
- **Two camera-curve implementations.** `BuiltinTransform` camera styles use
  `builtin_transforms/cameras.rs` (OCIO's constants). `BuiltinTransfer` (`TransferStyle`) uses
  `vfx-transfer`'s modules through `processor/math.rs`. They are not the same code (ROADMAP P3,
  "Duplicated constants").
- **Look chains** return to the scene reference after the last look; OCIO stays in the last
  look's process space (ROADMAP P3).
- **The CPU CDL is `vfx_lut::cdl`** (G7): `apply.rs` calls `apply_cdl_*` for both directions;
  `cdl_inverse_ops` exists for the GPU emitters and the optimizer. Parameters are checked with
  `vfx_lut::cdl::validate_cdl_params` (OCIO's ranges). A file CDL takes the `FileTransform`'s
  `cdl_style` (default NoClamp, as OCIO) and `ccc_id` picks a correction by id, then by index
  (`select_cdl`, `.ccc` and `.cdl` alike). `simpler_replacement` keeps the final clamp of an
  inverse ASC CDL that OCIO's own rewrite drops.
- The same CDL inversion once existed three times with the same bug. When fixing a formula,
  grep for its twins in `compile.rs`, `op.rs`, `builtin_transforms/` and `gpu/` and fix them in
  the same change.

## RGBA and moncurve follow OCIO

- **Alpha.** The OCIO CPU ops that change alpha are the matrix (full 4x4 + offset) and the gamma
  op (channel 3 with its own exponent / moncurve parameters); all others copy alpha.
  `apply_op_to_rgba` wraps `apply_op_to_rgb` with exactly that; `apply_rgba`, `apply_packed` with
  an alpha channel and `apply_planar` with an alpha plane use it. RGB-only paths are OCIO's
  alpha-0 case (PyOCIO on 3-channel images). Any op that builds a `ProcessorOp::Matrix`/
  `Exponent`/`ExponentWithLinear` internally keeps the alpha row/column/params at identity unless
  OCIO's op touches alpha too (the inverse ASC CDL uses `PassThru` for that reason; a Nuke `.vf`
  `global_transform` is OCIO's 4x4 `CreateMatrixOp`, LUT-then-matrix for the inverse).
  Gates: `processor::tests::rgba_matches_pyocio`, `tests/gpu_runtime_parity.rs`
  `rgba_ops_gpu_equals_cpu`.
- **ExponentWithLinear** takes LINEAR (default) or MIRROR only; `ProcessorOp::ExponentWithLinear`
  carries `mirror: bool`. `validate`/`set_negative_style` reject CLAMP/PASS_THRU with OCIO's
  `GammaOpData::ConvertStyleMonCurve` text, and the processor is the one moncurve evaluator.
- **Config reader leniency.** Transform slots go through `parse_optional_transform`: an
  `OcioError::Validation` fails the load (as OCIO: unknown tag, untagged mapping, unknown
  direction/style string, all with OCIO's `ParseUtils.cpp` messages and names only); an
  `OcioError::Yaml` (a missing field OCIO also tolerates at load time) leaves the slot empty.
- **Range.** `compile.rs::range_op` is shared by `Transform::Range` and CLF `<Range>`, via
  `vfx_lut::RangeParams::affine` (OCIO `RangeOpData`).

## Open items

Tracked in `docs/ROADMAP.md`: OCIO-11 leftovers (LUT_INV_FAST, FAST_LOG_EXP_POW, separable prefix),
E9 (temp file in `compile.rs`), and the §1 P1 UNVERIFIED rows.
- **LUT inverses (2026-09-23).** `inv_lut.rs` is the port of OCIO's exact renderers; a
  `Lut1d`/`Lut3d` op with `inverse: Some(..)` holds the FORWARD table and inverts it per pixel.
  The GPU bakes (`ProcessorOp::gpu_baked_inverse`). Do not reintroduce a baked CPU inverse.
  A half-domain table inverts through `InvLut1d::new_half` (`InvLut1DRendererHalfCode`); the
  GPU bake of any 1D inverse is a half-domain table (`gpu/lut1d_half.rs` samples it).
- **B-spline segment search.** `grading_bspline.rs` `eval_curve_*` follow OCIO's C loop: when
  no knot is above x, the segment is `knotsCnt - 2` (the last), not the one before.

**Config YAML is OCIO's, both ways (2026-09-23).** `config/loading.rs` reads exactly the keys,
value shapes and defaults of `OCIOYaml.cpp` `load()`, `config/saving.rs` mirrors each `save()`.
Do not add a key or a lenient spelling OCIO does not have: `tests/ocio_config_yaml_parity.rs`
compares every built-in OCIO config (pixels and `serialize` tree) with PyOpenColorIO, and
`tests/data/check_serialize_in_pyocio.py` checks that PyOCIO reads our output back. A FileTransform
`src` stays as written and is resolved in `Config::expand_transform`, which every config processor
goes through (colour-space transforms may contain ColorSpace/Look/DisplayView transforms). The
periodic hue splines (HUE_SAT/HUE_LUM/HUE_FX) are horizontal in `grading_bspline.rs`.
