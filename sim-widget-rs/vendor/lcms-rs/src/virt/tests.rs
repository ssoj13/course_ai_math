//! Tests for the virtual-profile constructors (`cmsvirt.c` port).

use super::*;
use crate::cmm::transform::create_transform;
use crate::color::{D50X, D50Y, D50Z};
use crate::color::pcs::pt;
use crate::profile::read_hl::INTENT_RELATIVE_COLORIMETRIC;
use crate::tags::TagValue;
use crate::types::format::FormatBuilder;

fn rgb8() -> crate::types::format::PixelFormat {
    FormatBuilder::new().colorspace(pt::RGB as u32).channels(3).bytes(1).build()
}

/// A named-colour transform takes the `CreateNamedColorDevicelink` branch
/// (`cmsvirt.c:1216-1222`) instead of the LUT path: the result is a NamedColor-class profile
/// carrying `ncl2`, with the colorant count re-pointed at the transform's OUTPUT space
/// (`:1104`) and every spot colour's device colorant recomputed by running its index through
/// the transform (`:1111-1114`).
#[test]
fn named_color_transform_yields_a_named_color_devicelink() {
    use crate::cmm::transform::create_multiprofile_transform;
    use crate::named::namedcolor::NamedColorList;
    use crate::profile::read_hl::INTENT_PERCEPTUAL;
    use crate::tags::descriptor::tag_sig;
    use crate::MAX_CHANNELS;

    let ctx = Context::new();

    // A synthetic named-colour source: index -> Lab PCS. Four colorants, so the re-point to the
    // 3-channel RGB output space is observable.
    let mut list = NamedColorList::new(4, "pre_", "_suf").expect("named color list");
    let mut dev = [0u16; MAX_CHANNELS];
    dev[..4].copy_from_slice(&[100, 200, 300, 400]);
    list.append("red", [0x8000, 0xC000, 0x9000], dev);
    list.append("green", [0x6000, 0x4000, 0xB000], dev);
    list.append("blue", [0x4000, 0x7000, 0x3000], dev);

    let mut named = Profile::create_placeholder(ctx.clone());
    named.set_device_class(class::NAMED_COLOR);
    named.set_color_space(colorspace::RGB);
    named.set_pcs(colorspace::LAB);
    named.write_tag(tag_sig::NAMED_COLOR2, TagValue::NamedColor(list)).expect("ncl2");

    let mut dest = create_srgb_profile().expect("sRGB");
    let index_fmt = FormatBuilder::new().channels(1).bytes(2).build();
    let rgb16 = FormatBuilder::new().colorspace(pt::RGB as u32).channels(3).bytes(2).build();
    let mut xf = create_multiprofile_transform(
        &ctx,
        &mut [&mut named, &mut dest],
        index_fmt,
        rgb16,
        INTENT_PERCEPTUAL,
        0,
    )
    .expect("named-colour transform");

    let mut dl = transform2devicelink(&mut xf, 4.3, 0).expect("named-colour devicelink");

    // Header per `cmsvirt.c:1088-1091`: NamedColor class, exit colour space, Lab PCS.
    assert_eq!(dl.device_class(), class::NAMED_COLOR);
    assert_eq!(dl.color_space(), colorspace::RGB);
    assert_eq!(dl.pcs(), colorspace::LAB);

    let TagValue::NamedColor(out) = dl.read_tag(tag_sig::NAMED_COLOR2).expect("ncl2 back") else {
        panic!("ncl2 must decode to a named colour list");
    };
    assert_eq!(out.count(), 3, "every spot colour survives");
    assert_eq!(
        out.colorant_count(),
        3,
        "ColorantCount must follow the pipeline OUTPUT channels, not the source's 4"
    );
    assert_eq!(out.prefix(), "pre_");
    assert_eq!(out.suffix(), "_suf");

    // The colorants must be RECOMPUTED, not the source's [100, 200, 300, 400].
    let colorants: Vec<[u16; 3]> = (0..3)
        .map(|i| {
            let c = out.get(i).expect("colour present");
            [c.device_colorant[0], c.device_colorant[1], c.device_colorant[2]]
        })
        .collect();
    assert!(
        colorants.iter().all(|c| *c != [100, 200, 300]),
        "device colorants must be re-run through the transform, got {colorants:?}"
    );
    assert!(
        colorants.iter().any(|c| *c != colorants[0]),
        "distinct Lab inputs must give distinct RGB colorants, got {colorants:?}"
    );
}

