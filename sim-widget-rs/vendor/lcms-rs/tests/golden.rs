//! Differential golden-vector validation against the REAL Little-CMS (lcms2 2.19, the same
//! version the port follows, built from vcpkg). `tools/gen-vectors/gen.c` produced
//! `tests/fixtures/golden/golden.txt` by running the C library; here the Rust port reproduces
//! each operation and asserts **byte-identical** results. This is the definitive bit-exactness
//! check that the earlier hand-derived / round-trip tests approximate.

use std::path::PathBuf;

use lcms_rs::cmm::transform::create_transform;
use lcms_rs::color::gamma::ToneCurve;
use lcms_rs::color::pcs::{cms_cie2000_delta_e, cms_float_to_lab_encoded, cms_float_to_xyz_encoded};
use lcms_rs::context::Context;
use lcms_rs::profile::Profile;
use lcms_rs::types::color::{CIELab, CIEXYZ};
use lcms_rs::types::format::{FormatBuilder, PixelFormat};
use lcms_rs::virt::{create_lab4_profile, create_srgb_profile, create_xyz_profile};

// lcms PT_* colour-space codes + TYPE_* format words (built explicitly to avoid depending on
// crate-internal `pt`/intent visibility).
fn fmt(pt: u32, chan: u32, bytes: u32, float: bool) -> PixelFormat {
    FormatBuilder::new().float(float).colorspace(pt).channels(chan).bytes(bytes).build()
}
fn rgb8() -> PixelFormat {
    fmt(4, 3, 1, false)
}
fn lab16() -> PixelFormat {
    fmt(10, 3, 2, false)
}
fn xyz16() -> PixelFormat {
    fmt(9, 3, 2, false)
}
fn cmyk8() -> PixelFormat {
    fmt(6, 4, 1, false)
}

const FLAGS_BPC: u32 = 0x2000; // cmsFLAGS_BLACKPOINTCOMPENSATION

fn golden_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/golden/golden.txt");
    p
}

fn testbed(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/icc");
    p.push(name);
    p
}

/// One parsed golden line: `key a b c: x y z ...` → (key, inputs, expected outputs).
struct Line {
    key: String,
    inp: Vec<f64>,
    out: Vec<i64>,
}

fn parse() -> Vec<Line> {
    let text = std::fs::read_to_string(golden_path())
        .unwrap_or_else(|e| panic!("read golden.txt (run tools/gen-vectors): {e}"));
    let mut v = Vec::new();
    for l in text.lines() {
        let l = l.trim();
        if l.is_empty() {
            continue;
        }
        let (lhs, rhs) = match l.split_once(':') {
            Some(x) => x,
            None => {
                // `de2000 <value>` — no colon.
                let mut it = l.split_whitespace();
                let key = it.next().unwrap().to_string();
                let out: Vec<i64> = Vec::new();
                let inp: Vec<f64> = it.map(|s| s.parse().unwrap()).collect();
                v.push(Line { key, inp, out });
                continue;
            }
        };
        let mut lit = lhs.split_whitespace();
        let key = lit.next().unwrap().to_string();
        let inp: Vec<f64> = lit.map(|s| s.parse().unwrap()).collect();
        let out: Vec<i64> = rhs.split_whitespace().map(|s| s.parse().unwrap()).collect();
        v.push(Line { key, inp, out });
    }
    v
}

/// Build a transform matching the C harness and run every matching golden line through it,
/// asserting byte-identical output. `in_chan`/`out_chan` and `out_bytes` describe the buffers.
#[allow(clippy::too_many_arguments)]
fn check_transform(
    lines: &[Line],
    key: &str,
    input: &mut Profile,
    in_fmt: PixelFormat,
    output: &mut Profile,
    out_fmt: PixelFormat,
    intent: u32,
    flags: u32,
    out_chan: usize,
    out_bytes: usize,
) {
    let ctx = Context::new();
    let t = create_transform(&ctx, input, in_fmt, output, out_fmt, intent, flags)
        .unwrap_or_else(|e| panic!("{key}: create_transform: {e}"));
    let mut n = 0;
    for line in lines.iter().filter(|l| l.key == key) {
        let src: Vec<u8> = line.inp.iter().map(|&x| x as u8).collect();
        let mut dst = vec![0u8; out_chan * out_bytes];
        t.do_transform(&src, &mut dst, 1);
        // Decode dst per out_bytes and compare to the expected integer list.
        let got: Vec<i64> = if out_bytes == 1 {
            dst.iter().map(|&b| b as i64).collect()
        } else {
            dst.chunks(2).map(|c| u16::from_ne_bytes([c[0], c[1]]) as i64).collect()
        };
        assert_eq!(
            got, line.out,
            "{key} input {:?}: got {:?}, lcms golden {:?}",
            line.inp, got, line.out
        );
        n += 1;
    }
    assert!(n > 0, "{key}: no golden lines matched");
}

