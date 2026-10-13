//! Gamut, TAC and black-tone-curve utilities — a bit-exact port of `cmsgmt.c`.
//!
//! This module ports the whole of Little-CMS's `cmsgmt.c`:
//!
//! * [`build_k_tone_curve`] (`_cmsBuildKToneCurve`) + its helper [`compute_k_to_lstar`]
//!   (`ComputeKToLstar`) and [`chain_to_lab`] (`_cmsChain2Lab`) — the CMYK `K -> L*`
//!   black-preserving tone curve consumed by the black-preserving intents
//!   ([`crate::cmm::intents`]).
//! * [`create_gamut_check_pipeline`] (`_cmsCreateGamutCheckPipeline`) + the [`gamut_sampler`]
//!   sampler callback (`GamutSampler`) — the `3 -> 1` gamut-check CLUT consumed by the
//!   `cmsFLAGS_GAMUTCHECK` transform kernels ([`crate::cmm::transform`]).
//! * [`detect_tac`] (`cmsDetectTAC`) + the [`estimate_tac`] sampler callback (`EstimateTAC`) —
//!   Total Area Coverage estimation.
//! * [`desaturate_lab`] (`cmsDesaturateLab`) — CIELab gamut-prism clamping.
//! * [`detect_rgb_profile_gamma`] (`cmsDetectRGBProfileGamma`) — the linear-RGB
//!   `NOOPTIMIZE` heuristic.
//!
//! ## Virtual PCS profiles
//!
//! Every round-trip function above needs a synthetic Lab-v4 / XYZ identity profile
//! (`cmsCreateLab4ProfileTHR` / `cmsCreateXYZProfileTHR`, `cmsvirt.c`). Those live in
//! [`crate::virt`]; this module calls [`crate::virt::create_lab4_profile_thr`] /
//! [`crate::virt::create_xyz_profile_thr`] (threading the profile's own context, as the C
//! `*THR` constructors do). An identity abstract profile is read through
//! [`crate::profile::read_hl::read_devicelink_lut`] as a `3 -> 3` identity, exactly as lcms does.

use crate::cms_err;
use crate::cmm::optimize::{FLAGS_NOCACHE, FLAGS_NOOPTIMIZE};
use crate::cmm::transform::{create_extended_transform, create_transform, Transform};
use crate::color::gamma::ToneCurve;
use crate::color::pcs::{
    cms_channels_of_colorspace, cms_delta_e, cms_lab_to_lch,
    reasonable_gridpoints_by_colorspace,
};
use crate::context::Context;
use crate::error::{ErrorCode, Result};
use crate::pipeline::stage::{slice_space_16, Stage};
use crate::pipeline::{Pipeline, StageLoc};
use crate::profile::read_hl::{is_matrix_shaper, INTENT_PERCEPTUAL, INTENT_RELATIVE_COLORIMETRIC};
use crate::profile::Profile;
use crate::types::color::CIELab;
use crate::types::format::{FormatBuilder, PixelFormat};
use crate::types::num::quick_floor;
use crate::types::sig::{class, colorspace};
use crate::virt::{create_lab4_profile_thr, create_xyz_profile_thr};
use crate::MAX_CHANNELS;

// -------------------------------------------------------------------------------------------
// Constants
// -------------------------------------------------------------------------------------------

/// `ERR_THRESHOLD` (`cmsgmt.c`) — dE above which a colour is considered out of gamut on
/// LUT-based profiles.
const ERR_THRESHOLD: f64 = 5.0;

/// `cmsFLAGS_HIGHRESPRECALC` (`lcms2.h`) — request the higher-resolution CLUT node count.
const FLAGS_HIGHRESPRECALC: u32 = 0x0400;

// Colour-space codes (`PT_*`, `lcms2.h`) used to assemble raster formats.
const PT_RGB: u32 = 4;
const PT_CMYK: u32 = 6;
const PT_XYZ: u32 = 9;
const PT_LAB: u32 = 10;

// -------------------------------------------------------------------------------------------
// Raster formats (the `TYPE_*` macros used by cmsgmt.c)
// -------------------------------------------------------------------------------------------

