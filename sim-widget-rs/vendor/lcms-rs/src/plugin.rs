//! The plugin subsystem — a pure-Rust analogue of `cmsplugin.c`'s dispatch.
//!
//! lcms2 lets callers extend the engine at run time through [`cmsPlugin`]/`cmsPluginTHR`,
//! which fan a linked list of `cmsPluginBase` records out to twelve `_cmsRegister*Plugin`
//! handlers, each stashing the extension in a per-`Context` "chunk". Every subsystem lookup
//! (`GetHandler`, `SearchIntent`, `_cmsGetFormatter`, `GetParametricCurveByType`, …) then
//! consults its context chunk *before* the built-in tables, so a registered plugin always
//! wins.
//!
//! # The two-step lookup (`_cmsContextGetClientChunk`)
//! C stores plugin lists in per-context *chunks* and reaches them through
//! `_cmsContextGetClientChunk` (`cmsplugin.c:747-772`), which is a strict two-step:
//!
//! 1. if the context owns a chunk for that category, return **it**;
//! 2. otherwise return **Context 0's** chunk.
//!
//! The two are never merged: the moment a context owns a chunk, Context 0 is shadowed
//! entirely — even if that chunk is empty. This port reproduces both steps exactly:
//!
//! * the process-global [`static@REGISTRY`] below **is** Context 0's chunk set. `cmsPlugin` /
//!   `cmsUnregisterPlugins` (`cmsplugin.c:541-543`, `:626-630`) target it, and so do the free
//!   [`register_plugin`] / [`unregister_plugins`] here;
//! * a [`Context`] with `plugins == None` owns no chunk and therefore reads the global —
//!   this is C's `ContextID == NULL`, which `_cmsGetContext` (`cmsplugin.c:714-721`) maps to
//!   `globalContext`;
//! * [`Context::register_plugin`] (`cmsPluginTHR`) and [`Context::unregister_plugins`]
//!   (`cmsUnregisterPluginsTHR`) give the context its **own** chunk, which from then on
//!   shadows the global for every lookup made through that context.
//!
//! Every `find_*` below therefore takes `ctx: Option<&Context>` as its first argument and
//! resolves it through [`read_reg`], the port of `_cmsContextGetClientChunk`. `None` means
//! "no context in scope" and reads Context 0, exactly as a C call with `ContextID == NULL`.
//!
//! # Categories
//! Ten of lcms's twelve categories are modelled. The two dropped are the ones Rust provides
//! natively: **Memory** (the sub-allocator → Rust ownership) and **Mutex** (→ `std` `Sync`),
//! exactly as [`crate::context`] already documents. For each supported category this module
//! stores the extension and exposes a `find_*` lookup that the owning subsystem calls before
//! its built-ins:
//!
//! | Category | Registry field | Consulted by |
//! |----------|----------------|--------------|
//! | ParametricCurves | `parametric_curves` | [`crate::color::gamma`] |
//! | Optimization | `optimizers` | [`crate::cmm::optimize::optimize_pipeline`] |
//! | Formatters | `formatters` | [`crate::cmm::pack::table`] |
//! | RenderingIntent | `intents` | [`crate::cmm::intents`] |
//! | Tag | `tags` | [`crate::tags::get_descriptor`] + [`avoid_type_check`] |
//! | TagType | `tag_types` | [`crate::tags::get_handler`] |
//! | MultiProcessElement | `mpe_types` | the `mpet` reader in [`crate::tags`] |
//! | Interpolation | `interpolators` | [`crate::math::interp::InterpParams::new_ex`] |
//! | Transform | `transforms` | [`crate::cmm::transform`] (`AllocEmptyTransform`) |
//! | Parallelization | `parallel` | [`crate::cmm::transform`] (`ParalellizeIfSuitable`) |
//!
//! The registry is **empty by default**, so with no plugin registered every subsystem takes
//! its built-in path byte-for-byte, exactly as lcms does before the first `cmsPlugin` call.

use std::io::{Read, Seek, Write};
use std::ops::Deref;
use std::sync::{Arc, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::context::Context;
use crate::error::Result;
use crate::tags::descriptor::TagDescriptor;
use crate::types::sig::Signature;

// Re-exported so downstream modules and plugin authors name one canonical type.
use crate::cmm::pack::{Formatter16In, Formatter16Out, FormatterFloatIn, FormatterFloatOut};
use crate::cmm::transform::Transform;
use crate::cmm::Stride;
use crate::math::interp::InterpParams;
use crate::pipeline::stage::Stage;
use crate::pipeline::Pipeline;
use crate::types::format::PixelFormat;

// ---------------------------------------------------------------------------------------
// Shared IO trait object for dynamic (plugin) serializers
// ---------------------------------------------------------------------------------------

/// A `Read + Seek` object usable behind `&mut dyn` (`cmsIOHANDLER` read side).
///
/// The built-in tag/MPE (de)serializers are generic over `impl Read + Seek` because they
/// reuse [`crate::io::endian`], which cannot go through a `&mut dyn Read`. A *plugin*
/// handler, however, is stored as a value and so must be object-safe: this blanket trait lets
/// a concrete `&mut R` coerce to `&mut dyn ReadSeek` at the call site.
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// A `Write + Seek` object usable behind `&mut dyn` (`cmsIOHANDLER` write side).
pub trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

// ---------------------------------------------------------------------------------------
// Category payloads
// ---------------------------------------------------------------------------------------

/// A parametric-curve evaluator (`cmsParametricCurveEvaluator`): `(type, params, x) -> y`.
///
/// Structurally identical to [`crate::color::gamma::ParametricEvaluator`]; the two `fn`
/// types are interchangeable.
pub type ParametricEvaluator = fn(i32, &[f64; 10], f64) -> f64;

/// A parametric-curve plugin (`cmsPluginParametricCurves`).
///
/// Adds one or more curve *types* (each with its ICC parameter count) evaluated by a single
/// [`ParametricEvaluator`], mirroring the C struct's `nFunctions`/`FunctionTypes[]`/
/// `ParameterCount[]`/`Evaluator` quartet.
#[derive(Clone)]
pub struct ParametricCurvePlugin {
    /// Curve type identifiers handled (`FunctionTypes`). Positive; the negative (analytic
    /// inverse) is matched automatically via `abs`.
    pub types: Vec<i32>,
    /// Parameter count per entry of [`Self::types`] (`ParameterCount`).
    pub counts: Vec<usize>,
    /// The evaluator shared by every listed type (`Evaluator`).
    pub evaluator: ParametricEvaluator,
}

/// An optimizer (`_cmsOptimizationFn` / `cmsPluginOptimization`).
///
/// Returns `Ok(true)` when it produced an optimized pipeline (and "won"), `Ok(false)` to
/// decline. Signature matches the built-in optimizers in [`crate::cmm::optimize`].
pub type OptimizerFn =
    fn(&mut crate::pipeline::Pipeline, u32, &mut PixelFormat, &mut PixelFormat, &mut u32) -> Result<bool>;

/// Direction argument of a formatter factory (`cmsFormatterDirection`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatterDir {
    /// Unpack device/PCS bytes into the engine's working buffer (`cmsFormatterInput`).
    Input,
    /// Pack the engine's working buffer back into device/PCS bytes (`cmsFormatterOutput`).
    Output,
}

