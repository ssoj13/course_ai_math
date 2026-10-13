//! Tag descriptors — the `SupportedTags` table of `cmstypes.c` (66 built-in tags) plus the
//! `DecideType` callbacks that choose which supported type to write for a given ICC version.
//!
//! `_cmsGetTagDescriptor` becomes [`get_descriptor`]; each `cmsTagDescriptor` becomes a
//! [`TagDescriptor`]. Plug-in tags (`_cmsRegisterTagPlugin`) are consulted first via the
//! [`crate::plugin`] registry, then this built-in `SupportedTags` table.

use crate::context::Context;
use crate::types::sig::Signature;

use super::handler::type_sig as ty;
use super::types::curve::n_segments;
use super::value::TagValue;

/// A tag descriptor (`cmsTagDescriptor`): element count, the acceptable type signatures (most
/// preferred first), and an optional writer-side type decision (`DecideType`).
#[derive(Debug, Clone)]
pub struct TagDescriptor {
    /// `ElemCount` — number of items the tag holds (e.g. 9 for a chromatic-adaptation matrix).
    pub elem_count: u32,
    /// `SupportedTypes` — type signatures this tag may be stored as, preferred first.
    pub supported_types: Vec<Signature>,
    /// `DecideType` — pick the on-disk type from the ICC version and the decoded value.
    pub decide_type: Option<fn(f64, &TagValue) -> Signature>,
}

// ---------------------------------------------------------------------------------------
// DecideType callbacks
// ---------------------------------------------------------------------------------------

/// `DecideLUTtypeA2B`.
fn decide_lut_a2b(version: f64, data: &TagValue) -> Signature {
    match data {
        TagValue::Pipeline(lut) if version < 4.0 => {
            if lut.save_as_8bits() { ty::LUT8 } else { ty::LUT16 }
        }
        TagValue::Pipeline(_) => ty::LUT_A2B,
        _ => ty::LUT16,
    }
}

/// `DecideLUTtypeB2A`.
fn decide_lut_b2a(version: f64, data: &TagValue) -> Signature {
    match data {
        TagValue::Pipeline(lut) if version < 4.0 => {
            if lut.save_as_8bits() { ty::LUT8 } else { ty::LUT16 }
        }
        TagValue::Pipeline(_) => ty::LUT_B2A,
        _ => ty::LUT16,
    }
}

/// `DecideXYZtype` — always plain XYZ (the broken alias is read-only).
fn decide_xyz(_version: f64, _data: &TagValue) -> Signature {
    ty::XYZ
}

/// `DecideCurveType` — `para` only for a V4 single-segment, non-inverted, ICC-parametric curve.
fn decide_curve_type(version: f64, data: &TagValue) -> Signature {
    let curve = match data {
        TagValue::Curve(c) => c,
        _ => return ty::CURVE,
    };
    if version < 4.0 {
        return ty::CURVE;
    }
    if n_segments(curve) != 1 {
        return ty::CURVE; // only 1-segment curves can be saved as parametric
    }
    let seg_type = curve.segment(0).map(|s| s.seg_type).unwrap_or(-1);
    if seg_type < 0 {
        return ty::CURVE; // only non-inverted curves
    }
    if seg_type > 5 {
        return ty::CURVE; // only ICC parametric curves
    }
    ty::PARAMETRIC_CURVE
}

/// `DecideTextType`.
fn decide_text_type(version: f64, _data: &TagValue) -> Signature {
    if version >= 4.0 {
        ty::MLU
    } else {
        ty::TEXT
    }
}

/// `DecideTextDescType`.
fn decide_text_desc_type(version: f64, _data: &TagValue) -> Signature {
    if version >= 4.0 {
        ty::MLU
    } else {
        ty::TEXT_DESCRIPTION
    }
}

// ---------------------------------------------------------------------------------------
// Tag signatures (cmsTagSignature, lcms2.h)
// ---------------------------------------------------------------------------------------

/// Tag signatures for the built-in `SupportedTags` table.
pub mod tag_sig {
    use crate::types::sig::Signature;