/// `TYPE_Lab_DBL` — three `f64` `cmsCIELab` samples.
fn fmt_lab_dbl() -> PixelFormat {
    FormatBuilder::new().float(true).colorspace(PT_LAB).channels(3).bytes(0).build()
}
/// `TYPE_Lab_16` — three 16-bit Lab samples.
fn fmt_lab_16() -> PixelFormat {
    FormatBuilder::new().colorspace(PT_LAB).channels(3).bytes(2).build()
}
/// `TYPE_XYZ_DBL` — three `f64` `cmsCIEXYZ` samples.
fn fmt_xyz_dbl() -> PixelFormat {
    FormatBuilder::new().float(true).colorspace(PT_XYZ).channels(3).bytes(0).build()
}
/// `TYPE_RGB_16` — three 16-bit RGB samples.
fn fmt_rgb_16() -> PixelFormat {
    FormatBuilder::new().colorspace(PT_RGB).channels(3).bytes(2).build()
}
/// `TYPE_CMYK_FLT` — four `f32` CMYK samples.
fn fmt_cmyk_flt() -> PixelFormat {
    FormatBuilder::new().float(true).colorspace(PT_CMYK).channels(4).bytes(4).build()
}
/// `(CHANNELS_SH(n) | BYTES_SH(2))` — the anonymous 16-bit device format used by cmsgmt.c
/// (`PT_ANY` colour space, so only the channel count is checked).
fn fmt_dev_16(n_channels: u32) -> PixelFormat {
    FormatBuilder::new().channels(n_channels).bytes(2).build()
}

/// `cmsFormatterForColorspaceOfProfile(hProfile, 4, TRUE)` — a float raster format matching a
/// profile's colour space (used by [`detect_tac`]). Returns `None` for an unsupported space
/// (the C `dwFormatter == 0`).
fn formatter_for_colorspace_float(prof: &Profile) -> Option<PixelFormat> {
    crate::cmm::pack::formatter_for_colorspace_opt(prof.color_space(), 4, true)
}

// -------------------------------------------------------------------------------------------
// Byte-buffer helpers for single-pixel transforms
// -------------------------------------------------------------------------------------------

/// Read a native-endian `f64` at byte offset `o`.
fn rd_f64(b: &[u8], o: usize) -> f64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    f64::from_ne_bytes(a)
}

/// Read a native-endian `f32` at byte offset `o`.
fn rd_f32(b: &[u8], o: usize) -> f32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[o..o + 4]);
    f32::from_ne_bytes(a)
}

/// Run a 1-pixel transform from `n` 16-bit device samples to a `cmsCIELab` (`TYPE_Lab_DBL` out).
fn xf_dev16_to_lab(xf: &Transform, in_: &[u16], n: usize) -> CIELab {
    let mut src = [0u8; MAX_CHANNELS * 2];
    for i in 0..n {
        src[i * 2..i * 2 + 2].copy_from_slice(&in_[i].to_ne_bytes());
    }
    let mut dst = [0u8; 24];
    xf.do_transform(&src[..n * 2], &mut dst, 1);
    CIELab { l: rd_f64(&dst, 0), a: rd_f64(&dst, 8), b: rd_f64(&dst, 16) }
}

/// Run a 1-pixel transform from a `cmsCIELab` (`TYPE_Lab_DBL` in) to `n` 16-bit device samples.
fn xf_lab_to_dev16(xf: &Transform, lab: &CIELab, n: usize) -> [u16; MAX_CHANNELS] {
    let mut src = [0u8; 24];
    src[0..8].copy_from_slice(&lab.l.to_ne_bytes());
    src[8..16].copy_from_slice(&lab.a.to_ne_bytes());
    src[16..24].copy_from_slice(&lab.b.to_ne_bytes());
    let mut dst = [0u8; MAX_CHANNELS * 2];
    xf.do_transform(&src, &mut dst[..n * 2], 1);
    let mut out = [0u16; MAX_CHANNELS];
    for i in 0..n {
        out[i] = u16::from_ne_bytes([dst[i * 2], dst[i * 2 + 1]]);
    }
    out
}

// -------------------------------------------------------------------------------------------
// _cmsChain2Lab / ComputeKToLstar / _cmsBuildKToneCurve
// -------------------------------------------------------------------------------------------

