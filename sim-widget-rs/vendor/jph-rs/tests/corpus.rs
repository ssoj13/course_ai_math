//! Committed fixtures: `tests/data/corpus` (raw PGM/PPM images: odd sizes, 1x1, 16-bit, solid,
//! random) and `tests/data/ict` (single-tile HT-lossy streams written by the C++ and Rust encoders).
//!
//! * every corpus image survives a lossless encode -> decode bit-exactly (with RCT for colour);
//! * the same images survive a lossy encode within a bounded error;
//! * the committed C++-encoded `.j2c` decode to the matching source picture within lossy tolerance.
use std::path::{Path, PathBuf};

mod common;
use common::read_pnm;
use jph_rs::mem::{EncodeOptions, EncodeParams, Image, Plane, Quant, Rounding, try_decode, try_encode, try_encode_with};

fn data(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(rel)
}

fn corpus() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(data("corpus"))
        .expect("corpus dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "pgm" || x == "ppm"))
        .collect();
    v.sort();
    assert!(v.len() >= 20, "corpus went missing: {} files", v.len());
    v
}

#[test]
fn corpus_lossless_roundtrip() {
    for p in corpus() {
        let img = read_pnm(&p);
        let params = EncodeParams { color_transform: img.planes.len() == 3, ..EncodeParams::default() };
        let bytes = try_encode(&img, &params).unwrap_or_else(|e| panic!("{}: encode: {e}", p.display()));
        let back = try_decode(&bytes).unwrap_or_else(|e| panic!("{}: decode: {e}", p.display()));
        assert_eq!(back, img, "{}", p.display());
    }
}

/// Peak absolute error between two same-shape images.
fn pae(a: &Image, b: &Image) -> i32 {
    a.planes
        .iter()
        .zip(&b.planes)
        .flat_map(|(x, y)| x.samples.iter().zip(&y.samples).map(|(&s, &t)| (s - t).abs()))
        .max()
        .unwrap_or(0)
}

#[test]
fn corpus_lossy_roundtrip_is_bounded() {
    for p in corpus() {
        let img = read_pnm(&p);
        // Step relative to the sample range, so 16-bit images get a proportionally coarser step.
        let step = if img.planes[0].bit_depth > 8 { 1.0 / 16.0 } else { 1.0 / 64.0 };
        let params = EncodeParams {
            quant: Quant::Step(step),
            color_transform: img.planes.len() == 3,
            ..EncodeParams::default()
        };
        let range = 1i32 << img.planes[0].bit_depth;
        for rounding in [Rounding::Cpp, Rounding::Trunc] {
            let bytes = try_encode_with(&img, &params, &EncodeOptions { rounding, ..Default::default() }).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            let back = try_decode(&bytes).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            let e = pae(&img, &back);
            assert!(e <= range / 16, "{} {rounding:?}: PAE {e} (range {range})", p.display());
        }
    }
}

#[test]
fn committed_cpp_streams_decode_close_to_source() {
    let src = read_pnm(&data("ict/ict_64.ppm"));
    for name in ["ict_cpp.j2c", "ict_hq.j2c", "ict_rust.j2c", "ict_rust64.j2c"] {
        let bytes = std::fs::read(data(&format!("ict/{name}"))).expect("stream");
        let img = try_decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(img.planes.len(), 3, "{name}");
        if (img.width, img.height) != (64, 64) {
            continue; // a different (smaller) source picture; decoding it cleanly is the check
        }
        let clamp = |i: &Image| Image {
            planes: i.planes.iter().map(|p| Plane { samples: p.samples.iter().map(|&s| s.clamp(0, 255)).collect(), ..p.clone() }).collect(),
            ..i.clone()
        };
        let e = pae(&src, &clamp(&img));
        // These are deliberately coarse lossy streams (some at 233-299 bytes for a 12 KB picture);
        // this only guards "decodes to something recognisably the source", not fidelity.
        assert!(e <= 255, "{name}: PAE {e}");
        let mean = src.planes.iter().zip(&img.planes).flat_map(|(a, b)| a.samples.iter().zip(&b.samples).map(|(&s, &t)| f64::from((s - t.clamp(0, 255)).abs()))).sum::<f64>()
            / (3.0 * 64.0 * 64.0);
        assert!(mean < 40.0, "{name}: mean abs error {mean:.1}");
    }
}

/// POC progression with inline, PPM-packed and PPT-packed packet headers (Kakadu fixtures): all
/// three must decode to the source image bit-exactly.
#[test]
fn kakadu_poc_with_packed_headers_matches_source() {
    let src = read_pnm(&data("corpus/rgb_prime_37x41.ppm"));
    for name in ["poc", "poc_ppm", "poc_ppt"] {
        let bytes = std::fs::read(data(&format!("kakadu/{name}.j2c"))).expect("fixture");
        let img = try_decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(img, src, "{name}");
    }
}

/// Multi-layer LRCP/RLCP with resolutions dropped: later layers must stay aligned (the dropped
/// resolutions' packets are consumed header-only). Reference = `kdu_expand -reduce 1`.
#[test]
fn kakadu_multilayer_reduce_matches_kdu_expand() {
    use jph_rs::mem::{DecodeParams, try_decode_with};
    for ord in ["LRCP", "RLCP"] {
        let bytes = std::fs::read(data(&format!("kakadu/ml_{ord}.j2c"))).expect("fixture");
        let want = read_pnm(&data(&format!("kakadu/ml_{ord}_r1.ppm")));
        let mut got = try_decode_with(&bytes, &DecodeParams { reduce: 1, ..Default::default() }).unwrap_or_else(|e| panic!("{ord}: {e}"));
        // A reduced (low-pass only) reconstruction may overshoot the sample range; the reference
        // tool clamps on output, `mem` hands out the raw values.
        for p in &mut got.planes {
            p.samples.iter_mut().for_each(|s| *s = (*s).clamp(0, 255));
        }
        assert_eq!(got, want, "{ord}");
    }
}

/// POC + PPT-packed headers + resolution reduction (`kdu_expand -reduce 1` reference).
#[test]
fn kakadu_poc_reduce_matches_kdu_expand() {
    use jph_rs::mem::{DecodeParams, try_decode_with};
    let bytes = std::fs::read(data("kakadu/poc_ppt.j2c")).expect("fixture");
    let want = read_pnm(&data("kakadu/poc_r1.ppm"));
    let mut got = try_decode_with(&bytes, &DecodeParams { reduce: 1, ..Default::default() }).expect("decode");
    for p in &mut got.planes {
        p.samples.iter_mut().for_each(|s| *s = (*s).clamp(0, 255));
    }
    assert_eq!(got, want);
}

/// Single-layer POC stream split into several tile-parts (buffered and swept once).
#[test]
fn kakadu_poc_across_tile_parts_matches_source() {
    let src = read_pnm(&data("corpus/rgb_prime_37x41.ppm"));
    let bytes = std::fs::read(data("kakadu/poc1tp.j2c")).expect("fixture");
    assert_eq!(try_decode(&bytes).expect("decode"), src);
}

/// Multi-layer, multi-tile-part streams with inline / PPM / PPT packed headers.
#[test]
fn kakadu_multilayer_tileparts_with_packed_headers_match_source() {
    let src = read_pnm(&data("corpus/rgb_prime_37x41.ppm"));
    for name in ["mlt", "mlt_ppm", "mlt_ppt"] {
        let bytes = std::fs::read(data(&format!("kakadu/{name}.j2c"))).expect("fixture");
        assert_eq!(try_decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}")), src, "{name}");
    }
}
