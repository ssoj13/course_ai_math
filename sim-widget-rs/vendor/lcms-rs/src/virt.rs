//! Virtual (built-in) profiles — a pure-Rust port of `cmsvirt.c`.
//!
//! These constructors synthesize ICC profiles entirely in memory (no file is read): the RGB /
//! gray / XYZ / Lab identity profiles, the sRGB and (experimental) OkLab spaces, the
//! linearization / ink-limiting device links, and the BCHSW and NULL abstract profiles. Every
//! constant, tag set, profile version and write order is transcribed verbatim from
//! Little-CMS `src/cmsvirt.c` so the emitted profiles are bit-for-bit faithful.
//!
//! # Fidelity notes (do not "clean up")
//! * Tag write order is preserved exactly (e.g. RGB colorants are written `rXYZ`, `bXYZ`,
//!   `gXYZ` — the odd Red/Blue/Green order of `cmsCreateRGBProfileTHR`).
//! * The transfer-function de-duplication of `cmsCreateRGBProfileTHR` (link `gTRC`/`bTRC` to
//!   `rTRC` when the caller passed the *same* curve object) is reproduced via pointer identity
//!   ([`std::ptr::eq`]) on the `&ToneCurve` arguments — the direct analogue of C's
//!   `TransferFunction[1] == TransferFunction[0]` pointer test.
//! * `cmsCreateLab2Profile` is written as **version 2.1** (not 4.4) and uses an identity CLUT
//!   (`_cmsStageAllocIdentityCLut`), whereas Lab4/XYZ use identity curves. This asymmetry is
//!   intentional and matches the reference.
//! * The chromatic-adaptation tag (`chad`) is emitted as a 9-element `s15Fixed16` array in
//!   **row-major** order (`v[0].n[0..2], v[1].n[0..2], v[2].n[0..2]`), matching the raw
//!   `cmsMAT3` memory layout lcms hands to the `s15Fixed16Array` writer.
//!
//! # `cmsTransform2DeviceLink`
//! [`transform2devicelink`] converts a compiled [`Transform`] into a device-link [`Profile`]
//! using the reusable core here ([`is_pcs`], [`fix_color_spaces`], [`ALLOWED_LUT_TYPES`],
//! [`check_one`], [`find_combination`]) plus the [`Transform`] accessors
//! ([`Transform::context`]/[`sequence`](Transform::sequence)/white points/colorants).
//!
//! A transform whose pipeline starts with a named-colour element takes the
//! `CreateNamedColorDevicelink` branch instead (`cmsvirt.c:1216-1222`), which is how several
//! named-colour databases get merged into one profile. Like C, it REBINDS the caller's transform
//! to `TYPE_NAMED_COLOR_INDEX` (`cmsvirt.c:1107`) - hence `&mut Transform` - so the input side
//! becomes a 1-channel 16-bit index rather than a colour, and each spot colour's device colorant
//! is recomputed by running its index through the transform.

use std::ptr;

use crate::cms_err;
use crate::cmm::optimize::{optimize_pipeline_thr, FLAGS_FORCE_CLUT, FLAGS_NOWHITEONWHITEFIXUP};
use crate::cmm::transform::Transform;
use crate::color::gamma::ToneCurve;
use crate::color::pcs::{
    cms_channels_of, cms_channels_of_colorspace, cms_float_to_lab_encoded,
    cms_lab_encoded_to_float, cms_lab_to_lch, cms_lab_to_xyz, cms_lch_to_lab, cms_xyy_to_xyz,
    cms_xyz_to_lab, lcms_colorspace,
};
use crate::color::wtpnt::{
    adaptation_matrix, build_rgb2xyz_transfer_matrix, d50_xyy, white_point_from_temp,
};
use crate::color::d50_xyz;
use crate::context::Context;
use crate::error::{ErrorCode, Result};
use crate::math::mat3::Mat3;
use crate::named::dict::Seq;
use crate::named::mlu::Mlu;
use crate::pipeline::stage::{STAGE_CLUT, STAGE_CURVE_SET, STAGE_MATRIX, STAGE_NAMED_COLOR};
use crate::pipeline::{Pipeline, Stage, StageLoc};
use crate::profile::read_hl::write_profile_sequence;
use crate::profile::Profile;
use crate::tags::descriptor::tag_sig;
use crate::tags::handler::type_sig;
use crate::tags::TagValue;
use crate::types::color::{CIEXYZ, CIExyY, CIExyYTriple};
use crate::types::format::{FormatBuilder, PixelFormat};
use crate::types::num::quick_saturate_word;
use crate::types::sig::{class, colorspace, Signature};

// ---------------------------------------------------------------------------------------
// Rendering-intent constants (lcms2.h). Only the two used by cmsvirt.c are needed here.
// ---------------------------------------------------------------------------------------

/// `INTENT_PERCEPTUAL` (lcms2.h).
const INTENT_PERCEPTUAL: u32 = 0;
/// `INTENT_RELATIVE_COLORIMETRIC` (lcms2.h).
const INTENT_RELATIVE_COLORIMETRIC: u32 = 1;

// ---------------------------------------------------------------------------------------
// SetTextTags / SetSeqDescTag helpers
// ---------------------------------------------------------------------------------------

/// `SetTextTags` — write the profile-description and copyright MLUs (`en`/`US`).
///
/// Mirrors `cmsMLUsetWide`: the ASCII descriptions are stored as their UTF-16 code units, which
/// is exactly what lcms's `wchar_t` → `u16` collapse produces for the built-in strings.
fn set_text_tags(profile: &mut Profile, description: &str) -> Result<()> {
    let mut description_mlu = Mlu::new();
    let mut copyright_mlu = Mlu::new();

    let desc_wide: Vec<u16> = description.encode_utf16().collect();
    let copy_wide: Vec<u16> = "No copyright, use freely".encode_utf16().collect();

    description_mlu.set_wide(*b"en", *b"US", &desc_wide)?;
    copyright_mlu.set_wide(*b"en", *b"US", &copy_wide)?;

    profile.write_tag(tag_sig::PROFILE_DESCRIPTION, TagValue::Mlu(description_mlu))?;
    profile.write_tag(tag_sig::COPYRIGHT, TagValue::Mlu(copyright_mlu))?;
    Ok(())
}

/// `SetSeqDescTag` — write a one-entry profile-sequence-description tag (manufacturer
/// `"Little CMS"`, model `Model`, all other fields zero).
fn set_seq_desc_tag(profile: &mut Profile, model: &str) -> Result<()> {
    let mut seq = Seq::new(1)?;
    // seq[0] is default-zeroed (deviceMfg/deviceModel/attributes/technology == 0).
    let mut manufacturer = Mlu::new();
    let mut model_mlu = Mlu::new();
    // cmsNoLanguage / cmsNoCountry are the all-zero language/country codes.
    manufacturer.set_ascii([0, 0], [0, 0], "Little CMS")?;
    model_mlu.set_ascii([0, 0], [0, 0], model)?;
    seq.seq[0].manufacturer = Some(manufacturer);
    seq.seq[0].model = Some(model_mlu);

    profile.write_tag(tag_sig::PROFILE_SEQUENCE_DESC, TagValue::Seq(seq))?;
    Ok(())
}