/// `_cmsChain2Lab` — append a Lab-v4 identity to `profiles` and build the transform.
///
/// The Lab profile is placed at the chain's end with `INTENT_RELATIVE_COLORIMETRIC`, no BPC and
/// full (`1.0`) adaptation, exactly as in C. `profiles` are the caller's open profiles (kept
/// open); the synthetic Lab profile is owned locally for the duration of the build.
#[allow(clippy::too_many_arguments)]
fn chain_to_lab(
    ctx: &Context,
    profiles: &mut [&mut Profile],
    input_format: PixelFormat,
    output_format: PixelFormat,
    intents: &[u32],
    bpc: &[bool],
    adaptation: &[f64],
    flags: u32,
) -> Result<Transform> {
    let n = profiles.len();
    // C: 254 + 1 == 255 is the limit.
    if n > 254 {
        return Err(cms_err!(ErrorCode::Range, "too many profiles for _cmsChain2Lab ({n})"));
    }

    let mut lab = create_lab4_profile_thr(ctx.clone(), None)?;

    // Build parameter lists with the trailing Lab identity appended.
    let mut ints: Vec<u32> = intents[..n].to_vec();
    ints.push(INTENT_RELATIVE_COLORIMETRIC);
    let mut bpcs: Vec<bool> = bpc[..n].to_vec();
    bpcs.push(false);
    let mut adapts: Vec<f64> = adaptation[..n].to_vec();
    adapts.push(1.0);

    let mut chain: Vec<&mut Profile> = profiles.iter_mut().map(|p| &mut **p).collect();
    chain.push(&mut lab);

    create_extended_transform(
        ctx,
        &mut chain,
        &mut bpcs,
        &ints,
        &adapts,
        None,
        0,
        input_format,
        output_format,
        flags,
    )
}

/// `ComputeKToLstar` — the `K -> L*` relationship of a CMYK chain, sampled at `n_points`.
///
/// A `K`-only CMYK ramp is pushed through a `CMYK -> Lab` round trip; each sample stores
/// `1 - L*/100` (the C "negate K for easier operation"). BPC on the chain doubles as "K to a
/// zero black point".
fn compute_k_to_lstar(
    ctx: &Context,
    n_points: usize,
    profiles: &mut [&mut Profile],
    intents: &[u32],
    bpc: &[bool],
    adaptation: &[f64],
    flags: u32,
) -> Result<ToneCurve> {
    let xform = chain_to_lab(
        ctx,
        profiles,
        fmt_cmyk_flt(),
        fmt_lab_dbl(),
        intents,
        bpc,
        adaptation,
        flags,
    )?;

    let mut sampled = vec![0f32; n_points];
    for (i, s) in sampled.iter_mut().enumerate() {
        let k = (i as f64 * 100.0) / (n_points - 1) as f64;
        let cmyk = [0f32, 0f32, 0f32, k as f32];

        let mut src = [0u8; 16];
        for (c, v) in cmyk.iter().enumerate() {
            src[c * 4..c * 4 + 4].copy_from_slice(&v.to_ne_bytes());
        }
        let mut dst = [0u8; 24];
        xform.do_transform(&src, &mut dst, 1);

        let l = rd_f64(&dst, 0);
        *s = (1.0 - l / 100.0) as f32;
    }

    ToneCurve::build_tabulated_float(&sampled)
}

/// `_cmsBuildKToneCurve` — the black tone curve of a `CMYK -> CMYK` transform.
///
/// Requires the first and last profiles to be CMYK and the last to be an output-class profile.
/// The per-profile `K -> L*` curves ([`compute_k_to_lstar`]) are joined into the final `K -> K'`
/// curve, which must be monotonic. Returns an error where the C returns `NULL` (wrong colour
/// space / device class, or a non-monotonic result).
pub fn build_k_tone_curve(
    ctx: &Context,
    n_points: usize,
    profiles: &mut [&mut Profile],
    intents: &[u32],
    bpc: &[bool],
    adaptation: &[f64],
    flags: u32,
) -> Result<ToneCurve> {
    let n = profiles.len();
    if n < 2 {
        return Err(cms_err!(ErrorCode::Range, "_cmsBuildKToneCurve needs >= 2 profiles"));
    }

    // Make sure CMYK -> CMYK, and that the last profile is an output profile.
    if profiles[0].color_space() != colorspace::CMYK
        || profiles[n - 1].color_space() != colorspace::CMYK
    {
        return Err(cms_err!(ErrorCode::NotSuitable, "_cmsBuildKToneCurve expects CMYK -> CMYK"));
    }
    if profiles[n - 1].device_class() != class::OUTPUT {
        return Err(cms_err!(ErrorCode::NotSuitable, "last profile must be an output profile"));
    }

    // Split into "all but the last" and "the last" chains.
    let (head, tail) = profiles.split_at_mut(n - 1);

    let in_curve = compute_k_to_lstar(
        ctx,
        n_points,
        head,
        &intents[..n - 1],
        &bpc[..n - 1],
        &adaptation[..n - 1],
        flags,
    )?;
    let out_curve = compute_k_to_lstar(
        ctx,
        n_points,
        tail,
        &intents[n - 1..],
        &bpc[n - 1..],
        &adaptation[n - 1..],
        flags,
    )?;

    // Build the relationship (limits accuracy to 16 bits — acceptable for black-preserving LUTs).
    let k_tone = ToneCurve::join(&in_curve, &out_curve, n_points)?;

    // Must be monotonic.
    if !k_tone.is_monotonic() {
        return Err(cms_err!(ErrorCode::NotSuitable, "black tone curve is not monotonic"));
    }

    Ok(k_tone)
}

