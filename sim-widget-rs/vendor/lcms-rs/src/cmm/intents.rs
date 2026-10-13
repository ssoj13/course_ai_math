//! Rendering intents and profile linking (`cmscnvrt.c`).
//!
//! This is the port of Little-CMS's intent engine: it chains a list of profiles into a single
//! [`Pipeline`] according to per-profile rendering intents, black-point compensation (BPC) and
//! adaptation states. [`link_profiles`] (`_cmsLinkProfiles`) is the entry point used by
//! [`crate::cmm::transform`]; it dispatches to a handler chosen by the first intent in the chain.
//!
//! Supported handlers:
//! - [`default_icc_intents`] (`DefaultICCintents`) — the four ICC intents (perceptual, relative
//!   colorimetric, saturation, ICC-absolute colorimetric), including absolute-colorimetric
//!   chromatic adaptation ([`compute_absolute_intent`]) and BPC ([`compute_black_point_compensation`]).
//!   Fully ported.
//! - [`black_preserving_k_only_intents`] / [`black_preserving_k_plane_intents`]
//!   (`BlackPreservingKOnlyIntents` / `BlackPreservingKPlaneIntents`) — the non-ICC
//!   black-preserving intents. The profile-shape checks and the fall-back to
//!   [`default_icc_intents`] for non-CMYK chains, plus the genuine CMYK→CMYK preserving CLUT
//!   build (`_cmsBuildKToneCurve` + `cmsDetectTAC` from [`crate::cmm::gamut`] and the
//!   `BlackPreservingGrayOnlySampler` / `BlackPreservingSampler` samplers) are ported in full.

use crate::cmm::blackpoint::{detect_black_point, detect_destination_black_point};
use crate::cmm::gamut::{build_k_tone_curve, detect_tac};
use crate::cmm::optimize::{FLAGS_NOCACHE, FLAGS_NOOPTIMIZE};
use crate::cmm::transform::{create_transform, Transform};
use crate::color::d50_xyz;
use crate::color::gamma::ToneCurve;
use crate::color::pcs::{
    cms_channels_of_colorspace, cms_delta_e, cms_xyy_to_xyz, cms_xyz_to_xyy,
    reasonable_gridpoints_by_colorspace,
};
use crate::color::wtpnt::{adaptation_matrix, temp_from_white_point, white_point_from_temp};
use crate::context::Context;
use crate::error::{ErrorCode, Result};
use crate::math::mat3::{Mat3, Vec3};
use crate::pipeline::{Pipeline, StageLoc};
use crate::pipeline::stage::Stage;
use crate::profile::read_hl::{
    read_chad, read_devicelink_lut, read_input_lut, read_media_white_point,
    read_output_lut, INTENT_ABSOLUTE_COLORIMETRIC, INTENT_PERCEPTUAL,
    INTENT_RELATIVE_COLORIMETRIC, INTENT_SATURATION,
};
use crate::profile::Profile;
use crate::types::color::{CIELab, CIEXYZ};
use crate::types::format::{FormatBuilder, PixelFormat};
use crate::types::num::quick_saturate_word;
use crate::types::sig::{class, colorspace, Signature};
use crate::virt::create_lab4_profile_thr;
use crate::cms_err;

// -------------------------------------------------------------------------------------------
// Intent codes and flags (lcms2.h) not already exported by profile::read_hl
// -------------------------------------------------------------------------------------------

/// `INTENT_PRESERVE_K_ONLY_PERCEPTUAL`.
pub const INTENT_PRESERVE_K_ONLY_PERCEPTUAL: u32 = 10;
/// `INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC`.
pub const INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC: u32 = 11;
/// `INTENT_PRESERVE_K_ONLY_SATURATION`.
pub const INTENT_PRESERVE_K_ONLY_SATURATION: u32 = 12;
/// `INTENT_PRESERVE_K_PLANE_PERCEPTUAL`.
pub const INTENT_PRESERVE_K_PLANE_PERCEPTUAL: u32 = 13;
/// `INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC`.
pub const INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC: u32 = 14;
/// `INTENT_PRESERVE_K_PLANE_SATURATION`.
pub const INTENT_PRESERVE_K_PLANE_SATURATION: u32 = 15;

/// `cmsFLAGS_NONEGATIVES` — clip negative device values after the last profile.
pub const FLAGS_NONEGATIVES: u32 = 0x8000;

/// The XYZ encoding maximum (`MAX_ENCODEABLE_XYZ`), reused from the packing layer.
use crate::cmm::pack::MAX_ENCODEABLE_XYZ;

// -------------------------------------------------------------------------------------------
// Intent registry (SearchIntent)
// -------------------------------------------------------------------------------------------

/// Signature of a built-in intent handler (`cmsIntentFn`).
type IntentFn = fn(
    &Context,
    &mut [&mut Profile],
    &[u32],
    &[bool],
    &[f64],
    u32,
) -> Result<Pipeline>;

/// `DefaultIntents[]` (`cmscnvrt.c:82-94`) — the ten built-in intents, in C's declaration order.
///
/// The description strings are part of the public API: `cmsGetSupportedIntents` hands them to
/// callers building an intent picker, so they are transcribed verbatim from the C table rather
/// than reworded.
static DEFAULT_INTENTS: &[(u32, &str, IntentFn)] = &[
    (INTENT_PERCEPTUAL, "Perceptual", default_icc_intents),
    (INTENT_RELATIVE_COLORIMETRIC, "Relative colorimetric", default_icc_intents),
    (INTENT_SATURATION, "Saturation", default_icc_intents),
    (INTENT_ABSOLUTE_COLORIMETRIC, "Absolute colorimetric", default_icc_intents),
    (
        INTENT_PRESERVE_K_ONLY_PERCEPTUAL,
        "Perceptual preserving black ink",
        black_preserving_k_only_intents,
    ),
    (
        INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC,
        "Relative colorimetric preserving black ink",
        black_preserving_k_only_intents,
    ),
    (
        INTENT_PRESERVE_K_ONLY_SATURATION,
        "Saturation preserving black ink",
        black_preserving_k_only_intents,
    ),
    (
        INTENT_PRESERVE_K_PLANE_PERCEPTUAL,
        "Perceptual preserving black plane",
        black_preserving_k_plane_intents,
    ),
    (
        INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC,
        "Relative colorimetric preserving black plane",
        black_preserving_k_plane_intents,
    ),
    (
        INTENT_PRESERVE_K_PLANE_SATURATION,
        "Saturation preserving black plane",
        black_preserving_k_plane_intents,
    ),
];