/// Flatten a [`Mat3`] to the 9-element **row-major** `s15Fixed16` array lcms writes for `chad`.
fn mat3_to_s15f16(m: &Mat3) -> Vec<f64> {
    let mut v = Vec::with_capacity(9);
    for row in 0..3 {
        for col in 0..3 {
            v.push(m.0[row].0[col]);
        }
    }
    v
}

// ---------------------------------------------------------------------------------------
// cmsCreateRGBProfile[THR]
// ---------------------------------------------------------------------------------------

/// `cmsCreateRGBProfileTHR` — a Display/RGB/XYZ profile (v4.4) from white point, primaries and
/// transfer functions.
///
/// `transfer` is an array of three curve *references*; passing the **same** reference for more
/// than one channel triggers the `gTRC`/`bTRC` → `rTRC` link de-duplication, exactly like the C
/// pointer test. Any argument may be `None` (an incomplete profile, as the C accepts `NULL`).
pub fn create_rgb_profile_thr(
    context: Context,
    white_point: Option<&CIExyY>,
    primaries: Option<&CIExyYTriple>,
    transfer: Option<[&ToneCurve; 3]>,
) -> Result<Profile> {
    let mut icc = Profile::create_placeholder(context);

    icc.set_profile_version(4.4);
    icc.set_device_class(class::DISPLAY);
    icc.set_color_space(colorspace::RGB);
    icc.set_pcs(colorspace::XYZ);
    icc.set_header_rendering_intent(INTENT_PERCEPTUAL);

    // Implemented tags:
    //  1 desc, 2 wtpt(=D50), 3..5 r/g/bXYZ, 6..8 r/g/bTRC, 9 chad, 10 chrm.
    set_text_tags(&mut icc, "RGB built-in")?;

    if let Some(wp) = white_point {
        // MediaWhitePoint is always D50 (the profile is chromatically adapted to D50).
        icc.write_tag(tag_sig::MEDIA_WHITE_POINT, TagValue::Xyz(d50_xyz()))?;

        let white_point_xyz = cms_xyy_to_xyz(wp);
        let chad = adaptation_matrix(None, &white_point_xyz, &d50_xyz())
            .ok_or_else(|| cms_err!(ErrorCode::Internal, "cannot build chromatic adaptation"))?;

        // A V4 tag, but understood by most CMMs regardless of version.
        icc.write_tag(
            tag_sig::CHROMATIC_ADAPTATION,
            TagValue::S15Fixed16Array(mat3_to_s15f16(&chad)),
        )?;
    }

    if let (Some(wp), Some(prims)) = (white_point, primaries) {
        let max_white = CIExyY { x: wp.x, y: wp.y, yy: 1.0 };

        let colorants = build_rgb2xyz_transfer_matrix(&max_white, prims).ok_or_else(|| {
            cms_err!(ErrorCode::Internal, "cannot build RGB2XYZ transfer matrix")
        })?;

        // MColorants.v[row].n[col]; Red is column 0, Green column 1, Blue column 2.
        let red = CIEXYZ {
            x: colorants.0[0].0[0],
            y: colorants.0[1].0[0],
            z: colorants.0[2].0[0],
        };
        let green = CIEXYZ {
            x: colorants.0[0].0[1],
            y: colorants.0[1].0[1],
            z: colorants.0[2].0[1],
        };
        let blue = CIEXYZ {
            x: colorants.0[0].0[2],
            y: colorants.0[1].0[2],
            z: colorants.0[2].0[2],
        };

        // Note the C write order: Red, Blue, Green.
        icc.write_tag(tag_sig::RED_COLORANT, TagValue::Xyz(red))?;
        icc.write_tag(tag_sig::BLUE_COLORANT, TagValue::Xyz(blue))?;
        icc.write_tag(tag_sig::GREEN_COLORANT, TagValue::Xyz(green))?;
    }

    if let Some(tf) = transfer {
        // Tries to minimize space (Richard Hughes' idea): link identical curves.
        icc.write_tag(tag_sig::RED_TRC, TagValue::Curve(tf[0].clone()))?;

        if ptr::eq(tf[1], tf[0]) {
            icc.link_tag(tag_sig::GREEN_TRC, tag_sig::RED_TRC)?;
        } else {
            icc.write_tag(tag_sig::GREEN_TRC, TagValue::Curve(tf[1].clone()))?;
        }

        if ptr::eq(tf[2], tf[0]) {
            icc.link_tag(tag_sig::BLUE_TRC, tag_sig::RED_TRC)?;
        } else {
            icc.write_tag(tag_sig::BLUE_TRC, TagValue::Curve(tf[2].clone()))?;
        }
    }

    if let Some(prims) = primaries {
        icc.write_tag(tag_sig::CHROMATICITY, TagValue::Chromaticity(*prims))?;
    }

    Ok(icc)
}

/// `cmsCreateRGBProfile` — [`create_rgb_profile_thr`] on a fresh context (C's `NULL` context).
pub fn create_rgb_profile(
    white_point: Option<&CIExyY>,
    primaries: Option<&CIExyYTriple>,
    transfer: Option<[&ToneCurve; 3]>,
) -> Result<Profile> {
    create_rgb_profile_thr(Context::new(), white_point, primaries, transfer)
}

// ---------------------------------------------------------------------------------------
// cmsCreateGrayProfile[THR]
// ---------------------------------------------------------------------------------------

/// `cmsCreateGrayProfileTHR` — a Display/Gray/XYZ profile (v4.4) from white point + transfer.
pub fn create_gray_profile_thr(
    context: Context,
    white_point: Option<&CIExyY>,
    transfer: Option<&ToneCurve>,
) -> Result<Profile> {
    let mut icc = Profile::create_placeholder(context);

    icc.set_profile_version(4.4);
    icc.set_device_class(class::DISPLAY);
    icc.set_color_space(colorspace::GRAY);
    icc.set_pcs(colorspace::XYZ);
    icc.set_header_rendering_intent(INTENT_PERCEPTUAL);

    // Tags: 1 desc, 2 wtpt, 3 kTRC.
    set_text_tags(&mut icc, "gray built-in")?;

    if let Some(wp) = white_point {
        // Here the media white point is the actual (unadapted) white point.
        let tmp = cms_xyy_to_xyz(wp);
        icc.write_tag(tag_sig::MEDIA_WHITE_POINT, TagValue::Xyz(tmp))?;
    }

    if let Some(tf) = transfer {
        icc.write_tag(tag_sig::GRAY_TRC, TagValue::Curve(tf.clone()))?;
    }

    Ok(icc)
}

/// `cmsCreateGrayProfile` — [`create_gray_profile_thr`] on a fresh context.
pub fn create_gray_profile(
    white_point: Option<&CIExyY>,
    transfer: Option<&ToneCurve>,
) -> Result<Profile> {
    create_gray_profile_thr(Context::new(), white_point, transfer)
}

// ---------------------------------------------------------------------------------------
// cmsCreateLinearizationDeviceLink[THR]
// ---------------------------------------------------------------------------------------