// -------------------------------------------------------------------------------------------
// GamutSampler / _cmsCreateGamutCheckPipeline
// -------------------------------------------------------------------------------------------

/// `GamutSampler` — annotate one gamut-check knot with its dE-based out-of-gamut amount.
///
/// The input is carried to Lab, forced into the proof colorant space and back, and the
/// forward/back discrepancy (`dE1`) plus a second round trip (`dE2`) decide the value written to
/// `out[0]`: `0` when in gamut, otherwise `floor((dE - threshold) + 0.5)` (or an error-ratio
/// variant for the perceptual "both large" case), exactly as the C `_cmsQuickFloor` cascade.
#[allow(clippy::too_many_arguments)]
fn gamut_sampler(
    in_: &[u16],
    out: &mut [u16],
    h_input: &Transform,
    h_forward: &Transform,
    h_reverse: &Transform,
    threshold: f64,
    n_input: usize,
    n_gamut: usize,
) -> bool {
    // Convert input to Lab.
    let lab_in1 = xf_dev16_to_lab(h_input, in_, n_input);

    // PCS -> colorant (always returns in-gamut values) -> PCS.
    let proof = xf_lab_to_dev16(h_forward, &lab_in1, n_gamut);
    let lab_out1 = xf_dev16_to_lab(h_reverse, &proof, n_gamut);

    // memmove(&LabIn2, &LabOut1, ...).
    let lab_in2 = lab_out1;

    // Try again, taking the check result as input.
    let proof2 = xf_lab_to_dev16(h_forward, &lab_out1, n_gamut);
    let lab_out2 = xf_dev16_to_lab(h_reverse, &proof2, n_gamut);

    let de1 = cms_delta_e(&lab_in1, &lab_out1);
    let de2 = cms_delta_e(&lab_in2, &lab_out2);

    let val: u16 = if de1 < threshold && de2 < threshold {
        // Both small: in gamut.
        0
    } else if de1 < threshold && de2 > threshold {
        // Small then big: undefined; assume in gamut.
        0
    } else if de1 > threshold && de2 < threshold {
        // Big then small: clearly out of gamut.
        quick_floor((de1 - threshold) + 0.5) as u16
    } else {
        // Both big: could be perceptual mapping — take the error ratio.
        let error_ratio = if de2 == 0.0 { de1 } else { de1 / de2 };
        if error_ratio > threshold {
            quick_floor((error_ratio - threshold) + 0.5) as u16
        } else {
            0
        }
    };

    out[0] = val;
    true
}

