//! Round-trip and fidelity tests for the profile I/O layer.
//!
//! The gate is byte-exactness: a cooked profile saved, reopened, and re-saved must reproduce
//! identical bytes (the reopened tags are blind-copied from the original image), and header /
//! tag values must survive the round trip. `computeID` must be deterministic and tag-sensitive.

use super::*;
use crate::color::gamma::ToneCurve;
use crate::tags::descriptor::tag_sig;
use crate::tags::TagValue;
use crate::types::color::CIEXYZ;
use crate::types::sig::{class, colorspace};

/// Build a small but non-trivial V4 profile: header fields plus an XYZ tag and a curve tag.
fn sample_profile() -> Profile {
    let mut p = Profile::create_placeholder(Context::new());
    p.set_profile_version(4.3);
    p.set_device_class(class::OUTPUT);
    p.set_color_space(colorspace::RGB);
    p.set_pcs(colorspace::XYZ);
    p.set_header_manufacturer(0x41424344); // 'ABCD'
    p.set_header_model(0x1234_5678);
    p.set_header_rendering_intent(1);
    p.set_header_flags(0x0000_0002);
    p.set_header_attributes(0x0000_0001_0000_0000);

    p.write_tag(
        tag_sig::MEDIA_WHITE_POINT,
        TagValue::Xyz(CIEXYZ { x: 0.9642, y: 1.0, z: 0.8249 }),
    )
    .unwrap();
    p.write_tag(
        tag_sig::GRAY_TRC,
        TagValue::Curve(ToneCurve::build_gamma(2.2).unwrap()),
    )
    .unwrap();
    p
}

#[test]
fn write_save_reopen_read_back() {
    let mut p = sample_profile();
    let bytes = p.save_to_mem().unwrap();

    let mut reopened = Profile::open_from_mem(Context::new(), &bytes).unwrap();

    // The XYZ tag comes back within the 15.16 quantization error.
    match reopened.read_tag(tag_sig::MEDIA_WHITE_POINT).unwrap() {
        TagValue::Xyz(xyz) => {
            assert!((xyz.x - 0.9642).abs() < 1.0 / 65536.0);
            assert!((xyz.y - 1.0).abs() < 1.0 / 65536.0);
            assert!((xyz.z - 0.8249).abs() < 1.0 / 65536.0);
        }
        other => panic!("expected XYZ, got {other:?}"),
    }

    // The curve tag comes back as a curve.
    assert!(matches!(reopened.read_tag(tag_sig::GRAY_TRC).unwrap(), TagValue::Curve(_)));

    // Header fields survive the round trip.
    assert_eq!(reopened.device_class(), class::OUTPUT);
    assert_eq!(reopened.color_space(), colorspace::RGB);
    assert_eq!(reopened.pcs(), colorspace::XYZ);
    assert_eq!(reopened.header_manufacturer(), 0x4142_4344);
    assert_eq!(reopened.header_model(), 0x1234_5678);
    assert_eq!(reopened.header_rendering_intent(), 1);
    assert_eq!(reopened.header_flags(), 0x0000_0002);
    assert_eq!(reopened.header_attributes(), 0x0000_0001_0000_0000);
    assert_eq!(reopened.header_creation_datetime(), p.header_creation_datetime());
    assert!((reopened.profile_version() - 4.3).abs() < 1e-9);
}

#[test]
fn save_reopen_save_is_byte_identical() {
    let mut p = sample_profile();
    let bytes1 = p.save_to_mem().unwrap();

    let mut reopened = Profile::open_from_mem(Context::new(), &bytes1).unwrap();
    let bytes2 = reopened.save_to_mem().unwrap();

    assert_eq!(bytes1, bytes2, "reopen + re-save must reproduce identical bytes");
}