/// `cmsCreateLinearizationDeviceLinkTHR` — a device link in `color_space` with one transfer
/// function per channel (a single prelinearization stage).
pub fn create_linearization_device_link_thr(
    context: Context,
    color_space: Signature,
    transfer: &[ToneCurve],
) -> Result<Profile> {
    let mut icc = Profile::create_placeholder(context);

    icc.set_profile_version(4.4);
    icc.set_device_class(class::LINK);
    icc.set_color_space(color_space);
    icc.set_pcs(color_space);
    icc.set_header_rendering_intent(INTENT_PERCEPTUAL);

    let n_channels = cms_channels_of_colorspace(color_space);
    if n_channels < 1 {
        return Err(cms_err!(ErrorCode::ColorspaceCheck, "unknown colour space"));
    }
    let n_channels = n_channels as u32;

    let mut pipeline = Pipeline::alloc(n_channels, n_channels)?;
    let stage = Stage::alloc_tone_curves(n_channels, Some(transfer))?;
    if !pipeline.insert_stage(StageLoc::Begin, stage) {
        return Err(cms_err!(ErrorCode::Internal, "linearization pipeline invalid"));
    }

    set_text_tags(&mut icc, "Linearization built-in")?;
    icc.write_tag(tag_sig::A_TO_B0, TagValue::Pipeline(pipeline))?;
    set_seq_desc_tag(&mut icc, "Linearization built-in")?;

    Ok(icc)
}

/// `cmsCreateLinearizationDeviceLink` — [`create_linearization_device_link_thr`] on a fresh
/// context.
pub fn create_linearization_device_link(
    color_space: Signature,
    transfer: &[ToneCurve],
) -> Result<Profile> {
    create_linearization_device_link_thr(Context::new(), color_space, transfer)
}

// ---------------------------------------------------------------------------------------
// cmsCreateInkLimitingDeviceLink[THR]
// ---------------------------------------------------------------------------------------

/// `InkLimitingSampler` — the CMYK ink-limiting kernel.
///
/// ```text
/// Sum = C + M + Y + K
/// If Sum > InkLimit and (C+M+Y) > 0:  Ratio = max(0, 1 - (Sum - InkLimit)/(C+M+Y))
/// else:                               Ratio = 1
/// C,M,Y *= Ratio;  K unchanged.
/// ```
fn ink_limiting_sampler(input: &[u16], output: &mut [u16], limit: f64) -> bool {
    let ink_limit = limit * 655.35;

    let sum_cmy = input[0] as f64 + input[1] as f64 + input[2] as f64;
    let sum_cmyk = sum_cmy + input[3] as f64;

    let ratio = if sum_cmyk > ink_limit && sum_cmy > 0.0 {
        let r = 1.0 - ((sum_cmyk - ink_limit) / sum_cmy);
        if r < 0.0 { 0.0 } else { r }
    } else {
        1.0
    };

    output[0] = quick_saturate_word(input[0] as f64 * ratio); // C
    output[1] = quick_saturate_word(input[1] as f64 * ratio); // M
    output[2] = quick_saturate_word(input[2] as f64 * ratio); // Y
    output[3] = input[3]; // K (untouched)
    true
}

/// `cmsCreateInkLimitingDeviceLinkTHR` — a CMYK device link enforcing a total-ink limit.
///
/// `limit` is a percentage; out-of-range values are clamped to `1..=400` (with a signalled
/// range warning), and a non-CMYK `color_space` is rejected — both exactly as the reference.
// The range check + clamp is transcribed verbatim from the C (two explicit comparisons rather
// than `RangeInclusive::contains` / `f64::clamp`), so keep clippy off both idiom lints.
#[allow(clippy::manual_range_contains, clippy::manual_clamp)]
pub fn create_ink_limiting_device_link_thr(
    context: Context,
    color_space: Signature,
    mut limit: f64,
) -> Result<Profile> {
    if color_space != colorspace::CMYK {
        context.signal(ErrorCode::ColorspaceCheck, "InkLimiting: Only CMYK currently supported");
        return Err(cms_err!(ErrorCode::ColorspaceCheck, "InkLimiting: Only CMYK currently supported"));
    }

    if limit < 1.0 || limit > 400.0 {
        context.signal(ErrorCode::Range, "InkLimiting: Limit should be between 1..400");
        if limit < 1.0 {
            limit = 1.0;
        }
        if limit > 400.0 {
            limit = 400.0;
        }
    }

    let mut icc = Profile::create_placeholder(context);

    icc.set_profile_version(4.4);
    icc.set_device_class(class::LINK);
    icc.set_color_space(color_space);
    icc.set_pcs(color_space);
    icc.set_header_rendering_intent(INTENT_PERCEPTUAL);

    let mut lut = Pipeline::alloc(4, 4)?;

    let n_channels = cms_channels_of(color_space);

    let mut clut = Stage::alloc_clut_16bit(17, n_channels, n_channels, None)?;
    if !clut.sample_clut_16(|in_, out| ink_limiting_sampler(in_, out, limit), 0) {
        return Err(cms_err!(ErrorCode::Internal, "ink-limiting sampling failed"));
    }

    if !lut.insert_stage(StageLoc::Begin, Stage::alloc_identity_curves(n_channels)?)
        || !lut.insert_stage(StageLoc::End, clut)
        || !lut.insert_stage(StageLoc::End, Stage::alloc_identity_curves(n_channels)?)
    {
        return Err(cms_err!(ErrorCode::Internal, "ink-limiting pipeline invalid"));
    }

    set_text_tags(&mut icc, "ink-limiting built-in")?;
    icc.write_tag(tag_sig::A_TO_B0, TagValue::Pipeline(lut))?;
    set_seq_desc_tag(&mut icc, "ink-limiting built-in")?;

    Ok(icc)
}

/// `cmsCreateInkLimitingDeviceLink` — [`create_ink_limiting_device_link_thr`] on a fresh
/// context.
pub fn create_ink_limiting_device_link(color_space: Signature, limit: f64) -> Result<Profile> {
    create_ink_limiting_device_link_thr(Context::new(), color_space, limit)
}

// ---------------------------------------------------------------------------------------
// cmsCreateLab2Profile[THR]
// ---------------------------------------------------------------------------------------

/// `cmsCreateLab2ProfileTHR` — a fake Lab identity, **version 2.1**, Abstract/Lab/Lab.
///
/// Note the version 2.1 (not 4.4) and the use of an identity CLUT rather than identity curves —
/// both intentional quirks of the reference.
pub fn create_lab2_profile_thr(context: Context, white_point: Option<&CIExyY>) -> Result<Profile> {
    let d50 = d50_xyy();
    let wp = white_point.unwrap_or(&d50);
    let mut profile = create_rgb_profile_thr(context, Some(wp), None, None)?;

    profile.set_profile_version(2.1);
    profile.set_device_class(class::ABSTRACT);
    profile.set_color_space(colorspace::LAB);
    profile.set_pcs(colorspace::LAB);

    set_text_tags(&mut profile, "Lab identity built-in")?;

    // An identity LUT is all we need.
    let mut lut = Pipeline::alloc(3, 3)?;
    if !lut.insert_stage(StageLoc::Begin, Stage::alloc_identity_clut(3)?) {
        return Err(cms_err!(ErrorCode::Internal, "Lab2 identity CLUT invalid"));
    }
    profile.write_tag(tag_sig::A_TO_B0, TagValue::Pipeline(lut))?;

    Ok(profile)
}

/// `cmsCreateLab2Profile` — [`create_lab2_profile_thr`] on a fresh context.
pub fn create_lab2_profile(white_point: Option<&CIExyY>) -> Result<Profile> {
    create_lab2_profile_thr(Context::new(), white_point)
}

