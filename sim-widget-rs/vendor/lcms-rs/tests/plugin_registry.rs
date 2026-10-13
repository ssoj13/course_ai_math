//! Behaviour-mutating plugin-subsystem tests (`cmsplugin.c` parity).
//!
//! These live in their own integration-test binary on purpose: the plugin registry is
//! process-global, so registering an extension here can never leak into the library unit
//! tests (which run in a separate process and rely on the registry staying empty, i.e. every
//! built-in path behaving byte-for-byte). Within this file the scenarios share one process, so
//! a [`Mutex`] serialises them and each unregisters on entry and exit.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use lcms_rs::cmm::transform::{create_transform, Transform};
use lcms_rs::cmm::Stride;
use lcms_rs::color::gamma::ToneCurve;
use lcms_rs::context::Context;
use lcms_rs::math::interp::InterpParams;
use lcms_rs::pipeline::stage::{Stage, StageEval};
use lcms_rs::pipeline::{Pipeline, StageLoc};
use lcms_rs::plugin::{
    self, InterpFunction, MpeTypePlugin, ParallelizationConfig, ParametricCurvePlugin, Plugin,
    PluginXform, TagPlugin, TransformFit,
};
use lcms_rs::tags::{get_descriptor, TagDescriptor};
use lcms_rs::types::format::{FormatBuilder, PixelFormat};
use lcms_rs::types::sig::Signature;
use lcms_rs::virt::create_srgb_profile;

/// Serialises the registry-mutating scenarios against each other.
static LOCK: Mutex<()> = Mutex::new(());

fn guard() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// --- ParametricCurves (DEFERRED #4) ----------------------------------------------------

/// A custom parametric curve: type 200 == "scale input by params[0]".
fn scale_eval(_type: i32, params: &[f64; 10], r: f64) -> f64 {
    r * params[0]
}

#[test]
fn custom_parametric_curve_is_accepted() {
    let _g = guard();
    plugin::unregister_plugins();

    // The built-in engine rejects an unknown parametric type.
    assert!(ToneCurve::build_parametric(200, &[2.0]).is_err());

    plugin::register_plugin(Plugin::ParametricCurves(ParametricCurvePlugin {
        types: vec![200],
        counts: vec![1],
        evaluator: scale_eval,
    }))
    .unwrap();

    // Now the same call builds a curve whose evaluation goes through the plugin.
    let curve = ToneCurve::build_parametric(200, &[2.0]).expect("plugin curve builds");
    assert!((curve.eval_f32(0.25) - 0.5).abs() < 1e-6, "got {}", curve.eval_f32(0.25));

    // Unregistering restores built-in-only behaviour.
    plugin::unregister_plugins();
    assert!(ToneCurve::build_parametric(200, &[2.0]).is_err());
}

// --- Optimization (DEFERRED #5) --------------------------------------------------------

static OPT_HITS: AtomicUsize = AtomicUsize::new(0);

/// A plugin optimizer that records its invocation and "wins" without mutating the pipeline.
fn recording_optimizer(
    _lut: &mut Pipeline,
    _intent: u32,
    _in_fmt: &mut PixelFormat,
    _out_fmt: &mut PixelFormat,
    _flags: &mut u32,
) -> lcms_rs::error::Result<bool> {
    OPT_HITS.fetch_add(1, Ordering::SeqCst);
    Ok(true)
}

#[test]
fn custom_optimizer_is_consulted_first() {
    let _g = guard();
    plugin::unregister_plugins();
    OPT_HITS.store(0, Ordering::SeqCst);

    // A single non-identity matrix survives PreOptimize, so the optimizer loop is reached.
    let mut lut = Pipeline::alloc(3, 3).unwrap();
    let m = [2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    lut.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m, None).unwrap());

    plugin::register_plugin(Plugin::Optimization(recording_optimizer)).unwrap();

    let (mut inf, mut outf, mut flags) = (PixelFormat(0), PixelFormat(0), 0u32);
    lcms_rs::cmm::optimize::optimize_pipeline(&mut lut, 0, &mut inf, &mut outf, &mut flags).unwrap();

    assert_eq!(OPT_HITS.load(Ordering::SeqCst), 1, "plugin optimizer must run");

    plugin::unregister_plugins();
    OPT_HITS.store(0, Ordering::SeqCst);
    let mut lut2 = Pipeline::alloc(3, 3).unwrap();
    lut2.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m, None).unwrap());
    lcms_rs::cmm::optimize::optimize_pipeline(&mut lut2, 0, &mut inf, &mut outf, &mut flags).unwrap();
    assert_eq!(OPT_HITS.load(Ordering::SeqCst), 0, "no plugin optimizer after unregister");
}

