//! N-dimensional CLUT interpolation — a bit-exact port of `cmsintrp.c`.
//!
//! Covers 1..=15 input channels and up to [`MAX_STAGE_CHANNELS`] outputs, in both a fast
//! fixed-point (`u16`) path and a full-precision `f32` path — exactly as lcms2 does.
//!
//! # Bit-exactness (do not "clean up")
//! The 16-bit tetrahedral / N-D kernels use the Sakamoto fixed-point trick, which lcms2
//! marks `CMS_NO_SANITIZE` because it **relies on signed-integer wraparound** (`c*r` can
//! exceed `i32`). We reproduce that with `wrapping_*` on `i32`/`u32` so results match the C
//! bit for bit. Refactoring the arithmetic — widening to `i64`, reordering — would silently
//! change output colours. The rounding shortcut `t = Rest + 0x8001; x = (t + (t>>16)) >> 16`
//! is kept verbatim (it is off-by-one at `0x7fff`/`0x17ffe` by design, matching C).
//!
//! # Structure vs C
//! lcms2 stores a function pointer (`cmsInterpFunction`) plus the table in `cmsInterpParams`.
//! Here [`InterpParams`] carries the **geometry** (dimensions, domain, strides) and stays
//! `Copy`; the table is passed explicitly to [`InterpParams::eval16`] /
//! [`InterpParams::eval_float`], which dispatch on channel count + flags (the body of
//! `DefaultInterpolatorsFactory`). This drops the `void*` erasure and needs no `unsafe`.
//!
//! The `cmsInterpFunction` indirection itself **is** kept, as plain `fn` pointers in
//! [`InterpParams::interp16`] / [`InterpParams::interp_float`] (plain fns so the struct
//! remains `Copy`). [`InterpParams::new_ex`] asks the plugin registry for them exactly as
//! `_cmsSetInterpolationRoutine` invokes `ptr->Interpolators`, and falls back to the built-in
//! dispatch when no plugin claims the geometry — the role of `DefaultInterpolatorsFactory`.

use crate::context::Context;
use crate::error::{Error, ErrorCode};
use crate::types::num::{quick_floor, to_fixed_domain};

/// Maximum number of input channels / interpolation dimensions (`MAX_INPUT_DIMENSIONS`).
pub const MAX_INPUT_DIMENSIONS: usize = 15;
/// Maximum output channels of a stage (`MAX_STAGE_CHANNELS`); bounds the scratch buffers.
pub const MAX_STAGE_CHANNELS: usize = 128;

/// `CMS_LERP_FLAGS_FLOAT` — select the floating-point interpolators.
pub const CMS_LERP_FLAGS_FLOAT: u32 = 0x0001;
/// `CMS_LERP_FLAGS_TRILINEAR` — hint to use trilinear instead of tetrahedral for 3 inputs.
pub const CMS_LERP_FLAGS_TRILINEAR: u32 = 0x0100;

/// Precomputed interpolation geometry (`cmsInterpParams`, minus the table + fn pointer).
#[derive(Debug, Clone, Copy)]
pub struct InterpParams {
    /// Interpolation flags (`CMS_LERP_FLAGS_*`).
    pub flags: u32,
    /// Number of input channels / dimensions.
    pub n_inputs: usize,
    /// Number of output channels.
    pub n_outputs: usize,
    /// Nodes per input dimension.
    pub n_samples: [u32; MAX_INPUT_DIMENSIONS],
    /// `Domain[i] = nSamples[i] - 1`.
    pub domain: [u32; MAX_INPUT_DIMENSIONS],
    /// Grid strides: `opta[0] = nOutputs`, `opta[i] = opta[i-1] * nSamples[nInputs-i]`.
    pub opta: [u32; MAX_INPUT_DIMENSIONS],
    /// Plugin override for the 16-bit kernel (`Interpolation.Lerp16`), or `None` for built-in.
    pub interp16: Option<crate::plugin::Interp16Fn>,
    /// Plugin override for the float kernel (`Interpolation.LerpFloat`), or `None` for built-in.
    pub interp_float: Option<crate::plugin::InterpFloatFn>,
}

/// Whether `DefaultInterpolatorsFactory` yields a non-null kernel for this channel pair.
///
/// Mirrors `cmsintrp.c:1179-1330`: the leading safety check rejects 4+ inputs with too many
/// outputs (`:1189`), and the `switch (nInputChannels)` enumerates cases 1..=15, falling to
/// `default: Interpolation.Lerp16 = NULL` (`:1325-1326`) for anything else — notably 0.
///
/// C consults the plugin factory *first* and only falls back to this one (`cmsintrp.c:92-98`),
/// so both checks apply solely when no plugin supplied a kernel.
fn default_factory_supports(n_inputs: usize, n_outputs: usize) -> bool {
    if n_inputs >= 4 && n_outputs >= MAX_STAGE_CHANNELS {
        return false;
    }
    (1..=MAX_INPUT_DIMENSIONS).contains(&n_inputs)
}

impl InterpParams {
    /// Build geometry with per-dimension sample counts (`_cmsComputeInterpParamsEx`).
    pub fn new_ex(
        n_samples: &[u32],
        n_inputs: usize,
        n_outputs: usize,
        flags: u32,
    ) -> crate::error::Result<Self> {
        Self::new_ex_for(None, n_samples, n_inputs, n_outputs, flags)
    }

    /// [`Self::new_ex`] resolving the interpolation kernel against `ctx`.
    ///
    /// This is where the context matters and the *only* place it does: C's
    /// `_cmsComputeInterpParamsEx(ContextID, ...)` hands the id to
    /// `_cmsSetInterpolationRoutine(ContextID, p)` (`cmsintrp.c:149`), which resolves the kernel
    /// once and stores the resulting function pointer in `p->Interpolation` (`cmsintrp.c:93`,
    /// `:98`). Evaluation never consults the registry again -- which is why [`InterpParams`]
    /// needs no lifetime and stays `Copy`.
    pub fn new_ex_thr(
        ctx: &Context,
        n_samples: &[u32],
        n_inputs: usize,
        n_outputs: usize,
        flags: u32,
    ) -> crate::error::Result<Self> {
        Self::new_ex_for(Some(ctx), n_samples, n_inputs, n_outputs, flags)
    }