/// `SearchIntent` — resolve an intent code to its handler.
///
/// Plugin-registered intents (`_cmsRegisterRenderingIntentPlugin`) are searched first —
/// newest first — then the ten built-in defaults, matching lcms's `SearchIntent` walk of the
/// context intent list ahead of `DefaultIntents`.
///
/// Note the asymmetry with [`get_supported_intents`], which is deliberate and matches C:
/// *lookup* consults plugins first (so they can override a built-in), *enumeration* lists the
/// built-ins first.
fn search_intent(ctx: Option<&Context>, intent: u32) -> Option<IntentFn> {
    if let Some(link) = crate::plugin::find_intent(ctx, intent) {
        return Some(link);
    }
    DEFAULT_INTENTS.iter().find(|(code, _, _)| *code == intent).map(|(_, _, f)| *f)
}

/// `cmsGetSupportedIntents` / `cmsGetSupportedIntentsTHR` (`cmscnvrt.c:1168-1205`).
///
/// Returns every selectable intent as `(code, description)`: the ten built-ins first, then the
/// plugin-registered ones — C's emission order, which is the reverse of the lookup order used
/// by [`search_intent`].
///
/// C takes a caller-sized buffer and returns the total count so a short buffer still reports
/// how many intents exist; an owned `Vec` carries the same information without the two-call
/// dance, so `nMax`/`Codes`/`Descriptions` have no analogue here.
pub fn get_supported_intents() -> Vec<(u32, String)> {
    get_supported_intents_for(None)
}

/// `cmsGetSupportedIntentsTHR` (`cmscnvrt.c:1168-1180`) — [`get_supported_intents`] for the
/// intents visible to `ctx`, which are its own if it owns a plugin chunk and Context 0's
/// otherwise.
pub fn get_supported_intents_thr(ctx: &Context) -> Vec<(u32, String)> {
    get_supported_intents_for(Some(ctx))
}

/// Shared body of the plain and `*THR` enumerations, so the two cannot drift apart.
fn get_supported_intents_for(ctx: Option<&Context>) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> =
        DEFAULT_INTENTS.iter().map(|(code, desc, _)| (*code, (*desc).to_string())).collect();
    out.extend(crate::plugin::intent_descriptions(ctx));
    out
}

// -------------------------------------------------------------------------------------------
// Matrix / offset helpers (ComputeBlackPointCompensation, absolute intent, layers)
// -------------------------------------------------------------------------------------------

/// Flatten a [`Mat3`] to a row-major `[f64; 9]` for [`Stage::alloc_matrix`].
fn mat3_row_major(m: &Mat3) -> [f64; 9] {
    let mut out = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            out[r * 3 + c] = m.0[r].0[c];
        }
    }
    out
}

/// A diagonal 3×3 scaling matrix.
fn diag(sx: f64, sy: f64, sz: f64) -> Mat3 {
    Mat3([
        Vec3([sx, 0.0, 0.0]),
        Vec3([0.0, sy, 0.0]),
        Vec3([0.0, 0.0, sz]),
    ])
}

/// `ComputeBlackPointCompensation` — linear XYZ scaling mapping `bp_in`→`bp_out` while keeping
/// D50 fixed, returned as a matrix + offset (the offset is *not* yet divided by the XYZ
/// encoding maximum; [`compute_conversion`] does that, matching the C ordering).
fn compute_black_point_compensation(bp_in: &CIEXYZ, bp_out: &CIEXYZ) -> (Mat3, Vec3) {
    let d50 = d50_xyz();

    let tx = bp_in.x - d50.x;
    let ty = bp_in.y - d50.y;
    let tz = bp_in.z - d50.z;

    let ax = (bp_out.x - d50.x) / tx;
    let ay = (bp_out.y - d50.y) / ty;
    let az = (bp_out.z - d50.z) / tz;

    let bx = -d50.x * (bp_out.x - bp_in.x) / tx;
    let by = -d50.y * (bp_out.y - bp_in.y) / ty;
    let bz = -d50.z * (bp_out.z - bp_in.z) / tz;

    (diag(ax, ay, az), Vec3([bx, by, bz]))
}

/// `CHAD2Temp` — approximate a blackbody temperature from a chromatic-adaptation matrix.
///
/// Returns `0.0` if the matrix is singular (the C `return FALSE`) and `-1.0` if the resulting
/// chromaticity has no valid correlated colour temperature.
fn chad2temp(chad: &Mat3) -> f64 {
    let inv = match chad.inverse() {
        Some(m) => m,
        None => return 0.0, // C returns FALSE (== 0.0) here.
    };

    let d50 = d50_xyz();
    let dest = inv.eval(Vec3([d50.x, d50.y, d50.z]));
    let dest_xyz = CIEXYZ { x: dest.0[0], y: dest.0[1], z: dest.0[2] };
    let chromaticity = cms_xyz_to_xyy(&dest_xyz);

    temp_from_white_point(&chromaticity).unwrap_or(-1.0)
}

/// `Temp2CHAD` — a chromatic-adaptation matrix from an output white at `temp` K to D50.
fn temp2chad(temp: f64) -> Option<Mat3> {
    let chromaticity = white_point_from_temp(temp).ok()?;
    let white = cms_xyy_to_xyz(&chromaticity);
    adaptation_matrix(None, &white, &d50_xyz())
}

