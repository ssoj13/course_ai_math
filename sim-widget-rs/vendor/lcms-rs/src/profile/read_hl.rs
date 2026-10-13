//! High-level profile readers — a pure-Rust port of `cmsio1.c`.
//!
//! Where [`super`] (the port of `cmsio0.c`) deals with the raw header, tag directory and
//! per-tag (de)serialization, this module is the *glue* that turns those low-level tags into
//! the objects the CMM actually consumes: media white point, chromatic-adaptation matrix,
//! ready-to-run [`Pipeline`]s for the input / output / device-link directions, the
//! matrix-shaper / CLUT / intent predicates, the profile-sequence machinery, and the
//! localized profile-info strings.
//!
//! All version glue (V2↔V4 Lab PCS fixups, XYZ/Lab float normalization, the "revert to
//! perceptual" fallbacks, the matrix-shaper synthesis from `rXYZ`/`gXYZ`/`bXYZ` + TRCs or
//! `grayTRC`) is transcribed verbatim from the reference so behaviour matches lcms2 exactly.
//!
//! ## Reuse vs. the C
//! lcms hands back *borrowed, profile-owned* objects from `cmsReadTag` and then
//! `cmsPipelineDup`s them. Our [`Profile::read_tag`] already returns an **owned clone**
//! ([`TagValue`]), so the explicit `cmsPipelineDup` collapses into "just use the value".
//!
//! ## Named-colour device class
//! The *named-colour* branches of [`read_input_lut`] and [`read_devicelink_lut`] build a
//! pipeline whose first element is the named-colour lookup stage ([`Stage::alloc_named_color`],
//! `_cmsStageAllocNamedColor`): the palette maps a colour index to Lab PCS (input LUT) or to
//! device colorants (device-link LUT), with the V2→V4 Lab matrix fixup appended per the C.

use crate::cms_err;
use crate::color::d50_xyz;
use crate::color::gamma::ToneCurve;
use crate::color::wtpnt::adaptation_matrix;
use crate::color::{D50X, D50Y, D50Z};
use crate::error::{ErrorCode, Result};
use crate::math::interp::CMS_LERP_FLAGS_TRILINEAR;
use crate::math::mat3::{Mat3, Vec3};
use crate::named::dict::Seq;
use crate::named::mlu::Mlu;
use crate::named::namedcolor::NamedColorList;
use crate::pipeline::{Pipeline, Stage, StageData, StageLoc};
use crate::tags::descriptor::tag_sig;
use crate::tags::handler::type_sig;
use crate::tags::TagValue;
use crate::types::color::CIEXYZ;
use crate::types::sig::{class, colorspace, Signature};

use super::Profile;

// ---------------------------------------------------------------------------------------
// Constants (INTENT_*, LCMS_USED_AS_*, the four LUT-tag tables, fixed-point adjustments)
// ---------------------------------------------------------------------------------------

/// `INTENT_PERCEPTUAL` (`lcms2.h`).
pub const INTENT_PERCEPTUAL: u32 = 0;
/// `INTENT_RELATIVE_COLORIMETRIC`.
pub const INTENT_RELATIVE_COLORIMETRIC: u32 = 1;
/// `INTENT_SATURATION`.
pub const INTENT_SATURATION: u32 = 2;
/// `INTENT_ABSOLUTE_COLORIMETRIC` — the largest intent handled by the LUT-tag tables.
pub const INTENT_ABSOLUTE_COLORIMETRIC: u32 = 3;

/// `LCMS_USED_AS_INPUT` — device→PCS direction (`cmsInt32Number` flags in `lcms2plugin.h`).
pub const LCMS_USED_AS_INPUT: u32 = 0;
/// `LCMS_USED_AS_OUTPUT` — PCS→device direction.
pub const LCMS_USED_AS_OUTPUT: u32 = 1;
/// `LCMS_USED_AS_PROOF` — proofing (input + relative-colorimetric output).
pub const LCMS_USED_AS_PROOF: u32 = 2;

/// `Device2PCS16` — 16-bit `AToBx` tags, indexed by intent (absolute falls back to relative).
const DEVICE_TO_PCS_16: [Signature; 4] =
    [tag_sig::A_TO_B0, tag_sig::A_TO_B1, tag_sig::A_TO_B2, tag_sig::A_TO_B1];
/// `Device2PCSFloat` — floating-point `DToBx` tags, indexed by intent.
const DEVICE_TO_PCS_FLOAT: [Signature; 4] =
    [tag_sig::D_TO_B0, tag_sig::D_TO_B1, tag_sig::D_TO_B2, tag_sig::D_TO_B3];
/// `PCS2Device16` — 16-bit `BToAx` tags, indexed by intent.
const PCS_TO_DEVICE_16: [Signature; 4] =
    [tag_sig::B_TO_A0, tag_sig::B_TO_A1, tag_sig::B_TO_A2, tag_sig::B_TO_A1];
/// `PCS2DeviceFloat` — floating-point `BToDx` tags, indexed by intent.
const PCS_TO_DEVICE_FLOAT: [Signature; 4] =
    [tag_sig::B_TO_D0, tag_sig::B_TO_D1, tag_sig::B_TO_D2, tag_sig::B_TO_D3];

/// `MAX_ENCODEABLE_XYZ` (`lcms2_internal.h`): `1 + 32767/32768`.
const MAX_ENCODEABLE_XYZ: f64 = 1.0 + 32767.0 / 32768.0;
/// `InpAdj` — 1.15 fixed-point → 0..1 input factor `1/MAX_ENCODEABLE_XYZ`.
const INP_ADJ: f64 = 1.0 / MAX_ENCODEABLE_XYZ;
/// `OutpAdj` — the reciprocal output factor `MAX_ENCODEABLE_XYZ`.
const OUTP_ADJ: f64 = MAX_ENCODEABLE_XYZ;

