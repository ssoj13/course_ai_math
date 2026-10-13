//! `LegacyViewingPipeline` against PyOpenColorIO and against the semantics it exists for.
//!
//! `tests/data/ocio_viewing_pipeline_pyocio.json` was made by PyOpenColorIO (version in its
//! `generator` field; script `tests/data/gen_ocio_viewing_pipeline.py`) from the fixture config
//! `tests/data/viewing_pipeline.ocio` (scene-linear `lin`, a log space `logcs` that is also the
//! `color_timing` role, a display `gam`, a data space `raw`, two looks in different process
//! spaces) and from the built-in `ocio://default` config with an ACEScct input. Every case is a
//! plain description rebuilt here with vfx-ocio's own types, run on RGBA probes.
//!
//! What the cases pin: exposure in `scene_linear` for linear, log and display-encoded inputs;
//! `color_timing`; no-op corrections dropping their conversion; the view's looks, `looksBypass`
//! and the pipeline's looks override; the channel view (incl. alpha with data bypass); the
//! display correction after the display transform; data bypass; an inverse display direction;
//! a dynamic exposure moved on one processor. The tests that do not need PyOpenColorIO build the
//! expected chain by hand, so a pipeline that merely agrees with itself cannot pass.

use serde_json::Value;
use std::path::PathBuf;
use vfx_ocio::{
    Config, DisplayViewTransform, DynamicPropertyType, EcDynamic, ExposureContrastStyle,
    ExposureContrastTransform, GpuLanguage, GpuProcessor, GpuUniformType, GpuUniformValue,
    LegacyViewingPipeline, MatrixTransform, OcioError, Processor, Transform, TransformDirection,
    builtin,
};

const FWD: TransformDirection = TransformDirection::Forward;

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn table() -> Value {
    let text = std::fs::read_to_string(data("ocio_viewing_pipeline_pyocio.json"))
        .expect("read ocio_viewing_pipeline_pyocio.json");
    serde_json::from_str(&text).expect("parse ocio_viewing_pipeline_pyocio.json")
}

fn fixture() -> Config {
    Config::from_file(data("viewing_pipeline.ocio")).expect("load viewing_pipeline.ocio")
}

fn config(name: &str) -> Config {
    match name {
        "fixture" => fixture(),
        // Only one active view: `find_view` must still see the others (OCIO `getView`).
        "fixture_active" => {
            let text = std::fs::read_to_string(data("viewing_pipeline.ocio")).unwrap();
            let old = "active_views: [Standard, Looked, Raw, Flat]";
            assert!(text.contains(old));
            Config::from_stream(&text.replace(old, "active_views: [Standard]")).unwrap()
        }
        "default" => builtin::default_config(),
        n => panic!("config {n}"),
    }
}

fn f(v: &Value, key: &str, default: f64) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(default)
}

/// The case's exposure/contrast transform, with OCIO's defaults for what it leaves out.
fn ec(spec: &Value) -> Transform {
    let mut dynamic = EcDynamic::NONE;
    for p in spec["dynamic"].as_array().into_iter().flatten() {
        match p.as_str() {
            Some("exposure") => dynamic.exposure = true,
            Some("contrast") => dynamic.contrast = true,
            Some("gamma") => dynamic.gamma = true,
            other => panic!("dynamic {other:?}"),
        }
    }
    Transform::ExposureContrast(ExposureContrastTransform {
        name: String::new(),
        exposure: f(spec, "exposure", 0.0),
        contrast: f(spec, "contrast", 1.0),
        gamma: f(spec, "gamma", 1.0),
        pivot: f(spec, "pivot", 0.18),
        log_exposure_step: 0.088,
        log_mid_gray: 0.435,
        style: match spec["style"].as_str().unwrap_or("linear") {
            "linear" => ExposureContrastStyle::Linear,
            "video" => ExposureContrastStyle::Video,
            s => panic!("style {s}"),
        },
        direction: FWD,
        dynamic,
    })
}

fn matrix(cv: &Value) -> Transform {
    let m: Vec<f64> = cv["matrix"]
        .as_array()
        .expect("channel view matrix")
        .iter()
        .map(|v| v.as_f64().expect("matrix entry"))
        .collect();
    Transform::Matrix(MatrixTransform {
        name: String::new(),
        matrix: m.try_into().expect("16 entries"),
        offset: [0.0; 4],
        direction: FWD,
    })
}

fn pipeline(case: &Value) -> LegacyViewingPipeline {
    let mut lvp = LegacyViewingPipeline::new();
    let dvt = DisplayViewTransform {
        src: case["src"].as_str().unwrap().into(),
        display: case["display"].as_str().unwrap().into(),
        view: case["view"].as_str().unwrap().into(),
        looks_bypass: case["looks_bypass"].as_bool().unwrap_or(false),
        data_bypass: case["data_bypass"].as_bool().unwrap_or(true),
        direction: match case["direction"].as_str().unwrap_or("forward") {
            "forward" => FWD,
            _ => TransformDirection::Inverse,
        },
        ..DisplayViewTransform::default()
    };
    lvp.set_display_view_transform(Some(dvt));
    if !case["linear_cc"].is_null() {
        lvp.set_linear_cc(Some(ec(&case["linear_cc"])));
    }
    if !case["color_timing_cc"].is_null() {
        lvp.set_color_timing_cc(Some(ec(&case["color_timing_cc"])));
    }
    if !case["display_cc"].is_null() {
        lvp.set_display_cc(Some(ec(&case["display_cc"])));
    }
    if !case["channel_view"].is_null() {
        lvp.set_channel_view(Some(matrix(&case["channel_view"])));
    }
    if let Some(looks) = case["looks_override"].as_str() {
        lvp.set_looks_override(looks);
        lvp.set_looks_override_enabled(true);
    }
    lvp
}