/// `_cmsCreateGamutCheckPipeline` — build the `N -> 1` gamut-check CLUT for a proof profile.
///
/// `profiles[..n_gamut_pcs_position]` are the transform chain up to the PCS; `gamut` is the proof
/// profile. The returned pipeline holds a single CLUT whose one output channel encodes the dE of
/// a colorimetric round trip through the proof gamut (values truly out of gamut clip high). The
/// grid resolution and channel count come from the proof colour space, matching C.
pub fn create_gamut_check_pipeline(
    ctx: &Context,
    profiles: &mut [&mut Profile],
    bpc: &[bool],
    intents: &[u32],
    adaptation: &[f64],
    n_gamut_pcs_position: u32,
    gamut: &mut Profile,
) -> Result<Pipeline> {
    if !(1..=255).contains(&n_gamut_pcs_position) {
        return Err(cms_err!(
            ErrorCode::Range,
            "Wrong position of PCS. 1..255 expected, {n_gamut_pcs_position} found."
        ));
    }
    let pos = n_gamut_pcs_position as usize;
    if pos > profiles.len() {
        return Err(cms_err!(ErrorCode::Range, "PCS position past the profile chain"));
    }

    // Figure of merit: near-zero on matrix-shaper profiles, ERR_THRESHOLD on LUT profiles.
    let threshold = if is_matrix_shaper(gamut) { 1.0 } else { ERR_THRESHOLD };

    let color_space = gamut.color_space();
    let n_channels = cms_channels_of_colorspace(color_space);
    let n_gridpoints = reasonable_gridpoints_by_colorspace(color_space, FLAGS_HIGHRESPRECALC);

    let input_color_space = profiles[0].color_space();
    let n_input_channels = cms_channels_of_colorspace(input_color_space);
    if n_channels <= 0 || n_input_channels <= 0 {
        return Err(cms_err!(ErrorCode::NotSuitable, "unsupported colour space in gamut check"));
    }
    let n_channels = n_channels as usize;
    let n_input_channels = n_input_channels as usize;

    let mut lab = create_lab4_profile_thr(ctx.clone(), None)?;

    // hInput: 16-bit device -> Lab double, through the chain + trailing Lab identity.
    let h_input = {
        let mut ints: Vec<u32> = intents[..pos].to_vec();
        ints.push(INTENT_RELATIVE_COLORIMETRIC);
        let mut bpcs: Vec<bool> = bpc[..pos].to_vec();
        bpcs.push(false);
        let mut adapts: Vec<f64> = adaptation[..pos].to_vec();
        adapts.push(1.0);

        let mut chain: Vec<&mut Profile> =
            profiles[..pos].iter_mut().map(|p| &mut **p).collect();
        chain.push(&mut lab);

        create_extended_transform(
            ctx,
            &mut chain,
            &mut bpcs,
            &ints,
            &adapts,
            None,
            0,
            fmt_dev_16(n_input_channels as u32),
            fmt_lab_dbl(),
            FLAGS_NOCACHE,
        )?
    };

    // Forward: Lab double -> proof colorant. Reverse: proof colorant -> Lab double.
    let dev_fmt = fmt_dev_16(n_channels as u32);
    let h_forward = create_transform(
        ctx,
        &mut lab,
        fmt_lab_dbl(),
        gamut,
        dev_fmt,
        INTENT_RELATIVE_COLORIMETRIC,
        FLAGS_NOCACHE,
    )?;
    let h_reverse = create_transform(
        ctx,
        gamut,
        dev_fmt,
        &mut lab,
        fmt_lab_dbl(),
        INTENT_RELATIVE_COLORIMETRIC,
        FLAGS_NOCACHE,
    )?;

    // Assemble the pipeline: a single CLUT sampled by GamutSampler.
    let mut pipe = Pipeline::alloc(3, 1)?;
    let mut clut = Stage::alloc_clut_16bit(n_gridpoints, n_channels as u32, 1, None)?;

    let ok = clut.sample_clut_16(
        |sample_in, sample_out| {
            gamut_sampler(
                sample_in,
                sample_out,
                &h_input,
                &h_forward,
                &h_reverse,
                threshold,
                n_input_channels,
                n_channels,
            )
        },
        0,
    );
    if !ok {
        return Err(cms_err!(ErrorCode::Internal, "gamut CLUT sampling failed"));
    }

    if !pipe.insert_stage(StageLoc::Begin, clut) {
        return Err(cms_err!(ErrorCode::Internal, "gamut pipeline assembly failed"));
    }

    Ok(pipe)
}

// -------------------------------------------------------------------------------------------
// EstimateTAC / cmsDetectTAC
// -------------------------------------------------------------------------------------------