/// `GrayInputMatrix` — the PCS illuminant scaled by [`INP_ADJ`] (`{InpAdj·D50XYZ}`).
const GRAY_INPUT_MATRIX: [f64; 3] = [INP_ADJ * D50X, INP_ADJ * D50Y, INP_ADJ * D50Z];
/// `OneToThreeInputMatrix` — replicate one channel across three (`{1,1,1}`).
const ONE_TO_THREE_INPUT_MATRIX: [f64; 3] = [1.0, 1.0, 1.0];
/// `PickYMatrix` — extract `Y` (scaled by [`OUTP_ADJ`]) for XYZ gray output.
const PICK_Y_MATRIX: [f64; 3] = [0.0, OUTP_ADJ * D50Y, 0.0];
/// `PickLstarMatrix` — extract `L*` for Lab gray output (`{1,0,0}`).
const PICK_LSTAR_MATRIX: [f64; 3] = [1.0, 0.0, 0.0];

/// The four `cmsInfoType` selectors used by the profile-info accessors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoType {
    /// `cmsInfoDescription` — profile description (`desc`, or `dscm` on macOS).
    Description,
    /// `cmsInfoManufacturer` — device manufacturer (`dmnd`).
    Manufacturer,
    /// `cmsInfoModel` — device model (`dmdd`).
    Model,
    /// `cmsInfoCopyright` — copyright (`cprt`).
    Copyright,
}

// ---------------------------------------------------------------------------------------
// Small tag-read helpers (turn cmsReadTag + a cast into a typed Option/Result)
// ---------------------------------------------------------------------------------------

/// Read `sig` and require it to decode to a [`Pipeline`] (the C `(cmsPipeline*)cmsReadTag`).
///
/// The returned value is already an owned clone, so it plays the role of `cmsPipelineDup`.
fn read_pipeline_tag(prof: &mut Profile, sig: Signature) -> Result<Pipeline> {
    match prof.read_tag(sig)? {
        TagValue::Pipeline(p) => Ok(p),
        _ => Err(cms_err!(ErrorCode::NotSuitable, "tag {sig} did not decode to a pipeline")),
    }
}

/// Read `sig` as an XYZ colorant/white-point value; `None` if absent or of the wrong type
/// (mirrors the C code treating a `NULL`/mismatched `cmsReadTag` as "not present").
fn read_xyz_tag(prof: &mut Profile, sig: Signature) -> Option<CIEXYZ> {
    match prof.read_tag(sig).ok()? {
        TagValue::Xyz(v) => Some(v),
        _ => None,
    }
}

/// Read `sig` as a tone curve; `None` if absent or of the wrong type.
fn read_curve_tag(prof: &mut Profile, sig: Signature) -> Option<ToneCurve> {
    match prof.read_tag(sig).ok()? {
        TagValue::Curve(c) => Some(c),
        _ => None,
    }
}

/// Read `sig` as an MLU and duplicate it (`GetMLUFromProfile`); `None` if absent.
fn read_mlu_tag(prof: &mut Profile, sig: Signature) -> Option<Mlu> {
    match prof.read_tag(sig).ok()? {
        TagValue::Mlu(m) => Some(m),
        _ => None,
    }
}

/// Read `sig` as a profile-sequence description; `None` if absent.
fn read_seq_tag(prof: &mut Profile, sig: Signature) -> Option<Seq> {
    match prof.read_tag(sig).ok()? {
        TagValue::Seq(s) => Some(s),
        _ => None,
    }
}

/// Read `sig` (the `ncl2` named-colour tag) as a [`NamedColorList`], erroring when the tag is
/// absent or of the wrong type — the C `if (nc == NULL) return NULL;` guard that aborts a
/// named-colour LUT build.
fn read_named_color_tag(prof: &mut Profile, sig: Signature) -> Result<NamedColorList> {
    match prof.read_tag(sig)? {
        TagValue::NamedColor(list) => Ok(list),
        _ => Err(cms_err!(ErrorCode::NotSuitable, "tag {sig} did not decode to a named-colour list")),
    }
}

/// `cmsPipelineInsertStage` + its `if (!...) goto Error` guard: insert `mpe`, turning a
/// failed `BlessLUT` (channel-count mismatch) into an error rather than a silent bad chain.
fn push_stage(lut: &mut Pipeline, loc: StageLoc, mpe: Stage) -> Result<()> {
    if lut.insert_stage(loc, mpe) {
        Ok(())
    } else {
        Err(cms_err!(ErrorCode::Internal, "pipeline stage insertion invalidated the chain"))
    }
}

/// Flatten a [`Mat3`] (three row vectors) to the row-major `[f64; 9]` that
/// [`Stage::alloc_matrix`] expects — the exact memory order of `(cmsFloat64Number*)&Mat`.
fn mat3_row_major(m: &Mat3) -> [f64; 9] {
    [
        m.0[0].0[0], m.0[0].0[1], m.0[0].0[2],
        m.0[1].0[0], m.0[1].0[1], m.0[1].0[2],
        m.0[2].0[0], m.0[2].0[1], m.0[2].0[2],
    ]
}

/// `ReadICCMatrixRGB2XYZ` — read the RGB colorants into a column-wise XYZ matrix.
///
/// Row `k` holds the `k`-th XYZ component of (Red, Green, Blue); `None` if any colorant tag
/// is missing.
fn read_icc_matrix_rgb2xyz(prof: &mut Profile) -> Option<Mat3> {
    let red = read_xyz_tag(prof, tag_sig::RED_COLORANT)?;
    let green = read_xyz_tag(prof, tag_sig::GREEN_COLORANT)?;
    let blue = read_xyz_tag(prof, tag_sig::BLUE_COLORANT)?;
    Some(Mat3::from_rows(
        Vec3::new(red.x, green.x, blue.x),
        Vec3::new(red.y, green.y, blue.y),
        Vec3::new(red.z, green.z, blue.z),
    ))
}

/// Read the three device TRCs (`rTRC`/`gTRC`/`bTRC`); `None` if any is missing.
fn read_rgb_trc(prof: &mut Profile) -> Option<[ToneCurve; 3]> {
    let r = read_curve_tag(prof, tag_sig::RED_TRC)?;
    let g = read_curve_tag(prof, tag_sig::GREEN_TRC)?;
    let b = read_curve_tag(prof, tag_sig::BLUE_TRC)?;
    Some([r, g, b])
}