// --- Tag + avoid-type-check (DEFERRED #7) ----------------------------------------------

#[test]
fn tag_plugin_descriptor_and_sig0_flag() {
    let _g = guard();
    plugin::unregister_plugins();

    // A brand-new tag signature is unknown to the built-in table.
    let custom = Signature::new(b"tst1");
    assert!(get_descriptor(custom).is_none());
    assert!(!plugin::avoid_type_check(None));

    // Register a descriptor for it plus the signature-0 type-check sentinel.
    plugin::register_plugin(Plugin::Tag(TagPlugin {
        sig: custom,
        descriptor: TagDescriptor { elem_count: 1, supported_types: vec![], decide_type: None },
    }))
    .unwrap();
    plugin::register_plugin(Plugin::Tag(TagPlugin {
        sig: Signature(0),
        descriptor: TagDescriptor { elem_count: 1, supported_types: vec![], decide_type: None },
    }))
    .unwrap();

    // The descriptor lookup now resolves the plugin tag, and type-checking is disabled.
    assert!(get_descriptor(custom).is_some());
    assert!(plugin::avoid_type_check(None));

    plugin::unregister_plugins();
    assert!(get_descriptor(custom).is_none());
    assert!(!plugin::avoid_type_check(None));
}

// --- MultiProcessElement type (DEFERRED #3) --------------------------------------------

/// A trivial 1->1 plugin stage.
#[derive(Debug)]
struct DoublerStage;
impl StageEval for DoublerStage {
    fn eval(&self, input: &[f32], output: &mut [f32]) {
        output[0] = input[0] * 2.0;
    }
    fn input_channels(&self) -> u32 {
        1
    }
    fn output_channels(&self) -> u32 {
        1
    }
}

/// A plugin MPE reader that builds the doubler stage (body content is irrelevant here).
fn read_doubler(_io: &mut dyn plugin::ReadSeek) -> lcms_rs::error::Result<Stage> {
    Ok(Stage::alloc_custom(Signature::new(b"dbl "), Arc::new(DoublerStage)))
}

#[test]
fn custom_mpe_type_has_signature_and_reader() {
    let _g = guard();
    plugin::unregister_plugins();

    let sig = Signature::new(b"dbl ");
    assert!(plugin::find_mpe_reader(None, sig).is_none());

    plugin::register_plugin(Plugin::MultiProcessElement(MpeTypePlugin {
        sig,
        read: read_doubler,
    }))
    .unwrap();

    // The reader is now discoverable by the `mpet` element dispatcher...
    let reader = plugin::find_mpe_reader(None, sig).expect("mpe reader registered");
    let mut empty: &[u8] = &[];
    let mut cursor = std::io::Cursor::new(&mut empty);
    let stage = reader(&mut cursor).unwrap();
    // ...and the produced Custom stage carries its own on-disk type signature (closes the
    // `Signature(0)` hole) and evaluates through the plugin.
    assert_eq!(stage.type_signature(), sig);
    let mut out = [0.0f32];
    stage.eval(&[0.5], &mut out);
    assert!((out[0] - 1.0).abs() < 1e-6, "got {}", out[0]);

    plugin::unregister_plugins();
    assert!(plugin::find_mpe_reader(None, sig).is_none());
}

// --- Interpolation (DEFERRED #5) -------------------------------------------------------

/// A 16-bit kernel that ignores the CLUT entirely and stamps a recognizable constant.
fn stamping_lerp16(p: &InterpParams, _table: &[u16], _input: &[u16], output: &mut [u16]) {
    for out in output.iter_mut().take(p.n_outputs) {
        *out = 0xBEEF;
    }
}

/// Claims only the 3->3 16-bit geometry, so the decline path stays exercised too.
fn stamping_factory(n_inputs: usize, n_outputs: usize, _flags: u32) -> Option<InterpFunction> {
    if n_inputs == 3 && n_outputs == 3 {
        Some(InterpFunction::Lerp16(stamping_lerp16))
    } else {
        None
    }
}

/// A 2-node 3->3 identity RGB CLUT.
fn identity_clut_2() -> Vec<u16> {
    let mut t = Vec::new();
    for r in 0..2u32 {
        for g in 0..2u32 {
            for b in 0..2u32 {
                t.extend_from_slice(&[
                    (r * 0xffff) as u16,
                    (g * 0xffff) as u16,
                    (b * 0xffff) as u16,
                ]);
            }
        }
    }
    t
}

