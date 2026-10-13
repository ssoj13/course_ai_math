//! RGN (Region of Interest, max-shift method, marker `0xFF5E`) decode regression test.
//!
//! RGN signals a region-of-interest max-shift (ITU-T T.800 §A.6.3 / §H.1): at encode the ROI
//! coefficients are scaled up so they occupy higher bitplanes than the background; the decoder
//! reconstructs against the full bit budget and shifts the background coefficients back up to their
//! true magnitude (OpenHTJ2K's `j2k_dequant_rev_scalar` convention — see `Codeblock::apply_roi`).
//! OpenJPH skips RGN entirely and OpenJPEG refuses it in the HT path, so this is a native feature.
//!
//! Validation: the conformance vector is reversible, so the decode is bit-exact against the
//! OpenHTJ2K oracle reference once we replicate the oracle's image-writer transform (level-shift a
//! signed component by `2^(bit_depth-1)`, then clamp to `[0, 2^bit_depth - 1]` — `main_dec.cpp`).
//! jph-rs, like OpenJPH, returns the raw reconstructed samples; the clamp lives in the consumer.
//!
//! Generate the reference with the dev harness:
//!   open_htj2k_dec -i ds0_ht_03_b11.j2k -o ref.pgm     ==> ref_00.pgm (per-component suffix)
//! Point `RGNVEC` at the `.j2k` codestream and `RGNREF` at the oracle `*_00.pgm`. Absent env =
//! skip (CI without the conformance set stays green). `ds0_ht_03` also exercises 8 quality layers,
//! POC, SOP, 2×2 tiles, and a tile split into one data-bearing plus three empty tile-parts, so it
//! doubles as a composed-feature regression.

use std::path::PathBuf;
mod common;

/// Load a binary PGM (`P5`). Samples are 1 byte when `maxval <= 255`, else 2 bytes big-endian.
fn load_pgm(path: &PathBuf) -> Vec<i32> {
    common::pgm(path).2
}


#[test]
#[ignore = "needs external conformance fixtures (env vars): run with --ignored"]
fn rgn_decode_matches_oracle_bit_exact() {
    let (Ok(vec_path), Ok(ref_path)) = (std::env::var("RGNVEC"), std::env::var("RGNREF")) else {
        panic!("RGNVEC/RGNREF unset — skipping RGN validation");
    };
    let img = jph_rs::mem::decode(&std::fs::read(&vec_path).unwrap());
    let expected = load_pgm(&PathBuf::from(&ref_path));
    let plane = &img.planes[0];

    // Replicate the oracle's PGM writer: signed components are level-shifted by 2^(bd-1), then
    // every sample is clamped to the nominal range. jph-rs returns raw reconstructed samples (the
    // ROI max-shift reconstruction can legitimately land one step outside the range at boundaries,
    // exactly as the oracle's own reconstruction does before it clamps).
    let lvl: i32 = if plane.signed { 1 << (plane.bit_depth - 1) } else { 0 };
    let maxval: i32 = (1 << plane.bit_depth) - 1;
    let got: Vec<i32> = plane.samples.iter().map(|&s| (s + lvl).clamp(0, maxval)).collect();

    assert_eq!(got.len(), expected.len(), "sample count");
    let ndiff = got.iter().zip(&expected).filter(|(a, b)| a != b).count();
    assert_eq!(ndiff, 0, "{ndiff} samples differ (ROI max-shift mis-applied?)");
    println!("RGN: bit-exact ({} samples, shift applied)", got.len());
}
