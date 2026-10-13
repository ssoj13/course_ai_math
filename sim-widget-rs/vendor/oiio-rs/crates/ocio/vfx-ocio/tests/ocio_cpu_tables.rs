//! OCIO's CPU results for LUT files, ExposureContrast and LogCamera (ROADMAP D4).
//!
//! `tests/data/ocio_cpu_tables_pyocio.json` was made by PyOpenColorIO (version in its
//! `generator` field; the script is `tests/data/gen_ocio_cpu_tables.py`) from:
//! - `FileTransform`s on OCIO's own fixture LUTs (copies in `vfx-lut/tests/data/ocio`): 1D, 3D,
//!   a 1D shaper + 3D (`.3dl`, Resolve `.cube`), a matrix; every interpolation the format takes
//!   (trilinear and tetrahedral for 3D), forward and inverse (OCIO's exact inverses);
//! - `ExposureContrastTransform` in the linear, video and logarithmic styles, both directions;
//! - `LogCameraTransform` with probes on both sides of its linear break.
//!
//! Probes are chromatic and off the LUT grids, so an axis-order, interpolation or direction
//! error shows. Every mismatch is collected, so one run shows the whole gap.

use std::path::PathBuf;

use serde_json::Value;
use vfx_ocio::{
    ExposureContrastStyle, ExposureContrastTransform, FileTransform, Interpolation,
    LogCameraTransform, Processor, Transform, TransformDirection,
};

fn table() -> Value {
    serde_json::from_str(include_str!("data/ocio_cpu_tables_pyocio.json")).expect("parse table")
}

fn rgb(v: &Value) -> [f32; 3] {
    std::array::from_fn(|c| v[c].as_f64().unwrap() as f32)
}

fn direction(v: &Value) -> TransformDirection {
    match v.as_str().unwrap() {
        "forward" => TransformDirection::Forward,
        "inverse" => TransformDirection::Inverse,
        d => panic!("direction {d}"),
    }
}

/// Applies `t` to `probes` and records every channel further than `tol` (relative above 1)
/// from OCIO's.
fn compare(
    label: &str,
    t: &Transform,
    probes: &[[f32; 3]],
    want: &[Value],
    tol: f32,
    failures: &mut Vec<String>,
) {
    let proc = match Processor::from_transform(t, TransformDirection::Forward) {
        Ok(p) => p,
        Err(e) => {
            failures.push(format!("{label}: not accepted ({e})"));
            return;
        }
    };
    let mut px = probes.to_vec();
    proc.apply_rgb(&mut px);
    for (k, w) in want.iter().enumerate() {
        let w = rgb(w);
        if (0..3).any(|c| (px[k][c] - w[c]).abs() > tol * w[c].abs().max(1.0)) {
            failures.push(format!(
                "{label}: {:?} -> {:?}, OCIO {w:?}",
                probes[k], px[k]
            ));
        }
    }
}

#[test]
fn file_transforms_match_ocio() {
    let data = table();
    let probes: Vec<[f32; 3]> = data["probes"].as_array().unwrap().iter().map(rgb).collect();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vfx-lut/tests/data/ocio");
    let mut failures = Vec::new();
    let rows = data["files"].as_array().unwrap();
    for row in rows {
        let file = row["file"].as_str().unwrap();
        let interpolation = match row["interpolation"].as_str().unwrap() {
            "linear" => Interpolation::Linear,
            "tetrahedral" => Interpolation::Tetrahedral,
            i => panic!("interpolation {i}"),
        };
        let t = Transform::FileTransform(FileTransform {
            src: dir.join(file),
            ccc_id: None,
            cdl_style: Default::default(),
            interpolation,
            direction: direction(&row["direction"]),
        });
        let label = format!("{file} {interpolation:?} {}", row["direction"]);
        // The reference is OCIO at OPTIMIZATION_LOSSLESS: its default (VERY_GOOD) swaps an
        // inverse LUT for a baked approximation (LUT_INV_FAST), where vfx-ocio inverts exactly.
        compare(
            &label,
            &t,
            &probes,
            row["out"].as_array().unwrap(),
            1e-5,
            &mut failures,
        );
    }
    assert!(
        failures.is_empty(),
        "{} mismatches over {} rows:\n{}",
        failures.len(),
        rows.len(),
        failures.join("\n")
    );
}

#[test]
fn exposure_contrast_matches_ocio() {
    let data = table();
    let probes: Vec<[f32; 3]> = data["probes"].as_array().unwrap().iter().map(rgb).collect();
    let mut failures = Vec::new();
    for row in data["exposure_contrast"].as_array().unwrap() {
        let style = match row["style"].as_str().unwrap() {
            "linear" => ExposureContrastStyle::Linear,
            "video" => ExposureContrastStyle::Video,
            "log" => ExposureContrastStyle::Logarithmic,
            s => panic!("style {s}"),
        };
        let f = |k: &str| row[k].as_f64().unwrap();
        let t = Transform::ExposureContrast(ExposureContrastTransform {
            name: String::new(),
            exposure: f("exposure"),
            contrast: f("contrast"),
            gamma: f("gamma"),
            pivot: f("pivot"),
            log_exposure_step: f("log_exposure_step"),
            log_mid_gray: f("log_mid_gray"),
            style,
            direction: direction(&row["direction"]),
            dynamic: vfx_ocio::EcDynamic::NONE,
        });
        let label = format!("EC {style:?} {}", row["direction"]);
        // OCIO's CPU path raises to the contrast power with `ssePower`, an approximation
        // (ExposureContrastOpCPU.cpp:202); vfx-ocio uses `powf`. They differ by ~1.2e-5
        // relative at 2.9.
        compare(
            &label,
            &t,
            &probes,
            row["out"].as_array().unwrap(),
            2e-5,
            &mut failures,
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn log_camera_matches_ocio_on_both_sides_of_the_break() {
    let data = table();
    let p = &data["log_camera"]["params"];
    let v3 = |k: &str| -> [f64; 3] { std::array::from_fn(|c| p[k][c].as_f64().unwrap()) };
    let mut failures = Vec::new();
    for row in data["log_camera"]["rows"].as_array().unwrap() {
        let t = Transform::LogCamera(LogCameraTransform {
            name: String::new(),
            base: p["base"].as_f64().unwrap(),
            log_side_slope: v3("logSideSlope"),
            log_side_offset: v3("logSideOffset"),
            lin_side_slope: v3("linSideSlope"),
            lin_side_offset: v3("linSideOffset"),
            lin_side_break: v3("linSideBreak"),
            linear_slope: None,
            direction: direction(&row["direction"]),
        });
        let probes: Vec<[f32; 3]> = row["probes"].as_array().unwrap().iter().map(rgb).collect();
        let label = format!("LogCamera {}", row["direction"]);
        compare(
            &label,
            &t,
            &probes,
            row["out"].as_array().unwrap(),
            1e-5,
            &mut failures,
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