#[test]
fn custom_interpolation_replaces_the_kernel() {
    let _g = guard();
    plugin::unregister_plugins();

    let table = identity_clut_2();
    let input = [0x4000u16, 0x8000, 0xC000];

    // Built-in tetrahedral kernel: an identity CLUT reproduces the input.
    let p = InterpParams::new(2, 3, 3, 0).unwrap();
    let mut out = [0u16; 3];
    p.eval16(&table, &input, &mut out);
    assert_eq!(out, input, "built-in identity CLUT");

    plugin::register_plugin(Plugin::Interpolation(stamping_factory)).unwrap();

    // Params built *after* registration carry the plugin kernel, and evaluation goes
    // through it (`_cmsSetInterpolationRoutine` is consulted at param-build time, as in C).
    let p = InterpParams::new(2, 3, 3, 0).unwrap();
    assert!(p.interp16.is_some(), "factory must install a kernel");
    let mut out = [0u16; 3];
    p.eval16(&table, &input, &mut out);
    assert_eq!(out, [0xBEEF; 3], "plugin kernel must replace the built-in");

    // A geometry the factory declines still gets the built-in kernel.
    let p4 = InterpParams::new(2, 4, 4, 0).unwrap();
    assert!(p4.interp16.is_none(), "declined geometry keeps the built-in");

    plugin::unregister_plugins();
    let p = InterpParams::new(2, 3, 3, 0).unwrap();
    assert!(p.interp16.is_none());
    let mut out = [0u16; 3];
    p.eval16(&table, &input, &mut out);
    assert_eq!(out, input, "built-in behaviour returns after unregister");
}

// --- Transform (DEFERRED #5) -----------------------------------------------------------

static XFORM_HITS: AtomicUsize = AtomicUsize::new(0);

/// A replacement pixel loop that inverts every byte instead of doing colour management.
fn inverting_xform(
    _t: &Transform,
    src: &[u8],
    dst: &mut [u8],
    ppl: usize,
    lines: usize,
    stride: &Stride,
) {
    XFORM_HITS.fetch_add(1, Ordering::SeqCst);
    for line in 0..lines {
        let src_line = line * stride.bytes_per_line_in;
        let dst_line = line * stride.bytes_per_line_out;
        for i in 0..ppl * 3 {
            dst[dst_line + i] = !src[src_line + i];
        }
    }
}

/// The factory always claims the transform and attaches a marker as private state.
fn inverting_factory(
    _lut: &mut Pipeline,
    _in_fmt: &mut PixelFormat,
    _out_fmt: &mut PixelFormat,
    _flags: &mut u32,
) -> Option<TransformFit> {
    Some(TransformFit {
        xform: PluginXform::Full(inverting_xform),
        user_data: Some(Arc::new(0xC0FFEEu32)),
    })
}

fn rgb8() -> PixelFormat {
    // PT_RGB (4), 3 channels, 1 byte each.
    FormatBuilder::new().colorspace(4).channels(3).bytes(1).build()
}

fn srgb_transform(ctx: &Context) -> Transform {
    let mut a = create_srgb_profile().unwrap();
    let mut b = create_srgb_profile().unwrap();
    create_transform(ctx, &mut a, rgb8(), &mut b, rgb8(), 0, 0).unwrap()
}

#[test]
fn custom_transform_replaces_the_pixel_loop() {
    let _g = guard();
    plugin::unregister_plugins();
    XFORM_HITS.store(0, Ordering::SeqCst);

    let ctx = Context::new();
    let src = [10u8, 20, 30, 40, 50, 60];

    // Baseline: sRGB->sRGB is near-identity, and certainly not a byte inversion.
    let mut base = [0u8; 6];
    srgb_transform(&ctx).do_transform(&src, &mut base, 2);
    assert_ne!(base, [!10u8, !20, !30, !40, !50, !60]);
    assert_eq!(XFORM_HITS.load(Ordering::SeqCst), 0);

    plugin::register_plugin(Plugin::Transform(inverting_factory)).unwrap();

    let t = srgb_transform(&ctx);
    assert!(t.user_data().is_some(), "factory user data must reach the transform");
    let mut out = [0u8; 6];
    t.do_transform(&src, &mut out, 2);
    assert_eq!(out, [!10u8, !20, !30, !40, !50, !60], "plugin loop must run");
    assert_eq!(XFORM_HITS.load(Ordering::SeqCst), 1);

    // Unregistering restores the built-in kernel byte-for-byte.
    plugin::unregister_plugins();
    let mut after = [0u8; 6];
    srgb_transform(&ctx).do_transform(&src, &mut after, 2);
    assert_eq!(after, base, "built-in transform returns after unregister");
    assert_eq!(XFORM_HITS.load(Ordering::SeqCst), 1, "no further plugin calls");
}