#[test]
fn srgb_to_srgb_all_intents_and_bpc() {
    let lines = parse();
    for intent in 0u32..=3 {
        let key = format!("srgb_srgb_i{intent}");
        let mut a = create_srgb_profile().unwrap();
        let mut b = create_srgb_profile().unwrap();
        check_transform(&lines, &key, &mut a, rgb8(), &mut b, rgb8(), intent, 0, 3, 1);
    }
    let mut a = create_srgb_profile().unwrap();
    let mut b = create_srgb_profile().unwrap();
    check_transform(&lines, "srgb_srgb_bpc", &mut a, rgb8(), &mut b, rgb8(), 1, FLAGS_BPC, 3, 1);
}

#[test]
fn srgb_to_lab_and_xyz() {
    let lines = parse();
    let mut s1 = create_srgb_profile().unwrap();
    let mut lab = create_lab4_profile(None).unwrap();
    check_transform(&lines, "srgb_lab16", &mut s1, rgb8(), &mut lab, lab16(), 1, 0, 3, 2);

    let mut s2 = create_srgb_profile().unwrap();
    let mut xyz = create_xyz_profile().unwrap();
    check_transform(&lines, "srgb_xyz16", &mut s2, rgb8(), &mut xyz, xyz16(), 1, 0, 3, 2);
}

#[test]
fn srgb_to_cmyk_clut() {
    let lines = parse();
    let mut srgb = create_srgb_profile().unwrap();
    let bytes = std::fs::read(testbed("test1.icc")).expect("test1.icc");
    let mut cmyk = Profile::open_from_mem(Context::new(), &bytes).expect("open test1.icc");
    // sRGB -> CMYK printer profile (full RGB->Lab->CLUT->CMYK path), perceptual.
    check_transform(&lines, "srgb_cmyk8_perc", &mut srgb, rgb8(), &mut cmyk, cmyk8(), 0, 0, 4, 1);
}

#[test]
fn lab_v4_encoding() {
    for line in parse().iter().filter(|l| l.key == "lab_v4_enc") {
        let lab = CIELab { l: line.inp[0], a: line.inp[1], b: line.inp[2] };
        let got = cms_float_to_lab_encoded(&lab);
        let exp: Vec<i64> = line.out.clone();
        assert_eq!(vec![got[0] as i64, got[1] as i64, got[2] as i64], exp, "lab_v4_enc {:?}", line.inp);
    }
}

#[test]
fn xyz_encoding() {
    for line in parse().iter().filter(|l| l.key == "xyz_enc") {
        let xyz = CIEXYZ { x: line.inp[0], y: line.inp[1], z: line.inp[2] };
        let got = cms_float_to_xyz_encoded(&xyz);
        assert_eq!(
            vec![got[0] as i64, got[1] as i64, got[2] as i64],
            line.out,
            "xyz_enc {:?}",
            line.inp
        );
    }
}

#[test]
fn srgb_tone_curve_16() {
    // sRGB parametric type-4 curve, eval at the golden's 16-bit points.
    let params = [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045];
    let curve = ToneCurve::build_parametric(4, &params).unwrap();
    for line in parse().iter().filter(|l| l.key == "srgb_curve16") {
        let inp = line.inp[0] as u16;
        let got = curve.eval_u16(inp) as i64;
        assert_eq!(got, line.out[0], "srgb_curve16 {inp}");
    }
}

#[test]
fn delta_e_2000() {
    // Same Lab pairs the C harness used, in order.
    let pairs = [
        (CIELab { l: 50.0, a: 2.6772, b: -79.7751 }, CIELab { l: 50.0, a: 0.0, b: -82.7485 }),
        (
            CIELab { l: 63.0109, a: -31.0961, b: -5.8663 },
            CIELab { l: 62.8187, a: -29.7946, b: -4.0864 },
        ),
    ];
    let golden: Vec<f64> = parse().iter().filter(|l| l.key == "de2000").map(|l| l.inp[0]).collect();
    assert_eq!(golden.len(), pairs.len());
    for (i, (a, b)) in pairs.iter().enumerate() {
        let got = cms_cie2000_delta_e(a, b, 1.0, 1.0, 1.0);
        assert!(
            (got - golden[i]).abs() < 1e-9,
            "de2000 pair {i}: got {got}, lcms golden {}",
            golden[i]
        );
    }
}