    pub const A_TO_B0: Signature = Signature::new(b"A2B0");
    pub const A_TO_B1: Signature = Signature::new(b"A2B1");
    pub const A_TO_B2: Signature = Signature::new(b"A2B2");
    pub const B_TO_A0: Signature = Signature::new(b"B2A0");
    pub const B_TO_A1: Signature = Signature::new(b"B2A1");
    pub const B_TO_A2: Signature = Signature::new(b"B2A2");
    pub const RED_COLORANT: Signature = Signature::new(b"rXYZ");
    pub const GREEN_COLORANT: Signature = Signature::new(b"gXYZ");
    pub const BLUE_COLORANT: Signature = Signature::new(b"bXYZ");
    pub const RED_TRC: Signature = Signature::new(b"rTRC");
    pub const GREEN_TRC: Signature = Signature::new(b"gTRC");
    pub const BLUE_TRC: Signature = Signature::new(b"bTRC");
    pub const CALIBRATION_DATE_TIME: Signature = Signature::new(b"calt");
    pub const CHAR_TARGET: Signature = Signature::new(b"targ");
    pub const CHROMATIC_ADAPTATION: Signature = Signature::new(b"chad");
    pub const CHROMATICITY: Signature = Signature::new(b"chrm");
    pub const COLORANT_ORDER: Signature = Signature::new(b"clro");
    pub const COLORANT_TABLE: Signature = Signature::new(b"clrt");
    pub const COLORANT_TABLE_OUT: Signature = Signature::new(b"clot");
    pub const COPYRIGHT: Signature = Signature::new(b"cprt");
    pub const DATE_TIME: Signature = Signature::new(b"dtim");
    pub const DEVICE_MFG_DESC: Signature = Signature::new(b"dmnd");
    pub const DEVICE_MODEL_DESC: Signature = Signature::new(b"dmdd");
    pub const GAMUT: Signature = Signature::new(b"gamt");
    pub const GRAY_TRC: Signature = Signature::new(b"kTRC");
    pub const LUMINANCE: Signature = Signature::new(b"lumi");
    pub const MEDIA_BLACK_POINT: Signature = Signature::new(b"bkpt");
    pub const MEDIA_WHITE_POINT: Signature = Signature::new(b"wtpt");
    pub const NAMED_COLOR2: Signature = Signature::new(b"ncl2");
    pub const PREVIEW0: Signature = Signature::new(b"pre0");
    pub const PREVIEW1: Signature = Signature::new(b"pre1");
    pub const PREVIEW2: Signature = Signature::new(b"pre2");
    pub const PROFILE_DESCRIPTION: Signature = Signature::new(b"desc");
    pub const PROFILE_SEQUENCE_DESC: Signature = Signature::new(b"pseq");
    pub const TECHNOLOGY: Signature = Signature::new(b"tech");
    pub const COLORIMETRIC_INTENT_IMAGE_STATE: Signature = Signature::new(b"ciis");
    pub const PERCEPTUAL_RENDERING_INTENT_GAMUT: Signature = Signature::new(b"rig0");
    pub const SATURATION_RENDERING_INTENT_GAMUT: Signature = Signature::new(b"rig2");
    pub const MEASUREMENT: Signature = Signature::new(b"meas");
    pub const PS2_CRD0: Signature = Signature::new(b"psd0");
    pub const PS2_CRD1: Signature = Signature::new(b"psd1");
    pub const PS2_CRD2: Signature = Signature::new(b"psd2");
    pub const PS2_CRD3: Signature = Signature::new(b"psd3");
    pub const PS2_CSA: Signature = Signature::new(b"ps2s");
    pub const PS2_RENDERING_INTENT: Signature = Signature::new(b"ps2i");
    pub const VIEWING_COND_DESC: Signature = Signature::new(b"vued");
    pub const UCR_BG: Signature = Signature::new(b"bfd ");
    pub const CRD_INFO: Signature = Signature::new(b"crdi");
    pub const D_TO_B0: Signature = Signature::new(b"D2B0");
    pub const D_TO_B1: Signature = Signature::new(b"D2B1");
    pub const D_TO_B2: Signature = Signature::new(b"D2B2");
    pub const D_TO_B3: Signature = Signature::new(b"D2B3");
    pub const B_TO_D0: Signature = Signature::new(b"B2D0");
    pub const B_TO_D1: Signature = Signature::new(b"B2D1");
    pub const B_TO_D2: Signature = Signature::new(b"B2D2");
    pub const B_TO_D3: Signature = Signature::new(b"B2D3");
    pub const SCREENING_DESC: Signature = Signature::new(b"scrd");
    pub const VIEWING_CONDITIONS: Signature = Signature::new(b"view");
    pub const SCREENING: Signature = Signature::new(b"scrn");
    pub const VCGT: Signature = Signature::new(b"vcgt");
    pub const META: Signature = Signature::new(b"meta");
    pub const PROFILE_SEQUENCE_ID: Signature = Signature::new(b"psid");
    pub const PROFILE_DESCRIPTION_ML: Signature = Signature::new(b"dscm");
    pub const CICP: Signature = Signature::new(b"cicp");
    pub const ARGYLL_ARTS: Signature = Signature::new(b"arts");
    pub const MHC2: Signature = Signature::new(b"MHC2");
}

