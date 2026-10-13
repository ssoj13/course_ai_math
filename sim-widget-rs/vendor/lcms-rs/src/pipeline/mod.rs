//! Pipelines (`cmsPipeline`) — a bit-exact port of the pipeline half of `cmslut.c`.
//!
//! A [`Pipeline`] is an ordered chain of [`Stage`]s evaluated with a two-slot ping-pong
//! buffer, exactly like lcms2's `_LUTevalFloat` / `_LUTeval16`. The C linked list of stages
//! becomes a `Vec<Stage>` and `cmsPipelineFree`/`cmsPipelineDup` collapse into `Drop`/`Clone`.
//!
//! # The optimization hook (`_cmsPipelineSetOptimizationParameters`)
//! lcms2 lets an optimizer swap in a fast whole-pipeline evaluator (`Eval16Fn` + `Data`) while
//! **keeping** the discrete, serializable stage chain. This port mirrors that with
//! [`Pipeline::fast_eval`] — an optional [`Eval16`] set by [`set_optimization_parameters`].
//! Any structural mutation ([`insert_stage`]/[`unlink_stage`]/[`cat`]) clears the hook, so a
//! stale fast path can never outlive the chain it summarized. Crucially the stage chain remains
//! intact and serializable (an `AToB`/`BToA` writer or [`crate::virt::find_combination`] still
//! sees real curve/matrix/CLUT stages), which is exactly what `cmsopt.c` preserves.
//!
//! ## Two evaluation lanes — the hook is 16-bit ONLY (do not "unify")
//! `_cmsPipelineSetOptimizationParameters` (`cmslut.c:1674`) assigns **only** `Eval16Fn`.
//! `EvalFloatFn` is deliberately left as `_LUTevalFloat` (`cmslut.c:1354`), and every optimizer
//! in `cmsopt.c` supplies a 16-bit-only evaluator (`Lerp16`, `PrelinEval8`/`16`,
//! `FastEvaluateCurves8`/`16`, `FastIdentity16`, `MatShaperEval16`). So:
//!
//! * [`eval_16`] (`cmsPipelineEval16`, `cmslut.c:1449`) runs the hook when installed.
//! * [`eval_float`] (`cmsPipelineEvalFloat`, `cmslut.c:1457`) **never** does — it always walks
//!   the discrete chain at full `f32` precision.
//!
//! Routing the float lane through the hook would quantize it to 16 bits (every fast evaluator
//! is a `u16 -> u16` table/fixed-point kernel) and would additionally clamp to `[0,1]`, so
//! out-of-range scene-linear values would be crushed. It also breaks
//! [`Pipeline::eval_reverse_float`], whose `JACOBIAN_EPSILON = 0.001` probe step falls inside a
//! single quantization bucket — the finite differences all come out zero, the Jacobian is
//! singular and the solve fails. The type system now enforces the split: the hook is an
//! [`Eval16`], which has no float entry point at all.
//!
//! [`set_optimization_parameters`]: Pipeline::set_optimization_parameters
//! [`eval_16`]: Pipeline::eval_16
//! [`eval_float`]: Pipeline::eval_float
//! [`insert_stage`]: Pipeline::insert_stage
//! [`unlink_stage`]: Pipeline::unlink_stage
//! [`cat`]: Pipeline::cat
//!
//! # Fidelity (do not "clean up")
//! * [`Pipeline::eval_16`] reproduces the exact `_LUTeval16` scaling when walking the chain:
//!   inputs go `u16 -> f32` via [`stage::from_16_to_float`], stages run in `f32`, and outputs go
//!   back `f32 -> u16` via [`stage::from_float_to_16`] (`_cmsQuickSaturateWord(x * 65535)`).
//!   On the fast path it hands the `u16` straight to the hook, exactly like C's
//!   `Eval16Fn(In, Out, Data)` — no float round-trip.
//! * [`Pipeline::eval_reverse_float`] is `cmsPipelineEvalReverseFloat`: 30-iteration
//!   Newton–Raphson with a finite-difference Jacobian (`JACOBIAN_EPSILON = 0.001`), solved
//!   through [`Mat3::solve`], with the same convergence/clip logic and 3→3 / 4→3 gating.