// --- Parallelization (DEFERRED #5) -----------------------------------------------------

static SCHED_HITS: AtomicUsize = AtomicUsize::new(0);

/// Counts scheduler invocations, then delegates to the real multi-threaded splitter.
fn counting_scheduler(
    t: &Transform,
    src: &[u8],
    dst: &mut [u8],
    ppl: usize,
    lines: usize,
    stride: &Stride,
) {
    SCHED_HITS.fetch_add(1, Ordering::SeqCst);
    plugin::threaded_scheduler(t, src, dst, ppl, lines, stride);
}

#[test]
fn parallelization_splits_lines_and_matches_single_threaded() {
    let _g = guard();
    plugin::unregister_plugins();
    SCHED_HITS.store(0, Ordering::SeqCst);

    let ctx = Context::new();

    // A buffer big enough for the 128 KiB-per-worker budget to ask for several slices.
    const PPL: usize = 512;
    const LINES: usize = 400;
    let bpl = PPL * 3;
    let src: Vec<u8> = (0..bpl * LINES).map(|i| (i % 251) as u8).collect();

    let mut expected = vec![0u8; src.len()];
    srgb_transform(&ctx)
        .do_transform_line_stride(&src, &mut expected, PPL, LINES, bpl, bpl, bpl * LINES, bpl * LINES);
    assert_eq!(SCHED_HITS.load(Ordering::SeqCst), 0, "no scheduler yet");

    plugin::register_plugin(Plugin::Parallelization(ParallelizationConfig {
        max_workers: 4,
        worker_flags: 0,
        scheduler: counting_scheduler,
    }))
    .unwrap();

    let t = srgb_transform(&ctx);
    assert_eq!(t.max_workers(), 4, "MaxWorkers must reach the transform");

    // Building the transform runs a couple of internal 1-pixel conversions (black-point
    // detection), which are scheduled too — exactly as in C, where cmssamp's helper
    // transforms also go through ParalellizeIfSuitable. Count only the measured call.
    SCHED_HITS.store(0, Ordering::SeqCst);
    let mut threaded = vec![0u8; src.len()];
    t.do_transform_line_stride(&src, &mut threaded, PPL, LINES, bpl, bpl, bpl * LINES, bpl * LINES);

    assert_eq!(SCHED_HITS.load(Ordering::SeqCst), 1, "scheduler must be invoked");
    assert_eq!(threaded, expected, "threaded result must be byte-identical");

    // The job really was split: the 128 KiB budget over ~1.2 MB (in+out) wants 4+ slices,
    // capped at MaxWorkers.
    assert!(bpl * LINES * 2 / (128 * 1024) >= 4, "test buffer must exceed the split threshold");

    plugin::unregister_plugins();
    SCHED_HITS.store(0, Ordering::SeqCst);
    let mut plain = vec![0u8; src.len()];
    srgb_transform(&ctx)
        .do_transform_line_stride(&src, &mut plain, PPL, LINES, bpl, bpl, bpl * LINES, bpl * LINES);
    assert_eq!(SCHED_HITS.load(Ordering::SeqCst), 0, "single-threaded after unregister");
    assert_eq!(plain, expected);
}

/// Thread ids the plugin pixel loop was executed on.
static WORKER_THREADS: Mutex<Vec<std::thread::ThreadId>> = Mutex::new(Vec::new());

/// A pixel loop that records its thread and copies the bytes through untouched.
fn thread_recording_xform(
    _t: &Transform,
    src: &[u8],
    dst: &mut [u8],
    ppl: usize,
    lines: usize,
    stride: &Stride,
) {
    WORKER_THREADS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(std::thread::current().id());
    for line in 0..lines {
        let (s, d) = (line * stride.bytes_per_line_in, line * stride.bytes_per_line_out);
        dst[d..d + ppl * 3].copy_from_slice(&src[s..s + ppl * 3]);
    }
}