/// A created sRGB profile round-trips through memory and carries sane desc / wtpt / rXYZ / rTRC.
#[test]
fn srgb_roundtrips_through_memory() {
    let mut srgb = create_srgb_profile().expect("sRGB profile");

    // Header shape: Display / RGB / XYZ, version 4.4.
    assert_eq!(srgb.device_class(), class::DISPLAY);
    assert_eq!(srgb.color_space(), colorspace::RGB);
    assert_eq!(srgb.pcs(), colorspace::XYZ);
    assert!((srgb.profile_version() - 4.4).abs() < 1e-6);

    let bytes = srgb.save_to_mem().expect("serialize sRGB");
    let mut reopened = Profile::open_from_mem(Context::new(), &bytes).expect("reopen sRGB");

    // desc — an MLU carrying the (overwritten) "sRGB built-in" description.
    match reopened.read_tag(tag_sig::PROFILE_DESCRIPTION).expect("desc") {
        TagValue::Mlu(_) => {}
        other => panic!("desc is not an MLU: {other:?}"),
    }

    // wtpt — D50 media white point.
    match reopened.read_tag(tag_sig::MEDIA_WHITE_POINT).expect("wtpt") {
        TagValue::Xyz(xyz) => {
            assert!((xyz.x - D50X).abs() < 1e-4, "wtpt.x = {}", xyz.x);
            assert!((xyz.y - D50Y).abs() < 1e-4, "wtpt.y = {}", xyz.y);
            assert!((xyz.z - D50Z).abs() < 1e-4, "wtpt.z = {}", xyz.z);
        }
        other => panic!("wtpt is not XYZ: {other:?}"),
    }

    // rXYZ — the red colorant, a plausible positive XYZ.
    match reopened.read_tag(tag_sig::RED_COLORANT).expect("rXYZ") {
        TagValue::Xyz(xyz) => {
            assert!(xyz.x > 0.0 && xyz.x < 1.0, "rXYZ.x = {}", xyz.x);
            assert!(xyz.y > 0.0 && xyz.y < 1.0, "rXYZ.y = {}", xyz.y);
        }
        other => panic!("rXYZ is not XYZ: {other:?}"),
    }

    // rTRC — the red tone curve (a curve object).
    match reopened.read_tag(tag_sig::RED_TRC).expect("rTRC") {
        TagValue::Curve(_) => {}
        other => panic!("rTRC is not a curve: {other:?}"),
    }
}

/// Lab4 is Abstract / Lab / Lab at version 4.4.
#[test]
fn lab4_is_abstract_lab_v44() {
    let p = create_lab4_profile(None).expect("Lab4 profile");
    assert_eq!(p.device_class(), class::ABSTRACT);
    assert_eq!(p.color_space(), colorspace::LAB);
    assert_eq!(p.pcs(), colorspace::LAB);
    assert!((p.profile_version() - 4.4).abs() < 1e-6, "version = {}", p.profile_version());
}

/// Lab2 is Abstract / Lab / Lab at version 2.1 (the intentional quirk).
#[test]
fn lab2_is_version_2_1() {
    let p = create_lab2_profile(None).expect("Lab2 profile");
    assert_eq!(p.device_class(), class::ABSTRACT);
    assert_eq!(p.color_space(), colorspace::LAB);
    assert_eq!(p.pcs(), colorspace::LAB);
    assert!((p.profile_version() - 2.1).abs() < 1e-6, "version = {}", p.profile_version());
}