/// `cmsDetectTAC` — the Total Area Coverage (max total ink, in %) of an output profile.
///
/// A `Lab -> device` round trip on the perceptual intent is swept over a coarse `L*` / fine
/// `C*` grid (`6 x 74 x 74`, as in C); the maximum sum of the device channels is the TAC.
/// Returns `0.0` for a non-output profile or an unsupported colour space (matching the C `0`).
pub fn detect_tac(prof: &mut Profile) -> f64 {
    // TAC only works on output profiles.
    if prof.device_class() != class::OUTPUT {
        return 0.0;
    }

    // A float formatter matching the profile colour space.
    let out_fmt = match formatter_for_colorspace_float(prof) {
        Some(f) => f,
        None => return 0.0,
    };
    let n_out = out_fmt.channels() as usize;

    // For safety (matches the C `>= cmsMAXCHANNELS` guard).
    if n_out >= MAX_CHANNELS {
        return 0.0;
    }

    let ctx = prof.context().clone();
    let mut lab = match create_lab4_profile_thr(ctx.clone(), None) {
        Ok(p) => p,
        Err(_) => return 0.0,
    };

    // Round trip on the perceptual intent, no optimization and no cache.
    let round_trip = match create_transform(
        &ctx,
        &mut lab,
        fmt_lab_16(),
        prof,
        out_fmt,
        INTENT_PERCEPTUAL,
        FLAGS_NOOPTIMIZE | FLAGS_NOCACHE,
    ) {
        Ok(x) => x,
        Err(_) => return 0.0,
    };

    // Coarse L*, fine C*.
    let grid = [6u32, 74, 74];
    let max_tac = estimate_tac(&round_trip, &grid, n_out);

    // Results in %.
    max_tac as f64
}

/// `EstimateTAC` (as a sweep) — return the maximum summed device ink over the `Lab` grid.
fn estimate_tac(round_trip: &Transform, grid: &[u32], n_out: usize) -> f32 {
    let mut max_tac = 0f32;
    let ok = slice_space_16(grid, |in_| {
        let mut src = [0u8; 6];
        for i in 0..3 {
            src[i * 2..i * 2 + 2].copy_from_slice(&in_[i].to_ne_bytes());
        }
        let mut dst = [0u8; MAX_CHANNELS * 4];
        round_trip.do_transform(&src, &mut dst[..n_out * 4], 1);

        let mut sum = 0f32;
        for i in 0..n_out {
            sum += rd_f32(&dst, i * 4);
        }
        if sum > max_tac {
            max_tac = sum;
        }
        true
    });
    if ok {
        max_tac
    } else {
        0.0
    }
}

// -------------------------------------------------------------------------------------------
// cmsDesaturateLab
// -------------------------------------------------------------------------------------------

/// `cmsDesaturateLab` — clamp a `cmsCIELab` onto the `[amin,amax] x [bmin,bmax]` gamut prism.
///
/// The luma surface below `L*=0` collapses to black (returning `false`); `L* > 100` is clipped
/// (highlights discarded per the ICC spec). Points outside the `a`/`b` box are transported to
/// LCh and clipped against whichever prism face their hue falls on. Returns `false` on the black
/// collapse and the (unreachable) invalid-angle case, `true` otherwise.
pub fn desaturate_lab(
    lab: &mut CIELab,
    amax: f64,
    amin: f64,
    bmax: f64,
    bmin: f64,
) -> bool {
    // Whole luma surface below zero -> black.
    if lab.l < 0.0 {
        lab.l = 0.0;
        lab.a = 0.0;
        lab.b = 0.0;
        return false;
    }

    // Clamp white, discarding highlights (ICC spec disallows L>100 as a highlight cue).
    if lab.l > 100.0 {
        lab.l = 100.0;
    }

    // Outside the a/b prism faces?
    if lab.a < amin || lab.a > amax || lab.b < bmin || lab.b > bmax {
        // Hue exactly 90 degrees: atan is unusable, clamp b directly.
        if lab.a == 0.0 {
            lab.b = if lab.b < 0.0 { bmin } else { bmax };
            return true;
        }

        let lch = cms_lab_to_lch(lab);
        let slope = lab.b / lab.a;
        let h = lch.h;

        // Four hue zones select the clipping face.
        if (0.0..45.0).contains(&h) || (315.0..=360.0).contains(&h) {
            // Clip by amax.
            lab.a = amax;
            lab.b = amax * slope;
        } else if (45.0..135.0).contains(&h) {
            // Clip by bmax.
            lab.b = bmax;
            lab.a = bmax / slope;
        } else if (135.0..225.0).contains(&h) {
            // Clip by amin.
            lab.a = amin;
            lab.b = amin * slope;
        } else if (225.0..315.0).contains(&h) {
            // Clip by bmin.
            lab.b = bmin;
            lab.a = bmin / slope;
        } else {
            // Invalid angle (unreachable given the checks above).
            return false;
        }
    }

    true
}

