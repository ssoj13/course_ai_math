//! Pixel-level ISO 15444 conformance: decode each vector in `$CONFDIR` and compare every component
//! with its reference `.pgx` in `$CONFDIR/references`, reporting the peak absolute error (PAE).
//!
//! Run: `CONFDIR=<dir> cargo test --release --test conf_ref -- --ignored --nocapture`
//! Optional: `CONFONLY=p1_06,hifi` (name substrings), `CONFTOL=1` (default allowed PAE).
//! `#[ignore]`d (the vectors are not redistributed here): panics if CONFDIR is unset instead of
//! passing vacuously. Failures print as `FAIL <stem>: <error>` (this also replaces the old sweep).
//!
//! Vectors listed in `KNOWN_GAPS` are documented unsupported/failing features: they are reported
//! but do not fail the test, and the test FAILS if one of them starts passing, so the list can
//! only shrink (delete the entry when you fix the feature).
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use jph_rs::mem::{DecodeParams, Image, try_decode_with};

/// `(stream stem, why)`. Keep in sync with CONFORMANCE.md.
const KNOWN_GAPS: &[(&str, &str)] = &[];

/// Vectors whose suite reference is a reduced-resolution decode: `(stem, resolution levels dropped)`.
const REDUCE: &[(&str, u32)] = &[("p0_08", 1)];

/// Per-vector peak-error baselines for lossy (irreversible) streams, measured 2026-09-30 against the
/// suite's reference `.pgx`. These are REGRESSION guards, not the ISO Part-4 limits (not verified
/// here): the test fails if the PAE grows. Lossless vectors use `CONFTOL` (default 1).
const PAE_BASELINE: &[(&str, i32)] = &[
    ("ATK_DFS_REV", 2),
    ("ds0_ht_06_b11", 2),
    ("ds0_ht_06_b15", 2),
    ("ds0_ht_06_b18", 2),
    ("hifi_ht1_02", 32),
    ("hifi_p1_02", 28),
    ("p0_04", 2),
    ("p0_06", 367),
    ("p1_02", 2),
    ("p1_04", 253),
    // Equals the OpenHTJ2K oracle's own PAE vs the suite reference (11/7/15): reference decoder rounding.
    ("p1_05", 15),
];

/// Reference component parsed from a PGX file (`PG <ML|LM> <+|-><depth> <w> <h>` + raw samples).
struct Pgx {
    w: usize,
    h: usize,
    samples: Vec<i32>,
}

fn read_pgx(p: &Path) -> Option<Pgx> {
    let data = std::fs::read(p).ok()?;
    let nl = data.iter().position(|&b| b == b'\n')?;
    let hdr = std::str::from_utf8(&data[..nl]).ok()?;
    let t: Vec<&str> = hdr.split_whitespace().collect();
    // Header forms seen: `PG ML -4 w h` (signed), `PG ML 12 w h` (unsigned, no sign token),
    // `PG ML +8 w h`, and `PG ML + 8 w h`.
    let (big, mut i) = (t.get(1)? == &"ML", 2);
    let mut sign_depth = t.get(i)?.to_string();
    if sign_depth == "+" || sign_depth == "-" {
        i += 1;
        sign_depth.push_str(t.get(i)?);
    }
    let signed = sign_depth.starts_with('-');
    let depth: u32 = sign_depth.trim_start_matches(['+', '-']).parse().ok()?;
    let (w, h): (usize, usize) = (t.get(i + 1)?.parse().ok()?, t.get(i + 2)?.parse().ok()?);
    let body = &data[nl + 1..];
    let bps = if depth <= 8 { 1 } else { 2 };
    if body.len() < w * h * bps {
        return None;
    }
    let samples = (0..w * h)
        .map(|k| {
            let v = if bps == 1 {
                i32::from(body[k])
            } else if big {
                i32::from(u16::from_be_bytes([body[2 * k], body[2 * k + 1]]))
            } else {
                i32::from(u16::from_le_bytes([body[2 * k], body[2 * k + 1]]))
            };
            // Signed samples are stored sign-extended to the word width (8 or 16 bits).
            match (signed, bps) {
                (true, 1) => i32::from(v as u8 as i8),
                (true, _) => i32::from(v as u16 as i16),
                _ => v,
            }
        })
        .collect();
    Some(Pgx { w, h, samples })
}