pub mod stage;

use std::sync::Arc;

use crate::math::interp::MAX_STAGE_CHANNELS;
use crate::math::mat3::{Mat3, Vec3};
use crate::types::sig::Signature;
use crate::MAX_CHANNELS;

pub use stage::{ClutData, ClutTable, Eval16, Stage, StageData, StageEval};

/// Where to insert a stage in a pipeline (`cmsStageLoc`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageLoc {
    /// `cmsAT_BEGIN` — prepend.
    Begin,
    /// `cmsAT_END` — append.
    End,
}

/// An ordered chain of [`Stage`]s (`cmsPipeline`).
#[derive(Clone, Debug)]
pub struct Pipeline {
    /// The stage chain (`Elements`).
    elements: Vec<Stage>,
    /// Cached input channel count (`InputChannels`); maintained by [`Pipeline::bless`].
    input_channels: u32,
    /// Cached output channel count (`OutputChannels`).
    output_channels: u32,
    /// Persist-as-8-bit hint for serialization (`SaveAs8Bits`).
    save_as_8bits: bool,
    /// Optional fast whole-pipeline evaluator installed by the optimizer (`Eval16Fn` + `Data`).
    /// `None` = walk the stage chain. Consumed by [`Pipeline::eval_16`] only — never by
    /// [`Pipeline::eval_float`]. See the module docs.
    fast_eval: Option<Arc<dyn Eval16 + Send + Sync>>,
}

impl Pipeline {
    /// `cmsPipelineAlloc` — an empty pipeline with the given channel counts.
    ///
    /// Zero channels are allowed as a placeholder; `>= cmsMAXCHANNELS` is rejected.
    pub fn alloc(input_channels: u32, output_channels: u32) -> crate::error::Result<Self> {
        if input_channels as usize >= MAX_CHANNELS || output_channels as usize >= MAX_CHANNELS {
            return Err(crate::cms_err!(
                crate::error::ErrorCode::Range,
                "too many channels ({input_channels} in / {output_channels} out, max={})",
                MAX_CHANNELS - 1
            ));
        }
        Ok(Pipeline {
            elements: Vec::new(),
            input_channels,
            output_channels,
            save_as_8bits: false,
            fast_eval: None,
        })
    }

    // -----------------------------------------------------------------------------------
    // Introspection
    // -----------------------------------------------------------------------------------

    /// `cmsPipelineInputChannels`.
    pub fn input_channels(&self) -> u32 {
        self.input_channels
    }

    /// `cmsPipelineOutputChannels`.
    pub fn output_channels(&self) -> u32 {
        self.output_channels
    }

    /// `cmsPipelineStageCount`.
    pub fn stage_count(&self) -> u32 {
        self.elements.len() as u32
    }

    /// The full stage chain.
    pub fn stages(&self) -> &[Stage] {
        &self.elements
    }

    /// `cmsPipelineGetPtrToFirstStage`.
    pub fn first_stage(&self) -> Option<&Stage> {
        self.elements.first()
    }

    /// `cmsPipelineGetPtrToLastStage`.
    pub fn last_stage(&self) -> Option<&Stage> {
        self.elements.last()
    }

    /// `cmsPipelineSetSaveAs8bitsFlag` — set the flag, returning its previous value.
    pub fn set_save_as_8bits(&mut self, on: bool) -> bool {
        let prev = self.save_as_8bits;
        self.save_as_8bits = on;
        prev
    }

    /// Current value of the save-as-8-bit hint.
    pub fn save_as_8bits(&self) -> bool {
        self.save_as_8bits
    }

    // -----------------------------------------------------------------------------------
    // Optimization hook (_cmsPipelineSetOptimizationParameters)
    // -----------------------------------------------------------------------------------