// ---------------------------------------------------------------------------------------
// Media white point + chromatic adaptation (_cmsReadMediaWhitePoint / _cmsReadCHAD)
// ---------------------------------------------------------------------------------------

/// `_cmsReadMediaWhitePoint` — the media white point, with the two lcms fixups.
///
/// Absent `wtpt` → D50; and a **V2 display** profile is forced to D50 even when it carries a
/// white point (many old display profiles store a non-D50 value the spec disallows).
pub fn read_media_white_point(prof: &mut Profile) -> Result<CIEXYZ> {
    let tag = match read_xyz_tag(prof, tag_sig::MEDIA_WHITE_POINT) {
        // If no wp, take D50.
        None => return Ok(d50_xyz()),
        Some(v) => v,
    };

    // V2 display profiles should give D50.
    if prof.encoded_icc_version() < 0x0400_0000 && prof.device_class() == class::DISPLAY {
        return Ok(d50_xyz());
    }

    Ok(tag)
}

/// `_cmsReadCHAD` — the chromatic-adaptation matrix, with the lcms fixups.
///
/// A present `chad` tag (nine `s15Fixed16`, row-major) is used directly. Otherwise the result
/// is identity, except for a **V2 display** profile, where the Bradford adaptation
/// `mediaWhite → D50` is synthesized (identity if there is no white point either).
pub fn read_chad(prof: &mut Profile) -> Result<Mat3> {
    if let Some(m) = read_chad_matrix(prof) {
        return Ok(m);
    }

    // No CHAD available, default it to identity.
    // V2 display profiles should give D50.
    if prof.encoded_icc_version() < 0x0400_0000 && prof.device_class() == class::DISPLAY {
        match read_xyz_tag(prof, tag_sig::MEDIA_WHITE_POINT) {
            None => return Ok(Mat3::identity()),
            Some(white) => {
                return adaptation_matrix(None, &white, &d50_xyz()).ok_or_else(|| {
                    cms_err!(ErrorCode::Range, "cannot build adaptation matrix from media white")
                });
            }
        }
    }

    Ok(Mat3::identity())
}

/// Read the `chad` tag as a [`Mat3`] (nine `s15Fixed16` decoded to `f64`, row-major); `None`
/// if absent or malformed.
fn read_chad_matrix(prof: &mut Profile) -> Option<Mat3> {
    match prof.read_tag(tag_sig::CHROMATIC_ADAPTATION).ok()? {
        TagValue::S15Fixed16Array(a) if a.len() >= 9 => Some(Mat3::from_rows(
            Vec3::new(a[0], a[1], a[2]),
            Vec3::new(a[3], a[4], a[5]),
            Vec3::new(a[6], a[7], a[8]),
        )),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------
// Input LUT (_cmsReadInputLUT and its helpers)
// ---------------------------------------------------------------------------------------

/// `_cmsReadInputLUT` — build the device→PCS pipeline for `intent`.
///
/// Precedence: named-colour class (palette → Lab PCS + V2→V4 fixup) → float `DToBx` → 16-bit
/// `AToBx` (reverting to perceptual if the exact-intent tag is absent, with the V2↔V4 Lab
/// fixups) → matrix-shaper synthesis (gray or RGB). `intent == 0xffffffff` deliberately skips
/// every LUT tag and forces the matrix-shaper path.
pub fn read_input_lut(prof: &mut Profile, intent: u32) -> Result<Pipeline> {
    // On named color, take the appropriate tag: the palette maps a colour index to Lab PCS
    // (`UsePCS = TRUE`), then a V2→V4 Lab matrix fixes up the PCS encoding
    // (`_cmsReadInputLUT`, cmsio1.c). The LabV2ToV4 stage is inserted unconditionally here.
    if prof.device_class() == class::NAMED_COLOR {
        let nc = read_named_color_tag(prof, tag_sig::NAMED_COLOR2)?;
        let mut lut = Pipeline::alloc(0, 0)?;
        push_stage(&mut lut, StageLoc::Begin, Stage::alloc_named_color(nc, true))?;
        push_stage(&mut lut, StageLoc::End, Stage::alloc_lab_v2_to_v4())?;
        return Ok(lut);
    }

    // Intent = 0xffffffff can be used to always retrieve the matrix-shaper.
    if intent <= INTENT_ABSOLUTE_COLORIMETRIC {
        let tag_float = DEVICE_TO_PCS_FLOAT[intent as usize];

        // Float tag takes precedence.
        if prof.is_tag(tag_float) {
            return read_float_input_tag(prof, tag_float);
        }

        // Revert to perceptual if no tag is found.
        let mut tag16 = DEVICE_TO_PCS_16[intent as usize];
        if !prof.is_tag(tag16) {
            tag16 = DEVICE_TO_PCS_16[0];
        }

        if prof.is_tag(tag16) {
            // First read the tag (this also records its true on-disk type).
            let mut lut = read_pipeline_tag(prof, tag16)?;
            let original_type = prof.tag_true_type(tag16);

            // We need to adjust data only for Lab16 on output.
            if original_type != Some(type_sig::LUT16) || prof.pcs() != colorspace::LAB {
                return Ok(lut);
            }

            // If the input is Lab, add also a conversion at the begin.
            if prof.color_space() == colorspace::LAB {
                push_stage(&mut lut, StageLoc::Begin, Stage::alloc_lab_v4_to_v2())?;
            }
            // Add a matrix for conversion V2 to V4 Lab PCS.
            push_stage(&mut lut, StageLoc::End, Stage::alloc_lab_v2_to_v4())?;
            return Ok(lut);
        }
    }

    // Lut not found, try to create a matrix-shaper.
    if prof.color_space() == colorspace::GRAY {
        return build_gray_input_matrix_pipeline(prof);
    }
    build_rgb_input_matrix_shaper(prof)
}

/// `_cmsReadFloatInputTag` — read a float `DToBx` LUT, normalizing Lab/XYZ on both ends.
fn read_float_input_tag(prof: &mut Profile, tag_float: Signature) -> Result<Pipeline> {
    let mut lut = read_pipeline_tag(prof, tag_float)?;
    let spc = prof.color_space();
    let pcs = prof.pcs();

    if spc == colorspace::LAB {
        push_stage(&mut lut, StageLoc::Begin, Stage::normalize_to_lab())?;
    } else if spc == colorspace::XYZ {
        push_stage(&mut lut, StageLoc::Begin, Stage::normalize_to_xyz())?;
    }

    if pcs == colorspace::LAB {
        push_stage(&mut lut, StageLoc::End, Stage::normalize_from_lab())?;
    } else if pcs == colorspace::XYZ {
        push_stage(&mut lut, StageLoc::End, Stage::normalize_from_xyz())?;
    }

    Ok(lut)
}

/// `BuildGrayInputMatrixPipeline` — synthesize a gray→PCS pipeline from `kTRC`.
fn build_gray_input_matrix_pipeline(prof: &mut Profile) -> Result<Pipeline> {
    let gray_trc = read_curve_tag(prof, tag_sig::GRAY_TRC)
        .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "gray profile lacks kTRC"))?;

    let mut lut = Pipeline::alloc(1, 3)?;

    if prof.pcs() == colorspace::LAB {
        // Implement as an identity matrix plus 3 tone curves (GrayTRC then two empty tabs).
        let empty = ToneCurve::build_tabulated_16(2, &[0x8080, 0x8080])?;
        let lab_curves = [gray_trc, empty.clone(), empty];
        push_stage(&mut lut, StageLoc::End, Stage::alloc_matrix(3, 1, &ONE_TO_THREE_INPUT_MATRIX, None)?)?;
        push_stage(&mut lut, StageLoc::End, Stage::alloc_tone_curves(3, Some(&lab_curves))?)?;
    } else {
        let curves = [gray_trc];
        push_stage(&mut lut, StageLoc::End, Stage::alloc_tone_curves(1, Some(&curves))?)?;
        push_stage(&mut lut, StageLoc::End, Stage::alloc_matrix(3, 1, &GRAY_INPUT_MATRIX, None)?)?;
    }

    Ok(lut)
}