/// Reference file prefix for a stream stem, following the suite's naming.
fn ref_prefix(stem: &str) -> Option<String> {
    if let Some(n) = stem.strip_prefix("p0_") {
        return Some(format!("c1p0_{n}"));
    }
    if let Some(n) = stem.strip_prefix("p1_") {
        return Some(format!("c1p1_{n}"));
    }
    match stem {
        "hifi_ht1_02" | "hifi_p1_02" => Some("hifi".into()),
        "ATK_DFS_IRV" => Some("p2_dfs_irv".into()),
        "ATK_DFS_REV" => Some("p2_dfs_atk".into()),
        _ => None,
    }
}

/// Where the reference component `c` of `stem` lives: the suite's own `references/` when the stem
/// has a mapping, else the OpenHTJ2K oracle's output in `$CONFORACLE` (`<stem>_NN.pgx`, produced by
/// `open_htj2k_dec -i <stem> -o <dir>/<stem>.pgx`), which covers the `ds0_*`/`ds1_*` HT vectors.
fn ref_path(dir: &Path, oracle: Option<&Path>, stem: &str, c: usize) -> Option<std::path::PathBuf> {
    // CONFREDUCE=n: ad-hoc reduced-resolution check against oracle output made with `-reduce n`.
    if std::env::var_os("CONFREDUCE").is_none() && let Some(prefix) = ref_prefix(stem) {
        return Some(dir.join("references").join(format!("{prefix}-{c}.pgx")));
    }
    oracle.map(|o| o.join(format!("{stem}_{c:02}.pgx"))).filter(|p| p.exists() || c > 0)
}

/// Peak absolute error per component vs the references, or an error text.
fn compare(img: &Image, dir: &Path, oracle: Option<&Path>, stem: &str) -> Result<Vec<i32>, String> {
    let mut out = Vec::new();
    for (c, p) in img.planes.iter().enumerate() {
        let rp = ref_path(dir, oracle, stem, c).ok_or_else(|| "no reference available".to_string())?;
        // The suite ships references for only the first 4 of p0_13's 257 components.
        if c > 0 && !rp.exists() {
            break;
        }
        let r = read_pgx(&rp).ok_or_else(|| format!("missing/invalid reference {}", rp.display()))?;
        if (r.w, r.h) != (p.width as usize, p.height as usize) {
            return Err(format!("comp {c}: size {}x{} vs reference {}x{}", p.width, p.height, r.w, r.h));
        }
        // Decoded samples are raw (no clamp for lossy overshoot): clamp to the component's range.
        let (lo, hi) = if p.signed {
            (-(1i32 << (p.bit_depth - 1)), (1i32 << (p.bit_depth - 1)) - 1)
        } else {
            (0, ((1u64 << p.bit_depth) - 1) as i32)
        };
        let pae = p.samples.iter().zip(&r.samples).map(|(&s, &e)| (s.clamp(lo, hi) - e).abs()).max().unwrap_or(0);
        // CONFDIAG=1: where do the significant (>1) differences sit? Helps tell a localised coding
        // problem (a few blocks) from a global one (scaling, wrong tables).
        if pae > 1 && std::env::var_os("CONFDIAG").is_some() {
            let w = p.width as usize;
            let bad: Vec<usize> = (0..p.samples.len()).filter(|&i| (p.samples[i].clamp(lo, hi) - r.samples[i]).abs() > 1).collect();
            let (x0, x1) = (bad.iter().map(|i| i % w).min().unwrap_or(0), bad.iter().map(|i| i % w).max().unwrap_or(0));
            let (y0, y1) = (bad.iter().map(|i| i / w).min().unwrap_or(0), bad.iter().map(|i| i / w).max().unwrap_or(0));
            let i0 = bad[0];
            let diffs: Vec<i64> = (0..p.samples.len()).map(|i| i64::from(p.samples[i].clamp(lo, hi) - r.samples[i])).collect();
            let mean = diffs.iter().sum::<i64>() as f64 / diffs.len() as f64;
            let row0: Vec<i64> = (0..w).step_by(32.max(w / 16)).map(|x| diffs[x]).collect();
            let col0: Vec<i64> = (0..p.height as usize).step_by(((p.height as usize) / 8).max(1)).map(|y| diffs[y * w]).collect();
            println!("DIAG  {stem} comp {c} row0 diffs {row0:?} col0 diffs {col0:?}");
            println!(
                "DIAG  {stem} comp {c}: {} of {} samples differ by >1, bbox x {x0}..={x1} y {y0}..={y1}; first @({}, {}) mine={} ref={}; signed diff mean {mean:.2} min {} max {}",
                bad.len(), p.samples.len(), i0 % w, i0 / w, p.samples[i0], r.samples[i0],
                diffs.iter().min().unwrap_or(&0), diffs.iter().max().unwrap_or(&0)
            );
        }
        out.push(pae);
    }
    Ok(out)
}