    /// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
    ///
    /// `pub(crate)` because callers that already hold an `Option<&Context>` (the tone-curve
    /// allocator, which is reached from both the plain and `*THR` builders) would otherwise
    /// have to branch on it just to pick between the two public entry points.
    pub(crate) fn new_ex_for(
        ctx: Option<&Context>,
        n_samples: &[u32],
        n_inputs: usize,
        n_outputs: usize,
        flags: u32,
    ) -> crate::error::Result<Self> {
        if n_inputs > MAX_INPUT_DIMENSIONS {
            return Err(Error::new(
                ErrorCode::Range,
                format!("too many input channels ({n_inputs}, max={MAX_INPUT_DIMENSIONS})"),
            ));
        }
        // C indexes `nSamples[0..InputChan)` unconditionally (`cmsintrp.c:137-141`, `:145-146`);
        // a caller passing a short slice would silently leave dimensions at zero here and then
        // panic on the `opta` back-index below, so require the slice to cover every dimension.
        if n_samples.len() < n_inputs {
            return Err(Error::new(
                ErrorCode::Range,
                format!("n_samples too short: {} < {n_inputs}", n_samples.len()),
            ));
        }
        // Stricter than C by design: `Domain[i] = nSamples[i] - 1` (`cmsintrp.c:140`) wraps to
        // `UINT_MAX` on a zero-node axis, which C then happily indexes with — undefined
        // behaviour. Rejecting keeps the "never panic at eval" invariant without inheriting it.
        if let Some(i) = n_samples[..n_inputs].iter().position(|&s| s == 0) {
            return Err(Error::new(
                ErrorCode::Range,
                format!("CLUT dimension {i} has zero nodes"),
            ));
        }

        let mut p = InterpParams {
            flags,
            n_inputs,
            n_outputs,
            n_samples: [0; MAX_INPUT_DIMENSIONS],
            domain: [0; MAX_INPUT_DIMENSIONS],
            opta: [0; MAX_INPUT_DIMENSIONS],
            interp16: None,
            interp_float: None,
        };

        // `_cmsSetInterpolationRoutine`: give a registered factory first refusal; if it
        // declines, the `None`s above leave `eval16`/`eval_float` on the built-in dispatch
        // (C's `DefaultInterpolatorsFactory`).
        match crate::plugin::find_interpolation_factory(ctx, n_inputs, n_outputs, flags) {
            Some(crate::plugin::InterpFunction::Lerp16(f)) => p.interp16 = Some(f),
            Some(crate::plugin::InterpFunction::LerpFloat(f)) => p.interp_float = Some(f),
            None => {}
        }

        // `_cmsSetInterpolationRoutine` returns FALSE when neither the plugin nor the default
        // factory supplied a kernel, and `_cmsComputeInterpParamsEx` then frees the params and
        // returns NULL (`cmsintrp.c:97-103`, `:149-153`). Reproducing that here is what keeps
        // the failure at construction: without it `n_inputs == 0` reaches the `_ =>` arm of the
        // eval dispatch, where `n_inputs - 1` underflows.
        // Key on the 16-bit kernel ALONE, never on the float one. C checks a single member of
        // the `cmsInterpFunction` union and says so outright - "Check for valid interpolator (we
        // just check one member of the union)" (`cmsintrp.c:99-103`) - testing `Lerp16` both
        // after the plugin factory and after the default one, whatever `dwFlags` says. So a
        // plugin that fills only the float kernel does NOT satisfy C: the default factory still
        // runs, and an unsupported channel count still fails construction.
        let has_plugin_kernel = p.interp16.is_some();
        if !has_plugin_kernel && !default_factory_supports(n_inputs, n_outputs) {
            return Err(Error::new(
                ErrorCode::Range,
                format!("unsupported interpolation ({n_inputs}->{n_outputs} channels)"),
            ));
        }

        for (i, &s) in n_samples.iter().enumerate().take(n_inputs) {
            p.n_samples[i] = s;
            p.domain[i] = s - 1;
        }
        // C computes `opta` in `cmsUInt32Number`, where overflow wraps by definition
        // (`cmsintrp.c:144-146`) and yields a stride that indexes outside the table. Reject
        // instead — a wrapped stride is never a usable interpolation geometry.
        p.opta[0] = n_outputs as u32;
        for i in 1..n_inputs {
            p.opta[i] = p.opta[i - 1]
                .checked_mul(n_samples[n_inputs - i])
                .ok_or_else(|| Error::new(ErrorCode::Range, "CLUT index stride overflow"))?;
        }
        Ok(p)
    }

    /// Build geometry with a uniform node count per dimension (`_cmsComputeInterpParams`).
    pub fn new(
        n_samples: u32,
        n_inputs: usize,
        n_outputs: usize,
        flags: u32,
    ) -> crate::error::Result<Self> {
        let samples = [n_samples; MAX_INPUT_DIMENSIONS];
        Self::new_ex(&samples, n_inputs, n_outputs, flags)
    }

    /// [`Self::new`] resolving the interpolation kernel against `ctx`
    /// (`_cmsComputeInterpParams`, which is `_cmsComputeInterpParamsEx` with a uniform grid).
    pub fn new_thr(
        ctx: &Context,
        n_samples: u32,
        n_inputs: usize,
        n_outputs: usize,
        flags: u32,
    ) -> crate::error::Result<Self> {
        Self::new_for(Some(ctx), n_samples, n_inputs, n_outputs, flags)
    }

    /// Shared body of [`Self::new`] and [`Self::new_thr`] — see [`Self::new_ex_for`] for why
    /// this is `pub(crate)`.
    pub(crate) fn new_for(
        ctx: Option<&Context>,
        n_samples: u32,
        n_inputs: usize,
        n_outputs: usize,
        flags: u32,
    ) -> crate::error::Result<Self> {
        let samples = [n_samples; MAX_INPUT_DIMENSIONS];
        Self::new_ex_for(ctx, &samples, n_inputs, n_outputs, flags)
    }

    /// 16-bit forward interpolation (`Interpolation.Lerp16`).
    ///
    /// Goes through the plugin kernel installed by [`Self::new_ex`] when there is one;
    /// otherwise dispatches to the same kernel the C factory would pick for this channel count
    /// and the `TRILINEAR` flag. `table` is the CLUT; `input`/`output` are channel vectors.
    pub fn eval16(&self, table: &[u16], input: &[u16], output: &mut [u16]) {
        if let Some(f) = self.interp16 {
            f(self, table, input, output);
            return;
        }
        match self.n_inputs {
            1 => {
                if self.n_outputs == 1 {
                    lin_lerp_1d(table, input, output, self);
                } else {
                    eval_1_input(table, input, output, self);
                }
            }
            2 => bilinear_16(table, input, output, self),
            3 => {
                if self.flags & CMS_LERP_FLAGS_TRILINEAR != 0 {
                    trilinear_16(table, input, output, self);
                } else {
                    tetrahedral_16(table, input, output, self);
                }
            }
            _ => eval_nd_16(table, input, output, self),
        }
    }