    /// `_cmsPipelineSetOptimizationParameters` — install `eval` as the fast whole-pipeline
    /// evaluator. [`eval_16`](Self::eval_16) / [`eval_float`](Self::eval_float) run it in place
    /// of walking the stage chain, which is kept intact for serialization. The caller is
    /// responsible for the fast path being functionally equivalent to the retained chain.
    pub fn set_optimization_parameters(&mut self, eval: Arc<dyn Eval16 + Send + Sync>) {
        self.fast_eval = Some(eval);
    }

    /// Drop any installed fast evaluator, reverting to direct stage-chain evaluation. Called
    /// automatically whenever the stage chain is structurally mutated.
    pub fn clear_optimization_parameters(&mut self) {
        self.fast_eval = None;
    }

    /// Whether a fast evaluator is currently installed (`Lut->Data != Lut`).
    /// `cmsPipelineCheckAndRetreiveStages` (`cmslut.c:110`) — succeed iff this pipeline is
    /// exactly `types.len()` stages long and their type signatures match `types` in order,
    /// returning references to them.
    pub fn check_and_retrieve_stages(&self, types: &[Signature]) -> Option<Vec<&Stage>> {
        stage::check_and_retrieve(&self.elements, types)
    }

    pub fn is_optimized(&self) -> bool {
        self.fast_eval.is_some()
    }

    // -----------------------------------------------------------------------------------
    // Chain maintenance (BlessLUT)
    // -----------------------------------------------------------------------------------

    /// `BlessLUT` — recompute the cached channel counts from the chain and validate that
    /// each stage's input matches the previous stage's output. Empty chains succeed
    /// unchanged; a channel-count mismatch returns `false`.
    fn bless(&mut self) -> bool {
        if !self.elements.is_empty() {
            self.input_channels = self.elements[0].input_channels;
            self.output_channels = self.elements[self.elements.len() - 1].output_channels;

            for pair in self.elements.windows(2) {
                if pair[1].input_channels != pair[0].output_channels {
                    return false;
                }
            }
        }
        true
    }

    /// `cmsPipelineInsertStage` — insert `mpe` at `loc`, then re-bless.
    ///
    /// Returns the `BlessLUT` result. As in C, a chain that fails validation still keeps the
    /// inserted stage (no rollback).
    pub fn insert_stage(&mut self, loc: StageLoc, mpe: Stage) -> bool {
        // The chain is changing — any installed fast evaluator no longer describes it.
        self.fast_eval = None;
        match loc {
            StageLoc::Begin => self.elements.insert(0, mpe),
            StageLoc::End => self.elements.push(mpe),
        }
        self.bless()
    }

    /// `cmsPipelineUnlinkStage` — remove and return the first/last stage.
    ///
    /// Returns `None` for an empty pipeline. The re-bless afterwards may fail (mirroring the
    /// C code, which ignores that result).
    pub fn unlink_stage(&mut self, loc: StageLoc) -> Option<Stage> {
        if self.elements.is_empty() {
            return None;
        }
        // The chain is changing — drop any installed fast evaluator.
        self.fast_eval = None;
        let mpe = match loc {
            StageLoc::Begin => Some(self.elements.remove(0)),
            StageLoc::End => self.elements.pop(),
        };
        self.bless();
        mpe
    }

    /// `cmsPipelineCat` — append clones of every stage of `other` onto `self`.
    ///
    /// If both pipelines are empty the channel counts are inherited from `other`.
    pub fn cat(&mut self, other: &Pipeline) -> bool {
        if self.elements.is_empty() && other.elements.is_empty() {
            self.input_channels = other.input_channels;
            self.output_channels = other.output_channels;
        }
        for mpe in &other.elements {
            if !self.insert_stage(StageLoc::End, mpe.clone()) {
                return false;
            }
        }
        self.bless()
    }

    // -----------------------------------------------------------------------------------
    // Evaluation
    // -----------------------------------------------------------------------------------