fn thread_recording_factory(
    _lut: &mut Pipeline,
    _in_fmt: &mut PixelFormat,
    _out_fmt: &mut PixelFormat,
    _flags: &mut u32,
) -> Option<TransformFit> {
    Some(TransformFit { xform: PluginXform::Full(thread_recording_xform), user_data: None })
}

#[test]
fn threaded_scheduler_runs_slices_on_several_threads() {
    let _g = guard();
    plugin::unregister_plugins();

    let ctx = Context::new();
    plugin::register_plugin(Plugin::Transform(thread_recording_factory)).unwrap();
    plugin::register_plugin(Plugin::Parallelization(ParallelizationConfig {
        max_workers: 4,
        worker_flags: 0,
        scheduler: plugin::threaded_scheduler,
    }))
    .unwrap();

    const PPL: usize = 512;
    const LINES: usize = 400;
    let bpl = PPL * 3;
    let src: Vec<u8> = (0..bpl * LINES).map(|i| (i % 251) as u8).collect();
    let mut dst = vec![0u8; src.len()];

    let t = srgb_transform(&ctx);
    WORKER_THREADS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    t.do_transform_line_stride(&src, &mut dst, PPL, LINES, bpl, bpl, bpl * LINES, bpl * LINES);

    assert_eq!(dst, src, "every slice must have been covered exactly once");

    let threads = WORKER_THREADS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let distinct: std::collections::HashSet<_> = threads.iter().collect();
    assert_eq!(threads.len(), 4, "MaxWorkers=4 must produce 4 slices");
    assert!(distinct.len() > 1, "slices must run on more than one thread, got {distinct:?}");

    plugin::unregister_plugins();
}

// --- Per-context isolation (_cmsContextGetClientChunk) ---------------------------------

/// A second custom parametric curve: type 201 == "add params[0] to the input".
fn offset_eval(_type: i32, params: &[f64; 10], r: f64) -> f64 {
    r + params[0]
}

/// A context owning a plugin chunk must not leak it into another context, and must not see
/// Context 0 either — the two halves of `_cmsContextGetClientChunk` (`cmsplugin.c:747-772`).
#[test]
fn plugin_registered_into_one_context_is_invisible_to_another() {
    let _g = guard();
    plugin::unregister_plugins();

    // A owns its own chunk (`cmsCreateContext`), holding curve type 200 and nothing else.
    let mut a = Context::with_plugin(
        Plugin::ParametricCurves(ParametricCurvePlugin {
            types: vec![200],
            counts: vec![1],
            evaluator: scale_eval,
        }),
        None,
    )
    .unwrap();

    // B owns no chunk, so it *is* Context 0 — which is empty.
    let b = Context::new();

    // A sees its curve; B and the process-global entry point do not.
    assert!(ToneCurve::build_parametric_thr(&a, 200, &[2.0]).is_ok(), "A must see its own plugin");
    assert!(ToneCurve::build_parametric_thr(&b, 200, &[2.0]).is_err(), "B must not see A's plugin");
    assert!(ToneCurve::build_parametric(200, &[2.0]).is_err(), "Context 0 must not see A's plugin");

    // The plugin is genuinely driving evaluation in A, not merely being accepted.
    let curve = ToneCurve::build_parametric_thr(&a, 200, &[2.0]).unwrap();
    assert!((curve.eval_f32(0.25) - 0.5).abs() < 1e-4, "got {}", curve.eval_f32(0.25));

    // `cmsPluginTHR` into a chunk-owning context stays private to it as well.
    a.register_plugin(Plugin::ParametricCurves(ParametricCurvePlugin {
        types: vec![201],
        counts: vec![1],
        evaluator: offset_eval,
    }))
    .unwrap();
    assert!(ToneCurve::build_parametric_thr(&a, 201, &[0.5]).is_ok(), "A must see its own THR plugin");
    assert!(ToneCurve::build_parametric_thr(&b, 201, &[0.5]).is_err(), "B must not see A's THR plugin");

    // Conversely, a Context-0 registration reaches B (which owns no chunk) but never A, whose
    // own chunk shadows Context 0 entirely.
    plugin::register_plugin(Plugin::ParametricCurves(ParametricCurvePlugin {
        types: vec![202],
        counts: vec![1],
        evaluator: scale_eval,
    }))
    .unwrap();
    assert!(ToneCurve::build_parametric_thr(&b, 202, &[2.0]).is_ok(), "B defers to Context 0");
    assert!(ToneCurve::build_parametric_thr(&a, 202, &[2.0]).is_err(), "A shadows Context 0");

    plugin::unregister_plugins();
}