/// `ComputeAbsoluteIntent` — join relative→absolute→relative scalings into a single 3×3 matrix,
/// honouring the observer adaptation state (`1.0` = fully adapted V4 default; `0.0` = undo
/// chromatic adaptation; intermediate = mixed CHAD temperature).
fn compute_absolute_intent(
    adaptation: f64,
    wp_in: &CIEXYZ,
    chad_in: &Mat3,
    wp_out: &CIEXYZ,
    chad_out: &Mat3,
) -> Option<Mat3> {
    let scale = diag(wp_in.x / wp_out.x, wp_in.y / wp_out.y, wp_in.z / wp_out.z);

    if adaptation == 1.0 {
        // Fully adapted observer: keep chromatic adaptation (standard V4 behaviour).
        return Some(scale);
    }

    if adaptation == 0.0 {
        // Observer not adapted: undo the chromatic adaptation.
        // m2 = chad_out * scale  (C's dead `_cmsMAT3per(m, &m2, chad_out)` is skipped — it is
        // immediately overwritten by the inverse path below).
        let m2 = Mat3::mul(chad_out, &scale);
        let m4 = chad_in.inverse()?;
        return Some(Mat3::mul(&m2, &m4));
    }

    // Incomplete adaptation: mix the source/destination temperatures.
    let m2 = chad_in.inverse()?;
    let m3 = Mat3::mul(&m2, &scale); // input white -> D50 times abs. scaling

    let temp_src = chad2temp(chad_in);
    let temp_dest = chad2temp(chad_out);
    if temp_src < 0.0 || temp_dest < 0.0 {
        return None;
    }

    if scale.is_identity() && (temp_src - temp_dest).abs() < 0.01 {
        return Some(Mat3::identity());
    }

    let temp = (1.0 - adaptation) * temp_dest + adaptation * temp_src;
    let mixed_chad = temp2chad(temp)?;
    Some(Mat3::mul(&m3, &mixed_chad))
}

/// `IsEmptyLayer` — is the matrix/offset close enough to identity/zero to skip?
fn is_empty_layer(m: &Mat3, off: &Vec3) -> bool {
    let ident = Mat3::identity();
    let mut diff = 0.0;
    for r in 0..3 {
        for c in 0..3 {
            diff += (m.0[r].0[c] - ident.0[r].0[c]).abs();
        }
    }
    for k in 0..3 {
        diff += off.0[k].abs();
    }
    diff < 0.002
}

// -------------------------------------------------------------------------------------------
// Colorspace helpers
// -------------------------------------------------------------------------------------------

/// `ColorSpaceIsCompatible` — may space `a` connect to space `b`?
fn color_space_is_compatible(a: Signature, b: Signature) -> bool {
    if a == b {
        return true;
    }
    // MCH4 (4CLR) substitutes for CMYK.
    if a == colorspace::CLR4 && b == colorspace::CMYK {
        return true;
    }
    if a == colorspace::CMYK && b == colorspace::CLR4 {
        return true;
    }
    // XYZ and Lab are interconvertible.
    if a == colorspace::XYZ && b == colorspace::LAB {
        return true;
    }
    if a == colorspace::LAB && b == colorspace::XYZ {
        return true;
    }
    false
}

/// `AddConversion` — append a PCS-space conversion (matrix + Lab/XYZ bridges) to `result`.
///
/// `m`/`off` operate in XYZ space; the PCS-mismatch handling exactly mirrors the C switch.
fn add_conversion(
    result: &mut Pipeline,
    in_pcs: Signature,
    out_pcs: Signature,
    m: &Mat3,
    off: &Vec3,
) -> Result<()> {
    let m9 = mat3_row_major(m);
    let off3 = off.0;

    let insert = |result: &mut Pipeline, stage: Stage| -> Result<()> {
        if !result.insert_stage(StageLoc::End, stage) {
            return Err(cms_err!(ErrorCode::Internal, "pipeline insert failed in AddConversion"));
        }
        Ok(())
    };
    let matrix_stage = || -> Result<Stage> { Stage::alloc_matrix(3, 3, &m9, Some(&off3)) };

    match in_pcs {
        colorspace::XYZ => match out_pcs {
            colorspace::XYZ => {
                if !is_empty_layer(m, off) {
                    insert(result, matrix_stage()?)?;
                }
            }
            colorspace::LAB => {
                if !is_empty_layer(m, off) {
                    insert(result, matrix_stage()?)?;
                }
                insert(result, Stage::alloc_xyz2lab())?;
            }
            _ => return Err(cms_err!(ErrorCode::ColorspaceCheck, "colorspace mismatch")),
        },
        colorspace::LAB => match out_pcs {
            colorspace::XYZ => {
                insert(result, Stage::alloc_lab2xyz())?;
                if !is_empty_layer(m, off) {
                    insert(result, matrix_stage()?)?;
                }
            }
            colorspace::LAB => {
                if !is_empty_layer(m, off) {
                    insert(result, Stage::alloc_lab2xyz())?;
                    insert(result, matrix_stage()?)?;
                    insert(result, Stage::alloc_xyz2lab())?;
                }
            }
            _ => return Err(cms_err!(ErrorCode::ColorspaceCheck, "colorspace mismatch")),
        },
        // Non-PCS spaces: must be identical.
        _ => {
            if in_pcs != out_pcs {
                return Err(cms_err!(ErrorCode::ColorspaceCheck, "colorspace mismatch"));
            }
        }
    }
    Ok(())
}