    /// `cmsPipelineEvalFloat` (`_LUTevalFloat`, `cmslut.c:1354`) — evaluate the chain in `f32`.
    ///
    /// Always walks the discrete stage chain, **even when a fast evaluator is installed** —
    /// `_cmsPipelineSetOptimizationParameters` only ever touches `Eval16Fn`, never
    /// `EvalFloatFn`. See the "Two evaluation lanes" section of the module docs before changing
    /// this: the optimizer hook is 16-bit and would quantize and clamp this lane.
    pub fn eval_float(&self, input: &[f32], output: &mut [f32]) {
        let mut storage = [[0f32; MAX_STAGE_CHANNELS]; 2];
        let ni = self.input_channels as usize;
        storage[0][..ni].copy_from_slice(&input[..ni]);

        let mut phase = 0usize;
        for mpe in &self.elements {
            let next = phase ^ 1;
            let (lo, hi) = storage.split_at_mut(1);
            let (src, dst) = if phase == 0 { (&lo[0], &mut hi[0]) } else { (&hi[0], &mut lo[0]) };
            mpe.eval(src, dst);
            phase = next;
        }

        let no = self.output_channels as usize;
        output[..no].copy_from_slice(&storage[phase][..no]);
    }

    /// `cmsPipelineEval16` (`_LUTeval16`, `cmslut.c:1329`) — evaluate at 16-bit precision.
    ///
    /// Runs the installed fast evaluator when one is present, handing it the `u16` directly —
    /// the exact shape of C's `Eval16Fn(In, Out, Data)` (`cmslut.c:1449`). Otherwise walks the
    /// stage chain, which runs in `f32` internally with `u16` boundary conversions.
    pub fn eval_16(&self, input: &[u16], output: &mut [u16]) {
        if let Some(ev) = &self.fast_eval {
            ev.eval16(input, output);
            return;
        }
        let mut storage = [[0f32; MAX_STAGE_CHANNELS]; 2];
        let ni = self.input_channels as usize;
        stage::from_16_to_float(&input[..ni], &mut storage[0][..ni]);

        let mut phase = 0usize;
        for mpe in &self.elements {
            let next = phase ^ 1;
            let (lo, hi) = storage.split_at_mut(1);
            let (src, dst) = if phase == 0 { (&lo[0], &mut hi[0]) } else { (&hi[0], &mut lo[0]) };
            mpe.eval(src, dst);
            phase = next;
        }

        let no = self.output_channels as usize;
        stage::from_float_to_16(&storage[phase][..no], &mut output[..no]);
    }

