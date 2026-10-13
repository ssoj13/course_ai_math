//! Packed packet headers (PPM `0xFF60` main / PPT `0xFF61` tile) decode regression.
//!
//! With PPM/PPT the packet headers are gathered into a marker buffer and the tile bitstream carries
//! only the packet bodies; the decoder must read headers from that buffer (advancing a persistent
//! bit cursor) while reading bodies from the tile stream. OpenJPH skips both markers. Validated for
//! the layer-innermost progressions (RPCL/PCRL/CPRL); the conformance vectors are irreversible 9/7,
//! so the comparison is against the OpenHTJ2K oracle within the ISO tolerance (peak abs error <= 1).
//!
//! Generate per-component references with the dev harness:
//!   open_htj2k_dec -i ds1_ht_03_b11.j2k -o ref.pgm   ==> ref_<NN>.pgm
//! Point `PPVEC` at the `.j2k` and `PPREF` at the reference prefix (`<PPREF>_00.pgm` etc.). Absent
//! env = skip, so CI without the conformance set stays green.

use std::path::PathBuf;
mod common;

fn load_pgm(path: &PathBuf) -> Option<Vec<i32>> {
    Some(common::pgm(path).2)
}


#[test]
#[ignore = "needs external conformance fixtures (env vars): run with --ignored"]
fn packed_headers_decode_within_tolerance() {
    let (Ok(vec_path), Ok(ref_prefix)) = (std::env::var("PPVEC"), std::env::var("PPREF")) else {
        panic!("PPVEC/PPREF unset — skipping PPM/PPT validation");
    };
    let img = jph_rs::mem::decode(&std::fs::read(&vec_path).unwrap());
    let mut checked = 0;
    for (i, p) in img.planes.iter().enumerate() {
        let Some(expected) = load_pgm(&PathBuf::from(format!("{ref_prefix}_{i:02}.pgm"))) else {
            continue;
        };
        // Replicate the oracle's PGM writer (signed level-shift + clamp), then bound the error.
        let lvl: i32 = if p.signed { 1 << (p.bit_depth - 1) } else { 0 };
        let maxval: i32 = (1 << p.bit_depth) - 1;
        let got: Vec<i32> = p.samples.iter().map(|&s| (s + lvl).clamp(0, maxval)).collect();
        assert_eq!(got.len(), expected.len(), "comp {i}: sample count");
        let pae = got.iter().zip(&expected).map(|(a, b)| (a - b).abs()).max().unwrap_or(0);
        assert!(pae <= 1, "comp {i}: peak abs error {pae} exceeds the ISO 9/7 tolerance of 1");
        checked += 1;
        println!("packed-headers comp {i}: PAE {pae} ({} samples)", got.len());
    }
    assert!(checked > 0, "no component references found at {ref_prefix}_NN.pgm");
}