/// `BuildRGBInputMatrixShaper` — synthesize an RGB→PCS matrix-shaper from colorants + TRCs.
fn build_rgb_input_matrix_shaper(prof: &mut Profile) -> Result<Pipeline> {
    let mut mat = read_icc_matrix_rgb2xyz(prof)
        .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "profile lacks RGB colorant tags"))?;

    // Adjust the matrix output from 0..0xffff to the 1.15 XYZ PCS encoding.
    for i in 0..3 {
        for j in 0..3 {
            mat.0[i].0[j] *= INP_ADJ;
        }
    }

    let shapes = read_rgb_trc(prof)
        .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "profile lacks RGB TRC tags"))?;

    let mut lut = Pipeline::alloc(3, 3)?;
    push_stage(&mut lut, StageLoc::End, Stage::alloc_tone_curves(3, Some(&shapes))?)?;
    let flat = mat3_row_major(&mat);
    push_stage(&mut lut, StageLoc::End, Stage::alloc_matrix(3, 3, &flat, None)?)?;

    // Tolerated even though the spec disallows it: a matrix-shaper feeding a Lab PCS.
    if prof.pcs() == colorspace::LAB {
        push_stage(&mut lut, StageLoc::End, Stage::alloc_xyz2lab())?;
    }

    Ok(lut)
}

// ---------------------------------------------------------------------------------------
// Output LUT (_cmsReadOutputLUT and its helpers)
// ---------------------------------------------------------------------------------------

/// `_cmsReadOutputLUT` — build the PCS→device pipeline for `intent`.
///
/// Mirrors [`read_input_lut`] in the opposite direction: float `BToDx` → 16-bit `BToAx`
/// (with trilinear-interpolation forcing and V4↔V2 Lab fixups for Lab PCS) → gray/RGB
/// matrix-shaper (the latter operating purely in XYZ).
pub fn read_output_lut(prof: &mut Profile, intent: u32) -> Result<Pipeline> {
    if intent <= INTENT_ABSOLUTE_COLORIMETRIC {
        let tag_float = PCS_TO_DEVICE_FLOAT[intent as usize];

        // Float tag takes precedence.
        if prof.is_tag(tag_float) {
            return read_float_output_tag(prof, tag_float);
        }

        // Revert to perceptual if no tag is found.
        let mut tag16 = PCS_TO_DEVICE_16[intent as usize];
        if !prof.is_tag(tag16) {
            tag16 = PCS_TO_DEVICE_16[0];
        }

        if prof.is_tag(tag16) {
            let lut = read_pipeline_tag(prof, tag16)?;
            // Info about the original type (recorded by the read above).
            let original_type = prof.tag_true_type(tag16);
            let is_lab_pcs = prof.pcs() == colorspace::LAB;

            // For 3D LUTs indexed in Lab, trilinear interpolation should be used.
            let mut lut = if is_lab_pcs { change_interp_to_trilinear(&lut)? } else { lut };

            // We need to adjust data only for Lab and Lut16 type.
            if original_type != Some(type_sig::LUT16) || !is_lab_pcs {
                return Ok(lut);
            }

            // Add a matrix for conversion V4 to V2 Lab PCS.
            push_stage(&mut lut, StageLoc::Begin, Stage::alloc_lab_v4_to_v2())?;
            // If the output is Lab, add also a conversion at the end.
            if prof.color_space() == colorspace::LAB {
                push_stage(&mut lut, StageLoc::End, Stage::alloc_lab_v2_to_v4())?;
            }
            return Ok(lut);
        }
    }

    // Lut not found, try to create a matrix-shaper.
    if prof.color_space() == colorspace::GRAY {
        return build_gray_output_pipeline(prof);
    }
    build_rgb_output_matrix_shaper(prof)
}