    /// Floating-point forward interpolation (`Interpolation.LerpFloat`).
    ///
    /// Prefers the plugin kernel, as [`Self::eval16`] does.
    pub fn eval_float(&self, table: &[f32], input: &[f32], output: &mut [f32]) {
        if let Some(f) = self.interp_float {
            f(self, table, input, output);
            return;
        }
        match self.n_inputs {
            1 => {
                if self.n_outputs == 1 {
                    lin_lerp_1d_float(table, input, output, self);
                } else {
                    eval_1_input_float(table, input, output, self);
                }
            }
            2 => bilinear_float(table, input, output, self),
            3 => {
                if self.flags & CMS_LERP_FLAGS_TRILINEAR != 0 {
                    trilinear_float(table, input, output, self);
                } else {
                    tetrahedral_float(table, input, output, self);
                }
            }
            _ => eval_nd_float(table, input, output, self),
        }
    }
}

// ---------------------------------------------------------------------------------------
// Fixed-point primitives
// ---------------------------------------------------------------------------------------

/// `LinearInterp` — inline fixed-point lerp. `a` is the 0..0xffff rest; `l`/`h` are nodes.
///
/// Relies on `cmsUInt32Number` wraparound when `h < l` (`(u32)(h - l)`), so we cast the
/// signed difference to `u32` and use `wrapping_*` to match exactly.
#[inline]
fn linear_interp(a: i32, l: u16, h: u16) -> u16 {
    let dif = ((h as i32 - l as i32) as u32)
        .wrapping_mul(a as u32)
        .wrapping_add(0x8000);
    let dif = (dif >> 16).wrapping_add(l as u32);
    dif as u16
}