/// `ComputeConversion` — the matrix + offset connecting profile `i-1` to profile `i`.
///
/// For absolute-colorimetric intents this is the chromatic-adaptation matrix; otherwise, when
/// BPC is requested, it is the black-point-compensation scaling. The offset is divided by the
/// XYZ encoding maximum on the way out (the pipeline works in encoded 0..1 XYZ).
fn compute_conversion(
    profiles: &mut [&mut Profile],
    i: usize,
    intent: u32,
    bpc: bool,
    adaptation: f64,
) -> Result<(Mat3, Vec3)> {
    let mut m = Mat3::identity();
    let mut off = Vec3([0.0, 0.0, 0.0]);

    if intent == INTENT_ABSOLUTE_COLORIMETRIC {
        let (left, right) = profiles.split_at_mut(i);
        let prev = &mut *left[i - 1];
        let wp_in = read_media_white_point(prev)?;
        let chad_in = read_chad(prev)?;
        let cur = &mut *right[0];
        let wp_out = read_media_white_point(cur)?;
        let chad_out = read_chad(cur)?;

        m = compute_absolute_intent(adaptation, &wp_in, &chad_in, &wp_out, &chad_out)
            .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "absolute intent is singular"))?;
    } else if bpc {
        let (left, right) = profiles.split_at_mut(i);
        let prev = &mut *left[i - 1];
        let bp_in = detect_black_point(prev, intent, 0).unwrap_or_default();
        let cur = &mut *right[0];
        let bp_out = detect_destination_black_point(cur, intent, 0).unwrap_or_default();

        // If black points are equal, do nothing.
        if bp_in.x != bp_out.x || bp_in.y != bp_out.y || bp_in.z != bp_out.z {
            let (mm, oo) = compute_black_point_compensation(&bp_in, &bp_out);
            m = mm;
            off = oo;
        }
    }

    for k in 0..3 {
        off.0[k] /= MAX_ENCODEABLE_XYZ;
    }

    Ok((m, off))
}

// -------------------------------------------------------------------------------------------
// DefaultICCintents
// -------------------------------------------------------------------------------------------

/// `DefaultICCintents` — the default handler for the four ICC-style intents.
///
/// Walks the profile chain, reading each profile's device/PCS LUT in the correct direction,
/// inserting PCS conversions (absolute CHAD / BPC) between links, and concatenating everything
/// into one [`Pipeline`]. Ported 1:1 from the C control flow.
pub fn default_icc_intents(
    _ctx: &Context,
    profiles: &mut [&mut Profile],
    intents: &[u32],
    bpc: &[bool],
    adaptation: &[f64],
    flags: u32,
) -> Result<Pipeline> {
    let n_profiles = profiles.len();
    if n_profiles == 0 {
        return Err(cms_err!(ErrorCode::Null, "no profiles to link"));
    }

    let mut result = Pipeline::alloc(0, 0)?;

    let mut current_color_space = profiles[0].color_space();
    // `color_space_out` tracks the exit space of the last processed profile.
    let mut color_space_out = colorspace::LAB;

    for i in 0..n_profiles {
        let intent = intents[i];

        // --- Header inspection + LUT read, in a scope that ends the &mut borrow of profiles[i]. ---
        let class_sig = profiles[i].device_class();
        let is_devicelink = class_sig == class::LINK || class_sig == class::ABSTRACT;

        let is_input = if i == 0 && !is_devicelink {
            true
        } else {
            current_color_space != colorspace::XYZ && current_color_space != colorspace::LAB
        };

        let (color_space_in, cs_out) = if is_input || is_devicelink {
            (profiles[i].color_space(), profiles[i].pcs())
        } else {
            (profiles[i].pcs(), profiles[i].color_space())
        };

        if !color_space_is_compatible(color_space_in, current_color_space) {
            return Err(cms_err!(ErrorCode::ColorspaceCheck, "ColorSpace mismatch"));
        }

        // Read the appropriate LUT and, where needed, the connecting conversion.
        let lut: Pipeline = if is_devicelink
            || (class_sig == class::NAMED_COLOR && n_profiles == 1)
        {
            let lut = read_devicelink_lut(&mut *profiles[i], intent)?;

            // Abstract profiles beyond the first apply a conversion; others use identity.
            let (m, off) = if class_sig == class::ABSTRACT && i > 0 {
                compute_conversion(profiles, i, intent, bpc[i], adaptation[i])?
            } else {
                (Mat3::identity(), Vec3([0.0, 0.0, 0.0]))
            };
            add_conversion(&mut result, current_color_space, color_space_in, &m, &off)?;
            lut
        } else if is_input {
            // Non-PCS connection: read input LUT, no conversion added.
            read_input_lut(&mut *profiles[i], intent)?
        } else {
            // PCS connection: intent may apply here.
            let lut = read_output_lut(&mut *profiles[i], intent)?;
            let (m, off) = compute_conversion(profiles, i, intent, bpc[i], adaptation[i])?;
            add_conversion(&mut result, current_color_space, color_space_in, &m, &off)?;
            lut
        };

        if !result.cat(&lut) {
            return Err(cms_err!(ErrorCode::Internal, "cmsPipelineCat failed"));
        }

        current_color_space = cs_out;
        color_space_out = cs_out;
    }

    // Optional clip of negative device values.
    if flags & FLAGS_NONEGATIVES != 0
        && (color_space_out == colorspace::GRAY
            || color_space_out == colorspace::RGB
            || color_space_out == colorspace::CMYK)
    {
        let clip = Stage::clip_negatives(cms_channels_of_colorspace(color_space_out) as u32);
        if !result.insert_stage(StageLoc::End, clip) {
            return Err(cms_err!(ErrorCode::Internal, "clip-negatives insert failed"));
        }
    }

    if cms_channels_of_colorspace(color_space_out) != result.output_channels() as i32 {
        return Err(cms_err!(ErrorCode::NotSuitable, "output channel count mismatch"));
    }

    Ok(result)
}

// -------------------------------------------------------------------------------------------
// Black-preserving intents
// -------------------------------------------------------------------------------------------

/// `TranslateNonICCIntents` — map a black-preserving intent to its ICC base intent.
fn translate_non_icc_intents(intent: u32) -> u32 {
    match intent {
        INTENT_PRESERVE_K_ONLY_PERCEPTUAL | INTENT_PRESERVE_K_PLANE_PERCEPTUAL => {
            INTENT_PERCEPTUAL
        }
        INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC
        | INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC => INTENT_RELATIVE_COLORIMETRIC,
        INTENT_PRESERVE_K_ONLY_SATURATION | INTENT_PRESERVE_K_PLANE_SATURATION => {
            INTENT_SATURATION
        }
        other => other,
    }
}

/// `is_cmyk_devicelink` — a CMYK→CMYK devicelink profile.
fn is_cmyk_devicelink(prof: &Profile) -> bool {
    prof.device_class() == class::LINK && prof.color_space() == colorspace::CMYK
}