/// `_cmsReadFloatOutputTag` — read a float `BToDx` LUT, undoing the PCS/data-space encoding.
fn read_float_output_tag(prof: &mut Profile, tag_float: Signature) -> Result<Pipeline> {
    let mut lut = read_pipeline_tag(prof, tag_float)?;
    let pcs = prof.pcs();
    let data_space = prof.color_space();

    if pcs == colorspace::LAB {
        push_stage(&mut lut, StageLoc::Begin, Stage::normalize_to_lab())?;
    } else if pcs == colorspace::XYZ {
        push_stage(&mut lut, StageLoc::Begin, Stage::normalize_to_xyz())?;
    }

    if data_space == colorspace::LAB {
        push_stage(&mut lut, StageLoc::End, Stage::normalize_from_lab())?;
    } else if data_space == colorspace::XYZ {
        push_stage(&mut lut, StageLoc::End, Stage::normalize_from_xyz())?;
    }

    Ok(lut)
}

/// `BuildGrayOutputPipeline` — synthesize a PCS→gray pipeline from the reversed `kTRC`.
///
/// Gray is taken from `Y` (XYZ PCS) or `L*` (Lab PCS), then through the inverse TRC.
fn build_gray_output_pipeline(prof: &mut Profile) -> Result<Pipeline> {
    let gray_trc = read_curve_tag(prof, tag_sig::GRAY_TRC)
        .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "gray profile lacks kTRC"))?;
    let rev_gray_trc = gray_trc.reverse()?;

    let mut lut = Pipeline::alloc(3, 1)?;

    if prof.pcs() == colorspace::LAB {
        push_stage(&mut lut, StageLoc::End, Stage::alloc_matrix(1, 3, &PICK_LSTAR_MATRIX, None)?)?;
    } else {
        push_stage(&mut lut, StageLoc::End, Stage::alloc_matrix(1, 3, &PICK_Y_MATRIX, None)?)?;
    }

    let curves = [rev_gray_trc];
    push_stage(&mut lut, StageLoc::End, Stage::alloc_tone_curves(1, Some(&curves))?)?;

    Ok(lut)
}

/// `BuildRGBOutputMatrixShaper` — synthesize a PCS(XYZ)→RGB matrix-shaper.
fn build_rgb_output_matrix_shaper(prof: &mut Profile) -> Result<Pipeline> {
    let mat = read_icc_matrix_rgb2xyz(prof)
        .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "profile lacks RGB colorant tags"))?;

    let mut inv = mat
        .inverse()
        .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "RGB colorant matrix is singular"))?;

    // Adjust the matrix input from the 1.15 XYZ PCS encoding to the 0..0xffff range.
    for i in 0..3 {
        for j in 0..3 {
            inv.0[i].0[j] *= OUTP_ADJ;
        }
    }

    let shapes = read_rgb_trc(prof)
        .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "profile lacks RGB TRC tags"))?;
    let inv_shapes = [shapes[0].reverse()?, shapes[1].reverse()?, shapes[2].reverse()?];

    let mut lut = Pipeline::alloc(3, 3)?;

    // Tolerated even though the spec disallows it: a Lab PCS feeding a matrix-shaper.
    if prof.pcs() == colorspace::LAB {
        push_stage(&mut lut, StageLoc::End, Stage::alloc_lab2xyz())?;
    }

    let flat = mat3_row_major(&inv);
    push_stage(&mut lut, StageLoc::End, Stage::alloc_matrix(3, 3, &flat, None)?)?;
    push_stage(&mut lut, StageLoc::End, Stage::alloc_tone_curves(3, Some(&inv_shapes))?)?;

    Ok(lut)
}

// ---------------------------------------------------------------------------------------
// Device-link LUT (_cmsReadDevicelinkLUT and its helper)
// ---------------------------------------------------------------------------------------

/// `_cmsReadDevicelinkLUT` — build the device-link (or abstract) pipeline for `intent`.
///
/// Abstract profiles are included; matrix-shaper synthesis is *not* available here. Precedence:
/// named-colour class (palette → device colorants, Lab-only V2→V4 fixup) → float `DToBx` for the
/// intent → float `DToB0` (used verbatim, no normalization) → 16-bit `AToBx` (reverting to
/// `AToB0`) with the trilinear and Lab fixups.
pub fn read_devicelink_lut(prof: &mut Profile, intent: u32) -> Result<Pipeline> {
    if intent > INTENT_ABSOLUTE_COLORIMETRIC {
        return Err(cms_err!(ErrorCode::Range, "intent {intent} out of range for devicelink"));
    }

    let tag16 = DEVICE_TO_PCS_16[intent as usize];
    let tag_float = DEVICE_TO_PCS_FLOAT[intent as usize];

    // On named color, take the appropriate tag: the palette maps a colour index straight to
    // device colorants (`UsePCS = FALSE`); a Lab-PCS devicelink still needs the V2→V4 Lab
    // fixup, which the C only appends when the colour space is Lab (`_cmsReadDevicelinkLUT`).
    if prof.device_class() == class::NAMED_COLOR {
        let nc = read_named_color_tag(prof, tag_sig::NAMED_COLOR2)?;
        let mut lut = Pipeline::alloc(0, 0)?;
        push_stage(&mut lut, StageLoc::Begin, Stage::alloc_named_color(nc, false))?;
        if prof.color_space() == colorspace::LAB {
            push_stage(&mut lut, StageLoc::End, Stage::alloc_lab_v2_to_v4())?;
        }
        return Ok(lut);
    }

    // Float tag takes precedence.
    if prof.is_tag(tag_float) {
        return read_float_devicelink_tag(prof, tag_float);
    }

    // A perceptual float tag is used verbatim, without the space normalization stages.
    let tag_float0 = DEVICE_TO_PCS_FLOAT[0];
    if prof.is_tag(tag_float0) {
        return read_pipeline_tag(prof, tag_float0);
    }

    // Is there any LUT-Based table? Revert to perceptual otherwise.
    let tag16 = if prof.is_tag(tag16) {
        tag16
    } else {
        let t0 = DEVICE_TO_PCS_16[0];
        if !prof.is_tag(t0) {
            return Err(cms_err!(ErrorCode::NotSuitable, "devicelink has no usable LUT tag"));
        }
        t0
    };

    let lut = read_pipeline_tag(prof, tag16)?;

    // For 3D LUTs indexed in Lab, trilinear interpolation should be used.
    let is_lab_pcs = prof.pcs() == colorspace::LAB;
    let mut lut = if is_lab_pcs { change_interp_to_trilinear(&lut)? } else { lut };

    let original_type = prof.tag_true_type(tag16);

    // We need to adjust data for Lab16 on output.
    if original_type != Some(type_sig::LUT16) {
        return Ok(lut);
    }

    // Here it is possible to get Lab on both sides.
    if prof.color_space() == colorspace::LAB {
        push_stage(&mut lut, StageLoc::Begin, Stage::alloc_lab_v4_to_v2())?;
    }
    if prof.pcs() == colorspace::LAB {
        push_stage(&mut lut, StageLoc::End, Stage::alloc_lab_v2_to_v4())?;
    }

    Ok(lut)
}