/// `CMS_PACK_FLAGS_FLOAT` — the factory-flag bit selecting the float formatter union arm.
pub const CMS_PACK_FLAGS_FLOAT: u32 = 0x0002;

/// The four concrete formatter shapes a factory may return (the `cmsFormatter` union).
#[derive(Clone, Copy)]
pub enum PluginFormatter {
    /// 16-bit input path (`cmsFormatter16`, input).
    In16(Formatter16In),
    /// 16-bit output path (`cmsFormatter16`, output).
    Out16(Formatter16Out),
    /// Float input path (`cmsFormatterFloat`, input).
    InFloat(FormatterFloatIn),
    /// Float output path (`cmsFormatterFloat`, output).
    OutFloat(FormatterFloatOut),
}

/// A formatter factory (`cmsFormatterFactory`): pick a packer/unpacker for a format word.
///
/// Called with the [`PixelFormat`], the [`FormatterDir`] and the pack flags (only
/// [`CMS_PACK_FLAGS_FLOAT`] is meaningful). Returns `None` to decline, exactly as the C
/// factory returns a `NULL`-union.
pub type FormatterFactory = fn(PixelFormat, FormatterDir, u32) -> Option<PluginFormatter>;

/// A rendering-intent link function (`cmsIntentFn`).
pub type IntentFn = fn(
    &crate::Context,
    &mut [&mut crate::profile::Profile],
    &[u32],
    &[bool],
    &[f64],
    u32,
) -> Result<crate::pipeline::Pipeline>;

/// A rendering-intent plugin (`cmsPluginRenderingIntent`): intent code, description and its
/// link builder.
#[derive(Clone)]
pub struct IntentPlugin {
    /// The intent code registered (`Intent`).
    pub intent: u32,
    /// Human-readable description (`Description`), kept for `cmsGetSupportedIntents`.
    pub description: String,
    /// The pipeline builder for this intent (`Link`).
    pub link: IntentFn,
}

/// A tag plugin (`cmsPluginTag`): a tag signature and its descriptor.
///
/// A plugin whose [`Self::sig`] is `0` is the type-check sentinel: registering it flips
/// [`avoid_type_check`] on, matching `_cmsAvoidTypeCheckOnTags` (which returns TRUE when any
/// registered tag has signature `0`).
#[derive(Clone)]
pub struct TagPlugin {
    /// The tag signature this descriptor applies to (`Signature`).
    pub sig: Signature,
    /// The descriptor (`Descriptor`): element count, supported types, decide-type callback.
    pub descriptor: TagDescriptor,
}

/// Dynamic reader for a plugin tag/MPE type (`cmsTagTypeHandler::ReadPtr` seam).
///
/// Receives the IO positioned after the 8-byte type/element base and the remaining body size.
pub type TypeReadFn =
    fn(&mut dyn ReadSeek, u32) -> Result<crate::tags::TagValue>;

/// Dynamic writer for a plugin tag type (`cmsTagTypeHandler::WritePtr` seam).
pub type TypeWriteFn = fn(&mut dyn WriteSeek, &crate::tags::TagValue) -> Result<()>;

/// A tag-type plugin (`cmsPluginTagType`): a type signature and its read/write functions.
#[derive(Clone, Copy, Debug)]
pub struct TagTypePlugin {
    /// The tag-type signature handled (`Handler.Signature`).
    pub sig: Signature,
    /// Body reader (`Handler.ReadPtr`).
    pub read: TypeReadFn,
    /// Body writer (`Handler.WritePtr`).
    pub write: TypeWriteFn,
}

/// Dynamic reader for a Multi-Process-Element type (`cmsMPEType` read seam).
///
/// Receives the IO positioned after the 8-byte element base and returns the built [`Stage`].
pub type MpeReadFn = fn(&mut dyn ReadSeek) -> Result<Stage>;

/// A Multi-Process-Element type plugin (`cmsPluginMultiProcessElement`).
///
/// Registers a new `mpet` element signature so the pipeline reader can materialise a
/// plugin-defined stage (closes the `Custom`-stage type-signature hole).
#[derive(Clone)]
pub struct MpeTypePlugin {
    /// The MPE element signature handled (`Handler.Signature`).
    pub sig: Signature,
    /// Element-body reader (`Handler.ReadPtr`).
    pub read: MpeReadFn,
}

// --- Interpolation (cmsPluginInterpolation) --------------------------------------------

/// A 16-bit interpolation kernel (the `Lerp16` arm of the `cmsInterpFunction` union).
///
/// # Deviation from C
/// lcms's `_cmsInterpFn16` is `(const cmsUInt16Number in[], cmsUInt16Number out[],
/// const cmsInterpParams* p)` and reaches the CLUT through `p->Table`. This port keeps the
/// table **out** of [`InterpParams`] (which is pure `Copy` geometry) and passes it explicitly,
/// so the kernel receives `(params, table, input, output)`. Everything else is identical.
pub type Interp16Fn = fn(&InterpParams, &[u16], &[u16], &mut [u16]);

/// A float interpolation kernel (the `LerpFloat` arm of `cmsInterpFunction`).
///
/// Same table-passing deviation as [`Interp16Fn`].
pub type InterpFloatFn = fn(&InterpParams, &[f32], &[f32], &mut [f32]);

/// The `cmsInterpFunction` union: a factory returns exactly one of the two kernel shapes.
///
/// In C this is a genuine `union` of two function pointers, which is why
/// `_cmsSetInterpolationRoutine` can test "is it null?" by looking at `Lerp16` alone. Rust
/// models the same either/or choice as an enum, which additionally makes the 16-bit/float
/// mix-up unrepresentable.
#[derive(Clone, Copy)]
pub enum InterpFunction {
    /// The fixed-point kernel (`Interpolation.Lerp16`).
    Lerp16(Interp16Fn),
    /// The floating-point kernel (`Interpolation.LerpFloat`).
    LerpFloat(InterpFloatFn),
}

/// An interpolation factory (`cmsInterpFnFactory`): `(nInputs, nOutputs, dwFlags)`.
///
/// Returns `None` to decline, exactly as the C factory returns a union whose `Lerp16` member
/// is `NULL` — in which case `_cmsSetInterpolationRoutine` falls back to
/// `DefaultInterpolatorsFactory` (this port's built-in dispatch in
/// [`InterpParams::eval16`]/[`InterpParams::eval_float`]).
pub type InterpFactory = fn(usize, usize, u32) -> Option<InterpFunction>;

// --- Transform (cmsPluginTransform) ----------------------------------------------------

/// A full transform pixel loop (`_cmsTransform2Fn`).
///
/// Receives the transform (`CMMcargo`), the input/output buffers, pixels-per-line, line count
/// and the [`Stride`] block — and is responsible for the *entire* conversion, replacing the
/// built-in kernel. A plugin loop typically pulls its private state back out with
/// [`Transform::user_data`] and its formatters with the `Transform` accessors.
pub type TransformFn = fn(&Transform, &[u8], &mut [u8], usize, usize, &Stride);