/// Shared prologue for the two black-preserving handlers: translate intents, trim trailing
/// CMYK devicelinks and decide whether the chain qualifies for black preservation. Returns
/// `Ok(Some(pipeline))` when the chain does *not* qualify (already handled by the fall-back to
/// [`default_icc_intents`]), or `Ok(None)` with the resolved intents + last-profile position
/// when it does qualify (and the CMYK-preserving build must run).
fn k_preserving_prologue(
    ctx: &Context,
    profiles: &mut [&mut Profile],
    intents: &[u32],
    bpc: &[bool],
    adaptation: &[f64],
    flags: u32,
) -> Result<(Vec<u32>, usize, Option<Pipeline>)> {
    let n_profiles = profiles.len();
    if !(1..=255).contains(&n_profiles) {
        return Err(cms_err!(ErrorCode::Range, "1..255 profiles expected"));
    }

    let icc_intents: Vec<u32> = intents.iter().map(|&x| translate_non_icc_intents(x)).collect();

    // Trim trailing CMYK->CMYK devicelinks.
    let mut last = n_profiles - 1;
    while is_cmyk_devicelink(profiles[last]) {
        if last < 2 {
            break;
        }
        last -= 1;
    }

    // Qualification: first profile CMYK, last-preservation profile CMYK or an output class.
    let first_is_cmyk = profiles[0].color_space() == colorspace::CMYK;
    let last_ok = profiles[last].color_space() == colorspace::CMYK
        || profiles[last].device_class() == class::OUTPUT;

    if !(first_is_cmyk && last_ok) {
        let lut = default_icc_intents(ctx, profiles, &icc_intents, bpc, adaptation, flags)?;
        return Ok((icc_intents, last, Some(lut)));
    }

    Ok((icc_intents, last, None))
}

/// `PT_LAB` (`lcms2.h`) — colour-space code of `TYPE_Lab_*`.
const PT_LAB: u32 = 10;

/// `CHANNELS_SH(4) | BYTES_SH(2)` — anonymous 16-bit CMYK device format.
fn fmt_cmyk_16() -> PixelFormat {
    FormatBuilder::new().channels(4).bytes(2).build()
}
/// `TYPE_Lab_DBL` — three `f64` `cmsCIELab` samples.
fn fmt_lab_dbl() -> PixelFormat {
    FormatBuilder::new().float(true).colorspace(PT_LAB).channels(3).bytes(0).build()
}
/// `FLOAT_SH(1) | CHANNELS_SH(4) | BYTES_SH(4)` — anonymous 4-channel float (CMYK, 0..1).
fn fmt_cmyk_flt() -> PixelFormat {
    FormatBuilder::new().float(true).channels(4).bytes(4).build()
}
/// `FLOAT_SH(1) | CHANNELS_SH(3) | BYTES_SH(4)` — anonymous 3-channel float (Lab, 0..1 domain).
fn fmt_lab3_flt() -> PixelFormat {
    FormatBuilder::new().float(true).channels(3).bytes(4).build()
}

/// Read a native-endian `f64` at byte offset `o`.
fn rd_f64(b: &[u8], o: usize) -> f64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    f64::from_ne_bytes(a)
}

/// `for (i = lastProfilePos + 1; i < nProfiles; i++)` — concatenate the trailing devicelinks
/// that the preservation build skipped over onto `result`.
fn append_trailing_devicelinks(
    result: &mut Pipeline,
    profiles: &mut [&mut Profile],
    icc_intents: &[u32],
    last: usize,
) -> Result<()> {
    for i in (last + 1)..profiles.len() {
        let devlink = read_devicelink_lut(&mut *profiles[i], icc_intents[i])?;
        if !result.cat(&devlink) {
            return Err(cms_err!(ErrorCode::Internal, "cmsPipelineCat failed"));
        }
    }
    Ok(())
}

/// `BlackPreservingGrayOnlySampler` — keep black-only input as black-only output.
fn black_preserving_gray_only_sampler(
    in_: &[u16],
    out: &mut [u16],
    cmyk2cmyk: &Pipeline,
    k_tone: &ToneCurve,
) -> bool {
    // If going across black only, keep black only (TAC does not apply — it is black ink).
    if in_[0] == 0 && in_[1] == 0 && in_[2] == 0 {
        out[0] = 0;
        out[1] = 0;
        out[2] = 0;
        out[3] = k_tone.eval_u16(in_[3]);
        return true;
    }

    // Keep the normal transform for other colours.
    cmyk2cmyk.eval_16(in_, out);
    true
}

/// `BlackPreservingKOnlyIntents` — preserve black ink when it is the only ink used.
///
/// Non-CMYK chains fall back to [`default_icc_intents`]. For qualifying CMYK→CMYK chains this
/// builds a `4→4` CLUT whose sampler routes black-only input through the `K→K'` tone curve
/// ([`build_k_tone_curve`]) and everything else through the plain ICC transform, then appends any
/// trailing CMYK devicelinks.
pub fn black_preserving_k_only_intents(
    ctx: &Context,
    profiles: &mut [&mut Profile],
    intents: &[u32],
    bpc: &[bool],
    adaptation: &[f64],
    flags: u32,
) -> Result<Pipeline> {
    let (icc_intents, last, fallback) =
        k_preserving_prologue(ctx, profiles, intents, bpc, adaptation, flags)?;
    if let Some(lut) = fallback {
        return Ok(lut);
    }

    let preservation_count = last + 1;

    // A LUT holding the normal ICC transform over the preservation profiles.
    let cmyk2cmyk = {
        let (head, _) = profiles.split_at_mut(preservation_count);
        default_icc_intents(
            ctx,
            head,
            &icc_intents[..preservation_count],
            &bpc[..preservation_count],
            &adaptation[..preservation_count],
            flags,
        )?
    };

    // The black-to-black tone curve.
    let k_tone = {
        let (head, _) = profiles.split_at_mut(preservation_count);
        build_k_tone_curve(
            ctx,
            4096,
            head,
            &icc_intents[..preservation_count],
            &bpc[..preservation_count],
            &adaptation[..preservation_count],
            flags,
        )?
    };

    let mut result = Pipeline::alloc(4, 4)?;

    let n_grid_points = reasonable_gridpoints_by_colorspace(colorspace::CMYK, flags);
    let mut clut = Stage::alloc_clut_16bit(n_grid_points, 4, 4, None)?;

    // Sample it — no pre/post linearization this time.
    let ok = clut.sample_clut_16(
        |in_, out| black_preserving_gray_only_sampler(in_, out, &cmyk2cmyk, &k_tone),
        0,
    );
    if !ok {
        return Err(cms_err!(ErrorCode::Internal, "K-only CLUT sampling failed"));
    }
    if !result.insert_stage(StageLoc::Begin, clut) {
        return Err(cms_err!(ErrorCode::Internal, "K-only pipeline assembly failed"));
    }

    // Insert possible devicelinks at the end.
    append_trailing_devicelinks(&mut result, profiles, &icc_intents, last)?;

    Ok(result)
}