/// `_cmsReadFloatDevicelinkTag` — read a float `DToBx` link, normalizing Lab/XYZ on both ends.
fn read_float_devicelink_tag(prof: &mut Profile, tag_float: Signature) -> Result<Pipeline> {
    let mut lut = read_pipeline_tag(prof, tag_float)?;
    let pcs = prof.pcs();
    let spc = prof.color_space();

    if spc == colorspace::LAB {
        push_stage(&mut lut, StageLoc::Begin, Stage::normalize_to_lab())?;
    } else if spc == colorspace::XYZ {
        push_stage(&mut lut, StageLoc::Begin, Stage::normalize_to_xyz())?;
    }

    if pcs == colorspace::LAB {
        push_stage(&mut lut, StageLoc::End, Stage::normalize_from_lab())?;
    } else if pcs == colorspace::XYZ {
        push_stage(&mut lut, StageLoc::End, Stage::normalize_from_xyz())?;
    }

    Ok(lut)
}

/// `ChangeInterpolationToTrilinear` — force every CLUT stage to trilinear interpolation.
///
/// The C mutates each `_cmsStageCLutData` in place (setting `CMS_LERP_FLAGS_TRILINEAR` and
/// re-selecting the interpolation routine). This port has no public mutable stage accessor, so
/// it rebuilds an equivalent pipeline whose CLUT stages carry the flag; because
/// [`crate::math::interp::InterpParams`] reads the flag at eval time, no routine re-selection
/// is needed. Non-CLUT stages are copied unchanged and the save-as-8-bit hint is preserved.
fn change_interp_to_trilinear(lut: &Pipeline) -> Result<Pipeline> {
    let mut out = Pipeline::alloc(lut.input_channels(), lut.output_channels())?;
    for stage in lut.stages() {
        let mut stage = stage.clone();
        if let StageData::Clut(clut) = &mut stage.data {
            clut.params.flags |= CMS_LERP_FLAGS_TRILINEAR;
        }
        if !out.insert_stage(StageLoc::End, stage) {
            return Err(cms_err!(
                ErrorCode::Internal,
                "trilinear rebuild invalidated the pipeline chain"
            ));
        }
    }
    out.set_save_as_8bits(lut.save_as_8bits());
    Ok(out)
}

// ---------------------------------------------------------------------------------------
// Matrix-shaper / CLUT / intent predicates
// ---------------------------------------------------------------------------------------

/// `cmsIsMatrixShaper` — is the profile implementable as a matrix-shaper?
///
/// Gray needs `kTRC`; RGB needs all three colorants **and** all three TRCs; everything else is
/// `false`.
pub fn is_matrix_shaper(prof: &Profile) -> bool {
    match prof.color_space() {
        cs if cs == colorspace::GRAY => prof.is_tag(tag_sig::GRAY_TRC),
        cs if cs == colorspace::RGB => {
            prof.is_tag(tag_sig::RED_COLORANT)
                && prof.is_tag(tag_sig::GREEN_COLORANT)
                && prof.is_tag(tag_sig::BLUE_COLORANT)
                && prof.is_tag(tag_sig::RED_TRC)
                && prof.is_tag(tag_sig::GREEN_TRC)
                && prof.is_tag(tag_sig::BLUE_TRC)
        }
        _ => false,
    }
}

/// `cmsIsCLUT` — is `intent` implemented as a CLUT in the given `used_direction`?
///
/// Device-links answer from the header rendering intent; proofing recurses (input + relative
/// colorimetric output); an unknown direction signals a range error and returns `false`.
pub fn is_clut(prof: &Profile, intent: u32, used_direction: u32) -> bool {
    // For devicelinks, the supported intent is the one stated in the header.
    if prof.device_class() == class::LINK {
        return prof.header_rendering_intent() == intent;
    }

    let tag_table: &[Signature; 4] = match used_direction {
        LCMS_USED_AS_INPUT => &DEVICE_TO_PCS_16,
        LCMS_USED_AS_OUTPUT => &PCS_TO_DEVICE_16,
        // For proofing, we need rel. colorimetric in output. Let's recurse.
        LCMS_USED_AS_PROOF => {
            return is_intent_supported(prof, intent, LCMS_USED_AS_INPUT)
                && is_intent_supported(prof, INTENT_RELATIVE_COLORIMETRIC, LCMS_USED_AS_OUTPUT);
        }
        other => {
            prof.context()
                .signal(ErrorCode::Range, &format!("Unexpected direction ({other})"));
            return false;
        }
    };

    // Extended intents are not strictly CLUT-based.
    if intent > INTENT_ABSOLUTE_COLORIMETRIC {
        return false;
    }

    prof.is_tag(tag_table[intent as usize])
}

/// `cmsIsIntentSupported` — CLUT support, or failing that any matrix-shaper.
///
/// The matrix-shaper "yes" is deliberately generous (V2 matrix shapers cannot honour a
/// non-zero black point under relative colorimetric), matching lcms.
pub fn is_intent_supported(prof: &Profile, intent: u32, used_direction: u32) -> bool {
    if is_clut(prof, intent, used_direction) {
        return true;
    }
    is_matrix_shaper(prof)
}

// ---------------------------------------------------------------------------------------
// Profile sequence (read / write / compile)
// ---------------------------------------------------------------------------------------