/// A legacy single-scanline transform loop (`_cmsTransformFn`, plug-ins older than 2.8).
///
/// Handles **one** scanline: `(cargo, input, output, pixels_per_line, bytes_per_plane_in)`.
/// Wrapped by the port of `_cmsTransform2toTransformAdaptor`, which drives it line by line.
pub type LegacyTransformFn = fn(&Transform, &[u8], &mut [u8], usize, usize);

/// Which of the two transform-loop generations a factory produced.
///
/// C distinguishes these by `cmsPluginBase::ExpectedVersion < 2080` and sets the
/// `_cmsTransformCollection::OldXform` flag; a Rust plugin states it directly.
#[derive(Clone, Copy)]
pub enum PluginXform {
    /// A modern multi-line loop (`_cmsTransform2Fn`).
    Full(TransformFn),
    /// A pre-2.8 single-scanline loop (`_cmsTransformFn`), driven by the adaptor.
    Legacy(LegacyTransformFn),
}

/// Opaque private state a transform plugin attaches to the transform (`_cmsTRANSFORM::UserData`).
///
/// C pairs a `void*` with a `_cmsFreeUserDataFn`; Rust needs no free callback — dropping the
/// [`Arc`] releases the payload — so the `FreeUserData` half of the C API has no analogue.
pub type XformUserData = Arc<dyn std::any::Any + Send + Sync>;

/// What a transform factory hands back when it decides to take over the conversion.
#[derive(Clone)]
pub struct TransformFit {
    /// The replacement pixel loop (`*xform` out-parameter).
    pub xform: PluginXform,
    /// Optional private state (`*UserData` out-parameter), later read via
    /// [`Transform::user_data`].
    pub user_data: Option<XformUserData>,
}

/// A transform factory (`_cmsTransform2Factory`).
///
/// Mirrors the C out-parameter set: the factory may rewrite the pipeline, the input/output
/// formats and the flags in place, and returns `Some` to claim the transform (C: `TRUE`) or
/// `None` to decline (C: `FALSE`).
pub type TransformFactory =
    fn(&mut Pipeline, &mut PixelFormat, &mut PixelFormat, &mut u32) -> Option<TransformFit>;

// --- Parallelization (cmsPluginParalellization) ----------------------------------------

/// `CMS_THREADED_GUESS_MAX_THREADS` — let the scheduler pick the worker count itself.
pub const CMS_THREADED_GUESS_MAX_THREADS: i32 = -1;

/// A parallelization plugin (`cmsPluginParalellization`).
///
/// Exactly as in C, lcms itself does **no** threading: the plugin supplies a `SchedulerFn`
/// which `ParalellizeIfSuitable` installs *in place of* the transform's pixel loop, demoting
/// the original loop to the "worker". The scheduler then splits the job and calls that worker
/// once per slice — see [`crate::plugin::threaded_scheduler`] for a ready-made implementation.
#[derive(Clone, Copy)]
pub struct ParallelizationConfig {
    /// Maximum worker threads the scheduler may use (`MaxWorkers`), or
    /// [`CMS_THREADED_GUESS_MAX_THREADS`] to auto-detect.
    pub max_workers: i32,
    /// Scheduler flags (`WorkerFlags`). Reserved and unused, as the C comment on
    /// `_cmsGetTransformWorkerFlags` notes.
    pub worker_flags: u32,
    /// The scheduler entry point (`SchedulerFn`), which becomes the transform's `xform`.
    pub scheduler: TransformFn,
}

// ---------------------------------------------------------------------------------------
// The registry
// ---------------------------------------------------------------------------------------

/// A single registrable extension (`cmsPluginBase` variant), one arm per supported category.
pub enum Plugin {
    /// `cmsPluginParametricCurves`.
    ParametricCurves(ParametricCurvePlugin),
    /// `cmsPluginOptimization`.
    Optimization(OptimizerFn),
    /// `cmsPluginFormatters`.
    Formatters(FormatterFactory),
    /// `cmsPluginRenderingIntent`.
    RenderingIntent(IntentPlugin),
    /// `cmsPluginTag`.
    Tag(TagPlugin),
    /// `cmsPluginTagType`.
    TagType(TagTypePlugin),
    /// `cmsPluginMultiProcessElement`.
    MultiProcessElement(MpeTypePlugin),
    /// `cmsPluginInterpolation`.
    Interpolation(InterpFactory),
    /// `cmsPluginTransform`.
    Transform(TransformFactory),
    /// `cmsPluginParalellization`.
    Parallelization(ParallelizationConfig),
}

/// One context's plugin chunk set — the ten `ctx->chunks[…Plugin]` entries in one struct.
///
/// Each `Vec` is searched **newest-first** (via `.rev()`), matching lcms which prepends every
/// newly registered plugin to the head of its list so later registrations shadow earlier ones.
///
/// `Clone` is the port of C's chunk duplication (`DupPluginCurvesList` and its nine siblings,
/// e.g. `cmsgamma.c:70-102`): a deep, at-copy-time snapshot, which is what `cmsDupContext`
/// (`cmsplugin.c:893-…`) hands the new context.
#[derive(Clone, Default)]
pub(crate) struct Registries {
    parametric_curves: Vec<ParametricCurvePlugin>,
    optimizers: Vec<OptimizerFn>,
    formatters: Vec<FormatterFactory>,
    intents: Vec<IntentPlugin>,
    tags: Vec<TagPlugin>,
    tag_types: Vec<TagTypePlugin>,
    mpe_types: Vec<MpeTypePlugin>,
    interpolators: Vec<InterpFactory>,
    transforms: Vec<TransformFactory>,
    parallel: Option<ParallelizationConfig>,
    /// `_cmsAvoidTypeCheckOnTags`: set when any registered [`TagPlugin`] has signature `0`.
    avoid_type_check: bool,
}

/// A context-owned plugin chunk set, shared by every [`Context`] clone of the same context.
///
/// `Context: Clone` copies the `Arc`, so clones are the *same* context and see each other's
/// registrations — C copies a `cmsContext` pointer with the same effect. The snapshot
/// semantics `cmsDupContext` needs are provided by [`Context::dup`] instead.
pub(crate) type PluginChunk = Arc<RwLock<Registries>>;

/// Context 0's chunk set (lazily initialised, empty) — the `globalContext` of `cmsplugin.c:635`.
static REGISTRY: OnceLock<RwLock<Registries>> = OnceLock::new();

/// Context 0's lock, created empty on first touch.
fn global() -> &'static RwLock<Registries> {
    REGISTRY.get_or_init(|| RwLock::new(Registries::default()))
}