#[test]
fn header_fields_roundtrip() {
    let mut p = Profile::create_placeholder(Context::new());
    p.set_profile_version(2.4);
    p.set_device_class(class::DISPLAY);
    p.set_color_space(colorspace::CMYK);
    p.set_pcs(colorspace::LAB);
    p.set_header_cmm(0x74657374); // 'test'
    p.set_header_manufacturer(0xDEAD_BEEF);
    p.set_header_model(0xCAFE_BABE);
    p.set_header_rendering_intent(3);
    p.set_header_flags(0xFFFF_0000);
    p.set_header_attributes(0x8000_0000_0000_0001);
    p.set_header_profile_id([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);

    let bytes = p.save_to_mem().unwrap();
    let reopened = Profile::open_from_mem(Context::new(), &bytes).unwrap();

    assert!((reopened.profile_version() - 2.4).abs() < 1e-9);
    assert_eq!(reopened.encoded_icc_version(), p.encoded_icc_version());
    assert_eq!(reopened.device_class(), class::DISPLAY);
    assert_eq!(reopened.color_space(), colorspace::CMYK);
    assert_eq!(reopened.pcs(), colorspace::LAB);
    assert_eq!(reopened.header_cmm(), 0x7465_7374);
    assert_eq!(reopened.header_manufacturer(), 0xDEAD_BEEF);
    assert_eq!(reopened.header_model(), 0xCAFE_BABE);
    assert_eq!(reopened.header_rendering_intent(), 3);
    assert_eq!(reopened.header_flags(), 0xFFFF_0000);
    assert_eq!(reopened.header_attributes(), 0x8000_0000_0000_0001);
    assert_eq!(reopened.header_profile_id(), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
}

#[test]
fn compute_id_is_deterministic_and_tag_sensitive() {
    let mut p = sample_profile();

    p.compute_id().unwrap();
    let id1 = p.profile_id();

    // Re-computing on the unchanged profile yields the same digest.
    p.compute_id().unwrap();
    let id2 = p.profile_id();
    assert_eq!(id1, id2, "computeID must be deterministic");
    assert_ne!(id1, [0u8; 16], "a real profile hashes to a non-zero ID");

    // Changing a tag changes the digest.
    p.write_tag(
        tag_sig::MEDIA_WHITE_POINT,
        TagValue::Xyz(CIEXYZ { x: 0.5, y: 0.5, z: 0.5 }),
    )
    .unwrap();
    p.compute_id().unwrap();
    assert_ne!(p.profile_id(), id1, "computeID must change when a tag changes");
}

#[test]
fn compute_id_excludes_flags_intent_and_id() {
    // Two profiles differing only in flags / rendering intent / stored ID must hash equally,
    // because computeID zeroes those three header fields before hashing.
    let mut a = sample_profile();
    let mut b = sample_profile();
    b.set_header_flags(0x1234_5678);
    b.set_header_rendering_intent(2);
    b.set_header_profile_id([9u8; 16]);

    a.compute_id().unwrap();
    b.compute_id().unwrap();
    assert_eq!(a.profile_id(), b.profile_id());
}

#[test]
fn raw_tag_roundtrips_verbatim() {
    let mut p = Profile::create_placeholder(Context::new());
    p.set_device_class(class::DISPLAY);
    let blob = [0xDEu8, 0xAD, 0xBE, 0xEF, 0x00, 0x11, 0x22, 0x33];
    p.write_raw_tag(tag_sig::PS2_CRD0, &blob).unwrap();

    let bytes = p.save_to_mem().unwrap();
    let mut reopened = Profile::open_from_mem(Context::new(), &bytes).unwrap();

    // The raw bytes are preserved exactly (the tag was written verbatim, no alignment inside).
    let read_back = reopened.read_raw_tag(tag_sig::PS2_CRD0).unwrap();
    assert_eq!(read_back, blob);
}

#[test]
fn linked_tags_share_one_block() {
    let mut p = sample_profile();
    // Link B2A0 to A2B0? Instead link two colorant/curve tags that share a block.
    p.write_tag(tag_sig::RED_TRC, TagValue::Curve(ToneCurve::build_gamma(1.8).unwrap()))
        .unwrap();
    p.link_tag(tag_sig::GREEN_TRC, tag_sig::RED_TRC).unwrap();
    p.link_tag(tag_sig::BLUE_TRC, tag_sig::RED_TRC).unwrap();

    let bytes = p.save_to_mem().unwrap();
    let reopened = Profile::open_from_mem(Context::new(), &bytes).unwrap();

    // All three TRC tags resolve to the same on-disk offset/size (the link was detected).
    let find = |sig: Signature| -> (u32, u32) {
        let t = reopened.tags().iter().find(|t| t.sig == sig).unwrap();
        match t.data {
            TagData::OnDisk { offset, size } => (offset, size),
            _ => panic!("expected on-disk tag"),
        }
    };
    // Green/Blue were detected as links to Red (identical offset+size); Red carries the data.
    let red = find(tag_sig::RED_TRC);
    assert_eq!(find(tag_sig::GREEN_TRC), red);
    assert_eq!(find(tag_sig::BLUE_TRC), red);
}

/// `cmsGetTagOffsetAndSize` must keep reporting the on-disk block AFTER the tag has been read.
///
/// C never clears `TagOffsets[i]`/`TagSizes[i]` on a read - `cmsReadTag` only fills
/// `TagPtrs[i]` - and zeroes them in exactly two places, `cmsWriteTag` (`cmsio0.c:2003`) and
/// `cmsWriteRawTag` (`:2210`). Deriving the answer from the payload enum instead would report
/// `(0, 0)` for every tag anyone had looked at, which is silently wrong rather than absent.
#[test]
fn tag_offset_and_size_survives_reading_the_tag() {
    let bytes = sample_profile().save_to_mem().unwrap();
    let mut p = Profile::open_from_mem(Context::new(), &bytes).unwrap();

    let slot = p.tags().iter().position(|t| t.sig == tag_sig::GRAY_TRC).expect("kTRC present");
    let before = p.tag_offset_and_size(slot).expect("slot in range");
    assert!(before.0 != 0 && before.1 != 0, "an on-disk tag must report a real block: {before:?}");

    // Reading decodes and caches the payload; the on-disk provenance must be unaffected.
    let v = p.read_tag(tag_sig::GRAY_TRC).expect("read kTRC");
    assert_eq!(p.tag_offset_and_size(slot), Some(before), "a read must not clear the offset");

    // Writing does clear it - the slot no longer maps to any region of the original file.
    p.write_tag(tag_sig::GRAY_TRC, v).expect("write kTRC");
    assert_eq!(p.tag_offset_and_size(slot), Some((0, 0)), "cmsWriteTag zeroes the offset");

    // Out of range stays C's FALSE.
    assert_eq!(p.tag_offset_and_size(9999), None);
}