/// `BlackPreservingSampler` — the K-plane sampler (16-bit CLUT, float internals).
///
/// Black-only input keeps black only. Otherwise the colorimetric CMYK is computed, and where its
/// K disagrees with the `K→K'` curve the CMY is recovered by reverse interpolation with K held
/// fixed, TAC-limited, and written out. `max_error` mirrors the C `bp->MaxError` diagnostic and
/// has no effect on the produced values.
#[allow(clippy::too_many_arguments)]
fn black_preserving_sampler(
    in_: &[u16],
    out: &mut [u16],
    cmyk2cmyk: &Pipeline,
    h_proof_output: &Transform,
    cmyk2lab: &Transform,
    k_tone: &ToneCurve,
    labk2cmyk: &Pipeline,
    max_tac: f64,
    max_error: &mut f64,
) -> bool {
    // 16 bits -> floating point.
    let mut inf = [0f32; 4];
    for i in 0..4 {
        inf[i] = in_[i] as f32 / 65535.0;
    }

    // Get the K across the tone curve.
    let mut labk = [0f32; 4];
    labk[3] = k_tone.eval_f32(inf[3]);

    // If going across black only, keep black only.
    if in_[0] == 0 && in_[1] == 0 && in_[2] == 0 {
        out[0] = 0;
        out[1] = 0;
        out[2] = 0;
        out[3] = quick_saturate_word(labk[3] as f64 * 65535.0);
        return true;
    }

    // Try the original transform.
    let mut outf = [0f32; 4];
    cmyk2cmyk.eval_float(&inf, &mut outf);

    // Store a copy of the floating-point result into 16-bit.
    for i in 0..4 {
        out[i] = quick_saturate_word(outf[i] as f64 * 65535.0);
    }

    // Maybe K is already ok (mostly at K=0).
    if (outf[3] - labk[3]).abs() < (3.0 / 65535.0) {
        return true;
    }

    // K differs; measure and keep the Lab (relative colorimetric) for later usage.
    let colorimetric_lab = proof_16_to_lab(h_proof_output, out);

    // Obtain the Lab of the output CMYK. After that we have Lab + K.
    let lab3 = cmyk_flt_to_lab3(cmyk2lab, &outf);
    labk[0] = lab3[0];
    labk[1] = lab3[1];
    labk[2] = lab3[2];

    // Obtain the corresponding CMY using reverse interpolation (K fixed in labk[3]).
    let mut recovered = [0f32; 4];
    if !labk2cmyk.eval_reverse_float(&labk, &mut recovered, Some(&outf)) {
        // Cannot find a suitable value, so use the colorimetric xform already in `out`.
        return true;
    }
    outf = recovered;

    // Make sure to pass through K (which is now fixed).
    outf[3] = labk[3];

    // Apply TAC if needed.
    let sum_cmy = outf[0] as f64 + outf[1] as f64 + outf[2] as f64;
    let sum_cmyk = sum_cmy + outf[3] as f64;
    let ratio = if sum_cmyk > max_tac {
        let r = 1.0 - ((sum_cmyk - max_tac) / sum_cmy);
        if r < 0.0 {
            0.0
        } else {
            r
        }
    } else {
        1.0
    };

    out[0] = quick_saturate_word(outf[0] as f64 * ratio * 65535.0); // C
    out[1] = quick_saturate_word(outf[1] as f64 * ratio * 65535.0); // M
    out[2] = quick_saturate_word(outf[2] as f64 * ratio * 65535.0); // Y
    out[3] = quick_saturate_word(outf[3] as f64 * 65535.0);

    // Estimate the error (diagnostic only; goes 16 bits -> Lab DBL).
    let black_preserving_lab = proof_16_to_lab(h_proof_output, out);
    let error = cms_delta_e(&colorimetric_lab, &black_preserving_lab);
    if error > *max_error {
        *max_error = error;
    }

    true
}

/// Transform a 16-bit CMYK slot to `cmsCIELab` (the proof-output measurement transform).
fn proof_16_to_lab(xform: &Transform, cmyk16: &[u16]) -> CIELab {
    let mut src = [0u8; 8];
    for i in 0..4 {
        src[i * 2..i * 2 + 2].copy_from_slice(&cmyk16[i].to_ne_bytes());
    }
    let mut dst = [0u8; 24];
    xform.do_transform(&src, &mut dst, 1);
    CIELab { l: rd_f64(&dst, 0), a: rd_f64(&dst, 8), b: rd_f64(&dst, 16) }
}

/// Transform a float CMYK to a raw 3-channel float Lab (0..1 domain).
fn cmyk_flt_to_lab3(xform: &Transform, cmyk: &[f32; 4]) -> [f32; 3] {
    let mut src = [0u8; 16];
    for i in 0..4 {
        src[i * 4..i * 4 + 4].copy_from_slice(&cmyk[i].to_ne_bytes());
    }
    let mut dst = [0u8; 12];
    xform.do_transform(&src, &mut dst, 1);
    let mut out = [0f32; 3];
    for i in 0..3 {
        out[i] = f32::from_ne_bytes([dst[i * 4], dst[i * 4 + 1], dst[i * 4 + 2], dst[i * 4 + 3]]);
    }
    out
}

