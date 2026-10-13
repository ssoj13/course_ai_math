//! Scalar-derived quantization (QCD/QCC `Sqcd & 0x1F == 1`) decode regression.
//!
//! Derived quantization signals one step size (the LL band) and derives the rest per ITU-T T.800
//! E.1.1 (`expn_b = max(0, expn_0 - (b-1)/3)`, mantissa shared) — OpenJPH rejected it outright. The
//! conformance vector is irreversible 9/7, so it is validated against the OpenHTJ2K oracle within
//! the ISO tolerance gate (peak absolute error <= 1, the float-rounding difference between
//! independent 9/7 implementations), not bit-exact.
//!
//! Generate the per-component references with the dev harness:
//!   open_htj2k_dec -i ds0_ht_05_b11.j2k -o ref.pgm   ==> ref_<NN>.pgm
//! Point `DQVEC` at the `.j2k` and `DQREF` at the reference prefix (so `<DQREF>_00.pgm` etc.).
//! Absent env = skip, so CI without the conformance set stays green.

use std::path::PathBuf;
mod common;

fn load_pgm(path: &PathBuf) -> Option<Vec<i32>> {
    Some(common::pgm(path).2)
}


#[test]
#[ignore = "needs external conformance fixtures (env vars): run with --ignored"]
fn derived_quant_decode_within_tolerance() {
    let (Ok(vec_path), Ok(ref_prefix)) = (std::env::var("DQVEC"), std::env::var("DQREF")) else {
        panic!("DQVEC/DQREF unset — skipping derived-quant validation");
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
        println!("derived-quant comp {i}: PAE {pae} ({} samples)", got.len());
    }
    assert!(checked > 0, "no component references found at {ref_prefix}_NN.pgm");
}