/// `_cmsReadProfileSequence` — merge `pseq` and `psid` into one sequence.
///
/// Absent both → `None`. With only one present, that one is returned. With both and matching
/// lengths, `psid`'s profile-ID and description overwrite the corresponding `pseq` fields;
/// mismatched lengths fall back to `pseq` alone.
pub fn read_profile_sequence(prof: &mut Profile) -> Option<Seq> {
    let profile_seq = read_seq_tag(prof, tag_sig::PROFILE_SEQUENCE_DESC);
    let profile_id = read_seq_tag(prof, tag_sig::PROFILE_SEQUENCE_ID);

    match (profile_seq, profile_id) {
        (None, None) => None,
        (None, Some(id)) => Some(id),
        (Some(seq), None) => Some(seq),
        (Some(seq), Some(id)) => {
            // We have to mix both together. For that they must agree in count.
            if seq.seq.len() != id.seq.len() {
                return Some(seq);
            }
            let mut new_seq = seq;
            for (dst, src) in new_seq.seq.iter_mut().zip(id.seq.iter()) {
                dst.profile_id = src.profile_id;
                dst.description = src.description.clone();
            }
            Some(new_seq)
        }
    }
}

/// `_cmsWriteProfileSequence` — write `pseq`, plus `psid` for V4 profiles.
pub fn write_profile_sequence(prof: &mut Profile, seq: &Seq) -> Result<()> {
    prof.write_tag(tag_sig::PROFILE_SEQUENCE_DESC, TagValue::Seq(seq.clone()))?;

    if prof.encoded_icc_version() >= 0x0400_0000 {
        prof.write_tag(tag_sig::PROFILE_SEQUENCE_ID, TagValue::Seq(seq.clone()))?;
    }

    Ok(())
}

/// `_cmsCompileProfileSequence` — build a sequence describing an array of profiles.
///
/// Each descriptor takes the header attributes / profile-ID / manufacturer / model, the
/// `tech` signature (0 when absent), and duplicated manufacturer / model / description MLUs.
pub fn compile_profile_sequence(profiles: &mut [Profile]) -> Result<Seq> {
    let mut seq = Seq::new(profiles.len())?;

    for (i, h) in profiles.iter_mut().enumerate() {
        let attributes = h.header_attributes();
        let profile_id = h.header_profile_id();
        let device_mfg = Signature(h.header_manufacturer());
        let device_model = Signature(h.header_model());
        let technology = match h.read_tag(tag_sig::TECHNOLOGY).ok() {
            Some(TagValue::Signature(s)) => s,
            _ => Signature(0),
        };
        let manufacturer = read_mlu_tag(h, tag_sig::DEVICE_MFG_DESC);
        let model = read_mlu_tag(h, tag_sig::DEVICE_MODEL_DESC);
        let description = read_mlu_tag(h, tag_sig::PROFILE_DESCRIPTION);

        let ps = &mut seq.seq[i];
        ps.attributes = attributes;
        ps.profile_id = profile_id;
        ps.device_mfg = device_mfg;
        ps.device_model = device_model;
        ps.technology = technology;
        ps.manufacturer = manufacturer;
        ps.model = model;
        ps.description = description;
    }

    Ok(seq)
}

// ---------------------------------------------------------------------------------------
// Profile info strings (cmsGetProfileInfo / *ASCII / *UTF8 via GetInfo)
// ---------------------------------------------------------------------------------------

/// `GetInfo` — the MLU backing an [`InfoType`], choosing the macOS `dscm` description tag when
/// present. `None` if the tag is absent.
fn get_info_mlu(prof: &mut Profile, info: InfoType) -> Option<Mlu> {
    let sig = match info {
        InfoType::Description => {
            // MacOS uses a proprietary tag for description.
            if prof.is_tag(tag_sig::PROFILE_DESCRIPTION_ML) {
                tag_sig::PROFILE_DESCRIPTION_ML
            } else {
                tag_sig::PROFILE_DESCRIPTION
            }
        }
        InfoType::Manufacturer => tag_sig::DEVICE_MFG_DESC,
        InfoType::Model => tag_sig::DEVICE_MODEL_DESC,
        InfoType::Copyright => tag_sig::COPYRIGHT,
    };
    read_mlu_tag(prof, sig)
}

/// `cmsGetProfileInfo` — the best-matching translation as UTF-16 code units, or `None`.
pub fn get_profile_info(
    prof: &mut Profile,
    info: InfoType,
    language: [u8; 2],
    country: [u8; 2],
) -> Option<Vec<u16>> {
    get_info_mlu(prof, info)?.get_wide(language, country)
}

/// `cmsGetProfileInfoASCII` — the best-matching translation as ASCII, or `None`.
pub fn get_profile_info_ascii(
    prof: &mut Profile,
    info: InfoType,
    language: [u8; 2],
    country: [u8; 2],
) -> Option<String> {
    get_info_mlu(prof, info)?.get_ascii(language, country)
}