/// `BlackPreservingKPlaneIntents` — preserve the whole black plane (K generation).
///
/// Non-CMYK chains fall back to [`default_icc_intents`]. For qualifying chains this builds a
/// `4→4` CLUT via [`black_preserving_sampler`], which uses the last profile's input LUT (searched
/// by reverse interpolation), its TAC ([`detect_tac`]), the `K→K'` curve, and two measurement
/// transforms to Lab, then appends any trailing CMYK devicelinks.
pub fn black_preserving_k_plane_intents(
    ctx: &Context,
    profiles: &mut [&mut Profile],
    intents: &[u32],
    bpc: &[bool],
    adaptation: &[f64],
    flags: u32,
) -> Result<Pipeline> {
    let (icc_intents, last, fallback) =
        k_preserving_prologue(ctx, profiles, intents, bpc, adaptation, flags)?;
    if let Some(lut) = fallback {
        return Ok(lut);
    }

    let preservation_count = last + 1;

    // The input LUT of the last profile (searched in reverse; it does black generation).
    let labk2cmyk = read_input_lut(&mut *profiles[last], INTENT_RELATIVE_COLORIMETRIC)?;

    // Total area coverage in the 0..1 domain.
    let max_tac = detect_tac(&mut *profiles[last]) / 100.0;
    if max_tac <= 0.0 {
        return Err(cms_err!(ErrorCode::NotSuitable, "K-plane: could not detect TAC"));
    }

    // A LUT holding the normal ICC transform over the preservation profiles.
    let cmyk2cmyk = {
        let (head, _) = profiles.split_at_mut(preservation_count);
        default_icc_intents(
            ctx,
            head,
            &icc_intents[..preservation_count],
            &bpc[..preservation_count],
            &adaptation[..preservation_count],
            flags,
        )?
    };

    // The black-to-black tone curve.
    let k_tone = {
        let (head, _) = profiles.split_at_mut(preservation_count);
        build_k_tone_curve(
            ctx,
            4096,
            head,
            &icc_intents[..preservation_count],
            &bpc[..preservation_count],
            &adaptation[..preservation_count],
            flags,
        )?
    };

    // Two measurement transforms from the last profile to Lab (16-bit and 0..1 float).
    let mut lab = create_lab4_profile_thr(ctx.clone(), None)?;
    let h_proof_output = create_transform(
        ctx,
        &mut *profiles[last],
        fmt_cmyk_16(),
        &mut lab,
        fmt_lab_dbl(),
        INTENT_RELATIVE_COLORIMETRIC,
        FLAGS_NOCACHE | FLAGS_NOOPTIMIZE,
    )?;
    let cmyk2lab = create_transform(
        ctx,
        &mut *profiles[last],
        fmt_cmyk_flt(),
        &mut lab,
        fmt_lab3_flt(),
        INTENT_RELATIVE_COLORIMETRIC,
        FLAGS_NOCACHE | FLAGS_NOOPTIMIZE,
    )?;

    // Diagnostic error accumulator (mirrors bp->MaxError; unused by the produced CLUT).
    let mut max_error = 0.0f64;

    let n_grid_points = reasonable_gridpoints_by_colorspace(colorspace::CMYK, flags);
    let mut clut = Stage::alloc_clut_16bit(n_grid_points, 4, 4, None)?;

    let ok = clut.sample_clut_16(
        |in_, out| {
            black_preserving_sampler(
                in_,
                out,
                &cmyk2cmyk,
                &h_proof_output,
                &cmyk2lab,
                &k_tone,
                &labk2cmyk,
                max_tac,
                &mut max_error,
            )
        },
        0,
    );
    if !ok {
        return Err(cms_err!(ErrorCode::Internal, "K-plane CLUT sampling failed"));
    }
    let _ = max_error; // diagnostic only

    let mut result = Pipeline::alloc(4, 4)?;
    if !result.insert_stage(StageLoc::Begin, clut) {
        return Err(cms_err!(ErrorCode::Internal, "K-plane pipeline assembly failed"));
    }

    // Insert possible devicelinks at the end.
    append_trailing_devicelinks(&mut result, profiles, &icc_intents, last)?;

    Ok(result)
}

// -------------------------------------------------------------------------------------------
// _cmsLinkProfiles
// -------------------------------------------------------------------------------------------