/// A gray profile has the expected header shape and a decodable kTRC.
#[test]
fn gray_profile_shape_and_trc() {
    let gamma = ToneCurve::build_gamma(2.2).expect("gamma");
    let d50 = d50_xyy();
    let mut p = create_gray_profile(Some(&d50), Some(&gamma)).expect("gray profile");
    assert_eq!(p.device_class(), class::DISPLAY);
    assert_eq!(p.color_space(), colorspace::GRAY);
    assert_eq!(p.pcs(), colorspace::XYZ);

    let bytes = p.save_to_mem().expect("serialize gray");
    let mut reopened = Profile::open_from_mem(Context::new(), &bytes).expect("reopen gray");
    match reopened.read_tag(tag_sig::GRAY_TRC).expect("kTRC") {
        TagValue::Curve(_) => {}
        other => panic!("kTRC is not a curve: {other:?}"),
    }
}

/// A created sRGB profile transforms to itself as a near-identity (relative colorimetric).
#[test]
fn srgb_self_transform_is_near_identity() {
    let ctx = Context::new();
    let mut src = create_srgb_profile().expect("sRGB src");
    let mut dst = create_srgb_profile().expect("sRGB dst");

    let xform = create_transform(
        &ctx,
        &mut src,
        rgb8(),
        &mut dst,
        rgb8(),
        INTENT_RELATIVE_COLORIMETRIC,
        0,
    )
    .expect("build sRGB->sRGB transform");

    let input: [u8; 9] = [0, 0, 0, 128, 64, 200, 255, 255, 255];
    let mut output = [0u8; 9];
    xform.do_transform(&input, &mut output, 3);

    for (i, (&a, &b)) in input.iter().zip(output.iter()).enumerate() {
        let d = (a as i32 - b as i32).abs();
        assert!(d <= 3, "channel {i}: in {a} vs out {b} (delta {d})");
    }
}

/// The BCHSW abstract profile builds (no white-point shift path).
#[test]
fn bchsw_abstract_builds() {
    let p = create_bchsw_abstract_profile(11, 0.0, 1.0, 0.0, 0.0, 6504, 6504)
        .expect("BCHSW profile");
    assert_eq!(p.device_class(), class::ABSTRACT);
    assert_eq!(p.color_space(), colorspace::LAB);
    assert_eq!(p.pcs(), colorspace::LAB);
}

/// The NULL profile is Output / Gray / Lab.
#[test]
fn null_profile_shape() {
    let p = create_null_profile().expect("NULL profile");
    assert_eq!(p.device_class(), class::OUTPUT);
    assert_eq!(p.color_space(), colorspace::GRAY);
    assert_eq!(p.pcs(), colorspace::LAB);
}

/// The OkLab profile is ColorSpace / 3CLR / XYZ at 4.4.
#[test]
fn oklab_profile_shape() {
    let p = create_oklab_profile(Context::new()).expect("OkLab profile");
    assert_eq!(p.device_class(), class::COLORSPACE);
    assert_eq!(p.color_space(), colorspace::CLR3);
    assert_eq!(p.pcs(), colorspace::XYZ);
}

/// `find_combination` recognises the classic V2 curves→CLUT→curves Lut16 shape.
#[test]
fn find_combination_matches_v2_lut16() {
    let mut lut = Pipeline::alloc(3, 3).unwrap();
    lut.insert_stage(StageLoc::End, Stage::alloc_identity_curves(3).unwrap());
    lut.insert_stage(StageLoc::End, Stage::alloc_identity_clut(3).unwrap());
    lut.insert_stage(StageLoc::End, Stage::alloc_identity_curves(3).unwrap());

    // Note: identity curves report `cvst` as their *type* signature (Implements is `idn `).
    let tab = find_combination(&lut, false, tag_sig::A_TO_B0).expect("a V2 combination");
    assert_eq!(tab.mpe_types, &[STAGE_CURVE_SET, STAGE_CLUT, STAGE_CURVE_SET]);
    assert!(!tab.is_v4);
}