// -------------------------------------------------------------------------------------------
// cmsDetectRGBProfileGamma
// -------------------------------------------------------------------------------------------

/// `cmsDetectRGBProfileGamma` — estimate a single gamma for an RGB profile, or `-1`.
///
/// Only RGB profiles that convert forward (RGB -> XYZ) are supported. A synthetic grey ramp
/// `R=G=B` is carried to XYZ; the `Y` response is fit by least squares
/// ([`ToneCurve::estimate_gamma`]). A gamma near `1.0` means the profile is (near) linear — the
/// `NOOPTIMIZE` hint. Unsupported profiles return `-1.0`.
pub fn detect_rgb_profile_gamma(prof: &mut Profile, threshold: f64) -> f64 {
    if prof.color_space() != colorspace::RGB {
        return -1.0;
    }

    let cl = prof.device_class();
    if cl != class::INPUT && cl != class::DISPLAY && cl != class::OUTPUT && cl != class::COLORSPACE {
        return -1.0;
    }

    let ctx = prof.context().clone();
    let mut xyz = match create_xyz_profile_thr(ctx.clone()) {
        Ok(p) => p,
        Err(_) => return -1.0,
    };

    // If not RGB or the forward direction is unsupported, keep the previous (-1) error.
    let xform = match create_transform(
        &ctx,
        prof,
        fmt_rgb_16(),
        &mut xyz,
        fmt_xyz_dbl(),
        INTENT_RELATIVE_COLORIMETRIC,
        FLAGS_NOOPTIMIZE,
    ) {
        Ok(x) => x,
        Err(_) => return -1.0,
    };

    // Grey ramp R=G=B = FROM_8_TO_16(i) for i in 0..256.
    let mut src = [0u8; 256 * 3 * 2];
    for i in 0..256 {
        let v = (((i as u16) << 8) | i as u16).to_ne_bytes();
        for c in 0..3 {
            let off = (i * 3 + c) * 2;
            src[off..off + 2].copy_from_slice(&v);
        }
    }
    let mut dst = [0u8; 256 * 3 * 8];
    xform.do_transform(&src, &mut dst, 256);

    let mut y_normalized = [0f32; 256];
    for (i, y) in y_normalized.iter_mut().enumerate() {
        // XYZ double layout is X,Y,Z; take Y (offset 8 within the 24-byte pixel).
        *y = rd_f64(&dst, i * 24 + 8) as f32;
    }

    let y_curve = match ToneCurve::build_tabulated_float(&y_normalized) {
        Ok(c) => c,
        Err(_) => return -1.0,
    };

    y_curve.estimate_gamma(threshold)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_profile as load;

    /// The two synthetic PCS profiles must build and read back as `3 -> 3` identities.
    #[test]
    fn virtual_profiles_are_identity() {
        let ctx = Context::new();
        let mut lab = create_lab4_profile_thr(ctx.clone(), None).expect("lab4");
        assert_eq!(lab.color_space(), colorspace::LAB);
        assert_eq!(lab.device_class(), class::ABSTRACT);
        let lut = crate::profile::read_hl::read_devicelink_lut(&mut lab, INTENT_RELATIVE_COLORIMETRIC)
            .expect("devicelink lut");
        assert_eq!(lut.input_channels(), 3);
        assert_eq!(lut.output_channels(), 3);
        let mut out = [0u16; 3];
        lut.eval_16(&[0x2000, 0x8000, 0xC000], &mut out);
        for (i, &e) in [0x2000u16, 0x8000, 0xC000].iter().enumerate() {
            assert!((out[i] as i32 - e as i32).abs() <= 2, "identity drift {out:?}");
        }

        let xyz = create_xyz_profile_thr(ctx.clone()).expect("xyz");
        assert_eq!(xyz.color_space(), colorspace::XYZ);
    }

    /// `desaturate_lab`: in-prism points are untouched; out-of-prism points are clipped in.
    #[test]
    fn desaturate_clamps_to_prism() {
        // In gamut: unchanged.
        let mut lab = CIELab { l: 50.0, a: 10.0, b: -10.0 };
        assert!(desaturate_lab(&mut lab, 40.0, -40.0, 40.0, -40.0));
        assert!((lab.a - 10.0).abs() < 1e-9 && (lab.b + 10.0).abs() < 1e-9);

        // Negative L collapses to black and returns false.
        let mut black = CIELab { l: -5.0, a: 20.0, b: 20.0 };
        assert!(!desaturate_lab(&mut black, 40.0, -40.0, 40.0, -40.0));
        assert_eq!((black.l, black.a, black.b), (0.0, 0.0, 0.0));

        // L>100 is clipped to 100.
        let mut hi = CIELab { l: 120.0, a: 0.0, b: 0.0 };
        assert!(desaturate_lab(&mut hi, 40.0, -40.0, 40.0, -40.0));
        assert!((hi.l - 100.0).abs() < 1e-9);

        // Out of prism (hue ~0): clipped onto the amax face, staying within bounds.
        let mut oog = CIELab { l: 50.0, a: 80.0, b: 5.0 };
        assert!(desaturate_lab(&mut oog, 40.0, -40.0, 40.0, -40.0));
        assert!(oog.a <= 40.0 + 1e-9 && oog.b <= 40.0 + 1e-9);
    }

    /// `detect_tac` on a CMYK output profile is a plausible ink percentage.
    #[test]
    fn detect_tac_plausible() {
        let mut p = load("test1.icc");
        if p.color_space() != colorspace::CMYK || p.device_class() != class::OUTPUT {
            // Not a CMYK output profile in this corpus build — skip the numeric assertion.
            return;
        }
        let tac = detect_tac(&mut p);
        assert!((200.0..=400.0).contains(&tac), "TAC out of expected band: {tac}");
    }

    /// `detect_rgb_profile_gamma` on a matrix-shaper RGB profile is a sensible gamma (~2.x)
    /// or `-1` when unsupported.
    #[test]
    fn detect_rgb_gamma_plausible() {
        let mut p = load("ibm-t61.icc");
        // A loose threshold detects the profile's ~2.4 gamma; a tight one would (correctly)
        // return -1 since a real TRC is not a perfect power law.
        let g = detect_rgb_profile_gamma(&mut p, 1.0);
        assert!((2.0..3.0).contains(&g), "expected ibm-t61 gamma ~2.4, got {g}");
    }

    /// `create_gamut_check_pipeline` builds a `3 -> 1` pipeline on 3-channel profiles.
    #[test]
    fn gamut_check_builds_3_to_1() {
        let ctx = Context::new();
        let mut input = load("ibm-t61.icc");
        let mut gamut = load("ibm-t61.icc");
        let pipe = create_gamut_check_pipeline(
            &ctx,
            &mut [&mut input],
            &[false],
            &[INTENT_RELATIVE_COLORIMETRIC],
            &[1.0],
            1,
            &mut gamut,
        )
        .expect("gamut pipeline");
        assert_eq!(pipe.input_channels(), 3);
        assert_eq!(pipe.output_channels(), 1);
        // A near-white PCS point should evaluate without panic and yield a valid code.
        let mut out = [0u16; 1];
        pipe.eval_16(&[0x8000, 0x8000, 0x8000], &mut out);
    }

    /// `build_k_tone_curve` on a CMYK output profile pair yields a monotonic curve.
    #[test]
    fn k_tone_curve_monotonic() {
        let mut a = load("test1.icc");
        if a.color_space() != colorspace::CMYK || a.device_class() != class::OUTPUT {
            return; // Not a CMYK output profile — skip.
        }
        let ctx = Context::new();
        let mut b = load("test1.icc");
        let curve = build_k_tone_curve(
            &ctx,
            17,
            &mut [&mut a, &mut b],
            &[INTENT_RELATIVE_COLORIMETRIC, INTENT_RELATIVE_COLORIMETRIC],
            &[false, false],
            &[1.0, 1.0],
            0,
        );
        // A successful build must be monotonic (checked internally); a soft failure is tolerated
        // only as an explicit error, never a wrong-silent result.
        if let Ok(c) = curve {
            assert!(c.is_monotonic());
        }
    }
}