/// `_cmsLinkProfiles` — chain profiles into a single [`Pipeline`].
///
/// Applies the Adobe BPC rules (BPC never for absolute; forced on for V4 perceptual/saturation),
/// then dispatches to the handler for the first intent in the chain. `bpc` is taken mutably
/// because those rules rewrite it in place, exactly as the C code does.
pub fn link_profiles(
    ctx: &Context,
    profiles: &mut [&mut Profile],
    intents: &[u32],
    bpc: &mut [bool],
    adaptation: &[f64],
    flags: u32,
) -> Result<Pipeline> {
    let n_profiles = profiles.len();
    if n_profiles == 0 || n_profiles > 255 {
        return Err(cms_err!(ErrorCode::Range, "Couldn't link {n_profiles} profiles"));
    }

    for i in 0..n_profiles {
        // BPC does not apply to absolute colorimetric...
        if intents[i] == INTENT_ABSOLUTE_COLORIMETRIC {
            bpc[i] = false;
        }
        // ...and is forced on for V4 perceptual/saturation.
        if (intents[i] == INTENT_PERCEPTUAL || intents[i] == INTENT_SATURATION)
            && profiles[i].encoded_icc_version() >= 0x0400_0000
        {
            bpc[i] = true;
        }
    }

    let handler = search_intent(Some(ctx), intents[0])
        .ok_or_else(|| cms_err!(ErrorCode::UnknownExtension, "Unsupported intent {}", intents[0]))?;

    handler(ctx, profiles, intents, bpc, adaptation, flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cmsGetSupportedIntents` must list the ten built-ins, in C's declaration order
    /// (`cmscnvrt.c:82-94`), with the descriptions C publishes. Before this existed the crate
    /// had no built-in intent table at all - `plugin::intent_descriptions()` covers only
    /// plugin-registered intents, so an intent picker would have come up empty.
    #[test]
    fn supported_intents_lists_the_ten_builtins_in_c_order() {
        let got = get_supported_intents();
        assert_eq!(got.len(), 10, "registry is empty by default, so exactly the built-ins");

        let want = [
            (INTENT_PERCEPTUAL, "Perceptual"),
            (INTENT_RELATIVE_COLORIMETRIC, "Relative colorimetric"),
            (INTENT_SATURATION, "Saturation"),
            (INTENT_ABSOLUTE_COLORIMETRIC, "Absolute colorimetric"),
            (INTENT_PRESERVE_K_ONLY_PERCEPTUAL, "Perceptual preserving black ink"),
            (
                INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC,
                "Relative colorimetric preserving black ink",
            ),
            (INTENT_PRESERVE_K_ONLY_SATURATION, "Saturation preserving black ink"),
            (INTENT_PRESERVE_K_PLANE_PERCEPTUAL, "Perceptual preserving black plane"),
            (
                INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC,
                "Relative colorimetric preserving black plane",
            ),
            (INTENT_PRESERVE_K_PLANE_SATURATION, "Saturation preserving black plane"),
        ];
        for (i, (code, desc)) in want.iter().enumerate() {
            assert_eq!(got[i].0, *code, "intent {i} code");
            assert_eq!(got[i].1, *desc, "intent {i} description");
        }

        // Every enumerated intent must actually resolve to a handler.
        for (code, _) in &got {
            assert!(
                search_intent(None, *code).is_some(),
                "intent {code} enumerated but not dispatchable"
            );
        }
    }

    #[test]
    fn intent_registry_dispatch() {
        assert!(search_intent(None, INTENT_PERCEPTUAL).is_some());
        assert!(search_intent(None, INTENT_ABSOLUTE_COLORIMETRIC).is_some());
        assert!(search_intent(None, INTENT_PRESERVE_K_ONLY_PERCEPTUAL).is_some());
        assert!(search_intent(None, INTENT_PRESERVE_K_PLANE_SATURATION).is_some());
        assert!(search_intent(None, 999).is_none());
    }

    #[test]
    fn translate_intents() {
        assert_eq!(translate_non_icc_intents(INTENT_PRESERVE_K_ONLY_PERCEPTUAL), INTENT_PERCEPTUAL);
        assert_eq!(
            translate_non_icc_intents(INTENT_PRESERVE_K_PLANE_RELATIVE_COLORIMETRIC),
            INTENT_RELATIVE_COLORIMETRIC
        );
        assert_eq!(translate_non_icc_intents(INTENT_PERCEPTUAL), INTENT_PERCEPTUAL);
    }

    #[test]
    fn colorspace_compatibility() {
        assert!(color_space_is_compatible(colorspace::RGB, colorspace::RGB));
        assert!(color_space_is_compatible(colorspace::CLR4, colorspace::CMYK));
        assert!(color_space_is_compatible(colorspace::XYZ, colorspace::LAB));
        assert!(!color_space_is_compatible(colorspace::RGB, colorspace::CMYK));
    }

    #[test]
    fn empty_layer_detects_identity() {
        assert!(is_empty_layer(&Mat3::identity(), &Vec3([0.0, 0.0, 0.0])));
        assert!(!is_empty_layer(&diag(2.0, 1.0, 1.0), &Vec3([0.0, 0.0, 0.0])));
        assert!(!is_empty_layer(&Mat3::identity(), &Vec3([1.0, 0.0, 0.0])));
    }

    #[test]
    fn absolute_intent_fully_adapted_is_white_ratio() {
        let wp_in = CIEXYZ { x: 0.9, y: 1.0, z: 0.8 };
        let wp_out = CIEXYZ { x: 0.95, y: 1.0, z: 1.0 };
        let chad = Mat3::identity();
        let m = compute_absolute_intent(1.0, &wp_in, &chad, &wp_out, &chad).unwrap();
        assert!((m.0[0].0[0] - wp_in.x / wp_out.x).abs() < 1e-12);
        assert!((m.0[1].0[1] - wp_in.y / wp_out.y).abs() < 1e-12);
        assert!((m.0[2].0[2] - wp_in.z / wp_out.z).abs() < 1e-12);
    }

    #[test]
    fn bpc_maps_black_points() {
        let bp_in = CIEXYZ { x: 0.02, y: 0.02, z: 0.02 };
        let bp_out = CIEXYZ { x: 0.005, y: 0.005, z: 0.005 };
        let (m, off) = compute_black_point_compensation(&bp_in, &bp_out);
        // Applying the affine map to bp_in must reproduce bp_out.
        let v = m.eval(Vec3([bp_in.x, bp_in.y, bp_in.z]));
        assert!((v.0[0] + off.0[0] - bp_out.x).abs() < 1e-12);
        assert!((v.0[1] + off.0[1] - bp_out.y).abs() < 1e-12);
        assert!((v.0[2] + off.0[2] - bp_out.z).abs() < 1e-12);
    }

    use crate::test_profile as load;

    /// The black-preserving K-only intent now builds a real CMYK→CMYK preserving CLUT and keeps
    /// pure black (C=M=Y=0) as pure black on the output.
    #[test]
    fn k_only_intent_builds_and_preserves_black() {
        let ctx = Context::new();
        let mut a = load("test1.icc"); // CMYK printer
        let mut b = load("test2.icc"); // CMYK printer (output class)
        let pipe = black_preserving_k_only_intents(
            &ctx,
            &mut [&mut a, &mut b],
            &[INTENT_PRESERVE_K_ONLY_RELATIVE_COLORIMETRIC; 2],
            &[false, false],
            &[1.0, 1.0],
            0,
        )
        .expect("K-only preserving pipeline must build");

        assert_eq!(pipe.input_channels(), 4);
        assert_eq!(pipe.output_channels(), 4);

        // Pure black (only K) must stay pure black: C = M = Y = 0 on the output.
        let mut out = [0u16; 4];
        pipe.eval_16(&[0, 0, 0, 0x8000], &mut out);
        assert_eq!((out[0], out[1], out[2]), (0, 0, 0), "K-only must preserve pure black: {out:?}");
    }
}