/// `is_pcs` / `fix_color_spaces` — link class for device↔device, abstract for PCS↔PCS.
#[test]
fn fix_color_spaces_link_and_abstract() {
    assert!(is_pcs(colorspace::XYZ) && is_pcs(colorspace::LAB));
    assert!(!is_pcs(colorspace::RGB));

    let mut p = Profile::create_placeholder(Context::new());
    // Without GUESSDEVICECLASS: always link class.
    fix_color_spaces(&mut p, colorspace::RGB, colorspace::XYZ, false);
    assert_eq!(p.device_class(), class::LINK);

    // With GUESSDEVICECLASS and PCS↔PCS: abstract.
    fix_color_spaces(&mut p, colorspace::LAB, colorspace::XYZ, true);
    assert_eq!(p.device_class(), class::ABSTRACT);

    // With GUESSDEVICECLASS and device→PCS: input class.
    fix_color_spaces(&mut p, colorspace::RGB, colorspace::XYZ, true);
    assert_eq!(p.device_class(), class::INPUT);
}

/// `cmsTransform2DeviceLink` — a transform over a discrete-MPE pipeline (here an RGB
/// linearization devicelink, a clean `[curves]` MPE chain) becomes a Link-class devicelink
/// profile that carries a writable `AToB0` LUT and round-trips through memory.
///
/// The transform is built with `cmsFLAGS_NOOPTIMIZE` so the pipeline keeps serializable MPE
/// stages: the built-in optimizers collapse a pipeline into a single fused evaluator stage which
/// no `AToB`/`BToA` tag type can represent (a property of `cmm::optimize`, orthogonal to this
/// function — see `DEFERRED.md`).
#[test]
fn transform2devicelink_builds_link_profile() {
    use crate::cmm::optimize::FLAGS_NOOPTIMIZE;
    use crate::cmm::transform::create_multiprofile_transform;
    use crate::color::gamma::ToneCurve;
    use crate::profile::read_hl::INTENT_RELATIVE_COLORIMETRIC;

    let ctx = Context::new();
    let rgb16 = FormatBuilder::new().colorspace(pt::RGB as u32).channels(3).bytes(2).build();

    // An RGB linearization devicelink: its AToB0 is a plain per-channel curve set.
    let g = ToneCurve::build_gamma(2.2).expect("gamma curve");
    let mut src = create_linearization_device_link(colorspace::RGB, &[g.clone(), g.clone(), g])
        .expect("linearization devicelink");
    let mut xf = create_multiprofile_transform(
        &ctx,
        &mut [&mut src],
        rgb16,
        rgb16,
        INTENT_RELATIVE_COLORIMETRIC,
        FLAGS_NOOPTIMIZE,
    )
    .expect("build transform");

    let mut dl = transform2devicelink(&mut xf, 4.3, 0).expect("transform2devicelink");
    assert_eq!(dl.device_class(), class::LINK);
    assert_eq!(dl.color_space(), colorspace::RGB);
    assert_eq!(dl.pcs(), colorspace::RGB);
    assert!(dl.is_tag(tag_sig::A_TO_B0), "devicelink must carry an AToB0 LUT");

    // Round-trips through memory, still a link with the LUT readable back.
    let bytes = dl.save_to_mem().expect("serialize devicelink");
    let mut reopened = Profile::open_from_mem(Context::new(), &bytes).expect("reopen devicelink");
    assert_eq!(reopened.device_class(), class::LINK);
    match reopened.read_tag(tag_sig::A_TO_B0).expect("read AToB0") {
        TagValue::Pipeline(p) => {
            assert_eq!(p.input_channels(), 3);
            assert_eq!(p.output_channels(), 3);
        }
        other => panic!("AToB0 is not a pipeline: {other:?}"),
    }
}
