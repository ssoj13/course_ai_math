# Deferred bridges ledger — NOTHING stays a permanent hole

Policy (per user): full parity with Little-CMS 2.19 (`vfx.ref/Little-CMS`), **no deviations, no unimplemented
holes**. A bridge may be *temporarily* deferred ONLY because its dependency isn't ported
yet. Every entry here MUST be closed when its dependency lands. This file is the checklist;
grep the code for `TODO(` and reconcile against this list — the two must always match, and
both must reach empty before the port is "done".

Legend: ⛔ open · ✅ closed

> **Re-opened and closed by bug-hunt pass 1** (`.bughunt/plan1.md`): row #16's named-color
> caveat rested on a claim that had gone stale — the named-color MPE *can* exist in a pipeline
> (row #2 landed exactly that), so `CreateNamedColorDevicelink` was a genuine hole rather than a
> blocked dependency. Now implemented; see row #18.

| # | Deferred item | Source fn | Blocked on | Closes in | Status |
|---|---|---|---|---|---|
| 1 | `cmsMD5computeID` profile wrapper (zero flags/intent/ID, SaveToMem, hash) | cmsmd5.c | profile Save-To-Mem | M3 | ✅ closed in `profile/save.rs::compute_id` |
| 7 | `_cmsAvoidTypeCheckOnTags` context flag (plugin sig-0 sentinel disables tag type checks) | cmsio0/plugin | plugin subsystem | plugin phase | ✅ closed in `plugin.rs` (`Plugin::Tag` with `sig==0` sets the `avoid_type_check` flag = `_cmsAvoidTypeCheckOnTags`; per-`Context` since row 19, resolved through the same chunk lookup) + `profile/mod.rs` (`read_tag`/`read_ondisk_tag` skip `is_type_supported` + the unknown-descriptor-fatal + item-count checks when the flag is set, matching cmsio0.c `avoidCheck`). Tag descriptors also route through the registry first (`descriptor.rs::get_descriptor` → `plugin::find_tag_descriptor`). Test `tag_plugin_descriptor_and_sig0_flag` in `tests/plugin_registry.rs` |
| 8 | Formatter plugin-factory chain (`_cmsGetFormatter` context-local factory list); stock formatters only | cmspack | plugin subsystem | plugin phase | ✅ closed in `plugin.rs` (`Plugin::Formatters` = `cmsFormatterFactory`; `find_{input,output}_formatter_{16,float}`) + `cmm/pack/table.rs` (all four `get_*_formatter_*` consult the factory chain before the stock tables, exactly as `_cmsGetFormatter`). Newest-registered factory wins |
| 9 | `cmsFormatterForColorspaceOfProfile` / `cmsFormatterForPCSOfProfile` helpers | cmspack | (profile now exists) | M4/M5 tidy | ✅ closed in `cmm/pack/mod.rs` (`formatter_for_colorspace`/`formatter_for_pcs`: sig→PT via `pcs::lcms_colorspace` + `cms_channels_of_colorspace`, `nBytes&7`, all-zero word on unsupported space) |
| 10 | **`Profile: Clone`** — needed so black-point round-trips can alias a profile at two array slots (`[Lab,prof,prof,Lab]`) | cmsxform/samp | profile io model | M5 | ✅ closed in `profile/mod.rs` (`#[derive(Clone)]`; `io` is now `Arc<Mutex<dyn IoHandler + Send>>` — shared backing store, deep tag dict; `Send+Sync` preserved) |
| 11 | Black-point ROUND-TRIP detection (`CreateRoundtripXForm`, `BlackPointAsDarkerColorant`, Adobe dest-black) — currently V4-const only, returns None on round-trip branch (BPC no-op, == C on failure) | cmssamp | virt profiles + #10 | M5 | ✅ closed in `cmm/blackpoint.rs` (full darker-colorant + perceptual-black + Adobe 256-step ramp; aliases via `Profile::clone`) |
| 12 | Black-preserving K-only/K-plane CMYK build (`_cmsBuildKToneCurve`, `cmsDetectTAC`); currently Err for CMYK-preserving (loud, not silent-wrong) | cmscnvrt/cmsgmt | cmsgmt + #10/#11 | M5 | ✅ closed in `cmm/intents.rs` (`BlackPreservingGrayOnlySampler`/`BlackPreservingSampler` wired into `SearchIntent`) |
| 13 | Gamut-check pipeline construction (`_cmsCreateGamutCheckPipeline`) — kernels done, construction None → GAMUTCHECK dropped (== C when no gamut profile) | cmsgmt/virt | virt profiles | M5 | ✅ closed in `cmm/transform.rs` (`create_gamut_check_pipeline` wired; GAMUTCHECK marks OOG with alarm codes) |
| 14 | `cmsDetectRGBProfileGamma` linear-RGB NOOPTIMIZE hint (optimization only) + `cmsFLAGS_KEEP_SEQUENCE` (needs `&mut [Profile]`) | cmsxform | virt + API | M5 | ✅ closed in `cmm/transform.rs` (NOOPTIMIZE hint via `detect_rgb_profile_gamma`; KEEP_SEQUENCE via `compile_profile_sequence` fed cloned profiles — #10 unblocked it) |
| 15 | `cmsCreateDeviceLinkFromCubeFile` (.cube→devicelink profile); ParseCube itself is COMPLETE, only the profile-write wrapper is stubbed (returns Err) | cmscgats | pipeline+profile write wiring | M6 tidy | ✅ closed in `cgats/mod.rs` (`create_device_link_from_cube_file[_thr]`: v4.4 Link/RGB/RGB profile, shaper ToneCurves + float CLUT → `AToB0` pipeline, TITLE→desc MLU). Note: the reference builds a `[CurveSet,CLUT]` pipeline which is **not** a serialisable `LutAToB` shape (needs a trailing B-curve set), so lcms uses the profile **live** as a transform input (`utils/tificc`) and never saves it — matched here; the test verifies the live tag, not save-to-disk |
| 16 | `cmsTransform2DeviceLink` + `CreateNamedColorDevicelink` — reusable core DONE in virt (fix_color_spaces, ALLOWED_LUT_TYPES, find_combination); wrapper needs `Transform::context()` + `sequence()` accessors + named-color-list retrieval | cmsvirt | small transform.rs accessors | closure pass | ✅ closed in `virt.rs` (`transform2devicelink` full LUT path + `Transform::context()`/`sequence()`). Two cross-module caveats, both out of this scope: the **named-color** devicelink branch is gated on the named-color pipeline stage + `cmsGetNamedColorList` (item #6, unreachable until a `NamedColor` MPE can exist); the FORCE_CLUT-materialization caveat is now **resolved by #17** (the optimizer keeps discrete serializable MPE stages), so a `transform2devicelink` of an already-optimized transform can now be written |
| 2 | `_cmsStageAllocNamedColor` (named-color pipeline stage) | cmsnamed.c / cmslut.c | transform (`cmsDoTransform`) | M4 | ✅ closed in `pipeline/stage.rs` (`Stage::alloc_named_color`; dedicated `StageData::NamedColor{list,use_pcs}`, `type_signature()`==`ncl `; `eval_named_color` = bit-exact `EvalNamedColorPCS`/`EvalNamedColor`, PCS via raw `PCS[i]/65535`, out-of-range→zeros). Consumer wiring fully closed in `profile/read_hl.rs`: `read_input_lut` builds `[named_color(UsePCS=true), LabV2ToV4]` and `read_devicelink_lut` builds `[named_color(UsePCS=false)(+LabV2ToV4 iff Lab space)]` (verbatim `_cmsReadInputLUT`/`_cmsReadDevicelinkLUT`, cmsio1.c); `named_color_blocked()` removed. Test `named_color_profile_builds_named_stage` confirms both directions yield a pipeline whose first stage is the `ncl ` element. |
| 3 | `Custom` stage type-signature (plugin MPE type registry) | cmslut.c | plugin subsystem | plugin phase | ✅ closed in `pipeline/stage.rs` (`Stage::type_signature` for `Custom` now returns the placeholder's `implements` sig = `cmsStageAllocPlaceholder`'s `Type`, no longer `Signature(0)`) + `plugin.rs` (`Plugin::MultiProcessElement` = `cmsPluginMultiProcessElement`; `find_mpe_reader`) + `tags/types/mpe.rs` (`read_mpe_elem` dispatches an unknown element sig to the registered `MpeReadFn` before erroring). Test `custom_mpe_type_has_signature_and_reader` |
| 4 | Parametric-curve plugin registry (beyond the 10 built-ins) | cmsgamma.c | plugin subsystem | plugin phase | ✅ closed in `plugin.rs` (`Plugin::ParametricCurves` = `cmsPluginParametricCurves`; `find_parametric_curve`) + `color/gamma.rs` (`get_parametric_curve_by_type` + `resolve_parametric_eval` consult the registry before `DefaultCurves`; `abs(Type)` keys the inverse). Test `custom_parametric_curve_is_accepted` |
| 5 | Interp/Optimizer/Formatter/Intent/Transform/MPE plugin registries | cmsplugin.c | plugin subsystem | plugin phase | ✅ closed in `plugin.rs` (`register_plugin`/`unregister_plugins` = `cmsPlugin`/`cmsUnregisterPlugins`; the registry was process-global at the time and became per-`Context` in row 19). **All ten supported categories wired & consumed:** Optimizer (`optimize.rs`), Formatter (#8), Intent (`intents.rs::search_intent` → `find_intent`), MPE (#3), Tag/TagType (`descriptor.rs`/`handler.rs`), Interpolation (`InterpParams` regained the `cmsInterpFunction` seam as two `Copy`-friendly `fn` fields; `new_ex` = `_cmsSetInterpolationRoutine`, `eval16`/`eval_float` prefer the override), Transform (`Transform::xform`/`user_data` set by a claiming factory in `alloc_empty_transform`, incl. the pre-2.8 `_cmsTransformFn` adaptor; `run_worker` dispatches), Parallelization (`ParalellizeIfSuitable`: the plugin's `SchedulerFn` displaces the pixel loop, which becomes `run_worker` — plus `plugin::threaded_scheduler`, a real `std::thread::scope` splitter with no `unsafe`). Memory + Mutex categories are intentionally dropped (Rust ownership / `Sync`), as `context.rs` documents |
| 6 | `cmsGetNamedColorList` / Dup/Free named-color transform glue | cmsnamed.c | transform | M4 | ✅ closed in `cmm/transform.rs` (`Transform::named_color_list` walks `Lut`'s first stage for the `ncl ` element, mirroring the C `v->Lut->Elements` check); `Stage::named_color_list` helper in `pipeline/stage.rs`. `DupNamedColorList`/`FreeNamedColorList` collapse into `NamedColorList`'s `Clone`/`Drop`. |
| 17 | **Optimizer produces an unserializable fused `Custom` stage** — `cmm::optimize` collapses matrix-shaper/prelin/resample into one `Custom(Arc<dyn StageEval>)`; C keeps discrete CLUT/curve/matrix stages + a *swappable evaluator*. Eval is bit-exact, but an optimized pipeline can't be dumped back to an AToB/BToA tag (blocks `transform2devicelink` of an already-optimized transform). To match C: add a fast-eval hook to `Pipeline` and have optimizers retain serializable stages | cmsopt/cmslut | Pipeline eval-hook redesign | opt refactor | ✅ closed in `pipeline/mod.rs` (`Pipeline::fast_eval` hook = `_cmsPipelineSetOptimizationParameters`; `set_optimization_parameters`/`is_optimized`; auto-cleared on structural mutation) + `cmm/optimize.rs` (all 4 optimizers now `install_optimized` = discrete serializable stages [curves]/[curves,matrix,curves]/[curves,CLUT]/[opt-curves,CLUT,opt-curves] + fast evaluator). Eval byte-identical (259 lib tests unchanged); `pipeline.stages()` is now serializable |

| 18 | **`CreateNamedColorDevicelink`** (`cmsvirt.c:1085`, dispatched `:1216`) | cmsvirt.c | — | bughunt pass 1 | ✅ closed in `virt.rs` (`create_named_color_devicelink`): dups the list from `named_color_list()`, re-points `ColorantCount` at the pipeline output channels (`:1104`), rebinds the transform to `TYPE_NAMED_COLOR_INDEX` → `(exit colorspace, 2 bytes, n channels)` through the now-ported `change_buffers_format` (`:1107`), recomputes each spot colour's `DeviceColorant` by index (`:1111-1114`), and writes `cmsSigNamedColor2Tag`. `transform2devicelink` takes `&mut Transform` because C mutates the caller's transform via that rebind. Stale "cannot exist in a pipeline" paragraph deleted. Test `named_color_transform_yields_a_named_color_devicelink`, both halves mutation-verified |

| 19 | **Per-context plugin chunks** — the registry was process-global, so `cmsPluginTHR`, `cmsUnregisterPluginsTHR` and the plugin half of `cmsCreateContext` were unimplementable and two consumers in one binary collided | cmsplugin.c | — | api census (step 0) | ✅ closed in `context.rs`/`plugin.rs`: `Context.plugins: Option<Arc<RwLock<Registries>>>`, `None` = "I am Context 0" resolving to the global static. Every lookup takes `Option<&Context>` and reproduces `_cmsContextGetClientChunk`'s two-step — own chunk if present, **else** Context 0, never merged. `Context::{register_plugin, unregister_plugins, with_plugin, dup, set_user_data}` cover the missing entry points; `dup` deep-copies so later parent registrations cannot leak in. Integration test `plugin_registered_into_one_context_is_invisible_to_another` pins it, incl. "A shadows Context 0"; mutation-verified |

## Plugin subsystem (item 3-5, 19) — DONE
lcms2 has 12 plugin categories (`cmsplugin.c`). Ten are modelled in `src/plugin.rs`, held
**per `Context`** exactly as lcms holds them per context chunk. A `Context` with no chunk of
its own resolves to the process-global registry, which IS Context 0 — where the non-`THR`
`cmsPlugin` registers and where every unscoped lookup lands.

The resolution is C's, verbatim: a context's own chunk **shadows** Context 0 entirely rather
than merging with it (`_cmsContextGetClientChunk`). An earlier revision of this file claimed
modelling Context 0 was sufficient; it is not — it covers the fallback but not the override,
and the override is the whole point of the `*THR` API. See row 19.

**Two categories (Memory, Mutex) are omitted — an explicit project-owner decision**, reviewed
and ratified after the omission was surfaced. Rationale: both exist in C to patch language
gaps (hand-rolled `malloc`/`free`, hand-rolled locking) that Rust covers natively via
ownership and `Sync`; there is no allocator/lock indirection in this port for a plugin to
displace. This is recorded as a deliberate scope decision, **not** an unimplemented feature.
The other ten categories are all implemented and consumed — no category is accepted-then-ignored.

Each supported subsystem's lookup now consults the registry **before** its built-ins,
matching lcms's `GetHandler`/`SearchIntent`/`_cmsGetFormatter`/`GetParametricCurveByType`
ordering, and every list is searched newest-first (lcms prepends on registration):

- ParametricCurves → `color/gamma.rs`  (#4)
- Optimization → `cmm/optimize.rs::optimize_pipeline`  (#5)
- Formatters → `cmm/pack/table.rs` (all four `get_*_formatter_*`)  (#8)
- RenderingIntent → `cmm/intents.rs::search_intent`
- Tag → `tags/descriptor.rs::get_descriptor` + the `avoid_type_check` sentinel  (#7)
- TagType → `tags/handler.rs::get_handler` (new `TypeHandler::Plugin` variant, dynamic
  read/write over `&mut dyn ReadSeek`/`WriteSeek`)
- MultiProcessElement → the `mpet` reader in `tags/types/mpe.rs`  (#3)
- Interpolation → `math/interp.rs::InterpParams::new_ex` (`_cmsSetInterpolationRoutine`)
- Transform → `cmm/transform.rs::alloc_empty_transform` (`AllocEmptyTransform` plugin loop)
- Parallelization → `cmm/transform.rs::parallelize_if_suitable` (`ParalellizeIfSuitable`)

**All ten supported categories are consumed.** The last three were wired by restoring the
fn-pointer seams the port had originally dropped:

- `InterpParams` regained the `cmsInterpFunction` indirection as two plain `fn` fields
  (`interp16`/`interp_float`), so the struct stays `Copy`. `new_ex` asks
  `plugin::find_interpolation_factory`; `eval16`/`eval_float` prefer the override and
  otherwise take the built-in dispatch (C's `DefaultInterpolatorsFactory`). The kernels take
  the CLUT as an argument rather than through `p->Table`, the port's one signature deviation.
- `Transform` gained `xform`/`user_data`, populated when a registered factory claims the
  transform, plus the pre-2.8 `_cmsTransformFn` adaptor. `run_worker` dispatches to the
  plugin loop when present and to the built-in kernels otherwise.
- Parallelization follows C exactly: lcms itself does **no** threading — the plugin supplies
  `SchedulerFn`, which displaces the pixel loop and demotes it to `Worker`. `run` calls the
  scheduler; the scheduler calls `Transform::run_worker` per slice. `plugin::threaded_scheduler`
  is a ready-made multi-threaded scheduler (`std::thread::scope`, disjoint `split_at`/
  `split_at_mut` slices, no `unsafe`), following the reference `threaded` plugin's strategy:
  128 KiB of traffic per worker, capped at `MaxWorkers`, split by scanlines.

The registry is **empty by default**, so with no plugin registered every subsystem takes its
built-in path byte-for-byte (verified: 262 all-features lib tests unchanged). Behaviour-
mutating scenarios live in `tests/plugin_registry.rs` (a separate binary, so Context 0
registry can never leak into the unit tests) — 8 scenarios, one per consumed category.

## Verification of "no holes"
Before declaring any milestone done: `grep -rn "TODO\|todo!\|unimplemented!\|unreachable!\|
FIXME" src/` — every hit must correspond to a tracked row above (dependency-ordered), never
a silent stub or an abandoned branch. `test.skip`/ignored tests are also holes.