// ---------------------------------------------------------------------------------------
// cmsCreateLab4Profile[THR]
// ---------------------------------------------------------------------------------------

/// `cmsCreateLab4ProfileTHR` — a fake Lab V4 identity, version 4.4, Abstract/Lab/Lab.
pub fn create_lab4_profile_thr(context: Context, white_point: Option<&CIExyY>) -> Result<Profile> {
    let xyz = match white_point {
        None => d50_xyz(),
        Some(wp) => cms_xyy_to_xyz(wp),
    };

    // Note: unlike Lab2, the RGB profile is created with a NULL white point (no wtpt/chad).
    let mut profile = create_rgb_profile_thr(context, None, None, None)?;

    profile.set_profile_version(4.4);
    profile.set_device_class(class::ABSTRACT);
    profile.set_color_space(colorspace::LAB);
    profile.set_pcs(colorspace::LAB);

    profile.write_tag(tag_sig::MEDIA_WHITE_POINT, TagValue::Xyz(xyz))?;
    set_text_tags(&mut profile, "Lab identity built-in")?;

    // An empty (identity-curves) LUT is all we need.
    let mut lut = Pipeline::alloc(3, 3)?;
    if !lut.insert_stage(StageLoc::Begin, Stage::alloc_identity_curves(3)?) {
        return Err(cms_err!(ErrorCode::Internal, "Lab4 identity curves invalid"));
    }
    profile.write_tag(tag_sig::A_TO_B0, TagValue::Pipeline(lut))?;

    Ok(profile)
}

/// `cmsCreateLab4Profile` — [`create_lab4_profile_thr`] on a fresh context.
pub fn create_lab4_profile(white_point: Option<&CIExyY>) -> Result<Profile> {
    create_lab4_profile_thr(Context::new(), white_point)
}

// ---------------------------------------------------------------------------------------
// cmsCreateXYZProfile[THR]
// ---------------------------------------------------------------------------------------

/// `cmsCreateXYZProfileTHR` — a fake XYZ identity, version 4.4, Abstract/XYZ/XYZ.
pub fn create_xyz_profile_thr(context: Context) -> Result<Profile> {
    let d50 = d50_xyy();
    let mut profile = create_rgb_profile_thr(context, Some(&d50), None, None)?;

    profile.set_profile_version(4.4);
    profile.set_device_class(class::ABSTRACT);
    profile.set_color_space(colorspace::XYZ);
    profile.set_pcs(colorspace::XYZ);

    set_text_tags(&mut profile, "XYZ identity built-in")?;

    let mut lut = Pipeline::alloc(3, 3)?;
    if !lut.insert_stage(StageLoc::Begin, Stage::alloc_identity_curves(3)?) {
        return Err(cms_err!(ErrorCode::Internal, "XYZ identity curves invalid"));
    }
    profile.write_tag(tag_sig::A_TO_B0, TagValue::Pipeline(lut))?;

    Ok(profile)
}

/// `cmsCreateXYZProfile` — [`create_xyz_profile_thr`] on a fresh context.
pub fn create_xyz_profile() -> Result<Profile> {
    create_xyz_profile_thr(Context::new())
}

// ---------------------------------------------------------------------------------------
// cmsCreate_sRGBProfile[THR]
// ---------------------------------------------------------------------------------------

/// `Build_sRGBGamma` — the sRGB transfer curve as a type-4 parametric curve.
///
/// Parameters `{2.4, 1/1.055, 0.055/1.055, 1/12.92, 0.04045}` — verbatim from the reference.
fn build_srgb_gamma() -> Result<ToneCurve> {
    let params = [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045];
    ToneCurve::build_parametric(4, &params)
}

/// `cmsCreate_sRGBProfileTHR` — the ICC virtual sRGB profile (D65 white, Rec.709 primaries,
/// sRGB gamma).
pub fn create_srgb_profile_thr(context: Context) -> Result<Profile> {
    let d65 = CIExyY { x: 0.3127, y: 0.3290, yy: 1.0 };
    let rec709_primaries = CIExyYTriple {
        red: CIExyY { x: 0.6400, y: 0.3300, yy: 1.0 },
        green: CIExyY { x: 0.3000, y: 0.6000, yy: 1.0 },
        blue: CIExyY { x: 0.1500, y: 0.0600, yy: 1.0 },
    };

    // One shared curve for all three channels — the shared reference triggers the TRC link
    // de-duplication in create_rgb_profile_thr.
    let gamma = build_srgb_gamma()?;

    let mut srgb = create_rgb_profile_thr(
        context,
        Some(&d65),
        Some(&rec709_primaries),
        Some([&gamma, &gamma, &gamma]),
    )?;

    set_text_tags(&mut srgb, "sRGB built-in")?;
    Ok(srgb)
}

/// `cmsCreate_sRGBProfile` — [`create_srgb_profile_thr`] on a fresh context.
pub fn create_srgb_profile() -> Result<Profile> {
    create_srgb_profile_thr(Context::new())
}

// ---------------------------------------------------------------------------------------
// cmsCreate_OkLabProfile (experimental — NON-serializable)
// ---------------------------------------------------------------------------------------

/// D65→D50 chromatic-adaptation matrix used by the OkLab profile (`M_D65_D50`).
#[rustfmt::skip]
const M_D65_D50: [f64; 9] = [
    1.047886, 0.022919, -0.050216,
    0.029582, 0.990484, -0.017079,
   -0.009252, 0.015073,  0.751678,
];
/// D50→D65 chromatic-adaptation matrix (`M_D50_D65`).
///
/// The literals are transcribed digit-for-digit from the C for bit-exactness; several carry more
/// decimals than an `f64` retains, so the `excessive_precision` lint is silenced (the same is
/// done for the other high-precision OkLab matrices below).
#[rustfmt::skip]
#[allow(clippy::excessive_precision)]
const M_D50_D65: [f64; 9] = [
     0.955512609517083, -0.023073214184645,  0.063308961782107,
    -0.028324949364887,  1.009942432477107,  0.021054814890112,
     0.012328875695483, -0.020535835374141,  1.330713916450354,
];
/// D65 XYZ→LMS matrix (`M_D65_LMS`).
#[rustfmt::skip]
const M_D65_LMS: [f64; 9] = [
    0.8189330101, 0.3618667424, -0.1288597137,
    0.0329845436, 0.9293118715,  0.0361456387,
    0.0482003018, 0.2643662691,  0.6338517070,
];
/// LMS→D65 XYZ matrix (`M_LMS_D65`).
#[rustfmt::skip]
#[allow(clippy::excessive_precision)]
const M_LMS_D65: [f64; 9] = [
    1.227013851103521, -0.557799980651822,  0.281256148966468,
   -0.040580178423281,  1.112256869616830, -0.071676678665601,
   -0.076381284505707, -0.421481978418013,  1.586163220440795,
];
/// LMS'→OkLab matrix (`M_LMSprime_OkLab`).
#[rustfmt::skip]
const M_LMSPRIME_OKLAB: [f64; 9] = [
    0.2104542553,  0.7936177850, -0.0040720468,
    1.9779984951, -2.4285922050,  0.4505937099,
    0.0259040371,  0.7827717662, -0.8086757660,
];
/// OkLab→LMS' matrix (`M_OkLab_LMSprime`).
#[rustfmt::skip]
#[allow(clippy::excessive_precision)]
const M_OKLAB_LMSPRIME: [f64; 9] = [
    0.999999998450520,  0.396337792173768,  0.215803758060759,
    1.000000008881761, -0.105561342323656, -0.063854174771706,
    1.000000054672411, -0.089484182094966, -1.291485537864092,
];