/// A read guard over whichever chunk set [`read_reg`] selected.
///
/// The two arms hold guards with different lifetimes ('static for Context 0, the context's
/// borrow for a local chunk), so they cannot be one type; `Deref` erases the difference for
/// every caller. Deliberately **not** a clone of the `Registries` — lookups are on hot paths.
enum RegGuard<'a> {
    /// Context 0's chunks (`globalContext.chunks[mc]`).
    Global(RwLockReadGuard<'static, Registries>),
    /// The context's own chunks (`ctx->chunks[mc]`).
    Local(RwLockReadGuard<'a, Registries>),
}

impl Deref for RegGuard<'_> {
    type Target = Registries;

    fn deref(&self) -> &Registries {
        match self {
            RegGuard::Global(g) => g,
            RegGuard::Local(g) => g,
        }
    }
}

/// `_cmsContextGetClientChunk` (`cmsplugin.c:747-772`) — the context's own chunk if it has
/// one, otherwise Context 0's. Never merges the two.
///
/// Recovers transparently from a poisoned lock: a prior panic while holding it must not turn
/// every later lookup into a panic, and lookups are read-only anyway.
fn read_reg(ctx: Option<&Context>) -> RegGuard<'_> {
    match ctx.and_then(Context::plugin_chunk) {
        Some(chunk) => RegGuard::Local(chunk.read().unwrap_or_else(|e| e.into_inner())),
        None => RegGuard::Global(global().read().unwrap_or_else(|e| e.into_inner())),
    }
}

/// Write-lock **Context 0**, recovering from poisoning as [`read_reg`] does.
fn write_reg() -> RwLockWriteGuard<'static, Registries> {
    global().write().unwrap_or_else(|e| e.into_inner())
}

/// `_cmsContextGetClientChunk` on the **write** side — mutate the chunk a registration targets.
///
/// Registration resolves its target through exactly the same two-step as a lookup:
/// `_cmsRegisterParametricCurvesPlugin` and its nine siblings open with
/// `_cmsContextGetClientChunk(ContextID, …)` (`cmsgamma.c:129`) and then prepend into whatever
/// that returned. So a `chunk` of `None` writes **Context 0**, which is why `cmsPlugin(p)` and
/// `cmsPluginTHR(NULL, p)` are the same operation (`cmsplugin.c:541-543`) — here they are the
/// same call, so they cannot drift apart.
///
/// Takes a closure rather than returning a guard because the two arms yield different guard
/// types, and a write guard has no cheap `Deref`-erasing wrapper the way [`RegGuard`] does.
pub(crate) fn with_reg_mut<R>(
    chunk: Option<&PluginChunk>,
    f: impl FnOnce(&mut Registries) -> R,
) -> R {
    match chunk {
        Some(chunk) => f(&mut chunk.write().unwrap_or_else(|e| e.into_inner())),
        None => f(&mut write_reg()),
    }
}