fn probes(case: &Value) -> Vec<[f32; 4]> {
    case["probes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| std::array::from_fn(|c| p[c].as_f64().unwrap() as f32))
        .collect()
}

fn run(proc: &Processor, px: &[[f32; 4]]) -> Vec<[f32; 4]> {
    let mut out = px.to_vec();
    proc.apply_rgba(&mut out);
    out
}

/// Records every channel further than `tol` (relative above 1) from `want` into `failures`.
fn compare(label: &str, got: &[[f32; 4]], want: &Value, tol: f32, failures: &mut Vec<String>) {
    for (k, w) in want.as_array().unwrap().iter().enumerate() {
        let w: [f32; 4] = std::array::from_fn(|c| w[c].as_f64().unwrap() as f32);
        if (0..4).any(|c| (got[k][c] - w[c]).abs() > tol * w[c].abs().max(1.0)) {
            failures.push(format!("{label}: probe {k} -> {:?}, OCIO {w:?}", got[k]));
        }
    }
}

const TOL: f32 = 2e-4;

#[test]
fn static_cases_match_pyocio() {
    let t = table();
    let mut failures = Vec::new();
    let mut n = 0;
    for case in t["cases"].as_array().unwrap() {
        let Some(want) = case.get("out") else {
            continue;
        };
        let name = case["name"].as_str().unwrap();
        let cfg = config(case["cfg"].as_str().unwrap());
        let proc = match pipeline(case).processor(&cfg) {
            Ok(p) => p,
            Err(e) => {
                failures.push(format!("{name}: {e}"));
                continue;
            }
        };
        compare(name, &run(&proc, &probes(case)), want, TOL, &mut failures);
        n += 1;
    }
    assert!(n >= 25, "only {n} static cases ran");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// OCIO's reference for the CPU path: one processor, the dynamic exposure moved between runs.
/// Ours must give the same pixels from ONE processor, with no rebuild in between.
#[test]
fn dynamic_exposure_moves_on_one_processor_like_ocio() {
    let t = table();
    let mut failures = Vec::new();
    let mut n = 0;
    for case in t["cases"].as_array().unwrap() {
        let Some(sets) = case["set_exposure"].as_array() else {
            continue;
        };
        let name = case["name"].as_str().unwrap();
        let cfg = config(case["cfg"].as_str().unwrap());
        let mut proc = pipeline(case).processor(&cfg).unwrap();
        let ops_before = proc.num_ops();
        let px = probes(case);
        let mut first_run = None;
        for (i, ev) in sets.iter().enumerate() {
            let ev = ev.as_f64().unwrap();
            proc.set_dynamic(DynamicPropertyType::Exposure, ev).unwrap();
            let read = proc
                .dynamic(DynamicPropertyType::Exposure)
                .expect("dynamic exposure");
            assert!(
                (read - ev).abs() < 1e-6,
                "{name}: read back {read}, set {ev}"
            );
            let got = run(&proc, &px);
            compare(
                &format!("{name} ev={ev}"),
                &got,
                &case["out_by_exposure"][i],
                TOL,
                &mut failures,
            );
            first_run.get_or_insert(got.clone());
            // The dial has to bite: a processor that ignored set_dynamic would still match
            // OCIO at the value it was built with.
            if i > 0 && ev != sets[0].as_f64().unwrap() {
                assert_ne!(
                    got,
                    first_run.clone().unwrap(),
                    "{name}: ev={ev} changed nothing"
                );
            }
        }
        assert_eq!(
            proc.num_ops(),
            ops_before,
            "{name}: set_dynamic rebuilt the op list"
        );
        n += 1;
    }
    assert_eq!(n, 3);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn errors_match_pyocio_text() {
    let t = table();
    let e = &t["errors"];
    let err = |r: Result<Processor, OcioError>| r.expect_err("an error");
    let fx = fixture();
    let case = |extra: Value| {
        let mut c = serde_json::json!({"src": "lin", "display": "Disp", "view": "Standard"});
        c.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        c
    };
    let ev1 = serde_json::json!({"exposure": 1.0});
    let want = |k: &str| e[k].as_str().unwrap().to_string();

    // No display transform at all.
    let no_dvt = err(LegacyViewingPipeline::new().processor(&fx));
    assert!(matches!(no_dvt, OcioError::Pipeline(_)));
    assert_eq!(no_dvt.to_string(), want("no_dvt"));

    // The roles: removed from the fixture's text, as the reference generator does.
    let src = std::fs::read_to_string(data("viewing_pipeline.ocio")).unwrap();
    let without = |line: &str| {
        assert!(src.contains(line));
        Config::from_stream(&src.replace(line, "")).expect("config without role")
    };
    let no_sl = without("  scene_linear: lin\n");
    let no_ct = without("  color_timing: logcs\n");
    let e1 = err(pipeline(&case(serde_json::json!({"linear_cc": ev1}))).processor(&no_sl));
    assert!(
        matches!(&e1, OcioError::RoleRequired { role, what } if role == "scene_linear" && *what == "LinearCC")
    );
    assert_eq!(e1.to_string(), want("no_scene_linear"));
    let e2 = err(pipeline(&case(serde_json::json!({"color_timing_cc": ev1}))).processor(&no_ct));
    assert!(
        matches!(&e2, OcioError::RoleRequired { role, what } if role == "color_timing" && *what == "ColorTimingCC")
    );
    assert_eq!(e2.to_string(), want("no_color_timing"));
    // A no-op correction never asks for the role.
    assert!(e["noop_without_role"].is_null());
    pipeline(&case(serde_json::json!({"linear_cc": {"exposure": 0.0}})))
        .processor(&no_sl)
        .expect("a no-op linear_cc must not need the scene_linear role");
    // ... but a dynamic one is never a no-op.
    assert!(
        pipeline(&case(
            serde_json::json!({"linear_cc": {"exposure": 0.0, "dynamic": ["exposure"]}})
        ))
        .processor(&no_sl)
        .is_err()
    );

    // Input colour space.
    let bad = err(pipeline(&case(serde_json::json!({"src": "nope"}))).processor(&fx));
    assert!(matches!(&bad, OcioError::InputColorSpaceNotFound { name } if name == "nope"));
    assert_eq!(bad.to_string(), want("unknown_input"));

    // DisplayViewTransform::validate through the pipeline's validate(): PyOpenColorIO's
    // constructor raises the bare text, the pipeline wraps it (LegacyViewingPipeline.cpp:166-171).
    for (extra, key) in [
        (serde_json::json!({"src": ""}), "empty_input"),
        (serde_json::json!({"display": ""}), "empty_display"),
        (serde_json::json!({"view": ""}), "empty_view"),
    ] {
        let p = pipeline(&case(extra));
        let bad = err(p.processor(&fx));
        assert!(matches!(bad, OcioError::Pipeline(_)), "{key}");
        assert_eq!(
            bad.to_string(),
            format!("LegacyViewingPipeline is not valid: {}", want(key)),
            "{key}"
        );
        assert!(p.validate().is_err());
    }

    // An undefined override look: OCIO's RunLookTokens text, with the config's looks listed.
    let nolook =
        err(pipeline(&case(serde_json::json!({"looks_override": "nolook"}))).processor(&fx));
    assert!(matches!(&nolook, OcioError::LookNotFound { name, .. } if name == "nolook"));
    assert_eq!(nolook.to_string(), want("unknown_override_look"));
}
/// Independent of PyOpenColorIO: the chain written out by hand. Exposure of a LOG input has to
/// act on scene-linear light, not on the log codes, which is what a host multiplying the input
/// would get wrong.
#[test]
fn exposure_acts_in_scene_linear_not_on_the_input_codes() {
    let cfg = fixture();
    let px = [
        [0.25f32, 0.4, 0.6, 1.0],
        [0.1, 0.5, 0.3, 1.0],
        [0.55, 0.2, 0.35, 1.0],
    ];
    let gain = |g: f64| {
        Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: [
                g, 0.0, 0.0, 0.0, 0.0, g, 0.0, 0.0, 0.0, 0.0, g, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            offset: [0.0; 4],
            direction: FWD,
        })
    };
    let to_lin = Transform::ColorSpace(vfx_ocio::ColorSpaceTransform {
        src: "logcs".into(),
        dst: "scene_linear".into(),
        data_bypass: true,
        direction: FWD,
    });
    let dvt = |src: &str| {
        Transform::DisplayView(DisplayViewTransform {
            src: src.into(),
            display: "Disp".into(),
            view: "Standard".into(),
            ..DisplayViewTransform::default()
        })
    };
    let build = |chain: Vec<Transform>| {
        cfg.processor_from_transform(&Transform::group(chain), FWD)
            .unwrap()
    };
    // +1 EV = x2 in scene-linear.
    let right = run(&build(vec![to_lin, gain(2.0), dvt("scene_linear")]), &px);
    // The host shortcut: x2 on the log codes.
    let naive = run(&build(vec![gain(2.0), dvt("logcs")]), &px);

    let mut case = serde_json::json!({
        "src": "logcs", "display": "Disp", "view": "Standard", "linear_cc": {"exposure": 1.0}
    });
    let lvp = pipeline(&case);
    let got = run(&lvp.processor(&cfg).unwrap(), &px);
    for (g, r) in got.iter().zip(&right) {
        for c in 0..4 {
            assert!(
                (g[c] - r[c]).abs() < 2e-5,
                "pipeline {g:?} vs by-hand {r:?}"
            );
        }
    }
    // The two chains must be far apart, or this test would not tell them apart.
    assert!(
        got.iter()
            .zip(&naive)
            .any(|(g, n)| (g[0] - n[0]).abs() > 0.05),
        "scene-linear exposure and code gain should differ visibly"
    );
    case["linear_cc"]["exposure"] = serde_json::json!(0.0);
}

/// `display_cc` runs after the display transform: the same value through the display processor
/// and then the correction on its own.
#[test]
fn display_cc_runs_after_the_display_transform() {
    let cfg = fixture();
    let px = [[0.18f32, 0.18, 0.18, 1.0], [0.5, 0.2, 0.7, 0.5]];
    let cc = ec(&serde_json::json!({"style": "video", "gamma": 1.4}));
    let display = cfg.display_processor("lin", "Disp", "Standard").unwrap();
    let by_hand = run(
        &Processor::from_transform(&cc, FWD).unwrap(),
        &run(&display, &px),
    );

    let case = serde_json::json!({
        "src": "lin", "display": "Disp", "view": "Standard",
        "display_cc": {"style": "video", "gamma": 1.4}
    });
    let got = run(&pipeline(&case).processor(&cfg).unwrap(), &px);
    for (g, w) in got.iter().zip(&by_hand) {
        for c in 0..3 {
            assert!((g[c] - w[c]).abs() < 2e-5, "{g:?} vs {w:?}");
        }
    }
    assert!(
        (got[0][0] - run(&display, &px)[0][0]).abs() > 1e-3,
        "display_cc did nothing"
    );
}

/// A no-op correction costs nothing: the same ops as no correction at all.
#[test]
fn noop_corrections_add_no_ops() {
    let cfg = fixture();
    let base = serde_json::json!({"src": "logcs", "display": "Disp", "view": "Standard"});
    let mut noop = base.clone();
    noop["linear_cc"] = serde_json::json!({"exposure": 0.0});
    noop["color_timing_cc"] = serde_json::json!({"exposure": 0.0});
    let a = pipeline(&base).processor(&cfg).unwrap();
    let b = pipeline(&noop).processor(&cfg).unwrap();
    assert_eq!(a.num_ops(), b.num_ops());
    // A dynamic one keeps its op and its round trip through scene_linear.
    noop["linear_cc"] = serde_json::json!({"exposure": 0.0, "dynamic": ["exposure"]});
    let c = pipeline(&noop).processor(&cfg).unwrap();
    assert!(c.num_ops() > a.num_ops());
}

#[test]
fn setters_and_getters_keep_what_ocio_keeps() {
    let mut lvp = LegacyViewingPipeline::new();
    assert!(lvp.display_view_transform().is_none());
    assert!(!lvp.looks_override_enabled());
    assert_eq!(lvp.looks_override(), "");
    assert!(lvp.validate().is_err());

    let dvt = DisplayViewTransform {
        src: "lin".into(),
        display: "Disp".into(),
        view: "Looked".into(),
        looks_bypass: false,
        ..DisplayViewTransform::default()
    };
    lvp.set_display_view_transform(Some(dvt));
    lvp.validate().expect("valid with a display transform");
    // OCIO bypasses the transform's own looks: the pipeline applies them.
    assert!(lvp.display_view_transform().unwrap().looks_bypass);

    let cc = ec(&serde_json::json!({"exposure": 1.0}));
    lvp.set_linear_cc(Some(cc.clone()));
    lvp.set_color_timing_cc(Some(cc.clone()));
    lvp.set_display_cc(Some(cc.clone()));
    lvp.set_channel_view(Some(cc));
    assert!(lvp.linear_cc().is_some() && lvp.color_timing_cc().is_some());
    assert!(lvp.display_cc().is_some() && lvp.channel_view().is_some());
    lvp.set_linear_cc(None);
    assert!(lvp.linear_cc().is_none());

    lvp.set_looks_override("warm");
    lvp.set_looks_override_enabled(true);
    assert_eq!(
        (lvp.looks_override(), lvp.looks_override_enabled()),
        ("warm", true)
    );

    lvp.set_display_view_transform(None);
    assert!(lvp.display_view_transform().is_none());
}

#[test]
fn set_dynamic_refuses_what_it_cannot_do() {
    let cfg = fixture();
    let case = serde_json::json!({
        "src": "lin", "display": "Disp", "view": "Standard", "linear_cc": {"exposure": 1.0}
    });
    // Static exposure: no dynamic property, as OCIO.
    let mut proc = pipeline(&case).processor(&cfg).unwrap();
    let e = proc
        .set_dynamic(DynamicPropertyType::Exposure, 1.0)
        .unwrap_err();
    assert!(matches!(e, OcioError::DynamicPropertyNotFound), "{e}");
    assert_eq!(
        e.to_string(),
        "Cannot find dynamic property; not used by CPU processor."
    );
    assert_eq!(proc.dynamic(DynamicPropertyType::Exposure), None);

    let case = serde_json::json!({
        "src": "lin", "display": "Disp", "view": "Standard",
        "linear_cc": {"exposure": 0.0, "dynamic": ["exposure"]}
    });
    let mut proc = pipeline(&case).processor(&cfg).unwrap();
    assert!(
        proc.set_dynamic(DynamicPropertyType::Exposure, f64::NAN)
            .is_err()
    );
    assert!(
        proc.set_dynamic(DynamicPropertyType::GradingPrimary, 1.0)
            .is_err()
    );
    // Gamma is not dynamic on this op.
    assert!(proc.set_dynamic(DynamicPropertyType::Gamma, 1.2).is_err());
}

/// OCIO's reverse renderers are the inverse of the forward ones in every style, for the power
/// path and for the exposure-only path that keeps negatives. (OCIO's own CPU Logarithmic
/// reverse ignores `log_exposure_step`, so a non-default step cannot be compared with it; the
/// round trip is the property that holds for the GPU shader and for this crate.)
#[test]
fn inverse_exposure_contrast_inverts_every_style() {
    let probes = [
        [0.18f32, 0.5, 0.9, 1.0],
        [0.02, 0.3, 1.7, 0.5],
        [-0.1, 0.0, 0.4, 1.0],
    ];
    for style in [
        ExposureContrastStyle::Linear,
        ExposureContrastStyle::Video,
        ExposureContrastStyle::Logarithmic,
    ] {
        for (exposure, contrast, gamma) in [(0.7, 1.3, 1.1), (1.5, 1.0, 1.0), (-0.4, 0.8, 1.25)] {
            let t = Transform::ExposureContrast(ExposureContrastTransform {
                exposure,
                contrast,
                gamma,
                style,
                log_exposure_step: 0.057,
                ..ExposureContrastTransform::default()
            });
            let fwd = Processor::from_transform(&t, FWD).unwrap();
            let inv = Processor::from_transform(&t, TransformDirection::Inverse).unwrap();
            let mut px = probes.to_vec();
            fwd.apply_rgba(&mut px);
            inv.apply_rgba(&mut px);
            for (g, p) in px.iter().zip(&probes) {
                // The power path clamps negatives to 0 (as OCIO); only the affine paths keep them.
                let keeps_negatives =
                    style == ExposureContrastStyle::Logarithmic || contrast * gamma == 1.0;
                for c in 0..3 {
                    if p[c] < 0.0 && !keeps_negatives {
                        continue;
                    }
                    assert!(
                        (g[c] - p[c]).abs() < 2e-5 * p[c].abs().max(1.0),
                        "{style:?} e={exposure} c={contrast} g={gamma}: {p:?} -> {g:?}"
                    );
                }
            }
        }
    }
}

/// A DELIBERATE difference from OCIO's CPU: the Logarithmic REVERSE with a non-default
/// `log_exposure_step`. OCIO's `ECLogarithmicRevRenderer::updateData` never sets `m_logExposureStep`,
/// so its CPU keeps the 0.088 default (`ExposureContrastOpCPU.cpp:651-657`) while its GPU shader
/// uses the op's step (`ExposureContrastOpGPU.cpp`, `AddECLogarithmicRevShader`:
/// `exposure = exposureVal * step`). This crate follows the GPU, the true inverse of the forward.
/// Both numbers are pinned (PyOpenColorIO 2.5.1 and the GPU formula) so the decision cannot
/// drift silently: exposure 1, contrast 1.3, gamma 1.1, step 0.2, grey inputs 0.3 / 0.5 / 0.7.
///   GPU form  out = in / c + lp - lp / c - E * step      (c = 1.43, lp = 0.435)
///   OCIO CPU  the same with step = 0.088 (pyocio: 0.25259441, 0.39245453, 0.53231466)
#[test]
fn log_reverse_follows_the_ocio_gpu_not_its_cpu_step_quirk() {
    let t = Transform::ExposureContrast(ExposureContrastTransform {
        exposure: 1.0,
        contrast: 1.3,
        gamma: 1.1,
        style: ExposureContrastStyle::Logarithmic,
        log_exposure_step: 0.2,
        ..ExposureContrastTransform::default()
    });
    let inv = Processor::from_transform(&t, TransformDirection::Inverse).unwrap();
    let got = run(&inv, &[[0.3; 4], [0.5; 4], [0.7; 4]]);
    let gpu_form = [0.140_594_41_f32, 0.280_454_55, 0.420_314_7];
    let ocio_cpu = [0.252_594_4_f32, 0.392_454_5, 0.532_314_7];
    for (i, g) in got.iter().enumerate() {
        assert!(
            (g[0] - gpu_form[i]).abs() < 2e-6,
            "{i}: {} vs GPU form {}",
            g[0],
            gpu_form[i]
        );
        assert!(
            (g[0] - ocio_cpu[i]).abs() > 0.1,
            "{i}: must not be OCIO's CPU value"
        );
    }
}
/// `Processor: Clone`, so a host keeps one processor for the GPU and clones it to move a dynamic
/// property on a CPU twin; the original is untouched.
#[test]
fn cloned_processor_moves_independently() {
    let cfg = fixture();
    let case = serde_json::json!({
        "src": "lin", "display": "Disp", "view": "Standard",
        "linear_cc": {"exposure": 0.0, "dynamic": ["exposure"]}
    });
    let px = [[0.18f32, 0.3, 0.5, 1.0]];
    let orig = pipeline(&case).processor(&cfg).unwrap();
    let mut twin = orig.clone();
    twin.set_dynamic(DynamicPropertyType::Exposure, 2.0)
        .unwrap();
    assert_eq!(orig.dynamic(DynamicPropertyType::Exposure), Some(0.0));
    assert_eq!(twin.dynamic(DynamicPropertyType::Exposure), Some(2.0));
    assert_ne!(run(&orig, &px), run(&twin, &px));
    assert_eq!(
        run(&orig, &px),
        run(&pipeline(&case).processor(&cfg).unwrap(), &px)
    );
}

/// OCIO `ExposureContrastOpData::getCacheID` leaves a dynamic value out: moving the dial is not a
/// different processor (a host keyed on the id must not rebuild), but a static value, a flag or
/// a direction is.
#[test]
fn cache_id_ignores_the_value_of_a_dynamic_parameter() {
    let cfg = fixture();
    let build = |cc: Value| {
        pipeline(&serde_json::json!({
            "src": "logcs", "display": "Disp", "view": "Standard", "linear_cc": cc
        }))
        .processor(&cfg)
        .unwrap()
    };
    let dynamic = serde_json::json!({"exposure": 0.0, "dynamic": ["exposure"]});
    let mut p = build(dynamic.clone());
    let id = p.cache_id();
    p.set_dynamic(DynamicPropertyType::Exposure, 1.7).unwrap();
    assert_eq!(p.cache_id(), id, "a dynamic value is not part of the id");
    assert_eq!(
        build(serde_json::json!({"exposure": 3.0, "dynamic": ["exposure"]})).cache_id(),
        id
    );

    // The direction is part of the id: the same parameters forward and inverse are two processors.
    let ec_dir = |dir| {
        let t = Transform::ExposureContrast(ExposureContrastTransform {
            exposure: 1.0,
            ..ExposureContrastTransform::default()
        });
        Processor::from_transform(&t, dir).unwrap().cache_id()
    };
    assert_ne!(ec_dir(FWD), ec_dir(TransformDirection::Inverse));

    // What does change it: a static value, which parameter is dynamic, the other values.
    assert_ne!(build(serde_json::json!({"exposure": 1.0})).cache_id(), id);
    assert_ne!(
        build(serde_json::json!({"exposure": 2.0})).cache_id(),
        build(serde_json::json!({"exposure": 1.0})).cache_id()
    );
    assert_ne!(
        build(serde_json::json!({"exposure": 0.0, "dynamic": ["gamma"]})).cache_id(),
        id
    );
    assert_ne!(
        build(serde_json::json!({"exposure": 0.0, "contrast": 1.2, "dynamic": ["exposure"]}))
            .cache_id(),
        id
    );
}

/// A missing exposure/contrast/gamma key is dynamic on load and a dynamic value is not written
/// (`OCIOYaml.cpp:1273-1285`, `:1298`); the flags and the serialized keys equal PyOpenColorIO's,
/// and the text reads back to the same flags.
#[test]
fn yaml_missing_keys_are_dynamic_and_round_trip() {
    let t = table();
    let cfg = fixture();
    let flags = |cfg: &Config, name: &str| {
        let cs = cfg.colorspace(name).unwrap();
        match cs.from_reference().unwrap() {
            Transform::ExposureContrast(e) => e.dynamic,
            other => panic!("{other:?}"),
        }
    };
    let text = cfg.serialize().expect("serialize");
    let back = Config::from_stream(&text).expect("serialized text reads back");
    for name in ["ecdyn", "ecstatic"] {
        let want = &t["ec_yaml"][name];
        let w = &want["flags"];
        let expect = EcDynamic {
            exposure: w["exposure"].as_bool().unwrap(),
            contrast: w["contrast"].as_bool().unwrap(),
            gamma: w["gamma"].as_bool().unwrap(),
        };
        assert_eq!(flags(&cfg, name), expect, "{name}: flags as PyOpenColorIO");
        assert_eq!(flags(&back, name), expect, "{name}: round trip");

        // Which keys are written: exactly PyOpenColorIO's (dynamic ones are left out, an
        // explicit default is kept).
        let line = text
            .lines()
            .find(|l| {
                l.contains("ExposureContrastTransform")
                    && l.contains(if name == "ecdyn" { "video" } else { "linear" })
            })
            .unwrap_or_else(|| panic!("no EC line for {name} in:\n{text}"));
        let key_set =
            |s: &str| ["exposure", "contrast", "gamma"].map(|k| s.contains(&format!("{k}:")));
        assert_eq!(
            key_set(line),
            key_set(want["serialized"].as_str().unwrap()),
            "{name}: {line}"
        );
    }
    // The dynamic flags reach the processor.
    let p = cfg.processor("lin", "ecdyn").unwrap();
    assert!(!p.has_dynamic_property(DynamicPropertyType::Exposure));
    assert!(p.has_dynamic_property(DynamicPropertyType::Contrast));
    assert!(p.has_dynamic_property(DynamicPropertyType::Gamma));
    assert!(!cfg.processor("lin", "ecstatic").unwrap().is_dynamic());
}

/// `|` fall-backs and the result space of a look string, from OCIO's `LookTransform.cpp:149-185`
/// and `BuildLookOps`: the first option whose files exist is used, the result space is the last
/// look's process space. (PyOpenColorIO does not expose `GetLooksResultColorSpace`; the pipeline
/// cases `override_fallback`, `override_cool_*` pin the same logic through pixels.)
#[test]
fn looks_result_space_follows_options_and_files() {
    let cfg = fixture();
    let ctx = cfg.context();
    let space = |s: &str| {
        cfg.looks_result_space(s, ctx)
            .map(|r| r.map(str::to_string))
    };
    assert_eq!(space("").unwrap(), None);
    assert_eq!(space("warm").unwrap().as_deref(), Some("logcs"));
    assert_eq!(space("cool").unwrap().as_deref(), Some("lin"));
    assert_eq!(space("warm, cool").unwrap().as_deref(), Some("lin"));
    assert_eq!(space("cool, warm").unwrap().as_deref(), Some("logcs"));
    assert_eq!(space("-warm").unwrap().as_deref(), Some("logcs"));
    // `gone` needs a LUT file that is not there: the next option is used.
    assert_eq!(space("gone | cool").unwrap().as_deref(), Some("lin"));
    assert_eq!(space("cool | gone").unwrap().as_deref(), Some("lin"));
    // Every option lacks a file: one error that lists them all, as OCIO.
    match space("gone | -gone") {
        Err(OcioError::MissingFile(m)) => {
            assert!(m.starts_with("(gone) The specified file reference"), "{m}");
            assert!(m.contains("  ...  (-gone) "), "{m}");
        }
        other => panic!("{other:?}"),
    }
    // A missing look is an error at once, not a fall-back (only a missing FILE moves on).
    assert!(matches!(
        space("nolook | cool"),
        Err(OcioError::LookNotFound { .. })
    ));
}

/// The context passed to `processor_from_transform_with_context` resolves the `$VAR`s of file
/// references (and look files); it used to be ignored in favour of the config's own.
#[test]
fn passed_context_resolves_file_transform_vars() {
    let cfg = fixture();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vfx-lut/tests/data/ocio");
    assert!(dir.join("cpf.spi1d").exists());
    let t = Transform::file("$LUTDIR/cpf.spi1d");
    let ctx_with = |d: &std::path::Path| {
        let mut c = cfg.context().clone();
        c.set("LUTDIR", d.to_string_lossy());
        c
    };
    cfg.processor_from_transform_with_context(&ctx_with(&dir), &t, FWD)
        .expect("the passed context supplies LUTDIR");
    assert!(
        cfg.processor_from_transform_with_context(&ctx_with(&dir.join("nowhere")), &t, FWD)
            .is_err()
    );
    // And without the variable anywhere there is nothing to resolve.
    assert!(cfg.processor_from_transform(&t, FWD).is_err());
}

/// `find_view` is OCIO's `getView`: case-insensitive, and not narrowed by `active_views`.
#[test]
fn find_view_ignores_active_views_and_case() {
    let cfg = config("fixture_active");
    assert!(cfg.find_view("Disp", "Looked").is_some());
    assert!(cfg.find_view("disp", "LOOKED").is_some());
    assert!(cfg.find_view("Disp", "").is_none());
    assert!(cfg.find_view("Nope", "Looked").is_none());
    assert!(cfg.find_view("Disp", "Nope").is_none());
    // The listing, unlike the lookup, honours the active list.
    assert_eq!(cfg.get_views("Disp").len(), 1);
}
/// The viewer path: the same processor compiles to WGSL with the exposure as a scalar uniform, so
/// the dial moves with `set_uniform_value` instead of a new shader.
#[test]
fn dynamic_exposure_is_a_wgsl_uniform() {
    let cfg = builtin::default_config();
    let case = serde_json::json!({
        "src": "ACEScct", "display": "sRGB - Display", "view": "Video (colorimetric)",
        "linear_cc": {"exposure": 0.0, "dynamic": ["exposure"]},
        "display_cc": {"style": "video", "gamma": 1.1}
    });
    let proc = pipeline(&case).processor(&cfg).unwrap();
    let gpu = GpuProcessor::from_processor_wgsl(&proc).expect("the whole pipeline emits WGSL");
    let code = gpu.generate_shader(GpuLanguage::Wgsl).unwrap();
    // The pipeline has two exposure-contrast ops (linear_cc, display_cc), hence two uniforms
    // called `*_exposure`; only the linear_cc one was declared dynamic.
    assert_eq!(
        code.uniforms()
            .iter()
            .filter(|u| u.name.ends_with("_exposure"))
            .count(),
        2
    );
    let names = gpu.dynamic_uniforms(DynamicPropertyType::Exposure);
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(gpu.dynamic_uniforms(DynamicPropertyType::Gamma).is_empty());
    assert!(
        gpu.dynamic_uniforms(DynamicPropertyType::Contrast)
            .is_empty()
    );
    let ev = code.uniform(&names[0]).expect("registered uniform");
    assert_eq!(ev.uniform_type, GpuUniformType::Float);
    assert_eq!(ev.default_value, GpuUniformValue::Float(0.0));
    let prelude = gpu.wgsl_prelude();
    // The shader reads a scalar uniform through its `<NAME>_OFFSET` slot constant.
    let slot = format!("scalar_f32({}_OFFSET)", names[0].to_uppercase());
    assert!(
        prelude.contains(&slot),
        "the shader must read the uniform via {slot}"
    );

    // Rebuilt from a processor that has been moved, the default follows (and the shader text
    // is unchanged: only a value differs).
    let mut moved = pipeline(&case).processor(&cfg).unwrap();
    moved
        .set_dynamic(DynamicPropertyType::Exposure, 1.5)
        .unwrap();
    let code2 = GpuProcessor::from_processor_wgsl(&moved)
        .unwrap()
        .generate_shader(GpuLanguage::Wgsl)
        .unwrap();
    assert_eq!(code.fragment_code(), code2.fragment_code());
    let ev2 = code2.uniform(&names[0]).expect("same uniform name");
    assert_eq!(ev2.default_value, GpuUniformValue::Float(1.5));
}

#[cfg(feature = "gpu")]
mod gpu {
    use super::*;
    use gpu_mem::wgpu;
    use vfx_ocio::{
        GpuBufferExecutor, GpuOcioResources, OCIO_UNIFORM_GROUP, WGSL_TRANSFORM_FUNCTION,
    };

    fn have_gpu(test: &str) -> bool {
        let present = gpu_mem::shared_device().is_some();
        if !present {
            vfx_core::test_support::gpu_unavailable(test, "gpu_mem::shared_device() is None");
        }
        present
    }

    fn close(g: &[[f32; 4]], c: &[[f32; 4]], what: &str) {
        for (i, (g, c)) in g.iter().zip(c).enumerate() {
            for k in 0..3 {
                assert!(
                    (g[k] - c[k]).abs() <= 2e-4 * c[k].abs().max(1.0),
                    "{what} pixel {i} chan {k}: gpu={} cpu={}",
                    g[k],
                    c[k]
                );
            }
        }
    }

    /// The one dynamic exposure uniform of `proc`'s WGSL.
    fn exposure_uniform(proc: &Processor) -> String {
        let mut names = GpuProcessor::from_processor_wgsl(proc)
            .unwrap()
            .dynamic_uniforms(DynamicPropertyType::Exposure);
        assert_eq!(names.len(), 1, "one dynamic exposure uniform: {names:?}");
        names.remove(0)
    }

    /// The runtime end to end: the viewer pipeline on the GPU equals the CPU at the built value,
    /// and moving ONLY the exposure uniform (no recompile) equals the CPU after `set_dynamic`.
    #[test]
    fn dynamic_exposure_uniform_moves_the_gpu_like_the_cpu() {
        if !have_gpu("dynamic_exposure_uniform_moves_the_gpu_like_the_cpu") {
            return;
        }
        let cfg = builtin::default_config();
        let case = serde_json::json!({
            "src": "ACEScct", "display": "sRGB - Display", "view": "Video (colorimetric)",
            "linear_cc": {"exposure": 0.0, "dynamic": ["exposure"]},
            "display_cc": {"style": "video", "gamma": 1.1}
        });
        let mut proc = pipeline(&case).processor(&cfg).unwrap();
        let name = exposure_uniform(&proc);

        // A ramp over the ACEScct range from below the toe (negative scene-linear, which an
        // exposure-only grade must keep on the GPU as on the CPU) up past mid grey.
        let px: Vec<[f32; 4]> = (0..16)
            .map(|i| {
                let v = 0.02 + 0.05 * i as f32;
                [v, v * 0.8 + 0.05, v * 0.6 + 0.1, 1.0]
            })
            .collect();
        let mut exec = GpuBufferExecutor::compile(&proc).expect("compile executor");
        for ev in [0.0f64, 1.0, -0.75, 2.0] {
            proc.set_dynamic(DynamicPropertyType::Exposure, ev).unwrap();
            assert!(
                exec.set_uniform_value(&name, GpuUniformValue::Float(ev as f32))
                    .unwrap(),
                "uniform {name} not found"
            );
            let gpu = exec.execute(px.len() as u32, 1, &px).expect("execute");
            close(&gpu, &run(&proc, &px), &format!("ev={ev}"));
        }
    }

    /// An INVERSE op: the uniform still holds the forward exposure (the shader inverts it), so
    /// moving only the uniform equals the CPU after `set_dynamic`, on the power path
    /// (contrast and gamma set) and with an inverse display direction around it.
    #[test]
    fn inverse_dynamic_exposure_uniform_moves_the_gpu_like_the_cpu() {
        if !have_gpu("inverse_dynamic_exposure_uniform_moves_the_gpu_like_the_cpu") {
            return;
        }
        let cfg = fixture();
        let case = serde_json::json!({
            "src": "lin", "display": "Disp", "view": "Standard", "direction": "inverse",
            "linear_cc": {"exposure": 0.0, "contrast": 1.3, "gamma": 1.1, "dynamic": ["exposure"]}
        });
        let mut proc = pipeline(&case).processor(&cfg).unwrap();
        let name = exposure_uniform(&proc);
        let px: Vec<[f32; 4]> = (1..=12)
            .map(|i| {
                let v = i as f32 / 13.0;
                [v, v * 0.7 + 0.1, 1.0 - v * 0.5, 1.0]
            })
            .collect();
        let mut exec = GpuBufferExecutor::compile(&proc).expect("compile executor");
        for ev in [0.0f64, 0.8, -0.6, 1.5] {
            proc.set_dynamic(DynamicPropertyType::Exposure, ev).unwrap();
            assert!(
                exec.set_uniform_value(&name, GpuUniformValue::Float(ev as f32))
                    .unwrap()
            );
            let gpu = exec.execute(px.len() as u32, 1, &px).expect("execute");
            close(&gpu, &run(&proc, &px), &format!("inverse ev={ev}"));
        }
    }

    /// Every style and direction on the GPU equals the CPU, including the Logarithmic reverse with
    /// a non-default step and the negative-keeping exposure-only path.
    #[test]
    fn exposure_contrast_every_style_and_direction_gpu_equals_cpu() {
        if !have_gpu("exposure_contrast_every_style_and_direction_gpu_equals_cpu") {
            return;
        }
        let px: Vec<[f32; 4]> = vec![
            [0.18, 0.5, 0.9, 1.0],
            [0.02, 0.3, 1.7, 0.5],
            [-0.1, 0.0, 0.4, 1.0],
            [0.7, -0.05, 0.25, 1.0],
        ];
        for style in [
            ExposureContrastStyle::Linear,
            ExposureContrastStyle::Video,
            ExposureContrastStyle::Logarithmic,
        ] {
            for (contrast, gamma) in [(1.3, 1.1), (1.0, 1.0)] {
                for direction in [FWD, TransformDirection::Inverse] {
                    let t = Transform::ExposureContrast(ExposureContrastTransform {
                        exposure: 0.7,
                        contrast,
                        gamma,
                        style,
                        log_exposure_step: 0.057,
                        ..ExposureContrastTransform::default()
                    });
                    let proc = Processor::from_transform(&t, direction).unwrap();
                    let gpu =
                        vfx_ocio::apply_gpu(&proc, px.len() as u32, 1, &px).expect("apply_gpu");
                    close(
                        &gpu,
                        &run(&proc, &px),
                        &format!("{style:?} c={contrast} g={gamma} {direction:?}"),
                    );
                }
            }
        }
    }

    const HOST_SHELL: &str = "
@group(0) @binding(0) var<storage, read> host_src: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> host_dst: array<vec4<f32>>;

@compute @workgroup_size(64)
fn host_apply(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= arrayLength(&host_src)) { return; }
    host_dst[gid.x] = WGSL_FN(host_src[gid.x]);
}
";

    /// The path a host with its OWN pipeline takes (exr-view): keep one `GpuShaderCode`, clone it,
    /// `set_uniform_value` on the clone and `GpuOcioResources::update_scalars` it, with the CPU
    /// twin being a `Processor` clone moved by `set_dynamic`. Forward and inverse pipelines.
    #[test]
    fn host_updates_the_dial_with_update_scalars() {
        if !have_gpu("host_updates_the_dial_with_update_scalars") {
            return;
        }
        let gpu = gpu_mem::shared_device().expect("device");
        let (device, queue) = (&gpu.device, &gpu.queue);
        let cfg = fixture();
        for direction in ["forward", "inverse"] {
            let case = serde_json::json!({
                "src": "logcs", "display": "Disp", "view": "Standard", "direction": direction,
                "linear_cc": {"exposure": 0.0, "contrast": 1.2, "dynamic": ["exposure"]},
                "display_cc": {"style": "video", "gamma": 1.2}
            });
            let proc = pipeline(&case).processor(&cfg).unwrap();
            let gp = GpuProcessor::from_processor_wgsl(&proc).unwrap();
            let template = gp.generate_shader(GpuLanguage::Wgsl).unwrap();
            let names = gp.dynamic_uniforms(DynamicPropertyType::Exposure);
            let [name] = names.as_slice() else {
                panic!("{names:?}")
            };

            let src_text = format!(
                "{}{}",
                gp.wgsl_prelude(),
                HOST_SHELL.replace("WGSL_FN", WGSL_TRANSFORM_FUNCTION)
            );
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("host shell"),
                source: wgpu::ShaderSource::Wgsl(src_text.into()),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("host pipeline"),
                layout: None,
                module: &module,
                entry_point: Some("host_apply"),
                compilation_options: Default::default(),
                cache: None,
            });
            let resources = GpuOcioResources::for_pipeline(device, queue, &template, &pipeline);

            let px: Vec<[f32; 4]> = (1..=20)
                .map(|i| {
                    let v = i as f32 / 21.0;
                    [v, v * 0.6 + 0.2, 1.0 - v * 0.8, 1.0]
                })
                .collect();
            let bytes = (px.len() * 16) as u64;
            let make = |usage| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: bytes,
                    usage,
                    mapped_at_creation: false,
                })
            };
            let src = make(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
            let dst = make(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
            let readback = make(wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
            queue.write_buffer(&src, 0, bytemuck::cast_slice(&px));
            let bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: src.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: dst.as_entire_binding(),
                    },
                ],
            });
            let dispatch = || -> Vec<[f32; 4]> {
                let mut enc = device.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &bg0, &[]);
                    if let Some(bg) = resources.bind_group(OCIO_UNIFORM_GROUP) {
                        pass.set_bind_group(OCIO_UNIFORM_GROUP, bg, &[]);
                    }
                    pass.dispatch_workgroups((px.len() as u32).div_ceil(64), 1, 1);
                }
                enc.copy_buffer_to_buffer(&dst, 0, &readback, 0, bytes);
                queue.submit(std::iter::once(enc.finish()));
                let slice = readback.slice(..);
                let (tx, rx) = std::sync::mpsc::channel();
                slice.map_async(wgpu::MapMode::Read, move |r| {
                    tx.send(r).expect("send map result");
                });
                device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .expect("poll");
                rx.recv().expect("map callback").expect("map");
                let out = bytemuck::cast_slice::<u8, [f32; 4]>(
                    &slice.get_mapped_range().expect("mapped range"),
                )
                .to_vec();
                readback.unmap();
                out
            };

            for ev in [0.0f64, 1.0, -0.8, 1.9] {
                // The template is never edited: each frame clones it, sets the value, uploads.
                let mut code = template.clone();
                assert!(code.set_uniform_value(name, GpuUniformValue::Float(ev as f32)));
                assert!(resources.update_scalars(queue, &code));
                let mut twin = proc.clone();
                twin.set_dynamic(DynamicPropertyType::Exposure, ev).unwrap();
                close(
                    &dispatch(),
                    &run(&twin, &px),
                    &format!("{direction} ev={ev}"),
                );
            }
            assert_eq!(
                proc.dynamic(DynamicPropertyType::Exposure),
                Some(0.0),
                "the processor the host kept was never moved"
            );
        }
    }
}