/// `cmsCreate_OkLabProfile` — the experimental OkLab colour space (ColorSpace/3CLR/XYZ, v4.4).
///
/// **This virtual profile cannot be saved as an ICC file**: its `AToB0`/`BToA0` pipelines use the
/// `NormalizeFrom/ToXyzFloat` matrix stages, which have no serializable `mpet` element form. It is
/// usable only in-memory (e.g. as a transform endpoint).
pub fn create_oklab_profile(context: Context) -> Result<Profile> {
    let xyz_pcs = Stage::normalize_from_xyz(); // XYZPCS (_cmsStageNormalizeFromXyzFloat)
    let pcs_xyz = Stage::normalize_to_xyz(); // PCSXYZ (_cmsStageNormalizeToXyzFloat)

    let d65_to_d50 = Stage::alloc_matrix(3, 3, &M_D65_D50, None)?;
    let d50_to_d65 = Stage::alloc_matrix(3, 3, &M_D50_D65, None)?;
    let d65_to_lms = Stage::alloc_matrix(3, 3, &M_D65_LMS, None)?;
    let lms_to_d65 = Stage::alloc_matrix(3, 3, &M_LMS_D65, None)?;

    let cube_root = ToneCurve::build_gamma(1.0 / 3.0)?;
    let cube = ToneCurve::build_gamma(3.0)?;

    let non_linearity_fw =
        Stage::alloc_tone_curves(3, Some(&[cube_root.clone(), cube_root.clone(), cube_root]))?;
    let non_linearity_rv =
        Stage::alloc_tone_curves(3, Some(&[cube.clone(), cube.clone(), cube]))?;

    let lmsprime_oklab = Stage::alloc_matrix(3, 3, &M_LMSPRIME_OKLAB, None)?;
    let oklab_lmsprime = Stage::alloc_matrix(3, 3, &M_OKLAB_LMSPRIME, None)?;

    let mut a_to_b = Pipeline::alloc(3, 3)?;
    let mut b_to_a = Pipeline::alloc(3, 3)?;

    let mut profile = Profile::create_placeholder(context);
    profile.set_profile_version(4.4);
    profile.set_device_class(class::COLORSPACE);
    profile.set_color_space(colorspace::CLR3); // cmsSig3colorData == '3CLR'
    profile.set_pcs(colorspace::XYZ);
    profile.set_header_rendering_intent(INTENT_RELATIVE_COLORIMETRIC);

    // PCS (XYZ/D50) -> OkLab
    let ok = b_to_a.insert_stage(StageLoc::End, pcs_xyz)
        && b_to_a.insert_stage(StageLoc::End, d50_to_d65)
        && b_to_a.insert_stage(StageLoc::End, d65_to_lms)
        && b_to_a.insert_stage(StageLoc::End, non_linearity_fw)
        && b_to_a.insert_stage(StageLoc::End, lmsprime_oklab);
    if !ok {
        return Err(cms_err!(ErrorCode::Internal, "OkLab BToA pipeline invalid"));
    }
    profile.write_tag(tag_sig::B_TO_A0, TagValue::Pipeline(b_to_a))?;

    // OkLab -> PCS (XYZ/D50)
    let ok = a_to_b.insert_stage(StageLoc::End, oklab_lmsprime)
        && a_to_b.insert_stage(StageLoc::End, non_linearity_rv)
        && a_to_b.insert_stage(StageLoc::End, lms_to_d65)
        && a_to_b.insert_stage(StageLoc::End, d65_to_d50)
        && a_to_b.insert_stage(StageLoc::End, xyz_pcs);
    if !ok {
        return Err(cms_err!(ErrorCode::Internal, "OkLab AToB pipeline invalid"));
    }
    profile.write_tag(tag_sig::A_TO_B0, TagValue::Pipeline(a_to_b))?;

    Ok(profile)
}

// ---------------------------------------------------------------------------------------
// cmsCreateBCHSWabstractProfile[THR]
// ---------------------------------------------------------------------------------------

/// `BCHSWADJUSTS` — the parameters consumed by [`bchsw_sampler`].
#[derive(Clone, Copy)]
struct BchswAdjusts {
    brightness: f64,
    contrast: f64,
    hue: f64,
    saturation: f64,
    adjust_wp: bool,
    wp_src: CIEXYZ,
    wp_dest: CIEXYZ,
}

/// `bchswSampler` — brightness/contrast/hue/saturation and optional white-point displacement,
/// worked in Lab/LCh space.
fn bchsw_sampler(input: &[u16], output: &mut [u16], bchsw: &BchswAdjusts) -> bool {
    let lab_in = cms_lab_encoded_to_float(&[input[0], input[1], input[2]]);
    let lch_in = cms_lab_to_lch(&lab_in);

    // Adjustments on LCh.
    let lch_out = crate::types::color::CIELCh {
        l: lch_in.l * bchsw.contrast + bchsw.brightness,
        c: lch_in.c + bchsw.saturation,
        h: lch_in.h + bchsw.hue,
    };

    let mut lab_out = cms_lch_to_lab(&lch_out);

    // Move white point in Lab.
    if bchsw.adjust_wp {
        let xyz = cms_lab_to_xyz(Some(&bchsw.wp_src), &lab_out);
        lab_out = cms_xyz_to_lab(Some(&bchsw.wp_dest), &xyz);
    }

    let enc = cms_float_to_lab_encoded(&lab_out);
    output[0] = enc[0];
    output[1] = enc[1];
    output[2] = enc[2];
    true
}

/// `cmsCreateBCHSWabstractProfileTHR` — an abstract Lab profile applying brightness, contrast,
/// hue, saturation and a white-point shift (`TempSrc` → `TempDest`).
#[allow(clippy::too_many_arguments)]
pub fn create_bchsw_abstract_profile_thr(
    context: Context,
    n_lut_points: u32,
    bright: f64,
    contrast: f64,
    hue: f64,
    saturation: f64,
    temp_src: u32,
    temp_dest: u32,
) -> Result<Profile> {
    let (adjust_wp, wp_src, wp_dest) = if temp_src == temp_dest {
        (false, CIEXYZ::default(), CIEXYZ::default())
    } else {
        let wp = white_point_from_temp(temp_src as f64)?;
        let src = cms_xyy_to_xyz(&wp);
        let wp = white_point_from_temp(temp_dest as f64)?;
        let dest = cms_xyy_to_xyz(&wp);
        (true, src, dest)
    };

    let bchsw = BchswAdjusts {
        brightness: bright,
        contrast,
        hue,
        saturation,
        adjust_wp,
        wp_src,
        wp_dest,
    };

    let mut icc = Profile::create_placeholder(context);

    icc.set_device_class(class::ABSTRACT);
    icc.set_color_space(colorspace::LAB);
    icc.set_pcs(colorspace::LAB);
    icc.set_header_rendering_intent(INTENT_PERCEPTUAL);

    let mut pipeline = Pipeline::alloc(3, 3)?;

    // A 3D grid, one node count (nLUTPoints) per dimension.
    let dimensions = [n_lut_points, n_lut_points, n_lut_points];
    let mut clut = Stage::alloc_clut_16bit_granular(&dimensions, 3, 3, None)?;

    if !clut.sample_clut_16(|in_, out| bchsw_sampler(in_, out, &bchsw), 0) {
        return Err(cms_err!(ErrorCode::Internal, "BCHSW sampling failed"));
    }
    if !pipeline.insert_stage(StageLoc::End, clut) {
        return Err(cms_err!(ErrorCode::Internal, "BCHSW pipeline invalid"));
    }

    set_text_tags(&mut icc, "BCHS built-in")?;
    icc.write_tag(tag_sig::MEDIA_WHITE_POINT, TagValue::Xyz(d50_xyz()))?;
    icc.write_tag(tag_sig::A_TO_B0, TagValue::Pipeline(pipeline))?;

    Ok(icc)
}

