//! End-to-end verification against real ICC profiles shipped with the C reference.
//!
//! This is the strongest available parity check short of golden vectors: it loads genuine
//! profiles produced by other tools, reads **every** tag (exercising the real tag-type
//! handlers on real data), and asserts the port's save is idempotent (save → reopen →
//! save is byte-identical) and that ProfileID computation is deterministic.

use std::path::PathBuf;

use lcms_rs::context::Context;
use lcms_rs::error::ErrorCode;
use lcms_rs::profile::Profile;
use lcms_rs::types::sig::Signature;

fn ref_profile(rel: &str) -> Vec<u8> {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/icc");
    p.push(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// Load a profile, read every tag it declares, and confirm save is idempotent.
fn exercise(name: &str) {
    let bytes = ref_profile(name);
    let mut prof = Profile::open_from_mem(Context::new(), &bytes)
        .unwrap_or_else(|e| panic!("open {name}: {e}"));

    // Header fields decoded to sane, printable 4CC signatures.
    let cs = prof.color_space();
    let pcs = prof.pcs();
    assert_ne!(cs.as_u32(), 0, "{name}: empty color space");
    assert_ne!(pcs.as_u32(), 0, "{name}: empty PCS");
    assert!(prof.profile_version() >= 2.0, "{name}: version {}", prof.profile_version());

    // Read EVERY tag — this runs the actual type handlers over real serialized data.
    let n = prof.tag_count();
    assert!(n > 0, "{name}: no tags");
    let sigs: Vec<Signature> = (0..n).map(|i| prof.tag_signature(i)).collect();
    let mut read = 0;
    for sig in sigs {
        // read_tag follows links and dispatches to the type handler. `UnknownExtension`
        // means lcms has no descriptor for this (private/vendor) tag either — the C
        // `cmsReadTag` returns NULL for it too; the tag is still preserved on save via
        // blind-copy. Any OTHER error is a real handler/serialization bug on genuine data.
        match prof.read_tag(sig) {
            Ok(_) => read += 1,
            Err(e) if e.code == ErrorCode::UnknownExtension => { /* unsupported tag, as in C */ }
            Err(e) => panic!("{name}: read tag {sig} failed: {e}"),
        }
    }
    assert!(read > 0, "{name}: read {read}/{n} tags (none readable)");

    // Idempotent save: our-save → reopen → our-save must be byte-identical.
    let saved1 = prof.save_to_mem().unwrap_or_else(|e| panic!("{name}: save1: {e}"));
    let mut prof2 = Profile::open_from_mem(Context::new(), &saved1)
        .unwrap_or_else(|e| panic!("{name}: reopen: {e}"));
    let saved2 = prof2.save_to_mem().unwrap_or_else(|e| panic!("{name}: save2: {e}"));
    assert_eq!(saved1, saved2, "{name}: save is not idempotent");

    // ProfileID is deterministic.
    let mut a = Profile::open_from_mem(Context::new(), &saved1).unwrap();
    let mut b = Profile::open_from_mem(Context::new(), &saved1).unwrap();
    a.compute_id().unwrap();
    b.compute_id().unwrap();
    assert_eq!(a.profile_id(), b.profile_id(), "{name}: ProfileID not deterministic");
    assert_ne!(a.profile_id(), [0u8; 16], "{name}: ProfileID is zero");
}

#[test]
fn crayons_rgb_display_profile() {
    // RGB matrix-shaper display profile (mntr/RGB/XYZ): colorants, TRCs, chad, desc.
    // (CMYK/Lab A2B/B2A CLUT coverage comes from test1/test2.icc in `plugin_test_profiles`.)
    exercise("crayons.icc");
}

#[test]
fn ibm_t61_rgb_display_profile() {
    // RGB display profile: matrix/shaper (rXYZ/gXYZ/bXYZ, rTRC/gTRC/bTRC), chad, desc.
    exercise("ibm-t61.icc");
}

#[test]
fn fuzzer_corpus_is_handled_without_panic() {
    // A fuzzer-corpus profile may contain deliberately malformed tags (e.g. a tag whose
    // size is smaller than the 8-byte type base). The port must handle every tag
    // gracefully — Ok or a typed Err, NEVER a panic / OOB read (matching C's NULL return).
    let bytes = ref_profile("alltags.icc");
    let Ok(mut prof) = Profile::open_from_mem(Context::new(), &bytes) else {
        return; // a corpus file may legitimately fail header validation — that's fine.
    };
    let n = prof.tag_count();
    let sigs: Vec<Signature> = (0..n).map(|i| prof.tag_signature(i)).collect();
    for sig in sigs {
        // Result either way; the point is it does not panic.
        let _ = prof.read_tag(sig);
    }
    // Even with unreadable tags, saving must not panic (unreadable tags are blind-copied).
    let _ = prof.save_to_mem();
}

#[test]
fn plugin_test_profiles() {
    for n in ["test0.icc", "test1.icc", "test2.icc", "test3.icc", "test5.icc"] {
        exercise(n);
    }
}