// ---------------------------------------------------------------------------------------
// The SupportedTags table (get_descriptor)
// ---------------------------------------------------------------------------------------

/// `_cmsGetTagDescriptor` — the descriptor for a tag signature, or `None`.
///
/// Plugin-registered tags (`_cmsRegisterTagPlugin`) are searched first — newest first — then
/// the built-in `SupportedTags` table, exactly as the C lookup walks the context tag list
/// ahead of `SupportedTags`.
pub fn get_descriptor(sig: Signature) -> Option<TagDescriptor> {
    get_descriptor_for(None, sig)
}

/// [`get_descriptor`] against the tag plugins visible to `ctx`.
///
/// C's `_cmsGetTagDescriptor(ContextID, sig)` takes the context because the tag list it walks
/// is a per-context chunk (`cmstypes.c`, `_cmsRegisterTagPlugin`); this is that form, and the
/// plain one is the `ContextID == NULL` call resolving to Context 0.
pub fn get_descriptor_thr(ctx: &Context, sig: Signature) -> Option<TagDescriptor> {
    get_descriptor_for(Some(ctx), sig)
}

/// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
fn get_descriptor_for(ctx: Option<&Context>, sig: Signature) -> Option<TagDescriptor> {
    use tag_sig as tg;

    if let Some(desc) = crate::plugin::find_tag_descriptor(ctx, sig) {
        return Some(desc);
    }

    // Shared type lists.
    let lut_a2b = || vec![ty::LUT16, ty::LUT_A2B, ty::LUT8];
    let lut_b2a = || vec![ty::LUT16, ty::LUT_B2A, ty::LUT8];
    let xyz_corbis = || vec![ty::XYZ, ty::CORBIS_BROKEN_XYZ];
    let trc = || vec![ty::CURVE, ty::PARAMETRIC_CURVE, ty::MONACO_BROKEN_CURVE];
    let curve2 = || vec![ty::CURVE, ty::PARAMETRIC_CURVE];
    let text3 = || vec![ty::TEXT, ty::MLU, ty::TEXT_DESCRIPTION];
    let desc3 = || vec![ty::TEXT_DESCRIPTION, ty::MLU, ty::TEXT];

    let d = |elem_count: u32,
             supported_types: Vec<Signature>,
             decide_type: Option<fn(f64, &TagValue) -> Signature>| TagDescriptor {
        elem_count,
        supported_types,
        decide_type,
    };

    let desc = match sig {
        s if s == tg::A_TO_B0 => d(1, lut_a2b(), Some(decide_lut_a2b)),
        s if s == tg::A_TO_B1 => d(1, lut_a2b(), Some(decide_lut_a2b)),
        s if s == tg::A_TO_B2 => d(1, lut_a2b(), Some(decide_lut_a2b)),
        s if s == tg::B_TO_A0 => d(1, lut_b2a(), Some(decide_lut_b2a)),
        s if s == tg::B_TO_A1 => d(1, lut_b2a(), Some(decide_lut_b2a)),
        s if s == tg::B_TO_A2 => d(1, lut_b2a(), Some(decide_lut_b2a)),

        s if s == tg::RED_COLORANT => d(1, xyz_corbis(), Some(decide_xyz)),
        s if s == tg::GREEN_COLORANT => d(1, xyz_corbis(), Some(decide_xyz)),
        s if s == tg::BLUE_COLORANT => d(1, xyz_corbis(), Some(decide_xyz)),

        s if s == tg::RED_TRC => d(1, trc(), Some(decide_curve_type)),
        s if s == tg::GREEN_TRC => d(1, trc(), Some(decide_curve_type)),
        s if s == tg::BLUE_TRC => d(1, trc(), Some(decide_curve_type)),

        s if s == tg::CALIBRATION_DATE_TIME => d(1, vec![ty::DATE_TIME], None),
        s if s == tg::CHAR_TARGET => d(1, vec![ty::TEXT], None),

        s if s == tg::CHROMATIC_ADAPTATION => d(9, vec![ty::S15FIXED16_ARRAY], None),
        s if s == tg::CHROMATICITY => d(1, vec![ty::CHROMATICITY], None),
        s if s == tg::COLORANT_ORDER => d(1, vec![ty::COLORANT_ORDER], None),
        s if s == tg::COLORANT_TABLE => d(1, vec![ty::COLORANT_TABLE], None),
        s if s == tg::COLORANT_TABLE_OUT => d(1, vec![ty::COLORANT_TABLE], None),

        s if s == tg::COPYRIGHT => d(1, text3(), Some(decide_text_type)),
        s if s == tg::DATE_TIME => d(1, vec![ty::DATE_TIME], None),

        s if s == tg::DEVICE_MFG_DESC => d(1, desc3(), Some(decide_text_desc_type)),
        s if s == tg::DEVICE_MODEL_DESC => d(1, desc3(), Some(decide_text_desc_type)),

        s if s == tg::GAMUT => d(1, lut_b2a(), Some(decide_lut_b2a)),

        s if s == tg::GRAY_TRC => d(1, curve2(), Some(decide_curve_type)),
        s if s == tg::LUMINANCE => d(1, vec![ty::XYZ], None),

        s if s == tg::MEDIA_BLACK_POINT => d(1, xyz_corbis(), None),
        s if s == tg::MEDIA_WHITE_POINT => d(1, xyz_corbis(), None),

        s if s == tg::NAMED_COLOR2 => d(1, vec![ty::NAMED_COLOR2], None),

        s if s == tg::PREVIEW0 => d(1, lut_b2a(), Some(decide_lut_b2a)),
        s if s == tg::PREVIEW1 => d(1, lut_b2a(), Some(decide_lut_b2a)),
        s if s == tg::PREVIEW2 => d(1, lut_b2a(), Some(decide_lut_b2a)),

        s if s == tg::PROFILE_DESCRIPTION => d(1, desc3(), Some(decide_text_desc_type)),
        s if s == tg::PROFILE_SEQUENCE_DESC => d(1, vec![ty::PROFILE_SEQUENCE_DESC], None),
        s if s == tg::TECHNOLOGY => d(1, vec![ty::SIGNATURE], None),

        s if s == tg::COLORIMETRIC_INTENT_IMAGE_STATE => d(1, vec![ty::SIGNATURE], None),
        s if s == tg::PERCEPTUAL_RENDERING_INTENT_GAMUT => d(1, vec![ty::SIGNATURE], None),
        s if s == tg::SATURATION_RENDERING_INTENT_GAMUT => d(1, vec![ty::SIGNATURE], None),

        s if s == tg::MEASUREMENT => d(1, vec![ty::MEASUREMENT], None),

        s if s == tg::PS2_CRD0 => d(1, vec![ty::DATA], None),
        s if s == tg::PS2_CRD1 => d(1, vec![ty::DATA], None),
        s if s == tg::PS2_CRD2 => d(1, vec![ty::DATA], None),
        s if s == tg::PS2_CRD3 => d(1, vec![ty::DATA], None),
        s if s == tg::PS2_CSA => d(1, vec![ty::DATA], None),
        s if s == tg::PS2_RENDERING_INTENT => d(1, vec![ty::DATA], None),

        s if s == tg::VIEWING_COND_DESC => d(1, desc3(), Some(decide_text_desc_type)),

        s if s == tg::UCR_BG => d(1, vec![ty::UCR_BG], None),
        s if s == tg::CRD_INFO => d(1, vec![ty::CRD_INFO], None),

        s if s == tg::D_TO_B0 => d(1, vec![ty::MPE], None),
        s if s == tg::D_TO_B1 => d(1, vec![ty::MPE], None),
        s if s == tg::D_TO_B2 => d(1, vec![ty::MPE], None),
        s if s == tg::D_TO_B3 => d(1, vec![ty::MPE], None),
        s if s == tg::B_TO_D0 => d(1, vec![ty::MPE], None),
        s if s == tg::B_TO_D1 => d(1, vec![ty::MPE], None),
        s if s == tg::B_TO_D2 => d(1, vec![ty::MPE], None),
        s if s == tg::B_TO_D3 => d(1, vec![ty::MPE], None),

        s if s == tg::SCREENING_DESC => d(1, vec![ty::TEXT_DESCRIPTION], None),
        s if s == tg::VIEWING_CONDITIONS => d(1, vec![ty::VIEWING_CONDITIONS], None),

        s if s == tg::SCREENING => d(1, vec![ty::SCREENING], None),
        s if s == tg::VCGT => d(1, vec![ty::VCGT], None),
        s if s == tg::META => d(1, vec![ty::DICT], None),
        s if s == tg::PROFILE_SEQUENCE_ID => d(1, vec![ty::PROFILE_SEQUENCE_ID], None),

        s if s == tg::PROFILE_DESCRIPTION_ML => d(1, vec![ty::MLU], None),
        s if s == tg::CICP => d(1, vec![ty::CICP], None),

        s if s == tg::ARGYLL_ARTS => d(9, vec![ty::S15FIXED16_ARRAY], None),
        s if s == tg::MHC2 => d(1, vec![ty::MHC2], None),

        _ => return None,
    };
    Some(desc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_count_and_lookup() {
        // A representative sample across families.
        assert_eq!(get_descriptor(tag_sig::A_TO_B0).unwrap().supported_types.len(), 3);
        assert_eq!(get_descriptor(tag_sig::CHROMATIC_ADAPTATION).unwrap().elem_count, 9);
        assert_eq!(get_descriptor(tag_sig::MHC2).unwrap().supported_types, vec![ty::MHC2]);
        assert!(get_descriptor(Signature::new(b"zzzz")).is_none());
    }

    #[test]
    fn decide_curve_type_switches_on_version() {
        use crate::color::gamma::ToneCurve;
        let para = TagValue::Curve(ToneCurve::build_parametric(4, &[2.4, 0.9, 0.05, 0.08, 0.04, 0.0, 0.0]).unwrap());
        assert_eq!(decide_curve_type(2.0, &para), ty::CURVE);
        assert_eq!(decide_curve_type(4.3, &para), ty::PARAMETRIC_CURVE);
        let tab = TagValue::Curve(ToneCurve::build_tabulated_16(4, &[0, 100, 200, 300]).unwrap());
        assert_eq!(decide_curve_type(4.3, &tab), ty::CURVE);
    }

    #[test]
    fn decide_lut_type_switches_on_version() {
        let mut lut = crate::pipeline::Pipeline::alloc(3, 3).unwrap();
        lut.insert_stage(crate::pipeline::StageLoc::End, crate::pipeline::Stage::alloc_identity(3));
        let v = TagValue::Pipeline(lut);
        assert_eq!(decide_lut_a2b(4.3, &v), ty::LUT_A2B);
        assert_eq!(decide_lut_a2b(2.0, &v), ty::LUT16);
        assert_eq!(decide_lut_b2a(4.3, &v), ty::LUT_B2A);
    }
}