/// `cmsCreateBCHSWabstractProfile` — [`create_bchsw_abstract_profile_thr`] on a fresh context.
#[allow(clippy::too_many_arguments)]
pub fn create_bchsw_abstract_profile(
    n_lut_points: u32,
    bright: f64,
    contrast: f64,
    hue: f64,
    saturation: f64,
    temp_src: u32,
    temp_dest: u32,
) -> Result<Profile> {
    create_bchsw_abstract_profile_thr(
        Context::new(),
        n_lut_points,
        bright,
        contrast,
        hue,
        saturation,
        temp_src,
        temp_dest,
    )
}

// ---------------------------------------------------------------------------------------
// cmsCreateNULLProfile[THR]
// ---------------------------------------------------------------------------------------

/// `cmsCreateNULLProfileTHR` — a fake NULL profile returning a single always-zero channel.
///
/// Output/Gray/Lab, version 4.4. Useful only for gamut-checking tricks. The pipeline is
/// `PostLin(3) · pick-L* matrix(1×3) · OutLin(1)` with the linearization curves flat at zero.
pub fn create_null_profile_thr(context: Context) -> Result<Profile> {
    let mut profile = Profile::create_placeholder(context);

    profile.set_profile_version(4.4);
    set_text_tags(&mut profile, "NULL profile built-in")?;

    profile.set_device_class(class::OUTPUT);
    profile.set_color_space(colorspace::GRAY);
    profile.set_pcs(colorspace::LAB);

    let mut lut = Pipeline::alloc(3, 1)?;

    // Flat two-entry tone curve, all zero (cmsBuildTabulatedToneCurve16 with {0, 0}).
    let empty = ToneCurve::build_tabulated_16(2, &[0, 0])?;
    let post_lin = Stage::alloc_tone_curves(3, Some(&[empty.clone(), empty.clone(), empty.clone()]))?;
    let out_lin = Stage::alloc_tone_curves(1, Some(&[empty]))?;

    // PickLstarMatrix = { 1, 0, 0 } (1 row, 3 cols).
    let pick_lstar = [1.0, 0.0, 0.0];
    let matrix = Stage::alloc_matrix(1, 3, &pick_lstar, None)?;

    if !lut.insert_stage(StageLoc::End, post_lin)
        || !lut.insert_stage(StageLoc::End, matrix)
        || !lut.insert_stage(StageLoc::End, out_lin)
    {
        return Err(cms_err!(ErrorCode::Internal, "NULL profile pipeline invalid"));
    }

    profile.write_tag(tag_sig::B_TO_A0, TagValue::Pipeline(lut))?;
    profile.write_tag(tag_sig::MEDIA_WHITE_POINT, TagValue::Xyz(d50_xyz()))?;

    Ok(profile)
}

/// `cmsCreateNULLProfile` — [`create_null_profile_thr`] on a fresh context.
pub fn create_null_profile() -> Result<Profile> {
    create_null_profile_thr(Context::new())
}

// ---------------------------------------------------------------------------------------
// cmsTransform2DeviceLink support (self-contained, tested core)
//
// The colour-space-agnostic core (IsPCS / FixColorSpaces / AllowedLUTTypes / CheckOne /
// FindCombination) is ported below and consumed by `transform2devicelink` further down.
// ---------------------------------------------------------------------------------------

/// `IsPCS` — is `color_space` a profile connection space (XYZ or Lab)?
pub fn is_pcs(color_space: Signature) -> bool {
    color_space == colorspace::XYZ || color_space == colorspace::LAB
}

/// `FixColorSpaces` — set device class / colour space / PCS on `profile` from the transform's
/// entry/exit spaces, honouring `cmsFLAGS_GUESSDEVICECLASS`.
///
/// `guess_device_class` corresponds to `(dwFlags & cmsFLAGS_GUESSDEVICECLASS) != 0`.
pub fn fix_color_spaces(
    profile: &mut Profile,
    color_space: Signature,
    pcs: Signature,
    guess_device_class: bool,
) {
    if guess_device_class {
        if is_pcs(color_space) && is_pcs(pcs) {
            profile.set_device_class(class::ABSTRACT);
            profile.set_color_space(color_space);
            profile.set_pcs(pcs);
            return;
        }
        if is_pcs(color_space) && !is_pcs(pcs) {
            profile.set_device_class(class::OUTPUT);
            profile.set_pcs(color_space);
            profile.set_color_space(pcs);
            return;
        }
        if is_pcs(pcs) && !is_pcs(color_space) {
            profile.set_device_class(class::INPUT);
            profile.set_color_space(color_space);
            profile.set_pcs(pcs);
            return;
        }
    }

    profile.set_device_class(class::LINK);
    profile.set_color_space(color_space);
    profile.set_pcs(pcs);
}

/// One row of the `AllowedLUTTypes` table: which MPE stage sequence a given LUT tag-type can
/// hold on a given ICC version (`cmsAllowedLUT`).
#[derive(Clone, Copy)]
pub struct AllowedLut {
    /// Is this a V4-only tag type?
    pub is_v4: bool,
    /// Required destination tag (`Signature(0)` = either).
    pub required_tag: Signature,
    /// The LUT tag type this row describes (informational; mirrors the C `LutType`).
    pub lut_type: Signature,
    /// The exact ordered stage-type sequence the pipeline must match.
    pub mpe_types: &'static [Signature],
}