/// `cmsGetProfileInfoUTF8` — the best-matching translation as UTF-8, or `None`.
pub fn get_profile_info_utf8(
    prof: &mut Profile,
    info: InfoType,
    language: [u8; 2],
    country: [u8; 2],
) -> Option<String> {
    get_info_mlu(prof, info)?.get_utf8(language, country)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Context;

    use crate::test_profile as open;

    /// ibm-t61 is an RGB display matrix-shaper: the predicate holds and both directions build
    /// 3→3 matrix-shaper pipelines.
    #[test]
    fn ibm_t61_is_matrix_shaper_and_builds_pipelines() {
        let mut p = open("ibm-t61.icc");
        assert_eq!(p.color_space(), colorspace::RGB);
        assert!(is_matrix_shaper(&p), "ibm-t61 should be a matrix shaper");

        let inp = read_input_lut(&mut p, INTENT_RELATIVE_COLORIMETRIC).expect("input lut");
        assert_eq!(inp.input_channels(), 3);
        assert_eq!(inp.output_channels(), 3);

        let outp = read_output_lut(&mut p, INTENT_RELATIVE_COLORIMETRIC).expect("output lut");
        assert_eq!(outp.input_channels(), 3);
        assert_eq!(outp.output_channels(), 3);
    }

    /// The white point is plausible (near D50 — a V2 display profile is forced to exactly D50).
    #[test]
    fn ibm_t61_white_point_near_d50() {
        let mut p = open("ibm-t61.icc");
        let wp = read_media_white_point(&mut p).expect("white point");
        assert!((wp.x - D50X).abs() < 0.05, "X = {}", wp.x);
        assert!((wp.y - D50Y).abs() < 0.05, "Y = {}", wp.y);
        assert!((wp.z - D50Z).abs() < 0.05, "Z = {}", wp.z);
    }

    /// CHAD reads without error and (for this V2 display profile) is close to identity.
    #[test]
    fn ibm_t61_chad_is_readable() {
        let mut p = open("ibm-t61.icc");
        let chad = read_chad(&mut p).expect("chad");
        // Sanity: the diagonal dominates (a near-identity Bradford/identity result).
        assert!(chad.0[0].0[0] > 0.5, "{chad:?}");
        assert!(chad.0[1].0[1] > 0.5, "{chad:?}");
        assert!(chad.0[2].0[2] > 0.5, "{chad:?}");
    }

    /// The profile description is a non-empty ASCII string.
    #[test]
    fn ibm_t61_description_ascii() {
        let mut p = open("ibm-t61.icc");
        let desc = get_profile_info_ascii(&mut p, InfoType::Description, *b"en", *b"US")
            .expect("description present");
        assert!(!desc.is_empty(), "description should not be empty");
    }

    /// test1 is a CMYK→Lab CLUT print profile: `is_clut` holds and the input LUT is
    /// CMYK(4)→PCS(3). (The bundled crayons/ibm-t61 profiles are RGB matrix-shapers, not the
    /// CMYK CLUT the task brief assumed; test1/test2 are the real CLUT fixtures.)
    #[test]
    fn cmyk_clut_is_clut_and_builds_input_lut() {
        let mut p = open("test1.icc");
        assert_eq!(p.color_space(), colorspace::CMYK);
        assert_eq!(p.pcs(), colorspace::LAB);
        assert!(
            is_clut(&p, INTENT_PERCEPTUAL, LCMS_USED_AS_INPUT),
            "test1 should be CLUT-based for perceptual input"
        );

        let inp = read_input_lut(&mut p, INTENT_PERCEPTUAL).expect("input lut");
        assert_eq!(inp.input_channels(), 4, "CMYK device space");
        assert_eq!(inp.output_channels(), 3, "3-channel PCS");
    }

    /// The same profile builds a PCS(3)→CMYK(4) output LUT.
    #[test]
    fn cmyk_clut_builds_output_lut() {
        let mut p = open("test1.icc");
        let outp = read_output_lut(&mut p, INTENT_PERCEPTUAL).expect("output lut");
        assert_eq!(outp.input_channels(), 3, "3-channel PCS");
        assert_eq!(outp.output_channels(), 4, "CMYK device space");
    }

    /// `is_intent_supported` agrees with `is_clut` for a CLUT profile.
    #[test]
    fn cmyk_clut_intent_supported() {
        let p = open("test1.icc");
        assert!(is_intent_supported(&p, INTENT_PERCEPTUAL, LCMS_USED_AS_INPUT));
    }

    /// A CLUT profile is not a matrix-shaper (`cmsIsMatrixShaper` only recognises gray/RGB
    /// with colorants + TRCs), so `read_input_lut` must go through the LUT path.
    #[test]
    fn cmyk_clut_not_matrix_shaper() {
        let p = open("test1.icc");
        assert!(!is_matrix_shaper(&p));
    }

    /// A synthetic named-colour profile now builds a real pipeline whose first element is the
    /// `ncl ` named-colour lookup stage — closing the `_cmsStageAllocNamedColor` residual that
    /// previously made both LUT directions error. No named-colour fixture ships in the testbed,
    /// so the profile is assembled in memory (placeholder header + a written `ncl2` tag).
    #[test]
    fn named_color_profile_builds_named_stage() {
        use crate::named::namedcolor::NamedColorList;
        use crate::pipeline::stage::STAGE_NAMED_COLOR;
        use crate::MAX_CHANNELS;

        let mut list = NamedColorList::new(3, "pre_", "_suf").unwrap();
        let mut dev = [0u16; MAX_CHANNELS];
        dev[..3].copy_from_slice(&[1000, 2000, 3000]);
        list.append("c0", [100, 30000, 65535], dev);
        list.append("c1", [200, 40000, 12345], dev);

        let mut p = Profile::create_placeholder(Context::new());
        p.set_device_class(class::NAMED_COLOR);
        p.set_color_space(colorspace::LAB); // Lab colour space exercises the V2->V4 devicelink fixup.
        p.set_pcs(colorspace::LAB);
        p.write_tag(tag_sig::NAMED_COLOR2, TagValue::NamedColor(list)).expect("write ncl2 tag");

        // Input LUT: colour index(1) -> Lab PCS(3), with the unconditional V2->V4 Lab matrix.
        let inp = read_input_lut(&mut p, INTENT_PERCEPTUAL).expect("named-colour input lut builds");
        assert_eq!(inp.input_channels(), 1, "one colour-index input");
        assert_eq!(inp.output_channels(), 3, "3-channel Lab PCS output");
        assert_eq!(
            inp.first_stage().expect("input lut has a first stage").type_signature(),
            STAGE_NAMED_COLOR,
            "first element must be the 'ncl ' named-colour stage"
        );

        // Device-link LUT: colour index(1) -> device colorants(3); Lab space appends the fixup.
        let dl = read_devicelink_lut(&mut p, INTENT_PERCEPTUAL).expect("named-colour devicelink lut builds");
        assert_eq!(dl.input_channels(), 1, "one colour-index input");
        assert_eq!(
            dl.first_stage().expect("devicelink lut has a first stage").type_signature(),
            STAGE_NAMED_COLOR,
            "first element must be the 'ncl ' named-colour stage"
        );
    }
}