/// Deep-copy Context 0's chunks — what `cmsDupContext(NULL, …)` produces, since `_cmsGetContext`
/// (`cmsplugin.c:714-721`) resolves a NULL id to `globalContext` before the chunk duplication.
pub(crate) fn snapshot_global() -> Registries {
    global().read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// A fresh, **empty** chunk set.
///
/// This is what `cmsCreateContext` installs: it calls every `_cmsAlloc*PluginChunk(ctx, NULL)`
/// (`cmsplugin.c:861-875`), and with `src == NULL` those install the pristine empty chunk
/// (e.g. `cmsgamma.c:114-118`) — **not** a copy of Context 0. The new context therefore
/// shadows Context 0 from birth.
pub(crate) fn empty_chunk() -> PluginChunk {
    Arc::new(RwLock::new(Registries::default()))
}

/// Wrap an existing chunk set for a context to own.
pub(crate) fn chunk_from(reg: Registries) -> PluginChunk {
    Arc::new(RwLock::new(reg))
}

/// Add one extension to a chunk set — the body shared by `cmsPlugin` and `cmsPluginTHR`,
/// which differ only in *which* chunk they target (`cmsplugin.c:541-543` vs `:546`).
pub(crate) fn push_plugin(reg: &mut Registries, plugin: Plugin) {
    match plugin {
        Plugin::ParametricCurves(p) => reg.parametric_curves.push(p),
        Plugin::Optimization(f) => reg.optimizers.push(f),
        Plugin::Formatters(f) => reg.formatters.push(f),
        Plugin::RenderingIntent(p) => reg.intents.push(p),
        Plugin::Tag(p) => {
            // A signature-0 tag is the type-check-disable sentinel.
            if p.sig.as_u32() == 0 {
                reg.avoid_type_check = true;
            }
            // No validation here: `_cmsRegisterTagPlugin` (`cmstypes.c:6214-6236`) copies the
            // descriptor verbatim and cannot fail, so an empty `supported_types` must register
            // just as it does in C. The empty case is handled where the list is indexed.
            reg.tags.push(p);
        }
        Plugin::TagType(p) => reg.tag_types.push(p),
        Plugin::MultiProcessElement(p) => reg.mpe_types.push(p),
        Plugin::Interpolation(f) => reg.interpolators.push(f),
        Plugin::Transform(t) => reg.transforms.push(t),
        Plugin::Parallelization(c) => reg.parallel = Some(c),
    }
}

// ---------------------------------------------------------------------------------------
// Public registration API (cmsPlugin / cmsUnregisterPlugins)
// ---------------------------------------------------------------------------------------

/// Register one extension into **Context 0** (`cmsPlugin`, single-record form).
///
/// `cmsPlugin` is defined as `cmsPluginTHR(NULL, p)` (`cmsplugin.c:541-543`), so this targets
/// the process-global chunk set. Use [`Context::register_plugin`] for the `*THR` form.
///
/// The extension is prepended to its category so it shadows the built-ins and any earlier
/// plugin of the same key. Always succeeds for the supported categories (the C version only
/// fails on an unrecognised magic/version, which the Rust type system makes unrepresentable).
pub fn register_plugin(plugin: Plugin) -> Result<()> {
    with_reg_mut(None, |reg| push_plugin(reg, plugin));
    Ok(())
}

/// Revert **Context 0** to its pristine, built-in-only state (`cmsUnregisterPlugins`).
///
/// `cmsUnregisterPlugins` is `cmsUnregisterPluginsTHR(NULL)` (`cmsplugin.c:626-630`); the
/// per-context form is [`Context::unregister_plugins`]. Contexts that own a chunk are
/// unaffected, because they no longer consult Context 0 at all.
///
/// There is deliberately no way to remove a single plugin (matching lcms: a single
/// `cmsPlugin` call can register many, so removal is all-or-nothing — `cmsplugin.c:775-779`).
pub fn unregister_plugins() {
    with_reg_mut(None, |reg| *reg = Registries::default());
}

// ---------------------------------------------------------------------------------------
// Lookups consulted by the individual subsystems (each searches plugins before built-ins)
// ---------------------------------------------------------------------------------------

/// Resolve a parametric curve type through the registry (`GetParametricCurveByType`,
/// plugin half). Returns `(parameter_count, evaluator)` for the newest plugin covering
/// `abs(type_)`, or `None` to fall through to the built-ins.
pub fn find_parametric_curve(
    ctx: Option<&Context>,
    type_: i32,
) -> Option<(usize, ParametricEvaluator)> {
    let reg = read_reg(ctx);
    let key = type_.abs();
    for p in reg.parametric_curves.iter().rev() {
        if let Some(i) = p.types.iter().position(|&t| t == key) {
            return Some((p.counts[i], p.evaluator));
        }
    }
    None
}

/// Snapshot the registered optimizers, newest first (`_cmsOptimizePipeline` plugin chain).
pub fn optimizers(ctx: Option<&Context>) -> Vec<OptimizerFn> {
    let reg = read_reg(ctx);
    reg.optimizers.iter().rev().copied().collect()
}

/// Find a plugin input 16-bit formatter for `fmt` (`_cmsGetFormatter`, 16-bit input).
pub fn find_input_formatter_16(ctx: Option<&Context>, fmt: PixelFormat) -> Option<Formatter16In> {
    find_formatter(ctx, fmt, FormatterDir::Input, 0).and_then(|f| match f {
        PluginFormatter::In16(f) => Some(f),
        _ => None,
    })
}

/// Find a plugin output 16-bit formatter for `fmt` (`_cmsGetFormatter`, 16-bit output).
pub fn find_output_formatter_16(ctx: Option<&Context>, fmt: PixelFormat) -> Option<Formatter16Out> {
    find_formatter(ctx, fmt, FormatterDir::Output, 0).and_then(|f| match f {
        PluginFormatter::Out16(f) => Some(f),
        _ => None,
    })
}

/// Find a plugin input float formatter for `fmt` (`_cmsGetFormatter`, float input).
pub fn find_input_formatter_float(
    ctx: Option<&Context>,
    fmt: PixelFormat,
) -> Option<FormatterFloatIn> {
    find_formatter(ctx, fmt, FormatterDir::Input, CMS_PACK_FLAGS_FLOAT).and_then(|f| match f {
        PluginFormatter::InFloat(f) => Some(f),
        _ => None,
    })
}

/// Find a plugin output float formatter for `fmt` (`_cmsGetFormatter`, float output).
pub fn find_output_formatter_float(
    ctx: Option<&Context>,
    fmt: PixelFormat,
) -> Option<FormatterFloatOut> {
    find_formatter(ctx, fmt, FormatterDir::Output, CMS_PACK_FLAGS_FLOAT).and_then(|f| match f {
        PluginFormatter::OutFloat(f) => Some(f),
        _ => None,
    })
}

/// Shared factory walk (newest-first) for the four typed formatter lookups.
fn find_formatter(
    ctx: Option<&Context>,
    fmt: PixelFormat,
    dir: FormatterDir,
    flags: u32,
) -> Option<PluginFormatter> {
    let reg = read_reg(ctx);
    for factory in reg.formatters.iter().rev() {
        if let Some(f) = factory(fmt, dir, flags) {
            return Some(f);
        }
    }
    None
}

/// Resolve an intent code through the registry (`SearchIntent`, plugin half).
pub fn find_intent(ctx: Option<&Context>, intent: u32) -> Option<IntentFn> {
    let reg = read_reg(ctx);
    reg.intents
        .iter()
        .rev()
        .find(|p| p.intent == intent)
        .map(|p| p.link)
}

/// All registered intent `(code, description)` pairs, newest first (`cmsGetSupportedIntents`).
pub fn intent_descriptions(ctx: Option<&Context>) -> Vec<(u32, String)> {
    let reg = read_reg(ctx);
    reg.intents
        .iter()
        .rev()
        .map(|p| (p.intent, p.description.clone()))
        .collect()
}

/// Resolve a tag descriptor through the registry (`_cmsGetTagDescriptor`, plugin half).
pub fn find_tag_descriptor(ctx: Option<&Context>, sig: Signature) -> Option<TagDescriptor> {
    let reg = read_reg(ctx);
    reg.tags
        .iter()
        .rev()
        .find(|p| p.sig == sig)
        .map(|p| p.descriptor.clone())
}

/// `_cmsAvoidTypeCheckOnTags` — whether a signature-0 tag plugin disabled tag type checking.
pub fn avoid_type_check(ctx: Option<&Context>) -> bool {
    read_reg(ctx).avoid_type_check
}

/// Resolve a tag-type handler through the registry (`_cmsGetTagTypeHandler`, plugin half).
pub fn find_tag_type(ctx: Option<&Context>, sig: Signature) -> Option<TagTypePlugin> {
    let reg = read_reg(ctx);
    reg.tag_types.iter().rev().find(|p| p.sig == sig).cloned()
}

/// Resolve an MPE element reader through the registry (`SupportedMPEtypes`, plugin half).
pub fn find_mpe_reader(ctx: Option<&Context>, sig: Signature) -> Option<MpeReadFn> {
    let reg = read_reg(ctx);
    reg.mpe_types.iter().rev().find(|p| p.sig == sig).map(|p| p.read)
}

/// Ask the registry for an interpolation kernel (`_cmsSetInterpolationRoutine`, plugin half).
///
/// Walks the registered factories newest-first and returns the first non-declining answer;
/// `None` means "no plugin handles this geometry", so the caller must fall back to the
/// built-in dispatch — the exact contract of C's `ptr->Interpolators` /
/// `DefaultInterpolatorsFactory` two-step.
pub fn find_interpolation_factory(
    ctx: Option<&Context>,
    n_inputs: usize,
    n_outputs: usize,
    flags: u32,
) -> Option<InterpFunction> {
    let reg = read_reg(ctx);
    reg.interpolators
        .iter()
        .rev()
        .find_map(|factory| factory(n_inputs, n_outputs, flags))
}

/// Offer a transform to the registered factories (`AllocEmptyTransform`, plugin loop).
///
/// C walks `ctx->TransformCollection` from the head, i.e. **newest first**, and the first
/// factory that returns `TRUE` takes control ("last plugin in the declaration order takes
/// control"). The pipeline, formats and flags are passed by `&mut` so a factory can rewrite
/// them, matching the C out-parameters.
pub fn find_transform_factory(
    ctx: Option<&Context>,
    lut: &mut Pipeline,
    input_format: &mut PixelFormat,
    output_format: &mut PixelFormat,
    flags: &mut u32,
) -> Option<TransformFit> {
    // Collect first: a factory may itself touch the registry, and holding the read lock
    // across the callback would deadlock the `RwLock`.
    let factories: Vec<TransformFactory> =
        read_reg(ctx).transforms.iter().rev().copied().collect();
    factories
        .into_iter()
        .find_map(|factory| factory(lut, input_format, output_format, flags))
}

/// The active parallelization plugin, if any (`cmsPluginParalellization`).
pub fn parallelization(ctx: Option<&Context>) -> Option<ParallelizationConfig> {
    read_reg(ctx).parallel
}

// ---------------------------------------------------------------------------------------
// A ready-made multi-threaded scheduler
// ---------------------------------------------------------------------------------------

/// Decide how many slices to cut the job into (`_cmsThrCountSlices`).
///
/// The reference plugin budgets **128 KiB of traffic per worker** (input + output), clamps to
/// `MaxWorkers` (auto-detected when [`CMS_THREADED_GUESS_MAX_THREADS`]) and never asks for
/// more slices than there are lines to hand out.
fn count_slices(max_workers: i32, lines: usize, bpl_in: usize, bpl_out: usize) -> usize {
    let cap = if max_workers == CMS_THREADED_GUESS_MAX_THREADS {
        std::thread::available_parallelism().map_or(1, |n| n.get())
    } else if max_workers > 0 {
        max_workers as usize
    } else {
        1
    };

    let mem = lines.saturating_mul(bpl_in).saturating_add(lines.saturating_mul(bpl_out));
    (mem / (128 * 1024)).clamp(1, cap).min(lines)
}

/// A multi-threaded `SchedulerFn`, the analogue of the reference `threaded` plugin's
/// `_cmsThrScheduler`.
///
/// Splits the scanlines across [`ParallelizationConfig::max_workers`] threads (using the
/// 128 KiB-per-worker budget of `_cmsThrCountSlices`) and runs each slice through
/// [`Transform::run_worker`] — the un-parallelized pixel loop that
/// [`ParallelizationConfig::scheduler`] displaced. Slice 0 runs on the calling thread, exactly
/// as the C scheduler does, and [`std::thread::scope`] joins the rest before returning.
///
/// # Safety and correctness
/// The slices are cut with [`slice::split_at`]/[`slice::split_at_mut`], so every worker owns a
/// **disjoint** region and no `unsafe` is needed. The result is therefore byte-identical to the
/// single-threaded run.
///
/// # Deviations from the C plugin
/// * C also has a **per-pixel** split (`SlicePerPixels`) for single-line jobs. That path relies
///   on pointer arithmetic with pixel spacing; this port declines to thread a single line and
///   runs it whole, which is a performance choice only — never a correctness one.
/// * Planar formats are likewise run whole: this port's kernels address planes as offsets from
///   the **buffer start** (`bytes_per_plane_*`), which a per-line slice would invalidate.
/// * When the buffers are shorter than `lines * bytes_per_line`, the job is run whole rather
///   than risking a split past the end.
pub fn threaded_scheduler(
    t: &Transform,
    input: &[u8],
    output: &mut [u8],
    ppl: usize,
    lines: usize,
    stride: &Stride,
) {
    let bpl_in = stride.bytes_per_line_in;
    let bpl_out = stride.bytes_per_line_out;

    let splittable = lines > 1
        && bpl_in > 0
        && bpl_out > 0
        && !t.input_format().planar()
        && !t.output_format().planar()
        && input.len() >= lines * bpl_in
        && output.len() >= lines * bpl_out;

    let slices = if splittable {
        count_slices(t.max_workers(), lines, bpl_in, bpl_out)
    } else {
        1
    };

    if slices <= 1 {
        t.run_worker(input, output, ppl, lines, stride);
        return;
    }

    // Cut the buffers up front so each job owns a disjoint region; the last slice absorbs the
    // lines left over by the integer division (C: "Add left lines because rounding").
    let lines_per_slice = lines / slices;
    let mut jobs: Vec<(&[u8], &mut [u8], usize)> = Vec::with_capacity(slices);
    let mut in_rest = input;
    let mut out_rest = &mut output[..];
    let mut remaining = lines;
    for i in 0..slices {
        let n = if i + 1 == slices { remaining } else { lines_per_slice };
        let (in_head, in_tail) = in_rest.split_at(n * bpl_in);
        let (out_head, out_tail) = std::mem::take(&mut out_rest).split_at_mut(n * bpl_out);
        in_rest = in_tail;
        out_rest = out_tail;
        remaining -= n;
        jobs.push((in_head, out_head, n));
    }

    std::thread::scope(|scope| {
        let mut jobs = jobs.into_iter();
        let master = jobs.next();
        for (src, dst, n) in jobs {
            scope.spawn(move || t.run_worker(src, dst, ppl, n, stride));
        }
        if let Some((src, dst, n)) = master {
            t.run_worker(src, dst, ppl, n, stride);
        }
    });
}

// ---------------------------------------------------------------------------------------
// Tests — see also `tests/plugin_registry.rs` for the behaviour-mutating scenarios, which
// run in a separate process so the process-global registry can never leak into the unit
// tests (which rely on it staying empty).
// ---------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use std::sync::Mutex;

    /// Serialises the registry-mutating unit tests in this module against each other.
    static LOCK: Mutex<()> = Mutex::new(());

    fn plug_eval(type_: i32, params: &[f64; 10], r: f64) -> f64 {
        // A trivial custom curve: type 200 == scale by params[0].
        let _ = type_;
        r * params[0]
    }

    #[test]
    fn parametric_registry_roundtrip() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();
        assert!(find_parametric_curve(None, 200).is_none());

        register_plugin(Plugin::ParametricCurves(ParametricCurvePlugin {
            types: vec![200],
            counts: vec![1],
            evaluator: plug_eval,
        }))
        .unwrap();

        let (count, ev) = find_parametric_curve(None, 200).expect("registered curve");
        assert_eq!(count, 1);
        assert_eq!(ev(200, &{ let mut p = [0.0; 10]; p[0] = 2.0; p }, 0.25), 0.5);
        // Negative (inverse) type resolves to the same collection.
        assert!(find_parametric_curve(None, -200).is_some());

        unregister_plugins();
        assert!(find_parametric_curve(None, 200).is_none());
    }

    #[test]
    fn tag_sig0_sets_avoid_flag() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();
        assert!(!avoid_type_check(None));

        register_plugin(Plugin::Tag(TagPlugin {
            sig: Signature(0),
            descriptor: TagDescriptor {
                elem_count: 1,
                supported_types: vec![],
                decide_type: None,
            },
        }))
        .unwrap();
        assert!(avoid_type_check(None));

        unregister_plugins();
        assert!(!avoid_type_check(None));
    }

    /// The signature-0 sentinel that flips `_cmsAvoidTypeCheckOnTags` on.
    fn avoid_check_sentinel() -> Plugin {
        Plugin::Tag(TagPlugin {
            sig: Signature(0),
            descriptor: TagDescriptor { elem_count: 1, supported_types: vec![], decide_type: None },
        })
    }

    /// A saved profile whose `chad` tag holds only `count` s15Fixed16 values.
    ///
    /// The descriptor demands nine, so a shorter body is exactly the payload C accepts under
    /// `_cmsAvoidTypeCheckOnTags` (`cmsio0.c:1860-1872`) and rejects without it.
    fn profile_with_short_chad(count: i32) -> Vec<u8> {
        let mut body = crate::tags::handler::type_sig::S15FIXED16_ARRAY
            .as_u32()
            .to_be_bytes()
            .to_vec();
        body.extend_from_slice(&0u32.to_be_bytes()); // reserved half of the 8-byte type base
        for i in 1..=count {
            body.extend_from_slice(&(i << 16).to_be_bytes());
        }

        let mut p = crate::profile::Profile::create_placeholder(crate::context::Context::new());
        p.write_raw_tag(crate::tags::descriptor::tag_sig::CHROMATIC_ADAPTATION, &body)
            .unwrap();
        p.save_to_mem().unwrap()
    }

    #[test]
    fn avoid_type_check_suppresses_the_elem_count_check() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();

        let bytes = profile_with_short_chad(4);
        let chad = crate::tags::descriptor::tag_sig::CHROMATIC_ADAPTATION;

        // Without the sentinel the short body trips the item-count check.
        let mut p = crate::profile::Profile::open_from_mem(crate::context::Context::new(), &bytes)
            .unwrap();
        assert_eq!(p.read_tag(chad).unwrap_err().code, ErrorCode::CorruptionDetected);

        // With it, C skips the count check along with the type checks, and so must we.
        register_plugin(avoid_check_sentinel()).unwrap();
        let mut p = crate::profile::Profile::open_from_mem(crate::context::Context::new(), &bytes)
            .unwrap();
        match p.read_tag(chad).expect("avoidCheck must suppress the item-count check") {
            crate::tags::TagValue::S15Fixed16Array(v) => assert_eq!(v.len(), 4),
            other => panic!("expected an s15Fixed16 array, got {other:?}"),
        }

        unregister_plugins();
    }

    #[test]
    fn missing_descriptor_is_fatal_for_a_cached_tag() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();

        // A plugin-only tag: writing it caches a cooked value plus its type handler.
        let custom = Signature::new(b"tst8");
        register_plugin(Plugin::Tag(TagPlugin {
            sig: custom,
            descriptor: TagDescriptor {
                elem_count: 1,
                supported_types: vec![crate::tags::handler::type_sig::S15FIXED16_ARRAY],
                decide_type: None,
            },
        }))
        .unwrap();

        let mut p = crate::profile::Profile::create_placeholder(crate::context::Context::new());
        p.write_tag(custom, crate::tags::TagValue::S15Fixed16Array(vec![1.0]))
            .unwrap();
        assert!(p.read_tag(custom).is_ok());

        // Drop the descriptor: C's cached branch resolves it and errors on NULL
        // (`cmsio0.c:1782-1783`), rather than handing back the stale value.
        unregister_plugins();
        assert_eq!(p.read_tag(custom).unwrap_err().code, ErrorCode::UnknownExtension);
        assert_eq!(p.read_raw_tag(custom).unwrap_err().code, ErrorCode::UnknownExtension);
    }

    #[test]
    fn empty_supported_types_errors_at_the_use_site_not_at_registration() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();

        // Registration itself must succeed: `_cmsRegisterTagPlugin` (`cmstypes.c:6214-6236`)
        // copies the descriptor verbatim and has no failure path for an empty type list.
        let custom = Signature::new(b"tst9");
        register_plugin(Plugin::Tag(TagPlugin {
            sig: custom,
            descriptor: TagDescriptor { elem_count: 1, supported_types: vec![], decide_type: None },
        }))
        .expect("an empty type list is a legal registration in C");
        assert!(crate::tags::get_descriptor(custom).is_some());

        // `cmsWriteTag` reads `SupportedTypes[0]`, fails `IsTypeSupported` on the garbage it gets,
        // and aborts with cmsERROR_UNKNOWN_EXTENSION (`cmsio0.c:1974-1985`) — never a panic.
        let mut p = crate::profile::Profile::create_placeholder(crate::context::Context::new());
        let err = p
            .write_tag(custom, crate::tags::TagValue::S15Fixed16Array(vec![1.0]))
            .expect_err("an empty type list must not be writable");
        assert_eq!(err.code, ErrorCode::UnknownExtension);

        // `SaveTags` reaches the same index for a *cooked* tag, but there C only signals and
        // skips it (`cmsio0.c:1490-1498`). Reproduce that: cook the tag while a usable descriptor
        // is registered, then shadow it with an empty-list one before saving.
        unregister_plugins();
        register_plugin(Plugin::Tag(TagPlugin {
            sig: custom,
            descriptor: TagDescriptor {
                elem_count: 1,
                supported_types: vec![crate::tags::handler::type_sig::S15FIXED16_ARRAY],
                decide_type: None,
            },
        }))
        .unwrap();
        let mut p = crate::profile::Profile::create_placeholder(crate::context::Context::new());
        p.write_tag(custom, crate::tags::TagValue::S15Fixed16Array(vec![1.0]))
            .unwrap();

        // Later registrations shadow earlier ones, so this is the descriptor `SaveTags` will see.
        register_plugin(Plugin::Tag(TagPlugin {
            sig: custom,
            descriptor: TagDescriptor { elem_count: 1, supported_types: vec![], decide_type: None },
        }))
        .unwrap();
        let saved = p.save_to_mem().expect("an unwritable tag is skipped, not fatal");
        let reopened =
            crate::profile::Profile::open_from_mem(crate::context::Context::new(), &saved).unwrap();
        assert!(!reopened.is_tag(custom), "the skipped tag must not reach the file");

        unregister_plugins();
    }

    #[test]
    fn parallelization_last_wins() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();
        assert!(parallelization(None).is_none());
        register_plugin(Plugin::Parallelization(ParallelizationConfig {
            max_workers: 4,
            worker_flags: 0,
            scheduler: threaded_scheduler,
        }))
        .unwrap();
        register_plugin(Plugin::Parallelization(ParallelizationConfig {
            max_workers: 8,
            worker_flags: 1,
            scheduler: threaded_scheduler,
        }))
        .unwrap();
        assert_eq!(parallelization(None).unwrap().max_workers, 8);
        unregister_plugins();
    }

    // -----------------------------------------------------------------------------------
    // Per-context chunks (`_cmsContextGetClientChunk`, `cmsplugin.c:747-772`)
    // -----------------------------------------------------------------------------------

    /// A parametric-curve plugin for `type_`, so each test can register a distinguishable one.
    fn curve_plugin(type_: i32) -> Plugin {
        Plugin::ParametricCurves(ParametricCurvePlugin {
            types: vec![type_],
            counts: vec![1],
            evaluator: plug_eval,
        })
    }

    /// Step 2 of `_cmsContextGetClientChunk`: a context with no chunk of its own reads
    /// Context 0. This is the `ContextID == NULL` path, and it is what keeps every existing
    /// consumer working after per-context chunks landed.
    #[test]
    fn context_without_a_chunk_reads_the_global() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();

        let ctx = Context::new();
        assert!(find_parametric_curve(Some(&ctx), 210).is_none());

        register_plugin(curve_plugin(210)).unwrap();
        assert!(
            find_parametric_curve(Some(&ctx), 210).is_some(),
            "a chunk-less context must fall back to Context 0"
        );

        unregister_plugins();
    }

    /// `cmsPluginTHR` scopes a registration to the chunk the context resolves to: for a context
    /// that owns one (`cmsCreateContext`), `_cmsRegisterXPlugin` writes only there
    /// (`cmsgamma.c:129,155-156`), so a sibling context and Context 0 must not see it.
    #[test]
    fn local_registration_is_invisible_elsewhere() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();

        let mut a = Context::with_plugin(curve_plugin(211), None).unwrap();
        a.register_plugin(curve_plugin(212)).unwrap();

        let b = Context::new();
        for type_ in [211, 212] {
            assert!(find_parametric_curve(Some(&a), type_).is_some(), "the owner must see it");
            assert!(find_parametric_curve(Some(&b), type_).is_none(), "a sibling must not");
            assert!(find_parametric_curve(None, type_).is_none(), "Context 0 must not");
        }

        unregister_plugins();
    }

    /// A context that owns **no** chunk *is* Context 0 — `_cmsGetContext` maps a NULL id to
    /// `globalContext` ("On 0, use global settings", `cmsplugin.c:719-721`), so `cmsPluginTHR`
    /// and `cmsPluginTHR(NULL, …)` alias `cmsPlugin`. Registering through such a context must
    /// therefore land globally and must NOT isolate it or shadow Context 0 away.
    #[test]
    fn a_chunkless_context_registers_into_context_zero() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();

        register_plugin(curve_plugin(224)).unwrap();
        let mut ctx = Context::new();
        assert!(find_parametric_curve(Some(&ctx), 224).is_some());

        ctx.register_plugin(curve_plugin(225)).unwrap();
        assert!(find_parametric_curve(Some(&ctx), 225).is_some(), "its own registration");
        assert!(
            find_parametric_curve(Some(&ctx), 224).is_some(),
            "registering must not shadow Context 0 away from a chunk-less context"
        );
        // The registration went into Context 0, so it is visible process-wide.
        assert!(find_parametric_curve(None, 225).is_some(), "must reach Context 0");
        assert!(find_parametric_curve(Some(&Context::new()), 225).is_some(), "and every peer");

        // `cmsUnregisterPluginsTHR(NULL)` is `cmsUnregisterPlugins()`: it clears Context 0.
        ctx.unregister_plugins();
        assert!(find_parametric_curve(None, 224).is_none());
        assert!(find_parametric_curve(None, 225).is_none());

        unregister_plugins();
    }

    /// Step 1 of `_cmsContextGetClientChunk` **shadows** rather than merges: once `ctx->chunks[mc]`
    /// is non-NULL the function returns it and never looks at `globalContext`. So a plugin
    /// registered globally *before* the context grew its own chunk becomes invisible to it.
    #[test]
    fn a_local_chunk_shadows_the_global_entirely() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();

        register_plugin(curve_plugin(226)).unwrap();
        let mut ctx = Context::with_plugin(curve_plugin(227), None).unwrap();
        ctx.register_plugin(curve_plugin(228)).unwrap();

        // Everything in the own chunk is visible...
        assert!(find_parametric_curve(Some(&ctx), 227).is_some());
        assert!(find_parametric_curve(Some(&ctx), 228).is_some());
        // ...and Context 0 is not consulted at all, even for what it alone holds.
        assert!(
            find_parametric_curve(Some(&ctx), 226).is_none(),
            "the chunks must shadow, not merge"
        );
        // Context 0 still has it, for anyone still reading Context 0.
        assert!(find_parametric_curve(None, 226).is_some());

        unregister_plugins();
    }

    /// `cmsDupContext` copies the chunks **at dup time** (`_cmsAlloc*Chunk(ctx, src)` →
    /// `DupPluginCurvesList`, `cmsgamma.c:70-102`), so the two contexts diverge afterwards.
    /// `Clone` is the opposite: it shares the chunk, because a copied `cmsContext` pointer is
    /// the same context.
    #[test]
    fn dup_snapshots_while_clone_shares() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();

        let mut parent = Context::with_plugin(curve_plugin(214), None).unwrap();

        let snapshot = parent.dup(None);
        let mut shared = parent.clone();

        // Both carry what existed at the split.
        assert!(find_parametric_curve(Some(&snapshot), 214).is_some());
        assert!(find_parametric_curve(Some(&shared), 214).is_some());

        // A later registration reaches the sharing clone but not the snapshot.
        parent.register_plugin(curve_plugin(215)).unwrap();
        assert!(
            find_parametric_curve(Some(&shared), 215).is_some(),
            "Clone shares the chunk, so it must see later registrations"
        );
        assert!(
            find_parametric_curve(Some(&snapshot), 215).is_none(),
            "dup froze the chunk at copy time"
        );

        // ...and the snapshot is independent in the other direction too.
        shared.register_plugin(curve_plugin(216)).unwrap();
        assert!(find_parametric_curve(Some(&parent), 216).is_some());
        assert!(find_parametric_curve(Some(&snapshot), 216).is_none());

        unregister_plugins();
    }

    /// `cmsDupContext(NULL, …)` still yields a context owning a chunk: `_cmsGetContext` maps the
    /// NULL id to `globalContext` (`cmsplugin.c:714-721`) and the duplication copies *that*.
    #[test]
    fn dup_of_a_chunkless_context_snapshots_context_zero() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();

        register_plugin(curve_plugin(217)).unwrap();
        let snapshot = Context::new().dup(None);
        assert!(find_parametric_curve(Some(&snapshot), 217).is_some(), "copied from Context 0");

        // Owning a chunk now, it is deaf to further global registrations.
        register_plugin(curve_plugin(218)).unwrap();
        assert!(find_parametric_curve(Some(&snapshot), 218).is_none());

        unregister_plugins();
    }

    /// `cmsUnregisterPluginsTHR(ctx)` resets that context's chunks only (`cmsplugin.c:780-796`);
    /// Context 0 is the business of the non-THR `cmsUnregisterPlugins`.
    #[test]
    fn local_unregister_leaves_the_global_alone() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();

        register_plugin(curve_plugin(219)).unwrap();
        let mut ctx = Context::with_plugin(curve_plugin(220), None).unwrap();

        ctx.unregister_plugins();
        assert!(find_parametric_curve(Some(&ctx), 220).is_none(), "the local one is gone");
        assert!(find_parametric_curve(None, 219).is_some(), "Context 0 must be untouched");
        // An emptied chunk is still a chunk, so Context 0 stays shadowed for this context.
        assert!(
            find_parametric_curve(Some(&ctx), 219).is_none(),
            "an empty own chunk still shadows Context 0"
        );

        unregister_plugins();
    }

    /// `cmsCreateContext(Plugin, UserData)` allocates its chunks with `src == NULL`
    /// (`cmsplugin.c:861-875`), i.e. **empty** — not copied from Context 0. The new context
    /// therefore sees its own plugin and nothing global.
    #[test]
    fn with_plugin_starts_empty_and_shadows_the_global() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unregister_plugins();

        register_plugin(curve_plugin(222)).unwrap();
        let ctx = Context::with_plugin(curve_plugin(223), None).unwrap();

        assert!(find_parametric_curve(Some(&ctx), 223).is_some(), "its own plugin");
        assert!(
            find_parametric_curve(Some(&ctx), 222).is_none(),
            "cmsCreateContext must not inherit Context 0"
        );

        unregister_plugins();
    }
}