/// `fclamp` — clamp a float sample to `[0, 1]`, treating tiny/NaN as 0.
#[inline]
fn fclamp(v: f32) -> f32 {
    if v < 1.0e-9 || v.is_nan() {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

// ---------------------------------------------------------------------------------------
// 1 input
// ---------------------------------------------------------------------------------------

/// `LinLerp1D` — 1→1 fixed-point.
fn lin_lerp_1d(table: &[u16], value: &[u16], output: &mut [u16], p: &InterpParams) {
    let dom0 = p.domain[0];
    if value[0] == 0xffff || dom0 == 0 {
        output[0] = table[dom0 as usize];
        return;
    }
    let val3 = to_fixed_domain(dom0 as i32 * value[0] as i32);
    let cell0 = (val3 >> 16) as usize;
    let rest = val3 & 0xFFFF;
    let y0 = table[cell0];
    let y1 = table[cell0 + 1];
    output[0] = linear_interp(rest, y0, y1);
}

/// `LinLerp1Dfloat` — 1→1 float.
fn lin_lerp_1d_float(table: &[f32], value: &[f32], output: &mut [f32], p: &InterpParams) {
    let val2 = fclamp(value[0]);
    if val2 == 1.0 || p.domain[0] == 0 {
        output[0] = table[p.domain[0] as usize];
        return;
    }
    let val2 = val2 * p.domain[0] as f32;
    let cell0 = val2.floor() as i32;
    let cell1 = val2.ceil() as i32;
    let rest = val2 - cell0 as f32;
    let y0 = table[cell0 as usize];
    let y1 = table[cell1 as usize];
    output[0] = y0 + (y1 - y0) * rest;
}

/// `Eval1Input` — 1→N fixed-point.
fn eval_1_input(table: &[u16], input: &[u16], output: &mut [u16], p16: &InterpParams) {
    let dom0 = p16.domain[0];
    if input[0] == 0xffff || dom0 == 0 {
        let y0 = (dom0 * p16.opta[0]) as usize;
        output[..p16.n_outputs].copy_from_slice(&table[y0..y0 + p16.n_outputs]);
        return;
    }
    let v = input[0] as i32 * dom0 as i32;
    let fk = to_fixed_domain(v);
    let k0 = fk >> 16;
    let rk = fk & 0xFFFF;
    let k1 = k0 + if input[0] != 0xFFFF { 1 } else { 0 };
    let kk0 = (p16.opta[0] as i32 * k0) as usize;
    let kk1 = (p16.opta[0] as i32 * k1) as usize;
    for out_chan in 0..p16.n_outputs {
        output[out_chan] = linear_interp(rk, table[kk0 + out_chan], table[kk1 + out_chan]);
    }
}

/// `Eval1InputFloat` — 1→N float.
fn eval_1_input_float(table: &[f32], value: &[f32], output: &mut [f32], p: &InterpParams) {
    let val2 = fclamp(value[0]);
    if val2 == 1.0 || p.domain[0] == 0 {
        let start = (p.domain[0] * p.opta[0]) as usize;
        output[..p.n_outputs].copy_from_slice(&table[start..start + p.n_outputs]);
        return;
    }
    let val2 = val2 * p.domain[0] as f32;
    let cell0 = val2.floor() as i32;
    let cell1 = val2.ceil() as i32;
    let rest = val2 - cell0 as f32;
    let cell0 = (cell0 * p.opta[0] as i32) as usize;
    let cell1 = (cell1 * p.opta[0] as i32) as usize;
    for out_chan in 0..p.n_outputs {
        let y0 = table[cell0 + out_chan];
        let y1 = table[cell1 + out_chan];
        output[out_chan] = y0 + (y1 - y0) * rest;
    }
}

// ---------------------------------------------------------------------------------------
// 2 inputs (bilinear)
// ---------------------------------------------------------------------------------------

fn bilinear_float(table: &[f32], input: &[f32], output: &mut [f32], p: &InterpParams) {
    let total_out = p.n_outputs;
    let px = fclamp(input[0]) * p.domain[0] as f32;
    let py = fclamp(input[1]) * p.domain[1] as f32;

    let x0 = quick_floor(px as f64);
    let fx = px - x0 as f32;
    let y0 = quick_floor(py as f64);
    let fy = py - y0 as f32;

    let xx0 = (p.opta[1] as i32 * x0) as usize;
    let xx1 = xx0 + if fclamp(input[0]) >= 1.0 { 0 } else { p.opta[1] as usize };
    let yy0 = (p.opta[0] as i32 * y0) as usize;
    let yy1 = yy0 + if fclamp(input[1]) >= 1.0 { 0 } else { p.opta[0] as usize };

    let lerp = |a: f32, l: f32, h: f32| l + (h - l) * a;
    for out_chan in 0..total_out {
        let d00 = table[xx0 + yy0 + out_chan];
        let d01 = table[xx0 + yy1 + out_chan];
        let d10 = table[xx1 + yy0 + out_chan];
        let d11 = table[xx1 + yy1 + out_chan];
        let dx0 = lerp(fx, d00, d10);
        let dx1 = lerp(fx, d01, d11);
        output[out_chan] = lerp(fy, dx0, dx1);
    }
}

fn bilinear_16(table: &[u16], input: &[u16], output: &mut [u16], p: &InterpParams) {
    let total_out = p.n_outputs;
    let fx = to_fixed_domain(input[0] as i32 * p.domain[0] as i32);
    let x0 = (fx >> 16) as usize;
    let rx = fx & 0xFFFF;
    let fy = to_fixed_domain(input[1] as i32 * p.domain[1] as i32);
    let y0 = (fy >> 16) as usize;
    let ry = fy & 0xFFFF;

    let xx0 = p.opta[1] as usize * x0;
    let xx1 = xx0 + if input[0] == 0xFFFF { 0 } else { p.opta[1] as usize };
    let yy0 = p.opta[0] as usize * y0;
    let yy1 = yy0 + if input[1] == 0xFFFF { 0 } else { p.opta[0] as usize };

    // LERP(a,l,h) = (cmsUInt16Number)(l + ROUND_FIXED_TO_INT((h-l)*a));
    // ROUND_FIXED_TO_INT(x) = (x+0x8000)>>16.
    //
    // The `(cmsUInt16Number)` cast is INSIDE the C macro (cmsintrp.c:417), so every
    // intermediate is truncated to 16 bits and zero-extended before it feeds the next
    // interpolation level. That matters only once `(h-l)*a` overflows i32 - which upstream
    // knows happens, hence the `CMS_NO_SANITIZE` on this function. Returning a raw i32 here
    // would carry a wrapped negative value into the next level's `h-l`.
    let lerp = |a: i32, l: i32, h: i32| -> u16 {
        (l + (((h - l).wrapping_mul(a)).wrapping_add(0x8000) >> 16)) as u16
    };
    for out_chan in 0..total_out {
        let d00 = table[xx0 + yy0 + out_chan] as i32;
        let d01 = table[xx0 + yy1 + out_chan] as i32;
        let d10 = table[xx1 + yy0 + out_chan] as i32;
        let d11 = table[xx1 + yy1 + out_chan] as i32;
        let dx0 = lerp(rx, d00, d10) as i32;
        let dx1 = lerp(rx, d01, d11) as i32;
        output[out_chan] = lerp(ry, dx0, dx1);
    }
}

// ---------------------------------------------------------------------------------------
// 3 inputs (trilinear / tetrahedral)
// ---------------------------------------------------------------------------------------

fn trilinear_float(table: &[f32], input: &[f32], output: &mut [f32], p: &InterpParams) {
    let total_out = p.n_outputs;
    let px = fclamp(input[0]) * p.domain[0] as f32;
    let py = fclamp(input[1]) * p.domain[1] as f32;
    let pz = fclamp(input[2]) * p.domain[2] as f32;

    // Trilinear uses full floor (not the fast-floor trick).
    let x0 = px.floor() as i32;
    let fx = px - x0 as f32;
    let y0 = py.floor() as i32;
    let fy = py - y0 as f32;
    let z0 = pz.floor() as i32;
    let fz = pz - z0 as f32;

    let xx0 = (p.opta[2] as i32 * x0) as usize;
    let xx1 = xx0 + if fclamp(input[0]) >= 1.0 { 0 } else { p.opta[2] as usize };
    let yy0 = (p.opta[1] as i32 * y0) as usize;
    let yy1 = yy0 + if fclamp(input[1]) >= 1.0 { 0 } else { p.opta[1] as usize };
    let zz0 = (p.opta[0] as i32 * z0) as usize;
    let zz1 = zz0 + if fclamp(input[2]) >= 1.0 { 0 } else { p.opta[0] as usize };

    let lerp = |a: f32, l: f32, h: f32| l + (h - l) * a;
    for out_chan in 0..total_out {
        let dens = |i: usize, j: usize, k: usize| table[i + j + k + out_chan];
        let d000 = dens(xx0, yy0, zz0);
        let d001 = dens(xx0, yy0, zz1);
        let d010 = dens(xx0, yy1, zz0);
        let d011 = dens(xx0, yy1, zz1);
        let d100 = dens(xx1, yy0, zz0);
        let d101 = dens(xx1, yy0, zz1);
        let d110 = dens(xx1, yy1, zz0);
        let d111 = dens(xx1, yy1, zz1);

        let dx00 = lerp(fx, d000, d100);
        let dx01 = lerp(fx, d001, d101);
        let dx10 = lerp(fx, d010, d110);
        let dx11 = lerp(fx, d011, d111);
        let dxy0 = lerp(fy, dx00, dx10);
        let dxy1 = lerp(fy, dx01, dx11);
        output[out_chan] = lerp(fz, dxy0, dxy1);
    }
}

fn trilinear_16(table: &[u16], input: &[u16], output: &mut [u16], p: &InterpParams) {
    let total_out = p.n_outputs;
    let fx = to_fixed_domain(input[0] as i32 * p.domain[0] as i32);
    let x0 = (fx >> 16) as usize;
    let rx = fx & 0xFFFF;
    let fy = to_fixed_domain(input[1] as i32 * p.domain[1] as i32);
    let y0 = (fy >> 16) as usize;
    let ry = fy & 0xFFFF;
    let fz = to_fixed_domain(input[2] as i32 * p.domain[2] as i32);
    let z0 = (fz >> 16) as usize;
    let rz = fz & 0xFFFF;

    let xx0 = p.opta[2] as usize * x0;
    let xx1 = xx0 + if input[0] == 0xFFFF { 0 } else { p.opta[2] as usize };
    let yy0 = p.opta[1] as usize * y0;
    let yy1 = yy0 + if input[1] == 0xFFFF { 0 } else { p.opta[1] as usize };
    let zz0 = p.opta[0] as usize * z0;
    let zz1 = zz0 + if input[2] == 0xFFFF { 0 } else { p.opta[0] as usize };

    // See bilinear_16: the `(cmsUInt16Number)` cast lives inside C's LERP macro
    // (cmsintrp.c:550), so each of the three levels truncates before feeding the next.
    let lerp = |a: i32, l: i32, h: i32| -> u16 {
        (l + (((h - l).wrapping_mul(a)).wrapping_add(0x8000) >> 16)) as u16
    };
    for out_chan in 0..total_out {
        let dens = |i: usize, j: usize, k: usize| table[i + j + k + out_chan] as i32;
        let d000 = dens(xx0, yy0, zz0);
        let d001 = dens(xx0, yy0, zz1);
        let d010 = dens(xx0, yy1, zz0);
        let d011 = dens(xx0, yy1, zz1);
        let d100 = dens(xx1, yy0, zz0);
        let d101 = dens(xx1, yy0, zz1);
        let d110 = dens(xx1, yy1, zz0);
        let d111 = dens(xx1, yy1, zz1);

        let dx00 = lerp(rx, d000, d100) as i32;
        let dx01 = lerp(rx, d001, d101) as i32;
        let dx10 = lerp(rx, d010, d110) as i32;
        let dx11 = lerp(rx, d011, d111) as i32;
        let dxy0 = lerp(ry, dx00, dx10) as i32;
        let dxy1 = lerp(ry, dx01, dx11) as i32;
        output[out_chan] = lerp(rz, dxy0, dxy1);
    }
}

fn tetrahedral_float(table: &[f32], input: &[f32], output: &mut [f32], p: &InterpParams) {
    let total_out = p.n_outputs;
    let px = fclamp(input[0]) * p.domain[0] as f32;
    let py = fclamp(input[1]) * p.domain[1] as f32;
    let pz = fclamp(input[2]) * p.domain[2] as f32;

    let x0 = px.floor() as i32;
    let rx = px - x0 as f32;
    let y0 = py.floor() as i32;
    let ry = py - y0 as f32;
    let z0 = pz.floor() as i32;
    let rz = pz - z0 as f32;

    let xx0 = (p.opta[2] as i32 * x0) as usize;
    let xx1 = xx0 + if fclamp(input[0]) >= 1.0 { 0 } else { p.opta[2] as usize };
    let yy0 = (p.opta[1] as i32 * y0) as usize;
    let yy1 = yy0 + if fclamp(input[1]) >= 1.0 { 0 } else { p.opta[1] as usize };
    let zz0 = (p.opta[0] as i32 * z0) as usize;
    let zz1 = zz0 + if fclamp(input[2]) >= 1.0 { 0 } else { p.opta[0] as usize };

    for out_chan in 0..total_out {
        let dens = |i: usize, j: usize, k: usize| table[i + j + k + out_chan];
        let c0 = dens(xx0, yy0, zz0);
        let (c1, c2, c3);
        if rx >= ry && ry >= rz {
            c1 = dens(xx1, yy0, zz0) - c0;
            c2 = dens(xx1, yy1, zz0) - dens(xx1, yy0, zz0);
            c3 = dens(xx1, yy1, zz1) - dens(xx1, yy1, zz0);
        } else if rx >= rz && rz >= ry {
            c1 = dens(xx1, yy0, zz0) - c0;
            c2 = dens(xx1, yy1, zz1) - dens(xx1, yy0, zz1);
            c3 = dens(xx1, yy0, zz1) - dens(xx1, yy0, zz0);
        } else if rz >= rx && rx >= ry {
            c1 = dens(xx1, yy0, zz1) - dens(xx0, yy0, zz1);
            c2 = dens(xx1, yy1, zz1) - dens(xx1, yy0, zz1);
            c3 = dens(xx0, yy0, zz1) - c0;
        } else if ry >= rx && rx >= rz {
            c1 = dens(xx1, yy1, zz0) - dens(xx0, yy1, zz0);
            c2 = dens(xx0, yy1, zz0) - c0;
            c3 = dens(xx1, yy1, zz1) - dens(xx1, yy1, zz0);
        } else if ry >= rz && rz >= rx {
            c1 = dens(xx1, yy1, zz1) - dens(xx0, yy1, zz1);
            c2 = dens(xx0, yy1, zz0) - c0;
            c3 = dens(xx0, yy1, zz1) - dens(xx0, yy1, zz0);
        } else if rz >= ry && ry >= rx {
            c1 = dens(xx1, yy1, zz1) - dens(xx0, yy1, zz1);
            c2 = dens(xx0, yy1, zz1) - dens(xx0, yy0, zz1);
            c3 = dens(xx0, yy0, zz1) - c0;
        } else {
            c1 = 0.0;
            c2 = 0.0;
            c3 = 0.0;
        }
        output[out_chan] = c0 + c1 * rx + c2 * ry + c3 * rz;
    }
}

/// `TetrahedralInterp16` — the optimized fixed-point 3→N kernel with the branch tree and
/// running table pointer. Uses `wrapping_*` (Sakamoto `CMS_NO_SANITIZE` semantics).
fn tetrahedral_16(table: &[u16], input: &[u16], output: &mut [u16], p: &InterpParams) {
    let fx = to_fixed_domain(input[0] as i32 * p.domain[0] as i32);
    let fy = to_fixed_domain(input[1] as i32 * p.domain[1] as i32);
    let fz = to_fixed_domain(input[2] as i32 * p.domain[2] as i32);
    let x0 = fx >> 16;
    let y0 = fy >> 16;
    let z0 = fz >> 16;
    let rx = fx & 0xFFFF;
    let ry = fy & 0xFFFF;
    let rz = fz & 0xFFFF;

    let xx0 = p.opta[2] as i32 * x0;
    let mut x1 = if input[0] == 0xFFFF { 0 } else { p.opta[2] as i32 };
    let yy0 = p.opta[1] as i32 * y0;
    let mut y1 = if input[1] == 0xFFFF { 0 } else { p.opta[1] as i32 };
    let zz0 = p.opta[0] as i32 * z0;
    let mut z1 = if input[2] == 0xFFFF { 0 } else { p.opta[0] as i32 };

    let base = (xx0 + yy0 + zz0) as usize; // LutTable += X0+Y0+Z0
    let total_out = p.n_outputs;

    // Rest = c1*rx + c2*ry + c3*rz + 0x8001; out = (u16)c0 + ((Rest + (Rest>>16)) >> 16).
    let store = |c0: i32, c1: i32, c2: i32, c3: i32| -> u16 {
        let rest = c1
            .wrapping_mul(rx)
            .wrapping_add(c2.wrapping_mul(ry))
            .wrapping_add(c3.wrapping_mul(rz))
            .wrapping_add(0x8001);
        (c0 as u16 as i32).wrapping_add((rest.wrapping_add(rest >> 16)) >> 16) as u16
    };
    let at = |o: usize, off: i32| table[base + o + off as usize] as i32;
    let c0_at = |o: usize| table[base + o] as i32;

    if rx >= ry {
        if ry >= rz {
            y1 += x1;
            z1 += y1;
            for (o, out) in output.iter_mut().take(total_out).enumerate() {
                let (c1, c2, c3, c0) = (at(o, x1), at(o, y1), at(o, z1), c0_at(o));
                *out = store(c0, c1 - c0, c2 - c1, c3 - c2);
            }
        } else if rz >= rx {
            x1 += z1;
            y1 += x1;
            for (o, out) in output.iter_mut().take(total_out).enumerate() {
                let (c1, c2, c3, c0) = (at(o, x1), at(o, y1), at(o, z1), c0_at(o));
                *out = store(c0, c1 - c3, c2 - c1, c3 - c0);
            }
        } else {
            z1 += x1;
            y1 += z1;
            for (o, out) in output.iter_mut().take(total_out).enumerate() {
                let (c1, c2, c3, c0) = (at(o, x1), at(o, y1), at(o, z1), c0_at(o));
                *out = store(c0, c1 - c0, c2 - c3, c3 - c1);
            }
        }
    } else if rx >= rz {
        x1 += y1;
        z1 += x1;
        for (o, out) in output.iter_mut().take(total_out).enumerate() {
            let (c1, c2, c3, c0) = (at(o, x1), at(o, y1), at(o, z1), c0_at(o));
            *out = store(c0, c1 - c2, c2 - c0, c3 - c1);
        }
    } else if ry >= rz {
        z1 += y1;
        x1 += z1;
        for (o, out) in output.iter_mut().take(total_out).enumerate() {
            let (c1, c2, c3, c0) = (at(o, x1), at(o, y1), at(o, z1), c0_at(o));
            *out = store(c0, c1 - c3, c2 - c0, c3 - c2);
        }
    } else {
        y1 += z1;
        x1 += y1;
        for (o, out) in output.iter_mut().take(total_out).enumerate() {
            let (c1, c2, c3, c0) = (at(o, x1), at(o, y1), at(o, z1), c0_at(o));
            *out = store(c0, c1 - c2, c2 - c3, c3 - c0);
        }
    }
}

// ---------------------------------------------------------------------------------------
// 4 inputs
// ---------------------------------------------------------------------------------------

/// `Eval4Inputs` — 4→N fixed-point: two tetrahedral evaluations at K0/K1 then a lerp.
fn eval_4_inputs(table: &[u16], input: &[u16], output: &mut [u16], p16: &InterpParams) {
    let fk = to_fixed_domain(input[0] as i32 * p16.domain[0] as i32);
    let fx = to_fixed_domain(input[1] as i32 * p16.domain[1] as i32);
    let fy = to_fixed_domain(input[2] as i32 * p16.domain[2] as i32);
    let fz = to_fixed_domain(input[3] as i32 * p16.domain[3] as i32);

    let k0 = fk >> 16;
    let x0 = fx >> 16;
    let y0 = fy >> 16;
    let z0 = fz >> 16;
    let rk = fk & 0xFFFF;
    let rx = fx & 0xFFFF;
    let ry = fy & 0xFFFF;
    let rz = fz & 0xFFFF;

    let kk0 = p16.opta[3] as i32 * k0;
    let kk1 = kk0 + if input[0] == 0xFFFF { 0 } else { p16.opta[3] as i32 };
    let xx0 = p16.opta[2] as i32 * x0;
    let x1 = xx0 + if input[1] == 0xFFFF { 0 } else { p16.opta[2] as i32 };
    let yy0 = p16.opta[1] as i32 * y0;
    let y1 = yy0 + if input[2] == 0xFFFF { 0 } else { p16.opta[1] as i32 };
    let zz0 = p16.opta[0] as i32 * z0;
    let z1 = zz0 + if input[3] == 0xFFFF { 0 } else { p16.opta[0] as i32 };

    let mut tmp1 = [0u16; MAX_STAGE_CHANNELS];
    let mut tmp2 = [0u16; MAX_STAGE_CHANNELS];

    tetra_cube_16(table, kk0 as usize, xx0, x1, yy0, y1, zz0, z1, rx, ry, rz, p16.n_outputs, &mut tmp1);
    tetra_cube_16(table, kk1 as usize, xx0, x1, yy0, y1, zz0, z1, rx, ry, rz, p16.n_outputs, &mut tmp2);

    for i in 0..p16.n_outputs {
        output[i] = linear_interp(rk, tmp1[i], tmp2[i]);
    }
}

/// One tetrahedral cube evaluation over a sub-table at `base`, using the DENS-difference
/// form (the body shared by both halves of `Eval4Inputs`).
#[allow(clippy::too_many_arguments)]
fn tetra_cube_16(
    table: &[u16],
    base: usize,
    xx0: i32,
    x1: i32,
    yy0: i32,
    y1: i32,
    zz0: i32,
    z1: i32,
    rx: i32,
    ry: i32,
    rz: i32,
    n_outputs: usize,
    out: &mut [u16],
) {
    for out_chan in 0..n_outputs {
        let dens = |i: i32, j: i32, k: i32| table[base + (i + j + k) as usize + out_chan] as i32;
        let c0 = dens(xx0, yy0, zz0);
        let (c1, c2, c3);
        if rx >= ry && ry >= rz {
            c1 = dens(x1, yy0, zz0) - c0;
            c2 = dens(x1, y1, zz0) - dens(x1, yy0, zz0);
            c3 = dens(x1, y1, z1) - dens(x1, y1, zz0);
        } else if rx >= rz && rz >= ry {
            c1 = dens(x1, yy0, zz0) - c0;
            c2 = dens(x1, y1, z1) - dens(x1, yy0, z1);
            c3 = dens(x1, yy0, z1) - dens(x1, yy0, zz0);
        } else if rz >= rx && rx >= ry {
            c1 = dens(x1, yy0, z1) - dens(xx0, yy0, z1);
            c2 = dens(x1, y1, z1) - dens(x1, yy0, z1);
            c3 = dens(xx0, yy0, z1) - c0;
        } else if ry >= rx && rx >= rz {
            c1 = dens(x1, y1, zz0) - dens(xx0, y1, zz0);
            c2 = dens(xx0, y1, zz0) - c0;
            c3 = dens(x1, y1, z1) - dens(x1, y1, zz0);
        } else if ry >= rz && rz >= rx {
            c1 = dens(x1, y1, z1) - dens(xx0, y1, z1);
            c2 = dens(xx0, y1, zz0) - c0;
            c3 = dens(xx0, y1, z1) - dens(xx0, y1, zz0);
        } else if rz >= ry && ry >= rx {
            c1 = dens(x1, y1, z1) - dens(xx0, y1, z1);
            c2 = dens(xx0, y1, z1) - dens(xx0, yy0, z1);
            c3 = dens(xx0, yy0, z1) - c0;
        } else {
            c1 = 0;
            c2 = 0;
            c3 = 0;
        }
        let rest = c1
            .wrapping_mul(rx)
            .wrapping_add(c2.wrapping_mul(ry))
            .wrapping_add(c3.wrapping_mul(rz))
            .wrapping_add(0x8001);
        out[out_chan] = (c0 as u16 as i32).wrapping_add((rest.wrapping_add(rest >> 16)) >> 16) as u16;
    }
}

/// `Eval4InputsFloat` — 4→N float: peel channel 0, two tetrahedral-float, then lerp.
fn eval_4_inputs_float(table: &[f32], input: &[f32], output: &mut [f32], p: &InterpParams) {
    let pk = fclamp(input[0]) * p.domain[0] as f32;
    let k0 = quick_floor(pk as f64);
    let rest = pk - k0 as f32;

    let kk0 = (p.opta[3] as i32 * k0) as usize;
    let kk1 = kk0 + if fclamp(input[0]) >= 1.0 { 0 } else { p.opta[3] as usize };

    // Sub-cube geometry: domain shifted left by one, 3 inputs.
    let mut p1 = *p;
    p1.n_inputs = 3;
    for i in 0..3 {
        p1.domain[i] = p.domain[i + 1];
    }

    let mut tmp1 = [0f32; MAX_STAGE_CHANNELS];
    let mut tmp2 = [0f32; MAX_STAGE_CHANNELS];
    tetrahedral_float(&table[kk0..], &input[1..], &mut tmp1, &p1);
    tetrahedral_float(&table[kk1..], &input[1..], &mut tmp2, &p1);

    for i in 0..p.n_outputs {
        let y0 = tmp1[i];
        let y1 = tmp2[i];
        output[i] = y0 + (y1 - y0) * rest;
    }
}

// ---------------------------------------------------------------------------------------
// 5..15 inputs — the EVAL_FNS recursion (peel channel 0, recurse on N-1)
// ---------------------------------------------------------------------------------------

fn eval_nd_16(table: &[u16], input: &[u16], output: &mut [u16], p16: &InterpParams) {
    if p16.n_inputs == 4 {
        eval_4_inputs(table, input, output, p16);
        return;
    }
    let nm = p16.n_inputs - 1;
    let fk = to_fixed_domain(input[0] as i32 * p16.domain[0] as i32);
    let k0 = fk >> 16;
    let rk = fk & 0xFFFF;
    let kk0 = (p16.opta[nm] as i32 * k0) as usize;
    let kk1 = (p16.opta[nm] as i32 * (k0 + if input[0] != 0xFFFF { 1 } else { 0 })) as usize;

    let mut p1 = *p16;
    p1.n_inputs = nm;
    for i in 0..nm {
        p1.domain[i] = p16.domain[i + 1];
    }

    let mut tmp1 = [0u16; MAX_STAGE_CHANNELS];
    let mut tmp2 = [0u16; MAX_STAGE_CHANNELS];
    eval_nd_16(&table[kk0..], &input[1..], &mut tmp1, &p1);
    eval_nd_16(&table[kk1..], &input[1..], &mut tmp2, &p1);

    for i in 0..p16.n_outputs {
        output[i] = linear_interp(rk, tmp1[i], tmp2[i]);
    }
}

fn eval_nd_float(table: &[f32], input: &[f32], output: &mut [f32], p: &InterpParams) {
    if p.n_inputs == 4 {
        eval_4_inputs_float(table, input, output, p);
        return;
    }
    let nm = p.n_inputs - 1;
    let pk = fclamp(input[0]) * p.domain[0] as f32;
    let k0 = quick_floor(pk as f64);
    let rest = pk - k0 as f32;
    let kk0 = (p.opta[nm] as i32 * k0) as usize;
    let kk1 = kk0 + if fclamp(input[0]) >= 1.0 { 0 } else { p.opta[nm] as usize };

    let mut p1 = *p;
    p1.n_inputs = nm;
    for i in 0..nm {
        p1.domain[i] = p.domain[i + 1];
    }

    let mut tmp1 = [0f32; MAX_STAGE_CHANNELS];
    let mut tmp2 = [0f32; MAX_STAGE_CHANNELS];
    eval_nd_float(&table[kk0..], &input[1..], &mut tmp1, &p1);
    eval_nd_float(&table[kk1..], &input[1..], &mut tmp2, &p1);

    for i in 0..p.n_outputs {
        let y0 = tmp1[i];
        let y1 = tmp2[i];
        output[i] = y0 + (y1 - y0) * rest;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_d_identity() {
        // 2-node identity ramp [0, 0xffff], domain 1.
        let p = InterpParams::new(2, 1, 1, 0).unwrap();
        let table = [0u16, 0xffff];
        let mut out = [0u16];
        // Endpoints exact.
        p.eval16(&table, &[0], &mut out);
        assert_eq!(out[0], 0);
        p.eval16(&table, &[0xffff], &mut out);
        assert_eq!(out[0], 0xffff);
        // Midpoint: hand-derived above = 0x8000.
        p.eval16(&table, &[0x8000], &mut out);
        assert_eq!(out[0], 0x8000);
    }

    /// H3 regression: C's `LERP` macro truncates every intermediate to `cmsUInt16Number`.
    ///
    /// `#define LERP(a,l,h) (cmsUInt16Number) (l + ROUND_FIXED_TO_INT(((h-l)*a)))`
    /// (`cmsintrp.c:417`) casts INSIDE the macro, so `dx0`/`dx1` are truncated and
    /// zero-extended before feeding the second level. It only shows once `(h-l)*a` overflows
    /// i32 - which upstream expects, hence `CMS_NO_SANITIZE` on `BilinearInterp16`.
    ///
    /// Here `(65535-0) * 65535 = 0xFFFE0001` wraps to -131071, so `dx0` is 65534 in C but -2
    /// without the cast. Expected values re-derived from the C integer path, not observed.
    #[test]
    fn bilinear_16_truncates_intermediates_like_the_c_macro() {
        // 2x2 grid, 1 output: opta = [1, 2], Domain = [1, 1].
        let p = InterpParams::new(2, 2, 1, 0).unwrap();
        let table = [0u16, 30000, 65535, 30000];
        let mut out = [0u16];
        p.eval16(&table, &[65534, 32768], &mut out);
        assert_eq!(
            out[0], 47766,
            "intermediate LERP results must truncate to u16 before the next level"
        );
    }

    #[test]
    fn one_d_float_identity() {
        let p = InterpParams::new(2, 1, 1, CMS_LERP_FLAGS_FLOAT).unwrap();
        let table = [0.0f32, 1.0];
        let mut out = [0.0f32];
        p.eval_float(&table, &[0.5], &mut out);
        assert!((out[0] - 0.5).abs() < 1e-6);
        p.eval_float(&table, &[1.0], &mut out);
        assert_eq!(out[0], 1.0);
    }

    // Build a 2^3 RGB->RGB identity CLUT: node[r][g][b] = (r,g,b) scaled to 0/0xffff.
    fn identity_clut_2() -> Vec<u16> {
        let mut t = Vec::new();
        for r in 0..2 {
            for g in 0..2 {
                for b in 0..2 {
                    t.push((r * 0xffff) as u16);
                    t.push((g * 0xffff) as u16);
                    t.push((b * 0xffff) as u16);
                }
            }
        }
        t
    }

    #[test]
    fn tetra16_corners_exact() {
        let table = identity_clut_2();
        let p = InterpParams::new(2, 3, 3, 0).unwrap();
        for &(r, g, b) in &[(0u16, 0u16, 0u16), (0xffff, 0, 0), (0, 0xffff, 0), (0xffff, 0xffff, 0xffff)] {
            let mut out = [0u16; 3];
            p.eval16(&table, &[r, g, b], &mut out);
            assert_eq!(out, [r, g, b], "corner ({r:x},{g:x},{b:x})");
        }
    }

    #[test]
    fn trilinear_and_tetra_agree_on_corners() {
        let table = identity_clut_2();
        let tetra = InterpParams::new(2, 3, 3, 0).unwrap();
        let tri = InterpParams::new(2, 3, 3, CMS_LERP_FLAGS_TRILINEAR).unwrap();
        for inp in [[0u16, 0, 0], [0xffff, 0xffff, 0xffff], [0xffff, 0, 0xffff]] {
            let (mut a, mut b) = ([0u16; 3], [0u16; 3]);
            tetra.eval16(&table, &inp, &mut a);
            tri.eval16(&table, &inp, &mut b);
            assert_eq!(a, b, "corner {inp:?}");
        }
    }

    #[test]
    fn tetra_float_midpoint_identity() {
        // Float identity CLUT, gray input -> gray output (all channels equal at r=g=b).
        let mut t = Vec::new();
        for r in 0..2 {
            for g in 0..2 {
                for b in 0..2 {
                    t.push(r as f32);
                    t.push(g as f32);
                    t.push(b as f32);
                }
            }
        }
        let p = InterpParams::new(2, 3, 3, CMS_LERP_FLAGS_FLOAT).unwrap();
        let mut out = [0f32; 3];
        p.eval_float(&t, &[0.5, 0.5, 0.5], &mut out);
        for c in out {
            assert!((c - 0.5).abs() < 1e-6, "got {out:?}");
        }
    }

    #[test]
    fn cmyk_4d_corners_exact() {
        // 2^4 CMYK->CMYK identity CLUT.
        let mut t = Vec::new();
        for c in 0..2 {
            for m in 0..2 {
                for y in 0..2 {
                    for k in 0..2 {
                        t.push((c * 0xffff) as u16);
                        t.push((m * 0xffff) as u16);
                        t.push((y * 0xffff) as u16);
                        t.push((k * 0xffff) as u16);
                    }
                }
            }
        }
        let p = InterpParams::new(2, 4, 4, 0).unwrap();
        for inp in [[0u16, 0, 0, 0], [0xffff, 0xffff, 0xffff, 0xffff], [0xffff, 0, 0xffff, 0]] {
            let mut out = [0u16; 4];
            p.eval16(&t, &inp, &mut out);
            assert_eq!(out, inp, "cmyk corner {inp:?}");
        }
    }

    #[test]
    fn opta_strides_computed() {
        // 3 inputs, 3 outputs, 17 nodes: opta = [3, 3*17, 3*17*17].
        let p = InterpParams::new(17, 3, 3, 0).unwrap();
        assert_eq!(p.opta[0], 3);
        assert_eq!(p.opta[1], 3 * 17);
        assert_eq!(p.opta[2], 3 * 17 * 17);
        assert_eq!(p.domain[0], 16);
    }

    #[test]
    fn too_many_inputs_rejected() {
        assert!(InterpParams::new(2, 16, 3, 0).is_err());
        assert!(InterpParams::new(2, 4, MAX_STAGE_CHANNELS, 0).is_err());
    }

    /// C's `DefaultInterpolatorsFactory` has no `case 0:`, so it falls to
    /// `default: Lerp16 = NULL` (`cmsintrp.c:1325-1326`); `_cmsSetInterpolationRoutine` then
    /// returns FALSE (`:101-103`) and `_cmsComputeInterpParamsEx` returns NULL (`:149-153`).
    /// Rejecting at construction is what keeps eval panic-free: the `_ =>` dispatch arm would
    /// otherwise reach `n_inputs - 1` and underflow.
    #[test]
    fn zero_inputs_rejected_at_construction() {
        assert!(InterpParams::new_ex(&[], 0, 3, 0).is_err());
        // Reachable through the CLUT constructor, where `cube_size(pts, 0)` is an empty
        // product and so passes the existing zero-entry check.
        assert!(crate::pipeline::Stage::alloc_clut_16bit_granular(&[], 0, 3, None).is_err());
    }

    /// A zero-node axis would make `Domain[i] = nSamples[i] - 1` underflow. C wraps it to
    /// `UINT_MAX` (`cmsintrp.c:140`) and then indexes with it; rejecting is deliberately
    /// stricter. A short `n_samples` slice is rejected for the same reason — C reads
    /// `nSamples[InputChan-i]` unconditionally (`:146`).
    #[test]
    fn degenerate_sample_counts_rejected() {
        assert!(InterpParams::new_ex(&[2, 0, 2], 3, 3, 0).is_err());
        assert!(InterpParams::new_ex(&[2, 2], 3, 3, 0).is_err());
    }
}