/// `AllowedLUTTypes[]` — the 11 permitted LUT/MPE combinations, transcribed verbatim.
///
/// `Signature(0)` in `required_tag` is C's `cmsSig0` "either A2B or B2A". The `mpe_types` use the
/// stage signatures `matf`/`cvst`/`clut` ([`STAGE_MATRIX`]/[`STAGE_CURVE_SET`]/[`STAGE_CLUT`]).
pub static ALLOWED_LUT_TYPES: &[AllowedLut] = &[
    AllowedLut { is_v4: false, required_tag: Signature(0), lut_type: type_sig::LUT16,
        mpe_types: &[STAGE_MATRIX, STAGE_CURVE_SET, STAGE_CLUT, STAGE_CURVE_SET] },
    AllowedLut { is_v4: false, required_tag: Signature(0), lut_type: type_sig::LUT16,
        mpe_types: &[STAGE_CURVE_SET, STAGE_CLUT, STAGE_CURVE_SET] },
    AllowedLut { is_v4: false, required_tag: Signature(0), lut_type: type_sig::LUT16,
        mpe_types: &[STAGE_CURVE_SET, STAGE_CLUT] },
    AllowedLut { is_v4: true, required_tag: Signature(0), lut_type: type_sig::LUT_A2B,
        mpe_types: &[STAGE_CURVE_SET] },
    AllowedLut { is_v4: true, required_tag: tag_sig::A_TO_B0, lut_type: type_sig::LUT_A2B,
        mpe_types: &[STAGE_CURVE_SET, STAGE_MATRIX, STAGE_CURVE_SET] },
    AllowedLut { is_v4: true, required_tag: tag_sig::A_TO_B0, lut_type: type_sig::LUT_A2B,
        mpe_types: &[STAGE_CURVE_SET, STAGE_CLUT, STAGE_CURVE_SET] },
    AllowedLut { is_v4: true, required_tag: tag_sig::A_TO_B0, lut_type: type_sig::LUT_A2B,
        mpe_types: &[STAGE_CURVE_SET, STAGE_CLUT, STAGE_CURVE_SET, STAGE_MATRIX, STAGE_CURVE_SET] },
    AllowedLut { is_v4: true, required_tag: tag_sig::B_TO_A0, lut_type: type_sig::LUT_B2A,
        mpe_types: &[STAGE_CURVE_SET] },
    AllowedLut { is_v4: true, required_tag: tag_sig::B_TO_A0, lut_type: type_sig::LUT_B2A,
        mpe_types: &[STAGE_CURVE_SET, STAGE_MATRIX, STAGE_CURVE_SET] },
    AllowedLut { is_v4: true, required_tag: tag_sig::B_TO_A0, lut_type: type_sig::LUT_B2A,
        mpe_types: &[STAGE_CURVE_SET, STAGE_CLUT, STAGE_CURVE_SET] },
    AllowedLut { is_v4: true, required_tag: tag_sig::B_TO_A0, lut_type: type_sig::LUT_B2A,
        mpe_types: &[STAGE_CURVE_SET, STAGE_MATRIX, STAGE_CURVE_SET, STAGE_CLUT, STAGE_CURVE_SET] },
];

/// `CheckOne` — does `lut`'s stage-type sequence match `tab.mpe_types` exactly (same length and
/// order)?
pub fn check_one(tab: &AllowedLut, lut: &Pipeline) -> bool {
    let stages = lut.stages();
    for (n, mpe) in stages.iter().enumerate() {
        if n >= tab.mpe_types.len() {
            return false;
        }
        if mpe.type_signature() != tab.mpe_types[n] {
            return false;
        }
    }
    stages.len() == tab.mpe_types.len()
}

/// `FindCombination` — the first [`ALLOWED_LUT_TYPES`] row compatible with `lut`, the ICC
/// version (`is_v4`) and the destination tag, or `None`.
pub fn find_combination(
    lut: &Pipeline,
    is_v4: bool,
    destination_tag: Signature,
) -> Option<&'static AllowedLut> {
    for tab in ALLOWED_LUT_TYPES {
        // C: `if (IsV4 ^ Tab->IsV4) continue;`
        if is_v4 != tab.is_v4 {
            continue;
        }
        if tab.required_tag != Signature(0) && tab.required_tag != destination_tag {
            continue;
        }
        if check_one(tab, lut) {
            return Some(tab);
        }
    }
    None
}

// ---------------------------------------------------------------------------------------
// cmsTransform2DeviceLink
// ---------------------------------------------------------------------------------------

/// `cmsFLAGS_GUESSDEVICECLASS` (`lcms2.h`).
const FLAGS_GUESSDEVICECLASS: u32 = 0x0020;
/// `cmsFLAGS_8BITS_DEVICELINK` (`lcms2.h`).
const FLAGS_8BITS_DEVICELINK: u32 = 0x0008;

/// `cmsTransform2DeviceLink` — dump a compiled [`Transform`] into a device-link [`Profile`].
///
/// The transform's optimized pipeline is duplicated, patched for the V2/V4 Lab encoding
/// mismatch when `version < 4.0`, and stored under `AToB0`/`BToA0` in the form
/// [`find_combination`] accepts (optimizing / forcing a CLUT as needed). Colorant tables, the
/// profile sequence (`cmsFLAGS_KEEP_SEQUENCE`), the media white point and the rendering intent
/// are carried over exactly as the reference does.
///
/// A named-color transform (a leading `NamedColor` MPE) is reported as unsupported: it requires
/// the named-color pipeline stage + `cmsGetNamedColorList` (`DEFERRED.md` #2/#6), a subsystem not
/// yet ported — and, since no `NamedColor` MPE can be constructed, currently unreachable.
/// `CreateNamedColorDevicelink` (`cmsvirt.c:1078-1124`) — dump a named-colour transform into a
/// named-colour profile carrying `cmsSigNamedColor2Tag`.
///
/// The list is read back from the transform's own pipeline, re-pointed at the transform's OUTPUT
/// space (`ColorantCount = cmsPipelineOutputChannels`), and every spot colour is run through the
/// transform by index to recompute its device colorant. That is why the transform is rebound to
/// `TYPE_NAMED_COLOR_INDEX` first: the input side becomes a 1-channel 16-bit index, not a colour.
///
/// This is the documented way to merge several named-colour databases into one profile.
fn create_named_color_devicelink(xform: &mut Transform) -> Result<Profile> {
    let ctx = xform.context().clone();
    let exit = xform.exit_color_space();

    let out_channels = xform
        .pipeline()
        .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "transform has no pipeline"))?
        .output_channels();

    let mut nc2 = xform
        .named_color_list()
        .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "transform carries no named color list"))?
        .clone();
    let n_colors = nc2.count();

    // The colorant count now follows the OUTPUT space, not the source profile's (`:1104`).
    nc2.set_colorant_count(out_channels)?;

    let mut profile = Profile::create_placeholder(ctx);
    profile.set_device_class(class::NAMED_COLOR);
    profile.set_color_space(exit);
    profile.set_pcs(colorspace::LAB);
    set_text_tags(&mut profile, "Named color devicelink")?;

    // Rebind the formatters: index in, device colour out (`:1107-1109`).
    let index_fmt = FormatBuilder::new().channels(1).bytes(2).build();
    let device_fmt = FormatBuilder::new()
        .colorspace(lcms_colorspace(exit).max(0) as u32)
        .channels(cms_channels_of_colorspace(exit).max(0) as u32)
        .bytes(2)
        .build();
    xform.change_buffers_format(index_fmt, device_fmt)?;

    // Apply the transform to the colorants, one spot colour per index (`:1111-1114`).
    //
    // C passes `&i` - an `int` - straight in, relying on the formatter reading only its first
    // two bytes on a little-endian host. Feed the index as a `u16` explicitly instead: that is
    // what `TYPE_NAMED_COLOR_INDEX` (`CHANNELS_SH(1)|BYTES_SH(2)`) actually declares, and it is
    // endian-independent.
    let mut dst = vec![0u8; out_channels as usize * 2];
    let mut vals = vec![0u16; out_channels as usize];
    for i in 0..n_colors {
        let index = (i as u16).to_ne_bytes();
        xform.do_transform(&index, &mut dst, 1);
        for (v, c) in vals.iter_mut().zip(dst.chunks_exact(2)) {
            *v = u16::from_ne_bytes([c[0], c[1]]);
        }
        if !nc2.set_device_colorant(i, &vals) {
            return Err(cms_err!(ErrorCode::Internal, "named color index {i} out of range"));
        }
    }

    profile.write_tag(tag_sig::NAMED_COLOR2, TagValue::NamedColor(nc2))?;
    Ok(profile)
}

