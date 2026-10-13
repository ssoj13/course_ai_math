//! Every OCIO builtin transform, by its exact OCIO style name, against PyOpenColorIO.
//!
//! `tests/data/ocio_builtins_pyocio.json` holds, for each of the 98 styles in OCIO's
//! `BuiltinTransformRegistry`, the forward CPU result on five probes (neutral, chromatic,
//! saturated, above 1, negative). It was generated with PyOpenColorIO 2.5.1; the generator is in
//! the file's `generator` field and the script is `tests/data/gen_ocio_builtins.py`
//! (run it from `tests/data`).
//!
//! The test collects every mismatch and reports them together, so one run shows the whole gap.

use serde_json::Value;
use vfx_ocio::{BuiltinTransform, Processor, Transform, TransformDirection};

/// Absolute tolerance scaled by magnitude: OCIO's own `BuiltinTransform_tests.cpp` uses 1e-5 to
/// 1e-6 per row on values near 1; outputs here reach ~100 (PQ, HDR), hence the relative part.
fn close(got: f32, want: f64) -> bool {
    let want = want as f32;
    (got - want).abs() <= 1e-4 * want.abs().max(1.0)
}

#[test]
fn every_ocio_builtin_matches_pyocio() {
    let data: Value =
        serde_json::from_str(include_str!("data/ocio_builtins_pyocio.json")).expect("parse table");
    let probes: Vec<[f32; 3]> = data["probes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| std::array::from_fn(|c| p[c].as_f64().unwrap() as f32))
        .collect();
    let mut failures = Vec::new();
    let builtins = data["builtins"].as_array().unwrap();
    for row in builtins {
        let style = row["style"].as_str().unwrap();
        let t = Transform::Builtin(BuiltinTransform {
            style: style.to_string(),
            direction: TransformDirection::Forward,
        });
        let proc = match Processor::from_transform(&t, TransformDirection::Forward) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("{style}: not accepted ({e})"));
                continue;
            }
        };
        let mut px = probes.clone();
        proc.apply_rgb(&mut px);
        for (k, want) in row["forward"].as_array().unwrap().iter().enumerate() {
            let want: [f64; 3] = std::array::from_fn(|c| want[c].as_f64().unwrap());
            if (0..3).any(|c| !close(px[k][c], want[c])) {
                failures.push(format!(
                    "{style}: probe {:?} -> {:?}, OCIO {:?}",
                    probes[k], px[k], want
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} builtin rows disagree with OCIO:\n{}",
        failures.len(),
        builtins.len() * probes.len(),
        failures.join("\n")
    );
}