#[test]
#[ignore = "needs external conformance fixtures (env vars): run with --ignored"]
fn conf_ref() {
    let Ok(dir) = std::env::var("CONFDIR") else {
        panic!("conf_ref: CONFDIR not set");
    };
    let dir = Path::new(&dir);
    let oracle_dir = std::env::var("CONFORACLE").ok();
    let oracle = oracle_dir.as_deref().map(Path::new);
    let only: Vec<String> = std::env::var("CONFONLY").map(|v| v.split(',').map(str::to_owned).collect()).unwrap_or_default();
    let tol: i32 = std::env::var("CONFTOL").ok().and_then(|v| v.parse().ok()).unwrap_or(1);

    let mut stems: Vec<String> = std::fs::read_dir(dir)
        .expect("CONFDIR must be readable")
        .filter_map(|e| e.ok())
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|_| e.path().extension().is_some_and(|x| x == "j2k" || x == "j2c")))
        .filter(|s| ref_path(dir, oracle, s, 0).is_some_and(|p| p.exists()))
        .collect();
    stems.sort();
    let mut bad = Vec::new();
    for stem in stems {
        if !only.is_empty() && !only.iter().any(|o| stem.contains(o.as_str())) {
            continue;
        }
        let path = ["j2k", "j2c"].iter().map(|x| dir.join(format!("{stem}.{x}"))).find(|p| p.exists()).expect("stream exists");
        let bytes = std::fs::read(&path).expect("readable");
        let reduce = REDUCE.iter().find(|(s, _)| *s == stem).map_or(0, |&(_, r)| r);
        let reduce = std::env::var("CONFREDUCE").ok().and_then(|v| v.parse().ok()).unwrap_or(reduce);
        let params = DecodeParams { reduce, ..Default::default() };
        let res = match catch_unwind(AssertUnwindSafe(|| try_decode_with(&bytes, &params))) {
            Ok(Ok(img)) => compare(&img, dir, oracle, &stem),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err("PANIC".into()),
        };
        let tol = PAE_BASELINE.iter().find(|(s, _)| *s == stem).map_or(tol, |&(_, t)| t);
        let gap = KNOWN_GAPS.iter().find(|(s, _)| *s == stem).map(|(_, why)| *why);
        match (&res, gap) {
            (Ok(pae), None) if pae.iter().all(|&e| e <= tol) => println!("OK    {stem} PAE={pae:?}"),
            (Ok(pae), None) => {
                println!("BAD   {stem} PAE={pae:?} (> {tol})");
                bad.push(stem);
            }
            (Ok(pae), Some(why)) if pae.iter().all(|&e| e <= tol) => {
                println!("FIXED {stem} PAE={pae:?} - remove from KNOWN_GAPS ({why})");
                bad.push(stem);
            }
            (Ok(pae), Some(why)) => println!("GAP   {stem} PAE={pae:?} ({why})"),
            (Err(e), None) => {
                println!("FAIL  {stem}: {e}");
                bad.push(stem);
            }
            (Err(e), Some(why)) => println!("GAP   {stem}: {e} ({why})"),
        }
    }
    assert!(bad.is_empty(), "conformance regressions / stale KNOWN_GAPS: {bad:?}");
}