    /// `cmsPipelineEvalReverseFloat` — invert a 3→3 (or 4→3) chain by Newton's method.
    ///
    /// `target` holds the desired output (for the 4→3 case `target[3]` is the fixed input
    /// channel). `hint`, if present, seeds the search (first 3 channels). On success `result`
    /// receives the recovered input (`input_channels` values) and `true` is returned; `false`
    /// means an unsupported shape or a singular Jacobian.
    pub fn eval_reverse_float(
        &self,
        target: &[f32],
        result: &mut [f32],
        hint: Option<&[f32]>,
    ) -> bool {
        const JACOBIAN_EPSILON: f32 = 0.001;
        const INVERSION_MAX_ITERATIONS: usize = 30;

        // Only 3->3 and 4->3 are supported.
        if self.input_channels != 3 && self.input_channels != 4 {
            return false;
        }
        if self.output_channels != 3 {
            return false;
        }

        let mut x = [0f32; 4];
        match hint {
            // Begin at any point; we choose 1/3 of the CMY axis.
            None => {
                x[0] = 0.3;
                x[1] = 0.3;
                x[2] = 0.3;
            }
            Some(h) => {
                x[..3].copy_from_slice(&h[..3]);
            }
        }
        // If the LUT is 4-dimensional, grab the fixed target[3].
        x[3] = if self.input_channels == 4 { target[3] } else { 0.0 };

        let mut last_error = 1e20f64;
        let mut fx = [0f32; 4];

        for _ in 0..INVERSION_MAX_ITERATIONS {
            // Get beginning fx.
            self.eval_float(&x, &mut fx);

            // Compute error.
            let error = euclidean_distance(&fx, target, 3);

            // If not convergent, return the last safe value.
            if error >= last_error {
                break;
            }

            // Keep the latest values.
            last_error = error;
            let nin = self.input_channels as usize;
            result[..nin].copy_from_slice(&x[..nin]);

            // Found an exact match?
            if error <= 0.0 {
                break;
            }

            // Obtain the slope (the Jacobian).
            let mut jacobian = Mat3::default();
            for j in 0..3 {
                let mut xd = x;
                inc_delta(&mut xd[j], JACOBIAN_EPSILON);

                let mut fxd = [0f32; 4];
                self.eval_float(&xd, &mut fxd);

                jacobian.0[0].0[j] = ((fxd[0] - fx[0]) / JACOBIAN_EPSILON) as f64;
                jacobian.0[1].0[j] = ((fxd[1] - fx[1]) / JACOBIAN_EPSILON) as f64;
                jacobian.0[2].0[j] = ((fxd[2] - fx[2]) / JACOBIAN_EPSILON) as f64;
            }

            // Solve the system J·Δ = fx - target.
            let rhs = Vec3::new(
                (fx[0] - target[0]) as f64,
                (fx[1] - target[1]) as f64,
                (fx[2] - target[2]) as f64,
            );
            let delta = match Mat3::solve(&jacobian, rhs) {
                Some(v) => v,
                None => return false,
            };

            // Move our guess.
            x[0] -= delta.0[0] as f32;
            x[1] -= delta.0[1] as f32;
            x[2] -= delta.0[2] as f32;

            // Some clipping (clamp is bit-identical to C's if/else-if, NaN included).
            for xj in x.iter_mut().take(3) {
                *xj = xj.clamp(0.0, 1.0);
            }
        }

        true
    }
}

/// `IncDelta` — nudge a coordinate by `epsilon`, reflecting off the upper boundary.
///
/// The `< 1.0 - epsilon` comparison is evaluated in `f64` (C promotes the `float` epsilon),
/// then the increment/decrement is applied in `f32`.
fn inc_delta(val: &mut f32, epsilon: f32) {
    if (*val as f64) < 1.0 - epsilon as f64 {
        *val += epsilon;
    } else {
        *val -= epsilon;
    }
}