pub fn transform2devicelink(xform: &mut Transform, version: f64, flags: u32) -> Result<Profile> {
    let mut dw_flags = flags;
    let ctx = xform.context().clone();

    // The pipeline must exist.
    let src_lut = xform
        .pipeline()
        .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "transform has no pipeline"))?;

    // A named-color transform dumps into a named-color devicelink instead (`cmsvirt.c:1216-1222`).
    if src_lut.first_stage().is_some_and(|s| s.type_signature() == STAGE_NAMED_COLOR) {
        return create_named_color_devicelink(xform);
    }

    // Work on a copy of the transformation.
    let mut lut = src_lut.clone();

    let entry = xform.entry_color_space();
    let exit = xform.exit_color_space();

    // Fix the Lab2/Lab4 issue on the input side.
    if entry == colorspace::LAB
        && version < 4.0
        && !lut.insert_stage(StageLoc::Begin, Stage::alloc_lab_v2_to_v4_curves()?)
    {
        return Err(cms_err!(ErrorCode::Internal, "insert LabV2ToV4 failed"));
    }
    // On the output side too. V2/V4 PCS Lab encoding cannot fix white misalignments here.
    if exit == colorspace::LAB && version < 4.0 {
        dw_flags |= FLAGS_NOWHITEONWHITEFIXUP;
        if !lut.insert_stage(StageLoc::End, Stage::alloc_lab_v4_to_v2()) {
            return Err(cms_err!(ErrorCode::Internal, "insert LabV4ToV2 failed"));
        }
    }

    let mut profile = Profile::create_placeholder(ctx.clone());
    profile.set_profile_version(version);

    fix_color_spaces(&mut profile, entry, exit, dw_flags & FLAGS_GUESSDEVICECLASS != 0);

    // Optimize the LUT and precalculate a devicelink.
    let chans_in = cms_channels_of_colorspace(entry);
    let chans_out = cms_channels_of_colorspace(exit);
    let cs_bits_in = lcms_colorspace(entry);
    let cs_bits_out = lcms_colorspace(exit);

    let mut frm_in: PixelFormat = FormatBuilder::new()
        .colorspace(cs_bits_in.max(0) as u32)
        .channels(chans_in.max(0) as u32)
        .bytes(2)
        .build();
    let mut frm_out: PixelFormat = FormatBuilder::new()
        .colorspace(cs_bits_out.max(0) as u32)
        .channels(chans_out.max(0) as u32)
        .bytes(2)
        .build();

    let device_class = profile.device_class();
    let destination_tag = if device_class == class::OUTPUT {
        tag_sig::B_TO_A0
    } else {
        tag_sig::A_TO_B0
    };
    let is_v4 = version >= 4.0;
    let intent = xform.rendering_intent();

    // Check if the profile/version can store the result as-is.
    let mut allowed = if dw_flags & FLAGS_FORCE_CLUT != 0 {
        None
    } else {
        find_combination(&lut, is_v4, destination_tag)
    };

    // Not directly storable: try to optimize.
    //
    // C discards the optimizer's result at both call sites (`cmsvirt.c:1278`, `:1290`): on
    // failure the LUT is left untouched and control falls through to the next attempt, which
    // frequently still succeeds. Propagating the error would skip the force-CLUT fallback
    // below entirely. Only the final `allowed.is_none()` check is fatal, exactly as in C.
    //
    // `dw_flags` is threaded through by reference (C passes `&dwFlags`), so an optimizer's
    // mutation stays visible to the `FLAGS_8BITS_DEVICELINK` test further down - which is what
    // `cmsvirt.c:1319` reads.
    if allowed.is_none() {
        if let Err(e) =
            optimize_pipeline_thr(&ctx, &mut lut, intent, &mut frm_in, &mut frm_out, &mut dw_flags)
        {
            ctx.signal(e.code, &format!("devicelink optimization skipped: {}", e.message));
        }
        allowed = find_combination(&lut, is_v4, destination_tag);
    }

    // Still not storable: force a CLUT that can certainly be written.
    if allowed.is_none() {
        dw_flags |= FLAGS_FORCE_CLUT;
        if let Err(e) =
            optimize_pipeline_thr(&ctx, &mut lut, intent, &mut frm_in, &mut frm_out, &mut dw_flags)
        {
            ctx.signal(e.code, &format!("devicelink force-CLUT optimization skipped: {}", e.message));
        }

        // Put identity curves at the ends if needed.
        let need_begin =
            lut.first_stage().is_some_and(|s| s.type_signature() != STAGE_CURVE_SET);
        if need_begin
            && !lut.insert_stage(StageLoc::Begin, Stage::alloc_identity_curves(chans_in.max(0) as u32)?)
        {
            return Err(cms_err!(ErrorCode::Internal, "insert input identity curves failed"));
        }
        let need_end = lut.last_stage().is_some_and(|s| s.type_signature() != STAGE_CURVE_SET);
        if need_end
            && !lut.insert_stage(StageLoc::End, Stage::alloc_identity_curves(chans_out.max(0) as u32)?)
        {
            return Err(cms_err!(ErrorCode::Internal, "insert output identity curves failed"));
        }

        allowed = find_combination(&lut, is_v4, destination_tag);
    }

    // Something is wrong — cannot store this transform.
    if allowed.is_none() {
        return Err(cms_err!(ErrorCode::NotSuitable, "cannot store transform as a devicelink"));
    }

    if dw_flags & FLAGS_8BITS_DEVICELINK != 0 {
        lut.set_save_as_8bits(true);
    }

    // Tag the profile with information and store the result.
    set_text_tags(&mut profile, "devicelink")?;
    profile.write_tag(destination_tag, TagValue::Pipeline(lut))?;

    // Colorant tables depend on the device class.
    if let Some(input_colorant) = xform.input_colorant() {
        if device_class == class::LINK || device_class == class::INPUT {
            profile.write_tag(
                tag_sig::COLORANT_TABLE,
                TagValue::ColorantTable(input_colorant.clone()),
            )?;
        }
    }
    if let Some(output_colorant) = xform.output_colorant() {
        if device_class == class::LINK {
            profile.write_tag(
                tag_sig::COLORANT_TABLE_OUT,
                TagValue::ColorantTable(output_colorant.clone()),
            )?;
        } else {
            profile.write_tag(
                tag_sig::COLORANT_TABLE,
                TagValue::ColorantTable(output_colorant.clone()),
            )?;
        }
    }

    // The profile sequence, only on devicelinks.
    if device_class == class::LINK {
        if let Some(seq) = xform.sequence() {
            write_profile_sequence(&mut profile, seq)?;
        }
    }

    // The media white point.
    if device_class == class::INPUT {
        profile.write_tag(tag_sig::MEDIA_WHITE_POINT, TagValue::Xyz(xform.entry_white_point()))?;
    } else {
        profile.write_tag(tag_sig::MEDIA_WHITE_POINT, TagValue::Xyz(xform.exit_white_point()))?;
    }

    // Per §7.2.15 in spec 4.3.
    profile.set_header_rendering_intent(intent);

    Ok(profile)
}

#[cfg(test)]
mod tests;