/// `EuclideanDistance` — `sqrtf(Σ (b[i]-a[i])²)` over `n` channels, accumulated in `f32`.
fn euclidean_distance(a: &[f32], b: &[f32], n: usize) -> f64 {
    let mut sum = 0f32;
    for i in 0..n {
        let dif = b[i] - a[i];
        sum += dif * dif;
    }
    sum.sqrt() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_pipeline_returns_input() {
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        assert!(lut.insert_stage(StageLoc::End, Stage::alloc_identity(3)));
        assert_eq!(lut.stage_count(), 1);
        assert_eq!(lut.input_channels(), 3);
        assert_eq!(lut.output_channels(), 3);

        let mut out = [0f32; 3];
        lut.eval_float(&[0.1, 0.5, 0.9], &mut out);
        assert_eq!(out, [0.1, 0.5, 0.9]);
    }

    #[test]
    fn empty_pipeline_alloc_channels() {
        let lut = Pipeline::alloc(4, 3).unwrap();
        assert_eq!(lut.input_channels(), 4);
        assert_eq!(lut.output_channels(), 3);
        assert_eq!(lut.stage_count(), 0);
        assert!(Pipeline::alloc(16, 3).is_err());
    }

    #[test]
    fn matrix_pipeline_eval() {
        // out = diag(0.5,0.5,0.5)·in
        let m = [0.5, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5];
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        lut.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m, None).unwrap());
        let mut out = [0f32; 3];
        lut.eval_float(&[0.4, 0.6, 1.0], &mut out);
        assert!((out[0] - 0.2).abs() < 1e-6, "{out:?}");
        assert!((out[1] - 0.3).abs() < 1e-6, "{out:?}");
        assert!((out[2] - 0.5).abs() < 1e-6, "{out:?}");
    }

    #[test]
    fn eval16_and_float_agree_on_matrix() {
        // A benign matrix keeps values in-range so the 16-bit round-trip is faithful.
        let m = [0.5, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5];
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        lut.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m, None).unwrap());

        let in16 = [0x4000u16, 0x8000, 0xC000];
        let mut out16 = [0u16; 3];
        lut.eval_16(&in16, &mut out16);

        let inf: Vec<f32> = in16.iter().map(|&v| v as f32 / 65535.0).collect();
        let mut outf = [0f32; 3];
        lut.eval_float(&inf, &mut outf);

        for k in 0..3 {
            let from16 = out16[k] as f32 / 65535.0;
            assert!((from16 - outf[k]).abs() < 1e-4, "chan {k}: {from16} vs {}", outf[k]);
        }
    }

    #[test]
    fn multi_stage_chain_and_bless() {
        // Lab -> XYZ -> Lab pipeline: 3->3 throughout; bless must accept it.
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        assert!(lut.insert_stage(StageLoc::End, Stage::alloc_lab2xyz()));
        assert!(lut.insert_stage(StageLoc::End, Stage::alloc_xyz2lab()));
        assert_eq!(lut.stage_count(), 2);

        let inp = [0.5f32, (128.0 / 255.0) as f32, (128.0 / 255.0) as f32];
        let mut out = [0f32; 3];
        lut.eval_float(&inp, &mut out);
        for k in 0..3 {
            assert!((out[k] - inp[k]).abs() < 1e-4, "{out:?} vs {inp:?}");
        }
    }

    #[test]
    fn bless_rejects_channel_mismatch() {
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        // 3->3 identity, then a 4->4 identity: chain is inconsistent.
        assert!(lut.insert_stage(StageLoc::End, Stage::alloc_identity(3)));
        assert!(!lut.insert_stage(StageLoc::End, Stage::alloc_identity(4)));
    }

    #[test]
    fn unlink_and_cat() {
        let mut a = Pipeline::alloc(3, 3).unwrap();
        a.insert_stage(StageLoc::End, Stage::alloc_identity(3));
        a.insert_stage(StageLoc::End, Stage::clip_negatives(3));
        assert_eq!(a.stage_count(), 2);

        let removed = a.unlink_stage(StageLoc::End).unwrap();
        assert_eq!(removed.type_signature(), stage::STAGE_CLIP_NEGATIVES);
        assert_eq!(a.stage_count(), 1);

        let mut b = Pipeline::alloc(3, 3).unwrap();
        b.insert_stage(StageLoc::End, Stage::clip_negatives(3));
        assert!(a.cat(&b));
        assert_eq!(a.stage_count(), 2);
    }

    #[test]
    fn reverse_eval_inverts_matrix_pipeline() {
        // Forward: out = diag(0.5)·in. Reverse of target=[0.2,0.25,0.3] -> in=[0.4,0.5,0.6].
        let m = [0.5, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5];
        let mut lut = Pipeline::alloc(3, 3).unwrap();
        lut.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m, None).unwrap());

        let target = [0.2f32, 0.25, 0.3];
        let mut result = [0f32; 3];
        assert!(lut.eval_reverse_float(&target, &mut result, None));
        assert!((result[0] - 0.4).abs() < 1e-3, "{result:?}");
        assert!((result[1] - 0.5).abs() < 1e-3, "{result:?}");
        assert!((result[2] - 0.6).abs() < 1e-3, "{result:?}");
    }

    #[test]
    fn reverse_eval_rejects_wrong_shape() {
        let mut lut = Pipeline::alloc(2, 3).unwrap();
        lut.insert_stage(
            StageLoc::End,
            Stage::alloc_matrix(3, 2, &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0], None).unwrap(),
        );
        let mut result = [0f32; 3];
        assert!(!lut.eval_reverse_float(&[0.1, 0.2, 0.3], &mut result, None));
    }
}
